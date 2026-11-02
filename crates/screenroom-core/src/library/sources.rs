use std::path::{Path, PathBuf};

use rusqlite::{Row, params};

use super::{Library, validated};
use crate::domain::{Source, SourceState};
use crate::error::{CoreError, Result};
use crate::volume::VolumeIdentity;

impl Library {
    pub fn sources(&self) -> Result<Vec<Source>> {
        let connection = self.connection();
        let mut statement = connection.prepare(
            "SELECT id, label, root_path, state, last_scan_ms FROM sources
             WHERE state != 'removed' ORDER BY label COLLATE NOCASE",
        )?;
        let rows = statement.query_map([], source_from_row)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn source(&self, id: &str) -> Result<Source> {
        let connection = self.connection();
        let mut statement = connection.prepare(
            "SELECT id, label, root_path, state, last_scan_ms FROM sources WHERE id = ?1",
        )?;
        statement
            .query_row([id], source_from_row)
            .map_err(|error| match error {
                rusqlite::Error::QueryReturnedNoRows => {
                    CoreError::library("source_missing", "The source is no longer in the library.")
                }
                other => CoreError::Sqlite(other),
            })
    }

    pub fn add_source(&self, root: impl AsRef<Path>, label: Option<&str>) -> Result<Source> {
        let root = self.canonical_root(root)?;
        self.check_overlapping_root(&root, None)?;
        let label = match label {
            Some(label) => validated(label, 120)?.to_string(),
            None => root
                .file_name()
                .map(|name| name.to_string_lossy().trim().to_string())
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| root.to_string_lossy().to_string()),
        };
        let root_path = root.to_string_lossy().to_string();
        let volume_identity = volume_identity_storage(&root);
        let connection = self.connection();

        let mut statement =
            connection.prepare("SELECT id, root_path FROM sources WHERE state = 'removed'")?;
        let removed = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
            .into_iter()
            .find(|(_, old_root)| self.path_key(Path::new(old_root)) == self.path_key(&root));
        drop(statement);
        if let Some((id, _)) = removed {
            connection.execute(
                "UPDATE sources SET label = ?1, state = 'available', volume_identity = ?2
                 WHERE id = ?3",
                params![label, volume_identity, id],
            )?;
            drop(connection);
            return self.source(&id);
        }
        let source = Source {
            id: super::new_id(),
            label,
            root_path,
            state: SourceState::Available,
            last_scan_ms: None,
        };
        connection.execute(
            "INSERT INTO sources(id, label, root_path, state, volume_identity)
             VALUES(?1, ?2, ?3, ?4, ?5)",
            params![
                source.id,
                source.label,
                source.root_path,
                source.state.as_str(),
                volume_identity
            ],
        )?;
        Ok(source)
    }

    pub fn relink_source(&self, id: &str, new_root: impl AsRef<Path>) -> Result<()> {
        let root = self.canonical_root(new_root)?;
        self.check_overlapping_root(&root, Some(id))?;
        let mut connection = self.connection();
        let tx = connection.transaction()?;
        let changed = tx.execute(
            "UPDATE sources SET root_path = ?1, state = 'available', volume_identity = ?2
             WHERE id = ?3",
            params![
                root.to_string_lossy().to_string(),
                volume_identity_storage(&root),
                id
            ],
        )?;
        if changed == 0 {
            return Err(CoreError::library(
                "source_missing",
                "The source is no longer in the library.",
            ));
        }
        tx.execute("UPDATE files SET available = 0 WHERE source_id = ?1", [id])?;
        tx.commit()?;
        Ok(())
    }

    pub fn source_volume_identity(&self, id: &str) -> Result<Option<VolumeIdentity>> {
        let connection = self.connection();
        let stored: Option<Option<String>> = connection
            .query_row(
                "SELECT volume_identity FROM sources WHERE id = ?1",
                [id],
                |row| row.get(0),
            )
            .map(Some)
            .or_else(|error| match error {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                other => Err(other),
            })?;
        Ok(stored
            .flatten()
            .and_then(|raw| VolumeIdentity::decode(&raw)))
    }

    pub(crate) fn record_volume_identity(&self, id: &str, identity: &VolumeIdentity) -> Result<()> {
        let changed = self.connection().execute(
            "UPDATE sources SET volume_identity = ?1 WHERE id = ?2",
            params![identity.encode(), id],
        )?;
        if changed == 0 {
            return Err(CoreError::library(
                "source_missing",
                "The source is no longer in the library.",
            ));
        }
        Ok(())
    }

    pub(crate) fn mark_source_unreachable(&self, id: &str, state: SourceState) -> Result<()> {
        self.connection().execute(
            "UPDATE sources SET state = ?1 WHERE id = ?2 AND state != 'removed'",
            params![state.as_str(), id],
        )?;
        Ok(())
    }

    pub fn rename_source(&self, id: &str, label: &str) -> Result<()> {
        let label = validated(label, 120)?.to_string();
        let changed = self.connection().execute(
            "UPDATE sources SET label = ?1 WHERE id = ?2",
            params![label, id],
        )?;
        if changed == 0 {
            return Err(CoreError::library(
                "source_missing",
                "The source is no longer in the library.",
            ));
        }
        Ok(())
    }

    pub fn remove_source(&self, id: &str) -> Result<()> {
        self.set_source_state(id, SourceState::Removed)
    }

