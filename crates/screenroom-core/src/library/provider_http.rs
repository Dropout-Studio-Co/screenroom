use std::fmt;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

pub const MAX_TRAILER_LINES: usize = 32;

pub const MAX_TRAILER_BYTES: usize = 8 * 1024;

pub const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

pub const MAX_RESPONSE_BYTES: usize = 512 * 1024;

pub const MAX_TRANSFER_BYTES: usize = 8 * 1024 * 1024;

pub const MAX_HEADER_BYTES: usize = 64 * 1024;

pub const DEFAULT_BUDGET_PER_MINUTE: u32 = 120;

pub const DEFAULT_MAX_ATTEMPTS: u32 = 3;

pub const BASE_BACKOFF_MS: i64 = 500;

pub const MAX_BACKOFF_MS: i64 = 30_000;

pub const MAX_RETRY_AFTER_MS: i64 = 60_000;

pub const RATE_WINDOW_MS: i64 = 60_000;

pub struct Secret(String);

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn expose(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Secret(redacted)")
    }
}

impl fmt::Display for Secret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("redacted")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpMethod {
    Get,

    Post,
}

impl HttpMethod {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct HttpRequest {
    pub method: HttpMethod,

    pub url: String,

    pub headers: Vec<(String, String)>,

    pub body: Vec<u8>,

    pub timeout: Duration,

    pub max_bytes: usize,

    pub redirect_hosts: Vec<String>,
}

impl HttpRequest {
    pub fn get(url: impl Into<String>) -> Self {
        Self {
            method: HttpMethod::Get,
            url: url.into(),
            headers: Vec::new(),
            body: Vec::new(),
            timeout: DEFAULT_TIMEOUT,
            max_bytes: MAX_RESPONSE_BYTES,
            redirect_hosts: Vec::new(),
        }
    }

    pub fn post(url: impl Into<String>, body: Vec<u8>) -> Self {
        Self {
            method: HttpMethod::Post,
            url: url.into(),
            headers: Vec::new(),
            body,
            timeout: DEFAULT_TIMEOUT,
            max_bytes: MAX_RESPONSE_BYTES,
            redirect_hosts: Vec::new(),
        }
    }

    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    pub fn with_max_bytes(mut self, max_bytes: usize) -> Self {
        self.max_bytes = max_bytes;
        self
    }

    pub fn with_redirect_hosts(
        mut self,
        hosts: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.redirect_hosts = hosts.into_iter().map(Into::into).collect();
        self
    }

    pub fn redacted_url(&self) -> String {
        redact_url(&self.url)
    }

    pub(crate) fn validate(&self) -> Result<(), TransportError> {
        if self.body.len() > self.max_bytes {
            return Err(TransportError::InvalidRequest(
                "the request body is larger than the response budget",
            ));
        }
        for (name, value) in &self.headers {
            if name.is_empty()
                || name.contains(':')
                || name.contains('\r')
                || name.contains('\n')
                || value.contains('\r')
                || value.contains('\n')
            {
                return Err(TransportError::InvalidRequest(
                    "a header is not well formed",
                ));
            }
        }
        Ok(())
    }
}

impl fmt::Debug for HttpRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HttpRequest")
            .field("method", &self.method)
            .field("url", &self.redacted_url())
            .field("headers", &self.headers.len())
            .field("body", &self.body.len())
            .field("timeout", &self.timeout)
            .field("max_bytes", &self.max_bytes)
            .field("redirect_hosts", &self.redirect_hosts)
            .finish()
    }
}

pub fn redact_url(url: &str) -> String {
    let Some((base, query)) = url.split_once('?') else {
        return url.to_string();
    };
    let redacted: Vec<String> = query
        .split('&')
        .map(|pair| match pair.split_once('=') {
            Some((name, _)) if is_secret_parameter(name) => format!("{name}=redacted"),
            _ => pair.to_string(),
        })
        .collect();
    format!("{base}?{}", redacted.join("&"))
}

