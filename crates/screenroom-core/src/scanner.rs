use std::collections::HashMap;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};

use crate::domain::SourceState;
use crate::error::{CoreError, Result};
use crate::filename::{ParsedName, parse_filename};
use crate::fingerprint::media_fingerprint;
use crate::library::{Ingest, Library, StoredLocator};
use crate::volume::VolumeIdentity;

pub const MEDIA_EXTENSIONS: &[&str] = &[
    "mkv", "mp4", "m4v", "avi", "mov", "webm", "mpeg", "mpg", "ts", "m2ts", "wmv", "flv", "ogv",
    "3gp",
];

const DOWNLOAD_SUFFIXES: &[&str] = &[".part", ".crdownload", ".download", ".aria2", ".!qB"];

const INGEST_CHUNK: usize = 256;

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
    pub(crate) root_reachable: bool,
    pub(crate) withdrawal_allowed: bool,
    pub(crate) missing: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VolumeCheck {
    Matched,

    Unknown,

    Mismatch,

    Unreadable,
}

impl VolumeCheck {
    fn classify(stored: Option<VolumeIdentity>, current: Option<VolumeIdentity>) -> Self {
        match (stored, current) {
            (Some(stored), Some(current)) => {
                if stored.matches(&current) {
                    Self::Matched
                } else {
                    Self::Mismatch
                }
            }
            (Some(_), None) => Self::Unreadable,
            (None, _) => Self::Unknown,
        }
    }

    fn usable(self) -> bool {
        !matches!(self, Self::Mismatch | Self::Unreadable)
    }
}

#[derive(Debug, Clone)]
struct PreparedFile {
    relative_path: String,
    fingerprint: String,
    size: i64,
    modified_ms: i64,
    parsed: ParsedName,
}

type LocatorFacts = HashMap<String, StoredLocator>;

#[derive(Debug)]
pub struct LibraryScanner<'a> {
    library: &'a Library,
    max_entries: u64,
    max_depth: usize,
    stable_age: Duration,
    max_issues: usize,
    full_fingerprint: bool,
}

impl<'a> LibraryScanner<'a> {
    pub fn new(library: &'a Library) -> Self {
        Self {
            library,
            max_entries: 250_000,
            max_depth: 64,
            stable_age: Duration::from_secs(15),
            max_issues: 1_000,
            full_fingerprint: false,
        }
    }

    pub fn with_limits(mut self, max_entries: u64, max_depth: usize, stable_age: Duration) -> Self {
        self.max_entries = max_entries;
        self.max_depth = max_depth;
        self.stable_age = stable_age;
        self
    }

