use std::fs;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::time::Duration;

use crate::cast_stream::{self, StreamServer};
use crate::casting::{
    self, AV_TRANSPORT_SERVICE, CastError, Compatibility, DeviceDescription, MediaFacts, Receiver,
    http,
};

pub const MAX_SOAP_BYTES: usize = 64 * 1024;

pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);

pub const DEFAULT_LIFETIME: Duration = Duration::from_secs(30 * 60);

pub const MIN_TIMEOUT: Duration = Duration::from_millis(250);

pub const MAX_TIMEOUT: Duration = Duration::from_secs(20);

const MAX_TEXT: usize = 300;

const RECEIVER_PORT: u16 = 1900;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportState {
    Playing,

    Paused,

    Stopped,

    Transitioning,

    NoMedia,

    Unknown(String),
}

impl TransportState {
    pub fn parse(raw: &str) -> TransportState {
        match raw.trim().to_ascii_uppercase().as_str() {
            "PLAYING" => TransportState::Playing,
            "PAUSED_PLAYBACK" | "PAUSED_RECORDING" => TransportState::Paused,
            "STOPPED" => TransportState::Stopped,
            "TRANSITIONING" => TransportState::Transitioning,
            "NO_MEDIA_PRESENT" => TransportState::NoMedia,
            other => TransportState::Unknown(format!("the receiver reported {other}")),
        }
    }
}

