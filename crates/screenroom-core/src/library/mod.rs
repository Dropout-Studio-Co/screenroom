mod artwork;
mod backup;
mod bookmarks;
mod catalog;
mod conversion;
mod lists;
mod markers;
mod media_info;
mod metadata;
mod playback;
mod recommend;
mod settings;
mod sources;
mod subtitles;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::domain::Profile;
use crate::error::{CoreError, Result};
use crate::paths::{PathStyle, location_key};
use crate::{SCHEMA_VERSION, schema};

pub use artwork::{Artwork, ArtworkCandidate};
pub use backup::{BackupInfo, BackupReport, RestoreReport};
pub use bookmarks::{Bookmark, BookmarkDraft, protocol_of};
pub use catalog::Ingest;
pub use conversion::{
    ConversionJob, ConversionPlan, ConversionProfile, ConversionStatus, OutputFacts, verify_output,
};
pub use media_info::UnprobedFile;
pub use metadata::EnrichmentSummary;
pub use playback::Checkpoint;
pub use settings::Preferences;
pub use subtitles::{SUBTITLE_EXTENSIONS, StoredSubtitle};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sort {
    #[default]
    Title,

    Added,

    Year,

    Progress,
}

impl Sort {
    fn order_by(self) -> &'static str {
        match self {
            Self::Title => {
                "title COLLATE NOCASE, COALESCE(season_number, 0), COALESCE(episode_number, 0), id"
            }
            Self::Added => "added_ms DESC, id",
            Self::Year => "year DESC, title COLLATE NOCASE, id",
            Self::Progress => "progress_updated_ms DESC, id",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LibraryQuery {
    pub profile_id: Option<String>,

    pub text: String,

    pub kind: Option<String>,

    pub work_id: Option<String>,

    pub genre: Option<String>,

    pub season: Option<i64>,

    pub available_only: bool,

    pub watched: Option<bool>,

    pub watchlist_only: bool,

    pub favorites_only: bool,

    pub group_by_work: bool,

    pub sort: Sort,

    pub offset: i64,

    pub limit: i64,
}

impl LibraryQuery {
    pub fn page(limit: i64) -> Self {
        Self {
            limit,
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathSemantics {
    CaseInsensitive,

    CaseSensitive,
}

impl PathSemantics {
    pub fn host() -> Self {
        if cfg!(any(windows, target_vendor = "apple")) {
            Self::CaseInsensitive
        } else {
            Self::CaseSensitive
        }
    }
}

pub struct Library {
    connection: Mutex<Connection>,
    active_scans: Mutex<HashSet<String>>,
    path_semantics: PathSemantics,
    path_style: PathStyle,

    path: Option<PathBuf>,
}

impl std::fmt::Debug for Library {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Library").finish_non_exhaustive()
    }
}

fn add_bookmarks_table(tx: &Connection) -> Result<()> {
    let present: i64 = tx.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'source_bookmarks'",
        [],
        |row| row.get(0),
    )?;
    if present == 0 {
        for statement in schema::MIGRATE_6_TO_7 {
            tx.execute(statement, [])?;
        }
    }
    Ok(())
}

fn add_subtitles_table(tx: &Connection) -> Result<()> {
    let present: i64 = tx.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'subtitles'",
        [],
        |row| row.get(0),
    )?;
    if present == 0 {
        for statement in schema::MIGRATE_7_TO_8 {
            tx.execute(statement, [])?;
        }
    }
    Ok(())
}

fn add_metadata_origin(tx: &Connection) -> Result<()> {
    let mut statement = tx.prepare("PRAGMA table_info(works)")?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if columns.iter().any(|name| name == "metadata_origin") {
        return Ok(());
    }
    for statement in schema::MIGRATE_5_TO_6 {
        tx.execute(statement, [])?;
    }
    Ok(())
}

impl Library {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)?;
        }
        Self::from_connection(
            Connection::open(path)?,
            Some(path.to_path_buf()),
            PathSemantics::host(),
        )
    }

    pub fn open_in_memory() -> Result<Self> {
        Self::from_connection(Connection::open_in_memory()?, None, PathSemantics::host())
    }

    pub fn with_path_style(mut self, style: PathStyle) -> Self {
        self.path_style = style;
        self
    }

    pub fn with_path_semantics(mut self, semantics: PathSemantics) -> Self {
        self.path_semantics = semantics;
        self
    }

    fn from_connection(
        mut connection: Connection,
        path: Option<PathBuf>,
        path_semantics: PathSemantics,
    ) -> Result<Self> {
        connection.execute_batch("PRAGMA foreign_keys = ON; PRAGMA busy_timeout = 5000;")?;
        let version: i64 =
            connection.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))?;
        let integrity: String =
            connection.query_row("PRAGMA quick_check", [], |row| row.get::<_, String>(0))?;
        if integrity != "ok" {
            return Err(CoreError::library(
                "corrupt_database",
                "The library failed its integrity check. Restore a validated backup.",
            ));
        }

        match version {
            0 => {
                let tx = connection.transaction()?;
                tx.execute_batch("PRAGMA foreign_keys = ON;")?;
                for statement in schema::CREATE {
                    tx.execute(statement, [])?;
                }
                tx.execute(
                    "INSERT INTO profiles(id, name) VALUES(?1, ?2)",
                    [new_id(), "Me".to_string()],
                )?;
                tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
                tx.commit()?;
            }
            v if v == SCHEMA_VERSION => {}
            2 => {
                let tx = connection.transaction()?;
                for statement in schema::MIGRATE_2_TO_3 {
                    tx.execute(statement, [])?;
                }
                for statement in schema::MIGRATE_3_TO_4 {
                    tx.execute(statement, [])?;
                }
                for statement in schema::MIGRATE_4_TO_5 {
                    tx.execute(statement, [])?;
                }
                add_metadata_origin(&tx)?;
                add_bookmarks_table(&tx)?;
                add_subtitles_table(&tx)?;
                tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
                tx.commit()?;
            }
            3 => {
                let tx = connection.transaction()?;
                for statement in schema::MIGRATE_3_TO_4 {
                    tx.execute(statement, [])?;
                }
                for statement in schema::MIGRATE_4_TO_5 {
                    tx.execute(statement, [])?;
                }
                add_metadata_origin(&tx)?;
                add_bookmarks_table(&tx)?;
                add_subtitles_table(&tx)?;
                tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
                tx.commit()?;
            }
            4 => {
                let tx = connection.transaction()?;
                for statement in schema::MIGRATE_4_TO_5 {
                    tx.execute(statement, [])?;
                }
                add_metadata_origin(&tx)?;
                add_bookmarks_table(&tx)?;
                add_subtitles_table(&tx)?;
                tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
                tx.commit()?;
            }
            5 => {
                let tx = connection.transaction()?;
                add_metadata_origin(&tx)?;
                add_bookmarks_table(&tx)?;
                add_subtitles_table(&tx)?;
                tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
                tx.commit()?;
            }
            6 => {
                let tx = connection.transaction()?;
                add_bookmarks_table(&tx)?;
                add_subtitles_table(&tx)?;
                tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
                tx.commit()?;
            }
            7 => {
                let tx = connection.transaction()?;
                add_subtitles_table(&tx)?;
                tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
                tx.commit()?;
            }
            1 => {
                return Err(CoreError::library(
                    "migration_unavailable",
                    "This database needs an unsupported migration.",
                ));
            }
            future => {
                return Err(CoreError::library(
                    "future_database",
                    format!(
                        "This library was written by a newer Screenroom (schema {future}). \
                         Update the app instead of resetting your data."
                    ),
                ));
            }
        }

        connection.execute("UPDATE sessions SET status = 'closed'", [])?;
        connection.execute(
            "UPDATE jobs SET status = 'queued' WHERE status = 'running'",
            [],
        )?;
        connection.execute(
            "UPDATE conversion_jobs
             SET status = 'failed', message = 'The app closed before this finished.',
                 updated_ms = ?1
             WHERE status IN ('queued','running','verifying')",
            [now_ms()],
        )?;

        Ok(Self {
            connection: Mutex::new(connection),
            active_scans: Mutex::new(HashSet::new()),
            path_semantics,
            path_style: PathStyle::host(),
            path,
        })
    }

    fn connection(&self) -> MutexGuard<'_, Connection> {
        self.connection
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub(crate) fn claim_scan(&self, source_id: &str) -> Result<()> {
        let mut active = self
            .active_scans
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !active.insert(source_id.to_string()) {
            return Err(CoreError::library(
                "scan_running",
                "This source is already being scanned.",
            ));
        }
        Ok(())
    }

    pub(crate) fn release_scan(&self, source_id: &str) {
        self.active_scans
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(source_id);
    }

    pub(crate) fn canonical_root(&self, root: impl AsRef<Path>) -> Result<PathBuf> {
        let root = root.as_ref();
        if !root.is_absolute() {
            return Err(CoreError::library(
                "invalid_root",
                "Choose an absolute folder path.",
            ));
        }
        if !root.exists() {
            return Err(CoreError::library(
                "source_missing",
                "The selected folder is unavailable.",
            ));
        }
        std::fs::canonicalize(root).map_err(|error| match error.kind() {
            std::io::ErrorKind::PermissionDenied => CoreError::library(
                "permission_lost",
                "The selected folder cannot be read. Grant access again.",
            ),
            std::io::ErrorKind::NotFound => {
                CoreError::library("source_missing", "The selected folder is unavailable.")
            }
            _ => CoreError::Io(error),
        })
    }

    pub fn path_key(&self, path: &Path) -> String {
        location_key(
            &path.to_string_lossy(),
            self.path_semantics,
            self.path_style,
        )
    }

    pub fn profiles(&self) -> Result<Vec<Profile>> {
        let connection = self.connection();
        let mut statement = connection.prepare("SELECT id, name FROM profiles ORDER BY rowid")?;
        let rows = statement.query_map([], |row| {
            Ok(Profile {
                id: row.get(0)?,
                name: row.get(1)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn add_profile(&self, name: &str) -> Result<Profile> {
        let name = validated(name, 120)?.to_string();
        let profile = Profile { id: new_id(), name };
        self.connection().execute(
            "INSERT INTO profiles(id, name) VALUES(?1, ?2)",
            [&profile.id, &profile.name],
        )?;
        Ok(profile)
    }

    pub fn rename_profile(&self, id: &str, name: &str) -> Result<()> {
        let name = validated(name, 120)?.to_string();
        let changed = self.connection().execute(
            "UPDATE profiles SET name = ?1 WHERE id = ?2",
            [name, id.to_string()],
        )?;
        if changed == 0 {
            return Err(CoreError::library(
                "profile_missing",
                "That profile is no longer in the library.",
            ));
        }
        Ok(())
    }

    pub fn remove_profile(&self, id: &str) -> Result<()> {
        let mut connection = self.connection();
        let tx = connection.transaction()?;
        let count: i64 = tx.query_row("SELECT COUNT(*) FROM profiles", [], |row| row.get(0))?;
        if count <= 1 {
            return Err(CoreError::library(
                "last_profile",
                "Keep at least one profile.",
            ));
        }
        let changed = tx.execute("DELETE FROM profiles WHERE id = ?1", [id])?;
        if changed == 0 {
            return Err(CoreError::library(
                "profile_missing",
                "That profile is no longer in the library.",
            ));
        }
        tx.execute(
            "DELETE FROM settings WHERE key = 'defaultProfileId' AND value = ?1",
            [id],
        )?;
        tx.commit()?;
        Ok(())
    }
}

pub(crate) fn validated(value: &str, max: usize) -> Result<&str> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.chars().count() > max || trimmed.contains('\0') {
        return Err(CoreError::library("invalid_input", "Enter a valid value."));
    }
    Ok(trimmed)
}

pub(crate) fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or_default()
}

pub(crate) fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removing_the_default_profile_clears_its_preference_and_unknown_ids_fail() {
        let library = Library::open_in_memory().unwrap();
        let profile = library.profiles().unwrap().remove(0);
        let guest = library.add_profile("Guest").unwrap();
        let preferences = Preferences {
            default_profile: Some(profile.id.clone()),
            ..Preferences::default()
        };
        library.set_preferences(&preferences).unwrap();

        let error = library.remove_profile("missing").unwrap_err();
        assert_eq!(error.code(), "profile_missing");
        library.remove_profile(&profile.id).unwrap();
        assert_eq!(library.preferences().unwrap().default_profile, None);
        assert!(
            library
                .profiles()
                .unwrap()
                .iter()
                .any(|item| item.id == guest.id)
        );
    }

    #[test]
    fn a_schema_three_library_gains_the_join_indexes_and_keeps_its_rows() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("library.db");
        {
            let connection = rusqlite::Connection::open(&path).expect("open");
            for statement in &schema::CREATE[..schema::CREATE.len() - 4] {
                connection.execute(statement, []).expect("schema");
            }
            connection
                .execute("INSERT INTO profiles(id, name) VALUES('p', 'Me')", [])
                .expect("profile");
            connection
                .pragma_update(None, "user_version", 3)
                .expect("version");
        }

        let library = Library::open(&path).expect("migrated");
        let version: i64 = library
            .connection()
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .expect("version");
        assert_eq!(version, SCHEMA_VERSION);
        assert_eq!(library.profiles().expect("profiles").len(), 1);

        for index in [
            "editions_work",
            "files_edition",
            "file_episodes_episode",
            "markers_edition",
        ] {
            let found: i64 = library
                .connection()
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name=?1",
                    [index],
                    |row| row.get(0),
                )
                .expect("index lookup");
            assert_eq!(found, 1, "{index} is missing after the migration");
        }
    }
}
