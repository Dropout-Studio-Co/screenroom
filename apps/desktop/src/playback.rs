use screenroom_core::{Checkpoint, CoreError, Library, PlaybackSession};

const CHECKPOINT_INTERVAL_MS: i64 = 1_000;

const END_TOLERANCE_MS: i64 = 1_000;

#[derive(Debug)]
pub struct OpenedPlayback {
    pub tracker: PlaybackTracker,

    pub resume_from_ms: i64,
}

#[derive(Debug)]
pub struct PlaybackTracker {
    session: PlaybackSession,
    sequence: i64,
    seek_generation: i64,
    last_saved_ms: Option<i64>,
    closed: bool,
}

impl PlaybackTracker {
    pub fn open(
        library: &Library,
        profile_id: &str,
        file_id: &str,
    ) -> Result<OpenedPlayback, CoreError> {
        let item = library.item(file_id, Some(profile_id))?.ok_or_else(|| {
            CoreError::library("file_missing", "This file is not in the library.")
        })?;
        let resume_from_ms =
            if item.duration_ms > 0 && item.position_ms >= item.duration_ms - END_TOLERANCE_MS {
                0
            } else {
                item.position_ms.max(0)
            };
        let session = library.start_session(profile_id, file_id)?;
        Ok(OpenedPlayback {
            tracker: Self {
                session,
                sequence: 0,
                seek_generation: 0,
                last_saved_ms: None,
                closed: false,
            },
            resume_from_ms,
        })
    }

    pub fn seek(&mut self, library: &Library) -> Result<i64, CoreError> {
        self.seek_generation = library.begin_seek(&self.session)?;

        self.last_saved_ms = None;
        Ok(self.seek_generation)
    }

    pub fn record(
        &mut self,
        library: &Library,
        position_ms: i64,
        duration_ms: i64,
        ended: bool,
        force: bool,
    ) -> Result<bool, CoreError> {
        if self.closed || duration_ms <= 0 || position_ms < 0 {
            return Ok(false);
        }

        let position_ms = position_ms.min(duration_ms);
        let due = force
            || ended
            || match self.last_saved_ms {
                None => true,
                Some(last) => (position_ms - last).abs() >= CHECKPOINT_INTERVAL_MS,
            };
        if !due {
            return Ok(false);
        }
        self.sequence += 1;
        let written = library.checkpoint(Checkpoint {
            session: &self.session,
            position_ms,
            duration_ms,
            sequence: self.sequence,
            seek_generation: self.seek_generation,
            ended,
        })?;
        if written {
            self.last_saved_ms = Some(position_ms);

            self.closed = ended;
        }
        Ok(written)
    }

