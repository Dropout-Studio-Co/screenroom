use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::error::{CoreError, Result};
use crate::{BACKUP_FORMAT_VERSION, SCHEMA_VERSION};

use super::{Library, new_id, now_ms};

const PROGRESS_POLL: Duration = Duration::from_millis(50);

const COPY_BUSY_TIMEOUT_MS: i64 = 250;

const COPY_RETRY_ATTEMPTS: u32 = 20;

const COPY_RETRY_BACKOFF: Duration = Duration::from_millis(100);

const COPY_RETRY_BACKOFF_MAX: Duration = Duration::from_millis(1_000);

const WRITER_BUSY_TIMEOUT_MS: i64 = 120_000;

const STALE_PARTIAL_AGE: Duration = Duration::from_secs(3_600);

const TABLES: &[&str] = &[
    "sources",
    "works",
    "seasons",
    "episodes",
    "editions",
    "files",
    "subtitles",
    "artwork",
    "file_episodes",
    "profiles",
    "sessions",
    "progress",
    "watchlist",
    "overrides",
    "markers",
    "external_identities",
    "jobs",
    "conversion_jobs",
    "settings",
    "enrichment_queue",
    "source_bookmarks",
];

const FORMAT_KEY: &str = "backupFormatVersion";

const CREATED_KEY: &str = "backupCreatedMs";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupInfo {
    pub path: String,

    pub format_version: i64,

    pub schema_version: i64,

    pub created_ms: Option<i64>,

    pub sources: i64,

    pub works: i64,

    pub files: i64,

    pub profiles: i64,
}

