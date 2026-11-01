use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

pub const MAX_RECEIVERS: usize = 12;

pub const MAX_PROBE: Duration = Duration::from_secs(3);

pub const SEARCH_TARGET: &str = "urn:schemas-upnp-org:device:MediaRenderer:1";

pub const MAX_DESCRIPTION_BYTES: usize = 64 * 1024;

pub const MIN_DESCRIPTION_TIMEOUT: Duration = Duration::from_millis(250);

pub const MAX_DESCRIPTION_TIMEOUT: Duration = Duration::from_secs(10);

pub const MAX_DESCRIPTION_URL: usize = 2_048;

pub const AV_TRANSPORT_SERVICE: &str = "urn:schemas-upnp-org:service:AVTransport:1";

pub const RENDERING_CONTROL_SERVICE: &str = "urn:schemas-upnp-org:service:RenderingControl:1";

pub const CAPABILITY_TEXT: &str = "A casting session can describe a receiver, serve one chosen \
file from a temporary endpoint that closes itself, and send AVTransport controls. That path has \
only been run against an in-process fake receiver: no real device has ever played anything \
through it, and no receiver has answered a search on this network either, so casting stays \
unverified until a device answers and you watch a film play on it.";

const MULTICAST_ADDRESS: &str = "239.255.255.250:1900";

const MAX_SERVICES: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CastError {
    Refused(String),

    Unreachable(String),

    TimedOut(String),

    Description(String),

    Service(String),

    Media(String),

    Stream(String),

    Session(String),

    State(String),

    Action {
        action: String,
        status: u16,
        detail: String,
    },

    Fault {
        action: String,
        code: String,
        description: String,
    },
}

impl std::fmt::Display for CastError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CastError::Refused(reason) => write!(formatter, "{reason}"),
            CastError::Unreachable(reason) => write!(formatter, "{reason}"),
            CastError::TimedOut(reason) => write!(formatter, "{reason}"),
            CastError::Description(reason) => write!(formatter, "{reason}"),
            CastError::Service(reason) => write!(formatter, "{reason}"),
            CastError::Media(reason) => write!(formatter, "{reason}"),
            CastError::Stream(reason) => write!(formatter, "{reason}"),
            CastError::Session(reason) => write!(formatter, "{reason}"),
            CastError::State(reason) => write!(formatter, "{reason}"),
            CastError::Action {
                action,
                status,
                detail,
            } => write!(
                formatter,
                "The receiver answered HTTP {status} to {action}: {detail}"
            ),
            CastError::Fault {
                action,
                code,
                description,
            } => write!(
                formatter,
                "The receiver refused {action} with a SOAP fault {code}: {description}"
            ),
        }
    }
}

impl std::error::Error for CastError {}

pub(crate) mod http {
    use std::io::{Read, Write};
    use std::net::{IpAddr, SocketAddr, TcpStream, ToSocketAddrs};
    use std::time::{Duration, Instant};

    use super::CastError;

    const MAX_URL: usize = 2_048;

    const MAX_HEADER_BYTES: usize = 16 * 1024;

    const MAX_ADDRESSES: usize = 4;

    pub(crate) struct Reply {
        pub(crate) status: u16,

        pub(crate) body: Vec<u8>,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) struct Url {
        pub(crate) scheme: String,

        pub(crate) host: String,

        pub(crate) port: u16,

        pub(crate) target: String,
    }

    impl Url {
        pub(crate) fn parse(raw: &str) -> Result<Url, CastError> {
            let raw = raw.trim();
            if raw.is_empty() {
                return Err(CastError::Refused(
                    "An empty address was given.".to_string(),
                ));
            }
            if raw.len() > MAX_URL {
                return Err(CastError::Refused(format!(
                    "The address is longer than {MAX_URL} characters."
                )));
            }
            if raw
                .chars()
                .any(|value| value.is_control() || value.is_whitespace())
            {
                return Err(CastError::Refused(
                    "The address contains whitespace or control characters.".to_string(),
                ));
            }
            let Some((scheme, rest)) = raw.split_once("://") else {
                return Err(CastError::Refused(
                    "The address has no scheme, so it cannot be used.".to_string(),
                ));
            };
            let scheme = scheme.to_ascii_lowercase();
            if scheme != "http" && scheme != "https" {
                return Err(CastError::Refused(format!(
                    "Only http and https addresses are accepted, not {scheme}."
                )));
            }
            let (authority, target) = match rest.find('/') {
                Some(index) => (&rest[..index], &rest[index..]),
                None => (rest, "/"),
            };
            if authority.is_empty() {
                return Err(CastError::Refused("The address names no host.".to_string()));
            }
            if authority.contains('@') {
                return Err(CastError::Refused(
                    "An address carrying credentials is refused.".to_string(),
                ));
            }
            let (host, port) = split_authority(authority, &scheme)?;
            let target = target.split('#').next().unwrap_or("/");
            let target = if target.is_empty() { "/" } else { target };
            Ok(Url {
                scheme,
                host,
                port,
                target: target.to_string(),
            })
        }

        pub(crate) fn authority(&self) -> String {
            let host = if self.host.contains(':') {
                format!("[{}]", self.host)
            } else {
                self.host.clone()
            };
            let default = if self.scheme == "https" { 443 } else { 80 };
            if self.port == default {
                host
            } else {
                format!("{host}:{}", self.port)
            }
        }

        pub(crate) fn canonical(&self) -> String {
            format!("{}://{}{}", self.scheme, self.authority(), self.target)
        }

        pub(crate) fn socket_addresses(&self) -> Result<Vec<SocketAddr>, CastError> {
            if let Ok(ip) = self.host.parse::<IpAddr>() {
                return Ok(vec![SocketAddr::new(ip, self.port)]);
            }
            let addresses = (self.host.as_str(), self.port)
                .to_socket_addrs()
                .map_err(|error| {
                    CastError::Unreachable(format!("Cannot resolve {}: {error}", self.host))
                })?
                .take(MAX_ADDRESSES)
                .collect::<Vec<_>>();
            if addresses.is_empty() {
                return Err(CastError::Unreachable(format!(
                    "{} has no address on this machine.",
                    self.host
                )));
            }
            Ok(addresses)
        }
    }

    fn split_authority(authority: &str, scheme: &str) -> Result<(String, u16), CastError> {
        let default = if scheme == "https" { 443 } else { 80 };
        if let Some(rest) = authority.strip_prefix('[') {
            let Some((host, tail)) = rest.split_once(']') else {
                return Err(CastError::Refused(
                    "The address has an unclosed IPv6 host.".to_string(),
                ));
            };
            if host.parse::<std::net::Ipv6Addr>().is_err() {
                return Err(CastError::Refused(
                    "The address names an invalid IPv6 host.".to_string(),
                ));
            }
            let port = match tail.strip_prefix(':') {
                Some(port) => parse_port(port)?,
                None if tail.is_empty() => default,
                None => {
                    return Err(CastError::Refused(
                        "The address has an unexpected IPv6 suffix.".to_string(),
                    ));
                }
            };
            return Ok((host.to_string(), port));
        }
        let (host, port) = match authority.rsplit_once(':') {
            Some((host, port))
                if !port.is_empty() && port.chars().all(|value| value.is_ascii_digit()) =>
            {
                (host, parse_port(port)?)
            }
            _ => (authority, default),
        };
        if host.is_empty() {
            return Err(CastError::Refused("The address names no host.".to_string()));
        }
        if !host
            .chars()
            .all(|value| value.is_ascii_alphanumeric() || matches!(value, '.' | '-' | '_'))
        {
            return Err(CastError::Refused(format!(
                "The address host {host} contains unexpected characters."
            )));
        }
        Ok((host.to_string(), port))
    }