fn is_secret_parameter(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.contains("key")
        || lower.contains("token")
        || lower.contains("secret")
        || lower == "password"
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,

    pub headers: Vec<(String, String)>,

    pub body: Vec<u8>,
}

impl HttpResponse {
    pub fn new(status: u16, body: Vec<u8>) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body,
        }
    }

    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    pub fn retry_after_ms(&self) -> Option<i64> {
        let value = self.header("retry-after")?.trim();
        let seconds = value.parse::<i64>().ok()?;
        Some(seconds.clamp(0, MAX_RETRY_AFTER_MS / 1_000) * 1_000)
    }
}

#[derive(Debug)]
pub enum TransportError {
    InvalidUrl,

    UnsupportedScheme(String),

    InvalidRequest(&'static str),

    Io(std::io::Error),

    Timeout { after: Duration },

    TooLarge { limit: usize },

    CertificateNotTrusted,
}

impl TransportError {
    pub fn retryable(&self) -> bool {
        match self {
            Self::Io(error) => !matches!(
                error.kind(),
                std::io::ErrorKind::InvalidInput | std::io::ErrorKind::NotFound
            ),
            Self::Timeout { .. } => true,
            _ => false,
        }
    }

    pub(crate) fn into_timeout(self, after: Duration) -> Self {
        match self {
            Self::Io(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                ) =>
            {
                Self::Timeout { after }
            }
            other => other,
        }
    }
}

impl fmt::Display for TransportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidUrl => formatter.write_str("that address is not a usable http address"),
            Self::UnsupportedScheme(scheme) => write!(
                formatter,
                "{scheme} needs a TLS transport that is not built in; point the provider at an \
                 http endpoint or supply a transport"
            ),
            Self::InvalidRequest(reason) => formatter.write_str(reason),
            Self::Io(error) => write!(formatter, "{error}"),
            Self::Timeout { after } => write!(
                formatter,
                "the request did not finish within {} ms, so it was abandoned",
                after.as_millis()
            ),
            Self::TooLarge { limit } => {
                write!(
                    formatter,
                    "the answer was larger than the {limit} byte budget"
                )
            }
            Self::CertificateNotTrusted => formatter
                .write_str("the server certificate was not trusted, so the request was refused"),
        }
    }
}

impl std::error::Error for TransportError {}

impl From<std::io::Error> for TransportError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

pub trait HttpTransport {
    fn send(&self, request: &HttpRequest) -> Result<HttpResponse, TransportError>;

    fn supports_scheme(&self, scheme: &str) -> bool {
        scheme.eq_ignore_ascii_case("http")
    }
}

pub fn url_scheme(url: &str) -> Option<String> {
    let (scheme, _) = url.split_once("://")?;
    let scheme = scheme.trim();
    if scheme.is_empty()
        || !scheme.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '+' | '-' | '.')
        })
    {
        return None;
    }
    Some(scheme.to_ascii_lowercase())
}

pub fn scheme_is_carried(
    transport: &dyn HttpTransport,
    url: &str,
) -> std::result::Result<(), String> {
    match url_scheme(url) {
        Some(scheme) if transport.supports_scheme(&scheme) => Ok(()),
        Some(scheme) => Err(format!(
            "The configured address uses {scheme}, which the transport in use cannot carry. \
             Point it at an http endpoint or supply a transport that speaks {scheme}."
        )),
        None => Err(
            "The configured address is not a usable http address, so no request can be made."
                .to_string(),
        ),
    }
}

pub(crate) struct Deadline {
    start: Instant,
    timeout: Duration,
}

impl Deadline {
    pub(crate) fn after(timeout: Duration) -> Self {
        Self {
            start: Instant::now(),
            timeout,
        }
    }