impl std::fmt::Display for TransportState {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TransportState::Playing => write!(formatter, "playing"),
            TransportState::Paused => write!(formatter, "paused"),
            TransportState::Stopped => write!(formatter, "stopped"),
            TransportState::Transitioning => write!(formatter, "changing state"),
            TransportState::NoMedia => write!(formatter, "no media loaded"),
            TransportState::Unknown(reason) => write!(formatter, "unreported ({reason})"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CastStatus {
    pub receiver_name: String,

    pub state: TransportState,

    pub stream_url: String,

    pub stream_running: bool,

    pub remaining: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRequest {
    pub receiver: Receiver,

    pub file: PathBuf,

    pub facts: MediaFacts,

    pub title: String,

    pub timeout: Duration,

    pub lifetime: Duration,

    pub max_clients: usize,

    pub allow_unverified: bool,

    pub bind: Option<SocketAddr>,
}

impl SessionRequest {
    pub fn new(
        receiver: Receiver,
        file: PathBuf,
        facts: MediaFacts,
        title: impl Into<String>,
    ) -> Self {
        SessionRequest {
            receiver,
            file,
            facts,
            title: title.into(),
            timeout: DEFAULT_TIMEOUT,
            lifetime: DEFAULT_LIFETIME,
            max_clients: 1,
            allow_unverified: false,
            bind: None,
        }
    }
}

pub struct CastSession {
    receiver: Receiver,

    description: DeviceDescription,

    av_control_url: String,

    stream: StreamServer,

    title: String,

    uri: String,

    state: TransportState,

    timeout: Duration,
}

pub fn connect(request: SessionRequest) -> Result<CastSession, CastError> {
    let report = casting::codec_report(&request.facts);
    match report.compatibility {
        Compatibility::Unplayable => return Err(CastError::Media(report.reason)),
        Compatibility::Unknown if !request.allow_unverified => {
            return Err(CastError::Media(format!(
                "{} Casting it anyway needs your confirmation.",
                report.reason
            )));
        }
        _ => {}
    }
    let metadata = fs::metadata(&request.file).map_err(|error| {
        CastError::Media(format!(
            "Cannot read {} to cast it: {error}",
            request.file.display()
        ))
    })?;
    if !metadata.is_file() {
        return Err(CastError::Media(
            "Only one file can be cast, not a folder.".to_string(),
        ));
    }
    let description = casting::describe_receiver(&request.receiver, request.timeout)?;
    let av_control_url = description.av_transport()?.control_url.clone();
    let bind = match request.bind {
        Some(bind) => bind,
        None => {
            let remote = receiver_remote(&request.receiver)?;
            SocketAddr::new(cast_stream::local_address_towards(remote)?, 0)
        }
    };
    let stream = StreamServer::start(
        request.file.clone(),
        bind,
        request.lifetime,
        request.max_clients,
    )?;
    let uri = stream.url(bind.ip());
    let metadata_xml = didl_metadata(&request.title, &uri, request.facts.mime(), metadata.len())?;
    let set = soap_set_av_transport_uri(&uri, &metadata_xml)?;
    let mut session = CastSession {
        receiver: request.receiver,
        description,
        av_control_url,
        stream,
        title: request.title,
        uri,
        state: TransportState::Unknown(
            "the receiver accepted Play and has not reported its state".to_string(),
        ),
        timeout: request.timeout.clamp(MIN_TIMEOUT, MAX_TIMEOUT),
    };
    soap_action(
        &session.av_control_url,
        "SetAVTransportURI",
        &set,
        session.timeout,
    )?;
    soap_action(
        &session.av_control_url,
        "Play",
        &soap_play(),
        session.timeout,
    )?;
    if let Some(state) = session.observe() {
        session.state = state;
    }
    Ok(session)
}

impl CastSession {
    pub fn receiver(&self) -> &Receiver {
        &self.receiver
    }

    pub fn description(&self) -> &DeviceDescription {
        &self.description
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn stream_url(&self) -> &str {
        &self.uri
    }

    pub fn state(&self) -> &TransportState {
        &self.state
    }

    pub fn is_streaming(&self) -> bool {
        self.stream.is_running()
    }

    pub fn remaining(&self) -> Duration {
        self.stream.remaining()
    }

    pub fn status(&self) -> CastStatus {
        CastStatus {
            receiver_name: self.receiver.name.clone(),
            state: self.state.clone(),
            stream_url: self.uri.clone(),
            stream_running: self.stream.is_running(),
            remaining: self.stream.remaining(),
        }
    }

    pub fn summary(&self) -> String {
        session_summary(&self.status())
    }

    pub fn refresh(&mut self) -> Result<TransportState, CastError> {
        let outcome = soap_action(
            &self.av_control_url,
            "GetTransportInfo",
            &soap_get_transport_info(),
            self.timeout,
        )
        .and_then(|body| {
            parse_transport_state(&body).ok_or_else(|| {
                CastError::State(
                    "The receiver's transport info did not report a transport state.".to_string(),
                )
            })
        });
        self.settle(outcome)
    }

    fn observe(&mut self) -> Option<TransportState> {
        soap_action(
            &self.av_control_url,
            "GetTransportInfo",
            &soap_get_transport_info(),
            self.timeout,
        )
        .ok()
        .and_then(|body| parse_transport_state(&body))
    }

    pub fn pause(&mut self) -> Result<TransportState, CastError> {
        self.perform("Pause", &soap_pause())
    }

    pub fn resume(&mut self) -> Result<TransportState, CastError> {
        self.perform("Play", &soap_play())
    }

    pub fn seek(&mut self, position: Duration) -> Result<TransportState, CastError> {
        self.perform("Seek", &soap_seek(position))
    }

    pub fn stop(&mut self) -> Result<TransportState, CastError> {
        let outcome = self.perform("Stop", &soap_stop());
        self.stream.stop();
        outcome
    }

    pub fn disconnect(mut self) -> Result<(), CastError> {
        let outcome = if self.stream.is_running() {
            self.perform("Stop", &soap_stop()).map(|_| ())
        } else {
            Ok(())
        };
        self.stream.stop();
        outcome
    }

    fn perform(&mut self, action: &str, envelope: &str) -> Result<TransportState, CastError> {
        if !self.stream.is_running() {
            return Err(CastError::Session(
                "The stream endpoint has already closed, so the receiver cannot play anything."
                    .to_string(),
            ));
        }
        match soap_action(&self.av_control_url, action, envelope, self.timeout) {
            Ok(_) => self.refresh(),
            Err(error) => self.settle(Err(error)),
        }
    }

    fn settle(
        &mut self,
        outcome: Result<TransportState, CastError>,
    ) -> Result<TransportState, CastError> {
        match outcome {
            Ok(state) => {
                self.state = state.clone();
                Ok(state)
            }
            Err(error) => {
                self.stream.stop();
                self.state = TransportState::Unknown(error.to_string());
                Err(error)
            }
        }
    }
}

impl Drop for CastSession {
    fn drop(&mut self) {
        self.stream.stop();
    }
}

pub fn session_summary(status: &CastStatus) -> String {
    if !status.stream_running {
        return format!(
            "The stream to {} is closed, so this casting session is over. The receiver last \
             reported {}.",
            status.receiver_name, status.state
        );
    }
    format!(
        "Casting to {} through a temporary endpoint that serves only this one file and closes by \
         itself in {}. The receiver reports {}. This is the receiver's own answer: no real-device \
         test is recorded for it.",
        status.receiver_name,
        human_duration(status.remaining),
        status.state
    )
}

pub fn casting_note(count: usize, status: Option<&CastStatus>) -> String {
    match status {
        Some(status) => session_summary(status),
        None => format!(
            "{} No casting session has been started from this machine, so nothing is being \
             streamed.",
            casting::summary(count)
        ),
    }
}

fn human_duration(remaining: Duration) -> String {
    let seconds = remaining.as_secs();
    if seconds >= 120 {
        format!("{} minutes", seconds / 60)
    } else if seconds >= 60 {
        "about a minute".to_string()
    } else {
        format!("{seconds} seconds")
    }
}

pub fn soap_set_av_transport_uri(uri: &str, metadata: &str) -> Result<String, CastError> {
    let url = http::Url::parse(uri)?;
    if url.scheme != "http" {
        return Err(CastError::Refused(
            "The media address must be the http address this build serves.".to_string(),
        ));
    }
    Ok(envelope(
        "SetAVTransportURI",
        &format!(
            "<InstanceID>0</InstanceID><CurrentURI>{}</CurrentURI>\
             <CurrentURIMetaData>{}</CurrentURIMetaData>",
            escape_xml(&url.canonical())?,
            escape_xml(metadata)?
        ),
    ))
}

pub fn soap_play() -> String {
    envelope("Play", "<InstanceID>0</InstanceID><Speed>1</Speed>")
}

pub fn soap_pause() -> String {
    envelope("Pause", "<InstanceID>0</InstanceID>")
}

pub fn soap_stop() -> String {
    envelope("Stop", "<InstanceID>0</InstanceID>")
}

pub fn soap_seek(position: Duration) -> String {
    envelope(
        "Seek",
        &format!(
            "<InstanceID>0</InstanceID><Unit>REL_TIME</Unit><Target>{}</Target>",
            transport_time(position)
        ),
    )
}

pub fn soap_get_transport_info() -> String {
    envelope("GetTransportInfo", "<InstanceID>0</InstanceID>")
}

pub fn transport_time(position: Duration) -> String {
    let total = position.as_secs();
    format!(
        "{:02}:{:02}:{:02}",
        total / 3_600,
        (total % 3_600) / 60,
        total % 60
    )
}

pub fn didl_metadata(title: &str, uri: &str, mime: &str, bytes: u64) -> Result<String, CastError> {
    Ok(format!(
        "<DIDL-Lite xmlns=\"urn:schemas-upnp-org:metadata-1-0/DIDL-Lite/\" \
         xmlns:dc=\"http://purl.org/dc/elements/1.1/\" \
         xmlns:upnp=\"urn:schemas-upnp-org:metadata-1-0/upnp/\">\
         <item id=\"0\" parentID=\"-1\" restricted=\"1\">\
         <dc:title>{}</dc:title>\
         <upnp:class>object.item.{}Item</upnp:class>\
         <res protocolInfo=\"http-get:*:{mime}:*\" size=\"{bytes}\">{}</res>\
         </item></DIDL-Lite>",
        escape_xml(title)?,
        if mime.starts_with("video") {
            "video"
        } else {
            "audio"
        },
        escape_xml(uri)?
    ))
}

fn envelope(action: &str, body: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
         <s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\" \
         s:encodingStyle=\"http://schemas.xmlsoap.org/soap/encoding/\">\
         <s:Body>\
         <u:{action} xmlns:u=\"{AV_TRANSPORT_SERVICE}\">{body}</u:{action}>\
         </s:Body></s:Envelope>"
    )
}

fn escape_xml(text: &str) -> Result<String, CastError> {
    if text
        .chars()
        .any(|value| value.is_control() && !matches!(value, '\t' | '\n' | '\r'))
    {
        return Err(CastError::Refused(
            "A value carried control characters, so it was not sent.".to_string(),
        ));
    }
    let mut out = String::with_capacity(text.len());
    for value in text.chars() {
        match value {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            other => out.push(other),
        }
    }
    Ok(out)
}

pub fn soap_action(
    control_url: &str,
    action: &str,
    envelope: &str,
    timeout: Duration,
) -> Result<String, CastError> {
    let timeout = timeout.clamp(MIN_TIMEOUT, MAX_TIMEOUT);
    let reply = post_soap(
        control_url,
        &format!("{AV_TRANSPORT_SERVICE}#{action}"),
        envelope,
        MAX_SOAP_BYTES,
        timeout,
    )?;
    let text = String::from_utf8_lossy(&reply.body).into_owned();
    if let Some(fault) = parse_fault(&text, action) {
        return Err(fault);
    }
    if reply.status != 200 {
        return Err(CastError::Action {
            action: action.to_string(),
            status: reply.status,
            detail: summarize(&text),
        });
    }
    Ok(text)
}

fn post_soap(
    control_url: &str,
    action: &str,
    envelope: &str,
    limit: usize,
    timeout: Duration,
) -> Result<http::Reply, CastError> {
    let headers = [
        ("SOAPACTION".to_string(), format!("\"{action}\"")),
        (
            "Content-Type".to_string(),
            "text/xml; charset=\"utf-8\"".to_string(),
        ),
    ];
    http::request(
        control_url,
        "POST",
        &headers,
        Some(envelope),
        limit,
        timeout,
    )
}

pub fn parse_fault(text: &str, action: &str) -> Option<CastError> {
    let is_fault = text.contains("<s:Fault")
        || text.contains("<Fault")
        || text.contains("faultcode")
        || text.contains("<UPnPError");
    if !is_fault {
        return None;
    }
    let code = casting::element_text(text, "errorCode")
        .or_else(|| casting::element_text(text, "faultcode"))
        .unwrap_or("unknown");
    let description = casting::element_text(text, "errorDescription")
        .or_else(|| casting::element_text(text, "faultstring"))
        .unwrap_or("the receiver refused without a description");
    Some(CastError::Fault {
        action: action.to_string(),
        code: clamp_text(&casting::decode_entities(code)),
        description: clamp_text(&casting::decode_entities(description)),
    })
}

pub fn parse_transport_state(text: &str) -> Option<TransportState> {
    casting::element_text(text, "CurrentTransportState")
        .map(|state| TransportState::parse(&casting::decode_entities(state)))
}

fn summarize(text: &str) -> String {
    let collapsed = text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace('<', " <");
    if collapsed.trim().is_empty() {
        return "the receiver answered without a body".to_string();
    }
    clamp_text(&collapsed)
}

fn clamp_text(value: &str) -> String {
    value
        .chars()
        .take(MAX_TEXT)
        .collect::<String>()
        .trim()
        .to_string()
}

fn receiver_remote(receiver: &Receiver) -> Result<SocketAddr, CastError> {
    let ip: IpAddr = receiver.from.parse().map_err(|_| {
        CastError::Session(format!(
            "The receiver address {} is not an IP address, so this machine cannot pick a local \
             address for it.",
            receiver.from
        ))
    })?;
    Ok(SocketAddr::new(ip, RECEIVER_PORT))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::thread::JoinHandle;

    const DESCRIPTION: &str = "<?xml version=\"1.0\"?><root xmlns=\"urn:schemas-upnp-org:device-1-0\"><device><deviceType>urn:schemas-upnp-org:device:MediaRenderer:1</deviceType><friendlyName>Fake Renderer</friendlyName><serviceList><service><serviceType>urn:schemas-upnp-org:service:AVTransport:1</serviceType><serviceId>urn:upnp-org:serviceId:AVTransport</serviceId><SCPDURL>/scpd/avt.xml</SCPDURL><controlURL>/control/avt</controlURL><eventSubURL>/event/avt</eventSubURL></service><service><serviceType>urn:schemas-upnp-org:service:RenderingControl:1</serviceType><serviceId>urn:upnp-org:serviceId:RenderingControl</serviceId><SCPDURL>/scpd/rcs.xml</SCPDURL><controlURL>/control/rcs</controlURL><eventSubURL>/event/rcs</eventSubURL></service></serviceList></device></root>";

    #[derive(Debug, Clone)]
    struct Recorded {
        action: String,
        body: String,
    }

    struct FakeState {
        transport: String,
        requests: Vec<Recorded>,
        refuse_play: bool,
    }

    struct FakeReceiver {
        address: SocketAddr,
        state: Arc<std::sync::Mutex<FakeState>>,
        stop: Arc<AtomicBool>,
        thread: Option<JoinHandle<()>>,
    }

    impl FakeReceiver {
        fn start(refuse_play: bool) -> FakeReceiver {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
            let address = listener.local_addr().expect("address");
            listener.set_nonblocking(true).expect("poll");
            let state = Arc::new(std::sync::Mutex::new(FakeState {
                transport: "STOPPED".to_string(),
                requests: Vec::new(),
                refuse_play,
            }));
            let stop = Arc::new(AtomicBool::new(false));
            let thread_state = Arc::clone(&state);
            let thread_stop = Arc::clone(&stop);
            let thread = std::thread::spawn(move || {
                while !thread_stop.load(Ordering::SeqCst) {
                    match listener.accept() {
                        Ok((mut stream, _)) => {
                            let _ = stream.set_nonblocking(false);
                            let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                            answer(&mut stream, &thread_state);
                        }
                        Err(_) => std::thread::sleep(Duration::from_millis(2)),
                    }
                }
            });
            FakeReceiver {
                address,
                state,
                stop,
                thread: Some(thread),
            }
        }

        fn receiver(&self) -> Receiver {
            Receiver {
                id: "uuid:fake".to_string(),
                name: "Fake Renderer".to_string(),
                kind: "DLNA MediaRenderer".to_string(),
                address: format!("http://{}/description.xml", self.address),
                from: self.address.ip().to_string(),
            }
        }

        fn actions(&self) -> Vec<String> {
            self.state
                .lock()
                .expect("state")
                .requests
                .iter()
                .map(|record| record.action.clone())
                .collect()
        }

        fn body_of(&self, action: &str) -> Option<String> {
            self.state
                .lock()
                .expect("state")
                .requests
                .iter()
                .find(|record| record.action == action)
                .map(|record| record.body.clone())
        }

        fn current_uri(&self) -> Option<String> {
            self.body_of("SetAVTransportURI").and_then(|body| {
                super::casting::element_text(&body, "CurrentURI")
                    .map(super::casting::decode_entities)
            })
        }
    }

    impl Drop for FakeReceiver {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::SeqCst);
            if let Some(handle) = self.thread.take() {
                let _ = handle.join();
            }
        }
    }

    fn answer(stream: &mut TcpStream, state: &Arc<std::sync::Mutex<FakeState>>) {
        let Some((method, target, headers, body)) = read_request(stream) else {
            return;
        };
        if method == "GET" && target == "/description.xml" {
            respond(stream, "200 OK", DESCRIPTION);
            return;
        }
        if method != "POST" || target != "/control/avt" {
            respond(stream, "404 Not Found", "missing");
            return;
        }
        let action = headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("SOAPACTION"))
            .and_then(|(_, value)| value.rsplit('#').next())
            .map(|value| value.trim_matches('"').to_string())
            .unwrap_or_default();
        let mut state = state.lock().expect("state");
        state.requests.push(Recorded {
            action: action.clone(),
            body: body.clone(),
        });
        if state.refuse_play && action == "Play" {
            respond(
                stream,
                "500 Internal Server Error",
                "<s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\"><s:Body><s:Fault><faultcode>s:Client</faultcode><faultstring>UPnPError</faultstring><detail><UPnPError xmlns=\"urn:schemas-upnp-org:control-1-0\"><errorCode>701</errorCode><errorDescription>Transition not available</errorDescription></UPnPError></detail></s:Fault></s:Body></s:Envelope>",
            );
            return;
        }
        match action.as_str() {
            "Play" => state.transport = "PLAYING".to_string(),
            "Pause" => state.transport = "PAUSED_PLAYBACK".to_string(),
            "Stop" => state.transport = "STOPPED".to_string(),
            _ => {}
        }
        let transport = state.transport.clone();
        drop(state);
        if action == "GetTransportInfo" {
            respond(
                stream,
                "200 OK",
                &format!(
                    "<s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\"><s:Body><u:GetTransportInfoResponse xmlns:u=\"urn:schemas-upnp-org:service:AVTransport:1\"><CurrentTransportState>{transport}</CurrentTransportState><CurrentTransportStatus>OK</CurrentTransportStatus><CurrentSpeed>1</CurrentSpeed></u:GetTransportInfoResponse></s:Body></s:Envelope>"
                ),
            );
            return;
        }
        respond(
            stream,
            "200 OK",
            &format!(
                "<s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\"><s:Body><u:{action}Response xmlns:u=\"urn:schemas-upnp-org:service:AVTransport:1\"/></s:Body></s:Envelope>"
            ),
        );
    }

