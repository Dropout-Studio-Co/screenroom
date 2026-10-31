use std::collections::HashSet;

use super::{Library, LibraryQuery, Sort};
use crate::domain::{LibraryItem, MediaKind, Recommendation};
use crate::error::Result;

pub const MAX_RECOMMENDATIONS: i64 = 60;

const IN_PROGRESS_SAMPLE: i64 = 10;

impl Library {
    pub fn recommendations(&self, profile_id: &str, limit: i64) -> Result<Vec<Recommendation>> {
        let limit = limit.clamp(1, MAX_RECOMMENDATIONS) as usize;
        let mut out: Vec<Recommendation> = Vec::new();
        let mut seen_works: HashSet<String> = HashSet::new();
        let mut seen_files: HashSet<String> = HashSet::new();

        let in_progress = self.continue_watching(profile_id, IN_PROGRESS_SAMPLE)?;

        let mut starters = in_progress.clone();
        starters.extend(self.items(&LibraryQuery {
            profile_id: Some(profile_id.to_string()),
            watched: Some(true),
            available_only: true,
            sort: Sort::Progress,
            limit: 10,
            ..LibraryQuery::default()
        })?);
        for item in starters
            .iter()
            .filter(|item| item.kind == MediaKind::Series)
        {
            let Some(next) = self.next_episode(&item.id, Some(profile_id))? else {
                continue;
            };
            push(
                &mut out,
                &mut seen_works,
                &mut seen_files,
                next,
                format!("Next episode of {}", item.title),
            );
        }
        seen_works.extend(in_progress.iter().map(|item| item.work_id.clone()));

        self.offer(
            &mut out,
            &mut seen_works,
            &mut seen_files,
            LibraryQuery {
                profile_id: Some(profile_id.to_string()),
                watchlist_only: true,
                watched: Some(false),
                available_only: true,
                group_by_work: true,
                sort: Sort::Added,
                limit: 20,
                ..LibraryQuery::default()
            },
            "On your watchlist",
        )?;

        let mut genres: Vec<String> = Vec::new();
        for item in &in_progress {
            for genre in &item.genres {
                if !genres.iter().any(|known| known == genre) {
                    genres.push(genre.clone());
                }
            }
        }
        for genre in genres.iter().take(2) {
            self.offer(
                &mut out,
                &mut seen_works,
                &mut seen_files,
                LibraryQuery {
                    profile_id: Some(profile_id.to_string()),
                    genre: Some(genre.clone()),
                    watched: Some(false),
                    available_only: true,
                    group_by_work: true,
                    sort: Sort::Title,
                    limit: 8,
                    ..LibraryQuery::default()
                },
                &format!("More {genre}"),
            )?;
        }

        self.offer(
            &mut out,
            &mut seen_works,
            &mut seen_files,
            LibraryQuery {
                profile_id: Some(profile_id.to_string()),
                watched: Some(false),
                available_only: true,
                group_by_work: true,
                sort: Sort::Added,
                limit: 12,
                ..LibraryQuery::default()
            },
            "Recently added",
        )?;

        out.truncate(limit);
        Ok(out)
    }

    #[allow(clippy::too_many_arguments)]
    fn offer(
        &self,
        out: &mut Vec<Recommendation>,
        seen_works: &mut HashSet<String>,
        seen_files: &mut HashSet<String>,
        query: LibraryQuery,
        reason: &str,
    ) -> Result<()> {
        for item in self.items(&query)? {
            push(out, seen_works, seen_files, item, reason.to_string());
        }
        Ok(())
    }
}

