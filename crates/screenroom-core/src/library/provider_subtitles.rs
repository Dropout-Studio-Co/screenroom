use std::cell::RefCell;
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::provider_http::{
    BASE_BACKOFF_MS, Clock, DEFAULT_BUDGET_PER_MINUTE, DEFAULT_MAX_ATTEMPTS, DEFAULT_TIMEOUT,
    HttpRequest, HttpTransport, MAX_BACKOFF_MS, MAX_RESPONSE_BYTES, Pacer, ProviderError, Secret,
    Sleeper, SystemClock, ThreadSleeper, encode_query_value, join_url, scheme_is_carried,
};
use super::provider_tls::HttpsTransport;
use super::provider_tmdb::ProviderCapability;
use super::subtitles::{MAX_SUBTITLE_BYTES, SUBTITLE_EXTENSIONS, StoredSubtitle};
use super::{Library, new_id};
use crate::domain::MediaKind;
use crate::error::{CoreError, Result};

pub const TSDB_PROVIDER: &str = "thesubtitledb";

pub const TSDB_BASE_URL: &str = "https://api.thesubtitledb.org";

pub const TSDB_API_KEY_ENVIRONMENT: &str = "SCREENROOM_TSDB_API_KEY";

pub const TSDB_BASE_URL_ENVIRONMENT: &str = "SCREENROOM_TSDB_BASE_URL";

pub const TSDB_CONFIG_ENVIRONMENT: &str = "SCREENROOM_TSDB_CONFIG";

pub const TSDB_CONFIG_DIR_ENVIRONMENT: &str = "SCREENROOM_CONFIG_DIR";

pub const TSDB_USER_AGENT: &str = "Screenroom 0.1";

pub const TSDB_KEY_FILE_NAME: &str = "subtitles.key";

pub const DOWNLOAD_HOST_DOMAINS: &[&str] = &["thesubtitledb.org"];

pub const BENGALI_CODES: &[&str] = &["bn", "ben"];

pub const DEFAULT_SUBTITLE_FORMAT: &str = "srt";

pub const MAX_RELEASE_CHARS: usize = 200;

const KEY_FILE_MAX_BYTES: u64 = 1_024;

const KEY_MIN_CHARACTERS: usize = 8;

const KEY_MAX_CHARACTERS: usize = 512;

pub fn subtitle_language_codes(language: &str) -> Vec<String> {
    let trimmed = language.trim().to_ascii_lowercase();
    if trimmed.is_empty() {
        return Vec::new();
    }
    if trimmed == "bn" || trimmed == "ben" || trimmed == "bengali" || trimmed == "bangla" {
        return BENGALI_CODES
            .iter()
            .map(|code| (*code).to_string())
            .collect();
    }
    vec![
        trimmed
            .split(['-', '_'])
            .next()
            .unwrap_or(trimmed.as_str())
            .to_string(),
    ]
}

pub fn subtitle_language_name(code: &str) -> String {
    let lower = code.trim().to_ascii_lowercase();
    let name = match lower.as_str() {
        "bn" | "ben" | "bengali" => "Bengali",
        "en" | "eng" => "English",
        "hi" | "hin" => "Hindi",
        "ur" | "urd" => "Urdu",
        "ar" | "ara" => "Arabic",
        "es" | "spa" => "Spanish",
        "fr" | "fre" | "fra" => "French",
        "de" | "ger" | "deu" => "German",
        "it" | "ita" => "Italian",
        "pt" | "por" => "Portuguese",
        "ru" | "rus" => "Russian",
        "ja" | "jpn" => "Japanese",
        "ko" | "kor" => "Korean",
        "zh" | "chi" | "zho" => "Chinese",
        "ta" | "tam" => "Tamil",
        "te" | "tel" => "Telugu",
        "ml" | "mal" => "Malayalam",
        "id" | "ind" => "Indonesian",
        "tr" | "tur" => "Turkish",
        "nl" | "dut" | "nld" => "Dutch",
        "pl" | "pol" => "Polish",
        "fa" | "per" | "fas" => "Persian",
        _ => return code.trim().to_string(),
    };
    name.to_string()
}

#[derive(Debug)]
pub struct SubtitleProviderConfig {
    pub base_url: String,

    pub user_agent: String,

    pub api_key: Option<Secret>,