    pub(crate) fn remaining(&self) -> Option<Duration> {
        let elapsed = self.start.elapsed();
        if elapsed >= self.timeout {
            return None;
        }
        Some(self.timeout - elapsed)
    }
}

struct DeadlineReader {
    stream: TcpStream,
    deadline: Deadline,
}

impl DeadlineReader {
    fn new(stream: TcpStream, deadline: Deadline) -> Self {
        Self { stream, deadline }
    }
}

impl Read for DeadlineReader {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let Some(remaining) = self.deadline.remaining() else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "the request deadline passed",
            ));
        };
        self.stream.set_read_timeout(Some(remaining))?;
        self.stream.read(buffer)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct TcpTransport {
    pub connect_timeout: Duration,
}

impl Default for TcpTransport {
    fn default() -> Self {
        Self {
            connect_timeout: DEFAULT_CONNECT_TIMEOUT,
        }
    }
}

impl TcpTransport {
    pub fn new(connect_timeout: Duration) -> Self {
        Self { connect_timeout }
    }
}

pub(crate) struct HttpTarget {
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) path: String,
}

fn parse_target(url: &str) -> Result<HttpTarget, TransportError> {
    parse_target_with(url, "http", 80)
}

pub(crate) fn parse_target_with(
    url: &str,
    required_scheme: &str,
    default_port: u16,
) -> Result<HttpTarget, TransportError> {
    if url.chars().any(|character| character.is_control()) {
        return Err(TransportError::InvalidUrl);
    }
    let (scheme, rest) = url.split_once("://").ok_or(TransportError::InvalidUrl)?;
    if !scheme.eq_ignore_ascii_case(required_scheme) {
        return Err(TransportError::UnsupportedScheme(
            scheme.to_ascii_lowercase(),
        ));
    }
    let (authority, path) = match rest.find('/') {
        Some(index) => (&rest[..index], &rest[index..]),
        None => (rest, "/"),
    };
    if authority.is_empty() || authority.contains('@') || authority.contains(' ') {
        return Err(TransportError::InvalidUrl);
    }
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) => {
            let port = port
                .parse::<u16>()
                .map_err(|_| TransportError::InvalidUrl)?;
            (host.to_string(), port)
        }
        None => (authority.to_string(), default_port),
    };
    if host.is_empty() {
        return Err(TransportError::InvalidUrl);
    }
    Ok(HttpTarget {
        host,
        port,
        path: path.to_string(),
    })
}

fn read_line_bounded(reader: &mut impl BufRead, limit: usize) -> Result<String, TransportError> {
    let mut buffer = Vec::new();
    let mut limited = reader.take(limit as u64);
    let read = limited.read_until(b'\n', &mut buffer)?;
    if read == limit && !buffer.ends_with(b"\n") {
        return Err(TransportError::InvalidRequest(
            "a response line was too long",
        ));
    }
    Ok(String::from_utf8_lossy(&buffer)
        .trim_end_matches(['\r', '\n'])
        .to_string())
}

fn read_bounded(reader: &mut impl Read, limit: usize) -> Result<Vec<u8>, TransportError> {
    let mut body = Vec::new();
    let mut chunk = [0_u8; 8_192];
    loop {
        let read = reader.read(&mut chunk)?;
        if read == 0 {
            return Ok(body);
        }
        if read > limit.saturating_sub(body.len()) {
            return Err(TransportError::TooLarge { limit });
        }
        body.extend_from_slice(&chunk[..read]);
    }
}

fn read_trailers(reader: &mut impl BufRead) -> Result<(), TransportError> {
    let mut lines = 0_usize;
    let mut bytes = 0_usize;
    loop {
        let line = read_line_bounded(reader, 1_024)?;
        if line.is_empty() {
            return Ok(());
        }
        lines += 1;
        bytes = bytes.saturating_add(line.len());
        if lines > MAX_TRAILER_LINES || bytes > MAX_TRAILER_BYTES {
            return Err(TransportError::InvalidRequest(
                "the chunked trailer was longer than the parser accepts",
            ));
        }
    }
}

