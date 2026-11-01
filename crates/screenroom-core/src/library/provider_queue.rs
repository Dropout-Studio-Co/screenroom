use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

use super::provider_http::{
    BASE_BACKOFF_MS, DEFAULT_MAX_ATTEMPTS, MAX_BACKOFF_MS, ProviderError, retry_delay_ms,
};
use super::provider_metadata::{ArtworkRetention, ReconcileMode};
use super::provider_tmdb::{
    CatalogueMetadataProvider, CatalogueSeries, CatalogueWork, ImageSize, MetadataQuery,
};
use super::{Library, new_id};
use crate::domain::MediaKind;
use crate::error::{CoreError, Result};

const ENRICHMENT_LANGUAGE: &str = "en-US";

const RECENT_VIEWING_MS: i64 = 30 * 24 * 60 * 60 * 1_000;

const MAX_ERROR_CHARS: usize = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnrichmentState {
    Queued,

    Running,

    RateLimited,

    WaitingNetwork,

    Failed,

    Done,

    Cancelled,
}

impl EnrichmentState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::RateLimited => "rateLimited",
            Self::WaitingNetwork => "waitingNetwork",
            Self::Failed => "failed",
            Self::Done => "done",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "running" => Self::Running,
            "rateLimited" => Self::RateLimited,
            "waitingNetwork" => Self::WaitingNetwork,
            "failed" => Self::Failed,
            "done" => Self::Done,
            "cancelled" => Self::Cancelled,
            _ => Self::Queued,
        }
    }

    pub fn is_active(self) -> bool {
        matches!(
            self,
            Self::Queued | Self::Running | Self::RateLimited | Self::WaitingNetwork
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "target")]
pub enum EnrichmentTarget {
    Work,

    Season { season: i64 },

    Episode { season: i64, episode: i64 },
}

impl EnrichmentTarget {
    pub fn kind(self) -> &'static str {
        match self {
            Self::Work => "work",
            Self::Season { .. } => "season",
            Self::Episode { .. } => "episode",
        }
    }

    pub fn season(self) -> Option<i64> {
        match self {
            Self::Work => None,
            Self::Season { season } => Some(season),
            Self::Episode { season, .. } => Some(season),
        }
    }

    pub fn episode(self) -> Option<i64> {
        match self {
            Self::Episode { episode, .. } => Some(episode),
            _ => None,
        }
    }

    fn describe(self) -> String {
        match self {
            Self::Work => "the work".to_string(),
            Self::Season { season } => format!("season {season}"),
            Self::Episode { season, episode } => format!("season {season} episode {episode}"),
        }
    }
}

