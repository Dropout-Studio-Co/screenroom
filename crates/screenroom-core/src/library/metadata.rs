use rusqlite::params;

use super::Library;
use super::media_info::under_root;
use crate::error::Result;
use crate::nfo::{self, NfoKind};

const ENRICHMENT_LIMIT: i64 = 20_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EnrichmentSummary {
    pub files: u64,

    pub works: u64,

    pub episodes: u64,
}

#[derive(Debug)]
struct Candidate {
    details: nfo::NfoDetails,
    work_id: String,
    episode_id: Option<String>,
    title: String,
    year: Option<i64>,
    overview: String,
    genres: String,
    manual_match: bool,
}

impl Library {
    pub fn enrich_from_sidecars(&self, source_id: Option<&str>) -> Result<EnrichmentSummary> {
        let candidates = self.sidecar_candidates(source_id)?;
        let mut summary = EnrichmentSummary {
            files: candidates.len() as u64,
            ..EnrichmentSummary::default()
        };
        if candidates.is_empty() {
            return Ok(summary);
        }
        let mut connection = self.connection();
        let tx = connection.transaction()?;
        for candidate in candidates {
            if let Some(episode_id) = &candidate.episode_id
                && candidate.details.kind == Some(NfoKind::Episode)
            {
                let changed = tx.execute(
                    "UPDATE episodes SET title = COALESCE(NULLIF(?1, ''), title),
                       overview = COALESCE(NULLIF(?2, ''), overview),
                       metadata_origin = 'nfo', metadata_provider_id = NULL
                     WHERE id = ?3 AND (title <> COALESCE(NULLIF(?1, ''), title)
                       OR overview <> COALESCE(NULLIF(?2, ''), overview)
                       OR metadata_origin <> 'nfo')",
                    params![
                        candidate.details.title,
                        candidate.details.overview,
                        episode_id
                    ],
                )?;
                summary.episodes += changed as u64;
            }
            if candidate.details.kind == Some(NfoKind::Episode) || candidate.manual_match {
                continue;
            }
            let title = if candidate.details.title.is_empty() {
                candidate.title.clone()
            } else {
                candidate.details.title.clone()
            };
            let overview = if candidate.details.overview.is_empty() {
                candidate.overview.clone()
            } else {
                candidate.details.overview.clone()
            };
            let year = candidate.details.year.or(candidate.year);
            let known: Vec<String> = serde_json::from_str(&candidate.genres).unwrap_or_default();
            let genres = if candidate.details.genres.is_empty() {
                known.clone()
            } else {
                candidate.details.genres.clone()
            };
            if title == candidate.title
                && year == candidate.year
                && overview == candidate.overview
                && genres == known
            {
                continue;
            }
            let genres = serde_json::to_string(&genres).unwrap_or_else(|_| "[]".to_string());
            tx.execute(
                "UPDATE works SET title = ?1, year = ?2, overview = ?3, genres = ?4,
                   metadata_origin = 'nfo', metadata_revision = metadata_revision + 1
                 WHERE id = ?5",
                params![title, year, overview, genres, candidate.work_id],
            )?;
            summary.works += 1;
        }
        tx.commit()?;
        Ok(summary)
    }

    fn sidecar_candidates(&self, source_id: Option<&str>) -> Result<Vec<Candidate>> {
        let connection = self.connection();
        let mut statement = connection.prepare(
            "SELECT f.relative_path, s.root_path, w.id, w.title, w.year, w.overview, w.genres,
               w.manual_match,
               (SELECT ep.id FROM episodes ep JOIN file_episodes fe ON fe.episode_id = ep.id
                 WHERE fe.file_id = f.id ORDER BY fe.sequence LIMIT 1) episode_id
             FROM files f
             JOIN editions e ON e.id = f.edition_id
             JOIN works w ON w.id = e.work_id
             JOIN sources s ON s.id = f.source_id
             WHERE f.available = 1 AND (?1 IS NULL OR f.source_id = ?1)
             ORDER BY f.id
             LIMIT ?2",
        )?;
        let rows = statement.query_map(params![source_id, ENRICHMENT_LIMIT], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<i64>>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, i64>(7)? == 1,
                row.get::<_, Option<String>>(8)?,
            ))
        })?;
        let mut candidates = Vec::new();
        for row in rows {
            let (relative, root, work_id, title, year, overview, genres, manual_match, episode_id) =
                row?;
            let Some(path) = under_root(&root, &relative) else {
                continue;
            };
            let Some((_, details)) = nfo::read(&path) else {
                continue;
            };
            candidates.push(Candidate {
                details,
                work_id,
                episode_id,
                title,
                year,
                overview,
                genres,
                manual_match,
            });
        }
        Ok(candidates)
    }
}

#[cfg(test)]
mod tests {
    use crate::SCHEMA_VERSION;
    use crate::library::{Ingest, Library};
    use crate::{parse_filename, schema};

    fn library_at(names: &[&str]) -> (tempfile::TempDir, Library, String) {
        let directory = tempfile::tempdir().expect("tempdir");
        let library = Library::open_in_memory().expect("library");
        let source = library.add_source(directory.path(), None).expect("source");
        for name in names {
            let path = directory.path().join(name);
            std::fs::create_dir_all(path.parent().unwrap()).expect("folders");
            std::fs::write(&path, b"video").expect("write");
            let parsed = parse_filename(name, None);
            library
                .ingest_file(Ingest {
                    source_id: &source.id,
                    relative_path: name,
                    fingerprint: name,
                    size: 5,
                    modified_ms: 1,
                    scan_id: "scan-1",
                    parsed: Some(&parsed),
                    expected_root: Some(&source.root_path),
                })
                .expect("ingest");
        }
        let source_id = source.id;
        (directory, library, source_id)
    }