fn read_chunked(reader: &mut impl BufRead, limit: usize) -> Result<Vec<u8>, TransportError> {
    let mut body = Vec::new();
    loop {
        let line = read_line_bounded(reader, 1_024)?;
        let size_text = line.split(';').next().unwrap_or("").trim();
        let size = usize::from_str_radix(size_text, 16).map_err(|_| TransportError::InvalidUrl)?;
        if size == 0 {
            read_trailers(reader)?;
            return Ok(body);
        }
        let remaining = limit.saturating_sub(body.len());
        if size > remaining {
            return Err(TransportError::TooLarge { limit });
        }
        let mut chunk = vec![0_u8; size];
        reader.read_exact(&mut chunk)?;
        body.extend_from_slice(&chunk);
        let terminator = read_line_bounded(reader, 8)?;
        if !terminator.is_empty() {
            return Err(TransportError::InvalidRequest("a chunk was not terminated"));
        }
    }
}

pub(crate) fn read_response(
    reader: &mut impl BufRead,
    limit: usize,
) -> Result<HttpResponse, TransportError> {
    let status_line = read_line_bounded(reader, 4_096)?;
    let mut parts = status_line.split_whitespace();
    let protocol = parts.next().unwrap_or("");
    if !protocol.starts_with("HTTP/") {
        return Err(TransportError::InvalidRequest(
            "the answer was not an HTTP response",
        ));
    }
    let status = parts
        .next()
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or(TransportError::InvalidRequest("the answer had no status"))?;

    let mut headers = Vec::new();
    let mut header_bytes = 0_usize;
    loop {
        let line = read_line_bounded(reader, 8_192)?;
        if line.is_empty() {
            break;
        }
        header_bytes += line.len();
        if header_bytes > MAX_HEADER_BYTES {
            return Err(TransportError::TooLarge {
                limit: MAX_HEADER_BYTES,
            });
        }
        if let Some((name, value)) = line.split_once(':') {
            headers.push((name.trim().to_string(), value.trim().to_string()));
        }
    }

    let chunked = headers.iter().any(|(name, value)| {
        name.eq_ignore_ascii_case("transfer-encoding")
            && value.to_ascii_lowercase().contains("chunked")
    });
    let content_length = headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, value)| value.trim().parse::<usize>().ok());
    let body = if chunked {
        read_chunked(reader, limit)?
    } else if let Some(length) = content_length {
        if length > limit {
            return Err(TransportError::TooLarge { limit });
        }
        let mut body = vec![0_u8; length];
        reader.read_exact(&mut body)?;
        body
    } else {
        read_bounded(reader, limit)?
    };

    Ok(HttpResponse {
        status,
        headers,
        body,
    })
}

impl HttpTransport for TcpTransport {
    fn send(&self, request: &HttpRequest) -> Result<HttpResponse, TransportError> {
        request.validate()?;
        let target = parse_target(&request.url)?;
        let limit = request.max_bytes.min(MAX_TRANSFER_BYTES);
        let address = (target.host.as_str(), target.port)
            .to_socket_addrs()?
            .next()
            .ok_or(TransportError::InvalidUrl)?;
        let deadline = Deadline::after(request.timeout);
        let socket = TcpStream::connect_timeout(&address, self.connect_timeout)?;
        socket.set_write_timeout(Some(request.timeout))?;
        let mut reader = BufReader::new(DeadlineReader::new(socket.try_clone()?, deadline));
        let mut writer = socket;

        let mut head = format!(
            "{} {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nAccept-Encoding: identity\r\n",
            request.method.as_str(),
            target.path,
            target.host
        );
        for (name, value) in &request.headers {
            head.push_str(&format!("{name}: {value}\r\n"));
        }
        if !request.body.is_empty() {
            head.push_str(&format!("Content-Length: {}\r\n", request.body.len()));
        }
        head.push_str("\r\n");
        writer.write_all(head.as_bytes())?;
        if !request.body.is_empty() {
            writer.write_all(&request.body)?;
        }
        writer.flush()?;

        read_response(&mut reader, limit).map_err(|error| error.into_timeout(request.timeout))
    }
}

