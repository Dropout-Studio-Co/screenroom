use std::cell::RefCell;
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::domain::MediaKind;

use super::artwork::MAX_ARTWORK_BYTES;
use super::provider_http::{
    BASE_BACKOFF_MS, Clock, DEFAULT_BUDGET_PER_MINUTE, DEFAULT_MAX_ATTEMPTS, DEFAULT_TIMEOUT,
    HttpRequest, HttpTransport, MAX_BACKOFF_MS, MAX_RESPONSE_BYTES, Pacer, ProviderError, Secret,
    Sleeper, SystemClock, ThreadSleeper, encode_query_value, join_url, scheme_is_carried,
};
use super::provider_tls::HttpsTransport;

pub const TMDB_PROVIDER: &str = "tmdb";

pub const TMDB_BASE_URL: &str = "https://api.themoviedb.org/3";

pub const TMDB_IMAGE_BASE_URL: &str = "https://image.tmdb.org/t/p";

pub const DEFAULT_LANGUAGE: &str = "en-US";

pub const API_KEY_ENVIRONMENT: &str = "SCREENROOM_TMDB_API_KEY";

pub const READ_TOKEN_ENVIRONMENT: &str = "SCREENROOM_TMDB_READ_TOKEN";

pub const CONFIG_ENVIRONMENT: &str = "SCREENROOM_PROVIDER_CONFIG";

pub const BASE_URL_ENVIRONMENT: &str = "SCREENROOM_TMDB_BASE_URL";

pub const IMAGE_BASE_URL_ENVIRONMENT: &str = "SCREENROOM_TMDB_IMAGE_BASE_URL";

pub const LANGUAGE_ENVIRONMENT: &str = "SCREENROOM_TMDB_LANGUAGE";

pub const MAX_SEARCH_RESULTS: usize = 40;

pub enum ApiCredential {
    ApiKey(Secret),

    ReadToken(Secret),
}

impl fmt::Debug for ApiCredential {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ApiKey(_) => formatter.write_str("ApiKey(redacted)"),
            Self::ReadToken(_) => formatter.write_str("ReadToken(redacted)"),
        }
    }
}

impl ApiCredential {
    pub fn from_environment() -> Option<Self> {
        if let Some(token) = environment_text(READ_TOKEN_ENVIRONMENT) {
            return Some(Self::ReadToken(Secret::new(token)));
        }
        environment_text(API_KEY_ENVIRONMENT).map(|key| Self::ApiKey(Secret::new(key)))
    }

    pub fn from_environment_or_file() -> Option<Self> {
        Self::from_environment().or_else(|| {
            let path = default_credential_path()?;
            Self::from_file(&path)
        })
    }

    pub fn from_file(path: &Path) -> Option<Self> {
        let text = std::fs::read_to_string(path).ok()?;
        Self::parse(text.trim())
    }

    pub fn parse(value: &str) -> Option<Self> {
        let value = value.trim();
        if value.is_empty() || value.len() > 512 || value.contains(char::is_whitespace) {
            return None;
        }
        if value.starts_with("eyJ") && value.contains('.') {
            return Some(Self::ReadToken(Secret::new(value)));
        }
        Some(Self::ApiKey(Secret::new(value)))
    }
}

pub const CREDENTIAL_FILE_NAME: &str = "tmdb.key";

pub const CREDENTIAL_DIRECTORY_ENVIRONMENT: &str = "SCREENROOM_CONFIG_DIR";

pub fn default_credential_path() -> Option<PathBuf> {
    let directory = if let Some(explicit) = std::env::var_os(CREDENTIAL_DIRECTORY_ENVIRONMENT) {
        PathBuf::from(explicit)
    } else if cfg!(target_os = "windows") {
        PathBuf::from(std::env::var_os("APPDATA").or_else(|| std::env::var_os("LOCALAPPDATA"))?)
            .join("Screenroom")
    } else if cfg!(target_os = "macos") {
        PathBuf::from(std::env::var_os("HOME")?).join("Library/Application Support/Screenroom")
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?
            .join("screenroom")
    };
    Some(directory.join(CREDENTIAL_FILE_NAME))
}

