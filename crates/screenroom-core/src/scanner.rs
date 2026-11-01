use std::collections::VecDeque;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};

use crate::domain::SourceState;
use crate::error::{CoreError, Result};
use crate::filename::parse_filename;
use crate::fingerprint::media_fingerprint;
use crate::library::{Ingest, Library};

pub const MEDIA_EXTENSIONS: &[&str] = &[
    "mkv", "mp4", "m4v", "avi", "mov", "webm", "mpeg", "mpg", "ts", "m2ts", "wmv", "flv", "ogv",
    "3gp",
];

const DOWNLOAD_SUFFIXES: &[&str] = &[".part", ".crdownload", ".download", ".aria2", ".!qB"];

#[derive(Debug, Clone, Default)]
pub struct ScanToken(Arc<AtomicBool>);

impl ScanToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanIssue {
    pub relative_path: String,

    pub code: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanUpdate {
    pub visited: u64,

    pub imported: u64,

    pub errors: Vec<ScanIssue>,

    pub done: bool,

    pub cancelled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanSummary {
    pub visited: u64,

    pub imported: u64,

    pub issues: Vec<ScanIssue>,

    pub cancelled: bool,

    pub complete: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PartialScan {
    pub(crate) root_trusted: bool,
    pub(crate) root_listed: bool,
    pub(crate) missing: Vec<String>,
}

#[derive(Debug)]
pub struct LibraryScanner<'a> {
    library: &'a Library,
    max_entries: u64,
    max_depth: usize,
    stable_age: Duration,
    max_issues: usize,
}

impl<'a> LibraryScanner<'a> {
    pub fn new(library: &'a Library) -> Self {
        Self {
            library,
            max_entries: 250_000,
            max_depth: 64,
            stable_age: Duration::from_secs(15),
            max_issues: 1_000,
        }
    }

    pub fn with_limits(mut self, max_entries: u64, max_depth: usize, stable_age: Duration) -> Self {
        self.max_entries = max_entries;
        self.max_depth = max_depth;
        self.stable_age = stable_age;
        self
    }

    pub fn scan(
        &self,
        source_id: &str,
        token: &ScanToken,
        mut on_update: impl FnMut(&ScanUpdate),
    ) -> Result<ScanSummary> {
        self.library.claim_scan(source_id)?;
        let _guard = ScanGuard {
            library: self.library,
            source_id: source_id.to_string(),
        };

        let source = self.library.source(source_id)?;
        if source.state == SourceState::Removed {
            return Err(CoreError::library(
                "source_removed",
                "This source was removed.",
            ));
        }
        if source.root_path.is_empty() {
            return Err(CoreError::library(
                "source_disconnected",
                "Relink the source before scanning.",
            ));
        }

        let mut issues = Issues::new(self.max_issues);
        let root = match std::fs::canonicalize(&source.root_path) {
            Ok(root) => root,
            Err(error) => {
                let (state, code) = unavailable_root(Path::new(&source.root_path), error.kind());
                self.library.set_source_state(source_id, state)?;
                issues.push("", code);
                let update = ScanUpdate {
                    visited: 0,
                    imported: 0,
                    errors: issues.snapshot(),
                    done: true,
                    cancelled: false,
                };
                on_update(&update);
                return Ok(ScanSummary {
                    visited: 0,
                    imported: 0,
                    issues: issues.into_vec(),
                    cancelled: false,
                    complete: false,
                });
            }
        };

        let stored_key = self.library.path_key(Path::new(&source.root_path));
        if self.library.path_key(&root) != stored_key {
            return Err(CoreError::library(
                "unsafe_path",
                "The source was replaced by a symbolic link. Relink it explicitly.",
            ));
        }

        let scan_id = crate::library::new_id();
        let mut visited = 0_u64;
        let mut imported = 0_u64;
        let mut bounded = false;
        let mut queue = VecDeque::from([(root.clone(), 0_usize)]);

        while let Some((directory, depth)) = queue.pop_front() {
            if token.is_cancelled() {
                break;
            }
            let entries = match std::fs::read_dir(&directory) {
                Ok(entries) => entries,
                Err(error) => {
                    issues.push_at(
                        &directory,
                        &root,
                        if error.kind() == std::io::ErrorKind::PermissionDenied {
                            "permission_lost"
                        } else {
                            "directory_unavailable"
                        },
                    );
                    continue;
                }
            };
            for entry in entries {
                if token.is_cancelled() {
                    break;
                }
                let entry = match entry {
                    Ok(entry) => entry,
                    Err(error) => {
                        issues.push_at(
                            &directory,
                            &root,
                            if error.kind() == std::io::ErrorKind::PermissionDenied {
                                "permission_lost"
                            } else {
                                "read_failed"
                            },
                        );
                        continue;
                    }
                };
                visited += 1;
                if visited > self.max_entries {
                    bounded = true;
                    issues.push("", "entry_limit");
                    break;
                }

                let path = entry.path();
                let Ok(relative) = path.strip_prefix(&root) else {
                    issues.push_at(&path, &root, "outside_root");
                    continue;
                };
                let Some(relative) = relative.to_str() else {
                    issues.push_at(&path, &root, "invalid_name");
                    continue;
                };
                let relative = relative.to_string();
                let Ok(file_type) = entry.file_type() else {
                    issues.push(&relative, "read_failed");
                    continue;
                };
                if file_type.is_symlink() {
                    continue;
                }
                if file_type.is_dir() {
                    if depth >= self.max_depth {
                        issues.push(&relative, "depth_limit");
                        continue;
                    }
                    if !is_hidden_name(&entry.file_name()) {
                        queue.push_back((path, depth + 1));
                    }
                    continue;
                }
                if !file_type.is_file() || is_hidden_name(&entry.file_name()) || !is_media(&path) {
                    continue;
                }
                match self.import(
                    source_id,
                    &root,
                    &source.root_path,
                    &relative,
                    &scan_id,
                    token,
                ) {
                    Ok(()) => {
                        imported += 1;
                        on_update(&ScanUpdate {
                            visited,
                            imported,
                            errors: issues.snapshot(),
                            done: false,
                            cancelled: false,
                        });
                    }
                    Err(error) if error.code() == "still_downloading" => {
                        issues.push(&relative, "still_downloading");
                    }
                    Err(error) if error.code() == "unsafe_path" => {
                        issues.push(&relative, "unsafe_path");
                    }
                    Err(error) if error.code() == "cancelled" => break,
                    Err(CoreError::Io(error))
                        if error.kind() == std::io::ErrorKind::PermissionDenied =>
                    {
                        issues.push(&relative, "permission_lost");
                    }
                    Err(CoreError::Io(_)) => issues.push(&relative, "read_failed"),
                    Err(error) => return Err(error),
                }
            }
            if bounded {
                break;
            }
        }

        let cancelled = token.is_cancelled();
        let complete = !bounded && issues.is_empty();
        if !cancelled {
            let partial = self.probe_partial_scan(
                source_id,
                &scan_id,
                Path::new(&source.root_path),
                complete,
            )?;
            self.library.finish_scan(
                source_id,
                &scan_id,
                complete,
                Some(&source.root_path),
                &partial,
            )?;
        }
        let update = ScanUpdate {
            visited,
            imported,
            errors: issues.snapshot(),
            done: true,
            cancelled,
        };
        on_update(&update);
        Ok(ScanSummary {
            visited,
            imported,
            issues: issues.into_vec(),
            cancelled,
            complete,
        })
    }

    fn probe_partial_scan(
        &self,
        source_id: &str,
        scan_id: &str,
        root: &Path,
        complete: bool,
    ) -> Result<PartialScan> {
        let resolved = std::fs::canonicalize(root).ok();
        let root_trusted = resolved
            .as_ref()
            .is_some_and(|resolved| self.library.path_key(resolved) == self.library.path_key(root));
        if complete {
            return Ok(PartialScan {
                root_trusted,
                root_listed: false,
                missing: Vec::new(),
            });
        }

        let root_listed = root_trusted && std::fs::read_dir(root).is_ok();
        let mut missing = Vec::new();
        if root_trusted {
            let limit = self.max_entries.max(1);
            for (id, relative) in self.library.pending_locators(source_id, scan_id, limit)? {
                if !is_plain_relative(&relative) {
                    continue;
                }
                #[cfg(test)]
                probe_observer::notify();
                if confirmed_missing(&root.join(&relative)) {
                    missing.push(id);
                }
            }
        }
        Ok(PartialScan {
            root_trusted,
            root_listed,
            missing,
        })
    }

    fn import(
        &self,
        source_id: &str,
        root: &Path,
        expected_root: &str,
        relative: &str,
        scan_id: &str,
        token: &ScanToken,
    ) -> Result<()> {
        let path = root.join(relative);
        let metadata = std::fs::metadata(&path)?;
        if metadata.len() == 0 || self.is_settling(metadata.modified().ok()) {
            return Err(CoreError::library(
                "still_downloading",
                "The file is still being written.",
            ));
        }
        if has_download_sidecar(&path) {
            return Err(CoreError::library(
                "still_downloading",
                "The file is still being downloaded.",
            ));
        }
        if token.is_cancelled() {
            return Err(CoreError::library("cancelled", "The scan was cancelled."));
        }

        let size = metadata.len();
        let fingerprint = media_fingerprint(&path, size)?;

        let after = std::fs::metadata(&path)?;
        if after.len() != size || after.modified().ok() != metadata.modified().ok() {
            return Err(CoreError::library(
                "still_downloading",
                "The file changed while it was being read.",
            ));
        }
        let modified_ms = after
            .modified()
            .ok()
            .and_then(|time| time.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map(|elapsed| elapsed.as_millis() as i64)
            .unwrap_or_default();

        let mut folder = Path::new(relative)
            .parent()
            .map(|parent| parent.to_string_lossy().to_string())
            .unwrap_or_default();
        if let Some(parent) = Path::new(&folder).file_name()
            && is_season_folder(&parent.to_string_lossy())
        {
            folder = Path::new(&folder)
                .parent()
                .map(|value| value.to_string_lossy().to_string())
                .unwrap_or_default();
        }
        let parsed = parse_filename(relative, Some(&folder));
        self.library.ingest_file(Ingest {
            source_id,
            relative_path: relative,
            fingerprint: &fingerprint,
            size: size as i64,
            modified_ms,
            scan_id,
            parsed: Some(&parsed),
            expected_root: Some(expected_root),
        })?;
        Ok(())
    }

    fn is_settling(&self, modified: Option<SystemTime>) -> bool {
        let Some(modified) = modified else {
            return false;
        };
        match SystemTime::now().duration_since(modified) {
            Ok(age) => age < self.stable_age,
            Err(_) => false,
        }
    }
}

struct ScanGuard<'a> {
    library: &'a Library,
    source_id: String,
}

impl Drop for ScanGuard<'_> {
    fn drop(&mut self) {
        self.library.release_scan(&self.source_id);
    }
}

fn is_hidden_name(name: &std::ffi::OsStr) -> bool {
    name.to_string_lossy().starts_with('.')
}

fn is_media(path: &Path) -> bool {
    path.extension()
        .map(|extension| extension.to_string_lossy().to_lowercase())
        .is_some_and(|extension| MEDIA_EXTENSIONS.contains(&extension.as_str()))
}

fn is_plain_relative(relative: &str) -> bool {
    let path = Path::new(relative);
    !relative.is_empty()
        && !path.is_absolute()
        && !path.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
}

fn confirmed_missing(path: &Path) -> bool {
    matches!(
        std::fs::metadata(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound
    )
}

fn is_season_folder(name: &str) -> bool {
    let lower = name.to_lowercase();
    let Some(number) = lower.strip_prefix("season") else {
        return false;
    };
    let number = number.trim_start_matches([' ', '.', '_', '-']);
    !number.is_empty() && number.chars().all(|character| character.is_ascii_digit())
}

fn unavailable_root(stored_root: &Path, kind: std::io::ErrorKind) -> (SourceState, &'static str) {
    if kind == std::io::ErrorKind::PermissionDenied {
        return (SourceState::PermissionLost, "permission_lost");
    }
    match stored_root.parent() {
        Some(parent) if !parent.as_os_str().is_empty() && parent.exists() => {
            (SourceState::Missing, "source_missing")
        }

        _ => (SourceState::Disconnected, "source_disconnected"),
    }
}

fn has_download_sidecar(path: &Path) -> bool {
    let name = path.to_string_lossy();
    DOWNLOAD_SUFFIXES
        .iter()
        .any(|suffix| Path::new(&format!("{name}{suffix}")).exists())
}

struct Issues {
    entries: Vec<ScanIssue>,
    max: usize,
}

impl Issues {
    fn new(max: usize) -> Self {
        Self {
            entries: Vec::new(),
            max,
        }
    }

    fn push(&mut self, relative_path: &str, code: &'static str) {
        if self.entries.len() < self.max {
            self.entries.push(ScanIssue {
                relative_path: relative_path.to_string(),
                code,
            });
        } else if self.entries.len() == self.max {
            self.entries.push(ScanIssue {
                relative_path: String::new(),
                code: "additional_errors_omitted",
            });
        }
    }

    fn push_at(&mut self, path: &Path, root: &Path, code: &'static str) {
        let relative = path
            .strip_prefix(root)
            .map(|value| value.to_string_lossy().to_string())
            .unwrap_or_default();
        self.push(&relative, code);
    }

    fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn snapshot(&self) -> Vec<ScanIssue> {
        self.entries.clone()
    }

    fn into_vec(self) -> Vec<ScanIssue> {
        self.entries
    }
}

#[cfg(test)]
pub(crate) mod probe_observer {
    use std::sync::{Arc, Mutex};

    type Observer = Arc<dyn Fn() + Send + Sync>;

    static OBSERVER: Mutex<Option<Observer>> = Mutex::new(None);

    pub(crate) fn install(observer: Observer) {
        *OBSERVER
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(observer);
    }

    pub(crate) fn clear() {
        *OBSERVER
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
    }

    pub(crate) fn notify() {
        let observer = OBSERVER
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        if let Some(observer) = observer {
            observer();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs::{self, File};
    use std::sync::atomic::AtomicUsize;
    use std::sync::mpsc;

    use tempfile::TempDir;

    use super::*;
    use crate::LibraryQuery;

    fn write_settled(path: &Path, contents: &[u8]) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, contents).unwrap();
        File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(SystemTime::now() - Duration::from_secs(3600))
            .unwrap();
    }

    #[test]
    fn a_partial_scan_collects_its_stats_before_opening_the_write_transaction() {
        let directory = TempDir::new().unwrap();
        let root = directory.path();
        write_settled(&root.join("Gone.2020.mkv"), b"gone");
        write_settled(&root.join("Keep.2020.mkv"), b"keep");

        let library = Arc::new(Library::open_in_memory().unwrap());
        let source = library.add_source(root, None).unwrap();
        let scanner =
            LibraryScanner::new(&library).with_limits(10_000, 32, Duration::from_secs(15));
        scanner.scan(&source.id, &ScanToken::new(), |_| {}).unwrap();

        fs::remove_file(root.join("Gone.2020.mkv")).unwrap();
        write_settled(&root.join("Downloading.2020.mkv"), b"partial");
        fs::write(root.join("Downloading.2020.mkv.part"), b"downloading").unwrap();

        let probed = Arc::new(AtomicUsize::new(0));
        let held = Arc::new(AtomicBool::new(false));
        {
            let reader = Arc::clone(&library);
            let probed = Arc::clone(&probed);
            let held = Arc::clone(&held);
            probe_observer::clear();
            probe_observer::install(Arc::new(move || {
                probed.fetch_add(1, Ordering::SeqCst);
                let (done_tx, done_rx) = mpsc::channel();
                let reader = Arc::clone(&reader);
                std::thread::spawn(move || {
                    let _ = done_tx.send(reader.count(&LibraryQuery::default()).is_ok());
                });
                if !done_rx
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap_or(false)
                {
                    held.store(true, Ordering::SeqCst);
                }
            }));
        }

        let summary = scanner.scan(&source.id, &ScanToken::new(), |_| {}).unwrap();
        probe_observer::clear();

        assert!(
            !summary.complete,
            "the download sidecar keeps the scan partial"
        );
        assert!(
            probed.load(Ordering::SeqCst) > 0,
            "the partial scan collected no stats to observe"
        );
        assert!(
            !held.load(Ordering::SeqCst),
            "a library read was blocked while the stats were being collected"
        );

        let items = library.items(&LibraryQuery::page(10)).unwrap();
        let gone = items
            .iter()
            .find(|item| item.filename == "Gone.2020.mkv")
            .unwrap();
        assert!(
            !gone.available,
            "the confirmed-missing file stayed available"
        );
        let keep = items
            .iter()
            .find(|item| item.filename == "Keep.2020.mkv")
            .unwrap();
        assert!(keep.available);
    }
}