    pub timeout: Duration,

    pub max_bytes: usize,

    pub budget_per_minute: u32,

    pub max_attempts: u32,

    pub configuration_error: Option<String>,
}

impl Default for SubtitleProviderConfig {
    fn default() -> Self {
        Self::unconfigured()
    }
}

impl SubtitleProviderConfig {
    pub fn unconfigured() -> Self {
        Self {
            base_url: TSDB_BASE_URL.to_string(),
            user_agent: TSDB_USER_AGENT.to_string(),
            api_key: None,
            timeout: DEFAULT_TIMEOUT,
            max_bytes: MAX_RESPONSE_BYTES,
            budget_per_minute: DEFAULT_BUDGET_PER_MINUTE,
            max_attempts: DEFAULT_MAX_ATTEMPTS,
            configuration_error: None,
        }
    }

    pub fn with_api_key(mut self, key: impl Into<String>) -> Self {
        self.api_key = Some(Secret::new(key));
        self
    }

    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    pub fn with_user_agent(mut self, user_agent: impl Into<String>) -> Self {
        self.user_agent = user_agent.into();
        self
    }

    pub fn with_budget_per_minute(mut self, budget: u32) -> Self {
        self.budget_per_minute = budget.max(1);
        self
    }

    pub fn load(path: &Path) -> std::result::Result<Self, String> {
        let metadata = std::fs::metadata(path)
            .map_err(|_| "The subtitle provider configuration cannot be read.".to_string())?;
        if !metadata.is_file() || metadata.len() > 65_536 {
            return Err("The subtitle provider configuration cannot be used.".to_string());
        }
        let body = std::fs::read_to_string(path)
            .map_err(|_| "The subtitle provider configuration cannot be read.".to_string())?;
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
                "user_agent" => config.user_agent = value.to_string(),
                "api_key" => config.api_key = Some(Secret::new(value)),
                _ => {}
            }
        }
        Ok(config)
    }

    pub fn from_configuration_dir(dir: Option<&Path>) -> Self {
        let environment_key = environment_text(TSDB_API_KEY_ENVIRONMENT);
        let mut config = match environment_text(TSDB_CONFIG_ENVIRONMENT) {
            Some(path) => match Self::load(Path::new(&path)) {
                Ok(loaded) => loaded,
                Err(error) => Self {
                    configuration_error: Some(error),
                    ..Self::unconfigured()
                },
            },
            None => Self::unconfigured(),
        };
        if let Some(value) = environment_text(TSDB_BASE_URL_ENVIRONMENT) {
            config.base_url = value;
        }
        if let Some(key) = environment_key {
            if key_is_usable(&key) {
                config.api_key = Some(Secret::new(key));
                config.configuration_error = None;
            } else {
                config.api_key = None;
                config.configuration_error = Some(KEY_REJECTED.to_string());
            }
            return config;
        }
        if let Some(configured) = config.api_key.take() {
            if key_is_usable(configured.expose()) {
                config.api_key = Some(configured);
            } else {
                config.configuration_error = Some(KEY_REJECTED.to_string());
            }
            return config;
        }
        if config.configuration_error.is_some() {
            return config;
        }
        if let Some(directory) = configuration_directory(dir) {
            match read_key_file(&directory) {
                Ok(Some(key)) => config.api_key = Some(Secret::new(key)),
                Ok(None) => {}
                Err(error) => config.configuration_error = Some(error),
            }
        }
        config
    }

    pub fn from_environment() -> Self {
        Self::from_configuration_dir(None)
    }
}

const KEY_REJECTED: &str =
    "The subtitle key was rejected. A key is 8 to 512 printable characters with no spaces.";

const KEY_FILE_REJECTED: &str = "The subtitle key file was rejected. It must hold a key of 8 to 512 printable characters with \
     no spaces.";

const KEY_FILE_UNREADABLE: &str = "The subtitle key file cannot be read.";

const KEY_FILE_PERMISSIONS: &str = "The subtitle key file can be read by other accounts. Restrict it to your own, for example \
     with chmod 600.";

const KEY_FILE_UNUSABLE: &str =
    "The subtitle key file is not a regular file of at most 1024 bytes.";