fn environment_text(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[derive(Debug)]
pub struct ProviderConfig {
    pub base_url: String,

    pub image_base_url: String,

    pub language: String,

    pub timeout: Duration,

    pub max_bytes: usize,

    pub budget_per_minute: u32,

    pub max_attempts: u32,

    pub credential: Option<ApiCredential>,

    pub configuration_error: Option<String>,
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self::unconfigured()
    }
}

impl ProviderConfig {
    pub fn unconfigured() -> Self {
        Self {
            base_url: TMDB_BASE_URL.to_string(),
            image_base_url: TMDB_IMAGE_BASE_URL.to_string(),
            language: DEFAULT_LANGUAGE.to_string(),
            timeout: DEFAULT_TIMEOUT,
            max_bytes: MAX_RESPONSE_BYTES,
            budget_per_minute: DEFAULT_BUDGET_PER_MINUTE,
            max_attempts: DEFAULT_MAX_ATTEMPTS,
            credential: None,
            configuration_error: None,
        }
    }

    pub fn configured(credential: ApiCredential) -> Self {
        Self {
            credential: Some(credential),
            ..Self::unconfigured()
        }
    }

    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    pub fn with_image_base_url(mut self, image_base_url: impl Into<String>) -> Self {
        self.image_base_url = image_base_url.into();
        self
    }

    pub fn with_language(mut self, language: impl Into<String>) -> Self {
        self.language = language.into();
        self
    }