fn push(
    out: &mut Vec<Recommendation>,
    seen_works: &mut HashSet<String>,
    seen_files: &mut HashSet<String>,
    item: LibraryItem,
    reason: String,
) {
    if !seen_works.insert(item.work_id.clone()) {
        return;
    }
    if !seen_files.insert(item.id.clone()) {
        return;
    }
    out.push(Recommendation { item, reason });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Ingest, parse_filename};

    fn library() -> (tempfile::TempDir, Library, String) {
        let directory = tempfile::tempdir().expect("tempdir");
        let library = Library::open_in_memory().expect("library");
        library
            .add_source(directory.path(), None)
            .expect("source source");
        let profile = library.profiles().expect("profiles")[0].id.clone();
        (directory, library, profile)
    }

    fn ingest(library: &Library, name: &str) -> String {
        let source = &library.sources().expect("sources")[0];
        let parsed = parse_filename(name, None);
        library
            .ingest_file(Ingest {
                source_id: &source.id,
                relative_path: name,
                fingerprint: name,
                size: 10,
                modified_ms: 1,
                scan_id: "scan-1",
                parsed: Some(&parsed),
                expected_root: Some(&source.root_path),
            })
            .expect("ingest")
    }

    fn set_genres(library: &Library, work_id: &str, genres: &[&str]) {
        let json = format!(
            "[{}]",
            genres
                .iter()
                .map(|genre| format!("\"{genre}\""))
                .collect::<Vec<_>>()
                .join(",")
        );
        library
            .connection()
            .execute(
                "UPDATE works SET genres = ?1 WHERE id = ?2",
                rusqlite::params![json, work_id],
            )
            .expect("genres");
    }

    #[test]
    fn the_next_episode_beats_everything_else_and_comes_with_a_reason() {
        let (directory, library, profile) = library();
        for name in ["Show.S01E01.mkv", "Show.S01E02.mkv", "Film.2020.mkv"] {
            std::fs::write(directory.path().join(name), b"video").expect("write");
            ingest(&library, name);
        }
        let first = library
            .items(&LibraryQuery {
                profile_id: Some(profile.clone()),
                text: "Show".to_string(),
                sort: Sort::Title,
                limit: 1,
                ..LibraryQuery::default()
            })
            .expect("items")[0]
            .clone();

        let session = library.start_session(&profile, &first.id).expect("session");
        library
            .checkpoint(crate::Checkpoint {
                session: &session,
                position_ms: 1_000,
                duration_ms: 2_000,
                sequence: 1,
                seek_generation: 0,
                ended: true,
            })
            .expect("checkpoint");

        let suggestions = library.recommendations(&profile, 10).expect("suggestions");
        assert!(!suggestions.is_empty());
        assert_eq!(suggestions[0].reason, "Next episode of Show");
        assert_eq!(suggestions[0].item.episode, Some(2));

        let works: HashSet<&str> = suggestions
            .iter()
            .map(|suggestion| suggestion.item.work_id.as_str())
            .collect();
        assert_eq!(works.len(), suggestions.len());
    }

    #[test]
    fn a_genre_in_progress_suggests_another_work_in_it() {
        let (directory, library, profile) = library();
        for name in [
            "Drama.One.2020.mkv",
            "Drama.Two.2021.mkv",
            "Comedy.2022.mkv",
        ] {
            std::fs::write(directory.path().join(name), b"video").expect("write");
            ingest(&library, name);
        }
        let one = library
            .item(
                &library
                    .items(&LibraryQuery {
                        text: "Drama One".to_string(),
                        limit: 1,
                        ..LibraryQuery::default()
                    })
                    .expect("items")[0]
                    .id,
                None,
            )
            .expect("read")
            .expect("present");
        set_genres(&library, &one.work_id, &["Drama"]);
        let two = library
            .items(&LibraryQuery {
                text: "Drama Two".to_string(),
                limit: 1,
                ..LibraryQuery::default()
            })
            .expect("items")[0]
            .work_id
            .clone();
        set_genres(&library, &two, &["Drama"]);

        let session = library.start_session(&profile, &one.id).expect("session");
        library
            .checkpoint(crate::Checkpoint {
                session: &session,
                position_ms: 500,
                duration_ms: 2_000,
                sequence: 1,
                seek_generation: 0,
                ended: false,
            })
            .expect("checkpoint");

        let suggestions = library.recommendations(&profile, 10).expect("suggestions");
        assert!(
            suggestions.iter().any(|suggestion| {
                suggestion.reason == "More Drama" && suggestion.item.work_id == two
            }),
            "the other Drama work should be offered: {suggestions:?}"
        );
        assert!(
            suggestions
                .iter()
                .all(|suggestion| suggestion.item.work_id != one.work_id),
            "an in-progress work must not be recommended again: {suggestions:?}"
        );
    }

    #[test]
    fn a_watched_or_missing_file_is_never_suggested() {
        let (directory, library, profile) = library();
        std::fs::write(directory.path().join("Seen.2020.mkv"), b"video").expect("write");
        let seen = ingest(&library, "Seen.2020.mkv");
        let item = library.item(&seen, None).expect("read").expect("present");
        library
            .mark_watched(&profile, &item.edition_id, true)
            .expect("watched");

        let suggestions = library.recommendations(&profile, 10).expect("suggestions");
        assert!(
            !suggestions
                .iter()
                .any(|suggestion| suggestion.item.id == seen),
            "{suggestions:?}"
        );
    }

    #[test]
    fn an_unstarted_watchlisted_show_starts_at_its_first_episode() {
        let (_directory, library, profile) = library();
        let first = ingest(&library, "Show.S01E01.mkv");
        ingest(&library, "Show.S01E02.mkv");
        let item = library.item(&first, None).unwrap().unwrap();
        library
            .set_watchlisted(&profile, &item.work_id, true)
            .unwrap();

        let suggestions = library.recommendations(&profile, 10).unwrap();
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].item.episode, Some(1));
    }
}
