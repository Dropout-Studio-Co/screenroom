use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use notify::event::{CreateKind, ModifyKind, RemoveKind};
use notify::{Event, EventKind, RecursiveMode, Watcher as _};

use screenroom_core::watch::{self, MAX_PLAN_ENTRIES, MediaChange};

pub const QUIET_WINDOW: Duration = Duration::from_millis(1_200);

pub const RECONCILE_INTERVAL: Duration = Duration::from_secs(600);

pub const SLEEP_GAP: Duration = Duration::from_secs(30);

pub const ARRIVAL_RETRY: Duration = Duration::from_secs(2);

pub const FAILED_WATCH_BACKOFF: Duration = Duration::from_secs(60);

const IDLE_POLL: Duration = Duration::from_millis(500);

const FLUSH_BOUND: usize = MAX_PLAN_ENTRIES * 4;

const ROOT_CHANGED_INFO: &str = "root changed";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Interpretation {
    pub changes: Vec<MediaChange>,
    pub rescan: Option<String>,
}

impl Interpretation {
    fn quiet() -> Self {
        Self::default()
    }

    fn rescan(reason: &str) -> Self {
        Self {
            changes: Vec::new(),
            rescan: Some(reason.to_string()),
        }
    }

    fn changes(changes: Vec<MediaChange>) -> Self {
        Self {
            changes,
            rescan: None,
        }
    }
}

pub fn interpret(event: &Event) -> Interpretation {
    if event.info() == Some(ROOT_CHANGED_INFO) {
        return Interpretation::rescan("the watched folder was moved or replaced");
    }
    match &event.kind {
        EventKind::Access(_) => Interpretation::quiet(),
        EventKind::Other | EventKind::Any => {
            Interpretation::rescan("the system reported changes it could not list")
        }
        EventKind::Create(CreateKind::Folder) => Interpretation::quiet(),
        EventKind::Create(_) => Interpretation::changes(added(&event.paths)),
        EventKind::Remove(RemoveKind::Folder) => {
            Interpretation::rescan("a folder inside the watched library was removed")
        }
        EventKind::Remove(_) => Interpretation::changes(removed(&event.paths)),
        EventKind::Modify(ModifyKind::Name(_)) => renamed(&event.paths),
        EventKind::Modify(_) => Interpretation::changes(modified(&event.paths)),
    }
}

fn renamed(paths: &[PathBuf]) -> Interpretation {
    match paths {
        [from, to, ..] => {
            Interpretation::changes(vec![MediaChange::Renamed(from.clone(), to.clone())])
        }
        [only] => Interpretation::changes(added(std::slice::from_ref(only))),
        [] => Interpretation::quiet(),
    }
}

fn accepted(paths: &[PathBuf]) -> impl Iterator<Item = &PathBuf> {
    paths.iter().filter(|path| watch::accept(path))
}

fn added(paths: &[PathBuf]) -> Vec<MediaChange> {
    accepted(paths)
        .map(|path| MediaChange::Added(path.clone()))
        .collect()
}

fn modified(paths: &[PathBuf]) -> Vec<MediaChange> {
    accepted(paths)
        .map(|path| MediaChange::Modified(path.clone()))
        .collect()
}

