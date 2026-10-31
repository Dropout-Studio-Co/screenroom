use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::error::{CoreError, Result};
use crate::{BACKUP_FORMAT_VERSION, SCHEMA_VERSION};

use super::{Library, new_id, now_ms};

const TABLES: &[&str] = &[
    "sources",
    "works",
    "seasons",
    "episodes",
    "editions",
    "files",
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
    "settings",
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

impl Library {
    pub fn backup_to(&self, path: impl AsRef<Path>) -> Result<BackupReport> {
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
        let staging = staging_path(&destination)?;

        let _ = std::fs::remove_file(&staging);

        let outcome = (|| -> Result<()> {
            let connection = self.connection();
            connection.execute("VACUUM INTO ?1", [staging.to_string_lossy().to_string()])?;
            stamp(&staging)?;
            Ok(())
        })();
        if let Err(error) = outcome {
            let _ = std::fs::remove_file(&staging);
            return Err(error);
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

        let previous_saved_to = match &self.path {
            Some(live) => {
                let safety = safety_path(live);
                let report = self.backup_to(&safety)?;
                Some(report.path)
            }
            None => None,
        };

        let mut connection = self.connection();

        connection.execute(
            "ATTACH DATABASE ?1 AS backup",
            [source.to_string_lossy().to_string()],
        )?;
        let outcome = replace_contents(&mut connection);

        let _ = connection.execute_batch("DETACH DATABASE backup;");
        outcome?;
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
}

fn replace_contents(connection: &mut Connection) -> Result<()> {
    let tx = connection.transaction()?;
    tx.execute_batch("PRAGMA defer_foreign_keys = ON;")?;
    for table in TABLES.iter().rev() {
        tx.execute(&format!("DELETE FROM main.{table}"), [])?;
    }

    let present: std::collections::HashSet<String> = {
        let mut statement =
            tx.prepare("SELECT name FROM backup.sqlite_master WHERE type = 'table'")?;
        let names = statement.query_map([], |row| row.get::<_, String>(0))?;
        names.collect::<rusqlite::Result<_>>()?
    };
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
    tx.commit()?;
    Ok(())
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
    let name = destination
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "library".to_string());
    Ok(parent.join(format!(".{name}.{}.partial", new_id())))
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