impl std::fmt::Display for BackupInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} works, {} files, {} folders, {} profiles (schema {}, format {})",
            self.works,
            self.files,
            self.sources,
            self.profiles,
            self.schema_version,
            self.format_version
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupReport {
    pub path: String,

    pub bytes: u64,

    pub info: BackupInfo,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RestoreReport {
    pub restored_from: String,

    pub previous_saved_to: Option<String>,

    pub info: BackupInfo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackupProgress {
    pub bytes: u64,
}

pub type BackupProgressFn<'a> = dyn Fn(BackupProgress) + 'a;

#[derive(Debug, Clone, Default)]
pub struct BackupCancel {
    cancelled: Arc<AtomicBool>,
}

impl BackupCancel {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

impl Library {
    pub fn backup_to(&self, path: impl AsRef<Path>) -> Result<BackupReport> {
        self.backup_to_with(path, &|_| {}, &BackupCancel::new())
    }

    pub fn backup_to_with(
        &self,
        path: impl AsRef<Path>,
        on_progress: &BackupProgressFn<'_>,
        cancel: &BackupCancel,
    ) -> Result<BackupReport> {
        let destination = absolute(path.as_ref(), "backup")?;
        if let Some(live) = &self.path
            && same_file(live, &destination)
        {
            return Err(CoreError::library(
                "backup_is_library",
                "Choose a different file: that path is the library itself.",
            ));
        }

        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent)?;
        }
        sweep_stale_partials(&destination);
        let staging = staging_path(&destination)?;

        let _ = std::fs::remove_file(&staging);

        let _writer_grace = WriterGrace::raise(self)?;

        let outcome = (|| -> Result<()> {
            if cancel.is_cancelled() {
                return Err(cancelled());
            }
            match &self.path {
                Some(live) => copy_into(live, &staging, on_progress, cancel)?,
                None => {
                    let connection = self.connection();
                    connection
                        .execute("VACUUM INTO ?1", [staging.to_string_lossy().to_string()])?;
                }
            }
            if cancel.is_cancelled() {
                return Err(cancelled());
            }
            stamp(&staging)?;
            Ok(())
        })();

        match outcome {
            Ok(()) if cancel.is_cancelled() => {
                let _ = std::fs::remove_file(&staging);
                return Err(cancelled());
            }
            Ok(()) => {}
            Err(error) => {
                let _ = std::fs::remove_file(&staging);
                return Err(error);
            }
        }

        std::fs::rename(&staging, &destination).map_err(|error| {
            let _ = std::fs::remove_file(&staging);
            CoreError::Io(error)
        })?;

        let bytes = std::fs::metadata(&destination)?.len();
        let info = inspect(&destination)?;
        Ok(BackupReport {
            path: destination.to_string_lossy().to_string(),
            bytes,
            info,
        })
    }

    pub fn inspect_backup(path: impl AsRef<Path>) -> Result<BackupInfo> {
        inspect(path.as_ref())
    }

    pub fn restore_from(&self, path: impl AsRef<Path>) -> Result<RestoreReport> {
        let source = path.as_ref();
        if let Some(live) = &self.path
            && same_file(live, source)
        {
            return Err(CoreError::library(
                "restore_is_library",
                "That file is the library itself, so there is nothing to restore from it.",
            ));
        }
        let info = inspect(source)?;
        self.check_candidate_tables(source)?;

        let previous_saved_to = match &self.path {
            Some(live) => {
                let safety = safety_path(live);
                let report = self.backup_to(&safety)?;
                Some(report.path)
            }
            None => None,
        };

        let mut connection = self.connection();

        let _ = connection.execute_batch("DETACH DATABASE backup;");

        connection.execute(
            "ATTACH DATABASE ?1 AS backup",
            [source.to_string_lossy().to_string()],
        )?;
        let outcome = replace_contents(&mut connection);
        let detached = connection.execute_batch("DETACH DATABASE backup;");

        outcome?;
        detached?;
        drop(connection);

        Ok(RestoreReport {
            restored_from: source.to_string_lossy().to_string(),
            previous_saved_to,
            info,
        })
    }

    pub fn path(&self) -> Option<PathBuf> {
        self.path.clone()
    }

    fn check_candidate_tables(&self, source: &Path) -> Result<()> {
        let expected: Vec<(&'static str, Vec<String>)> = {
            let connection = self.connection();
            let mut expected = Vec::with_capacity(TABLES.len());
            for table in TABLES {
                expected.push((*table, columns(&connection, table)?));
            }
            expected
        };

        let candidate = Connection::open_with_flags(
            source,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|_| unreadable())?;

        for (table, live_columns) in expected {
            if live_columns.is_empty() {
                continue;
            }
            let candidate_columns = columns(&candidate, table)?;
            if candidate_columns.is_empty() {
                continue;
            }
            if candidate_columns != live_columns {
                return Err(CoreError::library(
                    "backup_unsupported",
                    "That backup's tables do not match this library, so it was not restored.",
                ));
            }
        }
        Ok(())
    }
}

fn replace_contents(connection: &mut Connection) -> Result<()> {
    let tx = connection.transaction()?;
    tx.execute_batch("PRAGMA defer_foreign_keys = ON;")?;
    let live = table_names(&tx, "main")?;
    for table in TABLES.iter().rev() {
        if !live.contains(*table) {
            continue;
        }
        tx.execute(&format!("DELETE FROM main.{table}"), [])?;
    }

    let present = table_names(&tx, "backup")?;
    for table in TABLES {
        if !present.contains(*table) {
            continue;
        }
        tx.execute(
            &format!("INSERT INTO main.{table} SELECT * FROM backup.{table}"),
            [],
        )?;
    }

    tx.execute("DELETE FROM main.settings WHERE key = ?1", [FORMAT_KEY])?;
    tx.execute("DELETE FROM main.settings WHERE key = ?1", [CREATED_KEY])?;
    reset_interrupted_work(&tx)?;
    tx.commit()?;
    Ok(())
}

fn reset_interrupted_work(tx: &rusqlite::Transaction<'_>) -> Result<()> {
    tx.execute("UPDATE main.sessions SET status = 'closed'", [])?;
    tx.execute(
        "UPDATE main.jobs SET status = 'queued' WHERE status = 'running'",
        [],
    )?;
    tx.execute(
        "UPDATE main.enrichment_queue SET state = 'queued' WHERE state = 'running'",
        [],
    )?;
    tx.execute(
        "UPDATE main.conversion_jobs
         SET status = 'failed', message = 'The app closed before this finished.',
             updated_ms = ?1
         WHERE status IN ('queued','running','verifying')",
        [now_ms()],
    )?;
    Ok(())
}

fn table_names(
    tx: &rusqlite::Transaction<'_>,
    schema: &str,
) -> Result<std::collections::HashSet<String>> {
    let mut statement = tx.prepare(&format!(
        "SELECT name FROM {schema}.sqlite_master WHERE type = 'table'"
    ))?;
    let names = statement.query_map([], |row| row.get::<_, String>(0))?;
    Ok(names.collect::<rusqlite::Result<_>>()?)
}

fn copy_into(
    live: &Path,
    staging: &Path,
    on_progress: &BackupProgressFn<'_>,
    cancel: &BackupCancel,
) -> Result<()> {
    let mut backoff = COPY_RETRY_BACKOFF;
    let mut attempt = 1;
    loop {
        match copy_once(live, staging, on_progress, cancel) {
            Ok(()) => return Ok(()),
            Err(error) => {
                let _ = std::fs::remove_file(staging);
                let retryable = lock_contention(&error)
                    && !cancel.is_cancelled()
                    && attempt < COPY_RETRY_ATTEMPTS;
                if !retryable {
                    return Err(error);
                }
            }
        }
        attempt += 1;
        std::thread::sleep(backoff);
        backoff = backoff.saturating_mul(2).min(COPY_RETRY_BACKOFF_MAX);
    }
}

fn copy_once(
    live: &Path,
    staging: &Path,
    on_progress: &BackupProgressFn<'_>,
    cancel: &BackupCancel,
) -> Result<()> {
    let connection = Connection::open_with_flags(
        live,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(unreadable_source)?;
    connection.execute_batch(&format!("PRAGMA busy_timeout = {COPY_BUSY_TIMEOUT_MS};"))?;
    let interrupt = connection.get_interrupt_handle();
    let target = staging.to_string_lossy().to_string();

    let (outcome, reported) = std::thread::scope(|scope| {
        let worker = scope.spawn(move || connection.execute("VACUUM INTO ?1", [target]));
        let mut reported: Option<u64> = None;
        loop {
            if cancel.is_cancelled() {
                interrupt.interrupt();
            }
            let bytes = written_bytes(staging);
            if reported != Some(bytes) {
                reported = Some(bytes);
                on_progress(BackupProgress { bytes });
            }
            if worker.is_finished() {
                break;
            }
            std::thread::sleep(PROGRESS_POLL);
        }
        (worker.join(), reported)
    });

    match outcome {
        Ok(Ok(_)) => {
            let bytes = written_bytes(staging);
            if reported != Some(bytes) {
                on_progress(BackupProgress { bytes });
            }
            Ok(())
        }
        Ok(Err(error)) => Err(CoreError::Sqlite(error)),
        Err(_) => Err(CoreError::library(
            "backup_failed",
            "The backup stopped unexpectedly. Nothing was written.",
        )),
    }
}

struct WriterGrace<'a> {
    library: &'a Library,
    previous_ms: i64,
}

impl<'a> WriterGrace<'a> {
    fn raise(library: &'a Library) -> Result<Self> {
        let previous_ms: i64 = {
            let connection = library.connection();
            connection.query_row("PRAGMA busy_timeout", [], |row| row.get(0))?
        };
        {
            let connection = library.connection();
            connection
                .execute_batch(&format!("PRAGMA busy_timeout = {WRITER_BUSY_TIMEOUT_MS};"))?;
        }
        Ok(Self {
            library,
            previous_ms,
        })
    }
}

impl Drop for WriterGrace<'_> {
    fn drop(&mut self) {
        let connection = self.library.connection();
        let _ = connection.execute_batch(&format!("PRAGMA busy_timeout = {};", self.previous_ms));
    }
}