    fn parse_port(port: &str) -> Result<u16, CastError> {
        match port.parse::<u16>() {
            Ok(0) | Err(_) => Err(CastError::Refused(format!(
                "The address port {port} is not usable."
            ))),
            Ok(port) => Ok(port),
        }
    }

    pub(crate) fn get(raw_url: &str, limit: usize, timeout: Duration) -> Result<Reply, CastError> {
        request(raw_url, "GET", &[], None, limit, timeout)
    }

    pub(crate) fn request(
        raw_url: &str,
        method: &str,
        headers: &[(String, String)],
        body: Option<&str>,
        limit: usize,
        timeout: Duration,
    ) -> Result<Reply, CastError> {
        let url = Url::parse(raw_url)?;
        if url.scheme == "https" {
            return Err(CastError::Refused(
                "An https address needs a TLS stack this build does not ship; use the http address \
                 the receiver published."
                    .to_string(),
            ));
        }
        let mut stream = connect(&url.socket_addresses()?, timeout)?;
        let _ = stream.set_read_timeout(Some(timeout));
        let _ = stream.set_write_timeout(Some(timeout));
        let mut head = format!(
            "{method} {} HTTP/1.1\r\nHost: {}\r\nAccept: */*\r\nConnection: close\r\n\
             User-Agent: Screenroom\r\n",
            url.target,
            url.authority()
        );
        for (name, value) in headers {
            head.push_str(&format!("{name}: {value}\r\n"));
        }
        if let Some(body) = body {
            head.push_str(&format!("Content-Length: {}\r\n", body.len()));
        }
        head.push_str("\r\n");
        stream.write_all(head.as_bytes()).map_err(|error| {
            CastError::Unreachable(format!("The receiver dropped the request: {error}"))
        })?;
        if let Some(body) = body {
            stream.write_all(body.as_bytes()).map_err(|error| {
                CastError::Unreachable(format!("The receiver dropped the request: {error}"))
            })?;
        }
        stream.flush().map_err(|error| {
            CastError::Unreachable(format!("The receiver dropped the request: {error}"))
        })?;
        read_reply(stream, limit, timeout)
    }

    fn connect(addresses: &[SocketAddr], timeout: Duration) -> Result<TcpStream, CastError> {
        let mut last = None;
        for address in addresses {
            match TcpStream::connect_timeout(address, timeout) {
                Ok(stream) => return Ok(stream),
                Err(error) => last = Some(format!("{address}: {error}")),
            }
        }
        Err(CastError::Unreachable(match last {
            Some(detail) => format!("Cannot reach the receiver at {detail}"),
            None => "The address resolved to nothing.".to_string(),
        }))
    }