fn environment_text(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn key_is_usable(key: &str) -> bool {
    (KEY_MIN_CHARACTERS..=KEY_MAX_CHARACTERS).contains(&key.len())
        && key.bytes().all(|byte| byte.is_ascii_graphic())
}

fn configuration_directory(explicit: Option<&Path>) -> Option<PathBuf> {
    if let Some(directory) = explicit {
        return Some(directory.to_path_buf());
    }
    if let Some(directory) = environment_text(TSDB_CONFIG_DIR_ENVIRONMENT) {
        return Some(PathBuf::from(directory));
    }
    default_configuration_directory()
}

fn default_configuration_directory() -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        let home = environment_text("HOME")?;
        return Some(
            Path::new(&home)
                .join("Library")
                .join("Application Support")
                .join("Screenroom"),
        );
    }
    if cfg!(windows) {
        let base = environment_text("APPDATA").or_else(|| environment_text("LOCALAPPDATA"))?;
        return Some(Path::new(&base).join("Screenroom"));
    }
    let home = environment_text("HOME")?;
    Some(
        Path::new(&home)
            .join(".local")
            .join("share")
            .join("Screenroom"),
    )
}

#[cfg(unix)]
fn key_file_is_private(metadata: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o077 == 0
}

#[cfg(not(unix))]
fn key_file_is_private(_metadata: &std::fs::Metadata) -> bool {
    true
}

fn read_key_file(directory: &Path) -> std::result::Result<Option<String>, String> {
    let path = directory.join(TSDB_KEY_FILE_NAME);
    let metadata = match std::fs::metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(KEY_FILE_UNREADABLE.to_string()),
    };
    if !metadata.is_file() || metadata.len() > KEY_FILE_MAX_BYTES {
        return Err(KEY_FILE_UNUSABLE.to_string());
    }
    if !key_file_is_private(&metadata) {
        return Err(KEY_FILE_PERMISSIONS.to_string());
    }
    let body = std::fs::read_to_string(&path).map_err(|_| KEY_FILE_UNREADABLE.to_string())?;
    let key = body.trim();
    if !key_is_usable(key) {
        return Err(KEY_FILE_REJECTED.to_string());
    }
    Ok(Some(key.to_string()))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubtitleSearch {
    pub title: String,

    pub year: Option<i64>,

    pub kind: MediaKind,

    pub season: Option<i64>,

    pub episode: Option<i64>,

    pub language: String,

    pub tmdb_id: Option<i64>,

    pub imdb_id: Option<String>,

    pub release_name: Option<String>,
}

impl SubtitleSearch {
    pub fn movie(title: &str, year: Option<i64>, language: &str) -> Self {
        Self {
            title: title.to_string(),
            year,
            kind: MediaKind::Movie,
            season: None,
            episode: None,
            language: language.to_string(),
            tmdb_id: None,
            imdb_id: None,
            release_name: None,
        }
    }

    pub fn episode(
        title: &str,
        year: Option<i64>,
        season: i64,
        episode: i64,
        language: &str,
    ) -> Self {
        Self {
            title: title.to_string(),
            year,
            kind: MediaKind::Series,
            season: Some(season),
            episode: Some(episode),
            language: language.to_string(),
            tmdb_id: None,
            imdb_id: None,
            release_name: None,
        }
    }

    pub fn with_tmdb(mut self, id: i64) -> Self {
        self.tmdb_id = Some(id);
        self
    }

    pub fn with_imdb(mut self, id: impl Into<String>) -> Self {
        self.imdb_id = Some(id.into());
        self
    }