fn busy(error: &rusqlite::Error) -> bool {
    matches!(
        error,
        rusqlite::Error::SqliteFailure(code, _)
            if code.code == rusqlite::ErrorCode::DatabaseBusy
                || code.code == rusqlite::ErrorCode::DatabaseLocked
    )
}

fn lock_contention(error: &CoreError) -> bool {
    match error {
        CoreError::Sqlite(inner) => busy(inner),
        _ => false,
    }
}

fn unreadable_source(error: rusqlite::Error) -> CoreError {
    if busy(&error) {
        return CoreError::Sqlite(error);
    }
    CoreError::library(
        "backup_source_unreadable",
        "The library file cannot be read, so no backup was written.",
    )
}

fn written_bytes(path: &Path) -> u64 {
    std::fs::metadata(path)
        .map(|metadata| metadata.len())
        .unwrap_or_default()
}

fn cancelled() -> CoreError {
    CoreError::library(
        "backup_cancelled",
        "The backup was cancelled. Nothing was written.",
    )
}

fn columns(connection: &Connection, table: &str) -> Result<Vec<String>> {
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let names = statement
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(names)
}

fn stamp(path: &Path) -> Result<()> {
    let connection = Connection::open(path)?;
    connection.execute_batch(&format!(
        "INSERT INTO settings(key, value) VALUES('{FORMAT_KEY}', '{BACKUP_FORMAT_VERSION}')
           ON CONFLICT(key) DO UPDATE SET value = excluded.value;
         INSERT INTO settings(key, value) VALUES('{CREATED_KEY}', '{}')
           ON CONFLICT(key) DO UPDATE SET value = excluded.value;",
        now_ms()
    ))?;
    Ok(())
}