fn removed(paths: &[PathBuf]) -> Vec<MediaChange> {
    accepted(paths)
        .map(|path| MediaChange::Removed(path.clone()))
        .collect()
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Work {
    pub ingest: Vec<PathBuf>,

    pub withdraw: bool,
}

impl Work {
    pub fn is_empty(&self) -> bool {
        self.ingest.is_empty() && !self.withdraw
    }
}

pub fn work_for(root: &Path, changes: &[MediaChange]) -> Work {
    let plan = watch::plan(root, changes);
    let capped = plan.ingest.len() >= MAX_PLAN_ENTRIES
        || plan.forget.len() >= MAX_PLAN_ENTRIES
        || plan.relocations.len() >= MAX_PLAN_ENTRIES;
    if capped {
        return Work {
            ingest: Vec::new(),
            withdraw: true,
        };
    }
    Work {
        ingest: plan.ingest,
        withdraw: !plan.forget.is_empty() || !plan.relocations.is_empty(),
    }
}

pub fn slept(wall_elapsed: Duration, monotonic_elapsed: Duration) -> bool {
    wall_elapsed.saturating_sub(monotonic_elapsed) > SLEEP_GAP
}

pub fn clock() -> Duration {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Watched {
    Changes {
        root: PathBuf,
        changes: Vec<MediaChange>,
    },

    Rescan {
        root: PathBuf,
        reason: String,
    },

    Stopped {
        root: PathBuf,
        reason: String,
    },
}

impl Watched {
    pub fn root(&self) -> &Path {
        match self {
            Self::Changes { root, .. } | Self::Rescan { root, .. } | Self::Stopped { root, .. } => {
                root
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Job {
    Scan {
        source: String,
        quiet: bool,
    },

    Ingest {
        source: String,
        root: PathBuf,
        paths: Vec<PathBuf>,
    },
}

impl Job {
    pub fn source(&self) -> &str {
        match self {
            Self::Scan { source, .. } | Self::Ingest { source, .. } => source,
        }
    }

    pub fn paths(&self) -> &[PathBuf] {
        match self {
            Self::Scan { .. } => &[],
            Self::Ingest { paths, .. } => paths,
        }
    }

    pub fn is_scan(&self) -> bool {
        matches!(self, Self::Scan { .. })
    }

    pub fn is_quiet(&self) -> bool {
        match self {
            Self::Scan { quiet, .. } => *quiet,
            Self::Ingest { .. } => false,
        }
    }
}

#[derive(Debug, Default)]
pub struct Queue {
    running: Option<Job>,
    scans: BTreeMap<String, bool>,
    ingest: BTreeMap<String, (PathBuf, Vec<PathBuf>)>,
}

impl Queue {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn running(&self) -> Option<&Job> {
        self.running.as_ref()
    }

    pub fn queued(&self) -> usize {
        self.scans.len() + self.ingest.len()
    }

    pub fn submit(&mut self, job: Job) -> Option<Job> {
        if self.running.is_none() {
            self.running = Some(job.clone());
            return Some(job);
        }
        self.hold(job);
        None
    }

    fn hold(&mut self, job: Job) {
        match job {
            Job::Scan { source, quiet } => {
                self.ingest.remove(&source);
                self.scans
                    .entry(source)
                    .and_modify(|existing| *existing = *existing && quiet)
                    .or_insert(quiet);
            }
            Job::Ingest {
                source,
                root,
                paths,
            } => {
                if self.scans.contains_key(&source) {
                    return;
                }
                let entry = self
                    .ingest
                    .entry(source)
                    .or_insert_with(|| (root, Vec::new()));
                for path in paths {
                    if !entry.1.contains(&path) {
                        entry.1.push(path);
                    }
                }
            }
        }
    }

    pub fn finish(&mut self) -> Option<Job> {
        self.running = None;
        let next = self.next();
        self.running = next.clone();
        next
    }

    fn next(&mut self) -> Option<Job> {
        if let Some((source, quiet)) = self.scans.pop_first() {
            self.ingest.remove(&source);
            return Some(Job::Scan { source, quiet });
        }
        let source = self.ingest.keys().next().cloned()?;
        let (root, paths) = self.ingest.remove(&source)?;
        Some(Job::Ingest {
            source,
            root,
            paths,
        })
    }

    pub fn pending_ingest(&self, source: &str) -> Option<&[PathBuf]> {
        self.ingest.get(source).map(|(_, paths)| paths.as_slice())
    }

    pub fn is_scan_queued(&self, source: &str) -> bool {
        self.scans.contains_key(source)
    }

    pub fn cancel_pending(&mut self) {
        self.scans.clear();
        self.ingest.clear();
    }

    pub fn drop_source(&mut self, source: &str) {
        self.scans.remove(source);
        self.ingest.remove(source);
    }
}

pub struct RootWatch {
    root: PathBuf,
    stop: Arc<AtomicBool>,
    events: Receiver<Watched>,
}

impl RootWatch {
    pub fn spawn(root: &Path) -> Self {
        let (events, receiver) = channel();
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let worker_root = root.to_path_buf();
        let worker_events = events.clone();
        let started = std::thread::Builder::new()
            .name(format!("screenroom-watch-{}", root.display()))
            .spawn(move || run(worker_root, worker_stop, worker_events));
        if started.is_err() {
            let _ = events.send(Watched::Stopped {
                root: root.to_path_buf(),
                reason: "the file-system watcher thread could not start".to_string(),
            });
        }
        Self {
            root: root.to_path_buf(),
            stop,
            events: receiver,
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn poll(&self) -> Vec<Watched> {
        self.events.try_iter().collect()
    }
}

impl Drop for RootWatch {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

impl std::fmt::Debug for RootWatch {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RootWatch")
            .field("root", &self.root)
            .finish()
    }
}

fn run(root: PathBuf, stop: Arc<AtomicBool>, events: Sender<Watched>) {
    let (raw, inbox) = channel();
    let mut watcher = match notify::recommended_watcher(raw) {
        Ok(watcher) => watcher,
        Err(error) => {
            return stop_with(
                events,
                root,
                format!("the file-system watcher could not be created: {error}"),
            );
        }
    };
    if let Err(error) = watcher.watch(&root, RecursiveMode::Recursive) {
        let reason = format!("{} cannot be watched right now: {error}", root.display());
        return stop_with(events, root, reason);
    }

    let mut pending: Vec<MediaChange> = Vec::new();
    let mut rescan: Option<String> = None;
    let mut hidden: HashSet<PathBuf> = HashSet::new();
    loop {
        if stop.load(Ordering::SeqCst) {
            return;
        }
        let idle = pending.is_empty() && rescan.is_none();
        let timeout = if idle { IDLE_POLL } else { QUIET_WINDOW };
        match inbox.recv_timeout(timeout) {
            Ok(Ok(event)) => {
                let reading = interpret(&event);
                if rescan.is_none()
                    && let Some(reason) = reading.rescan
                {
                    rescan = Some(reason);
                }
                pending.extend(reading.changes);
                if pending.len() >= FLUSH_BOUND
                    && !flush(&root, &mut pending, &mut rescan, &mut hidden, &events)
                {
                    return;
                }
            }
            Ok(Err(error)) => {
                return stop_with(
                    events,
                    root,
                    format!("the file-system watcher reported an error: {error}"),
                );
            }
            Err(RecvTimeoutError::Disconnected) => {
                return stop_with(
                    events,
                    root,
                    "the file-system watcher stopped reporting changes".to_string(),
                );
            }
            Err(RecvTimeoutError::Timeout) => {
                if idle {
                    hidden.clear();
                    if !root.is_dir() {
                        return stop_with(
                            events,
                            root,
                            "the watched folder is no longer there".to_string(),
                        );
                    }
                    continue;
                }
                if !flush(&root, &mut pending, &mut rescan, &mut hidden, &events) {
                    return;
                }
            }
        }
    }
}

fn flush(
    root: &Path,
    pending: &mut Vec<MediaChange>,
    rescan: &mut Option<String>,
    hidden: &mut HashSet<PathBuf>,
    events: &Sender<Watched>,
) -> bool {
    let changes = watch::settle(pending.as_slice(), hidden, QUIET_WINDOW, Instant::now());
    pending.clear();
    hidden.clear();
    if let Some(reason) = rescan.take() {
        return events
            .send(Watched::Rescan {
                root: root.to_path_buf(),
                reason,
            })
            .is_ok();
    }
    if changes.is_empty() {
        return true;
    }
    events
        .send(Watched::Changes {
            root: root.to_path_buf(),
            changes,
        })
        .is_ok()
}

fn stop_with(events: Sender<Watched>, root: PathBuf, reason: String) {
    let _ = events.send(Watched::Stopped { root, reason });
}

#[derive(Debug, Default)]
pub struct WatchSet {
    roots: Vec<RootWatch>,
    retry: BTreeMap<PathBuf, Instant>,
}

impl WatchSet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.roots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.roots.is_empty()
    }

    pub fn is_watching(&self, root: &Path) -> bool {
        self.roots.iter().any(|watch| watch.root() == root)
    }

    pub fn watching(&self) -> Vec<PathBuf> {
        self.roots
            .iter()
            .map(|watch| watch.root().to_path_buf())
            .collect()
    }

    pub fn sync(&mut self, roots: &[PathBuf]) -> Vec<PathBuf> {
        self.roots.retain(|watch| roots.contains(&watch.root));
        let mut armed = Vec::new();
        for root in roots {
            if self.is_watching(root) {
                continue;
            }
            if self.retry.get(root).is_some_and(|at| Instant::now() < *at) {
                continue;
            }
            self.retry.remove(root);
            self.roots.push(RootWatch::spawn(root));
            armed.push(root.clone());
        }
        armed
    }

    pub fn clear_retries(&mut self) {
        self.retry.clear();
    }

    pub fn poll(&mut self) -> Vec<Watched> {
        let mut events = Vec::new();
        let mut stopped = Vec::new();
        for watch in &self.roots {
            for event in watch.poll() {
                if matches!(event, Watched::Stopped { .. }) {
                    stopped.push(watch.root().to_path_buf());
                }
                events.push(event);
            }
        }
        for root in stopped {
            self.roots.retain(|watch| watch.root() != root);
            self.retry
                .insert(root, Instant::now() + FAILED_WATCH_BACKOFF);
        }
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::{AccessKind, RenameMode};

    fn root() -> PathBuf {
        PathBuf::from("/media")
    }

    fn added_batch(names: &[&str]) -> Vec<MediaChange> {
        names
            .iter()
            .map(|name| MediaChange::Added(PathBuf::from(format!("/media/{name}"))))
            .collect()
    }

    #[test]
    fn a_brief_clock_gap_is_not_sleep() {
        assert!(!slept(
            Duration::from_millis(250),
            Duration::from_millis(249)
        ));
        assert!(!slept(Duration::from_secs(20), Duration::from_secs(19)));
        assert!(!slept(Duration::from_secs(30), Duration::from_secs(29)));
    }

    #[test]
    fn a_wall_clock_far_ahead_of_the_monotonic_clock_is_sleep() {
        assert!(slept(
            Duration::from_secs(3_600),
            Duration::from_millis(250)
        ));
        assert!(slept(Duration::from_secs(31), Duration::ZERO));
        assert!(!slept(Duration::from_secs(360), Duration::from_secs(354)));
    }

    #[test]
    fn a_backwards_wall_clock_is_not_sleep() {
        assert!(!slept(Duration::ZERO, Duration::from_secs(250)));
        assert!(!slept(Duration::from_secs(5), Duration::from_secs(60)));
    }

    #[test]
    fn a_batch_of_arrivals_is_an_ingest_and_a_removal_is_a_withdrawal() {
        let arrivals = work_for(
            &root(),
            &added_batch(&["Arrival.2016.mkv", "Dune.2021.mkv"]),
        );
        assert_eq!(
            arrivals.ingest,
            vec![
                PathBuf::from("/media/Arrival.2016.mkv"),
                PathBuf::from("/media/Dune.2021.mkv")
            ]
        );
        assert!(
            !arrivals.withdraw,
            "an arrival must never withdraw anything"
        );
        assert!(!arrivals.is_empty());

        let removal = work_for(
            &root(),
            &[MediaChange::Removed(PathBuf::from(
                "/media/Arrival.2016.mkv",
            ))],
        );
        assert!(
            removal.withdraw,
            "a removal needs the walk that can withdraw"
        );
        assert!(removal.ingest.is_empty());

        let both = work_for(
            &root(),
            &[
                MediaChange::Added(PathBuf::from("/media/Dune.2021.mkv")),
                MediaChange::Removed(PathBuf::from("/media/Arrival.2016.mkv")),
            ],
        );
        assert_eq!(both.ingest, vec![PathBuf::from("/media/Dune.2021.mkv")]);
        assert!(both.withdraw, "the same walk also ingests the arrival");

        assert!(work_for(&root(), &added_batch(&["notes.txt"])).is_empty());
        assert!(
            work_for(
                &root(),
                &[MediaChange::Added(PathBuf::from("/elsewhere/Film.mkv"))]
            )
            .is_empty()
        );
    }

    #[test]
    fn a_batch_over_the_plan_bound_escalates_to_a_full_walk() {
        let burst: Vec<MediaChange> = (0..MAX_PLAN_ENTRIES + 50)
            .map(|index| MediaChange::Added(PathBuf::from(format!("/media/Film.{index}.mkv"))))
            .collect();

        let work = work_for(&root(), &burst);

        assert!(work.withdraw, "a capped plan must not run as a partial job");
        assert!(
            work.ingest.is_empty(),
            "the walk that covers the burst ingests those paths itself"
        );
    }

    #[test]
    fn a_second_batch_for_a_running_root_is_queued_rather_than_started_again() {
        let mut queue = Queue::new();
        let first = queue
            .submit(Job::Ingest {
                source: "movies".to_string(),
                root: root(),
                paths: vec![PathBuf::from("/media/Arrival.2016.mkv")],
            })
            .expect("an idle queue starts the first job");
        assert_eq!(queue.running(), Some(&first));

        assert!(
            queue
                .submit(Job::Ingest {
                    source: "movies".to_string(),
                    root: root(),
                    paths: vec![PathBuf::from("/media/Dune.2021.mkv")],
                })
                .is_none(),
            "a second scanner started while one was running"
        );
        assert_eq!(queue.queued(), 1, "the batch was dropped instead of queued");

        let next = queue.finish().expect("the queued batch runs next");
        assert_eq!(next.paths().len(), 1);
        assert_eq!(next.paths()[0], PathBuf::from("/media/Dune.2021.mkv"));
        assert_eq!(queue.queued(), 0);

        let mut queue = Queue::new();
        queue
            .submit(Job::Scan {
                source: "movies".to_string(),
                quiet: false,
            })
            .expect("an idle queue starts the walk");
        assert!(
            queue
                .submit(Job::Ingest {
                    source: "movies".to_string(),
                    root: root(),
                    paths: vec![PathBuf::from("/media/Arrival.2016.mkv")],
                })
                .is_none(),
            "a second scanner started while the walk ran"
        );
        assert!(
            queue
                .submit(Job::Ingest {
                    source: "movies".to_string(),
                    root: root(),
                    paths: vec![PathBuf::from("/media/Arrival.2016.mkv")],
                })
                .is_none(),
            "a second scanner started while the walk ran"
        );
        assert_eq!(queue.queued(), 1, "the same path was queued twice");
        let next = queue.finish().expect("the batch behind the walk runs next");
        assert_eq!(next.paths(), [PathBuf::from("/media/Arrival.2016.mkv")]);
    }

    #[test]
    fn a_walk_that_is_already_queued_absorbs_the_batches_behind_it() {
        let mut queue = Queue::new();
        queue.submit(Job::Ingest {
            source: "movies".to_string(),
            root: root(),
            paths: vec![PathBuf::from("/media/Arrival.2016.mkv")],
        });
        queue.submit(Job::Scan {
            source: "movies".to_string(),
            quiet: true,
        });
        queue.submit(Job::Ingest {
            source: "movies".to_string(),
            root: root(),
            paths: vec![PathBuf::from("/media/Dune.2021.mkv")],
        });

        assert_eq!(queue.queued(), 1);
        let next = queue.finish().expect("the walk");
        assert!(next.is_scan());
        assert_eq!(next.source(), "movies");
        assert!(queue.finish().is_none());
    }

    #[test]
    fn an_event_kind_becomes_exactly_the_change_it_describes() {
        let file = PathBuf::from("/media/Arrival.2016.mkv");
        let created =
            interpret(&Event::new(EventKind::Create(CreateKind::File)).add_path(file.clone()));
        assert_eq!(created.changes, vec![MediaChange::Added(file.clone())]);
        assert!(created.rescan.is_none());

        let removed =
            interpret(&Event::new(EventKind::Remove(RemoveKind::File)).add_path(file.clone()));
        assert_eq!(removed.changes, vec![MediaChange::Removed(file.clone())]);

        let written = interpret(
            &Event::new(EventKind::Modify(ModifyKind::Data(
                notify::event::DataChange::Content,
            )))
            .add_path(file.clone()),
        );
        assert_eq!(written.changes, vec![MediaChange::Modified(file.clone())]);

        let renamed_without_a_partner = interpret(
            &Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::Any)))
                .add_path(file.clone()),
        );
        assert_eq!(
            renamed_without_a_partner.changes,
            vec![MediaChange::Added(file.clone())],
            "a rename whose other side is unknown re-adds rather than withdraws"
        );

        let dropped = interpret(&Event::new(EventKind::Other));
        assert!(dropped.rescan.is_some(), "a dropped event needs a walk");

        let touched = interpret(
            &Event::new(EventKind::Access(AccessKind::Open(
                notify::event::AccessMode::Read,
            )))
            .add_path(file.clone()),
        );
        assert!(touched.changes.is_empty() && touched.rescan.is_none());

        let folder_gone = interpret(
            &Event::new(EventKind::Remove(RemoveKind::Folder))
                .add_path(PathBuf::from("/media/Show")),
        );
        assert!(
            folder_gone.rescan.is_some(),
            "files under a removed folder are not listed individually"
        );

        let root_changed = interpret(
            &Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::From)))
                .add_path(PathBuf::from("/media"))
                .set_info(ROOT_CHANGED_INFO),
        );
        assert!(root_changed.rescan.is_some());

        let partial = interpret(
            &Event::new(EventKind::Create(CreateKind::File))
                .add_path(PathBuf::from("/media/Arrival.2016.mkv.part")),
        );
        assert!(
            partial.changes.is_empty(),
            "a download in progress was accepted"
        );
    }

    #[test]
    fn a_watcher_reports_the_folder_leaving_and_stops_watching_it() {
        let directory = tempfile::TempDir::new().expect("temp dir");
        let root = directory.path().join("media");
        std::fs::create_dir_all(&root).expect("folder");

        let mut set = WatchSet::new();
        assert_eq!(
            set.sync(std::slice::from_ref(&root)),
            vec![root.clone()],
            "a readable folder is armed"
        );
        assert!(set.is_watching(&root));

        std::fs::remove_dir_all(&root).expect("remove folder");

        let deadline = Instant::now() + Duration::from_secs(10);
        let mut stopped = false;
        while Instant::now() < deadline && !stopped {
            for event in set.poll() {
                if let Watched::Stopped { root: lost, reason } = event {
                    assert_eq!(lost, root);
                    assert!(!reason.is_empty());
                    stopped = true;
                }
            }
            std::thread::sleep(Duration::from_millis(50));
        }

        assert!(stopped, "a folder that vanished was never reported");
        assert!(
            !set.is_watching(&root),
            "the app still believed it was watching"
        );
    }
}
