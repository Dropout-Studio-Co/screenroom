use std::cell::RefCell;
use std::fmt;
use std::path::Path;
use std::time::Duration;

use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

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

pub const OPENSUBTITLES_PROVIDER: &str = "opensubtitles";

pub const OPENSUBTITLES_BASE_URL: &str = "https://api.opensubtitles.com/api/v1";

pub const OPENSUBTITLES_API_KEY_ENVIRONMENT: &str = "SCREENROOM_OPENSUBTITLES_API_KEY";

pub const OPENSUBTITLES_BASE_URL_ENVIRONMENT: &str = "SCREENROOM_OPENSUBTITLES_BASE_URL";

pub const OPENSUBTITLES_CONFIG_ENVIRONMENT: &str = "SCREENROOM_OPENSUBTITLES_CONFIG";

pub const OPENSUBTITLES_USER_AGENT: &str = "Screenroom 0.1";

pub const BENGALI_CODES: &[&str] = &["bn", "ben"];

pub const DEFAULT_SUBTITLE_FORMAT: &str = "srt";

pub const MAX_RELEASE_CHARS: usize = 200;

pub const DOWNLOAD_HOST_DOMAINS: &[&str] = &["opensubtitles.org", "opensubtitles.com"];

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
            base_url: OPENSUBTITLES_BASE_URL.to_string(),
            user_agent: OPENSUBTITLES_USER_AGENT.to_string(),
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

    pub fn from_environment() -> Self {
        let mut config = match environment_text(OPENSUBTITLES_CONFIG_ENVIRONMENT) {
            Some(path) => match Self::load(Path::new(&path)) {
                Ok(loaded) => loaded,
                Err(error) => Self {
                    configuration_error: Some(error),
                    ..Self::unconfigured()
                },
            },
            None => Self::unconfigured(),
        };
        if let Some(value) = environment_text(OPENSUBTITLES_BASE_URL_ENVIRONMENT) {
            config.base_url = value;
        }
        if config.api_key.is_none()
            && let Some(key) = environment_text(OPENSUBTITLES_API_KEY_ENVIRONMENT)
        {
            config.api_key = Some(Secret::new(key));
        }
        config
    }
}

