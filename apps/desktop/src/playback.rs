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
    ) -> Result<bool, CoreError> {
        if self.closed || duration_ms <= 0 || position_ms < 0 {
            return Ok(false);
        }

        let position_ms = position_ms.min(duration_ms);
        let due = ended
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

    #[test]
    fn resume_policy_constants_stay_consistent() {
        const { assert!(END_TOLERANCE_MS <= CHECKPOINT_INTERVAL_MS) };
        const { assert!(CHECKPOINT_INTERVAL_MS >= 1_000) };
    }
}
