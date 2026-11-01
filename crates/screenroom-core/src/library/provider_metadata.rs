use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

use super::Library;
use super::artwork::MAX_ARTWORK_BYTES;
use super::provider_tmdb::{CatalogueEpisode, CatalogueSeason, CatalogueWork, ProviderImage};
use crate::error::{CoreError, Result};

pub const METADATA_ORIGIN_PROVIDER: &str = "provider";

pub const ARTWORK_RETENTION_SETTING: &str = "metadataArtworkRetention";

pub const MAX_TITLE_CHARS: usize = 300;

pub const MAX_OVERVIEW_CHARS: usize = 4_000;

pub const MAX_GENRES: usize = 20;

pub const EARLIEST_YEAR: i64 = 1870;

pub const LATEST_YEAR: i64 = 2100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetadataOrigin {
    Legacy,

    Filename,

    Nfo,

    Manual,

    Provider,
}

impl MetadataOrigin {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Legacy => "legacy",
            Self::Filename => "filename",
            Self::Nfo => "nfo",
            Self::Manual => "manual",
            Self::Provider => METADATA_ORIGIN_PROVIDER,
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "filename" => Self::Filename,
            "nfo" => Self::Nfo,
            "manual" => Self::Manual,
            "provider" => Self::Provider,
            _ => Self::Legacy,
        }
    }

    pub fn precedence(self) -> u8 {
        match self {
            Self::Legacy => 0,
            Self::Filename => 1,
            Self::Provider => 2,
            Self::Nfo => 3,
            Self::Manual => 4,
        }
    }

    pub fn accepts_background_provider(self) -> bool {
        matches!(self, Self::Legacy | Self::Filename | Self::Provider)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReconcileMode {
    Background,

    OwnerRequested,
}

pub fn replacement_allowed(
    origin: MetadataOrigin,
    manual_match: bool,
    mode: ReconcileMode,
) -> bool {
    if manual_match || origin == MetadataOrigin::Manual {
        return false;
    }
    if origin == MetadataOrigin::Nfo && mode == ReconcileMode::Background {
        return false;
    }
    true
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetadataSnapshot {
    pub origin: MetadataOrigin,

    pub manual_match: bool,

    pub title: String,

    pub year: Option<i64>,

    pub overview: String,

    pub genres: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MetadataPatch {
    pub title: Option<String>,

    pub year: Option<i64>,

    pub overview: Option<String>,

    pub genres: Option<Vec<String>>,
}

impl MetadataPatch {
    pub fn from_work(work: &CatalogueWork) -> Self {
        Self {
            title: Some(work.title.clone()),
            year: work.year,
            overview: Some(work.overview.clone()),
            genres: Some(work.genres.clone()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconcileDecision {
    pub applied: bool,

    pub patch: MetadataPatch,

    pub origin: MetadataOrigin,

    pub blocked_by: Option<MetadataOrigin>,
}

fn clean_text(value: &str, max_chars: usize) -> String {
    let trimmed = value.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }
    trimmed
        .chars()
        .take(max_chars)
        .collect::<String>()
        .trim_end()
        .to_string()
}

fn clean_genres(genres: &[String]) -> Vec<String> {
    let mut cleaned: Vec<String> = Vec::new();
    for genre in genres {
        let name = clean_text(genre, 60);
        if name.is_empty() {
            continue;
        }
        if cleaned
            .iter()
            .any(|known| known.eq_ignore_ascii_case(&name))
        {
            continue;
        }
        cleaned.push(name);
        if cleaned.len() == MAX_GENRES {
            break;
        }
    }
    cleaned
}

pub fn reconcile(
    current: &MetadataSnapshot,
    candidate: &MetadataPatch,
    mode: ReconcileMode,
) -> ReconcileDecision {
    if !replacement_allowed(current.origin, current.manual_match, mode) {
        let blocked_by = if current.manual_match || current.origin == MetadataOrigin::Manual {
            MetadataOrigin::Manual
        } else {
            MetadataOrigin::Nfo
        };
        return ReconcileDecision {
            applied: false,
            patch: MetadataPatch::default(),
            origin: current.origin,
            blocked_by: Some(blocked_by),
        };
    }

    let mut patch = MetadataPatch::default();

    if let Some(title) = candidate.title.as_deref() {
        let title = clean_text(title, MAX_TITLE_CHARS);
        if !title.is_empty() && title != current.title {
            patch.title = Some(title);
        }
    }

    if let Some(year) = candidate.year
        && (EARLIEST_YEAR..=LATEST_YEAR).contains(&year)
        && Some(year) != current.year
    {
        patch.year = Some(year);
    }

    if let Some(overview) = candidate.overview.as_deref() {
        let overview = clean_text(overview, MAX_OVERVIEW_CHARS);
        if !overview.is_empty() && overview != current.overview {
            patch.overview = Some(overview);
        }
    }

    if let Some(genres) = candidate.genres.as_deref() {
        let genres = clean_genres(genres);
        let unchanged = genres.len() == current.genres.len()
            && genres
                .iter()
                .zip(current.genres.iter())
                .all(|(left, right)| left.eq_ignore_ascii_case(right));
        if !genres.is_empty() && !unchanged {
            patch.genres = Some(genres);
        }
    }

    let applied = patch != MetadataPatch::default();
    ReconcileDecision {
        applied,
        patch,
        origin: if applied {
            MetadataOrigin::Provider
        } else {
            current.origin
        },
        blocked_by: None,
    }
}

pub fn jpeg_dimensions(bytes: &[u8]) -> Option<(i64, i64)> {
    if bytes.len() < 4 || bytes[0] != 0xFF || bytes[1] != 0xD8 {
        return None;
    }
    let mut index = 2_usize;
    while index + 3 < bytes.len() {
        if bytes[index] != 0xFF {
            index += 1;
            continue;
        }
        let marker = bytes[index + 1];
        index += 2;
        if marker == 0xD8 || marker == 0x01 || (0xD0..=0xD7).contains(&marker) {
            continue;
        }
        if marker == 0xD9 {
            return None;
        }
        if index + 2 > bytes.len() {
            return None;
        }
        let length = u16::from_be_bytes([bytes[index], bytes[index + 1]]) as usize;
        if length < 2 || index + length > bytes.len() {
            return None;
        }
        let is_frame_header =
            (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC);
        if is_frame_header {
            if length < 7 || index + 6 >= bytes.len() {
                return None;
            }
            let height = u16::from_be_bytes([bytes[index + 3], bytes[index + 4]]) as i64;
            let width = u16::from_be_bytes([bytes[index + 5], bytes[index + 6]]) as i64;
            return (width > 0 && height > 0).then_some((width, height));
        }
        index += length;
    }
    None
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtworkRetention {
    DoNotStore,

    KeepLocally,
}

impl Library {
    pub fn work_metadata(&self, work_id: &str) -> Result<MetadataSnapshot> {
        let connection = self.connection();
        let row = connection
            .query_row(
                "SELECT title, year, overview, genres, metadata_origin, manual_match
                 FROM works WHERE id = ?1",
                [work_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<i64>>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, i64>(5)?,
                    ))
                },
            )
            .optional()?;
        let Some((title, year, overview, genres, origin, manual_match)) = row else {
            return Err(CoreError::library(
                "work_missing",
                "That work is no longer in the library.",
            ));
        };
        Ok(MetadataSnapshot {
            origin: MetadataOrigin::parse(&origin),
            manual_match: manual_match == 1,
            title,
            year,
            overview,
            genres: serde_json::from_str(&genres).unwrap_or_default(),
        })
    }

    pub fn set_manual_match(&self, work_id: &str, manual: bool) -> Result<()> {
        let changed = self.connection().execute(
            "UPDATE works SET manual_match = ?2,
               metadata_origin = CASE WHEN ?2 = 1 THEN 'manual'
                                      WHEN metadata_origin = 'manual' THEN 'legacy'
                                      ELSE metadata_origin END,
               metadata_revision = metadata_revision + 1
             WHERE id = ?1",
            params![work_id, if manual { 1 } else { 0 }],
        )?;
        if changed == 0 {
            return Err(CoreError::library(
                "work_missing",
                "That work is no longer in the library.",
            ));
        }
        Ok(())
    }

    pub fn apply_catalogue_work(
        &self,
        work_id: &str,
        work: &CatalogueWork,
        provider: &str,
        mode: ReconcileMode,
    ) -> Result<ReconcileDecision> {
        let current = self.work_metadata(work_id)?;
        let decision = reconcile(&current, &MetadataPatch::from_work(work), mode);
        if decision.applied {
            let title = decision
                .patch
                .title
                .clone()
                .unwrap_or_else(|| current.title.clone());
            let year = decision.patch.year.or(current.year);
            let overview = decision
                .patch
                .overview
                .clone()
                .unwrap_or_else(|| current.overview.clone());
            let genres = decision
                .patch
                .genres
                .clone()
                .unwrap_or_else(|| current.genres.clone());
            let genres = serde_json::to_string(&genres).unwrap_or_else(|_| "[]".to_string());
            self.connection().execute(
                "UPDATE works SET title = ?1, year = ?2, overview = ?3, genres = ?4,
                   metadata_origin = 'provider', metadata_revision = metadata_revision + 1
                 WHERE id = ?5",
                params![title, year, overview, genres, work_id],
            )?;
        }
        self.record_provider_identity(work_id, provider, &work.external_id)?;
        Ok(decision)
    }

    pub fn record_provider_identity(
        &self,
        work_id: &str,
        provider: &str,
        external_id: &str,
    ) -> Result<()> {
        self.connection().execute(
            "INSERT INTO external_identities(work_id, provider, external_id, confidence)
             VALUES(?1, ?2, ?3, 1.0)
             ON CONFLICT(work_id, provider) DO UPDATE SET
               external_id = excluded.external_id, confidence = excluded.confidence",
            params![work_id, provider, external_id],
        )?;
        Ok(())
    }

    pub fn provider_identity(&self, work_id: &str, provider: &str) -> Result<Option<String>> {
        Ok(self
            .connection()
            .query_row(
                "SELECT external_id FROM external_identities
                 WHERE work_id = ?1 AND provider = ?2",
                params![work_id, provider],
                |row| row.get(0),
            )
            .optional()?)
    }

    pub fn apply_catalogue_season(
        &self,
        work_id: &str,
        season: &CatalogueSeason,
        provider: &str,
        mode: ReconcileMode,
    ) -> Result<bool> {
        let connection = self.connection();
        let row = connection
            .query_row(
                "SELECT id, metadata_origin FROM seasons WHERE work_id = ?1 AND number = ?2",
                params![work_id, season.number],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?;
        let Some((season_id, origin)) = row else {
            return Err(CoreError::library(
                "season_missing",
                "That season is not in the library.",
            ));
        };
        if !replacement_allowed(MetadataOrigin::parse(&origin), false, mode) {
            return Ok(false);
        }
        let episode_count = (season.episodes.len() as i64).max(0);
        connection.execute(
            "UPDATE seasons SET episode_count = CASE WHEN ?1 > 0 THEN ?1 ELSE episode_count END,
               metadata_origin = 'provider', metadata_provider_id = ?2
             WHERE id = ?3",
            params![episode_count, provider, season_id],
        )?;
        Ok(true)
    }

    pub fn apply_catalogue_episode(
        &self,
        work_id: &str,
        season_number: i64,
        episode: &CatalogueEpisode,
        provider: &str,
        mode: ReconcileMode,
    ) -> Result<bool> {
        let connection = self.connection();
        let row = connection
            .query_row(
                "SELECT e.id, e.metadata_origin FROM episodes e
                 JOIN seasons s ON s.id = e.season_id
                 WHERE s.work_id = ?1 AND s.number = ?2 AND e.number = ?3",
                params![work_id, season_number, episode.number],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?;
        let Some((episode_id, origin)) = row else {
            return Err(CoreError::library(
                "episode_missing",
                "That episode is not in the library.",
            ));
        };
        if !replacement_allowed(MetadataOrigin::parse(&origin), false, mode) {
            return Ok(false);
        }
        let title = clean_text(&episode.title, MAX_TITLE_CHARS);
        let overview = clean_text(&episode.overview, MAX_OVERVIEW_CHARS);
        connection.execute(
            "UPDATE episodes SET
               title = CASE WHEN ?1 <> '' THEN ?1 ELSE title END,
               overview = CASE WHEN ?2 <> '' THEN ?2 ELSE overview END,
               runtime_ms = CASE WHEN ?3 > 0 THEN ?3 ELSE runtime_ms END,
               metadata_origin = 'provider', metadata_provider_id = ?4
             WHERE id = ?5",
            params![title, overview, episode.runtime_ms, provider, episode_id],
        )?;
        Ok(true)
    }

    pub fn artwork_retention(&self) -> Result<ArtworkRetention> {
        Ok(match self.setting(ARTWORK_RETENTION_SETTING)?.as_deref() {
            Some("keep") => ArtworkRetention::KeepLocally,
            _ => ArtworkRetention::DoNotStore,
        })
    }

    pub fn set_artwork_retention(&self, retention: ArtworkRetention) -> Result<()> {
        let value = match retention {
            ArtworkRetention::KeepLocally => "keep",
            ArtworkRetention::DoNotStore => "skip",
        };
        self.set_setting(ARTWORK_RETENTION_SETTING, value)
    }

    pub fn apply_provider_artwork(&self, work_id: &str, image: &ProviderImage) -> Result<bool> {
        if self.artwork_retention()? == ArtworkRetention::DoNotStore {
            return Ok(false);
        }
        if image.bytes.is_empty() || image.bytes.len() > MAX_ARTWORK_BYTES {
            return Err(CoreError::library(
                "provider_artwork_size",
                "The provider image was empty or larger than the artwork budget.",
            ));
        }
        let Some((width, height)) = jpeg_dimensions(&image.bytes) else {
            return Err(CoreError::library(
                "provider_artwork_format",
                "The provider image was not a usable JPEG.",
            ));
        };
        self.store_artwork(work_id, None, &image.bytes, width, height, 0)?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(origin: MetadataOrigin, manual_match: bool) -> MetadataSnapshot {
        MetadataSnapshot {
            origin,
            manual_match,
            title: "parsed title 1998".to_string(),
            year: Some(1998),
            overview: String::new(),
            genres: Vec::new(),
        }
    }

    fn candidate() -> MetadataPatch {
        MetadataPatch {
            title: Some("The Lantern Keeper".to_string()),
            year: Some(1998),
            overview: Some("A keeper tends a light that never goes out.".to_string()),
            genres: Some(vec!["Drama".to_string(), "drama".to_string()]),
        }
    }

    #[test]
    fn a_manual_match_beats_a_provider_in_every_mode() {
        for mode in [ReconcileMode::Background, ReconcileMode::OwnerRequested] {
            let decision = reconcile(&snapshot(MetadataOrigin::Manual, true), &candidate(), mode);
            assert!(!decision.applied);
            assert_eq!(decision.blocked_by, Some(MetadataOrigin::Manual));
            assert_eq!(decision.origin, MetadataOrigin::Manual);
            assert_eq!(decision.patch, MetadataPatch::default());
        }
    }

    #[test]
    fn a_provider_beats_a_filename_guess_and_keeps_the_bound_values() {
        let decision = reconcile(
            &snapshot(MetadataOrigin::Filename, false),
            &candidate(),
            ReconcileMode::Background,
        );
        assert!(decision.applied);
        assert_eq!(decision.origin, MetadataOrigin::Provider);
        assert_eq!(decision.blocked_by, None);
        assert_eq!(decision.patch.title.as_deref(), Some("The Lantern Keeper"));
        assert_eq!(decision.patch.year, None);
        assert_eq!(decision.patch.genres, Some(vec!["Drama".to_string()]));
        assert_eq!(
            decision.patch.overview.as_deref(),
            Some("A keeper tends a light that never goes out.")
        );
    }

    #[test]
    fn an_nfo_value_beats_a_provider_in_the_background_and_yields_when_asked() {
        let background = reconcile(
            &snapshot(MetadataOrigin::Nfo, false),
            &candidate(),
            ReconcileMode::Background,
        );
        assert!(!background.applied);
        assert_eq!(background.blocked_by, Some(MetadataOrigin::Nfo));
        assert_eq!(background.origin, MetadataOrigin::Nfo);

        let requested = reconcile(
            &snapshot(MetadataOrigin::Nfo, false),
            &candidate(),
            ReconcileMode::OwnerRequested,
        );
        assert!(requested.applied);
        assert_eq!(requested.origin, MetadataOrigin::Provider);
    }

    #[test]
    fn empty_or_out_of_range_provider_values_never_erase_what_is_known() {
        let current = MetadataSnapshot {
            origin: MetadataOrigin::Legacy,
            manual_match: false,
            title: "Kept".to_string(),
            year: Some(1998),
            overview: "Kept overview".to_string(),
            genres: vec!["Drama".to_string()],
        };
        let decision = reconcile(
            &current,
            &MetadataPatch {
                title: Some("   ".to_string()),
                year: Some(1492),
                overview: Some(String::new()),
                genres: Some(Vec::new()),
            },
            ReconcileMode::Background,
        );
        assert!(!decision.applied);
        assert_eq!(decision.origin, MetadataOrigin::Legacy);
        assert_eq!(decision.patch, MetadataPatch::default());
    }

    #[test]
    fn a_jpeg_frame_header_gives_the_size_and_other_bytes_do_not() {
        let mut bytes = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x04, 0x00, 0x00];
        bytes.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x11, 0x08, 0x01, 0xE0, 0x02, 0x80, 0x03]);
        bytes.extend_from_slice(&[0x00; 10]);
        assert_eq!(jpeg_dimensions(&bytes), Some((640, 480)));
        assert_eq!(jpeg_dimensions(b"not an image"), None);
        assert_eq!(jpeg_dimensions(&[0xFF, 0xD8, 0xFF, 0xD9]), None);
    }
}