    pub fn with_release(mut self, name: impl Into<String>) -> Self {
        self.release_name = Some(name.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubtitleMatch {
    pub id: i64,

    pub language: String,

    pub language_name: String,

    pub release_name: String,

    pub format: String,

    pub cues: i64,

    pub hearing_impaired: bool,

    pub ai_transcribed: bool,

    pub encoding: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DownloadedSubtitle {
    pub id: i64,

    pub language: String,

    pub language_name: String,

    pub release_name: String,

    pub format: String,

    pub bytes: Vec<u8>,
}

pub trait SubtitleProvider {
    fn provider_name(&self) -> &'static str;

    fn capability(&self) -> ProviderCapability;

    fn search(
        &self,
        request: &SubtitleSearch,
    ) -> std::result::Result<Vec<SubtitleMatch>, ProviderError>;

    fn download(
        &self,
        id: i64,
        language: &str,
        format_hint: &str,
    ) -> std::result::Result<DownloadedSubtitle, ProviderError>;
}

pub struct TsdbProvider {
    config: SubtitleProviderConfig,
    transport: Box<dyn HttpTransport>,
    pacer: RefCell<Pacer>,
}

impl fmt::Debug for TsdbProvider {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TsdbProvider")
            .field("base_url", &self.config.base_url)
            .field("keyed_search", &self.search_is_keyed())
            .finish_non_exhaustive()
    }
}

impl TsdbProvider {
    pub fn new(config: SubtitleProviderConfig) -> Self {
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
        config: SubtitleProviderConfig,
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

    pub fn with_system_clock(
        config: SubtitleProviderConfig,
        transport: Box<dyn HttpTransport>,
    ) -> Self {
        Self::with_parts(
            config,
            transport,
            Box::new(SystemClock),
            Box::new(ThreadSleeper),
        )
    }

    pub fn config(&self) -> &SubtitleProviderConfig {
        &self.config
    }

    pub fn search_is_keyed(&self) -> bool {
        self.config.api_key.is_some() && self.base_is_secure()
    }

    fn base_is_secure(&self) -> bool {
        self.config
            .base_url
            .trim()
            .to_ascii_lowercase()
            .starts_with("https://")
    }

    fn with_agent(&self, request: HttpRequest) -> HttpRequest {
        request
            .with_header("User-Agent", self.config.user_agent.clone())
            .with_header("Accept", "application/json")
    }

    fn with_key(&self, request: HttpRequest) -> HttpRequest {
        let request = self.with_agent(request);
        match &self.config.api_key {
            Some(key) if self.base_is_secure() => {
                request.with_header("Authorization", format!("Bearer {}", key.expose()))
            }
            _ => request,
        }
    }

    fn url_with_query(&self, path: &str, params: &[(&str, String)]) -> String {
        let mut url = join_url(&self.config.base_url, path);
        if params.is_empty() {
            return url;
        }
        let query: Vec<String> = params
            .iter()
            .map(|(name, value)| format!("{name}={}", encode_query_value(value)))
            .collect();
        url.push('?');
        url.push_str(&query.join("&"));
        url
    }

    fn lookup_url(&self, path: &str, languages: &[String]) -> String {
        self.url_with_query(
            path,
            &[
                ("lang", languages.join(",")),
                ("sort", "cues".to_string()),
                ("limit", "100".to_string()),
            ],
        )
    }

    fn lookup_urls(&self, request: &SubtitleSearch, languages: &[String]) -> Vec<String> {
        let mut urls = Vec::new();
        if let Some(tmdb_id) = request.tmdb_id.filter(|id| *id > 0) {
            match request.kind {
                MediaKind::Movie => {
                    urls.push(self.lookup_url(&format!("/v1/by-tmdb/movie/{tmdb_id}"), languages))
                }
                MediaKind::Series => {
                    if let (Some(season), Some(episode)) = (request.season, request.episode) {
                        urls.push(self.lookup_url(
                            &format!("/v1/by-tmdb/tv/{tmdb_id}/season/{season}/episode/{episode}"),
                            languages,
                        ));
                    }
                }
                MediaKind::Unknown => {}
            }
        }
        if let Some(imdb_id) = request.imdb_id.as_deref().and_then(usable_identifier) {
            let path = match (request.kind, request.season, request.episode) {
                (MediaKind::Series, Some(season), Some(episode)) => {
                    format!("/v1/by-imdb/{imdb_id}/season/{season}/episode/{episode}")
                }
                _ => format!("/v1/by-imdb/{imdb_id}"),
            };
            urls.push(self.lookup_url(&path, languages));
        }
        if let Some(release) = request
            .release_name
            .as_deref()
            .map(str::trim)
            .filter(|release| !release.is_empty())
        {
            urls.push(self.url_with_query(
                "/v1/by-releasename",
                &[
                    ("release", release.to_string()),
                    ("lang", languages.join(",")),
                    ("sort", "cues".to_string()),
                    ("limit", "100".to_string()),
                ],
            ));
        }
        let title = request.title.trim();
        if !title.is_empty()
            && let Some(kind) = kind_parameter(request.kind)
        {
            urls.push(self.url_with_query(
                "/v1/by-title",
                &[
                    ("q", title.to_string()),
                    ("type", kind.to_string()),
                    ("lang", languages.join(",")),
                    ("sort", "cues".to_string()),
                    ("limit", "100".to_string()),
                ],
            ));
        }
        urls
    }

    fn lookup(
        &self,
        url: &str,
        request: &SubtitleSearch,
    ) -> std::result::Result<Option<Vec<SubtitleMatch>>, ProviderError> {
        let mut http = self.with_agent(HttpRequest::get(url.to_string()));
        http.max_bytes = self.config.max_bytes;
        http.timeout = self.config.timeout;
        let value = {
            let mut pacer = self.pacer.borrow_mut();
            match pacer.execute(|| self.value_from(&http)) {
                Ok(value) => value,
                Err(ProviderError::NotFound) => return Ok(None),
                Err(error) => return Err(error),
            }
        };
        self.matches_from(&value, request)
    }

    fn search_by_title(
        &self,
        request: &SubtitleSearch,
        languages: &[String],
    ) -> std::result::Result<Option<Vec<SubtitleMatch>>, ProviderError> {
        let title = request.title.trim();
        let Some(kind) = kind_parameter(request.kind) else {
            return Ok(None);
        };
        if title.is_empty() {
            return Ok(None);
        }
        let url = self.url_with_query(
            "/v1/by-search",
            &[
                ("q", title.to_string()),
                ("type", kind.to_string()),
                ("lang", languages.join(",")),
                ("sort", "cues".to_string()),
                ("limit", "100".to_string()),
            ],
        );
        let mut http = self.with_key(HttpRequest::get(url));
        http.max_bytes = self.config.max_bytes;
        http.timeout = self.config.timeout;
        let value = {
            let mut pacer = self.pacer.borrow_mut();
            match pacer.execute(|| self.value_from(&http)) {
                Ok(value) => value,
                Err(ProviderError::NotFound) => return Ok(None),
                Err(error) => return Err(error),
            }
        };
        let results = value.get("results").and_then(Value::as_array).ok_or(
            ProviderError::InvalidResponse("the search had no results list"),
        )?;
        let Some(chosen) = choose_search_result(results, request, languages) else {
            return Ok(None);
        };
        let Some(url) = self.result_lookup_url(chosen, request, languages) else {
            return Ok(None);
        };
        self.lookup(&url, request)
    }

    fn result_lookup_url(
        &self,
        result: &Value,
        request: &SubtitleSearch,
        languages: &[String],
    ) -> Option<String> {
        let tmdb_id = result
            .get("tmdb_id")
            .and_then(Value::as_i64)
            .filter(|id| *id > 0);
        let imdb_id = result
            .get("imdb")
            .and_then(Value::as_str)
            .and_then(usable_identifier);
        match (request.kind, request.season, request.episode) {
            (MediaKind::Movie, _, _) => match (tmdb_id, imdb_id) {
                (Some(tmdb_id), _) => {
                    Some(self.lookup_url(&format!("/v1/by-tmdb/movie/{tmdb_id}"), languages))
                }
                (None, Some(imdb_id)) => {
                    Some(self.lookup_url(&format!("/v1/by-imdb/{imdb_id}"), languages))
                }
                (None, None) => None,
            },
            (MediaKind::Series, Some(season), Some(episode)) => match (tmdb_id, imdb_id) {
                (Some(tmdb_id), _) => Some(self.lookup_url(
                    &format!("/v1/by-tmdb/tv/{tmdb_id}/season/{season}/episode/{episode}"),
                    languages,
                )),
                (None, Some(imdb_id)) => Some(self.lookup_url(
                    &format!("/v1/by-imdb/{imdb_id}/season/{season}/episode/{episode}"),
                    languages,
                )),
                (None, None) => None,
            },
            (MediaKind::Series, _, _) => {
                imdb_id.map(|imdb_id| self.lookup_url(&format!("/v1/by-imdb/{imdb_id}"), languages))
            }
            (MediaKind::Unknown, _, _) => None,
        }
    }

    fn value_from(&self, request: &HttpRequest) -> std::result::Result<Value, ProviderError> {
        let response = self.transport.send(request)?;
        if !(200..300).contains(&response.status) {
            return Err(status_error(response.status, response.retry_after_ms()));
        }
        serde_json::from_slice(&response.body)
            .map_err(|_| ProviderError::InvalidResponse("the body was not JSON"))
    }

    fn matches_from(
        &self,
        value: &Value,
        request: &SubtitleSearch,
    ) -> std::result::Result<Option<Vec<SubtitleMatch>>, ProviderError> {
        let title = value.get("title").ok_or(ProviderError::InvalidResponse(
            "the answer had no title object",
        ))?;
        if !title_matches(title, request) {
            return Ok(None);
        }
        let items = value
            .get("subtitles")
            .and_then(|subtitles| subtitles.get("items"))
            .and_then(Value::as_array)
            .ok_or(ProviderError::InvalidResponse(
                "the answer had no subtitle list",
            ))?;
        let matches: Vec<SubtitleMatch> = items.iter().filter_map(subtitle_match_from).collect();
        if matches.is_empty() {
            Ok(None)
        } else {
            Ok(Some(matches))
        }
    }

    fn download_host_allowed(&self, host: &str) -> bool {
        let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
        if host.is_empty() || host_is_loopback_or_local(&host) {
            return false;
        }
        if let Some((_, base_host)) = absolute_url_parts(&self.config.base_url)
            && base_host.eq_ignore_ascii_case(&host)
        {
            return true;
        }
        DOWNLOAD_HOST_DOMAINS
            .iter()
            .any(|domain| host == *domain || host.ends_with(&format!(".{domain}")))
    }

    fn secure_download_url(&self, url: &str) -> std::result::Result<String, ProviderError> {
        let Some((scheme, host)) = absolute_url_parts(url) else {
            return Err(ProviderError::InvalidResponse(
                "the download address was unusable",
            ));
        };
        if scheme != "https" {
            return Err(ProviderError::InvalidResponse(
                "the download address was not secure",
            ));
        }
        if !self.download_host_allowed(&host) {
            return Err(ProviderError::InvalidResponse(
                "the download link pointed outside the configured provider",
            ));
        }
        Ok(url.to_string())
    }
}

fn kind_parameter(kind: MediaKind) -> Option<&'static str> {
    match kind {
        MediaKind::Movie => Some("movie"),
        MediaKind::Series => Some("tv"),
        MediaKind::Unknown => None,
    }
}

fn usable_identifier(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    (!trimmed.is_empty()
        && trimmed.len() <= 32
        && trimmed
            .chars()
            .all(|character| character.is_ascii_alphanumeric()))
    .then_some(trimmed)
}

fn years_apart(remote: i64, requested: i64) -> bool {
    remote.saturating_sub(requested).unsigned_abs() > 1
}

fn title_matches(title: &Value, request: &SubtitleSearch) -> bool {
    let Some(expected) = kind_parameter(request.kind) else {
        return false;
    };
    let actual = title
        .get("media_type")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if actual != expected {
        return false;
    }
    if let (Some(remote), Some(requested)) =
        (title.get("year").and_then(Value::as_i64), request.year)
        && years_apart(remote, requested)
    {
        return false;
    }
    true
}

fn choose_search_result<'a>(
    results: &'a [Value],
    request: &SubtitleSearch,
    languages: &[String],
) -> Option<&'a Value> {
    let wanted = request.title.trim().to_lowercase();
    let expected = kind_parameter(request.kind)?;
    let preferred_language = languages.first().map(String::as_str).unwrap_or("en");
    let mut best: Option<(&Value, i64)> = None;
    for result in results {
        let name = result
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_lowercase();
        if name != wanted {
            continue;
        }
        let kind = result
            .get("media_type")
            .and_then(Value::as_str)
            .or_else(|| result.get("kind").and_then(Value::as_str))
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        if kind != expected {
            continue;
        }
        if let (Some(remote), Some(requested)) =
            (result.get("year").and_then(Value::as_i64), request.year)
            && years_apart(remote, requested)
        {
            continue;
        }
        let count = result
            .get("language_counts")
            .and_then(|counts| counts.get(preferred_language))
            .and_then(Value::as_i64)
            .unwrap_or_default();
        match best {
            Some((_, best_count)) if count <= best_count => {}
            _ => best = Some((result, count)),
        }
    }
    best.map(|(result, _)| result)
}

fn subtitle_match_from(item: &Value) -> Option<SubtitleMatch> {
    let id = item.get("id").and_then(Value::as_i64)?;
    let language = text(item, "language")?;
    let format = text(item, "format")?.to_ascii_lowercase();
    if !SUBTITLE_EXTENSIONS.contains(&format.as_str()) {
        return None;
    }
    Some(SubtitleMatch {
        id,
        language_name: subtitle_language_name(&language),
        language,
        release_name: clean_release(
            item.get("release_name")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        ),
        format,
        cues: integer(item, "cues").unwrap_or_default(),
        hearing_impaired: boolean(item, "hearing_impaired"),
        ai_transcribed: boolean(item, "ai_transcribed"),
        encoding: text(item, "encoding").unwrap_or_default(),
    })
}

fn status_error(status: u16, retry_after_ms: Option<i64>) -> ProviderError {
    if status == 503 && retry_after_ms.is_some() {
        return ProviderError::RateLimited { retry_after_ms };
    }
    ProviderError::from_status(status, retry_after_ms)
}

fn absolute_url_parts(url: &str) -> Option<(String, String)> {
    if url.chars().any(|character| character.is_control()) {
        return None;
    }
    let (scheme, rest) = url.split_once("://")?;
    let scheme = scheme.trim().to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return None;
    }
    let authority = rest.split(['/', '?', '#']).next()?;
    if authority.is_empty()
        || authority.contains('@')
        || authority.contains(char::is_whitespace)
        || authority.contains('\\')
    {
        return None;
    }
    let host = match authority.strip_prefix('[') {
        Some(bracketed) => bracketed.split_once(']')?.0.to_string(),
        None => authority.split(':').next()?.to_string(),
    };
    if host.is_empty() {
        return None;
    }
    Some((scheme, host))
}