    fn read_reply(
        mut stream: TcpStream,
        limit: usize,
        timeout: Duration,
    ) -> Result<Reply, CastError> {
        let deadline = Instant::now() + timeout;
        let cap = limit.saturating_add(MAX_HEADER_BYTES);
        let mut raw = Vec::with_capacity(1_024);
        let mut buffer = [0_u8; 8 * 1024];
        loop {
            if Instant::now() >= deadline {
                return Err(CastError::TimedOut(
                    "The receiver did not finish answering in time.".to_string(),
                ));
            }
            match stream.read(&mut buffer) {
                Ok(0) => break,
                Ok(read) => {
                    raw.extend_from_slice(&buffer[..read]);
                    if raw.len() > cap {
                        return Err(CastError::Description(format!(
                            "The answer is larger than the {limit} byte limit."
                        )));
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    return Err(CastError::TimedOut(
                        "The receiver stopped answering partway through.".to_string(),
                    ));
                }
                Err(error) => {
                    return Err(CastError::Unreachable(format!(
                        "The receiver dropped the answer: {error}"
                    )));
                }
            }
        }
        parse_reply(&raw, limit)
    }

    fn parse_reply(raw: &[u8], limit: usize) -> Result<Reply, CastError> {
        let mut split = find(raw, b"\r\n\r\n");
        let mut separator = 4;
        if split.is_none() {
            split = find(raw, b"\n\n");
            separator = 2;
        }
        let split = split.ok_or_else(|| {
            CastError::Description("The answer ended before its headers did.".to_string())
        })?;
        let head = std::str::from_utf8(&raw[..split]).map_err(|_| {
            CastError::Description("The answer headers are not valid text.".to_string())
        })?;
        let body = &raw[split + separator..];
        let mut lines = head.lines();
        let status_line = lines
            .next()
            .ok_or_else(|| CastError::Description("The answer has no status line.".to_string()))?;
        if !status_line.starts_with("HTTP/") {
            return Err(CastError::Description(
                "The answer is not an HTTP response.".to_string(),
            ));
        }
        let status = status_line
            .split_whitespace()
            .nth(1)
            .and_then(|code| code.parse::<u16>().ok())
            .ok_or_else(|| {
                CastError::Description("The answer has no HTTP status code.".to_string())
            })?;
        let mut chunked = false;
        let mut length = None;
        for line in lines {
            let Some((name, value)) = line.split_once(':') else {
                continue;
            };
            let name = name.trim().to_ascii_lowercase();
            let value = value.trim();
            if name == "transfer-encoding" && value.to_ascii_lowercase().contains("chunked") {
                chunked = true;
            }
            if name == "content-length" {
                length = value.parse::<usize>().ok();
            }
        }
        if chunked {
            return Ok(Reply {
                status,
                body: decode_chunked(body, limit)?,
            });
        }
        if let Some(length) = length {
            if length > limit {
                return Err(CastError::Description(format!(
                    "The answer declares {length} bytes, more than the {limit} byte limit."
                )));
            }
            if body.len() < length {
                return Err(CastError::Description(format!(
                    "The answer ended after {} of {length} bytes.",
                    body.len()
                )));
            }
            return Ok(Reply {
                status,
                body: body[..length].to_vec(),
            });
        }
        if body.len() > limit {
            return Err(CastError::Description(format!(
                "The answer is larger than the {limit} byte limit."
            )));
        }
        Ok(Reply {
            status,
            body: body.to_vec(),
        })
    }

    fn decode_chunked(raw: &[u8], limit: usize) -> Result<Vec<u8>, CastError> {
        let mut out = Vec::new();
        let mut at = 0;
        loop {
            let Some(line_end) = find(&raw[at..], b"\r\n") else {
                return Err(CastError::Description(
                    "The answer ended inside a chunk header.".to_string(),
                ));
            };
            let line = std::str::from_utf8(&raw[at..at + line_end])
                .map_err(|_| {
                    CastError::Description("A chunk header is not valid text.".to_string())
                })?
                .split(';')
                .next()
                .unwrap_or("")
                .trim();
            let Ok(size) = u64::from_str_radix(line, 16) else {
                return Err(CastError::Description(
                    "A chunk header is not a hexadecimal size.".to_string(),
                ));
            };
            at += line_end + 2;
            if size == 0 {
                break;
            }
            let Ok(size) = usize::try_from(size) else {
                return Err(CastError::Description(
                    "A chunk is larger than this machine can hold.".to_string(),
                ));
            };
            let Some(end) = at.checked_add(size) else {
                return Err(CastError::Description(
                    "A chunk size overflows.".to_string(),
                ));
            };
            if end.saturating_add(2) > raw.len() {
                return Err(CastError::Description(
                    "The answer ended inside a chunk.".to_string(),
                ));
            }
            if out.len().saturating_add(size) > limit {
                return Err(CastError::Description(format!(
                    "The answer is larger than the {limit} byte limit."
                )));
            }
            out.extend_from_slice(&raw[at..end]);
            at = end;
            if raw.get(at..at + 2) != Some(b"\r\n") {
                return Err(CastError::Description(
                    "A chunk does not end with a line break.".to_string(),
                ));
            }
            at += 2;
        }
        Ok(out)
    }

    fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        if needle.is_empty() || haystack.len() < needle.len() {
            return None;
        }
        haystack
            .windows(needle.len())
            .position(|window| window == needle)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Receiver {
    pub id: String,

    pub name: String,

    pub kind: String,

    pub address: String,

    pub from: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CastService {
    pub service_type: String,

    pub control_url: String,

    pub scpd_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceDescription {
    pub friendly_name: String,

    pub services: Vec<CastService>,
}

impl DeviceDescription {
    pub fn av_transport(&self) -> Result<&CastService, CastError> {
        self.services
            .iter()
            .find(|service| service.service_type.contains("AVTransport"))
            .ok_or_else(|| {
                CastError::Service(
                    "The device description lists no AVTransport service, so it cannot be told to \
                     play anything."
                        .to_string(),
                )
            })
    }

    pub fn rendering_control(&self) -> Option<&CastService> {
        self.services
            .iter()
            .find(|service| service.service_type.contains("RenderingControl"))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capability {
    pub can_start_a_session: bool,

    pub verified_on_a_receiver: bool,
}

pub fn capability() -> Capability {
    Capability {
        can_start_a_session: true,
        verified_on_a_receiver: false,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compatibility {
    Playable,

    Unplayable,

    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodecReport {
    pub compatibility: Compatibility,

    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MediaFacts {
    pub container: String,

    pub video_codec: Option<String>,

    pub audio_codec: Option<String>,
}

impl MediaFacts {
    pub fn video(container: &str, video_codec: &str, audio_codec: &str) -> Self {
        MediaFacts {
            container: container.to_string(),
            video_codec: Some(video_codec.to_string()),
            audio_codec: Some(audio_codec.to_string()),
        }
    }

    pub fn audio(container: &str, audio_codec: &str) -> Self {
        MediaFacts {
            container: container.to_string(),
            video_codec: None,
            audio_codec: Some(audio_codec.to_string()),
        }
    }

    pub fn mime(&self) -> &'static str {
        mime_for(&self.container)
    }
}

pub fn mime_for(container: &str) -> &'static str {
    match normalize(container).as_str() {
        "mp4" | "m4v" => "video/mp4",
        "mov" => "video/quicktime",
        "mkv" => "video/x-matroska",
        "webm" => "video/webm",
        "avi" => "video/x-msvideo",
        "mpeg" | "mpg" => "video/mpeg",
        "ts" | "m2ts" => "video/mp2t",
        "mp3" => "audio/mpeg",
        "flac" => "audio/flac",
        "wav" => "audio/wav",
        "m4a" => "audio/mp4",
        "ogg" | "oga" => "audio/ogg",
        _ => "application/octet-stream",
    }
}

pub fn search_request() -> String {
    format!(
        "M-SEARCH * HTTP/1.1\r\nHOST: 239.255.255.250:1900\r\nMAN: \"ssdp:discover\"\r\nMX: 1\r\nST: {SEARCH_TARGET}\r\n\r\n"
    )
}

fn header<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    text.lines().skip(1).find_map(|line| {
        let (key, value) = line.split_once(':')?;
        key.trim()
            .eq_ignore_ascii_case(name)
            .then(|| value.trim())
            .filter(|value| !value.is_empty())
    })
}

fn clamp(value: &str, limit: usize) -> String {
    value
        .chars()
        .take(limit)
        .collect::<String>()
        .trim()
        .to_string()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HostReach {
    LocalNetwork,

    LocalOnly,

    Public,
}

fn classify_host(host: &str) -> HostReach {
    match host.parse::<IpAddr>() {
        Ok(address) => classify_ip(address),
        Err(_) => classify_name(host),
    }
}

fn classify_ip(address: IpAddr) -> HostReach {
    match address {
        IpAddr::V4(address) => classify_v4(address),
        IpAddr::V6(address) => classify_v6(address),
    }
}

fn classify_v4(address: Ipv4Addr) -> HostReach {
    let octets = address.octets();
    if address.is_loopback()
        || address.is_unspecified()
        || address.is_multicast()
        || address.is_link_local()
        || address == Ipv4Addr::BROADCAST
        || octets[0] == 0
    {
        return HostReach::LocalOnly;
    }
    if address.is_private() || (octets[0] == 100 && (64..128).contains(&octets[1])) {
        return HostReach::LocalNetwork;
    }
    HostReach::Public
}

fn classify_v6(address: Ipv6Addr) -> HostReach {
    if let Some(mapped) = address.to_ipv4_mapped() {
        return classify_v4(mapped);
    }
    if address.is_loopback() || address.is_unspecified() || address.is_multicast() {
        return HostReach::LocalOnly;
    }
    let segments = address.segments();
    if segments[0] & 0xffc0 == 0xfe80 {
        return HostReach::LocalOnly;
    }
    if segments[0] & 0xfe00 == 0xfc00 {
        return HostReach::LocalNetwork;
    }
    HostReach::Public
}

fn classify_name(host: &str) -> HostReach {
    let name = host.trim_end_matches('.').to_ascii_lowercase();
    if name == "localhost"
        || name.ends_with(".localhost")
        || name == "ip6-localhost"
        || name == "ip6-loopback"
        || name == "localdomain"
        || name.ends_with(".localdomain")
    {
        return HostReach::LocalOnly;
    }
    if name == "home.arpa"
        || !name.contains('.')
        || name.ends_with(".local")
        || name.ends_with(".lan")
        || name.ends_with(".home.arpa")
        || name.ends_with(".internal")
    {
        return HostReach::LocalNetwork;
    }
    HostReach::Public
}

fn permit_location(location: &http::Url, responder: IpAddr) -> bool {
    let is_responder = location
        .host
        .parse::<IpAddr>()
        .is_ok_and(|host| host == responder);
    is_responder || classify_host(&location.host) == HostReach::LocalNetwork
}

fn permit_control(control: &http::Url, base: &http::Url) -> bool {
    let same_authority = control.host.eq_ignore_ascii_case(&base.host) && control.port == base.port;
    same_authority || classify_host(&control.host) == HostReach::LocalNetwork
}

pub fn parse_response(text: &str, from: SocketAddr) -> Option<Receiver> {
    if text.len() > 8_192 || !text.starts_with("HTTP/1.1 200") {
        return None;
    }
    let target = header(text, "ST").or_else(|| header(text, "NT"))?;
    if !target.contains("MediaRenderer") {
        return None;
    }
    let location = header(text, "LOCATION")?;
    if location.len() > MAX_DESCRIPTION_URL {
        return None;
    }
    let location = http::Url::parse(location).ok()?;
    if !permit_location(&location, from.ip()) {
        return None;
    }
    let location = location.canonical();
    let id = header(text, "USN")
        .or_else(|| header(text, "UUID"))
        .map(|value| clamp(value, 120))
        .unwrap_or_else(|| clamp(&location, 120));
    let name = header(text, "FRIENDLYNAME")
        .or_else(|| header(text, "SERVER"))
        .map(|value| clamp(value, 80))
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| format!("A device at {}", from.ip()));
    Some(Receiver {
        id,
        name,
        kind: "DLNA MediaRenderer".to_string(),
        address: location,
        from: from.ip().to_string(),
    })
}

pub fn discover(timeout: Duration, limit: usize) -> Result<Vec<Receiver>, String> {
    let socket = UdpSocket::bind("0.0.0.0:0")
        .map_err(|error| format!("Cannot open a discovery socket: {error}"))?;
    socket
        .set_read_timeout(Some(Duration::from_millis(200)))
        .map_err(|error| format!("Cannot set the discovery timeout: {error}"))?;
    socket
        .send_to(search_request().as_bytes(), MULTICAST_ADDRESS)
        .map_err(|error| search_not_sent(&error))?;

    Ok(collect_replies(
        &socket,
        Instant::now() + timeout.min(MAX_PROBE),
        limit,
    ))
}

fn search_not_sent(error: &std::io::Error) -> String {
    format!(
        "Nothing was looked for: this machine refused to send the receiver search ({error}). No \
         device was asked, so this is not an empty result. If the local network is working, grant \
         this app local network access in the system privacy settings (on macOS: Privacy and \
         Security, Local Network), then search again."
    )
}

fn collect_replies(socket: &UdpSocket, deadline: Instant, limit: usize) -> Vec<Receiver> {
    let limit = limit.clamp(1, MAX_RECEIVERS);
    let mut found: Vec<Receiver> = Vec::new();
    let mut buffer = [0_u8; 4_096];
    while Instant::now() < deadline && found.len() < limit {
        let Ok((read, from)) = socket.recv_from(&mut buffer) else {
            continue;
        };
        let text = String::from_utf8_lossy(&buffer[..read]);
        if let Some(receiver) = parse_response(&text, from)
            && !found.iter().any(|known| known.id == receiver.id)
        {
            found.push(receiver);
        }
    }
    found
}

pub fn describe(address: &str, timeout: Duration) -> Result<DeviceDescription, CastError> {
    let timeout = timeout.clamp(MIN_DESCRIPTION_TIMEOUT, MAX_DESCRIPTION_TIMEOUT);
    let reply = http::get(address, MAX_DESCRIPTION_BYTES, timeout)?;
    if reply.status != 200 {
        return Err(CastError::Description(format!(
            "The device description answered HTTP {} instead of 200.",
            reply.status
        )));
    }
    let xml = String::from_utf8(reply.body).map_err(|_| {
        CastError::Description("The device description is not valid UTF-8.".to_string())
    })?;
    parse_description(&xml, address)
}

pub fn describe_receiver(
    receiver: &Receiver,
    timeout: Duration,
) -> Result<DeviceDescription, CastError> {
    describe(&receiver.address, timeout)
}

pub fn parse_description(xml: &str, base: &str) -> Result<DeviceDescription, CastError> {
    if xml.len() > MAX_DESCRIPTION_BYTES {
        return Err(CastError::Description(format!(
            "The device description is larger than the {MAX_DESCRIPTION_BYTES} byte limit."
        )));
    }
    if xml.trim().is_empty() {
        return Err(CastError::Description(
            "The device description was empty.".to_string(),
        ));
    }
    let friendly_name = element_text(xml, "friendlyName")
        .map(decode_entities)
        .map(|name| clamp(&name, 80))
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "A DLNA renderer".to_string());
    let mut services = Vec::new();
    for block in element_blocks(xml, "service") {
        let Some(service_type) = element_text(block, "serviceType") else {
            continue;
        };
        let service_type = decode_entities(service_type);
        if service_type.trim().is_empty() {
            continue;
        }
        let Some(control) = element_text(block, "controlURL") else {
            continue;
        };
        let control_url = resolve_url(base, &decode_entities(control))?;
        let scpd_url = element_text(block, "SCPDURL")
            .map(decode_entities)
            .filter(|value| !value.trim().is_empty())
            .map(|value| resolve_url(base, &value))
            .transpose()?
            .unwrap_or_default();
        services.push(CastService {
            service_type: service_type.trim().to_string(),
            control_url,
            scpd_url,
        });
        if services.len() >= MAX_SERVICES {
            break;
        }
    }
    if services.is_empty() {
        return Err(CastError::Description(
            "The device description lists no service with a control address.".to_string(),
        ));
    }
    Ok(DeviceDescription {
        friendly_name,
        services,
    })
}

pub fn resolve_url(base: &str, reference: &str) -> Result<String, CastError> {
    let reference = reference.trim();
    if reference.is_empty() {
        return Err(CastError::Description(
            "A service listed an empty address.".to_string(),
        ));
    }
    if reference
        .chars()
        .any(|value| value.is_control() || value.is_whitespace())
    {
        return Err(CastError::Description(
            "A service address contains whitespace or control characters.".to_string(),
        ));
    }
    let lowered = reference.to_ascii_lowercase();
    let absolute_scheme = lowered.starts_with("http://") || lowered.starts_with("https://");
    let scheme_prefix = reference
        .split('/')
        .next()
        .unwrap_or(reference)
        .split_once(':')
        .map(|(scheme, _)| scheme.to_string());
    if scheme_prefix.is_some() && !absolute_scheme {
        return Err(CastError::Description(format!(
            "The service address {reference} uses a scheme this build cannot use."
        )));
    }
    let absolute = if absolute_scheme {
        reference.to_string()
    } else {
        let base_url = http::Url::parse(base)?;
        if reference.starts_with("//") {
            format!("{}:{reference}", base_url.scheme)
        } else if reference.starts_with('/') {
            format!("{}://{}{reference}", base_url.scheme, base_url.authority())
        } else {
            let base_path = base_url.target.split('?').next().unwrap_or("/");
            let directory = match base_path.rfind('/') {
                Some(index) => &base_path[..index + 1],
                None => "/",
            };
            format!(
                "{}://{}{}",
                base_url.scheme,
                base_url.authority(),
                normalize_path(&format!("{directory}{reference}"))
            )
        }
    };
    let resolved = http::Url::parse(&absolute)?;
    let base_url = http::Url::parse(base)?;
    if !permit_control(&resolved, &base_url) {
        return Err(CastError::Description(format!(
            "The service address {reference} is neither the receiver's own address nor a host on \
             this network."
        )));
    }
    Ok(resolved.canonical())
}

pub fn codec_report(facts: &MediaFacts) -> CodecReport {
    let container = normalize(&facts.container);
    if container.is_empty() {
        return report(
            Compatibility::Unknown,
            "No container was reported for this file, so nothing about its playback can be claimed.",
        );
    }
    let video = facts
        .video_codec
        .as_deref()
        .map(normalize)
        .filter(|codec| !codec.is_empty());
    let audio = facts
        .audio_codec
        .as_deref()
        .map(normalize)
        .filter(|codec| !codec.is_empty());
    match container.as_str() {
        "mp4" | "m4v" | "mov" => {
            match video.as_deref() {
                Some(other @ ("hevc" | "h265" | "av1")) => {
                    return report(
                        Compatibility::Unknown,
                        format!(
                            "{container} carrying {other} video depends on the renderer: HEVC and \
                             AV1 support cannot be claimed from the container alone."
                        ),
                    );
                }
                Some("h264") => {}
                None => {
                    return report(
                        Compatibility::Unknown,
                        format!(
                            "{container} was reported without a video codec, so only the container \
                             is known and playback is not claimed."
                        ),
                    );
                }
                Some(other) => {
                    return report(
                        Compatibility::Unplayable,
                        format!(
                            "The video codec {other} is not one the DLNA renderer profile checked \
                             here plays from {container}."
                        ),
                    );
                }
            }
            match audio.as_deref() {
                Some(other @ ("ac3" | "eac3" | "dts" | "truehd")) => {
                    return report(
                        Compatibility::Unknown,
                        format!(
                            "{other} audio in {container} depends on the renderer and cannot be \
                             claimed here."
                        ),
                    );
                }
                Some("aac") | Some("mp3") => {}
                None => {
                    return report(
                        Compatibility::Unknown,
                        format!(
                            "{container} was reported without an audio codec, so the sound side is \
                             unknown and playback is not claimed."
                        ),
                    );
                }
                Some(other) => {
                    return report(
                        Compatibility::Unplayable,
                        format!(
                            "The audio codec {other} is not one the DLNA renderer profile checked \
                             here plays from {container}."
                        ),
                    );
                }
            }
            report(
                Compatibility::Playable,
                "H.264 video with AAC or MP3 audio in an MP4 or QuickTime container is the \
                 combination this build treats as playable."
                    .to_string(),
            )
        }
        "webm" => {
            let video_ok = matches!(video.as_deref(), Some("vp8") | Some("vp9"));
            let audio_ok = matches!(audio.as_deref(), Some("opus") | Some("vorbis"));
            if video_ok && audio_ok {
                report(
                    Compatibility::Playable,
                    "VP8 or VP9 video with Opus or Vorbis audio in WebM is the combination this \
                     build treats as playable."
                        .to_string(),
                )
            } else {
                report(
                    Compatibility::Unknown,
                    "The WebM container here does not carry the video and audio pair this build \
                     can justify, and the renderer has not been asked."
                        .to_string(),
                )
            }
        }
        "mp3" | "flac" | "wav" => {
            if video.is_some() {
                return report(
                    Compatibility::Unknown,
                    format!(
                        "{container} was reported with a video track, which an audio container does \
                         not normally carry."
                    ),
                );
            }
            match audio.as_deref() {
                Some(codec) if audio_matches_container(&container, codec) => report(
                    Compatibility::Playable,
                    format!(
                        "{container} is carried as plain audio, which the renderer profile here \
                         plays."
                    ),
                ),
                Some(other) => report(
                    Compatibility::Unplayable,
                    format!(
                        "The audio codec {other} is not what a {container} file carries in the \
                         profile checked here."
                    ),
                ),
                None => report(
                    Compatibility::Unknown,
                    format!(
                        "{container} was reported without an audio codec, so playback is not \
                         claimed."
                    ),
                ),
            }
        }
        "mkv" => report(
            Compatibility::Unknown,
            "Matroska holds almost anything; a renderer has to advertise it and this build cannot \
             check that from the container."
                .to_string(),
        ),
        "avi" | "mpeg" | "mpg" | "ts" | "m2ts" => report(
            Compatibility::Unknown,
            format!(
                "{container} is recognised but its codecs vary between files, so compatibility is \
                 not claimed."
            ),
        ),
        other => report(
            Compatibility::Unplayable,
            format!(
                "The container {other} is outside the renderer profile this build checks, so it is \
                 not offered as compatible."
            ),
        ),
    }
}

fn audio_matches_container(container: &str, codec: &str) -> bool {
    match container {
        "mp3" => codec == "mp3",
        "flac" => codec == "flac",
        "wav" => matches!(
            codec,
            "pcm_s16le" | "pcm_s24le" | "pcm_s32le" | "pcm_f32le" | "pcm_u8"
        ),
        _ => false,
    }
}

fn report(compatibility: Compatibility, reason: impl Into<String>) -> CodecReport {
    CodecReport {
        compatibility,
        reason: reason.into(),
    }
}

pub fn summary(count: usize) -> String {
    match count {
        0 => "No receiver answered. Discovery is not the same as casting, and nothing was sent."
            .to_string(),
        1 => "One receiver answered. That is discovery only: no stream has been sent, and nothing \
              plays until you start a casting session for a file."
            .to_string(),
        count => format!(
            "{count} receivers answered. That is discovery only: no stream has been sent, and \
             nothing plays until you start a casting session for a file."
        ),
    }
}

fn opens_tag(xml: &str, start: usize) -> bool {
    let Some(open) = xml[..start].rfind('<') else {
        return false;
    };
    let between = &xml[open + 1..start];
    between
        .chars()
        .all(|value| value.is_ascii_alphanumeric() || value == ':')
        && (between.is_empty() || between.ends_with(':'))
}

pub(crate) fn element_text<'a>(xml: &'a str, name: &str) -> Option<&'a str> {
    let mut from = 0;
    while let Some(index) = xml[from..].find(name) {
        let start = from + index;
        let after = xml[start + name.len()..].chars().next();
        if opens_tag(xml, start) && after == Some('>') {
            let body = &xml[start + name.len() + 1..];
            let end = body.find('<')?;
            return Some(&body[..end]);
        }
        from = start + name.len();
    }
    None
}

fn element_blocks<'a>(xml: &'a str, name: &str) -> Vec<&'a str> {
    let open = format!("<{name}>");
    let close = format!("</{name}>");
    let mut blocks = Vec::new();
    let mut from = 0;
    while let Some(index) = xml[from..].find(&open) {
        let body = from + index + open.len();
        let Some(end) = xml[body..].find(&close) else {
            break;
        };
        blocks.push(&xml[body..body + end]);
        from = body + end + close.len();
        if blocks.len() >= MAX_SERVICES {
            break;
        }
    }
    blocks
}

pub(crate) fn decode_entities(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(index) = rest.find('&') {
        out.push_str(&rest[..index]);
        let tail = &rest[index..];
        let Some(end) = tail.find(';').filter(|end| *end <= 12) else {
            out.push('&');
            rest = &tail[1..];
            continue;
        };
        let entity = &tail[1..end];
        let replacement = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => None,
        };
        if let Some(replacement) = replacement {
            out.push(replacement);
            rest = &tail[end + 1..];
        } else if let Some(number) = entity
            .strip_prefix("#x")
            .or_else(|| entity.strip_prefix("#X"))
            .and_then(|value| u32::from_str_radix(value, 16).ok())
            .or_else(|| {
                entity
                    .strip_prefix('#')
                    .and_then(|value| value.parse::<u32>().ok())
            })
            .and_then(char::from_u32)
        {
            out.push(number);
            rest = &tail[end + 1..];
        } else {
            out.push('&');
            rest = &tail[1..];
        }
    }
    out.push_str(rest);
    out
}

fn normalize(value: &str) -> String {
    value.trim().trim_start_matches('.').to_ascii_lowercase()
}

fn normalize_path(path: &str) -> String {
    let (path, query) = match path.split_once('?') {
        Some((path, query)) => (path, Some(query)),
        None => (path, None),
    };
    let mut parts: Vec<&str> = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    let mut out = format!("/{}", parts.join("/"));
    if let Some(query) = query {
        out.push('?');
        out.push_str(query);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const RESPONSE: &str = "HTTP/1.1 200 OK\r\nCACHE-CONTROL: max-age=1800\r\nST: urn:schemas-upnp-org:device:MediaRenderer:1\r\nUSN: uuid:1111::urn:schemas-upnp-org:device:MediaRenderer:1\r\nLOCATION: http://192.168.1.20:9197/dmr\r\nSERVER: Linux/4.0 UPnP/1.0 TestRenderer/1.0\r\n\r\n";

    const DESCRIPTION: &str = "<?xml version=\"1.0\"?>\n<root xmlns=\"urn:schemas-upnp-org:device-1-0\">\n<device>\n<deviceType>urn:schemas-upnp-org:device:MediaRenderer:1</deviceType>\n<friendlyName>Living Room &amp; Kitchen</friendlyName>\n<serviceList>\n<service>\n<serviceType>urn:schemas-upnp-org:service:AVTransport:1</serviceType>\n<serviceId>urn:upnp-org:serviceId:AVTransport</serviceId>\n<SCPDURL>/scpd/avt.xml</SCPDURL>\n<controlURL>/control/avt</controlURL>\n<eventSubURL>/event/avt</eventSubURL>\n</service>\n<service>\n<serviceType>urn:schemas-upnp-org:service:RenderingControl:1</serviceType>\n<serviceId>urn:upnp-org:serviceId:RenderingControl</serviceId>\n<SCPDURL>scpd/rcs.xml</SCPDURL>\n<controlURL>../control/rcs</controlURL>\n<eventSubURL>/event/rcs</eventSubURL>\n</service>\n</serviceList>\n</device>\n</root>\n";

    const SECOND_RESPONSE: &str = "HTTP/1.1 200 OK\r\nST: urn:schemas-upnp-org:device:MediaRenderer:1\r\nUSN: uuid:2222::urn:schemas-upnp-org:device:MediaRenderer:1\r\nLOCATION: http://127.0.0.1:9198/dmr\r\n\r\n";

    const THIRD_RESPONSE: &str = "HTTP/1.1 200 OK\r\nST: urn:schemas-upnp-org:device:MediaRenderer:1\r\nUSN: uuid:3333::urn:schemas-upnp-org:device:MediaRenderer:1\r\nLOCATION: http://127.0.0.1:9199/dmr\r\n\r\n";

    fn from() -> SocketAddr {
        "192.168.1.20:1900".parse().expect("addr")
    }

    fn one_shot(status_line: &str, headers: &[(&str, &str)], body: &[u8]) -> SocketAddr {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let address = listener.local_addr().expect("address");
        let status_line = status_line.to_string();
        let headers: Vec<(String, String)> = headers
            .iter()
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect();
        let body = body.to_vec();
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut scratch = [0_u8; 1024];
                let _ = stream.read(&mut scratch);
                let mut head = format!(
                    "HTTP/1.1 {status_line}\r\nConnection: close\r\nContent-Length: {}\r\n",
                    body.len()
                );
                for (name, value) in headers {
                    head.push_str(&format!("{name}: {value}\r\n"));
                }
                head.push_str("\r\n");
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(&body);
                let _ = stream.flush();
            }
        });
        address
    }

    #[test]
    fn a_renderer_response_becomes_a_receiver_with_its_own_provenance() {
        let receiver = parse_response(RESPONSE, from()).expect("receiver");
        assert_eq!(receiver.kind, "DLNA MediaRenderer");
        assert!(receiver.id.starts_with("uuid:1111"));
        assert_eq!(receiver.address, "http://192.168.1.20:9197/dmr");
        assert_eq!(receiver.from, "192.168.1.20");
        assert!(receiver.name.contains("TestRenderer"));
    }

    #[test]
    fn other_devices_and_malformed_answers_are_refused() {
        let other = RESPONSE.replace("MediaRenderer", "MediaServer");
        assert!(parse_response(&other, from()).is_none());
        assert!(parse_response("", from()).is_none());
        assert!(parse_response("HTTP/1.1 404 Not Found\r\n\r\n", from()).is_none());
        let no_location = RESPONSE.replace("LOCATION: http://192.168.1.20:9197/dmr", "");
        assert!(parse_response(&no_location, from()).is_none());
        let file_location = RESPONSE.replace("http://192.168.1.20:9197/dmr", "file:///etc/passwd");
        assert!(parse_response(&file_location, from()).is_none());
        let long = format!("{RESPONSE}USN: {}\r\n", "x".repeat(9_000));
        assert!(parse_response(&long, from()).is_none());
    }

    #[test]
    fn a_location_that_points_away_from_the_responder_is_refused() {
        for refused in [
            "http://127.0.0.1:9197/dmr",
            "http://127.9.9.9:9197/dmr",
            "http://[::1]:9197/dmr",
            "http://localhost:9197/dmr",
            "http://203.0.113.9:9197/dmr",
            "http://169.254.1.1:9197/dmr",
            "http://0.0.0.0:9197/dmr",
            "file:///etc/passwd",
            "http://user:secret@192.168.1.20:9197/dmr",
        ] {
            let answer = RESPONSE.replace("http://192.168.1.20:9197/dmr", refused);
            assert!(
                parse_response(&answer, from()).is_none(),
                "a location at {refused} must be refused"
            );
        }
        let loopback =
            RESPONSE.replace("http://192.168.1.20:9197/dmr", "http://127.0.0.1:9197/dmr");
        let receiver = parse_response(&loopback, "127.0.0.1:1900".parse().expect("addr"))
            .expect("the responder's own loopback address is allowed");
        assert_eq!(receiver.address, "http://127.0.0.1:9197/dmr");
        for accepted in [
            "http://192.168.1.77:9197/dmr",
            "http://10.0.0.5:9197/dmr",
            "http://renderer.local:9197/dmr",
            "http://[fd00::20]:9197/dmr",
        ] {
            let answer = RESPONSE.replace("http://192.168.1.20:9197/dmr", accepted);
            assert!(
                parse_response(&answer, from()).is_some(),
                "a location on this network at {accepted} must be accepted"
            );
        }
    }

    #[test]
    fn the_search_is_a_real_ssdp_probe_for_renderers_only() {
        let request = search_request();
        assert!(request.starts_with("M-SEARCH * HTTP/1.1\r\n"));
        assert!(request.contains("ST: urn:schemas-upnp-org:device:MediaRenderer:1"));
        assert!(request.contains("MAN: \"ssdp:discover\""));
        assert!(request.ends_with("\r\n\r\n"));
    }

    #[test]
    fn the_summary_never_claims_that_casting_works() {
        for count in [0, 1, 3] {
            let text = summary(count);
            assert!(text.contains("not the same as casting") || text.contains("discovery only"));
            assert!(text.contains("no stream has been sent") || text.contains("nothing was sent"));
        }
    }

    #[test]
    fn replies_are_reported_once_per_receiver() {
        let probe = UdpSocket::bind("127.0.0.1:0").expect("bind");
        probe
            .set_read_timeout(Some(Duration::from_millis(20)))
            .expect("timeout");
        let target = probe.local_addr().expect("address");
        let sender = UdpSocket::bind("127.0.0.1:0").expect("bind");
        for _ in 0..2 {
            sender
                .send_to(RESPONSE.as_bytes(), target)
                .expect("the same receiver answers twice");
        }
        sender
            .send_to(SECOND_RESPONSE.as_bytes(), target)
            .expect("a second receiver answers");

        let found = collect_replies(
            &probe,
            Instant::now() + Duration::from_millis(150),
            MAX_RECEIVERS,
        );
        assert_eq!(found.len(), 2, "one entry per receiver, got {found:?}");
        assert!(found[0].id.starts_with("uuid:1111"));
        assert!(found[1].id.starts_with("uuid:2222"));
    }

    #[test]
    fn collection_stops_at_the_limit_and_leaves_the_rest_for_the_next_call() {
        let probe = UdpSocket::bind("127.0.0.1:0").expect("bind");
        probe
            .set_read_timeout(Some(Duration::from_millis(20)))
            .expect("timeout");
        let target = probe.local_addr().expect("address");
        let sender = UdpSocket::bind("127.0.0.1:0").expect("bind");
        for response in [RESPONSE, SECOND_RESPONSE, THIRD_RESPONSE] {
            sender
                .send_to(response.as_bytes(), target)
                .expect("three receivers answer");
        }

        let first = collect_replies(&probe, Instant::now() + Duration::from_millis(150), 1);
        assert_eq!(first.len(), 1, "a limit of one must report one receiver");
        let rest = collect_replies(
            &probe,
            Instant::now() + Duration::from_millis(60),
            MAX_RECEIVERS,
        );
        assert_eq!(
            rest.len(),
            2,
            "the receivers beyond the limit were still waiting, got {rest:?}"
        );
        assert!(rest.iter().all(|receiver| receiver.id != first[0].id));
    }

    #[test]
    fn a_search_that_could_not_be_sent_says_so_instead_of_reporting_an_empty_network() {
        let denied = std::io::Error::from(std::io::ErrorKind::HostUnreachable);
        let text = search_not_sent(&denied);
        assert!(
            text.contains("Nothing was looked for"),
            "the text must say that no search happened, got {text}"
        );
        assert!(
            text.contains("not an empty result"),
            "the text must not read as a network with no receivers, got {text}"
        );
        assert!(
            text.contains("Local Network"),
            "the text must name an achievable recovery, got {text}"
        );
        assert!(
            text.contains(&denied.to_string()),
            "the text must carry the real reason, got {text}"
        );
    }

    #[test]
    fn a_description_yields_the_control_addresses_a_session_needs() {
        let description =
            parse_description(DESCRIPTION, "http://192.168.1.20:9197/dmr").expect("description");
        assert_eq!(description.friendly_name, "Living Room & Kitchen");
        assert_eq!(description.services.len(), 2);
        let av = description.av_transport().expect("AVTransport");
        assert_eq!(
            av.service_type,
            "urn:schemas-upnp-org:service:AVTransport:1"
        );
        assert_eq!(av.control_url, "http://192.168.1.20:9197/control/avt");
        assert_eq!(av.scpd_url, "http://192.168.1.20:9197/scpd/avt.xml");
        let rendering = description.rendering_control().expect("RenderingControl");
        assert_eq!(
            rendering.control_url,
            "http://192.168.1.20:9197/control/rcs"
        );
        assert_eq!(rendering.scpd_url, "http://192.168.1.20:9197/scpd/rcs.xml");
    }

    #[test]
    fn relative_service_addresses_resolve_against_the_description_base() {
        let base = "http://192.168.1.20:9197/device/description.xml";
        let cases = [
            ("/control/avt", "http://192.168.1.20:9197/control/avt"),
            ("control/avt", "http://192.168.1.20:9197/device/control/avt"),
            ("../control/avt", "http://192.168.1.20:9197/control/avt"),
            (
                "./control/avt",
                "http://192.168.1.20:9197/device/control/avt",
            ),
        ];
        for (reference, expected) in cases {
            assert_eq!(
                resolve_url(base, reference).expect(reference),
                expected,
                "{reference}"
            );
        }
        for refused in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "data:text/xml,<a/>",
        ] {
            assert!(
                resolve_url(base, refused).is_err(),
                "{refused} must be refused"
            );
        }
    }

    #[test]
    fn a_service_address_on_another_lan_host_is_accepted_and_a_public_one_is_not() {
        let base = "http://192.168.1.20:9197/device/description.xml";
        assert_eq!(
            resolve_url(base, "http://10.0.0.5:5000/control").expect("a lan host"),
            "http://10.0.0.5:5000/control"
        );
        assert_eq!(
            resolve_url(base, "//10.0.0.5:5000/control").expect("a lan host"),
            "http://10.0.0.5:5000/control"
        );
        assert_eq!(
            resolve_url(base, "https://renderer.local/control").expect("a lan name"),
            "https://renderer.local/control"
        );
        assert_eq!(
            resolve_url(base, "http://[fd00::20]:9197/control").expect("a lan address"),
            "http://[fd00::20]:9197/control"
        );
        for refused in [
            "http://203.0.113.9/control",
            "http://renderer.example.com/control",
            "http://[2001:db8::1]/control",
        ] {
            assert!(
                resolve_url(base, refused).is_err(),
                "an unrelated public host must be refused, {refused}"
            );
        }
    }

    #[test]
    fn a_loopback_control_address_from_a_remote_description_is_refused() {
        let base = "http://192.168.1.20:9197/device/description.xml";
        for refused in [
            "http://127.0.0.1:8080/control",
            "http://127.9.9.9/control",
            "http://[::1]:8080/control",
            "http://[::ffff:127.0.0.1]:8080/control",
            "http://localhost:8080/control",
            "http://127.0.0.1:8080/control?next=/",
            "//127.0.0.1:8080/control",
            "http://169.254.1.1/control",
            "http://0.0.0.0:8080/control",
            "http://[fe80::1]:8080/control",
        ] {
            assert!(
                resolve_url(base, refused).is_err(),
                "a local-only target must be refused, {refused}"
            );
        }
        let local = "http://127.0.0.1:9197/device/description.xml";
        assert_eq!(
            resolve_url(local, "/control/avt").expect("the description's own authority"),
            "http://127.0.0.1:9197/control/avt"
        );
        assert_eq!(
            resolve_url(local, "http://127.0.0.1:9197/control/avt")
                .expect("the description's own authority"),
            "http://127.0.0.1:9197/control/avt"
        );
        assert!(
            resolve_url(local, "http://127.0.0.1:8080/control").is_err(),
            "a different port on the same local host is not the description authority"
        );
    }

    #[test]
    fn a_description_that_is_not_well_formed_is_an_error_never_a_panic() {
        let base = "http://192.168.1.20:9197/dmr";
        let no_services = DESCRIPTION
            .replace("<service>", "<broken>")
            .replace("</service>", "</broken>");
        let file_control = DESCRIPTION.replace("/control/avt", "file:///etc/passwd");
        let no_control = DESCRIPTION
            .replace("<controlURL>/control/avt</controlURL>", "")
            .replace("<controlURL>../control/rcs</controlURL>", "");
        let truncated = &DESCRIPTION[..DESCRIPTION.len() / 2];
        let garbage = "<<<>>>&&&;;;<<service>><service>>";
        let oversized = format!("{DESCRIPTION}<!--{}-->", "x".repeat(MAX_DESCRIPTION_BYTES));
        for broken in [
            "",
            "   ",
            no_services.as_str(),
            file_control.as_str(),
            no_control.as_str(),
            truncated,
            garbage,
            oversized.as_str(),
        ] {
            assert!(
                parse_description(broken, base).is_err(),
                "a malformed description must be refused"
            );
        }
        let no_transport = DESCRIPTION.replace("AVTransport", "ConnectionManager");
        let parsed = parse_description(&no_transport, base).expect("services still parse");
        assert!(parsed.av_transport().is_err());
    }

    #[test]
    fn describing_a_receiver_fetches_the_description_over_http() {
        let address = one_shot(
            "200 OK",
            &[("Content-Type", "text/xml")],
            DESCRIPTION.as_bytes(),
        );
        let url = format!("http://{address}/description.xml");
        let description = describe(&url, Duration::from_secs(2)).expect("description");
        assert_eq!(description.friendly_name, "Living Room & Kitchen");
        assert_eq!(description.services.len(), 2);
    }

    #[test]
    fn an_oversized_or_refused_description_is_an_error() {
        let oversized = "x".repeat(MAX_DESCRIPTION_BYTES + 4_096);
        let address = one_shot(
            "200 OK",
            &[("Content-Type", "text/xml")],
            oversized.as_bytes(),
        );
        let url = format!("http://{address}/description.xml");
        assert!(describe(&url, Duration::from_secs(2)).is_err());

        let address = one_shot("404 Not Found", &[], b"missing");
        let url = format!("http://{address}/description.xml");
        assert!(matches!(
            describe(&url, Duration::from_secs(2)),
            Err(CastError::Description(_))
        ));
    }

    #[test]
    fn an_address_this_build_cannot_reach_is_refused_before_any_socket() {
        assert!(matches!(
            describe("https://192.0.2.1/description.xml", Duration::from_secs(1)),
            Err(CastError::Refused(_))
        ));
        assert!(matches!(
            describe("file:///etc/passwd", Duration::from_secs(1)),
            Err(CastError::Refused(_))
        ));
        assert!(matches!(
            describe("http://user:secret@192.168.1.2/dmr", Duration::from_secs(1)),
            Err(CastError::Refused(_))
        ));
    }

    fn one_shot_raw(response: &[u8]) -> SocketAddr {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let address = listener.local_addr().expect("address");
        let response = response.to_vec();
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut scratch = [0_u8; 1_024];
                let _ = stream.read(&mut scratch);
                let _ = stream.write_all(&response);
                let _ = stream.flush();
            }
        });
        address
    }

    #[test]
    fn a_chunked_description_is_decoded_and_a_hostile_chunk_is_refused() {
        let body = DESCRIPTION.as_bytes();
        let mut raw =
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n".to_vec();
        raw.extend_from_slice(format!("{:x}\r\n", body.len()).as_bytes());
        raw.extend_from_slice(body);
        raw.extend_from_slice(b"\r\n0\r\n\r\n");
        let address = one_shot_raw(&raw);
        let description = describe(
            &format!("http://{address}/description.xml"),
            Duration::from_secs(2),
        )
        .expect("chunked description");
        assert_eq!(description.services.len(), 2);

        for hostile in [
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\nffffffffffffffff\r\n".as_slice(),
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n4\r\n<ab".as_slice(),
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\nzz\r\n".as_slice(),
        ] {
            let address = one_shot_raw(hostile);
            assert!(
                describe(
                    &format!("http://{address}/description.xml"),
                    Duration::from_secs(2)
                )
                .is_err(),
                "a hostile chunked answer must be refused, not panic"
            );
        }
    }

    #[test]
    fn the_codec_profile_claims_only_what_it_can_justify() {
        let playable = [
            MediaFacts::video("mp4", "h264", "aac"),
            MediaFacts::video("mov", "h264", "mp3"),
            MediaFacts::video("webm", "vp9", "opus"),
            MediaFacts::audio("mp3", "mp3"),
            MediaFacts::audio("flac", "flac"),
            MediaFacts::audio("wav", "pcm_s16le"),
        ];
        for facts in playable {
            let report = codec_report(&facts);
            assert_eq!(
                report.compatibility,
                Compatibility::Playable,
                "{facts:?}: {}",
                report.reason
            );
            assert!(!report.reason.is_empty());
        }
        let unplayable = [
            MediaFacts::video("wmv", "wmv3", "wma"),
            MediaFacts::video("mp4", "mpeg2video", "aac"),
            MediaFacts::video("mp4", "h264", "vorbis"),
            MediaFacts::video("flv", "h264", "aac"),
            MediaFacts::audio("flac", "mp3"),
        ];
        for facts in unplayable {
            let report = codec_report(&facts);
            assert_eq!(
                report.compatibility,
                Compatibility::Unplayable,
                "{facts:?}: {}",
                report.reason
            );
            assert!(
                report.reason.contains("not one")
                    || report.reason.contains("outside")
                    || report.reason.contains("not what")
            );
        }
        let unproven = [
            MediaFacts::video("mp4", "hevc", "aac"),
            MediaFacts::video("mp4", "h264", "dts"),
            MediaFacts::video("mkv", "h264", "aac"),
            MediaFacts::video("avi", "mpeg4", "mp3"),
            MediaFacts::video("ts", "h264", "aac"),
            MediaFacts {
                container: "mp4".to_string(),
                video_codec: None,
                audio_codec: None,
            },
            MediaFacts {
                container: "mp4".to_string(),
                video_codec: Some("h264".to_string()),
                audio_codec: None,
            },
            MediaFacts {
                container: "flac".to_string(),
                video_codec: None,
                audio_codec: None,
            },
        ];
        for facts in unproven {
            let report = codec_report(&facts);
            assert_eq!(
                report.compatibility,
                Compatibility::Unknown,
                "{facts:?}: {}",
                report.reason
            );
            assert!(!report.reason.is_empty());
        }
        let missing = codec_report(&MediaFacts::default());
        assert_eq!(missing.compatibility, Compatibility::Unknown);
        assert!(missing.reason.contains("No container"));
    }

    #[test]
    fn the_capability_text_never_claims_a_verified_receiver() {
        let capability = capability();
        assert!(capability.can_start_a_session);
        assert!(!capability.verified_on_a_receiver);
        assert!(CAPABILITY_TEXT.contains("fake receiver"));
        assert!(CAPABILITY_TEXT.contains("no real device"));
        assert!(
            CAPABILITY_TEXT.contains("no receiver has answered a search"),
            "the text must carry what the network survey found, got {CAPABILITY_TEXT}"
        );
        assert!(!CAPABILITY_TEXT.contains("works with"));
    }

    #[test]
    fn xml_entities_are_decoded_without_swallowing_unknown_text() {
        assert_eq!(decode_entities("a &amp; b"), "a & b");
        assert_eq!(decode_entities("&lt;tag&gt;"), "<tag>");
        assert_eq!(decode_entities("&#65;&#x42;"), "AB");
        assert_eq!(decode_entities("100% & more"), "100% & more");
        assert_eq!(decode_entities("&notanentity;"), "&notanentity;");
    }
}
