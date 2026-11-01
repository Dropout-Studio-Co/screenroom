use std::path::Path;
use std::time::{Duration, Instant};

use screenroom_player::{Player, PlayerSnapshot};

pub const PREVIEW_DELAY: Duration = Duration::from_millis(600);

pub const PREVIEW_MAX_DIMENSION: u32 = 960;

pub const PREVIEW_MAX: Duration = Duration::from_secs(20);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewRefusal {
    ReducedMotion,

    Playing,

    Unavailable,

    NoPlayer,
}

impl PreviewRefusal {
    pub fn reason(self) -> &'static str {
        match self {
            Self::ReducedMotion => "Previews are off because Reduce motion is on.",
            Self::Playing => "A preview never runs while something is playing.",
            Self::Unavailable => "That file cannot be read, so its poster stays.",
            Self::NoPlayer => "Playback is unavailable on this machine, so previews are still.",
        }
    }
}

pub fn may_preview(
    reduced_motion: bool,
    playing: bool,
    available: bool,
    player_available: bool,
) -> Result<(), PreviewRefusal> {
    if !player_available {
        return Err(PreviewRefusal::NoPlayer);
    }
    if !available {
        return Err(PreviewRefusal::Unavailable);
    }
    if playing {
        return Err(PreviewRefusal::Playing);
    }
    if reduced_motion {
        return Err(PreviewRefusal::ReducedMotion);
    }
    Ok(())
}

pub fn preview_position_ms(duration_ms: i64) -> i64 {
    if duration_ms <= 0 {
        return 30_000;
    }
    (duration_ms / 5)
        .clamp(1_000, 300_000)
        .min((duration_ms / 2).max(0))
}

pub struct Preview {
    player: Player,

    file_id: String,

    started: Instant,

    moved: bool,
}

impl Preview {
    pub fn start(file_id: &str, path: &Path, _duration_ms: i64) -> Result<Self, String> {
        let player = Player::start_with_max_dimension(PREVIEW_MAX_DIMENSION)
            .map_err(|error| error.to_string())?;
        player.set_volume(0.0);
        player.load_at(path, None);
        Ok(Self {
            player,
            file_id: file_id.to_string(),
            started: Instant::now(),
            moved: false,
        })
    }

    pub fn tick(&mut self) -> PlayerSnapshot {
        let snapshot = self.player.snapshot();
        if self.moved || snapshot.duration_ms() <= 0 {
            return snapshot;
        }
        self.moved = true;
        let target = preview_position_ms(snapshot.duration_ms());
        if target <= 1 {
            return snapshot;
        }
        self.player.seek(target as f64 / 1_000.0);
        self.player.snapshot()
    }

    pub fn file_id(&self) -> &str {
        &self.file_id
    }

    pub fn snapshot(&self) -> PlayerSnapshot {
        self.player.snapshot()
    }

    pub fn frame_ticks(&self) -> screenroom_player::FrameTicks {
        self.player.frame_ticks()
    }

    pub fn expired(&self) -> bool {
        self.started.elapsed() >= PREVIEW_MAX
    }

    pub fn ended(&self) -> bool {
        self.snapshot().ended
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_preview_is_refused_whenever_it_could_get_in_the_way() {
        assert_eq!(may_preview(false, false, true, true), Ok(()));
        assert_eq!(
            may_preview(true, false, true, true),
            Err(PreviewRefusal::ReducedMotion)
        );
        assert_eq!(
            may_preview(false, true, true, true),
            Err(PreviewRefusal::Playing)
        );
        assert_eq!(
            may_preview(false, false, false, true),
            Err(PreviewRefusal::Unavailable)
        );
        assert_eq!(
            may_preview(false, false, true, false),
            Err(PreviewRefusal::NoPlayer)
        );
        assert_eq!(
            may_preview(true, true, false, false),
            Err(PreviewRefusal::NoPlayer)
        );
        assert!(!PreviewRefusal::ReducedMotion.reason().is_empty());
    }

    #[test]
    fn a_preview_starts_inside_the_file_and_never_at_its_opening() {
        assert_eq!(preview_position_ms(0), 30_000);
        assert_eq!(preview_position_ms(30_000), 6_000);
        assert_eq!(preview_position_ms(600_000), 120_000);
        assert_eq!(preview_position_ms(10_800_000), 300_000);
        assert_eq!(preview_position_ms(4_000), 1_000);
        assert!(
            preview_position_ms(4_000) <= 4_000,
            "a short clip starts inside itself, not past its end"
        );
    }

    #[test]
    fn the_delay_is_long_enough_to_not_fire_while_the_pointer_crosses_a_row() {
        assert!(PREVIEW_DELAY >= Duration::from_millis(400));
        assert!(PREVIEW_MAX > PREVIEW_DELAY);
    }
}