    pub fn set_source_state(&self, id: &str, state: SourceState) -> Result<()> {
        let mut connection = self.connection();
        let tx = connection.transaction()?;
        let changed = tx.execute(
            "UPDATE sources SET state = ?1 WHERE id = ?2",
            params![state.as_str(), id],
        )?;
        if changed == 0 {
            return Err(CoreError::library(
                "source_missing",
                "The source is no longer in the library.",
            ));
        }
        if state != SourceState::Available {
            tx.execute("UPDATE files SET available = 0 WHERE source_id = ?1", [id])?;
        }
        tx.commit()?;
        Ok(())
    }

    fn check_overlapping_root(&self, candidate: &Path, except: Option<&str>) -> Result<()> {
        for source in self.sources()? {
            if Some(source.id.as_str()) == except || source.root_path.is_empty() {
                continue;
            }
            if self.overlaps(candidate, Path::new(&source.root_path)) {
                return Err(CoreError::library(
                    "duplicate_source",
                    "This folder overlaps an existing source.",
                ));
            }
        }
        Ok(())
    }

    fn overlaps(&self, left: &Path, right: &Path) -> bool {
        let left: PathBuf = self.path_key(left).into();
        let right: PathBuf = self.path_key(right).into();
        left == right || left.starts_with(&right) || right.starts_with(&left)
    }
}

fn volume_identity_storage(root: &Path) -> Option<String> {
    VolumeIdentity::probe(root).map(|identity| identity.encode())
}

fn source_from_row(row: &Row<'_>) -> rusqlite::Result<Source> {
    let state: String = row.get(3)?;
    Ok(Source {
        id: row.get(0)?,
        label: row.get(1)?,
        root_path: row.get(2)?,
        state: SourceState::parse(&state).ok_or_else(|| {
            rusqlite::Error::FromSqlConversionFailure(
                3,
                rusqlite::types::Type::Text,
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("unknown source state {state:?}"),
                )),
            )
        })?,
        last_scan_ms: row.get(4)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regrant_matches_removed_roots_by_location_key() {
        let directory = tempfile::tempdir().unwrap();

        let base = std::fs::canonicalize(directory.path()).unwrap();
        let root = base.join("media");
        std::fs::create_dir(&root).unwrap();
        let library = Library::open_in_memory().unwrap();
        let source = library.add_source(&root, None).unwrap();
        library.remove_source(&source.id).unwrap();
        let alternate = format!("{}//media", root.parent().unwrap().display());
        library
            .connection()
            .execute(
                "UPDATE sources SET root_path = ?1 WHERE id = ?2",
                rusqlite::params![alternate, source.id],
            )
            .unwrap();

        let restored = library.add_source(&root, None).unwrap();
        assert_eq!(restored.id, source.id);
    }

    fn media_root(directory: &tempfile::TempDir, name: &str) -> PathBuf {
        let root = directory.path().join(name);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn a_source_with_no_label_is_named_after_its_folder_not_its_full_path() {
        let directory = tempfile::tempdir().unwrap();
        let root = media_root(&directory, "Films");
        let library = Library::open_in_memory().unwrap();

        let source = library.add_source(&root, None).unwrap();
        assert_eq!(source.label, "Films");
        assert_eq!(source.state, SourceState::Available);
        assert_eq!(source.last_scan_ms, None);
        assert_eq!(library.sources().unwrap(), vec![source]);
    }

    #[test]
    fn an_explicit_label_is_trimmed_and_a_blank_one_is_refused() {
        let directory = tempfile::tempdir().unwrap();
        let library = Library::open_in_memory().unwrap();
        let root = media_root(&directory, "Films");

        for blank in ["", "   ", "\t\n"] {
            let error = library.add_source(&root, Some(blank)).unwrap_err();
            assert_eq!(error.code(), "invalid_input", "{blank:?} is not a label");
        }
        assert!(
            library.sources().unwrap().is_empty(),
            "a refused label must not leave a half-made source"
        );

        let source = library
            .add_source(&root, Some("  Living room drive  "))
            .unwrap();
        assert_eq!(source.label, "Living room drive");
        assert_eq!(
            library.rename_source(&source.id, "   ").unwrap_err().code(),
            "invalid_input",
            "renaming to nothing is refused, not silently emptied"
        );
        assert_eq!(
            library.source(&source.id).unwrap().label,
            "Living room drive"
        );
        library.rename_source(&source.id, " Bedroom ").unwrap();
        assert_eq!(library.source(&source.id).unwrap().label, "Bedroom");
    }

    #[test]
    fn a_label_too_long_for_the_interface_is_refused() {
        let directory = tempfile::tempdir().unwrap();
        let library = Library::open_in_memory().unwrap();
        let root = media_root(&directory, "Films");
        let long = "a".repeat(121);
        assert_eq!(
            library.add_source(&root, Some(&long)).unwrap_err().code(),
            "invalid_input"
        );
        assert!(library.sources().unwrap().is_empty());

        let source = library.add_source(&root, None).unwrap();
        assert_eq!(
            library.rename_source(&source.id, &long).unwrap_err().code(),
            "invalid_input"
        );
        assert_eq!(library.source(&source.id).unwrap().label, "Films");
    }

    #[test]
    fn an_unknown_source_is_reported_by_name_rather_than_returned_empty() {
        let library = Library::open_in_memory().unwrap();
        assert_eq!(
            library.source("missing").unwrap_err().code(),
            "source_missing"
        );
        assert_eq!(
            library
                .rename_source("missing", "Anything")
                .unwrap_err()
                .code(),
            "source_missing"
        );
        assert_eq!(
            library
                .set_source_state("missing", SourceState::Disconnected)
                .unwrap_err()
                .code(),
            "source_missing"
        );
    }
}
