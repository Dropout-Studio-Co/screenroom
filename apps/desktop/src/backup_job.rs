use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::JoinHandle;

use screenroom_core::{BackupCancel, BackupReport, CoreError, Library, RestoreReport};

pub struct BackupJob {
    destination: PathBuf,
    written: Arc<AtomicU64>,
    cancel: BackupCancel,
    reported: u64,
    handle: Option<JoinHandle<Result<BackupReport, CoreError>>>,
    outcome: Option<Result<BackupReport, CoreError>>,
}

impl std::fmt::Debug for BackupJob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BackupJob")
            .field("destination", &self.destination)
            .field("written", &self.written())
            .field("finished", &self.outcome.is_some())
            .finish_non_exhaustive()
    }
}

impl BackupJob {
    pub fn start(library: Arc<Library>, destination: PathBuf) -> Self {
        let written = Arc::new(AtomicU64::new(0));
        let cancel = BackupCancel::new();
        let reporter = Arc::clone(&written);
        let stopping = cancel.clone();
        let target = destination.clone();
        let handle = std::thread::spawn(move || {
            let progress = move |report: screenroom_core::BackupProgress| {
                reporter.store(report.bytes, Ordering::Relaxed);
            };
            library.backup_to_with(&target, &progress, &stopping)
        });
        Self {
            destination,
            written,
            cancel,
            reported: 0,
            handle: Some(handle),
            outcome: None,
        }
    }

    pub fn destination(&self) -> &Path {
        &self.destination
    }

    pub fn written(&self) -> u64 {
        self.written.load(Ordering::Relaxed)
    }

    pub fn cancel(&self) {
        self.cancel.cancel();
    }

    pub fn take_progress(&mut self) -> Option<u64> {
        let written = self.written();
        if written == self.reported {
            return None;
        }
        self.reported = written;
        Some(written)
    }

    pub fn outcome(&mut self) -> Option<&Result<BackupReport, CoreError>> {
        if self.outcome.is_none()
            && let Some(handle) = self.handle.take()
        {
            let result = handle.join().unwrap_or_else(|_| {
                Err(CoreError::library(
                    "backup_failed",
                    "The backup thread stopped before it finished.",
                ))
            });
            self.outcome = Some(result);
        }
        self.outcome.as_ref()
    }

    pub fn take_outcome(&mut self) -> Option<Result<BackupReport, CoreError>> {
        self.outcome()?;
        self.outcome.take()
    }

    pub fn finished(&self) -> bool {
        self.outcome.is_some()
    }
}

pub struct RestoreJob {
    source: PathBuf,
    handle: Option<JoinHandle<Result<RestoreReport, CoreError>>>,
    outcome: Option<Result<RestoreReport, CoreError>>,
}

impl std::fmt::Debug for RestoreJob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RestoreJob")
            .field("source", &self.source)
            .field("finished", &self.outcome.is_some())
            .finish_non_exhaustive()
    }
}

impl RestoreJob {
    pub fn start(library: Arc<Library>, source: PathBuf) -> Self {
        let origin = source.clone();
        let handle = std::thread::spawn(move || library.restore_from(origin));
        Self {
            source,
            handle: Some(handle),
            outcome: None,
        }
    }

    pub fn source(&self) -> &Path {
        &self.source
    }

    pub fn outcome(&mut self) -> Option<&Result<RestoreReport, CoreError>> {
        if self.outcome.is_none()
            && let Some(handle) = self.handle.take()
        {
            let result = handle.join().unwrap_or_else(|_| {
                Err(CoreError::library(
                    "restore_failed",
                    "The restore thread stopped before it finished.",
                ))
            });
            self.outcome = Some(result);
        }
        self.outcome.as_ref()
    }

    pub fn take_outcome(&mut self) -> Option<Result<RestoreReport, CoreError>> {
        self.outcome()?;
        self.outcome.take()
    }

    pub fn finished(&self) -> bool {
        self.outcome.is_some()
    }
}

pub fn megabytes(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_progress_reading_is_only_reported_once_per_change() {
        let written = Arc::new(AtomicU64::new(0));
        let mut job = BackupJob {
            destination: PathBuf::from("/tmp/example.db"),
            written: Arc::clone(&written),
            cancel: BackupCancel::new(),
            reported: 0,
            handle: None,
            outcome: None,
        };
        assert_eq!(job.take_progress(), None);
        written.store(4_096, Ordering::Relaxed);
        assert_eq!(job.take_progress(), Some(4_096));
        assert_eq!(job.take_progress(), None);
        written.store(8_192, Ordering::Relaxed);
        assert_eq!(job.take_progress(), Some(8_192));
    }

    #[test]
    fn bytes_are_reported_in_megabytes() {
        assert_eq!(megabytes(0), "0.0 MB");
        assert_eq!(megabytes(1024 * 1024), "1.0 MB");
        assert_eq!(megabytes(3 * 1024 * 1024 / 2), "1.5 MB");
    }

    #[test]
    fn a_job_that_was_never_started_reports_nothing_rather_than_hanging() {
        let mut job = BackupJob {
            destination: PathBuf::from("/tmp/example.db"),
            written: Arc::new(AtomicU64::new(0)),
            cancel: BackupCancel::new(),
            reported: 0,
            handle: None,
            outcome: None,
        };
        assert!(!job.finished());
        assert!(job.take_progress().is_none());
    }
}