fn environment_text(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubtitleSearch {
    pub title: String,

    pub year: Option<i64>,

    pub kind: MediaKind,

    pub season: Option<i64>,

    pub episode: Option<i64>,

    pub language: String,

    pub file_hash: Option<String>,
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
            file_hash: None,
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
            file_hash: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubtitleMatch {
    pub id: String,

    pub file_id: i64,

    pub language: String,

    pub language_name: String,

    pub release_name: String,

    pub format: String,

    pub downloads: i64,

    pub hearing_impaired: bool,

    pub file_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DownloadedSubtitle {
    pub file_id: i64,

    pub language: String,

    pub language_name: String,

    pub release_name: String,

    pub format: String,

    pub bytes: Vec<u8>,

    pub remaining_downloads: Option<i64>,
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
        file_id: i64,
        language: &str,
        format_hint: &str,
    ) -> std::result::Result<DownloadedSubtitle, ProviderError>;
}

pub struct OpenSubtitlesProvider {
    config: SubtitleProviderConfig,
    transport: Box<dyn HttpTransport>,
    pacer: RefCell<Pacer>,
}

impl fmt::Debug for OpenSubtitlesProvider {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OpenSubtitlesProvider")
            .field("base_url", &self.config.base_url)
            .field("configured", &self.config.api_key.is_some())
            .finish_non_exhaustive()
    }
}

impl OpenSubtitlesProvider {
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

    fn configured(&self) -> bool {
        self.config.api_key.is_some()
    }

    fn with_key(&self, request: HttpRequest) -> HttpRequest {
        let request = self.with_agent(request);
        match &self.config.api_key {
            Some(key) => request.with_header("Api-Key", key.expose()),
            None => request,
        }
    }

    fn with_agent(&self, request: HttpRequest) -> HttpRequest {
        request
            .with_header("User-Agent", self.config.user_agent.clone())
            .with_header("Accept", "application/json")
    }

    fn search_value(&self, request: &SubtitleSearch) -> std::result::Result<Value, ProviderError> {
        if !self.configured() {
            return Err(ProviderError::NotConfigured);
        }
        let languages = subtitle_language_codes(&request.language);
        let mut params = vec![
            ("query", request.title.trim().to_string()),
            ("languages", languages.join(",")),
        ];
        if let Some(year) = request.year {
            params.push(("year", year.to_string()));
        }
        if request.kind == MediaKind::Series {
            params.push(("type", "episode".to_string()));
            if let Some(season) = request.season {
                params.push(("season_number", season.to_string()));
            }
            if let Some(episode) = request.episode {
                params.push(("episode_number", episode.to_string()));
            }
        } else {
            params.push(("type", "movie".to_string()));
        }
        if let Some(hash) = request
            .file_hash
            .as_deref()
            .map(str::trim)
            .filter(|hash| !hash.is_empty())
        {
            params.push(("moviehash", hash.to_string()));
        }
        let mut url = join_url(&self.config.base_url, "/subtitles");
        let query: Vec<String> = params
            .iter()
            .map(|(name, value)| format!("{name}={}", encode_query_value(value)))
            .collect();
        url.push('?');
        url.push_str(&query.join("&"));
        let mut request = self.with_key(HttpRequest::get(url));
        request.max_bytes = self.config.max_bytes;
        request.timeout = self.config.timeout;
        let mut pacer = self.pacer.borrow_mut();
        pacer.execute(|| self.value_from(&request))
    }

    fn value_from(&self, request: &HttpRequest) -> std::result::Result<Value, ProviderError> {
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

    fn resolve_link(&self, link: &str) -> std::result::Result<String, ProviderError> {
        let link = link.trim();
        if link.is_empty() {
            return Err(ProviderError::InvalidResponse("the download had no link"));
        }
        if link.starts_with("http://") || link.starts_with("https://") {
            let Some((_, host)) = absolute_url_parts(link) else {
                return Err(ProviderError::InvalidResponse(
                    "the download link was unusable",
                ));
            };
            if !self.download_host_allowed(&host) {
                return Err(ProviderError::InvalidResponse(
                    "the download link pointed outside the configured provider",
                ));
            }
            return Ok(link.to_string());
        }
        if link.starts_with('/') {
            let base = &self.config.base_url;
            let origin = match base.split_once("://") {
                Some((scheme, rest)) => match rest.find('/') {
                    Some(index) => format!("{scheme}://{}", &rest[..index]),
                    None => base.trim_end_matches('/').to_string(),
                },
                None => {
                    return Err(ProviderError::InvalidResponse(
                        "the download link was unusable",
                    ));
                }
            };
            return Ok(format!("{origin}{link}"));
        }
        Err(ProviderError::InvalidResponse(
            "the download link was unusable",
        ))
    }
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

fn format_from_name(name: &str) -> Option<String> {
    let extension = name.rsplit_once('.')?.1.trim().to_ascii_lowercase();
    SUBTITLE_EXTENSIONS
        .contains(&extension.as_str())
        .then_some(extension)
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

fn match_from(value: &Value) -> Option<SubtitleMatch> {
    let attributes = value.get("attributes")?;
    let id = match value.get("id")? {
        Value::String(id) => id.clone(),
        Value::Number(number) => number.to_string(),
        _ => return None,
    };
    let files = attributes.get("files").and_then(Value::as_array);
    let first_file = files.and_then(|entries| entries.first());
    let file_id = first_file
        .and_then(|file| integer(file, "file_id"))
        .or_else(|| integer(attributes, "file_id"))?;
    let file_name = first_file
        .and_then(|file| text(file, "file_name"))
        .or_else(|| text(attributes, "file_name"))
        .unwrap_or_default();
    let language = text(attributes, "language").unwrap_or_default();
    if language.is_empty() {
        return None;
    }
    let release_name = text(attributes, "release")
        .or_else(|| first_file.and_then(|file| text(file, "release")))
        .or_else(|| (!file_name.is_empty()).then(|| file_name.clone()))
        .unwrap_or_default();
    let format = text(attributes, "format")
        .or_else(|| format_from_name(&file_name))
        .unwrap_or_else(|| DEFAULT_SUBTITLE_FORMAT.to_string());
    Some(SubtitleMatch {
        id,
        file_id,
        language_name: subtitle_language_name(&language),
        language,
        release_name,
        format,
        downloads: integer(attributes, "download_count").unwrap_or_default(),
        hearing_impaired: attributes
            .get("hearing_impaired")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        file_name,
    })
}

impl SubtitleProvider for OpenSubtitlesProvider {
    fn provider_name(&self) -> &'static str {
        OPENSUBTITLES_PROVIDER
    }

    fn capability(&self) -> ProviderCapability {
        if let Some(error) = &self.config.configuration_error {
            return ProviderCapability::not_configured(OPENSUBTITLES_PROVIDER, error.clone());
        }
        if self.config.api_key.is_none() {
            return ProviderCapability::not_configured(
                OPENSUBTITLES_PROVIDER,
                "Subtitle search is not configured. Set SCREENROOM_OPENSUBTITLES_API_KEY or \
                 SCREENROOM_OPENSUBTITLES_CONFIG first.",
            );
        }
        if let Err(reason) = scheme_is_carried(self.transport.as_ref(), &self.config.base_url) {
            return ProviderCapability::not_configured(OPENSUBTITLES_PROVIDER, reason);
        }
        ProviderCapability::configured(OPENSUBTITLES_PROVIDER, "en")
    }

    fn search(
        &self,
        request: &SubtitleSearch,
    ) -> std::result::Result<Vec<SubtitleMatch>, ProviderError> {
        let value = self.search_value(request)?;
        let Some(data) = value.get("data").and_then(Value::as_array) else {
            return Err(ProviderError::InvalidResponse(
                "the search had no data list",
            ));
        };
        Ok(data.iter().filter_map(match_from).collect())
    }

    fn download(
        &self,
        file_id: i64,
        language: &str,
        format_hint: &str,
    ) -> std::result::Result<DownloadedSubtitle, ProviderError> {
        if !self.configured() {
            return Err(ProviderError::NotConfigured);
        }
        if file_id <= 0 {
            return Err(ProviderError::InvalidResponse(
                "the download handle was not usable",
            ));
        }
        let url = join_url(&self.config.base_url, "/download");
        let body = json!({ "file_id": file_id }).to_string().into_bytes();
        let mut request = self
            .with_key(HttpRequest::post(url, body))
            .with_header("Content-Type", "application/json");
        request.timeout = self.config.timeout;
        request.max_bytes = self.config.max_bytes;
        let value = {
            let mut pacer = self.pacer.borrow_mut();
            pacer.execute(|| self.value_from(&request))?
        };
        let link = text(&value, "link")
            .ok_or(ProviderError::InvalidResponse("the download had no link"))?;
        let file_name = text(&value, "file_name").unwrap_or_default();
        let resolved = self.resolve_link(&link)?;
        let response = {
            let mut request = HttpRequest::get(resolved)
                .with_max_bytes(MAX_SUBTITLE_BYTES as usize)
                .with_header("User-Agent", self.config.user_agent.clone())
                .with_header("Accept", "*/*");
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
                Ok(response)
            })?
        };
        let format = format_from_name(&file_name)
            .or_else(|| {
                let hint = format_hint.trim().to_ascii_lowercase();
                SUBTITLE_EXTENSIONS.contains(&hint.as_str()).then_some(hint)
            })
            .unwrap_or_else(|| DEFAULT_SUBTITLE_FORMAT.to_string());
        Ok(DownloadedSubtitle {
            file_id,
            language: language.to_string(),
            language_name: subtitle_language_name(language),
            release_name: file_name,
            format,
            bytes: response.body,
            remaining_downloads: integer(&value, "remaining"),
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
            provenance: "downloaded from OpenSubtitles".to_string(),
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