#[derive(Debug)]
pub enum ProviderError {
    NotConfigured,

    Unauthorized,

    LoginRequired,

    QuotaExceeded,

    RateLimited { retry_after_ms: Option<i64> },

    NotFound,

    Rejected { status: u16 },

    Server { status: u16 },

    InvalidResponse(&'static str),

    Transport(TransportError),
}

impl ProviderError {
    pub fn from_status(status: u16, retry_after_ms: Option<i64>) -> Self {
        match status {
            401 | 403 => Self::Unauthorized,
            404 => Self::NotFound,
            406 => Self::QuotaExceeded,
            407 => Self::LoginRequired,
            408 | 425 => Self::Server { status },
            429 => Self::RateLimited { retry_after_ms },
            500..=599 => Self::Server { status },
            other => Self::Rejected { status: other },
        }
    }

    pub fn code(&self) -> &'static str {
        match self {
            Self::NotConfigured => "provider_not_configured",
            Self::Unauthorized => "provider_unauthorized",
            Self::LoginRequired => "provider_login_required",
            Self::QuotaExceeded => "provider_quota",
            Self::RateLimited { .. } => "provider_rate_limited",
            Self::NotFound => "provider_not_found",
            Self::Rejected { .. } => "provider_rejected",
            Self::Server { .. } => "provider_server",
            Self::InvalidResponse(_) => "provider_invalid_response",
            Self::Transport(_) => "provider_transport",
        }
    }

    pub fn message(&self) -> String {
        match self {
            Self::NotConfigured => "Online metadata is not configured.".to_string(),
            Self::Unauthorized => "The provider rejected the configured key.".to_string(),
            Self::LoginRequired => {
                "The provider needs a signed-in session for this request.".to_string()
            }
            Self::QuotaExceeded => {
                "The provider reports that its download quota is used up.".to_string()
            }
            Self::RateLimited { .. } => {
                "The provider is rate limiting requests. Try again later.".to_string()
            }
            Self::NotFound => "The provider has no matching entry.".to_string(),
            Self::Rejected { status } => {
                format!("The provider rejected the request (status {status}).")
            }
            Self::Server { status } => {
                format!("The provider could not answer (status {status}).")
            }
            Self::InvalidResponse(reason) => {
                format!("The provider sent an unexpected answer: {reason}.")
            }
            Self::Transport(error) => format!("The request could not be made: {error}."),
        }
    }

    pub fn retryable(&self) -> bool {
        match self {
            Self::RateLimited { .. } | Self::Server { .. } => true,
            Self::Transport(error) => error.retryable(),
            _ => false,
        }
    }

    pub fn retry_after_ms(&self) -> Option<i64> {
        match self {
            Self::RateLimited { retry_after_ms } => *retry_after_ms,
            _ => None,
        }
    }
}

impl fmt::Display for ProviderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message())
    }
}

impl std::error::Error for ProviderError {}

impl From<TransportError> for ProviderError {
    fn from(error: TransportError) -> Self {
        Self::Transport(error)
    }
}

pub trait Clock {
    fn now_ms(&self) -> i64;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_millis() as i64)
            .unwrap_or_default()
    }
}