pub fn describe_enrichment_target(target: EnrichmentTarget) -> String {
    target.describe()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnrichmentTask {
    pub id: String,

    pub work_id: String,

    pub target: EnrichmentTarget,

    pub language: String,

    pub priority: i64,

    pub state: EnrichmentState,

    pub attempts: i64,

    pub next_attempt_ms: i64,

    pub last_error: Option<String>,

    pub created_ms: i64,

    pub updated_ms: i64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnrichmentQueueDepth {
    pub queued: i64,

    pub running: i64,

    pub rate_limited: i64,

    pub waiting_network: i64,

    pub failed: i64,

    pub done: i64,
}

impl EnrichmentQueueDepth {
    pub fn remaining(self) -> i64 {
        self.queued + self.running + self.rate_limited + self.waiting_network
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnrichmentDrainReport {
    pub configured: bool,

    pub attempted: u64,

    pub succeeded: u64,

    pub failed: u64,

    pub rate_limited: u64,

    pub waiting_network: u64,

    pub remaining: i64,
}

#[derive(Debug)]
enum EnrichmentFailure {
    Provider(ProviderError),

    Local(String),
}

impl From<ProviderError> for EnrichmentFailure {
    fn from(error: ProviderError) -> Self {
        Self::Provider(error)
    }
}

impl From<CoreError> for EnrichmentFailure {
    fn from(error: CoreError) -> Self {
        Self::Local(error.to_string())
    }
}

struct WorkRow {
    title: String,
    year: Option<i64>,
    kind: MediaKind,
    manual_match: bool,
}

fn valid_language(language: &str) -> Result<String> {
    let trimmed = language.trim();
    if trimmed.is_empty()
        || trimmed.chars().count() > 35
        || !trimmed.chars().all(|character| {
            character.is_ascii_alphanumeric() || character == '-' || character == '_'
        })
    {
        return Err(CoreError::library(
            "invalid_language",
            "Use a language tag such as \"en-US\" or \"bn\".",
        ));
    }
    Ok(trimmed.to_string())
}

fn task_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<EnrichmentTask> {
    let kind: String = row.get(2)?;
    let season: Option<i64> = row.get(3)?;
    let episode: Option<i64> = row.get(4)?;
    let target = match (kind.as_str(), season, episode) {
        ("season", Some(season), _) => EnrichmentTarget::Season { season },
        ("episode", Some(season), Some(episode)) => EnrichmentTarget::Episode { season, episode },
        _ => EnrichmentTarget::Work,
    };
    let state: String = row.get(7)?;
    Ok(EnrichmentTask {
        id: row.get(0)?,
        work_id: row.get(1)?,
        target,
        language: row.get(5)?,
        priority: row.get(6)?,
        state: EnrichmentState::parse(&state),
        attempts: row.get(8)?,
        next_attempt_ms: row.get(9)?,
        last_error: row.get(10)?,
        created_ms: row.get(11)?,
        updated_ms: row.get(12)?,
    })
}

const TASK_COLUMNS: &str = "id, work_id, target_kind, season_number, episode_number, language,
    priority, state, attempts, next_attempt_ms, last_error, created_ms, updated_ms";

fn truncated(message: &str) -> String {
    let trimmed = message.trim();
    if trimmed.chars().count() <= MAX_ERROR_CHARS {
        return trimmed.to_string();
    }
    trimmed.chars().take(MAX_ERROR_CHARS).collect()
}

fn work_from_series(series: &CatalogueSeries) -> CatalogueWork {
    CatalogueWork {
        external_id: series.external_id.clone(),
        title: series.title.clone(),
        original_title: series.original_title.clone(),
        year: series.year,
        overview: series.overview.clone(),
        genres: series.genres.clone(),
        runtime_ms: 0,
        poster_path: series.poster_path.clone(),
        backdrop_path: series.backdrop_path.clone(),
    }
}

impl Library {
    pub fn enqueue_enrichment(
        &self,
        work_id: &str,
        target: EnrichmentTarget,
        language: &str,
        priority: i64,
    ) -> Result<String> {
        self.enqueue_enrichment_inner(work_id, target, language, priority)
            .map(|(id, _)| id)
    }

    fn enqueue_enrichment_inner(
        &self,
        work_id: &str,
        target: EnrichmentTarget,
        language: &str,
        priority: i64,
    ) -> Result<(String, bool)> {
        let language = valid_language(language)?;
        let known: bool = self.connection().query_row(
            "SELECT EXISTS(SELECT 1 FROM works WHERE id = ?1)",
            [work_id],
            |row| row.get(0),
        )?;
        if !known {
            return Err(CoreError::library(
                "work_missing",
                "That work is no longer in the library.",
            ));
        }
        let connection = self.connection();
        let active: Option<String> = connection
            .query_row(
                "SELECT id FROM enrichment_queue
                 WHERE work_id = ?1 AND target_kind = ?2
                   AND IFNULL(season_number, -1) = IFNULL(?3, -1)
                   AND IFNULL(episode_number, -1) = IFNULL(?4, -1)
                   AND state NOT IN ('done','cancelled','failed')
                 LIMIT 1",
                params![work_id, target.kind(), target.season(), target.episode()],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(id) = active {
            return Ok((id, false));
        }
        let now = super::now_ms();
        let exhausted: Option<String> = connection
            .query_row(
                "SELECT id FROM enrichment_queue
                 WHERE work_id = ?1 AND target_kind = ?2
                   AND IFNULL(season_number, -1) = IFNULL(?3, -1)
                   AND IFNULL(episode_number, -1) = IFNULL(?4, -1)
                   AND state = 'failed'
                 ORDER BY updated_ms DESC, id
                 LIMIT 1",
                params![work_id, target.kind(), target.season(), target.episode()],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(id) = exhausted {
            connection.execute(
                "UPDATE enrichment_queue
                 SET language = ?2, priority = ?3, state = 'queued', attempts = 0,
                     next_attempt_ms = 0, last_error = NULL, updated_ms = ?4
                 WHERE id = ?1",
                params![id, language, priority.clamp(-1_000, 1_000), now],
            )?;
            return Ok((id, true));
        }
        let id = new_id();
        connection.execute(
            "INSERT INTO enrichment_queue(id, work_id, target_kind, season_number, episode_number,
               language, priority, state, attempts, next_attempt_ms, last_error, created_ms, updated_ms)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, 'queued', 0, 0, NULL, ?8, ?8)",
            params![
                id,
                work_id,
                target.kind(),
                target.season(),
                target.episode(),
                language,
                priority.clamp(-1_000, 1_000),
                now
            ],
        )?;
        Ok((id, true))
    }

    pub fn enqueue_whole_library(&self, language: &str) -> Result<usize> {
        let language = valid_language(language)?;
        let cutoff = super::now_ms() - RECENT_VIEWING_MS;
        let works: Vec<(String, i64)> = {
            let connection = self.connection();
            let mut statement = connection.prepare(
                "SELECT w.id,
                   CASE WHEN EXISTS(SELECT 1 FROM progress p JOIN editions e ON e.id = p.edition_id
                                    WHERE e.work_id = w.id AND p.updated_ms >= ?1) THEN 100
                        WHEN EXISTS(SELECT 1 FROM watchlist wl WHERE wl.work_id = w.id
                                    AND (wl.favorite = 1 OR wl.watchlisted = 1)) THEN 50
                        ELSE 0 END priority
                 FROM works w
                 WHERE w.manual_match = 0
                   AND EXISTS(SELECT 1 FROM editions e JOIN files f ON f.edition_id = e.id
                              JOIN sources s ON s.id = f.source_id
                              WHERE e.work_id = w.id AND s.state != 'removed')
                 ORDER BY priority DESC, w.title COLLATE NOCASE, w.id",
            )?;
            let rows = statement.query_map([cutoff], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        let mut queued = 0_usize;
        for (work_id, priority) in works {
            let (_, created) = self.enqueue_enrichment_inner(
                &work_id,
                EnrichmentTarget::Work,
                &language,
                priority,
            )?;
            if created {
                queued += 1;
            }
        }
        Ok(queued)
    }

    pub fn due_enrichment_tasks(&self, at_ms: i64, limit: i64) -> Result<Vec<EnrichmentTask>> {
        let limit = limit.clamp(1, 500);
        let connection = self.connection();
        let mut statement = connection.prepare(&format!(
            "SELECT {TASK_COLUMNS} FROM enrichment_queue
             WHERE state IN ('queued','rateLimited','waitingNetwork') AND next_attempt_ms <= ?1
             ORDER BY priority DESC, next_attempt_ms, created_ms, id LIMIT ?2"
        ))?;
        let rows = statement.query_map(params![at_ms, limit], task_from_row)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn enrichment_task(&self, id: &str) -> Result<Option<EnrichmentTask>> {
        let connection = self.connection();
        let mut statement = connection.prepare(&format!(
            "SELECT {TASK_COLUMNS} FROM enrichment_queue WHERE id = ?1"
        ))?;
        let mut rows = statement.query([id])?;
        match rows.next()? {
            Some(row) => Ok(Some(task_from_row(row)?)),
            None => Ok(None),
        }
    }

    pub fn claim_enrichment_task(&self, id: &str, at_ms: i64) -> Result<bool> {
        let changed = self.connection().execute(
            "UPDATE enrichment_queue SET state = 'running', updated_ms = ?2
             WHERE id = ?1 AND state IN ('queued','rateLimited','waitingNetwork')",
            params![id, at_ms],
        )?;
        Ok(changed > 0)
    }

    pub fn complete_enrichment_task(&self, id: &str, at_ms: i64) -> Result<()> {
        self.connection().execute(
            "UPDATE enrichment_queue SET state = 'done', last_error = NULL, updated_ms = ?2
             WHERE id = ?1",
            params![id, at_ms],
        )?;
        Ok(())
    }

    pub fn fail_enrichment_task(
        &self,
        id: &str,
        message: &str,
        retry_at_ms: Option<i64>,
        max_attempts: u32,
        at_ms: i64,
    ) -> Result<EnrichmentState> {
        let connection = self.connection();
        let attempts: i64 = connection.query_row(
            "SELECT attempts FROM enrichment_queue WHERE id = ?1",
            [id],
            |row| row.get(0),
        )?;
        let attempts = attempts + 1;
        let exhausted = attempts >= i64::from(max_attempts.max(1));
        let state = if exhausted {
            EnrichmentState::Failed
        } else {
            EnrichmentState::Queued
        };
        let next_attempt = match (exhausted, retry_at_ms) {
            (true, _) => 0,
            (false, Some(retry_at)) => retry_at.max(at_ms),
            (false, None) => at_ms + retry_delay_ms(attempts, BASE_BACKOFF_MS, MAX_BACKOFF_MS),
        };
        connection.execute(
            "UPDATE enrichment_queue SET state = ?2, attempts = ?3, next_attempt_ms = ?4,
               last_error = ?5, updated_ms = ?6 WHERE id = ?1",
            params![
                id,
                state.as_str(),
                attempts,
                next_attempt,
                truncated(message),
                at_ms
            ],
        )?;
        Ok(state)
    }

    pub fn defer_enrichment_task(
        &self,
        id: &str,
        state: EnrichmentState,
        next_attempt_ms: i64,
        message: &str,
        at_ms: i64,
    ) -> Result<()> {
        let state = if state.is_active() {
            state
        } else {
            EnrichmentState::Queued
        };
        self.connection().execute(
            "UPDATE enrichment_queue SET state = ?2, next_attempt_ms = ?3, last_error = ?4,
               updated_ms = ?5,
               attempts = attempts + CASE WHEN ?2 = 'waitingNetwork' THEN 1 ELSE 0 END
             WHERE id = ?1",
            params![
                id,
                state.as_str(),
                next_attempt_ms.max(0),
                truncated(message),
                at_ms
            ],
        )?;
        Ok(())
    }

    pub fn enrichment_queue_depth(&self) -> Result<EnrichmentQueueDepth> {
        let connection = self.connection();
        let mut statement =
            connection.prepare("SELECT state, COUNT(*) FROM enrichment_queue GROUP BY state")?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
        let mut depth = EnrichmentQueueDepth::default();
        for row in rows {
            let (state, count) = row?;
            match EnrichmentState::parse(&state) {
                EnrichmentState::Queued => depth.queued = count,
                EnrichmentState::Running => depth.running = count,
                EnrichmentState::RateLimited => depth.rate_limited = count,
                EnrichmentState::WaitingNetwork => depth.waiting_network = count,
                EnrichmentState::Failed => depth.failed = count,
                EnrichmentState::Done => depth.done = count,
                EnrichmentState::Cancelled => {}
            }
        }
        Ok(depth)
    }

    pub fn prune_enrichment_history(&self, before_ms: i64) -> Result<usize> {
        let removed = self.connection().execute(
            "DELETE FROM enrichment_queue
             WHERE state IN ('done','cancelled','failed') AND updated_ms < ?1",
            [before_ms],
        )?;
        Ok(removed)
    }

    pub fn enrichment_language(&self) -> &'static str {
        ENRICHMENT_LANGUAGE
    }

    pub fn drain_enrichment_queue(
        &self,
        provider: &dyn CatalogueMetadataProvider,
        limit: i64,
        at_ms: i64,
    ) -> Result<EnrichmentDrainReport> {
        let capability = provider.capability();
        let mut report = EnrichmentDrainReport {
            configured: capability.is_configured(),
            ..EnrichmentDrainReport::default()
        };
        if !report.configured {
            report.remaining = self.enrichment_queue_depth()?.remaining();
            return Ok(report);
        }

        let limit = limit.clamp(1, 500);
        for _ in 0..limit {
            let Some(task) = self.due_enrichment_tasks(at_ms, 1)?.into_iter().next() else {
                break;
            };
            if !self.claim_enrichment_task(&task.id, at_ms)? {
                continue;
            }
            report.attempted += 1;
            match self.apply_enrichment_task(provider, &task) {
                Ok(()) => {
                    self.complete_enrichment_task(&task.id, at_ms)?;
                    report.succeeded += 1;
                }
                Err(EnrichmentFailure::Provider(ProviderError::RateLimited { retry_after_ms })) => {
                    let wait = retry_after_ms.unwrap_or_else(|| {
                        retry_delay_ms(task.attempts + 1, BASE_BACKOFF_MS, MAX_BACKOFF_MS)
                    });
                    self.defer_enrichment_task(
                        &task.id,
                        EnrichmentState::RateLimited,
                        at_ms + wait.max(1),
                        &ProviderError::RateLimited { retry_after_ms }.message(),
                        at_ms,
                    )?;
                    report.rate_limited += 1;
                    break;
                }
                Err(EnrichmentFailure::Provider(ProviderError::NotConfigured)) => {
                    self.defer_enrichment_task(
                        &task.id,
                        EnrichmentState::Queued,
                        0,
                        &ProviderError::NotConfigured.message(),
                        at_ms,
                    )?;
                    report.configured = false;
                    break;
                }
                Err(EnrichmentFailure::Provider(error @ ProviderError::Transport(_))) => {
                    let wait = retry_delay_ms(task.attempts + 1, BASE_BACKOFF_MS, MAX_BACKOFF_MS);
                    self.defer_enrichment_task(
                        &task.id,
                        EnrichmentState::WaitingNetwork,
                        at_ms + wait.max(1),
                        &error.message(),
                        at_ms,
                    )?;
                    report.waiting_network += 1;
                    break;
                }
                Err(EnrichmentFailure::Provider(error)) => {
                    self.fail_enrichment_task(
                        &task.id,
                        &error.message(),
                        None,
                        DEFAULT_MAX_ATTEMPTS,
                        at_ms,
                    )?;
                    report.failed += 1;
                }
                Err(EnrichmentFailure::Local(message)) => {
                    self.fail_enrichment_task(
                        &task.id,
                        &message,
                        None,
                        DEFAULT_MAX_ATTEMPTS,
                        at_ms,
                    )?;
                    report.failed += 1;
                }
            }
        }

        report.remaining = self.enrichment_queue_depth()?.remaining();
        Ok(report)
    }

    fn enrichment_work(&self, work_id: &str) -> Result<Option<WorkRow>> {
        let connection = self.connection();
        let row = connection
            .query_row(
                "SELECT title, year, kind, manual_match FROM works WHERE id = ?1",
                [work_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<i64>>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                    ))
                },
            )
            .optional()?;
        Ok(row.map(|(title, year, kind, manual_match)| WorkRow {
            title,
            year,
            kind: MediaKind::parse(&kind).unwrap_or(MediaKind::Unknown),
            manual_match: manual_match == 1,
        }))
    }

    fn apply_enrichment_task(
        &self,
        provider: &dyn CatalogueMetadataProvider,
        task: &EnrichmentTask,
    ) -> std::result::Result<(), EnrichmentFailure> {
        let Some(work) = self.enrichment_work(&task.work_id)? else {
            return Err(EnrichmentFailure::Local(
                "That work is no longer in the library.".to_string(),
            ));
        };
        if work.manual_match {
            return Ok(());
        }

        let lookup_kind = if task.target == EnrichmentTarget::Work {
            work.kind
        } else {
            if work.kind == MediaKind::Movie {
                return Err(EnrichmentFailure::Local(
                    "That work is a film, so it has no seasons.".to_string(),
                ));
            }
            MediaKind::Series
        };

        let identity = self.provider_identity(&task.work_id, provider.provider_name())?;
        let (external_id, kind) = match (identity, lookup_kind) {
            (Some(id), kind) if kind != MediaKind::Unknown => (id, kind),
            _ => {
                let query = MetadataQuery {
                    title: work.title.clone(),
                    year: work.year,
                    kind: lookup_kind,
                };
                let best = provider
                    .search(&query)?
                    .into_iter()
                    .next()
                    .ok_or(ProviderError::NotFound)?;
                (best.external_id, best.kind)
            }
        };

        match task.target {
            EnrichmentTarget::Work => {
                let detail = provider.work(&external_id, kind)?;
                self.apply_catalogue_work(
                    &task.work_id,
                    &detail,
                    provider.provider_name(),
                    ReconcileMode::Background,
                )?;
                self.retain_poster(provider, &task.work_id, detail.poster_path.as_deref())?;
            }
            EnrichmentTarget::Season { season } => {
                if kind != MediaKind::Series {
                    return Err(EnrichmentFailure::Provider(ProviderError::NotFound));
                }
                let series = provider.series(&external_id)?;
                let work_detail = work_from_series(&series);
                self.apply_catalogue_work(
                    &task.work_id,
                    &work_detail,
                    provider.provider_name(),
                    ReconcileMode::Background,
                )?;
                let detail = provider.season(&external_id, season)?;
                self.apply_catalogue_season(
                    &task.work_id,
                    &detail,
                    provider.provider_name(),
                    ReconcileMode::Background,
                )?;
                for episode in &detail.episodes {
                    match self.apply_catalogue_episode(
                        &task.work_id,
                        season,
                        episode,
                        provider.provider_name(),
                        ReconcileMode::Background,
                    ) {
                        Ok(_) => {}
                        Err(error) if error.code() == "episode_missing" => {}
                        Err(error) => return Err(error.into()),
                    }
                }
                self.retain_poster(provider, &task.work_id, work_detail.poster_path.as_deref())?;
            }
            EnrichmentTarget::Episode { season, episode } => {
                if kind != MediaKind::Series {
                    return Err(EnrichmentFailure::Provider(ProviderError::NotFound));
                }
                let detail = provider.episode(&external_id, season, episode)?;
                self.apply_catalogue_episode(
                    &task.work_id,
                    season,
                    &detail,
                    provider.provider_name(),
                    ReconcileMode::Background,
                )?;
            }
        }
        Ok(())
    }

    fn retain_poster(
        &self,
        provider: &dyn CatalogueMetadataProvider,
        work_id: &str,
        poster_path: Option<&str>,
    ) -> std::result::Result<(), EnrichmentFailure> {
        if self.artwork_retention()? != ArtworkRetention::KeepLocally {
            return Ok(());
        }
        let Some(path) = poster_path else {
            return Ok(());
        };
        let Ok(image) = provider.image(path, ImageSize::Poster) else {
            return Ok(());
        };
        self.apply_provider_artwork(work_id, &image)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::MetadataOrigin;
    use crate::{SCHEMA_VERSION, schema};

    fn queue_is_present(library: &Library) -> bool {
        let found: i64 = library
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'enrichment_queue'",
                [],
                |row| row.get(0),
            )
            .expect("table lookup");
        found == 1
    }

    fn index_is_present(library: &Library, name: &str) -> bool {
        let found: i64 = library
            .connection()
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = ?1",
                [name],
                |row| row.get(0),
            )
            .expect("index lookup");
        found == 1
    }

    #[test]
    fn a_fresh_library_has_the_queue_table_and_both_indexes() {
        let library = Library::open_in_memory().expect("library");
        assert!(queue_is_present(&library));
        assert!(index_is_present(&library, "enrichment_queue_active"));
        assert!(index_is_present(&library, "enrichment_queue_due"));
        assert_eq!(
            library.enrichment_queue_depth().expect("depth"),
            EnrichmentQueueDepth::default()
        );
    }

    #[test]
    fn a_schema_seven_library_gains_the_queue_and_keeps_its_rows() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("library.db");
        {
            let connection = rusqlite::Connection::open(&path).expect("open");
            for statement in schema::CREATE {
                connection.execute(statement, []).expect("schema");
            }
            connection
                .execute("DROP TABLE enrichment_queue", [])
                .expect("drop queue");
            connection
                .execute("DROP TABLE subtitles", [])
                .expect("drop subtitles");
            connection
                .execute("INSERT INTO profiles(id, name) VALUES('p', 'Me')", [])
                .expect("profile");
            connection
                .execute(
                    "INSERT INTO works(id, identity_key, title, kind, metadata_origin)
                     VALUES('w', 'k', 'Kept', 'movie', 'nfo')",
                    [],
                )
                .expect("work");
            connection
                .pragma_update(None, "user_version", 7)
                .expect("version");
        }

        let library = Library::open(&path).expect("migrated");
        let version: i64 = library
            .connection()
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .expect("version");
        assert_eq!(version, SCHEMA_VERSION);
        assert_eq!(version, 9);
        assert!(queue_is_present(&library));
        assert!(index_is_present(&library, "enrichment_queue_active"));
        assert!(index_is_present(&library, "enrichment_queue_due"));

        let snapshot = library.work_metadata("w").expect("kept work");
        assert_eq!(snapshot.title, "Kept");
        assert_eq!(snapshot.origin, MetadataOrigin::Nfo);
    }

    #[test]
    fn an_unknown_work_or_a_bad_language_is_refused_before_a_row_exists() {
        let library = Library::open_in_memory().expect("library");
        let missing = library
            .enqueue_enrichment("missing", EnrichmentTarget::Work, "en-US", 0)
            .expect_err("an unknown work");
        assert_eq!(missing.code(), "work_missing");

        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("film.mkv");
        std::fs::write(&path, b"video").expect("write");
        let source = library.add_source(directory.path(), None).expect("source");
        let parsed = crate::parse_filename("film.mkv", None);
        library
            .ingest_file(crate::library::Ingest {
                source_id: &source.id,
                relative_path: "film.mkv",
                fingerprint: "film",
                size: 5,
                modified_ms: 1,
                scan_id: "scan-1",
                parsed: Some(&parsed),
                expected_root: Some(&source.root_path),
            })
            .expect("ingest");
        let work = library
            .items(&super::super::LibraryQuery::page(10))
            .expect("items")
            .remove(0)
            .work_id;

        let language = library
            .enqueue_enrichment(&work, EnrichmentTarget::Work, "not a language!", 0)
            .expect_err("a bad language");
        assert_eq!(language.code(), "invalid_language");
        assert_eq!(library.enrichment_queue_depth().expect("depth").queued, 0);
    }

    #[test]
    fn re_enqueueing_an_active_request_returns_the_same_row_and_a_done_one_reopens() {
        let directory = tempfile::tempdir().expect("tempdir");
        let library = Library::open_in_memory().expect("library");
        let source = library.add_source(directory.path(), None).expect("source");
        std::fs::write(directory.path().join("film.mkv"), b"video").expect("write");
        let parsed = crate::parse_filename("film.mkv", None);
        library
            .ingest_file(crate::library::Ingest {
                source_id: &source.id,
                relative_path: "film.mkv",
                fingerprint: "film",
                size: 5,
                modified_ms: 1,
                scan_id: "scan-1",
                parsed: Some(&parsed),
                expected_root: Some(&source.root_path),
            })
            .expect("ingest");
        let work = library
            .items(&super::super::LibraryQuery::page(10))
            .expect("items")
            .remove(0)
            .work_id;

        let first = library
            .enqueue_enrichment(&work, EnrichmentTarget::Work, "en-US", 0)
            .expect("first");
        let again = library
            .enqueue_enrichment(&work, EnrichmentTarget::Work, "en-US", 0)
            .expect("again");
        assert_eq!(first, again);
        assert_eq!(library.enrichment_queue_depth().expect("depth").queued, 1);

        library
            .complete_enrichment_task(&first, 1_000)
            .expect("complete");
        let reopened = library
            .enqueue_enrichment(&work, EnrichmentTarget::Work, "en-US", 0)
            .expect("reopened");
        assert_ne!(first, reopened);
        assert_eq!(library.enrichment_queue_depth().expect("depth").queued, 1);
        assert_eq!(library.enrichment_queue_depth().expect("depth").done, 1);
    }
}
