use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant};

use crate::scanner::MEDIA_EXTENSIONS;

pub const MAX_PLAN_ENTRIES: usize = 4_096;

const PARTIAL_SUFFIXES: &[&str] = &[
    ".part",
    ".crdownload",
    ".tmp",
    ".download",
    ".opdownload",
    ".aria2",
    ".!qb",
];

const IGNORED_NAMES: &[&str] = &[".DS_Store", "$RECYCLE.BIN", ".Spotlight-V100", ".Trashes"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaChange {
    Added(PathBuf),

    Modified(PathBuf),

    Removed(PathBuf),

    Renamed(PathBuf, PathBuf),
}

impl MediaChange {
    pub fn target(&self) -> &Path {
        match self {
            Self::Added(path) | Self::Modified(path) | Self::Removed(path) => path,
            Self::Renamed(_, to) => to,
        }
    }

    pub fn previous(&self) -> Option<&Path> {
        match self {
            Self::Renamed(from, _) => Some(from),
            _ => None,
        }
    }

    fn is_removal(&self) -> bool {
        matches!(self, Self::Removed(_))
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    pub ingest: Vec<PathBuf>,

    pub forget: Vec<PathBuf>,

    pub relocations: Vec<(PathBuf, PathBuf)>,
}

impl Plan {
    pub fn is_empty(&self) -> bool {
        self.ingest.is_empty() && self.forget.is_empty() && self.relocations.is_empty()
    }
}

pub fn accept(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    if name.is_empty() {
        return false;
    }
    if path.components().any(|component| match component {
        Component::Normal(part) => {
            let part = part.to_string_lossy();
            part.starts_with('.')
                || IGNORED_NAMES
                    .iter()
                    .any(|ignored| part.eq_ignore_ascii_case(ignored))
        }
        _ => false,
    }) {
        return false;
    }
    let name = name.to_lowercase();
    if PARTIAL_SUFFIXES.iter().any(|suffix| name.ends_with(suffix)) {
        return false;
    }
    let Some(extension) = Path::new(&name).extension() else {
        return false;
    };
    let extension = extension.to_string_lossy();
    MEDIA_EXTENSIONS.contains(&extension.as_ref())
}

pub fn settle(
    events: &[MediaChange],
    hidden: &mut HashSet<PathBuf>,
    quiet: Duration,
    _now: Instant,
) -> Vec<MediaChange> {
    let suppress = !quiet.is_zero();
    if !suppress {
        hidden.clear();
    }

    let mut order: Vec<PathBuf> = Vec::new();
    let mut collapsed: HashMap<PathBuf, MediaChange> = HashMap::new();
    for event in events {
        let key = event.target().to_path_buf();
        let replacement = match collapsed.get(&key) {
            Some(existing) => existing.is_removal() || !event.is_removal(),
            None => {
                order.push(key.clone());
                true
            }
        };
        if replacement {
            collapsed.insert(key, event.clone());
        }
    }

    let mut emitted = Vec::new();
    for key in order {
        let Some(change) = collapsed.remove(&key) else {
            continue;
        };
        if suppress && !hidden.insert(key) {
            continue;
        }
        emitted.push(change);
    }
    emitted
}

pub fn plan(root: &Path, changes: &[MediaChange]) -> Plan {
    let mut plan = Plan::default();
    let Some(root) = normalize(root) else {
        return plan;
    };

    for change in changes {
        match change {
            MediaChange::Added(path) | MediaChange::Modified(path) => {
                if scheduled(&root, path) {
                    push_unique(&mut plan.ingest, path);
                }
            }
            MediaChange::Removed(path) => {
                if scheduled(&root, path) {
                    push_unique(&mut plan.forget, path);
                }
            }
            MediaChange::Renamed(from, to) => {
                let from_inside = scheduled(&root, from);
                let to_inside = scheduled(&root, to);
                match (from_inside, to_inside) {
                    (true, true) if from != to => {
                        let relocation = (from.clone(), to.clone());
                        if plan.relocations.len() < MAX_PLAN_ENTRIES
                            && !plan.relocations.contains(&relocation)
                        {
                            plan.relocations.push(relocation);
                        }
                    }
                    (false, true) => push_unique(&mut plan.ingest, to),
                    (true, false) => push_unique(&mut plan.forget, from),
                    _ => {}
                }
            }
        }
    }

    let relocated_from: HashSet<&Path> = plan
        .relocations
        .iter()
        .map(|(from, _)| from.as_path())
        .collect();
    let relocated_to: HashSet<&Path> = plan
        .relocations
        .iter()
        .map(|(_, to)| to.as_path())
        .collect();
    plan.forget
        .retain(|path| !relocated_from.contains(path.as_path()));
    plan.ingest
        .retain(|path| !relocated_to.contains(path.as_path()));
    plan
}

fn push_unique(paths: &mut Vec<PathBuf>, path: &Path) {
    if paths.len() < MAX_PLAN_ENTRIES && !paths.iter().any(|known| known == path) {
        paths.push(path.to_path_buf());
    }
}

fn scheduled(root: &Path, path: &Path) -> bool {
    let Some(normalized) = normalize(path) else {
        return false;
    };
    let Ok(relative) = normalized.strip_prefix(root) else {
        return false;
    };
    if relative.as_os_str().is_empty() {
        return false;
    }
    accept(relative)
}

pub(crate) fn normalize(path: &Path) -> Option<PathBuf> {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => normalized.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    return None;
                }
            }
            Component::Normal(part) => normalized.push(part),
        }
    }
    (!normalized.as_os_str().is_empty()).then_some(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quiet() -> Duration {
        Duration::from_secs(2)
    }

    fn instant() -> Instant {
        Instant::now()
    }

    fn added(name: &str) -> MediaChange {
        MediaChange::Added(PathBuf::from(name))
    }

    #[test]
    fn a_rename_inside_the_root_collapses_to_one_relocation() {
        let root = Path::new("/media");
        let changes = [
            MediaChange::Renamed(
                PathBuf::from("/media/Old.2020.mkv"),
                PathBuf::from("/media/New.2020.mkv"),
            ),
            MediaChange::Removed(PathBuf::from("/media/Old.2020.mkv")),
            MediaChange::Added(PathBuf::from("/media/New.2020.mkv")),
        ];

        let plan = plan(root, &changes);

        assert_eq!(
            plan.relocations,
            vec![(
                PathBuf::from("/media/Old.2020.mkv"),
                PathBuf::from("/media/New.2020.mkv")
            )]
        );
        assert!(plan.forget.is_empty(), "a relocation must not withdraw");
        assert!(plan.ingest.is_empty(), "a relocation must not re-add");
    }

    #[test]
    fn a_path_outside_the_root_is_rejected() {
        let root = Path::new("/media");
        let changes = [
            added("/elsewhere/Film.2020.mkv"),
            MediaChange::Removed(PathBuf::from("/media/../../etc/passwd.mkv")),
            MediaChange::Added(PathBuf::from("../escape.mkv")),
            MediaChange::Renamed(
                PathBuf::from("/media/Old.2020.mkv"),
                PathBuf::from("/elsewhere/New.2020.mkv"),
            ),
            added("/media/Kept.2020.mkv"),
        ];

        let plan = plan(root, &changes);

        assert_eq!(plan.ingest, vec![PathBuf::from("/media/Kept.2020.mkv")]);
        assert_eq!(plan.forget, vec![PathBuf::from("/media/Old.2020.mkv")]);
        assert!(plan.relocations.is_empty());
    }

    #[test]
    fn a_burst_of_five_thousand_events_for_two_hundred_files_yields_two_hundred() {
        let mut events = Vec::new();
        for index in 0..200 {
            for round in 0..25 {
                let path = PathBuf::from(format!("/media/Film.{index}.2020.mkv"));
                events.push(if round % 2 == 0 {
                    MediaChange::Added(path)
                } else {
                    MediaChange::Modified(path)
                });
            }
        }
        assert_eq!(events.len(), 5_000);

        let mut hidden = HashSet::new();
        let emitted = settle(&events, &mut hidden, quiet(), instant());
        let plan = plan(Path::new("/media"), &emitted);

        assert_eq!(emitted.len(), 200);
        assert_eq!(plan.ingest.len(), 200);
    }

    #[test]
    fn a_partial_download_is_refused() {
        for suffix in [".part", ".crdownload", ".tmp", ".download", ".opdownload"] {
            let path = format!("/media/Film.2020.mkv{suffix}");
            assert!(
                !accept(Path::new(&path)),
                "{path} was accepted while still downloading"
            );
            assert!(
                !accept(Path::new(&format!("/media/Film.2020{suffix}"))),
                "{suffix} was accepted without a media extension"
            );
        }
        assert!(accept(Path::new("/media/Film.2020.mkv")));
    }

    #[test]
    fn an_apple_double_companion_is_refused() {
        assert!(!accept(Path::new("/media/._Film.2020.mkv")));
        assert!(!accept(Path::new("/media/Show/._Episode.S01E01.mkv")));
        assert!(accept(Path::new("/media/Film.2020.mkv")));
    }

    #[test]
    fn system_folders_and_hidden_paths_are_refused() {
        for path in [
            "/media/$RECYCLE.BIN/Film.2020.mkv",
            "/media/.Spotlight-V100/Film.2020.mkv",
            "/media/.Trashes/Film.2020.mkv",
            "/media/.hidden/Film.2020.mkv",
            "/media/.DS_Store",
        ] {
            assert!(!accept(Path::new(path)), "{path} was accepted");
        }
        assert!(accept(Path::new("/media/Show/Season 1/Episode.S01E01.mkv")));
    }

    #[test]
    fn an_identical_event_twice_inside_the_quiet_window_emits_once() {
        let mut hidden = HashSet::new();
        let burst = [added("/media/Film.2020.mkv"), added("/media/Film.2020.mkv")];

        let first = settle(&burst, &mut hidden, quiet(), instant());
        assert_eq!(first.len(), 1);

        let second = settle(&burst, &mut hidden, quiet(), instant());
        assert!(
            second.is_empty(),
            "a repeat inside the quiet window emitted again"
        );

        hidden.clear();
        let third = settle(&burst, &mut hidden, quiet(), instant());
        assert_eq!(third.len(), 1, "a new window emits the path again");
    }

    #[test]
    fn a_collapsed_burst_keeps_the_latest_state_per_path() {
        let mut hidden = HashSet::new();
        let burst = [
            MediaChange::Removed(PathBuf::from("/media/Film.2020.mkv")),
            MediaChange::Added(PathBuf::from("/media/Film.2020.mkv")),
            MediaChange::Modified(PathBuf::from("/media/Show.S01E01.mkv")),
        ];

        let emitted = settle(&burst, &mut hidden, quiet(), instant());

        assert_eq!(emitted.len(), 2);
        assert_eq!(
            plan(Path::new("/media"), &emitted).ingest,
            vec![
                PathBuf::from("/media/Film.2020.mkv"),
                PathBuf::from("/media/Show.S01E01.mkv")
            ],
            "an add after a remove is an ingest, not a withdrawal"
        );
    }

    #[test]
    fn a_zero_quiet_window_lets_every_drain_through() {
        let mut hidden = HashSet::new();
        let burst = [added("/media/Film.2020.mkv")];
        let zero = Duration::ZERO;

        assert_eq!(settle(&burst, &mut hidden, zero, instant()).len(), 1);
        assert_eq!(
            settle(&burst, &mut hidden, zero, instant()).len(),
            1,
            "a forced drain is never suppressed"
        );
    }

    #[test]
    fn the_plan_is_bounded() {
        let events: Vec<MediaChange> = (0..MAX_PLAN_ENTRIES * 2)
            .map(|index| added(&format!("/media/Film.{index}.2020.mkv")))
            .collect();

        let plan = plan(Path::new("/media"), &events);

        assert_eq!(plan.ingest.len(), MAX_PLAN_ENTRIES);
    }

    #[test]
    fn a_removal_is_forgotten_and_an_addition_is_ingested_once() {
        let plan = plan(
            Path::new("/media"),
            &[
                MediaChange::Added(PathBuf::from("/media/New.2020.mkv")),
                MediaChange::Added(PathBuf::from("/media/New.2020.mkv")),
                MediaChange::Removed(PathBuf::from("/media/Gone.2020.mkv")),
                MediaChange::Modified(PathBuf::from("/media/Edited.2020.mkv")),
                added("/media/notes.txt"),
            ],
        );

        assert_eq!(plan.ingest.len(), 2);
        assert_eq!(plan.forget, vec![PathBuf::from("/media/Gone.2020.mkv")]);
        assert!(!plan.is_empty());
    }

    #[test]
    fn a_rename_that_leaves_the_root_is_a_removal_and_one_that_enters_is_an_addition() {
        let plan = plan(
            Path::new("/media"),
            &[
                MediaChange::Renamed(
                    PathBuf::from("/media/Film.2020.mkv"),
                    PathBuf::from("/elsewhere/Film.2020.mkv"),
                ),
                MediaChange::Renamed(
                    PathBuf::from("/elsewhere/Other.2020.mkv"),
                    PathBuf::from("/media/Other.2020.mkv"),
                ),
            ],
        );

        assert_eq!(plan.forget, vec![PathBuf::from("/media/Film.2020.mkv")]);
        assert_eq!(plan.ingest, vec![PathBuf::from("/media/Other.2020.mkv")]);
        assert!(plan.relocations.is_empty());
    }
}