    pub fn with_full_fingerprint(mut self, enabled: bool) -> Self {
        self.full_fingerprint = enabled;
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

        let current_volume = VolumeIdentity::probe(&root);
        let volume = VolumeCheck::classify(
            self.library.source_volume_identity(source_id)?,
            current_volume.clone(),
        );
        let facts = self.library.locator_facts(source_id)?;

        let scan_id = crate::library::new_id();
        let mut visited = 0_u64;
        let mut imported = 0_u64;
        let mut bounded = false;
        let mut prepared: Vec<PreparedFile> = Vec::new();
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
                match self.prepare(&root, &relative, &facts, token) {
                    Ok(file) => {
                        prepared.push(file);
                        imported += 1;
                        if prepared.len() >= INGEST_CHUNK {
                            self.apply(source_id, &source.root_path, &scan_id, &prepared)?;
                            prepared.clear();
                        }
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

        if !prepared.is_empty() {
            self.apply(source_id, &source.root_path, &scan_id, &prepared)?;
        }

        let cancelled = token.is_cancelled();
        let volume = self.settle_volume(source_id, volume, &current_volume, imported)?;
        if volume == VolumeCheck::Mismatch {
            issues.push("", "volume_mismatch");
        } else if volume == VolumeCheck::Unreadable {
            issues.push("", "volume_unreadable");
        }
        let complete = !bounded && issues.is_empty();
        let known = facts.len() as u64;
        let withdrawal = withdrawal_allowed(volume, imported, known);
        if !cancelled {
            let partial = self.probe_partial_scan(
                source_id,
                &scan_id,
                &root,
                complete && withdrawal,
                withdrawal,
                volume.usable(),
            )?;
            self.library.finish_scan(
                source_id,
                &scan_id,
                complete,
                Some(&source.root_path),
                &partial,
            )?;
            if !volume.usable() {
                self.library
                    .mark_source_unreachable(source_id, SourceState::Disconnected)?;
            }
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

    pub fn scan_paths(
        &mut self,
        root: &Path,
        paths: &[PathBuf],
        token: &ScanToken,
        mut on_update: impl FnMut(&ScanUpdate),
    ) -> Result<ScanSummary> {
        let resolved = self.source_for_root(root)?;
        self.library.claim_scan(&resolved.id)?;
        let _guard = ScanGuard {
            library: self.library,
            source_id: resolved.id.clone(),
        };

        let source = self.library.source(&resolved.id)?;
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
        let root = std::fs::canonicalize(&source.root_path)?;
        if self.library.path_key(&root) != self.library.path_key(Path::new(&source.root_path)) {
            return Err(CoreError::library(
                "unsafe_path",
                "The source was replaced by a symbolic link. Relink it explicitly.",
            ));
        }

        let mut issues = Issues::new(self.max_issues);
        let current_volume = VolumeIdentity::probe(&root);
        let volume = VolumeCheck::classify(
            self.library.source_volume_identity(&source.id)?,
            current_volume.clone(),
        );
        let facts = self.library.locator_facts(&source.id)?;
        let scan_id = crate::library::new_id();
        let mut prepared: Vec<PreparedFile> = Vec::new();
        let mut visited = 0_u64;
        let mut imported = 0_u64;

        for path in paths {
            if token.is_cancelled() {
                break;
            }
            visited += 1;
            let Some(relative) = relative_to(&root, path) else {
                issues.push_at(path, &root, "outside_root");
                continue;
            };
            if !crate::watch::accept(Path::new(&relative)) {
                continue;
            }
            if reached_through_symlink(&root, &relative) {
                issues.push(&relative, "unsafe_path");
                continue;
            }
            match std::fs::symlink_metadata(root.join(&relative)) {
                Ok(metadata) if metadata.file_type().is_symlink() => continue,
                Ok(metadata) if metadata.is_dir() => continue,
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                    issues.push(&relative, "permission_lost");
                    continue;
                }
                Err(_) => {
                    issues.push(&relative, "read_failed");
                    continue;
                }
            }
            match self.prepare(&root, &relative, &facts, token) {
                Ok(file) => {
                    prepared.push(file);
                    imported += 1;
                    if prepared.len() >= INGEST_CHUNK {
                        self.apply(&source.id, &source.root_path, &scan_id, &prepared)?;
                        prepared.clear();
                    }
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

        if !prepared.is_empty() {
            self.apply(&source.id, &source.root_path, &scan_id, &prepared)?;
        }

        let cancelled = token.is_cancelled();
        let volume = self.settle_volume(&source.id, volume, &current_volume, imported)?;
        if volume == VolumeCheck::Mismatch {
            issues.push("", "volume_mismatch");
        } else if volume == VolumeCheck::Unreadable {
            issues.push("", "volume_unreadable");
        }
        if !cancelled {
            let root_listed = std::fs::read_dir(&root).is_ok();
            let partial = PartialScan {
                root_trusted: true,
                root_reachable: root_listed && volume.usable(),
                withdrawal_allowed: false,
                missing: Vec::new(),
            };
            self.library.finish_scan(
                &source.id,
                &scan_id,
                false,
                Some(&source.root_path),
                &partial,
            )?;
            if !volume.usable() {
                self.library
                    .mark_source_unreachable(&source.id, SourceState::Disconnected)?;
            }
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
            complete: false,
        })
    }

    fn source_for_root(&self, root: &Path) -> Result<crate::domain::Source> {
        let key = self.library.path_key(root);
        self.library
            .sources()?
            .into_iter()
            .find(|source| self.library.path_key(Path::new(&source.root_path)) == key)
            .ok_or_else(|| {
                CoreError::library("source_missing", "That folder is not a library source.")
            })
    }

    fn settle_volume(
        &self,
        source_id: &str,
        volume: VolumeCheck,
        current: &Option<VolumeIdentity>,
        imported: u64,
    ) -> Result<VolumeCheck> {
        if imported == 0 || volume == VolumeCheck::Matched {
            return Ok(volume);
        }
        let Some(identity) = current else {
            return Ok(volume);
        };
        let adopt = match volume {
            VolumeCheck::Unknown => true,
            VolumeCheck::Mismatch | VolumeCheck::Unreadable => self.full_fingerprint,
            VolumeCheck::Matched => false,
        };
        if adopt {
            self.library.record_volume_identity(source_id, identity)?;
            return Ok(VolumeCheck::Matched);
        }
        Ok(volume)
    }

    fn prepare(
        &self,
        root: &Path,
        relative: &str,
        facts: &LocatorFacts,
        token: &ScanToken,
    ) -> Result<PreparedFile> {
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
        let modified_ms = millis(metadata.modified().ok());
        let stored = facts.get(relative);
        let reusable = !self.full_fingerprint
            && stored.is_some_and(|stored| {
                stored.size == size as i64
                    && stored.modified_ms == modified_ms
                    && !stored.fingerprint.is_empty()
            });
        let (fingerprint, modified_ms) = if reusable {
            (
                stored
                    .map(|stored| stored.fingerprint.clone())
                    .unwrap_or_default(),
                modified_ms,
            )
        } else {
            #[cfg(test)]
            read_observer::notify();
            let fingerprint = media_fingerprint(&path, size)?;
            let after = std::fs::metadata(&path)?;
            if after.len() != size || after.modified().ok() != metadata.modified().ok() {
                return Err(CoreError::library(
                    "still_downloading",
                    "The file changed while it was being read.",
                ));
            }
            (fingerprint, millis(after.modified().ok()))
        };

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
        Ok(PreparedFile {
            relative_path: relative.to_string(),
            fingerprint,
            size: size as i64,
            modified_ms,
            parsed,
        })
    }

    fn apply(
        &self,
        source_id: &str,
        expected_root: &str,
        scan_id: &str,
        prepared: &[PreparedFile],
    ) -> Result<()> {
        let ingests: Vec<Ingest<'_>> = prepared
            .iter()
            .map(|file| Ingest {
                source_id,
                relative_path: &file.relative_path,
                fingerprint: &file.fingerprint,
                size: file.size,
                modified_ms: file.modified_ms,
                scan_id,
                parsed: Some(&file.parsed),
                expected_root: Some(expected_root),
            })
            .collect();
        self.library.ingest_files(&ingests)?;
        Ok(())
    }

    fn probe_partial_scan(
        &self,
        source_id: &str,
        scan_id: &str,
        root: &Path,
        complete_withdrawal: bool,
        withdrawal_allowed: bool,
        volume_usable: bool,
    ) -> Result<PartialScan> {
        let resolved = std::fs::canonicalize(root).ok();
        let root_trusted = resolved
            .as_ref()
            .is_some_and(|resolved| self.library.path_key(resolved) == self.library.path_key(root));
        let root_listed = root_trusted && volume_usable && std::fs::read_dir(root).is_ok();
        if complete_withdrawal {
            return Ok(PartialScan {
                root_trusted,
                root_reachable: root_listed,
                withdrawal_allowed,
                missing: Vec::new(),
            });
        }

        let mut missing = Vec::new();
        if root_trusted && withdrawal_allowed {
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
            root_reachable: root_listed,
            withdrawal_allowed,
            missing,
        })
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

fn withdrawal_allowed(volume: VolumeCheck, imported: u64, known: u64) -> bool {
    match volume {
        VolumeCheck::Matched => true,
        VolumeCheck::Unknown => imported > 0 || known == 0,
        VolumeCheck::Mismatch | VolumeCheck::Unreadable => false,
    }
}

fn reached_through_symlink(root: &Path, relative: &str) -> bool {
    let mut current = root.to_path_buf();
    let path = Path::new(relative);
    let mut ancestors: Vec<&std::ffi::OsStr> = path
        .parent()
        .map(|parent| parent.iter().collect())
        .unwrap_or_default();
    ancestors.retain(|part| !part.is_empty());
    for part in ancestors {
        current.push(part);
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    return true;
                }
            }
            Err(_) => return true,
        }
    }
    false
}

fn relative_to(root: &Path, path: &Path) -> Option<String> {
    let candidate = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    let normalized = crate::watch::normalize(&candidate)?;
    let root = crate::watch::normalize(root)?;
    let relative = normalized.strip_prefix(&root).ok()?;
    if relative.as_os_str().is_empty() {
        return None;
    }
    relative.to_str().map(str::to_string)
}

fn millis(time: Option<SystemTime>) -> i64 {
    time.and_then(|time| time.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or_default()
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
pub(crate) mod read_observer {
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

    fn counter_on_this_thread() -> (Arc<AtomicUsize>, impl Fn() + Send + Sync) {
        let counter = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&counter);
        let thread = std::thread::current().id();
        (counter, move || {
            if std::thread::current().id() == thread {
                observed.fetch_add(1, Ordering::SeqCst);
            }
        })
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

    #[test]
    fn unchanged_files_are_not_read_again() {
        let directory = TempDir::new().unwrap();
        let root = directory.path();
        write_settled(&root.join("One.2020.mkv"), b"one");
        write_settled(&root.join("Two.2020.mkv"), b"two");
        write_settled(&root.join("Three.2020.mkv"), b"three");

        let library = Library::open_in_memory().unwrap();
        let source = library.add_source(root, None).unwrap();
        let scanner =
            LibraryScanner::new(&library).with_limits(10_000, 32, Duration::from_secs(15));

        let (reads, observe) = counter_on_this_thread();
        read_observer::clear();
        read_observer::install(Arc::new(observe));
        let first = scanner.scan(&source.id, &ScanToken::new(), |_| {}).unwrap();
        assert_eq!(first.imported, 3);
        assert_eq!(
            reads.load(Ordering::SeqCst),
            3,
            "the first scan must fingerprint every file"
        );
        assert!(first.complete, "issues: {:?}", first.issues);

        reads.store(0, Ordering::SeqCst);
        let second = scanner.scan(&source.id, &ScanToken::new(), |_| {}).unwrap();
        assert_eq!(second.imported, 3);
        assert_eq!(
            reads.load(Ordering::SeqCst),
            0,
            "an unchanged rescan re-read three files"
        );

        reads.store(0, Ordering::SeqCst);
        write_settled(&root.join("Two.2020.mkv"), b"two, revised and longer");
        let third = scanner.scan(&source.id, &ScanToken::new(), |_| {}).unwrap();
        assert_eq!(third.imported, 3);
        assert_eq!(
            reads.load(Ordering::SeqCst),
            1,
            "only the changed file may be read again"
        );
        read_observer::clear();
    }

    #[test]
    fn a_long_scan_publishes_what_it_has_before_it_finishes() {
        let directory = TempDir::new().unwrap();
        let root = directory.path();
        let total = INGEST_CHUNK * 2 + 4;
        for index in 0..total {
            write_settled(&root.join(format!("Bulk.{index:05}.2020.mkv")), b"data");
        }

        let library = Library::open_in_memory().unwrap();
        let source = library.add_source(root, None).unwrap();
        let scanner =
            LibraryScanner::new(&library).with_limits(10_000, 32, Duration::from_secs(15));

        let _observer = crate::library::commit_observer::test_guard();
        let seen = Arc::new(AtomicUsize::new(0));
        let committed = Arc::clone(&seen);
        crate::library::commit_observer::install(Arc::new(move || {
            committed.fetch_add(1, Ordering::SeqCst);
        }));

        let summary = scanner.scan(&source.id, &ScanToken::new(), |_| {}).unwrap();
        crate::library::commit_observer::clear();

        assert_eq!(summary.imported as usize, total);
        assert!(
            seen.load(Ordering::SeqCst) >= 3,
            "a walk longer than one chunk must publish its rows before it ends"
        );
        assert!(
            seen.load(Ordering::SeqCst) < total,
            "and must not fall back to one transaction per file"
        );
    }

    #[test]
    fn a_scan_commits_once_for_a_batch_of_files_however_many_arrive() {
        let directory = TempDir::new().unwrap();
        let root = directory.path();
        for index in 0..8 {
            write_settled(&root.join(format!("First.{index:02}.2020.mkv")), b"data");
        }

        let library = Library::open_in_memory().unwrap();
        let source = library.add_source(root, None).unwrap();
        let scanner =
            LibraryScanner::new(&library).with_limits(10_000, 32, Duration::from_secs(15));

        let _observer = crate::library::commit_observer::test_guard();
        let commits = Arc::new(AtomicUsize::new(0));
        let thread = std::thread::current().id();
        let counted = Arc::clone(&commits);
        crate::library::commit_observer::install(Arc::new(move || {
            if std::thread::current().id() == thread {
                counted.fetch_add(1, Ordering::SeqCst);
            }
        }));

        let first = scanner.scan(&source.id, &ScanToken::new(), |_| {}).unwrap();
        assert_eq!(first.imported, 8);
        let eight = commits.swap(0, Ordering::SeqCst);

        for index in 0..32 {
            write_settled(&root.join(format!("Second.{index:02}.2020.mkv")), b"data");
        }
        let second = scanner.scan(&source.id, &ScanToken::new(), |_| {}).unwrap();
        assert_eq!(second.imported, 40);
        let forty = commits.swap(0, Ordering::SeqCst);
        crate::library::commit_observer::clear();

        assert_eq!(
            eight, forty,
            "the transaction count grew with the file count: {eight} for 8 files, {forty} for 40"
        );
        assert_eq!(eight, 1, "8 files were ingested in {eight} transactions");
        assert_eq!(forty, 1, "40 files were ingested in {forty} transactions");
    }

    #[test]
    fn a_stale_empty_mount_point_withdraws_nothing() {
        let directory = TempDir::new().unwrap();
        let root = directory.path().join("drive");
        fs::create_dir_all(&root).unwrap();
        write_settled(&root.join("Film.2020.mkv"), b"film");
        write_settled(&root.join("Show.S01E01.mkv"), b"episode");

        let library = Library::open_in_memory().unwrap();
        let source = library.add_source(&root, None).unwrap();
        let scanner =
            LibraryScanner::new(&library).with_limits(10_000, 32, Duration::from_secs(15));
        scanner.scan(&source.id, &ScanToken::new(), |_| {}).unwrap();
        assert_eq!(library.count(&LibraryQuery::default()).unwrap(), 2);

        library
            .record_volume_identity(
                &source.id,
                &VolumeIdentity::new(9_999, Some("the-drive-that-left".into())),
            )
            .unwrap();

        fs::remove_file(root.join("Film.2020.mkv")).unwrap();
        fs::remove_file(root.join("Show.S01E01.mkv")).unwrap();

        let summary = scanner.scan(&source.id, &ScanToken::new(), |_| {}).unwrap();

        assert!(
            summary
                .issues
                .iter()
                .any(|issue| issue.code == "volume_mismatch"),
            "the walk did not report the replaced volume: {:?}",
            summary.issues
        );
        assert!(!summary.complete);
        assert_eq!(
            library
                .items(&LibraryQuery::page(10))
                .unwrap()
                .iter()
                .filter(|item| item.available)
                .count(),
            2,
            "an empty walk of a different volume withdrew the library"
        );
        assert_eq!(library.count(&LibraryQuery::default()).unwrap(), 2);
        assert_eq!(
            library.source(&source.id).unwrap().state,
            SourceState::Disconnected
        );
    }

    #[test]
    fn a_volume_that_cannot_be_identified_never_withdraws() {
        assert!(withdrawal_allowed(VolumeCheck::Matched, 0, 10));
        assert!(!withdrawal_allowed(VolumeCheck::Unreadable, 10, 10));
        assert!(!withdrawal_allowed(VolumeCheck::Mismatch, 10, 10));
        assert!(withdrawal_allowed(VolumeCheck::Unknown, 1, 10));
        assert!(!withdrawal_allowed(VolumeCheck::Unknown, 0, 10));
        assert!(withdrawal_allowed(VolumeCheck::Unknown, 0, 0));
    }

    #[test]
    fn a_volume_check_names_the_two_ways_identity_can_fail() {
        let stored = VolumeIdentity::new(1, Some("abc".into()));
        assert_eq!(
            VolumeCheck::classify(Some(stored.clone()), Some(stored.clone())),
            VolumeCheck::Matched
        );
        assert_eq!(
            VolumeCheck::classify(
                Some(stored.clone()),
                Some(VolumeIdentity::new(2, Some("d".into())))
            ),
            VolumeCheck::Mismatch
        );
        assert_eq!(
            VolumeCheck::classify(Some(stored), None),
            VolumeCheck::Unreadable
        );
        assert_eq!(VolumeCheck::classify(None, None), VolumeCheck::Unknown);
    }

    #[test]
    fn the_first_scan_of_a_source_without_a_volume_identity_adopts_one() {
        let directory = TempDir::new().unwrap();
        let root = directory.path().join("media");
        fs::create_dir_all(&root).unwrap();
        write_settled(&root.join("Film.2020.mkv"), b"film");
        let database = directory.path().join("library.db");

        let source_id = {
            let library = Library::open(&database).unwrap();
            let source = library.add_source(&root, None).unwrap();
            source.id
        };
        {
            let connection = rusqlite::Connection::open(&database).unwrap();
            connection
                .execute("UPDATE sources SET volume_identity = NULL", [])
                .unwrap();
        }

        let library = Library::open(&database).unwrap();
        assert_eq!(library.source_volume_identity(&source_id).unwrap(), None);
        LibraryScanner::new(&library)
            .with_limits(10_000, 32, Duration::from_secs(15))
            .scan(&source_id, &ScanToken::new(), |_| {})
            .unwrap();

        assert!(
            library
                .source_volume_identity(&source_id)
                .unwrap()
                .is_some(),
            "a verified walk left the source without a volume identity"
        );
    }
}