    type Incoming = (String, String, Vec<(String, String)>, String);

    fn read_request(stream: &mut TcpStream) -> Option<Incoming> {
        let mut raw = Vec::new();
        let mut buffer = [0_u8; 1_024];
        let head_end = loop {
            let read = stream.read(&mut buffer).ok()?;
            if read == 0 {
                return None;
            }
            raw.extend_from_slice(&buffer[..read]);
            if let Some(index) = raw.windows(4).position(|window| window == b"\r\n\r\n") {
                break index;
            }
            if raw.len() > 16 * 1024 {
                return None;
            }
        };
        let head = String::from_utf8_lossy(&raw[..head_end]).into_owned();
        let mut body = raw[head_end + 4..].to_vec();
        let mut lines = head.lines();
        let request_line = lines.next()?.to_string();
        let mut parts = request_line.split_whitespace();
        let method = parts.next()?.to_string();
        let target = parts.next()?.to_string();
        let mut headers = Vec::new();
        let mut length = 0;
        for line in lines {
            let Some((name, value)) = line.split_once(':') else {
                continue;
            };
            if name.trim().eq_ignore_ascii_case("content-length") {
                length = value.trim().parse().unwrap_or(0);
            }
            headers.push((name.trim().to_string(), value.trim().to_string()));
        }
        while body.len() < length {
            let read = stream.read(&mut buffer).ok()?;
            if read == 0 {
                break;
            }
            body.extend_from_slice(&buffer[..read]);
        }
        Some((
            method,
            target,
            headers,
            String::from_utf8_lossy(&body).into_owned(),
        ))
    }