fn inspect(path: &Path) -> Result<BackupInfo> {
    if !path.is_absolute() {
        return Err(CoreError::library(
            "invalid_backup_path",
            "Choose an absolute backup path.",
        ));
    }
    if !path.is_file() {
        return Err(CoreError::library(
            "backup_missing",
            "That backup file is not available.",
        ));
    }

    let connection = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|_| unreadable())?;

    let integrity: String = connection
        .query_row("PRAGMA quick_check", [], |row| row.get::<_, String>(0))
        .map_err(|_| unreadable())?;
    if integrity != "ok" {
        return Err(CoreError::library(
            "backup_corrupt",
            "That backup failed its integrity check.",
        ));
    }

    let schema_version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
        .map_err(|_| unreadable())?;
    if !(2..=SCHEMA_VERSION).contains(&schema_version) {
        return Err(CoreError::library(
            "backup_unsupported",
            "That backup was written by an incompatible Screenroom version.",
        ));
    }

    let format_version = setting(&connection, FORMAT_KEY)
        .and_then(|value| value.parse::<i64>().ok())
        .ok_or_else(|| {
            CoreError::library(
                "backup_unrecognised",
                "That file is a SQLite database but not a Screenroom backup.",
            )
        })?;
    if format_version > BACKUP_FORMAT_VERSION {
        return Err(CoreError::library(
            "backup_unsupported",
            "That backup is newer than this Screenroom can restore.",
        ));
    }

    let created_ms = setting(&connection, CREATED_KEY).and_then(|value| value.parse::<i64>().ok());

    Ok(BackupInfo {
        path: path.to_string_lossy().to_string(),
        format_version,
        schema_version,
        created_ms,
        sources: count(&connection, "sources")?,
        works: count(&connection, "works")?,
        files: count(&connection, "files")?,
        profiles: count(&connection, "profiles")?,
    })
}

fn unreadable() -> CoreError {
    CoreError::library("backup_unreadable", "That file is not a Screenroom backup.")
}

fn setting(connection: &Connection, key: &str) -> Option<String> {
    connection
        .query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| {
            row.get(0)
        })
        .optional()
        .ok()
        .flatten()
}

fn count(connection: &Connection, table: &str) -> Result<i64> {
    Ok(
        connection.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
            row.get(0)
        })?,
    )
}

fn absolute(path: &Path, what: &'static str) -> Result<PathBuf> {
    if !path.is_absolute() {
        return Err(CoreError::library(
            "invalid_backup_path",
            format!("Choose an absolute path for the {what}."),
        ));
    }
    if path.is_dir() {
        return Err(CoreError::library(
            "backup_is_folder",
            "Choose a file name, not a folder.",
        ));
    }
    Ok(path.to_path_buf())
}

fn staging_path(destination: &Path) -> Result<PathBuf> {
    let parent = destination.parent().ok_or_else(|| {
        CoreError::library("invalid_backup_path", "Choose an absolute backup path.")
    })?;
    Ok(parent.join(format!(
        "{}{}.partial",
        partial_prefix(destination),
        new_id()
    )))
}

fn partial_prefix(destination: &Path) -> String {
    let name = destination
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "library".to_string());
    format!(".{name}.")
}

