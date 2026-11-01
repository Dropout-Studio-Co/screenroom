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