    fn respond(stream: &mut TcpStream, status: &str, body: &str) {
        let head = format!(
            "HTTP/1.1 {status}\r\nContent-Type: text/xml; charset=\"utf-8\"\r\n\
             Content-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = stream.write_all(head.as_bytes());
        let _ = stream.write_all(body.as_bytes());
        let _ = stream.flush();
    }

    fn one_shot(status: &str, body: &str) -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let address = listener.local_addr().expect("address");
        let status = status.to_string();
        let body = body.to_string();
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let _ = read_request(&mut stream);
                respond(&mut stream, &status, &body);
            }
        });
        address
    }

    fn hardware() -> (tempfile::TempDir, PathBuf, Vec<u8>) {
        let directory = tempfile::tempdir().expect("tempdir");
        let file = directory.path().join("film.mp4");
        let media: Vec<u8> = (0..4_096).map(|index| (index % 251) as u8).collect();
        std::fs::write(&file, &media).expect("write");
        (directory, file, media)
    }

    fn fetch(url: &str, range: &str) -> (u16, Vec<u8>) {
        let parsed = super::http::Url::parse(url).expect("url");
        let addresses = parsed.socket_addresses().expect("addresses");
        let mut stream =
            TcpStream::connect_timeout(&addresses[0], Duration::from_secs(2)).expect("connect");
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("timeout");
        stream
            .write_all(
                format!(
                    "GET {} HTTP/1.1\r\nHost: test\r\nRange: {range}\r\nConnection: close\r\n\r\n",
                    parsed.target
                )
                .as_bytes(),
            )
            .expect("write");
        let mut raw = Vec::new();
        let _ = stream.read_to_end(&mut raw);
        let text = String::from_utf8_lossy(&raw).into_owned();
        let status = text
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .and_then(|code| code.parse::<u16>().ok())
            .unwrap_or(0);
        let body = match raw.windows(4).position(|window| window == b"\r\n\r\n") {
            Some(index) => raw[index + 4..].to_vec(),
            None => Vec::new(),
        };
        (status, body)
    }

    #[test]
    fn the_soap_envelopes_have_the_shape_a_renderer_expects() {
        let play = soap_play();
        assert!(play.starts_with("<?xml version=\"1.0\" encoding=\"utf-8\"?>"));
        assert!(play.contains(
            "<s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\" \
             s:encodingStyle=\"http://schemas.xmlsoap.org/soap/encoding/\">"
        ));
        assert!(play.contains("<u:Play xmlns:u=\"urn:schemas-upnp-org:service:AVTransport:1\">"));
        assert!(play.contains("<InstanceID>0</InstanceID><Speed>1</Speed>"));
        assert!(play.contains("</u:Play></s:Body></s:Envelope>"));
        assert_eq!(
            soap_pause().matches("<InstanceID>0</InstanceID>").count(),
            1
        );
        assert!(soap_stop().contains("<u:Stop"));
        assert!(soap_get_transport_info().contains("<u:GetTransportInfo"));
        let seek = soap_seek(Duration::from_secs(90));
        assert!(seek.contains("<Unit>REL_TIME</Unit><Target>00:01:30</Target>"));
        assert_eq!(transport_time(Duration::from_secs(3_725)), "01:02:05");
    }

    #[test]
    fn the_transport_uri_is_escaped_and_only_http_is_offered() {
        let metadata = didl_metadata("A & B <c>", "http://127.0.0.1:9/stream/t", "video/mp4", 12)
            .expect("metadata");
        assert!(metadata.contains("<dc:title>A &amp; B &lt;c&gt;</dc:title>"));
        assert!(metadata.contains("<upnp:class>object.item.videoItem</upnp:class>"));
        assert!(metadata.contains("protocolInfo=\"http-get:*:video/mp4:*\""));
        assert!(metadata.contains("size=\"12\""));
        let envelope =
            soap_set_av_transport_uri("http://127.0.0.1:9/stream/t", &metadata).expect("envelope");
        assert!(envelope.contains("<CurrentURI>http://127.0.0.1:9/stream/t</CurrentURI>"));
        assert!(
            envelope.contains("&lt;DIDL-Lite"),
            "the DIDL travels as an escaped text node"
        );
        assert!(envelope.contains("&lt;/DIDL-Lite&gt;"));
        assert!(
            !envelope.contains("<DIDL-Lite"),
            "raw markup inside the envelope would be malformed"
        );
        for refused in [
            "file:///etc/passwd",
            "https://127.0.0.1:9/stream/t",
            "javascript:alert(1)",
        ] {
            assert!(
                soap_set_av_transport_uri(refused, &metadata).is_err(),
                "{refused}"
            );
        }
    }

    #[test]
    fn a_non_200_answer_and_a_soap_fault_are_different_errors() {
        let fault = one_shot(
            "500 Internal Server Error",
            "<s:Envelope><s:Body><s:Fault><faultcode>s:Client</faultcode><faultstring>UPnPError\
             </faultstring><detail><UPnPError><errorCode>701</errorCode><errorDescription>\
             Transition not available</errorDescription></UPnPError></detail></s:Fault></s:Body>\
             </s:Envelope>",
        );
        let outcome = soap_action(
            &format!("http://{fault}/control/avt"),
            "Play",
            &soap_play(),
            Duration::from_secs(2),
        );
        match outcome {
            Err(CastError::Fault {
                action,
                code,
                description,
            }) => {
                assert_eq!(action, "Play");
                assert_eq!(code, "701");
                assert_eq!(description, "Transition not available");
            }
            other => panic!("expected a fault, got {other:?}"),
        }

        let plain = one_shot("503 Service Unavailable", "busy");
        let outcome = soap_action(
            &format!("http://{plain}/control/avt"),
            "Play",
            &soap_play(),
            Duration::from_secs(2),
        );
        match outcome {
            Err(CastError::Action {
                action,
                status,
                detail,
            }) => {
                assert_eq!(action, "Play");
                assert_eq!(status, 503);
                assert!(detail.contains("busy"));
            }
            other => panic!("expected a plain refusal, got {other:?}"),
        }
    }

    #[test]
    fn the_transport_state_comes_from_the_receivers_own_words() {
        assert_eq!(
            parse_transport_state("<CurrentTransportState>PLAYING</CurrentTransportState>"),
            Some(TransportState::Playing)
        );
        assert_eq!(
            parse_transport_state(
                "<u:CurrentTransportState>PAUSED_PLAYBACK</u:CurrentTransportState>"
            ),
            Some(TransportState::Paused)
        );
        assert_eq!(
            parse_transport_state(
                "<CurrentTransportState>NO_MEDIA_PRESENT</CurrentTransportState>"
            ),
            Some(TransportState::NoMedia)
        );
        assert_eq!(parse_transport_state("<nothing/>"), None);
        assert_eq!(
            TransportState::parse("WAT"),
            TransportState::Unknown("the receiver reported WAT".to_string())
        );
        assert!(
            session_summary(&CastStatus {
                receiver_name: "R".to_string(),
                state: TransportState::Paused,
                stream_url: "http://127.0.0.1:1/stream/t".to_string(),
                stream_running: true,
                remaining: Duration::from_secs(600),
            })
            .contains("no real-device test is recorded")
        );
    }

    #[test]
    fn a_full_session_runs_from_description_to_disconnect_without_a_device() {
        let receiver = FakeReceiver::start(false);
        let (_directory, file, media) = hardware();
        let request = SessionRequest {
            bind: Some("127.0.0.1:0".parse().expect("bind")),
            ..SessionRequest::new(
                receiver.receiver(),
                file,
                MediaFacts::video("mp4", "h264", "aac"),
                "A film",
            )
        };
        let mut session = connect(request).expect("session");
        assert_eq!(session.state(), &TransportState::Playing);
        assert_eq!(
            receiver.actions(),
            vec!["SetAVTransportURI", "Play", "GetTransportInfo"]
        );
        let uri = receiver
            .current_uri()
            .expect("the receiver saw the media address");
        assert_eq!(uri, session.stream_url());
        let (status, body) = fetch(&uri, "bytes=0-15");
        assert_eq!(status, 206);
        assert_eq!(body, media[..16].to_vec());

        assert_eq!(session.pause().expect("pause"), TransportState::Paused);
        assert!(receiver.body_of("Pause").is_some());
        let seek = super::soap_seek(Duration::from_secs(90));
        assert!(seek.contains("00:01:30"));
        assert_eq!(
            session.seek(Duration::from_secs(90)).expect("seek"),
            TransportState::Paused
        );
        assert!(
            receiver
                .body_of("Seek")
                .expect("seek reached the receiver")
                .contains("<Target>00:01:30</Target>")
        );
        assert_eq!(session.resume().expect("resume"), TransportState::Playing);
        assert_eq!(session.stop().expect("stop"), TransportState::Stopped);
        assert!(!session.is_streaming());

        let address = session.stream_url().to_string();
        let parsed = super::http::Url::parse(&address).expect("url");
        let endpoint = SocketAddr::new(parsed.host.parse().expect("ip"), parsed.port);
        session.disconnect().expect("disconnect");
        assert!(
            TcpStream::connect_timeout(&endpoint, Duration::from_millis(300)).is_err(),
            "disconnect must leave no listener running"
        );
    }

    #[test]
    fn a_receiver_that_refuses_play_names_the_fault_and_closes_the_endpoint() {
        let receiver = FakeReceiver::start(true);
        let (_directory, file, _media) = hardware();
        let request = SessionRequest {
            bind: Some("127.0.0.1:0".parse().expect("bind")),
            ..SessionRequest::new(
                receiver.receiver(),
                file,
                MediaFacts::video("mp4", "h264", "aac"),
                "A film",
            )
        };
        let outcome = connect(request);
        match outcome {
            Err(CastError::Fault { action, code, .. }) => {
                assert_eq!(action, "Play");
                assert_eq!(code, "701");
            }
            other => panic!(
                "a refused Play must be a fault, got {:?}",
                other.map(|session| session.state().clone())
            ),
        }
        assert_eq!(receiver.actions(), vec!["SetAVTransportURI", "Play"]);
        let uri = receiver
            .current_uri()
            .expect("the receiver still saw the media address");
        let parsed = super::http::Url::parse(&uri).expect("url");
        let endpoint = SocketAddr::new(parsed.host.parse().expect("ip"), parsed.port);
        assert!(
            TcpStream::connect_timeout(&endpoint, Duration::from_millis(300)).is_err(),
            "a refused Play must not leave a listener behind"
        );
    }

    #[test]
    fn a_file_the_profile_cannot_justify_is_refused_before_any_network_call() {
        let (_directory, file, _media) = hardware();
        let unreachable = Receiver {
            id: "uuid:dead".to_string(),
            name: "Dead".to_string(),
            kind: "DLNA MediaRenderer".to_string(),
            address: "http://127.0.0.1:1/description.xml".to_string(),
            from: "127.0.0.1".to_string(),
        };
        let refused = connect(SessionRequest::new(
            unreachable.clone(),
            file.clone(),
            MediaFacts::video("wmv", "wmv3", "wma"),
            "A film",
        ));
        assert!(
            matches!(refused, Err(CastError::Media(_))),
            "an unjustifiable codec must be refused without touching the network"
        );
        let unproven = connect(SessionRequest::new(
            unreachable,
            file,
            MediaFacts::video("mkv", "h264", "aac"),
            "A film",
        ));
        assert!(matches!(unproven, Err(CastError::Media(_))));
    }

    #[test]
    fn a_lost_receiver_ends_the_session_and_leaves_no_listener() {
        let receiver = FakeReceiver::start(false);
        let (_directory, file, _media) = hardware();
        let mut session = connect(SessionRequest {
            bind: Some("127.0.0.1:0".parse().expect("bind")),
            ..SessionRequest::new(
                receiver.receiver(),
                file,
                MediaFacts::video("mp4", "h264", "aac"),
                "A film",
            )
        })
        .expect("session");
        let parsed = super::http::Url::parse(session.stream_url()).expect("url");
        let endpoint = SocketAddr::new(parsed.host.parse().expect("ip"), parsed.port);
        drop(receiver);
        let outcome = session.pause();
        assert!(outcome.is_err(), "a lost receiver must be an error");
        assert!(!session.is_streaming(), "the endpoint must be closed");
        assert!(
            TcpStream::connect_timeout(&endpoint, Duration::from_millis(300)).is_err(),
            "the listener must be gone"
        );
        assert!(session.summary().contains("closed"));
    }

    #[test]
    fn the_session_note_never_claims_a_verified_cast() {
        let status = CastStatus {
            receiver_name: "Living room".to_string(),
            state: TransportState::Playing,
            stream_url: "http://127.0.0.1:1/stream/t".to_string(),
            stream_running: true,
            remaining: DEFAULT_LIFETIME,
        };
        let note = casting_note(1, Some(&status));
        assert!(note.contains("temporary endpoint"));
        assert!(note.contains("playing"));
        assert!(note.contains("no real-device test"));
        let idle = casting_note(0, None);
        assert!(idle.contains("No receiver answered"));
        assert!(idle.contains("nothing is being streamed"));
        let found = casting_note(2, None);
        assert!(found.contains("discovery only"));
    }

    #[test]
    fn the_http_client_is_bounded_and_refuses_what_it_cannot_do() {
        let oversized = "x".repeat(MAX_SOAP_BYTES + 8_192);
        let address = one_shot("200 OK", &oversized);
        let outcome = super::http::get(
            &format!("http://{address}/big"),
            1_024,
            Duration::from_secs(2),
        );
        assert!(outcome.is_err(), "an oversized answer must be refused");

        assert!(super::http::Url::parse("file:///etc/passwd").is_err());
        assert!(super::http::Url::parse("http://user:secret@host/x").is_err());
        assert!(super::http::Url::parse("http://host:0/x").is_err());
        assert!(super::http::Url::parse("http://[::1]:8080/x").is_ok());
        assert_eq!(
            super::http::Url::parse("http://[::1]:8080/x")
                .expect("ipv6")
                .authority(),
            "[::1]:8080"
        );
        let needs_tls =
            super::http::get("https://127.0.0.1:1/x", 1_024, Duration::from_millis(250));
        assert!(matches!(needs_tls, Err(CastError::Refused(_))));
    }

    #[test]
    fn an_unreachable_receiver_is_a_clear_error_and_never_a_panic() {
        let (_directory, file, _media) = hardware();
        let receiver = Receiver {
            id: "uuid:dead".to_string(),
            name: "Dead".to_string(),
            kind: "DLNA MediaRenderer".to_string(),
            address: "http://127.0.0.1:1/description.xml".to_string(),
            from: "127.0.0.1".to_string(),
        };
        let outcome = connect(SessionRequest::new(
            receiver,
            file,
            MediaFacts::video("mp4", "h264", "aac"),
            "A film",
        ));
        assert!(
            matches!(outcome, Err(CastError::Unreachable(_))),
            "a refused description must be an unreachable error"
        );
    }

    #[test]
    fn a_slow_receiver_times_out_instead_of_hanging() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let address = listener.local_addr().expect("address");
        std::thread::spawn(move || {
            if let Ok((stream, _)) = listener.accept() {
                std::thread::sleep(Duration::from_millis(600));
                drop(stream);
            }
        });
        let started = std::time::Instant::now();
        let outcome = super::http::get(
            &format!("http://{address}/slow"),
            1_024,
            Duration::from_millis(250),
        );
        assert!(matches!(outcome, Err(CastError::TimedOut(_))));
        assert!(started.elapsed() < Duration::from_secs(3));
    }
}