    pub fn close(self, library: &Library) -> Result<(), CoreError> {
        if self.closed {
            return Ok(());
        }
        library.end_session(&self.session)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use screenroom_core::{Ingest, Library, LibraryQuery, parse_filename};

    #[test]
    fn resume_policy_constants_stay_consistent() {
        const { assert!(END_TOLERANCE_MS <= CHECKPOINT_INTERVAL_MS) };
        const { assert!(CHECKPOINT_INTERVAL_MS >= 1_000) };
    }

    /// A library holding one film, already left at `position_ms` of `duration_ms`.
    fn watched_film(
        position_ms: i64,
        duration_ms: i64,
    ) -> (tempfile::TempDir, Library, String, String) {
        let directory = tempfile::tempdir().expect("temp dir");
        let library = Library::open_in_memory().expect("library");
        let source = library.add_source(directory.path(), None).expect("source");
        let name = "Film (2020).mkv";
        std::fs::write(directory.path().join(name), b"video").expect("write");
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
        let file_id = library.items(&LibraryQuery::page(10)).expect("items")[0]
            .id
            .clone();
        let profile_id = library.profiles().expect("profiles").remove(0).id;

        if duration_ms > 0 {
            let session = library
                .start_session(&profile_id, &file_id)
                .expect("session");
            library
                .checkpoint(Checkpoint {
                    session: &session,
                    position_ms,
                    duration_ms,
                    sequence: 1,
                    seek_generation: 0,
                    ended: false,
                })
                .expect("checkpoint");
            library.end_session(&session).expect("end session");
        }
        (directory, library, profile_id, file_id)
    }

    #[test]
    fn a_film_left_midway_resumes_where_it_stopped() {
        let (_directory, library, profile_id, file_id) = watched_film(4_200, 60_000);
        let opened = PlaybackTracker::open(&library, &profile_id, &file_id).expect("open");
        assert_eq!(opened.resume_from_ms, 4_200);
        opened.tracker.close(&library).expect("close");
    }

    #[test]
    fn a_film_left_at_its_very_end_starts_over_instead_of_at_the_credits() {
        for last_look in [60_000, 59_500, 59_001] {
            let (_directory, library, profile_id, file_id) = watched_film(last_look, 60_000);
            let opened = PlaybackTracker::open(&library, &profile_id, &file_id).expect("open");
            assert_eq!(
                opened.resume_from_ms, 0,
                "{last_look} ms of a 60 s film is the end, not a resume point"
            );
            opened.tracker.close(&library).expect("close");
        }

        let (_directory, library, profile_id, file_id) = watched_film(58_500, 60_000);
        let opened = PlaybackTracker::open(&library, &profile_id, &file_id).expect("open");
        assert_eq!(
            opened.resume_from_ms, 58_500,
            "just before the tolerance is still a real resume point"
        );
    }

    #[test]
    fn a_file_with_no_recorded_progress_starts_from_the_beginning() {
        let (_directory, library, profile_id, file_id) = watched_film(0, 0);
        let opened = PlaybackTracker::open(&library, &profile_id, &file_id).expect("open");
        assert_eq!(opened.resume_from_ms, 0);
        let item = library
            .item(&file_id, Some(&profile_id))
            .expect("item")
            .expect("present");
        assert_eq!(item.duration_ms, 0, "nothing has probed this file yet");
        assert_eq!(item.position_ms, 0);
        opened.tracker.close(&library).expect("close");
    }

    #[test]
    fn progress_is_written_once_a_second_and_immediately_when_forced() {
        let (_directory, library, profile_id, file_id) = watched_film(0, 0);
        let mut tracker = PlaybackTracker::open(&library, &profile_id, &file_id)
            .expect("open")
            .tracker;

        assert!(
            tracker
                .record(&library, 1_000, 60_000, false, false)
                .expect("first"),
            "the first checkpoint of a session is always due"
        );
        assert!(
            !tracker
                .record(&library, 1_400, 60_000, false, false)
                .expect("too soon"),
            "half a second later is not worth a write"
        );
        assert!(
            tracker
                .record(&library, 2_200, 60_000, false, false)
                .expect("due"),
            "a second after the last write is due"
        );
        assert!(
            tracker
                .record(&library, 2_300, 60_000, false, true)
                .expect("forced"),
            "quit forces the last position out"
        );

        let stored = library
            .item(&file_id, Some(&profile_id))
            .expect("item")
            .expect("present");
        assert_eq!(stored.position_ms, 2_300);
        assert_eq!(stored.duration_ms, 60_000);
        assert!(!stored.watched);
    }

    #[test]
    fn an_ending_checkpoint_closes_the_session_and_a_closed_session_writes_nothing() {
        let (_directory, library, profile_id, file_id) = watched_film(0, 0);
        let mut tracker = PlaybackTracker::open(&library, &profile_id, &file_id)
            .expect("open")
            .tracker;

        assert!(
            tracker
                .record(&library, 60_000, 60_000, true, false)
                .expect("ended")
        );
        assert!(
            !tracker
                .record(&library, 61_000, 60_000, false, true)
                .expect("after the end"),
            "a finished session must not accept more progress"
        );
        tracker.close(&library).expect("closing twice is harmless");

        let stored = library
            .item(&file_id, Some(&profile_id))
            .expect("item")
            .expect("present");
        assert!(stored.watched, "reaching the end marks the film watched");
    }

    #[test]
    fn nonsense_progress_is_never_written() {
        let (_directory, library, profile_id, file_id) = watched_film(0, 0);
        let mut tracker = PlaybackTracker::open(&library, &profile_id, &file_id)
            .expect("open")
            .tracker;

        assert!(
            !tracker
                .record(&library, -1, 60_000, false, true)
                .expect("negative")
        );
        assert!(
            !tracker
                .record(&library, 1_000, 0, false, true)
                .expect("no duration")
        );
        assert!(
            tracker
                .record(&library, 90_000, 60_000, false, true)
                .expect("clamped"),
            "a position past the reported end is clamped, not discarded"
        );
        let stored = library
            .item(&file_id, Some(&profile_id))
            .expect("item")
            .expect("present");
        assert_eq!(stored.position_ms, 60_000);
    }
}