fn host_is_loopback_or_local(host: &str) -> bool {
    host.parse::<std::net::IpAddr>().is_ok()
        || host == "localhost"
        || host.ends_with(".localhost")
        || host.ends_with(".local")
}

fn text(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

fn integer(value: &Value, key: &str) -> Option<i64> {
    value.get(key).and_then(Value::as_i64)
}

fn boolean(value: &Value, key: &str) -> bool {
    value.get(key).and_then(Value::as_bool).unwrap_or(false)
}

impl SubtitleProvider for TsdbProvider {
    fn provider_name(&self) -> &'static str {
        TSDB_PROVIDER
    }

    fn capability(&self) -> ProviderCapability {
        if let Some(error) = &self.config.configuration_error {
            return ProviderCapability::not_configured(TSDB_PROVIDER, error.clone());
        }
        if let Err(reason) = scheme_is_carried(self.transport.as_ref(), &self.config.base_url) {
            return ProviderCapability::not_configured(TSDB_PROVIDER, reason);
        }
        if self.config.api_key.is_some() && !self.base_is_secure() {
            return ProviderCapability::not_configured(
                TSDB_PROVIDER,
                "The subtitle key is not sent over plain http. Point the provider at an https \
                 address before using a key.",
            );
        }
        let mut capability = ProviderCapability::configured(TSDB_PROVIDER, "en");
        capability.detail = if self.search_is_keyed() {
            "Online subtitle lookups and keyed title search are configured for The Subtitle DB."
                .to_string()
        } else {
            "Online subtitle lookups work for The Subtitle DB without a key; keyed title search \
             needs a free The Subtitle DB key."
                .to_string()
        };
        capability
    }

    fn search(
        &self,
        request: &SubtitleSearch,
    ) -> std::result::Result<Vec<SubtitleMatch>, ProviderError> {
        if self.config.configuration_error.is_some() {
            return Err(ProviderError::NotConfigured);
        }
        let languages = subtitle_language_codes(&request.language);
        if languages.is_empty() || kind_parameter(request.kind).is_none() {
            return Ok(Vec::new());
        }
        for url in self.lookup_urls(request, &languages) {
            if let Some(matches) = self.lookup(&url, request)? {
                return Ok(matches);
            }
        }
        if self.search_is_keyed()
            && let Some(matches) = self.search_by_title(request, &languages)?
        {
            return Ok(matches);
        }
        Ok(Vec::new())
    }

    fn download(
        &self,
        id: i64,
        language: &str,
        format_hint: &str,
    ) -> std::result::Result<DownloadedSubtitle, ProviderError> {
        if self.config.configuration_error.is_some() {
            return Err(ProviderError::NotConfigured);
        }
        if id <= 0 {
            return Err(ProviderError::InvalidResponse(
                "the download handle was not usable",
            ));
        }
        let url =
            self.secure_download_url(&join_url(&self.config.base_url, &format!("/get/{id}")))?;
        let mut request = HttpRequest::get(url)
            .with_header("User-Agent", self.config.user_agent.clone())
            .with_header("Accept", "*/*")
            .with_max_bytes(MAX_SUBTITLE_BYTES as usize)
            .with_redirect_hosts(DOWNLOAD_HOST_DOMAINS.iter().copied());
        if let Some(key) = &self.config.api_key {
            request = request.with_header("Authorization", format!("Bearer {}", key.expose()));
        }
        request.timeout = self.config.timeout;
        let response = {
            let mut pacer = self.pacer.borrow_mut();
            pacer.execute(|| {
                let response = self.transport.send(&request)?;
                if !(200..300).contains(&response.status) {
                    return Err(status_error(response.status, response.retry_after_ms()));
                }
                Ok(response)
            })?
        };
        let hint = format_hint.trim().to_ascii_lowercase();
        let format = if SUBTITLE_EXTENSIONS.contains(&hint.as_str()) {
            hint
        } else {
            DEFAULT_SUBTITLE_FORMAT.to_string()
        };
        Ok(DownloadedSubtitle {
            id,
            language: language.to_string(),
            language_name: subtitle_language_name(language),
            release_name: String::new(),
            format,
            bytes: response.body,
        })
    }
}