fn sweep_stale_partials(destination: &Path) {
    let Some(parent) = destination.parent() else {
        return;
    };
    let prefix = partial_prefix(destination);
    let Ok(entries) = std::fs::read_dir(parent) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.starts_with(&prefix) || !name.ends_with(".partial") {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let Ok(modified) = metadata.modified() else {
            continue;
        };
        let stale = modified
            .elapsed()
            .map(|age| age >= STALE_PARTIAL_AGE)
            .unwrap_or(false);
        if stale {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

fn safety_path(library: &Path) -> PathBuf {
    let name = library
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "library".to_string());
    library.with_file_name(format!("{name}.{}.before-restore", new_id()))
}

fn same_file(left: &Path, right: &Path) -> bool {
    match (std::fs::canonicalize(left), std::fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;
    use std::sync::mpsc;

    use super::*;
    use crate::{Ingest, LibraryQuery, parse_filename};

    fn library_on_disk(directory: &tempfile::TempDir) -> (Library, PathBuf) {
        let media = directory.path().join("media");
        std::fs::create_dir_all(&media).unwrap();
        let path = directory.path().join("library.db");
        let library = Library::open(&path).unwrap();
        let source = library.add_source(&media, None).unwrap();
        for name in ["Heat (1995).mkv", "Alien (1979).mkv"] {
            std::fs::write(media.join(name), b"not really a video").unwrap();
            let parsed = parse_filename(name, None);
            library
                .ingest_file(Ingest {
                    source_id: &source.id,
                    relative_path: name,
                    fingerprint: name,
                    size: 18,
                    modified_ms: 1,
                    scan_id: "test",
                    parsed: Some(&parsed),
                    expected_root: Some(&source.root_path),
                })
                .unwrap();
        }
        (library, path)
    }

    #[test]
    fn a_backup_leaves_the_library_free_for_other_readers_while_it_copies() {
        let directory = tempfile::tempdir().unwrap();
        let (library, _path) = library_on_disk(&directory);
        let library = Arc::new(library);
        let destination = directory.path().join("copy.db");
        let cancel = BackupCancel::new();
        let observed = Arc::new(AtomicBool::new(false));
        let reads = Arc::new(AtomicUsize::new(0));

        let report = library
            .backup_to_with(
                &destination,
                &|_| {
                    let (done_tx, done_rx) = mpsc::channel();
                    let reader = Arc::clone(&library);
                    std::thread::spawn(move || {
                        let _ = done_tx.send(reader.count(&LibraryQuery::default()).is_ok());
                    });
                    if done_rx
                        .recv_timeout(Duration::from_secs(5))
                        .unwrap_or(false)
                    {
                        observed.store(true, Ordering::SeqCst);
                        reads.fetch_add(1, Ordering::SeqCst);
                    }
                },
                &cancel,
            )
            .expect("backup");

        assert_eq!(report.info.files, 2);
        assert!(
            observed.load(Ordering::SeqCst) && reads.load(Ordering::SeqCst) > 0,
            "a library read was blocked while the backup was copying"
        );
        assert!(destination.is_file());
    }

    #[test]
    fn a_cancelled_backup_writes_nothing_that_could_pass_for_a_backup() {
        let directory = tempfile::tempdir().unwrap();
        let (library, _path) = library_on_disk(&directory);
        let destination = directory.path().join("copy.db");
        let cancel = BackupCancel::new();
        let seen = Arc::new(AtomicBool::new(false));

        let error = {
            let progress_cancel = cancel.clone();
            let seen = Arc::clone(&seen);
            library
                .backup_to_with(
                    &destination,
                    &move |_| {
                        seen.store(true, Ordering::SeqCst);
                        progress_cancel.cancel();
                    },
                    &cancel,
                )
                .unwrap_err()
        };

        assert_eq!(error.code(), "backup_cancelled");
        assert!(
            seen.load(Ordering::SeqCst),
            "the backup never reported its progress"
        );
        assert!(!destination.exists(), "a cancelled backup left its output");
        assert_eq!(
            Library::inspect_backup(&destination).unwrap_err().code(),
            "backup_missing"
        );
        let leftovers: Vec<String> = std::fs::read_dir(directory.path())
            .unwrap()
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().to_string())
            .filter(|name| name.ends_with(".partial"))
            .collect();
        assert!(leftovers.is_empty(), "left behind: {leftovers:?}");
    }

    #[test]
    fn a_restore_that_fails_after_it_starts_leaves_the_live_file_byte_identical() {
        let directory = tempfile::tempdir().unwrap();
        let (library, path) = library_on_disk(&directory);
        let backup = directory.path().join("copy.db");
        library.backup_to(&backup).expect("backup");

        {
            let connection = Connection::open(&path).unwrap();
            connection
                .execute_batch(
                    "CREATE TRIGGER refuse_restore BEFORE INSERT ON works
                     BEGIN SELECT RAISE(ABORT, 'test restore refusal'); END;",
                )
                .unwrap();
        }

        let before = std::fs::read(&path).unwrap();
        let error = library.restore_from(&backup).unwrap_err();
        assert_eq!(error.code(), "database_error");
        assert_eq!(
            std::fs::read(&path).unwrap(),
            before,
            "a failed restore changed the live library file"
        );
        assert_eq!(library.count(&LibraryQuery::default()).unwrap(), 2);
    }

    fn busy_timeout(library: &Library) -> i64 {
        let connection = library.connection();
        connection
            .query_row("PRAGMA busy_timeout", [], |row| row.get(0))
            .unwrap()
    }

    fn library_with_every_table(directory: &tempfile::TempDir) -> (Library, PathBuf) {
        let media = directory.path().join("media");
        std::fs::create_dir_all(&media).unwrap();
        let path = directory.path().join("library.db");
        let library = Library::open(&path).unwrap();
        let source = library.add_source(&media, None).unwrap();
        let name = "Heat (1995).mkv";
        std::fs::write(media.join(name), b"not really a video").unwrap();
        let parsed = parse_filename(name, None);
        let file_id = library
            .ingest_file(Ingest {
                source_id: &source.id,
                relative_path: name,
                fingerprint: name,
                size: 18,
                modified_ms: 1,
                scan_id: "test",
                parsed: Some(&parsed),
                expected_root: Some(&source.root_path),
            })
            .unwrap();
        let work_id = library.item(&file_id, None).unwrap().unwrap().work_id;
        let profile = library.profiles().unwrap().remove(0);
        let subtitle_file = directory.path().join("heat.en.srt");
        std::fs::write(&subtitle_file, b"1\n00:00:01,000 --> 00:00:02,000\nHello\n").unwrap();
        library.import_subtitle(&file_id, &subtitle_file).unwrap();
        library
            .add_bookmark(&crate::BookmarkDraft {
                label: "Streaming shelf".to_string(),
                address: "https://example.org/shelf".to_string(),
                access: String::new(),
            })
            .unwrap();
        library.set_setting("everyTableTest", "kept").unwrap();
        library
            .enqueue_enrichment(&work_id, crate::EnrichmentTarget::Work, "en-US", 0)
            .unwrap();

        {
            let connection = library.connection();
            let edition_id: String = connection
                .query_row(
                    "SELECT edition_id FROM files WHERE id = ?1",
                    [&file_id],
                    |row| row.get(0),
                )
                .unwrap();
            connection
                .execute(
                    "INSERT INTO seasons(id, work_id, number) VALUES('season-1', ?1, 1)",
                    [&work_id],
                )
                .unwrap();
            connection
                .execute(
                    "INSERT INTO episodes(id, season_id, number, title)
                     VALUES('episode-1', 'season-1', 1, 'Pilot')",
                    [],
                )
                .unwrap();
            connection
                .execute(
                    "INSERT INTO file_episodes(file_id, episode_id, sequence)
                     VALUES(?1, 'episode-1', 0)",
                    [&file_id],
                )
                .unwrap();
            connection
                .execute(
                    "INSERT INTO artwork(work_id, file_id, mime, width, height, sampled_at_ms,
                        created_ms, bytes)
                     VALUES(?1, ?2, 'image/jpeg', 16, 16, 1, 1, x'00ff')",
                    rusqlite::params![work_id, file_id],
                )
                .unwrap();
            connection
                .execute(
                    "INSERT INTO sessions(id, profile_id, file_id, edition_id, generation, status,
                        sequence, seek_generation)
                     VALUES('session-1', ?1, ?2, ?3, 1, 'active', 1, 0)",
                    rusqlite::params![profile.id, file_id, edition_id],
                )
                .unwrap();
            connection
                .execute(
                    "INSERT INTO progress(profile_id, edition_id, file_id, position_ms,
                        duration_ms, watched, hidden, updated_ms)
                     VALUES(?1, ?2, ?3, 1000, 60000, 0, 0, 1)",
                    rusqlite::params![profile.id, edition_id, file_id],
                )
                .unwrap();
            connection
                .execute(
                    "INSERT INTO watchlist(profile_id, work_id, favorite, watchlisted, created_ms)
                     VALUES(?1, ?2, 1, 1, 1)",
                    rusqlite::params![profile.id, work_id],
                )
                .unwrap();
            connection
                .execute(
                    "INSERT INTO overrides(work_id, field, value, updated_ms)
                     VALUES(?1, 'title', 'Heat', 1)",
                    [&work_id],
                )
                .unwrap();
            connection
                .execute(
                    "INSERT INTO markers(id, edition_id, kind, start_ms, end_ms, duration_ms, origin)
                     VALUES('marker-1', ?1, 'intro', 1000, 2000, 60000, 'file')",
                    [&edition_id],
                )
                .unwrap();
            connection
                .execute(
                    "INSERT INTO external_identities(work_id, provider, external_id, confidence)
                     VALUES(?1, 'tmdb', '949', 1.0)",
                    [&work_id],
                )
                .unwrap();
            connection
                .execute(
                    "INSERT INTO jobs(id, kind, target, status)
                     VALUES('job-1', 'probe', 'target-1', 'queued')",
                    [],
                )
                .unwrap();
            connection
                .execute(
                    "INSERT INTO conversion_jobs(id, file_id, title, profile, status, progress,
                        message, source_path, output_path, source_size, source_modified_ms,
                        output_bytes, created_ms, updated_ms)
                     VALUES('conversion-1', ?1, 'Heat', 'remux', 'done', 100, '', '/in.mkv',
                        '/out.mp4', 1, 1, 1, 1, 1)",
                    [&file_id],
                )
                .unwrap();
        }

        (library, path)
    }

    fn row_counts(path: &Path) -> Vec<(&'static str, i64)> {
        let connection = Connection::open(path).unwrap();
        TABLES
            .iter()
            .map(|table| {
                let count = connection
                    .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                        row.get(0)
                    })
                    .unwrap();
                (*table, count)
            })
            .collect()
    }

    #[test]
    fn a_backup_carries_every_table_it_names_and_restores_all_of_them() {
        let directory = tempfile::tempdir().unwrap();
        let (library, path) = library_with_every_table(&directory);
        let destination = directory.path().join("copy.db");
        library.backup_to(&destination).expect("backup");

        let live = row_counts(&path);
        let copied = row_counts(&destination);
        for (table, count) in &copied {
            assert!(*count > 0, "{table} is not covered by this test");
            if *table == "settings" {
                continue;
            }
            let live_count = live
                .iter()
                .find(|(name, _)| name == table)
                .map(|(_, count)| *count)
                .unwrap();
            assert_eq!(live_count, *count, "the backup left {table} rows behind");
        }
        let live_settings = live
            .iter()
            .find(|(name, _)| *name == "settings")
            .map(|(_, count)| *count)
            .unwrap();
        let copied_settings = copied
            .iter()
            .find(|(name, _)| *name == "settings")
            .map(|(_, count)| *count)
            .unwrap();
        assert_eq!(
            copied_settings,
            live_settings + 2,
            "the backup carries its two stamps and every setting"
        );

        let restore_directory = tempfile::tempdir().unwrap();
        let restore_path = restore_directory.path().join("library.db");
        let target = Library::open(&restore_path).unwrap();
        target.restore_from(&destination).expect("restore");

        let restored = row_counts(&restore_path);
        for (table, count) in &restored {
            let copied_count = copied
                .iter()
                .find(|(name, _)| name == table)
                .map(|(_, count)| *count)
                .unwrap();
            if *table == "settings" {
                assert_eq!(
                    *count,
                    copied_count - 2,
                    "a restore keeps the settings but not the backup stamp"
                );
                continue;
            }
            assert_eq!(*count, copied_count, "{table} did not come back");
        }
    }

    #[test]
    fn a_writer_is_allowed_to_wait_for_the_copy_instead_of_failing() {
        let directory = tempfile::tempdir().unwrap();
        let (library, _path) = library_on_disk(&directory);
        let library = Arc::new(library);
        let destination = directory.path().join("copy.db");
        let before = busy_timeout(&library);
        let during = Arc::new(std::sync::Mutex::new(None::<i64>));

        let recorded = Arc::clone(&during);
        let watched = Arc::clone(&library);
        library
            .backup_to_with(
                &destination,
                &move |_| {
                    let mut slot = recorded.lock().unwrap();
                    if slot.is_none() {
                        *slot = Some(busy_timeout(&watched));
                    }
                },
                &BackupCancel::new(),
            )
            .expect("backup");

        let during = during.lock().unwrap().expect("a copy ran");
        assert!(
            during > before,
            "a writer must wait for the copy instead of failing after {before} ms"
        );
        assert_eq!(
            busy_timeout(&library),
            before,
            "the library is left with the timeout it had"
        );
    }

    #[test]
    fn a_write_that_lands_during_a_copy_is_not_lost() {
        let directory = tempfile::tempdir().unwrap();
        let (library, path) = library_on_disk(&directory);
        let library = Arc::new(library);
        let destination = directory.path().join("copy.db");
        let (started_tx, started_rx) = mpsc::channel::<()>();

        let writer_path = path.clone();
        let writer = std::thread::spawn(move || {
            if started_rx.recv_timeout(Duration::from_secs(20)).is_err() {
                return false;
            }
            let connection = Connection::open(&writer_path).unwrap();
            connection
                .execute_batch(&format!("PRAGMA busy_timeout = {COPY_BUSY_TIMEOUT_MS};"))
                .unwrap();
            let deadline = std::time::Instant::now() + Duration::from_secs(20);
            loop {
                match connection.execute(
                    "INSERT INTO settings(key, value) VALUES('writtenDuringCopy', 'kept')",
                    [],
                ) {
                    Ok(_) => return true,
                    Err(error) if busy(&error) && std::time::Instant::now() < deadline => {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) => return false,
                }
            }
        });

        let start = std::sync::Mutex::new(Some(started_tx));
        library
            .backup_to_with(
                &destination,
                &move |_| {
                    if let Some(sender) = start.lock().unwrap().take() {
                        let _ = sender.send(());
                    }
                },
                &BackupCancel::new(),
            )
            .expect("backup");

        assert!(
            writer.join().unwrap(),
            "the write that landed during the copy was lost to SQLITE_BUSY"
        );
        let connection = Connection::open(&path).unwrap();
        let kept: String = connection
            .query_row(
                "SELECT value FROM settings WHERE key = 'writtenDuringCopy'",
                [],
                |row| row.get(0),
            )
            .expect("the write survived");
        assert_eq!(kept, "kept");
        assert!(destination.is_file());
    }

    #[test]
    fn a_missing_library_file_is_reported_and_never_recreated() {
        let directory = tempfile::tempdir().unwrap();
        let (library, path) = library_on_disk(&directory);
        std::fs::remove_file(&path).unwrap();
        let destination = directory.path().join("copy.db");

        let error = library.backup_to(&destination).unwrap_err();

        assert_eq!(error.code(), "backup_source_unreadable");
        assert!(
            !path.exists(),
            "opening the library for the copy must never invent an empty one"
        );
        assert!(!destination.exists());
    }

    #[test]
    fn a_copy_left_over_by_a_crash_is_swept_before_the_next_backup() {
        let directory = tempfile::tempdir().unwrap();
        let (library, _path) = library_on_disk(&directory);
        let destination = directory.path().join("copy.db");
        let stale = directory
            .path()
            .join(".copy.db.11111111-1111-1111-1111-111111111111.partial");
        let running = directory
            .path()
            .join(".copy.db.22222222-2222-2222-2222-222222222222.partial");
        std::fs::write(&stale, vec![0_u8; 4_096]).unwrap();
        std::fs::write(&running, vec![0_u8; 4_096]).unwrap();
        std::fs::File::options()
            .write(true)
            .open(&stale)
            .unwrap()
            .set_modified(std::time::SystemTime::now() - Duration::from_secs(7_200))
            .unwrap();

        library.backup_to(&destination).expect("backup");

        assert!(
            !stale.exists(),
            "a full-size copy left by a crash is never collected"
        );
        assert!(
            running.exists(),
            "a copy that may still be running is left alone"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_real_failure_is_reported_instead_of_the_cancel_that_arrived_with_it() {
        use std::os::unix::fs::PermissionsExt;

        let directory = tempfile::tempdir().unwrap();
        let (library, _path) = library_on_disk(&directory);
        let walled = directory.path().join("walled");
        std::fs::create_dir_all(&walled).unwrap();
        let writable = std::fs::metadata(&walled).unwrap().permissions();
        std::fs::set_permissions(&walled, std::fs::Permissions::from_mode(0o500)).unwrap();

        let cancel = BackupCancel::new();
        let during = cancel.clone();
        let outcome =
            library.backup_to_with(walled.join("copy.db"), &move |_| during.cancel(), &cancel);
        std::fs::set_permissions(&walled, writable).unwrap();

        let error = outcome.unwrap_err();
        assert_eq!(
            error.code(),
            "database_error",
            "the disk was the problem, not the cancel: {error}"
        );
    }

    #[test]
    fn a_failed_restore_detaches_its_copy_so_the_next_one_runs() {
        let directory = tempfile::tempdir().unwrap();
        let (library, path) = library_on_disk(&directory);
        let backup = directory.path().join("copy.db");
        library.backup_to(&backup).expect("backup");

        {
            let connection = Connection::open(&path).unwrap();
            connection
                .execute_batch(
                    "CREATE TRIGGER refuse_restore BEFORE INSERT ON works
                     BEGIN SELECT RAISE(ABORT, 'test restore refusal'); END;",
                )
                .unwrap();
        }
        assert_eq!(
            library.restore_from(&backup).unwrap_err().code(),
            "database_error"
        );
        {
            let connection = Connection::open(&path).unwrap();
            connection
                .execute_batch("DROP TRIGGER refuse_restore;")
                .unwrap();
        }

        library
            .restore_from(&backup)
            .expect("a failed restore must not leave its copy attached");
        assert_eq!(library.count(&LibraryQuery::default()).unwrap(), 2);
    }
}