pub trait Sleeper {
    fn sleep_ms(&self, millis: i64);
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ThreadSleeper;

impl Sleeper for ThreadSleeper {
    fn sleep_ms(&self, millis: i64) {
        if millis > 0 {
            std::thread::sleep(Duration::from_millis(millis as u64));
        }
    }
}

pub fn backoff_ms(attempt: u32, base_ms: i64, cap_ms: i64) -> i64 {
    if attempt == 0 {
        return 0;
    }
    let shift = (attempt - 1).min(20);
    base_ms.saturating_mul(1_i64 << shift).min(cap_ms)
}

pub fn retry_delay_ms(attempt: i64, base_ms: i64, cap_ms: i64) -> i64 {
    if attempt <= 0 {
        return 0;
    }
    backoff_ms(attempt as u32, base_ms, cap_ms)
}

pub struct Pacer {
    budget_per_minute: u32,
    stamps: Vec<i64>,
    max_attempts: u32,
    base_ms: i64,
    cap_ms: i64,
    clock: Box<dyn Clock>,
    sleeper: Box<dyn Sleeper>,
}

impl fmt::Debug for Pacer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Pacer")
            .field("budget_per_minute", &self.budget_per_minute)
            .field("max_attempts", &self.max_attempts)
            .finish_non_exhaustive()
    }
}

impl Pacer {
    pub fn new(budget_per_minute: u32, clock: Box<dyn Clock>, sleeper: Box<dyn Sleeper>) -> Self {
        Self {
            budget_per_minute: budget_per_minute.max(1),
            stamps: Vec::new(),
            max_attempts: DEFAULT_MAX_ATTEMPTS,
            base_ms: BASE_BACKOFF_MS,
            cap_ms: MAX_BACKOFF_MS,
            clock,
            sleeper,
        }
    }

    pub fn system(budget_per_minute: u32) -> Self {
        Self::new(
            budget_per_minute,
            Box::new(SystemClock),
            Box::new(ThreadSleeper),
        )
    }

    pub fn with_retry(mut self, max_attempts: u32, base_ms: i64, cap_ms: i64) -> Self {
        self.max_attempts = max_attempts.max(1);
        self.base_ms = base_ms.max(1);
        self.cap_ms = cap_ms.max(self.base_ms);
        self
    }

    pub fn backoff(&self, attempt: u32) -> i64 {
        backoff_ms(attempt, self.base_ms, self.cap_ms)
    }

    pub fn acquire(&mut self) {
        let now = self.clock.now_ms();
        self.stamps.retain(|stamp| now - *stamp < RATE_WINDOW_MS);
        if self.stamps.len() as u32 >= self.budget_per_minute {
            let oldest = self.stamps[0];
            let wait = (oldest + RATE_WINDOW_MS - now).max(1);
            self.sleeper.sleep_ms(wait);
            let later = self.clock.now_ms();
            self.stamps.retain(|stamp| later - *stamp < RATE_WINDOW_MS);
        }
        self.stamps.push(self.clock.now_ms());
    }

    pub fn wait_for(&mut self, millis: i64) {
        if millis > 0 {
            self.sleeper.sleep_ms(millis);
        }
    }

    pub fn execute<T>(
        &mut self,
        mut call: impl FnMut() -> Result<T, ProviderError>,
    ) -> Result<T, ProviderError> {
        let attempts = self.max_attempts;
        let mut attempt = 0;
        loop {
            attempt += 1;
            self.acquire();
            match call() {
                Ok(value) => return Ok(value),
                Err(error) => {
                    if attempt >= attempts || !error.retryable() {
                        return Err(error);
                    }
                    let backoff = self.backoff(attempt);
                    let delay = match error.retry_after_ms() {
                        Some(retry_after) => retry_after.clamp(0, MAX_RETRY_AFTER_MS).max(backoff),
                        None => backoff,
                    };
                    self.wait_for(delay);
                }
            }
        }
    }
}

pub fn encode_query_value(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        let character = *byte as char;
        if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.' | '~') {
            encoded.push(character);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

pub fn join_url(base: &str, path: &str) -> String {
    let base = base.trim_end_matches('/');
    if path.starts_with('/') {
        format!("{base}{path}")
    } else {
        format!("{base}/{path}")
    }
}