    pub fn with_budget_per_minute(mut self, budget: u32) -> Self {
        self.budget_per_minute = budget.max(1);
        self
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let metadata = std::fs::metadata(path)
            .map_err(|_| "The provider configuration file cannot be read.".to_string())?;
        if !metadata.is_file() || metadata.len() > 65_536 {
            return Err("The provider configuration file cannot be used.".to_string());
        }
        let body = std::fs::read_to_string(path)
            .map_err(|_| "The provider configuration file cannot be read.".to_string())?;
        let mut config = Self::unconfigured();
        for line in body.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let value = value.trim();
            if value.is_empty() {
                continue;
            }
            match key.trim().to_ascii_lowercase().as_str() {
                "base_url" => config.base_url = value.to_string(),
                "image_base_url" => config.image_base_url = value.to_string(),
                "language" => config.language = value.to_string(),
                "api_key" => config.credential = Some(ApiCredential::ApiKey(Secret::new(value))),
                "read_token" => {
                    config.credential = Some(ApiCredential::ReadToken(Secret::new(value)))
                }
                _ => {}
            }
        }
        Ok(config)
    }

    pub fn from_environment() -> Self {
        let mut config = match environment_text(CONFIG_ENVIRONMENT) {
            Some(path) => match Self::load(Path::new(&path)) {
                Ok(loaded) => loaded,
                Err(error) => Self {
                    configuration_error: Some(error),
                    ..Self::unconfigured()
                },
            },
            None => Self::unconfigured(),
        };
        if let Some(value) = environment_text(BASE_URL_ENVIRONMENT) {
            config.base_url = value;
        }
        if let Some(value) = environment_text(IMAGE_BASE_URL_ENVIRONMENT) {
            config.image_base_url = value;
        }
        if let Some(value) = environment_text(LANGUAGE_ENVIRONMENT) {
            config.language = value;
        }
        if config.credential.is_none() {
            config.credential = ApiCredential::from_environment_or_file();
        }
        config
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderCapability {
    pub provider: &'static str,

    pub configured: bool,

    pub language: String,

    pub detail: String,
}

impl ProviderCapability {
    pub fn configured(provider: &'static str, language: &str) -> Self {
        Self {
            provider,
            configured: true,
            language: language.to_string(),
            detail: "Online metadata is configured.".to_string(),
        }
    }

    pub fn not_configured(provider: &'static str, reason: impl Into<String>) -> Self {
        Self {
            provider,
            configured: false,
            language: DEFAULT_LANGUAGE.to_string(),
            detail: reason.into(),
        }
    }

    pub fn is_configured(&self) -> bool {
        self.configured
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct MetadataQuery {
    pub title: String,

    pub year: Option<i64>,

    pub kind: MediaKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CatalogueMatch {
    pub external_id: String,

    pub title: String,

    pub year: Option<i64>,

    pub kind: MediaKind,

    pub overview: String,

    pub poster_path: Option<String>,

    pub backdrop_path: Option<String>,

    pub popularity: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CatalogueWork {
    pub external_id: String,

    pub title: String,

    pub original_title: String,

    pub year: Option<i64>,

    pub overview: String,

    pub genres: Vec<String>,

    pub runtime_ms: i64,

    pub poster_path: Option<String>,

    pub backdrop_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CatalogueSeasonSummary {
    pub number: i64,

    pub title: String,

    pub episode_count: i64,

    pub poster_path: Option<String>,

    pub year: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CatalogueSeries {
    pub external_id: String,

    pub title: String,

    pub original_title: String,

    pub year: Option<i64>,

    pub overview: String,

    pub genres: Vec<String>,

    pub poster_path: Option<String>,

    pub backdrop_path: Option<String>,

    pub seasons: Vec<CatalogueSeasonSummary>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CatalogueEpisode {
    pub season: i64,

    pub number: i64,

    pub title: String,

    pub overview: String,

    pub runtime_ms: i64,

    pub still_path: Option<String>,

    pub year: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CatalogueSeason {
    pub number: i64,

    pub title: String,

    pub overview: String,

    pub poster_path: Option<String>,

    pub year: Option<i64>,

    pub episodes: Vec<CatalogueEpisode>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderImage {
    pub bytes: Vec<u8>,

    pub mime: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageSize {
    Thumbnail,

    Card,

    Poster,

    Backdrop,

    Original,
}

impl ImageSize {
    fn segment(self) -> &'static str {
        match self {
            Self::Thumbnail => "w185",
            Self::Card => "w342",
            Self::Poster => "w500",
            Self::Backdrop => "w1280",
            Self::Original => "original",
        }
    }
}

pub trait CatalogueMetadataProvider {
    fn provider_name(&self) -> &'static str;

    fn capability(&self) -> ProviderCapability;

    fn search(&self, query: &MetadataQuery) -> Result<Vec<CatalogueMatch>, ProviderError>;

    fn work(&self, external_id: &str, kind: MediaKind) -> Result<CatalogueWork, ProviderError>;

    fn series(&self, external_id: &str) -> Result<CatalogueSeries, ProviderError>;

    fn season(&self, external_id: &str, number: i64) -> Result<CatalogueSeason, ProviderError>;

    fn episode(
        &self,
        external_id: &str,
        season: i64,
        episode: i64,
    ) -> Result<CatalogueEpisode, ProviderError>;

    fn image_url(&self, path: &str, size: ImageSize) -> Option<String>;

    fn image(&self, path: &str, size: ImageSize) -> Result<ProviderImage, ProviderError>;
}

pub struct TmdbProvider {
    config: ProviderConfig,
    transport: Box<dyn HttpTransport>,
    pacer: RefCell<Pacer>,
}

impl fmt::Debug for TmdbProvider {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TmdbProvider")
            .field("base_url", &self.config.base_url)
            .field("language", &self.config.language)
            .field("configured", &self.config.credential.is_some())
            .finish_non_exhaustive()
    }
}

impl TmdbProvider {
    pub fn new(config: ProviderConfig) -> Self {
        let pacer = Pacer::system(config.budget_per_minute).with_retry(
            config.max_attempts,
            BASE_BACKOFF_MS,
            MAX_BACKOFF_MS,
        );
        Self {
            config,
            transport: Box::new(HttpsTransport::default()),
            pacer: RefCell::new(pacer),
        }
    }

    pub fn with_parts(
        config: ProviderConfig,
        transport: Box<dyn HttpTransport>,
        clock: Box<dyn Clock>,
        sleeper: Box<dyn Sleeper>,
    ) -> Self {
        let pacer = Pacer::new(config.budget_per_minute, clock, sleeper).with_retry(
            config.max_attempts,
            BASE_BACKOFF_MS,
            MAX_BACKOFF_MS,
        );
        Self {
            config,
            transport,
            pacer: RefCell::new(pacer),
        }
    }

    pub fn with_system_clock(config: ProviderConfig, transport: Box<dyn HttpTransport>) -> Self {
        Self::with_parts(
            config,
            transport,
            Box::new(SystemClock),
            Box::new(ThreadSleeper),
        )
    }

    pub fn config(&self) -> &ProviderConfig {
        &self.config
    }

    pub fn max_attempts(&self) -> u32 {
        self.config.max_attempts
    }

    fn credential_present(&self) -> bool {
        self.config.credential.is_some()
    }

    fn endpoint(&self, path: &str, params: &[(&str, String)]) -> HttpRequest {
        let mut query = vec![format!(
            "language={}",
            encode_query_value(&self.config.language)
        )];
        if let Some(ApiCredential::ApiKey(key)) = &self.config.credential {
            query.push(format!("api_key={}", encode_query_value(key.expose())));
        }
        for (name, value) in params {
            query.push(format!("{name}={}", encode_query_value(value)));
        }
        let mut url = join_url(&self.config.base_url, path);
        url.push('?');
        url.push_str(&query.join("&"));
        let mut request = HttpRequest::get(url).with_header("Accept", "application/json");
        request.max_bytes = self.config.max_bytes;
        request.timeout = self.config.timeout;
        if let Some(ApiCredential::ReadToken(token)) = &self.config.credential {
            request = request.with_header("Authorization", format!("Bearer {}", token.expose()));
        }
        request
    }

    fn get_value(&self, path: &str, params: &[(&str, String)]) -> Result<Value, ProviderError> {
        if !self.credential_present() {
            return Err(ProviderError::NotConfigured);
        }
        let request = self.endpoint(path, params);
        let mut pacer = self.pacer.borrow_mut();
        pacer.execute(|| self.send(&request))
    }

    fn send(&self, request: &HttpRequest) -> Result<Value, ProviderError> {
        let response = self.transport.send(request)?;
        if !(200..300).contains(&response.status) {
            return Err(ProviderError::from_status(
                response.status,
                response.retry_after_ms(),
            ));
        }
        serde_json::from_slice(&response.body)
            .map_err(|_| ProviderError::InvalidResponse("the body was not JSON"))
    }

    fn matches_from(&self, value: &Value, requested: MediaKind) -> Vec<CatalogueMatch> {
        let Some(results) = value.get("results").and_then(Value::as_array) else {
            return Vec::new();
        };
        results
            .iter()
            .take(MAX_SEARCH_RESULTS)
            .filter_map(|row| catalogue_match(row, requested))
            .collect()
    }
}

fn catalogue_match(row: &Value, requested: MediaKind) -> Option<CatalogueMatch> {
    let media = row
        .get("media_type")
        .and_then(Value::as_str)
        .unwrap_or("movie");
    let kind = match requested {
        MediaKind::Series => MediaKind::Series,
        MediaKind::Movie => MediaKind::Movie,
        MediaKind::Unknown => match media {
            "tv" => MediaKind::Series,
            "movie" => MediaKind::Movie,
            _ => return None,
        },
    };
    let external_id = row_id(row)?;
    let title = match kind {
        MediaKind::Series => text(row, "name").or_else(|| text(row, "original_name"))?,
        _ => text(row, "title").or_else(|| text(row, "original_title"))?,
    };
    let year = match kind {
        MediaKind::Series => year_of(row, "first_air_date"),
        _ => year_of(row, "release_date"),
    };
    Some(CatalogueMatch {
        external_id,
        title,
        year,
        kind,
        overview: text(row, "overview").unwrap_or_default(),
        poster_path: text(row, "poster_path"),
        backdrop_path: text(row, "backdrop_path"),
        popularity: row
            .get("popularity")
            .and_then(Value::as_f64)
            .unwrap_or_default(),
    })
}

fn row_id(row: &Value) -> Option<String> {
    match row.get("id")? {
        Value::Number(number) => number.as_i64().map(|value| value.to_string()),
        Value::String(value) if !value.trim().is_empty() => Some(value.trim().to_string()),
        _ => None,
    }
}

fn resolved_id(row: &Value, fallback: &str) -> String {
    row_id(row).unwrap_or_else(|| fallback.to_string())
}

fn text(row: &Value, key: &str) -> Option<String> {
    row.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn year_of(row: &Value, key: &str) -> Option<i64> {
    let date = text(row, key)?;
    date.split('-').next()?.parse::<i64>().ok()
}

fn integer(row: &Value, key: &str) -> Option<i64> {
    row.get(key).and_then(Value::as_i64)
}

fn genre_names(row: &Value) -> Vec<String> {
    let Some(entries) = row.get("genres").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut genres: Vec<String> = Vec::new();
    for entry in entries {
        let Some(name) = entry
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|name| !name.is_empty())
        else {
            continue;
        };
        if !genres.iter().any(|known| known.eq_ignore_ascii_case(name)) {
            genres.push(name.to_string());
        }
        if genres.len() == 20 {
            break;
        }
    }
    genres
}

fn runtime_ms(row: &Value) -> i64 {
    integer(row, "runtime")
        .filter(|minutes| *minutes > 0)
        .map(|minutes| minutes * 60_000)
        .unwrap_or_default()
}

pub fn safe_image_path(path: &str) -> Option<&str> {
    let trimmed = path.trim();
    if trimmed.is_empty()
        || !trimmed.starts_with('/')
        || trimmed.contains("..")
        || trimmed.contains("://")
        || trimmed
            .chars()
            .any(|character| character.is_control() || character == '\\' || character == ' ')
    {
        return None;
    }
    Some(trimmed)
}

fn mime_from(headers: &[(String, String)]) -> String {
    headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("content-type"))
        .map(|(_, value)| value.split(';').next().unwrap_or("").trim().to_string())
        .filter(|value| value.starts_with("image/"))
        .unwrap_or_else(|| "image/jpeg".to_string())
}

impl CatalogueMetadataProvider for TmdbProvider {
    fn provider_name(&self) -> &'static str {
        TMDB_PROVIDER
    }

    fn capability(&self) -> ProviderCapability {
        if let Some(error) = &self.config.configuration_error {
            return ProviderCapability::not_configured(TMDB_PROVIDER, error.clone());
        }
        if self.config.credential.is_none() {
            return ProviderCapability::not_configured(
                TMDB_PROVIDER,
                "Online metadata is not configured. Put a TMDB key in the provider \
                 file, or set SCREENROOM_TMDB_API_KEY, SCREENROOM_TMDB_READ_TOKEN or \
                 SCREENROOM_PROVIDER_CONFIG first.",
            );
        }
        if let Err(reason) = scheme_is_carried(self.transport.as_ref(), &self.config.base_url) {
            return ProviderCapability::not_configured(TMDB_PROVIDER, reason);
        }
        ProviderCapability::configured(TMDB_PROVIDER, &self.config.language)
    }

    fn search(&self, query: &MetadataQuery) -> Result<Vec<CatalogueMatch>, ProviderError> {
        let path = match query.kind {
            MediaKind::Movie => "/search/movie",
            MediaKind::Series => "/search/tv",
            MediaKind::Unknown => "/search/multi",
        };
        let mut params = vec![
            ("query", query.title.trim().to_string()),
            ("include_adult", "false".to_string()),
        ];
        if let Some(year) = query.year.filter(|year| (1870..=2100).contains(year)) {
            let name = if query.kind == MediaKind::Series {
                "first_air_date_year"
            } else {
                "year"
            };
            params.push((name, year.to_string()));
        }
        let value = self.get_value(path, &params)?;
        Ok(self.matches_from(&value, query.kind))
    }

    fn work(&self, external_id: &str, kind: MediaKind) -> Result<CatalogueWork, ProviderError> {
        if kind == MediaKind::Series {
            return self.series(external_id).map(|series| CatalogueWork {
                external_id: series.external_id,
                title: series.title,
                original_title: series.original_title,
                year: series.year,
                overview: series.overview,
                genres: series.genres,
                runtime_ms: 0,
                poster_path: series.poster_path,
                backdrop_path: series.backdrop_path,
            });
        }
        let path = format!("/movie/{}", encode_query_value(external_id.trim()));
        let value = self.get_value(&path, &[])?;
        Ok(CatalogueWork {
            external_id: resolved_id(&value, external_id),
            title: text(&value, "title")
                .or_else(|| text(&value, "original_title"))
                .ok_or(ProviderError::InvalidResponse("the work had no title"))?,
            original_title: text(&value, "original_title").unwrap_or_default(),
            year: year_of(&value, "release_date"),
            overview: text(&value, "overview").unwrap_or_default(),
            genres: genre_names(&value),
            runtime_ms: runtime_ms(&value),
            poster_path: text(&value, "poster_path"),
            backdrop_path: text(&value, "backdrop_path"),
        })
    }

    fn series(&self, external_id: &str) -> Result<CatalogueSeries, ProviderError> {
        let path = format!("/tv/{}", encode_query_value(external_id.trim()));
        let value = self.get_value(&path, &[])?;
        let seasons = value
            .get("seasons")
            .and_then(Value::as_array)
            .map(|entries| {
                entries
                    .iter()
                    .filter_map(|entry| {
                        let number = integer(entry, "season_number")?;
                        Some(CatalogueSeasonSummary {
                            number,
                            title: text(entry, "name")
                                .unwrap_or_else(|| format!("Season {number}")),
                            episode_count: integer(entry, "episode_count").unwrap_or_default(),
                            poster_path: text(entry, "poster_path"),
                            year: year_of(entry, "air_date"),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(CatalogueSeries {
            external_id: resolved_id(&value, external_id),
            title: text(&value, "name")
                .or_else(|| text(&value, "original_name"))
                .ok_or(ProviderError::InvalidResponse("the series had no name"))?,
            original_title: text(&value, "original_name").unwrap_or_default(),
            year: year_of(&value, "first_air_date"),
            overview: text(&value, "overview").unwrap_or_default(),
            genres: genre_names(&value),
            poster_path: text(&value, "poster_path"),
            backdrop_path: text(&value, "backdrop_path"),
            seasons,
        })
    }

    fn season(&self, external_id: &str, number: i64) -> Result<CatalogueSeason, ProviderError> {
        if number < 0 {
            return Err(ProviderError::InvalidResponse(
                "the season number was negative",
            ));
        }
        let path = format!(
            "/tv/{}/season/{}",
            encode_query_value(external_id.trim()),
            number
        );
        let value = self.get_value(&path, &[])?;
        let episodes = value
            .get("episodes")
            .and_then(Value::as_array)
            .map(|entries| {
                entries
                    .iter()
                    .filter_map(|entry| {
                        let episode_number = integer(entry, "episode_number")?;
                        Some(CatalogueEpisode {
                            season: integer(entry, "season_number").unwrap_or(number),
                            number: episode_number,
                            title: text(entry, "name")
                                .unwrap_or_else(|| format!("Episode {episode_number}")),
                            overview: text(entry, "overview").unwrap_or_default(),
                            runtime_ms: runtime_ms(entry),
                            still_path: text(entry, "still_path"),
                            year: year_of(entry, "air_date"),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(CatalogueSeason {
            number: integer(&value, "season_number").unwrap_or(number),
            title: text(&value, "name").unwrap_or_else(|| format!("Season {number}")),
            overview: text(&value, "overview").unwrap_or_default(),
            poster_path: text(&value, "poster_path"),
            year: year_of(&value, "air_date"),
            episodes,
        })
    }

    fn episode(
        &self,
        external_id: &str,
        season: i64,
        episode: i64,
    ) -> Result<CatalogueEpisode, ProviderError> {
        if season < 0 || episode < 0 {
            return Err(ProviderError::InvalidResponse(
                "the episode number was negative",
            ));
        }
        let path = format!(
            "/tv/{}/season/{}/episode/{}",
            encode_query_value(external_id.trim()),
            season,
            episode
        );
        let value = self.get_value(&path, &[])?;
        Ok(CatalogueEpisode {
            season: integer(&value, "season_number").unwrap_or(season),
            number: integer(&value, "episode_number").unwrap_or(episode),
            title: text(&value, "name").unwrap_or_else(|| format!("Episode {episode}")),
            overview: text(&value, "overview").unwrap_or_default(),
            runtime_ms: runtime_ms(&value),
            still_path: text(&value, "still_path"),
            year: year_of(&value, "air_date"),
        })
    }

    fn image_url(&self, path: &str, size: ImageSize) -> Option<String> {
        let path = safe_image_path(path)?;
        Some(format!(
            "{}/{}{}",
            self.config.image_base_url.trim_end_matches('/'),
            size.segment(),
            path
        ))
    }

    fn image(&self, path: &str, size: ImageSize) -> Result<ProviderImage, ProviderError> {
        if !self.credential_present() {
            return Err(ProviderError::NotConfigured);
        }
        let url = self
            .image_url(path, size)
            .ok_or(ProviderError::InvalidResponse(
                "the artwork path was not usable",
            ))?;
        let mut request = HttpRequest::get(url)
            .with_header("Accept", "image/*")
            .with_max_bytes(MAX_ARTWORK_BYTES.min(MAX_RESPONSE_BYTES));
        request.timeout = self.config.timeout;
        let mut pacer = self.pacer.borrow_mut();
        pacer.execute(|| {
            let response = self.transport.send(&request)?;
            if !(200..300).contains(&response.status) {
                return Err(ProviderError::from_status(
                    response.status,
                    response.retry_after_ms(),
                ));
            }
            if response.body.is_empty() {
                return Err(ProviderError::InvalidResponse("the artwork was empty"));
            }
            Ok(ProviderImage {
                mime: mime_from(&response.headers),
                bytes: response.body,
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_file_supplies_the_credential_a_finder_launch_would_not_have() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("tmdb.key");
        std::fs::write(&path, "  3317a41fa5e5c79457e997a7f4b6d851\n").expect("key");

        let credential = ApiCredential::from_file(&path).expect("a key file is a credential");
        assert!(matches!(credential, ApiCredential::ApiKey(_)));
        assert!(
            format!("{credential:?}").contains("redacted"),
            "the credential must never print its own value"
        );

        let token = directory.path().join("read.token");
        std::fs::write(&token, "eyJhbGciOiJIUzI1NiJ9.payload.signature").expect("token");
        assert!(matches!(
            ApiCredential::from_file(&token),
            Some(ApiCredential::ReadToken(_))
        ));
    }

    #[test]
    fn an_empty_or_malformed_key_file_is_not_a_credential() {
        let directory = tempfile::tempdir().expect("tempdir");
        let empty = directory.path().join("empty.key");
        std::fs::write(&empty, "   \n").expect("empty");
        assert!(ApiCredential::from_file(&empty).is_none());

        let spaced = directory.path().join("spaced.key");
        std::fs::write(&spaced, "two words").expect("spaced");
        assert!(ApiCredential::from_file(&spaced).is_none());

        assert!(ApiCredential::from_file(&directory.path().join("absent.key")).is_none());
    }

    #[test]
    fn the_explicit_config_directory_wins_over_the_platform_default() {
        let directory = tempfile::tempdir().expect("tempdir");
        let previous = std::env::var_os(CREDENTIAL_DIRECTORY_ENVIRONMENT);
        unsafe { std::env::set_var(CREDENTIAL_DIRECTORY_ENVIRONMENT, directory.path()) };
        let resolved = default_credential_path().expect("a path");
        match previous {
            Some(value) => unsafe { std::env::set_var(CREDENTIAL_DIRECTORY_ENVIRONMENT, value) },
            None => unsafe { std::env::remove_var(CREDENTIAL_DIRECTORY_ENVIRONMENT) },
        }
        assert_eq!(resolved, directory.path().join(CREDENTIAL_FILE_NAME));
    }

    #[test]
    fn a_config_file_supplies_a_credential_without_exposing_it() {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("provider.conf");
        std::fs::write(
            &path,
            "api_key = file-key-123\nbase_url = http://provider.invalid/3\nlanguage = bn\n",
        )
        .expect("config");
        let config = ProviderConfig::load(&path).expect("loaded");
        assert_eq!(config.base_url, "http://provider.invalid/3");
        assert_eq!(config.language, "bn");
        let provider = TmdbProvider::new(config);
        let capability = provider.capability();
        assert!(capability.is_configured());
        assert_eq!(capability.language, "bn");
        let debug = format!("{provider:?}");
        assert!(!debug.contains("file-key-123"), "{debug}");
    }

    #[test]
    fn a_config_file_that_cannot_be_read_is_a_reported_error() {
        let directory = tempfile::tempdir().expect("tempdir");
        let error = ProviderConfig::load(&directory.path().join("absent.conf"))
            .expect_err("a missing file is an error");
        assert!(error.contains("cannot be read"), "{error}");
    }

    #[test]
    fn a_read_token_becomes_a_bearer_header_rather_than_a_query_parameter() {
        let config = ProviderConfig::configured(ApiCredential::ReadToken(Secret::new("token-abc")))
            .with_base_url("http://provider.invalid/3");
        let provider = TmdbProvider::with_system_clock(
            config,
            Box::new(crate::library::provider_http::TcpTransport::default()),
        );
        let request = provider.endpoint("/movie/55001", &[]);
        assert!(!request.url.contains("token-abc"));
        assert!(request.headers.iter().any(|(name, value)| {
            name.eq_ignore_ascii_case("authorization") && value == "Bearer token-abc"
        }));
        assert!(!format!("{request:?}").contains("token-abc"));
    }
}