fn clean_release(value: &str) -> String {
    let cleaned: String = value
        .chars()
        .filter(|character| !character.is_control())
        .collect();
    cleaned.trim().chars().take(MAX_RELEASE_CHARS).collect()
}

fn looks_textual(bytes: &[u8]) -> bool {
    let sample = &bytes[..bytes.len().min(8_192)];
    !sample.contains(&0) && sample.iter().any(|byte| !byte.is_ascii_whitespace())
}

impl Library {
    pub fn import_downloaded_subtitle(
        &self,
        file_id: &str,
        downloaded: &DownloadedSubtitle,
    ) -> Result<StoredSubtitle> {
        if downloaded.bytes.is_empty() || downloaded.bytes.len() as u64 > MAX_SUBTITLE_BYTES {
            return Err(CoreError::library(
                "subtitle_size",
                "That subtitle is empty or larger than 4 MiB.",
            ));
        }
        if !looks_textual(&downloaded.bytes) {
            return Err(CoreError::library(
                "subtitle_binary",
                "That download was not a text subtitle.",
            ));
        }
        let format = downloaded.format.trim().to_ascii_lowercase();
        if !SUBTITLE_EXTENSIONS.contains(&format.as_str()) {
            return Err(CoreError::library(
                "subtitle_format",
                "That download was not a subtitle format Screenroom can keep.",
            ));
        }
        let known: bool = self.connection().query_row(
            "SELECT EXISTS(SELECT 1 FROM files WHERE id = ?1)",
            [file_id],
            |row| row.get(0),
        )?;
        if !known {
            return Err(CoreError::library(
                "subtitle_file",
                "That file is no longer in the library.",
            ));
        }
        let release = clean_release(&downloaded.release_name);
        let language = subtitle_language_name(&downloaded.language);
        let label = if release.is_empty() {
            format!("{language} · downloaded")
        } else {
            format!("{language} · downloaded · {release}")
        };
        let stored = StoredSubtitle {
            id: new_id(),
            file_id: file_id.to_string(),
            label,
            language,
            format,
            provenance: "downloaded from The Subtitle DB".to_string(),
            bytes: downloaded.bytes.len() as i64,
            created_ms: super::now_ms(),
        };
        self.connection().execute(
            "INSERT INTO subtitles(id, file_id, label, language, format, provenance, bytes, created_ms)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                stored.id,
                stored.file_id,
                stored.label,
                stored.language,
                stored.format,
                stored.provenance,
                downloaded.bytes,
                stored.created_ms
            ],
        )?;
        Ok(stored)
    }
}
