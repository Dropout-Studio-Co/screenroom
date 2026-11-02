use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaKind {
    Movie,

    Series,

    Unknown,
}

impl MediaKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Movie => "movie",
            Self::Series => "series",
            Self::Unknown => "unknown",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "movie" => Some(Self::Movie),
            "series" => Some(Self::Series),
            "unknown" => Some(Self::Unknown),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SourceState {
    Available,

    Disconnected,

    PermissionLost,

    Missing,

    Removed,
}

impl SourceState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::Disconnected => "disconnected",
            Self::PermissionLost => "permissionLost",
            Self::Missing => "missing",
            Self::Removed => "removed",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "available" => Some(Self::Available),
            "disconnected" => Some(Self::Disconnected),
            "permissionLost" => Some(Self::PermissionLost),
            "missing" => Some(Self::Missing),
            "removed" => Some(Self::Removed),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Source {
    pub id: String,

    pub label: String,

    pub root_path: String,

    pub state: SourceState,

    pub last_scan_ms: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profile {
    pub id: String,

    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LibraryItem {
    pub id: String,

    pub work_id: String,

    pub edition_id: String,

    pub source_id: String,

    pub title: String,

    pub filename: String,

    pub relative_path: String,

    pub kind: MediaKind,

    pub overview: String,

    pub episode_title: String,

    pub episode_overview: String,

    pub genres: Vec<String>,

    pub year: Option<i64>,

    pub season: Option<i64>,

    pub episode: Option<i64>,

    pub episode_end: Option<i64>,

    pub duration_ms: i64,

    pub position_ms: i64,

    pub added_ms: i64,

    pub available: bool,

    pub watched: bool,

    pub favorite: bool,

    pub watchlisted: bool,

    pub manual_match: bool,

    pub metadata_origin: String,
}

impl LibraryItem {
    pub fn episode_label(&self) -> String {
        match (self.kind, self.season, self.episode) {
            (MediaKind::Series, Some(season), Some(episode)) => {
                let mut label = format!("S{season:02}E{episode:02}");
                if let Some(end) = self.episode_end
                    && end != episode
                {
                    label.push_str(&format!("\u{2013}E{end:02}"));
                }
                label
            }
            _ => String::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Recommendation {
    pub item: LibraryItem,

    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaybackSession {
    pub id: String,

    pub profile_id: String,

    pub file_id: String,

    pub edition_id: String,

    pub generation: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SegmentMarker {
    pub id: String,

    pub edition_id: String,

    pub kind: String,

    pub start_ms: i64,

    pub end_ms: i64,

    pub duration_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackgroundJob {
    pub id: String,

    pub kind: String,

    pub target: String,

    pub status: String,

    pub attempts: i64,

    pub priority: i64,

    pub next_retry_ms: i64,

    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn episode(kind: MediaKind, season: Option<i64>, episode: Option<i64>) -> LibraryItem {
        LibraryItem {
            id: "item".to_string(),
            work_id: "work".to_string(),
            edition_id: "edition".to_string(),
            source_id: "source".to_string(),
            title: "Northern Lights".to_string(),
            filename: "Northern Lights S02E07.mkv".to_string(),
            relative_path: "Northern Lights/Season 2/Northern Lights S02E07.mkv".to_string(),
            kind,
            overview: String::new(),
            episode_title: String::new(),
            episode_overview: String::new(),
            genres: Vec::new(),
            year: None,
            season,
            episode,
            episode_end: None,
            duration_ms: 0,
            position_ms: 0,
            added_ms: 0,
            available: true,
            watched: false,
            favorite: false,
            watchlisted: false,
            manual_match: false,
            metadata_origin: String::new(),
        }
    }

    #[test]
    fn media_kind_round_trips_through_its_stored_spelling() {
        for kind in [MediaKind::Movie, MediaKind::Series, MediaKind::Unknown] {
            assert_eq!(MediaKind::parse(kind.as_str()), Some(kind));
        }
        assert_eq!(MediaKind::parse("series"), Some(MediaKind::Series));
        assert_eq!(MediaKind::parse("Series"), None);
        assert_eq!(MediaKind::parse(""), None);
        assert_eq!(MediaKind::parse("episode"), None);
    }

    #[test]
    fn source_state_round_trips_through_its_stored_spelling() {
        let all = [
            SourceState::Available,
            SourceState::Disconnected,
            SourceState::PermissionLost,
            SourceState::Missing,
            SourceState::Removed,
        ];
        for state in all {
            assert_eq!(SourceState::parse(state.as_str()), Some(state));
        }
        assert_eq!(SourceState::PermissionLost.as_str(), "permissionLost");
        assert_eq!(SourceState::parse("permission_lost"), None);
        assert_eq!(SourceState::parse("available "), None);
        assert_ne!(SourceState::Missing, SourceState::Disconnected);
    }

    #[test]
    fn an_episode_label_is_zero_padded_and_only_exists_for_an_episode() {
        assert_eq!(
            episode(MediaKind::Series, Some(2), Some(7)).episode_label(),
            "S02E07"
        );
        assert_eq!(
            episode(MediaKind::Series, Some(10), Some(103)).episode_label(),
            "S10E103",
            "a season above nine must not be truncated"
        );
        assert_eq!(
            episode(MediaKind::Movie, None, None).episode_label(),
            "",
            "a film has no episode to name"
        );
        assert_eq!(
            episode(MediaKind::Unknown, Some(1), Some(1)).episode_label(),
            "",
            "an unclassified file keeps the plain play label"
        );
        assert_eq!(
            episode(MediaKind::Series, Some(1), None).episode_label(),
            "",
            "a series entry without an episode number has nothing to name"
        );
        assert_eq!(
            episode(MediaKind::Series, None, Some(4)).episode_label(),
            "",
            "an episode without its season is not fabricated into one"
        );
    }

    #[test]
    fn a_multi_episode_file_reads_as_a_range_and_a_single_one_does_not() {
        let mut item = episode(MediaKind::Series, Some(2), Some(7));
        item.episode_end = Some(9);
        assert_eq!(item.episode_label(), "S02E07\u{2013}E09");

        item.episode_end = Some(7);
        assert_eq!(
            item.episode_label(),
            "S02E07",
            "an end equal to the start is not a range"
        );

        item.episode_end = None;
        assert_eq!(item.episode_label(), "S02E07");
    }
}
