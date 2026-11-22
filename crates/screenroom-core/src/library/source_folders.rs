use std::collections::{BTreeSet, HashMap};
use std::path::{Component, Path};

use rusqlite::params;

use super::{Library, now_ms};
use crate::domain::{SourceFolder, SourceMedia};
use crate::error::{CoreError, Result};

const MAX_SELECTIONS: usize = 2_000;

const MAX_SELECTION_CHARS: usize = 1_024;

impl Library {
    pub fn source_folders(&self, source_id: &str) -> Result<Vec<String>> {
        self.ensure_source_present(source_id)?;
        let connection = self.connection();
        let mut statement =
            connection.prepare("SELECT relative_path FROM source_folders WHERE source_id = ?1")?;
        let rows = statement.query_map([source_id], |row| row.get::<_, String>(0))?;
        let mut folders = rows.collect::<rusqlite::Result<Vec<_>>>()?;
        folders.sort();
        Ok(folders)
    }

    pub fn all_source_folders(&self) -> Result<Vec<SourceFolder>> {
        let connection = self.connection();
        let mut statement = connection.prepare(
            "SELECT f.source_id, f.relative_path FROM source_folders f
             JOIN sources s ON s.id = f.source_id
             WHERE s.state != 'removed' ORDER BY f.source_id, f.relative_path",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(SourceFolder {
                source_id: row.get(0)?,
                relative_path: row.get(1)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn configure_source_folders(&self, source_id: &str, folders: &[String]) -> Result<()> {
        let selections = normalize_selections(folders)?;
        let mut connection = self.connection();
        let tx = connection.transaction()?;
        let present: i64 = tx.query_row(
            "SELECT COUNT(*) FROM sources WHERE id = ?1 AND state != 'removed'",
            [source_id],
            |row| row.get(0),
        )?;
        if present == 0 {
            return Err(CoreError::library(
                "source_missing",
                "The source is no longer in the library.",
            ));
        }
        tx.execute(
            "DELETE FROM source_folders WHERE source_id = ?1",
            [source_id],
        )?;
        let added_ms = now_ms();
        {
            let mut statement = tx.prepare(
                "INSERT INTO source_folders(source_id, relative_path, added_ms)
                 VALUES(?1, ?2, ?3)",
            )?;
            for selection in &selections {
                statement.execute(params![source_id, selection, added_ms])?;
            }
        }
        tx.execute(
            "UPDATE sources SET folders_configured = 1 WHERE id = ?1",
            [source_id],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn source_media(&self) -> Result<Vec<SourceMedia>> {
        let connection = self.connection();
        let mut statement = connection.prepare(
            "SELECT s.id,
               (SELECT COUNT(*) FROM files f WHERE f.source_id = s.id AND f.available = 1
                  AND s.state = 'available'),
               (SELECT COUNT(*) FROM files f WHERE f.source_id = s.id)
             FROM sources s WHERE s.state != 'removed'
             ORDER BY s.label COLLATE NOCASE, s.id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(SourceMedia {
                source_id: row.get(0)?,
                available: row.get(1)?,
                total: row.get(2)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn folder_media_counts(&self, source_id: &str) -> Result<Vec<(String, i64)>> {
        self.ensure_source_present(source_id)?;
        let connection = self.connection();
        let mut statement = connection.prepare(
            "SELECT f.relative_path FROM files f JOIN sources s ON s.id = f.source_id
                 WHERE f.source_id = ?1 AND f.available = 1 AND s.state = 'available'",
        )?;
        let rows = statement.query_map([source_id], |row| row.get::<_, String>(0))?;
        let mut counts: HashMap<String, i64> = HashMap::new();
        for relative in rows {
            let relative = relative?;
            let directory = Path::new(&relative)
                .parent()
                .map(|parent| parent.to_string_lossy().to_string())
                .unwrap_or_default();
            *counts.entry(directory).or_insert(0) += 1;
        }
        let mut directories: Vec<(String, i64)> = counts.into_iter().collect();
        directories.sort();
        Ok(directories)
    }

    pub fn available_media_count(&self) -> Result<i64> {
        let connection = self.connection();
        Ok(connection.query_row(
            "SELECT COUNT(*) FROM files f JOIN sources s ON s.id = f.source_id
             WHERE f.available = 1 AND s.state = 'available'",
            [],
            |row| row.get(0),
        )?)
    }

    fn ensure_source_present(&self, source_id: &str) -> Result<()> {
        let present: i64 = self.connection().query_row(
            "SELECT COUNT(*) FROM sources WHERE id = ?1 AND state != 'removed'",
            [source_id],
            |row| row.get(0),
        )?;
        if present == 0 {
            return Err(CoreError::library(
                "source_missing",
                "The source is no longer in the library.",
            ));
        }
        Ok(())
    }
}

pub(crate) fn selection_covers(selections: &[String], relative: &str) -> bool {
    selections
        .iter()
        .any(|selection| selection.is_empty() || covers_path(selection, relative))
}

fn covers_path(parent: &str, child: &str) -> bool {
    parent == child
        || parent.is_empty()
        || child
            .strip_prefix(parent)
            .is_some_and(|rest| rest.starts_with('/'))
}

fn normalize_selections(folders: &[String]) -> Result<Vec<String>> {
    if folders.len() > MAX_SELECTIONS {
        return Err(invalid_selection(format!(
            "Select at most {MAX_SELECTIONS} folders for one source."
        )));
    }
    let selected: BTreeSet<String> = folders
        .iter()
        .map(|folder| validate_selection(folder))
        .collect::<Result<_>>()?;
    let mut kept: Vec<String> = Vec::with_capacity(selected.len());
    for candidate in selected {
        if kept.iter().any(|parent| covers_path(parent, &candidate)) {
            continue;
        }
        kept.push(candidate);
    }
    Ok(kept)
}

fn validate_selection(folder: &str) -> Result<String> {
    if folder.contains('\0') {
        return Err(invalid_selection(
            "A selected folder cannot contain a null character.",
        ));
    }
    if folder.contains('\\') {
        return Err(invalid_selection(
            "Name a folder inside the source with forward slashes.",
        ));
    }
    let trimmed = folder.trim_matches('/');
    if trimmed.chars().count() > MAX_SELECTION_CHARS {
        return Err(invalid_selection(format!(
            "A selected folder cannot name more than {MAX_SELECTION_CHARS} characters."
        )));
    }
    if trimmed.is_empty() {
        return Ok(String::new());
    }
    if Path::new(trimmed).is_absolute() {
        return Err(invalid_selection(
            "Select a folder relative to the source, never an absolute path.",
        ));
    }
    let mut parts: Vec<&str> = Vec::new();
    for component in Path::new(trimmed).components() {
        match component {
            Component::Normal(part) => {
                let part = part.to_str().ok_or_else(|| {
                    invalid_selection("A selected folder must be valid UTF-8 text.")
                })?;
                if part.is_empty() {
                    continue;
                }
                parts.push(part);
            }
            _ => {
                return Err(invalid_selection(
                    "A selected folder must stay inside the source and cannot contain '.' or '..'.",
                ));
            }
        }
    }
    if parts.is_empty() {
        return Ok(String::new());
    }
    Ok(parts.join("/"))
}

fn invalid_selection(message: impl Into<String>) -> CoreError {
    CoreError::library("invalid_input", message.into())
}

#[cfg(test)]
mod tests {
    use rusqlite::params;

    use super::*;
    use crate::{Checkpoint, Ingest, LibraryQuery, parse_filename};

    fn media_root(directory: &tempfile::TempDir, name: &str) -> std::path::PathBuf {
        let root = directory.path().join(name);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn selection(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    fn ingest(library: &Library, source_id: &str, root: &str, relative: &str, available: bool) {
        let parsed = parse_filename(relative, None);
        library
            .ingest_file(Ingest {
                source_id,
                relative_path: relative,
                fingerprint: relative,
                size: 1,
                modified_ms: 1,
                scan_id: "test",
                parsed: Some(&parsed),
                expected_root: Some(root),
            })
            .unwrap();
        if !available {
            library
                .connection()
                .execute(
                    "UPDATE files SET available = 0 WHERE source_id = ?1 AND relative_path = ?2",
                    params![source_id, relative],
                )
                .unwrap();
        }
    }

    #[test]
    fn selections_are_trimmed_deduplicated_and_pruned_to_the_parent_that_covers_them() {
        let directory = tempfile::tempdir().unwrap();
        let root = media_root(&directory, "media");
        let library = Library::open_in_memory().unwrap();
        let source = library.add_source(&root, None).unwrap();

        library
            .configure_source_folders(
                &source.id,
                &selection(&[
                    "/Movies/",
                    "Movies",
                    "Shows//Season 1/",
                    "Shows/Season 1",
                    "Movies/Extras",
                ]),
            )
            .unwrap();
        assert_eq!(
            library.source_folders(&source.id).unwrap(),
            selection(&["Movies", "Shows/Season 1"])
        );

        library
            .configure_source_folders(&source.id, &selection(&["Shows", "Shows/Season 1"]))
            .unwrap();
        assert_eq!(
            library.source_folders(&source.id).unwrap(),
            selection(&["Shows"]),
            "a parent selection covers the child that sits inside it"
        );

        library
            .configure_source_folders(&source.id, &selection(&["Shows", ""]))
            .unwrap();
        assert_eq!(
            library.source_folders(&source.id).unwrap(),
            selection(&[""]),
            "the root covers every other selection"
        );

        let reloaded = library.source(&source.id).unwrap();
        assert!(reloaded.folders_configured);
        assert_eq!(reloaded.folder_count, 1);
    }

    #[test]
    fn a_selection_that_is_not_a_plain_relative_folder_is_refused() {
        let directory = tempfile::tempdir().unwrap();
        let root = media_root(&directory, "media");
        let library = Library::open_in_memory().unwrap();
        let source = library.add_source(&root, None).unwrap();

        for refused in [
            "..",
            "../Movies",
            "Movies/../Shows",
            "./Movies",
            "Movies\\Shows",
            "Movies\0Shows",
        ] {
            let error = library
                .configure_source_folders(&source.id, &selection(&[refused]))
                .unwrap_err();
            assert_eq!(error.code(), "invalid_input", "{refused:?} was accepted");
        }

        let too_long = "a".repeat(MAX_SELECTION_CHARS + 1);
        assert_eq!(
            library
                .configure_source_folders(&source.id, &selection(&[&too_long]))
                .unwrap_err()
                .code(),
            "invalid_input"
        );

        let too_many: Vec<String> = (0..=MAX_SELECTIONS)
            .map(|index| format!("Folder {index}"))
            .collect();
        assert_eq!(
            library
                .configure_source_folders(&source.id, &too_many)
                .unwrap_err()
                .code(),
            "invalid_input"
        );

        assert!(
            library.source_folders(&source.id).unwrap().is_empty(),
            "a refused selection left rows behind"
        );
        assert!(
            !library.source(&source.id).unwrap().folders_configured,
            "a refused selection configured the source"
        );
    }

    #[test]
    fn an_empty_selection_is_a_configured_source_that_indexes_nothing() {
        let directory = tempfile::tempdir().unwrap();
        let root = media_root(&directory, "media");
        let library = Library::open_in_memory().unwrap();
        let source = library.add_source(&root, None).unwrap();

        library.configure_source_folders(&source.id, &[]).unwrap();

        let reloaded = library.source(&source.id).unwrap();
        assert!(reloaded.folders_configured);
        assert_eq!(reloaded.folder_count, 0);
        assert!(library.source_folders(&source.id).unwrap().is_empty());
    }

    #[test]
    fn an_unknown_source_cannot_be_configured_or_asked_for_its_folders() {
        let library = Library::open_in_memory().unwrap();
        assert_eq!(
            library
                .configure_source_folders("missing", &selection(&["Movies"]))
                .unwrap_err()
                .code(),
            "source_missing"
        );
        assert_eq!(
            library.source_folders("missing").unwrap_err().code(),
            "source_missing"
        );
        assert_eq!(
            library.folder_media_counts("missing").unwrap_err().code(),
            "source_missing"
        );
    }

    #[test]
    fn a_removed_source_leaves_the_selections_and_the_counts() {
        let directory = tempfile::tempdir().unwrap();
        let root = media_root(&directory, "media");
        let library = Library::open_in_memory().unwrap();
        let source = library.add_source(&root, None).unwrap();
        library
            .configure_source_folders(&source.id, &selection(&["Movies"]))
            .unwrap();
        assert_eq!(library.all_source_folders().unwrap().len(), 1);

        library.remove_source(&source.id).unwrap();

        assert!(library.all_source_folders().unwrap().is_empty());
        assert!(library.source_media().unwrap().is_empty());
        assert_eq!(library.available_media_count().unwrap(), 0);
    }

    #[test]
    fn counts_follow_the_files_and_the_directories_that_hold_them() {
        let directory = tempfile::tempdir().unwrap();
        let root = media_root(&directory, "media");
        let library = Library::open_in_memory().unwrap();
        let source = library.add_source(&root, None).unwrap();
        let stored_root = library.source(&source.id).unwrap().root_path;

        ingest(&library, &source.id, &stored_root, "Film.2020.mkv", true);
        ingest(
            &library,
            &source.id,
            &stored_root,
            "Movies/Heat.1995.mkv",
            true,
        );
        ingest(
            &library,
            &source.id,
            &stored_root,
            "Movies/Gone.1999.mkv",
            false,
        );
        ingest(
            &library,
            &source.id,
            &stored_root,
            "Shows/Season 1/Show.S01E01.mkv",
            true,
        );

        let media = library.source_media().unwrap();
        assert_eq!(
            media,
            vec![SourceMedia {
                source_id: source.id.clone(),
                available: 3,
                total: 4,
            }]
        );
        assert_eq!(library.available_media_count().unwrap(), 3);

        assert_eq!(
            library.folder_media_counts(&source.id).unwrap(),
            vec![
                (String::new(), 1),
                ("Movies".to_string(), 1),
                ("Shows/Season 1".to_string(), 1),
            ]
        );

        assert_eq!(library.count(&LibraryQuery::default()).unwrap(), 4);
    }

    #[test]
    fn an_unavailable_source_is_not_counted_as_available_media() {
        let directory = tempfile::tempdir().unwrap();
        let root = media_root(&directory, "media");
        let library = Library::open_in_memory().unwrap();
        let source = library.add_source(&root, None).unwrap();
        let stored_root = library.source(&source.id).unwrap().root_path;
        ingest(&library, &source.id, &stored_root, "Film.2020.mkv", true);

        library
            .set_source_state(&source.id, crate::SourceState::Disconnected)
            .unwrap();
        library
            .connection()
            .execute(
                "UPDATE files SET available = 1 WHERE source_id = ?1",
                [&source.id],
            )
            .unwrap();

        assert_eq!(
            library.available_media_count().unwrap(),
            0,
            "an unplugged drive must not count as available media"
        );
        assert_eq!(
            library.source_media().unwrap()[0].total,
            1,
            "the row and its metadata are still kept"
        );
    }

    #[test]
    fn an_unavailable_source_never_fills_continue_watching_or_item_availability() {
        let directory = tempfile::tempdir().unwrap();
        let root = media_root(&directory, "media");
        let library = Library::open_in_memory().unwrap();
        let source = library.add_source(&root, None).unwrap();
        let stored_root = library.source(&source.id).unwrap().root_path;
        ingest(&library, &source.id, &stored_root, "Film.2020.mkv", true);
        let profile = library.profiles().unwrap()[0].id.clone();
        let item = library.items(&LibraryQuery::page(1)).unwrap().remove(0);
        let session = library.start_session(&profile, &item.id).unwrap();
        library
            .checkpoint(Checkpoint {
                session: &session,
                position_ms: 1_000,
                duration_ms: 60_000,
                sequence: 1,
                seek_generation: 0,
                ended: false,
            })
            .unwrap();
        assert_eq!(library.continue_watching(&profile, 10).unwrap().len(), 1);

        library
            .set_source_state(&source.id, crate::SourceState::Disconnected)
            .unwrap();
        library
            .connection()
            .execute(
                "UPDATE files SET available = 1 WHERE source_id = ?1",
                [&source.id],
            )
            .unwrap();

        assert!(
            library.continue_watching(&profile, 10).unwrap().is_empty(),
            "a disconnected source must not fill Continue watching"
        );
        let item = library.item(&item.id, Some(&profile)).unwrap().unwrap();
        assert!(
            !item.available,
            "a disconnected source must not report an item as available"
        );
        assert_eq!(
            library.available_media_count().unwrap(),
            0,
            "a disconnected source must not count as available media"
        );
        let media = &library.source_media().unwrap()[0];
        assert_eq!(media.total, 1);
        assert_eq!(media.available, 0);
    }

    #[test]
    fn probing_an_absent_source_marks_it_unavailable_without_losing_its_rows() {
        let directory = tempfile::tempdir().unwrap();
        let root = media_root(&directory, "media");
        let library = Library::open_in_memory().unwrap();
        let source = library.add_source(&root, None).unwrap();
        let stored_root = library.source(&source.id).unwrap().root_path;
        ingest(&library, &source.id, &stored_root, "Film.2020.mkv", true);

        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(
            library.probe_source(&source.id).unwrap(),
            crate::SourceState::Missing
        );
        assert_eq!(
            library.source(&source.id).unwrap().state,
            crate::SourceState::Missing
        );
        assert_eq!(library.available_media_count().unwrap(), 0);
        assert_eq!(library.source_media().unwrap()[0].total, 1);

        std::fs::create_dir_all(&root).unwrap();
        assert_eq!(
            library.probe_source(&source.id).unwrap(),
            crate::SourceState::Available,
            "a readable root is reported as available"
        );
        assert_eq!(
            library.source(&source.id).unwrap().state,
            crate::SourceState::Missing,
            "only a scan may restore availability"
        );
    }

    #[test]
    fn every_selection_is_listed_by_source_and_path() {
        let directory = tempfile::tempdir().unwrap();
        let first = media_root(&directory, "first");
        let second = media_root(&directory, "second");
        let library = Library::open_in_memory().unwrap();
        let alpha = library.add_source(&first, Some("Alpha")).unwrap();
        let beta = library.add_source(&second, Some("Beta")).unwrap();

        library
            .configure_source_folders(&beta.id, &selection(&["Zed", "Able"]))
            .unwrap();
        library
            .configure_source_folders(&alpha.id, &selection(&["Movies"]))
            .unwrap();

        let mut expected = vec![
            SourceFolder {
                source_id: alpha.id.clone(),
                relative_path: "Movies".to_string(),
            },
            SourceFolder {
                source_id: beta.id.clone(),
                relative_path: "Able".to_string(),
            },
            SourceFolder {
                source_id: beta.id.clone(),
                relative_path: "Zed".to_string(),
            },
        ];
        expected.sort_by(|left, right| {
            left.source_id
                .cmp(&right.source_id)
                .then_with(|| left.relative_path.cmp(&right.relative_path))
        });
        assert_eq!(library.all_source_folders().unwrap(), expected);
    }

    #[test]
    fn a_watched_path_is_covered_only_by_a_selection_above_it() {
        let selections = selection(&["Movies", "Shows/Season 1"]);
        assert!(selection_covers(&selections, "Movies/Heat.1995.mkv"));
        assert!(selection_covers(
            &selections,
            "Shows/Season 1/Show.S01E01.mkv"
        ));
        assert!(!selection_covers(
            &selections,
            "Shows/Season 2/Show.S02E01.mkv"
        ));
        assert!(!selection_covers(&selections, "Moviesque/Film.mkv"));
        assert!(selection_covers(&selection(&[""]), "anything/at/all.mkv"));
        assert!(!selection_covers(&[], "anything/at/all.mkv"));
    }
}