    fn details(library: &Library, relative: &str) -> (String, Option<i64>, String, Vec<String>) {
        let item = library
            .items(&crate::library::LibraryQuery {
                text: relative.to_string(),
                limit: 10,
                ..crate::library::LibraryQuery::default()
            })
            .expect("query")
            .into_iter()
            .next()
            .expect("item");
        (item.title, item.year, item.overview, item.genres)
    }

    #[test]
    fn a_sidecar_beside_a_film_becomes_its_real_title_and_genres() {
        let (directory, library, source_id) = library_at(&["Some.Release.2019.mkv"]);
        std::fs::write(
            directory.path().join("Some.Release.2019.nfo"),
            b"<movie><title>The Real Title</title><year>2019</year><plot>A plot.</plot>\
              <genre>Drama</genre><genre>Thriller</genre></movie>",
        )
        .unwrap();

        let summary = library.enrich_from_sidecars(Some(&source_id)).unwrap();
        assert_eq!(summary.works, 1);
        assert_eq!(summary.episodes, 0);

        let (title, year, overview, genres) = details(&library, "Some.Release.2019.mkv");
        assert_eq!(title, "The Real Title");
        assert_eq!(year, Some(2019));
        assert_eq!(overview, "A plot.");
        assert_eq!(genres, vec!["Drama", "Thriller"]);
        assert_eq!(library.known_genres().unwrap(), vec!["Drama", "Thriller"]);

        assert_eq!(
            library
                .enrich_from_sidecars(Some(&source_id))
                .unwrap()
                .works,
            0
        );
        assert_eq!(
            details(&library, "Some.Release.2019.mkv").0,
            "The Real Title"
        );
    }

    #[test]
    fn a_folder_wide_series_sidecar_names_the_series_and_an_episode_sidecar_names_the_episode() {
        let (directory, library, _source_id) =
            library_at(&["Show/Show.S01E02.mkv", "Show/Show.S01E03.mkv"]);
        std::fs::write(
            directory.path().join("Show/tvshow.nfo"),
            b"<tvshow><title>The Show</title><genre>Comedy</genre></tvshow>",
        )
        .unwrap();
        std::fs::write(
            directory.path().join("Show/Show.S01E02.nfo"),
            b"<episodedetails><title>Second</title><plot>A second episode.</plot></episodedetails>",
        )
        .unwrap();

        let summary = library.enrich_from_sidecars(None).unwrap();
        assert!(summary.works >= 1);
        assert_eq!(summary.episodes, 1);

        let first = details(&library, "Show.S01E02.mkv");
        assert_eq!(first.0, "The Show");
        assert_eq!(first.3, vec!["Comedy"]);
        let item = library
            .items(&crate::library::LibraryQuery {
                text: "Show.S01E02.mkv".to_string(),
                limit: 10,
                ..crate::library::LibraryQuery::default()
            })
            .unwrap()
            .remove(0);
        assert_eq!(item.episode_title, "Second");
        assert_eq!(item.episode_overview, "A second episode.");
        assert_eq!(item.metadata_origin, "nfo");
    }

    #[test]
    fn a_manual_match_and_a_file_without_a_sidecar_are_left_alone() {
        let (directory, library, source_id) = library_at(&["Chosen.2020.mkv", "Plain.2021.mkv"]);
        std::fs::write(
            directory.path().join("Chosen.2020.nfo"),
            b"<movie><title>Not Chosen</title><genre>Horror</genre></movie>",
        )
        .unwrap();
        library
            .connection()
            .execute(
                "UPDATE works SET manual_match = 1 WHERE id = (SELECT w.id FROM works w
                   JOIN editions e ON e.work_id = w.id JOIN files f ON f.edition_id = e.id
                   WHERE f.relative_path = 'Chosen.2020.mkv')",
                [],
            )
            .unwrap();

        let summary = library.enrich_from_sidecars(Some(&source_id)).unwrap();
        assert_eq!(summary.works, 0);
        let (_, _, _, genres) = details(&library, "Chosen.2020.mkv");
        assert!(genres.is_empty());
        assert!(library.known_genres().unwrap().is_empty());
    }

    #[test]
    fn a_schema_five_library_gains_the_origin_column_without_losing_rows() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("library.db");
        {
            let connection = rusqlite::Connection::open(&path).unwrap();
            for statement in schema::CREATE {
                connection.execute(statement, []).unwrap();
            }
            connection
                .execute("ALTER TABLE works DROP COLUMN metadata_origin", [])
                .unwrap();
            connection
                .execute("INSERT INTO profiles(id, name) VALUES('p', 'Me')", [])
                .unwrap();
            connection
                .execute(
                    "INSERT INTO works(id, identity_key, title, kind) VALUES('w', 'k', 'Kept', 'movie')",
                    [],
                )
                .unwrap();
            connection.pragma_update(None, "user_version", 5).unwrap();
        }
        let library = Library::open(&path).unwrap();
        let version: i64 = library
            .connection()
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
        let (title, origin): (String, String) = library
            .connection()
            .query_row(
                "SELECT title, metadata_origin FROM works WHERE id = 'w'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(title, "Kept");
        assert_eq!(origin, "legacy");
    }
}
