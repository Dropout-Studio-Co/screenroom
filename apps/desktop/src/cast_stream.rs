use std::collections::hash_map::RandomState;
use std::fs::File;
use std::hash::{BuildHasher, Hasher};
use std::io::{Read, Seek, SeekFrom, Write};
use std::net::{IpAddr, Shutdown, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::casting::CastError;

pub const MAX_LIFETIME: Duration = Duration::from_secs(6 * 60 * 60);

pub const MAX_CLIENTS: usize = 4;

const MAX_HEADER_BYTES: usize = 8 * 1024;

const MAX_TARGET_BYTES: usize = 2_048;

const CHUNK_BYTES: usize = 64 * 1024;

const HEADER_DEADLINE: Duration = Duration::from_secs(2);

const HEADER_POLL: Duration = Duration::from_millis(100);

const WRITE_TIMEOUT: Duration = Duration::from_secs(5);

const REFUSAL_TIMEOUT: Duration = Duration::from_millis(250);

const ACCEPT_POLL: Duration = Duration::from_millis(5);

const MAX_HANDSHAKES: usize = 8;

const ITERATIONS: u64 = 2;

struct ServerState {
    stop: AtomicBool,

    active: AtomicUsize,

    handshakes: AtomicUsize,

    limit: usize,

    deadline: Instant,

    workers: Mutex<Vec<JoinHandle<()>>>,
}

impl ServerState {
    fn workers(&self) -> std::sync::MutexGuard<'_, Vec<JoinHandle<()>>> {
        self.workers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn reap_finished_workers(&self) {
        self.workers().retain(|worker| !worker.is_finished());
    }

    fn join_workers(&self) {
        let workers = std::mem::take(&mut *self.workers());
        for worker in workers {
            let _ = worker.join();
        }
    }
}

struct HandshakeSlot {
    state: Arc<ServerState>,
}

impl HandshakeSlot {
    fn reserve(state: &Arc<ServerState>) -> Option<HandshakeSlot> {
        let mut current = state.handshakes.load(Ordering::SeqCst);
        loop {
            if current >= MAX_HANDSHAKES {
                return None;
            }
            match state.handshakes.compare_exchange(
                current,
                current + 1,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => {
                    return Some(HandshakeSlot {
                        state: Arc::clone(state),
                    });
                }
                Err(observed) => current = observed,
            }
        }
    }
}

impl Drop for HandshakeSlot {
    fn drop(&mut self) {
        self.state.handshakes.fetch_sub(1, Ordering::SeqCst);
    }
}

struct ActiveClient<'a> {
    state: &'a ServerState,
}

impl Drop for ActiveClient<'_> {
    fn drop(&mut self) {
        self.state.active.fetch_sub(1, Ordering::SeqCst);
    }
}

pub struct StreamServer {
    address: SocketAddr,

    token: String,

    state: Arc<ServerState>,

    thread: Option<JoinHandle<()>>,

    length: u64,

    expires: Instant,
}

struct Request {
    method: String,

    target: String,

    range: Option<String>,
}

enum Requested {
    Full,

    Partial { start: u64, end: u64 },

    Unsatisfiable,
}

impl StreamServer {
    pub fn start(
        file: PathBuf,
        bind: SocketAddr,
        lifetime: Duration,
        max_clients: usize,
    ) -> Result<StreamServer, CastError> {
        if bind.ip().is_unspecified() {
            return Err(CastError::Stream(
                "Bind the exact address the receiver will call back on, not an unspecified one."
                    .to_string(),
            ));
        }
        if lifetime.is_zero() {
            return Err(CastError::Stream(
                "A casting stream needs a bounded lifetime.".to_string(),
            ));
        }
        let metadata = std::fs::metadata(&file).map_err(|error| {
            CastError::Stream(format!(
                "Cannot read {} to serve it: {error}",
                file.display()
            ))
        })?;
        if !metadata.is_file() {
            return Err(CastError::Stream(
                "Only one regular file can be served.".to_string(),
            ));
        }
        let length = metadata.len();
        let listener = TcpListener::bind(bind).map_err(|error| {
            CastError::Stream(format!("Cannot open a stream endpoint on {bind}: {error}"))
        })?;
        listener.set_nonblocking(true).map_err(|error| {
            CastError::Stream(format!("Cannot make the stream endpoint poll: {error}"))
        })?;
        let address = listener.local_addr().map_err(|error| {
            CastError::Stream(format!("Cannot read the stream address: {error}"))
        })?;
        let token = fresh_token();
        let expires = Instant::now() + lifetime.min(MAX_LIFETIME);
        let state = Arc::new(ServerState {
            stop: AtomicBool::new(false),
            active: AtomicUsize::new(0),
            handshakes: AtomicUsize::new(0),
            limit: max_clients.clamp(1, MAX_CLIENTS),
            deadline: expires,
            workers: Mutex::new(Vec::new()),
        });
        let thread_state = Arc::clone(&state);
        let thread_token = token.clone();
        let thread_file = file.clone();
        let thread =
            thread::spawn(move || accept_loop(listener, thread_file, thread_token, thread_state));
        Ok(StreamServer {
            address,
            token,
            state,
            thread: Some(thread),
            length,
            expires,
        })
    }

    pub fn address(&self) -> SocketAddr {
        self.address
    }

    pub fn port(&self) -> u16 {
        self.address.port()
    }

    pub fn url(&self, host: IpAddr) -> String {
        format!(
            "http://{}/stream/{}",
            SocketAddr::new(host, self.address.port()),
            self.token
        )
    }

    pub fn bytes(&self) -> u64 {
        self.length
    }

    pub fn remaining(&self) -> Duration {
        self.expires.saturating_duration_since(Instant::now())
    }

    pub fn is_running(&self) -> bool {
        let accept_thread_alive = self
            .thread
            .as_ref()
            .is_some_and(|handle| !handle.is_finished());
        accept_thread_alive
            && !self.state.stop.load(Ordering::SeqCst)
            && Instant::now() < self.expires
    }

    pub fn active_clients(&self) -> usize {
        self.state.active.load(Ordering::SeqCst)
    }

    pub fn stop(&mut self) {
        self.state.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
        self.state.join_workers();
    }
}

impl Drop for StreamServer {
    fn drop(&mut self) {
        self.stop();
    }
}

pub fn local_address_towards(remote: SocketAddr) -> Result<IpAddr, CastError> {
    let bind: SocketAddr = if remote.is_ipv4() {
        "0.0.0.0:0".parse().expect("ipv4 wildcard")
    } else {
        "[::]:0".parse().expect("ipv6 wildcard")
    };
    let socket = UdpSocket::bind(bind).map_err(|error| {
        CastError::Stream(format!("Cannot look up this machine's address: {error}"))
    })?;
    socket.connect(remote).map_err(|error| {
        CastError::Stream(format!(
            "Cannot reach the receiver at {remote} to learn this machine's address: {error}"
        ))
    })?;
    let local = socket.local_addr().map_err(|error| {
        CastError::Stream(format!("Cannot read this machine's address: {error}"))
    })?;
    let ip = local.ip();
    if ip.is_unspecified() || (ip.is_loopback() && !remote.ip().is_loopback()) {
        return Err(CastError::Stream(format!(
            "This machine has no address on the receiver's network ({ip} would not reach {remote})."
        )));
    }
    Ok(ip)
}

fn accept_loop(listener: TcpListener, file: PathBuf, token: String, state: Arc<ServerState>) {
    let file = Arc::new(file);
    while !state.stop.load(Ordering::SeqCst) && Instant::now() < state.deadline {
        state.reap_finished_workers();
        let Ok((stream, _)) = listener.accept() else {
            thread::sleep(ACCEPT_POLL);
            continue;
        };
        let Some(handshake) = HandshakeSlot::reserve(&state) else {
            let mut busy = stream;
            respond_or_close(
                &mut busy,
                503,
                "Service Unavailable",
                "too many requests are waiting to be read",
                REFUSAL_TIMEOUT,
            );
            continue;
        };
        let child_state = Arc::clone(&state);
        let child_file = Arc::clone(&file);
        let child_token = token.clone();
        let handle = thread::spawn(move || {
            serve(stream, &child_file, &child_token, &child_state, handshake);
        });
        state.workers().push(handle);
    }
}

fn serve(
    mut stream: TcpStream,
    file: &Path,
    token: &str,
    state: &ServerState,
    handshake: HandshakeSlot,
) {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_write_timeout(Some(WRITE_TIMEOUT));
    let request = read_request(&mut stream, state);
    drop(handshake);
    let Some(request) = request else {
        return;
    };
    if state.stop.load(Ordering::SeqCst) || Instant::now() >= state.deadline {
        respond_or_close(
            &mut stream,
            503,
            "Service Unavailable",
            "this casting session has ended",
            WRITE_TIMEOUT,
        );
        return;
    }
    if request.method != "GET" && request.method != "HEAD" {
        respond_or_close(
            &mut stream,
            405,
            "Method Not Allowed",
            "only GET and HEAD are served",
            WRITE_TIMEOUT,
        );
        return;
    }
    let requested_path = request.target.split('?').next().unwrap_or("");
    if requested_path != format!("/stream/{token}") {
        respond_or_close(
            &mut stream,
            404,
            "Not Found",
            "no such resource",
            WRITE_TIMEOUT,
        );
        return;
    }
    let Ok(metadata) = std::fs::metadata(file) else {
        respond_or_close(
            &mut stream,
            503,
            "Service Unavailable",
            "the file is no longer readable",
            WRITE_TIMEOUT,
        );
        return;
    };
    let length = metadata.len();
    let (status, start, count, content_range) = match parse_range(request.range.as_deref(), length)
    {
        Requested::Full => ("200 OK", 0, length, None),
        Requested::Partial { start, end } => (
            "206 Partial Content",
            start,
            end - start + 1,
            Some(format!("bytes {start}-{end}/{length}")),
        ),
        Requested::Unsatisfiable => {
            let head = format!(
                "HTTP/1.1 416 Range Not Satisfiable\r\nContent-Range: bytes */{length}\r\n\
                 Content-Length: 0\r\nConnection: close\r\n\r\n"
            );
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.flush();
            return;
        }
    };
    if !charge_client(state) {
        respond_or_close(
            &mut stream,
            503,
            "Service Unavailable",
            &busy_message(state.limit),
            WRITE_TIMEOUT,
        );
        return;
    }
    let _active = ActiveClient { state };
    let mut head = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {count}\r\nContent-Type: {}\r\n\
         Accept-Ranges: bytes\r\nConnection: close\r\n",
        mime_for_path(file)
    );
    if let Some(content_range) = content_range {
        head.push_str(&format!("Content-Range: {content_range}\r\n"));
    }
    head.push_str("\r\n");
    if stream.write_all(head.as_bytes()).is_err() {
        return;
    }
    if request.method == "HEAD" {
        let _ = stream.flush();
        return;
    }
    if count == 0 {
        let _ = stream.flush();
        return;
    }
    let Ok(mut handle) = File::open(file) else {
        return;
    };
    if handle.seek(SeekFrom::Start(start)).is_err() {
        return;
    }
    let mut remaining = count;
    let mut buffer = vec![0_u8; CHUNK_BYTES];
    while remaining > 0 {
        if state.stop.load(Ordering::SeqCst) || Instant::now() >= state.deadline {
            return;
        }
        let want = (remaining.min(CHUNK_BYTES as u64)) as usize;
        let read = match handle.read(&mut buffer[..want]) {
            Ok(0) => return,
            Ok(read) => read,
            Err(_) => return,
        };
        if stream.write_all(&buffer[..read]).is_err() {
            return;
        }
        remaining -= read as u64;
    }
    let _ = stream.flush();
}

fn charge_client(state: &ServerState) -> bool {
    let mut current = state.active.load(Ordering::SeqCst);
    loop {
        if current >= state.limit {
            return false;
        }
        match state.active.compare_exchange(
            current,
            current + 1,
            Ordering::SeqCst,
            Ordering::SeqCst,
        ) {
            Ok(_) => return true,
            Err(observed) => current = observed,
        }
    }
}

fn busy_message(limit: usize) -> String {
    match limit {
        1 => "only one client is served by this casting session".to_string(),
        limit => format!("this casting session serves at most {limit} clients at once"),
    }
}

fn read_request(stream: &mut TcpStream, state: &ServerState) -> Option<Request> {
    let deadline = Instant::now() + HEADER_DEADLINE;
    let mut raw = Vec::new();
    let mut buffer = [0_u8; 1_024];
    loop {
        if state.stop.load(Ordering::SeqCst) {
            return None;
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return None;
        }
        let _ = stream.set_read_timeout(Some(remaining.min(HEADER_POLL)));
        match stream.read(&mut buffer) {
            Ok(0) => return None,
            Ok(read) => raw.extend_from_slice(&buffer[..read]),
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock
                        | std::io::ErrorKind::TimedOut
                        | std::io::ErrorKind::Interrupted
                ) => {}
            Err(_) => return None,
        }
        if raw.len() > MAX_HEADER_BYTES {
            return None;
        }
        if raw.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }
    let text = String::from_utf8_lossy(&raw);
    let mut lines = text.split("\r\n");
    let request_line = lines.next()?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next()?.to_ascii_uppercase();
    let target = parts.next()?.to_string();
    let version = parts.next()?;
    if !version.starts_with("HTTP/") {
        return None;
    }
    if target.len() > MAX_TARGET_BYTES || target.chars().any(|value| value.is_control()) {
        return None;
    }
    let mut range = None;
    for line in lines {
        if line.is_empty() {
            break;
        }
        let (name, value) = line.split_once(':')?;
        if name.trim().eq_ignore_ascii_case("range") {
            range = Some(value.trim().to_string());
        }
    }
    Some(Request {
        method,
        target,
        range,
    })
}

fn parse_range(header: Option<&str>, length: u64) -> Requested {
    let Some(value) = header else {
        return Requested::Full;
    };
    let value = value.trim();
    let Some(unit) = value.get(..6) else {
        return Requested::Full;
    };
    if !unit.eq_ignore_ascii_case("bytes=") {
        return Requested::Full;
    }
    let spec = value[6..].trim();
    if spec.contains(',') {
        return Requested::Full;
    }
    let Some((first, second)) = spec.split_once('-') else {
        return Requested::Full;
    };
    let first = first.trim();
    let second = second.trim();
    if first.is_empty() {
        let Ok(suffix) = second.parse::<u64>() else {
            return Requested::Full;
        };
        if suffix == 0 || length == 0 {
            return Requested::Unsatisfiable;
        }
        return Requested::Partial {
            start: length.saturating_sub(suffix),
            end: length - 1,
        };
    }
    let Ok(start) = first.parse::<u64>() else {
        return Requested::Full;
    };
    if length == 0 || start >= length {
        return Requested::Unsatisfiable;
    }
    let end = if second.is_empty() {
        length - 1
    } else {
        let Ok(end) = second.parse::<u64>() else {
            return Requested::Full;
        };
        end.min(length - 1)
    };
    if end < start {
        return Requested::Unsatisfiable;
    }
    Requested::Partial { start, end }
}

fn respond_text(
    stream: &mut TcpStream,
    status: u16,
    reason: &str,
    message: &str,
) -> std::io::Result<()> {
    let body = format!("{message}\n");
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/plain; charset=utf-8\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(body.as_bytes())?;
    stream.flush()
}

fn respond_or_close(
    stream: &mut TcpStream,
    status: u16,
    reason: &str,
    message: &str,
    timeout: Duration,
) {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_write_timeout(Some(timeout));
    if respond_text(stream, status, reason, message).is_err() {
        let _ = stream.shutdown(Shutdown::Both);
    }
}

fn mime_for_path(path: &Path) -> &'static str {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    crate::casting::mime_for(extension)
}

fn fresh_token() -> String {
    let mut token = String::new();
    for round in 0..ITERATIONS {
        let state = RandomState::new();
        let mut hasher = state.build_hasher();
        hasher.write_u64(round);
        hasher.write_u32(std::process::id());
        hasher.write_u128(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|elapsed| elapsed.as_nanos())
                .unwrap_or(0),
        );
        token.push_str(&format!("{:016x}", hasher.finish()));
    }
    token
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(bytes: usize) -> (tempfile::TempDir, PathBuf, Vec<u8>) {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("film.mp4");
        let media: Vec<u8> = (0..bytes).map(|index| (index % 251) as u8).collect();
        std::fs::write(&path, &media).expect("write");
        (directory, path, media)
    }

    fn server(
        bytes: usize,
        lifetime: Duration,
        clients: usize,
    ) -> (tempfile::TempDir, StreamServer, Vec<u8>) {
        let (directory, path, media) = fixture(bytes);
        let server = StreamServer::start(
            path,
            "127.0.0.1:0".parse().expect("bind"),
            lifetime,
            clients,
        )
        .expect("server");
        (directory, server, media)
    }

    struct Reply {
        head: String,
        body: Vec<u8>,
    }

    fn request(server: &StreamServer, method: &str, target: &str, range: Option<&str>) -> Reply {
        let mut stream = TcpStream::connect(server.address()).expect("connect");
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("timeout");
        let mut head = format!("{method} {target} HTTP/1.1\r\nHost: test\r\n");
        if let Some(range) = range {
            head.push_str(&format!("Range: {range}\r\n"));
        }
        head.push_str("Connection: close\r\n\r\n");
        stream.write_all(head.as_bytes()).expect("write");
        let mut raw = Vec::new();
        let _ = stream.read_to_end(&mut raw);
        let split = raw
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .unwrap_or(raw.len());
        let head = String::from_utf8_lossy(&raw[..split]).into_owned();
        let body = if split < raw.len() {
            raw[split + 4..].to_vec()
        } else {
            Vec::new()
        };
        Reply { head, body }
    }

    fn body_of(reply: &Reply) -> &[u8] {
        &reply.body
    }

    fn status_of(reply: &Reply) -> &str {
        reply.head.lines().next().unwrap_or("")
    }

    fn token_path(server: &StreamServer) -> String {
        let url = server.url("127.0.0.1".parse().expect("ip"));
        let tail = url.rsplit('/').next().expect("token");
        format!("/stream/{tail}")
    }

    fn hold_stream(server: &StreamServer, path: &str) -> TcpStream {
        let mut stream = TcpStream::connect(server.address()).expect("connect");
        stream
            .write_all(
                format!("GET {path} HTTP/1.1\r\nHost: test\r\nConnection: close\r\n\r\n")
                    .as_bytes(),
            )
            .expect("write");
        stream
    }

    fn wait_for_clients(server: &StreamServer, expected: usize) {
        let deadline = Instant::now() + Duration::from_secs(3);
        while server.active_clients() != expected && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(
            server.active_clients(),
            expected,
            "the client budget did not settle"
        );
    }

    #[test]
    fn only_the_opaque_token_is_served_and_every_other_path_is_a_404() {
        let (_directory, server, media) = server(1_024, Duration::from_secs(60), 1);
        let path = token_path(&server);
        let served = request(&server, "GET", &path, None);
        assert_eq!(status_of(&served), "HTTP/1.1 200 OK");
        assert_eq!(body_of(&served), &media[..]);

        for refused in [
            "/".to_string(),
            "/stream".to_string(),
            "/stream/".to_string(),
            "/stream/wrong".to_string(),
            "/stream/wrong/token".to_string(),
            format!("{path}/../etc/passwd"),
            "/etc/passwd".to_string(),
        ] {
            let reply = request(&server, "GET", &refused, None);
            assert_eq!(status_of(&reply), "HTTP/1.1 404 Not Found", "{refused}");
        }

        let posted = request(&server, "POST", &path, None);
        assert_eq!(status_of(&posted), "HTTP/1.1 405 Method Not Allowed");

        let with_query = request(&server, "GET", &format!("{path}?seek=5"), None);
        assert_eq!(
            status_of(&with_query),
            "HTTP/1.1 200 OK",
            "a renderer may add a query to the opaque address"
        );
    }

    #[test]
    fn a_range_request_returns_206_with_the_exact_bytes_and_content_range() {
        let (_directory, server, media) = server(4_096, Duration::from_secs(60), 1);
        let path = token_path(&server);
        let reply = request(&server, "GET", &path, Some("bytes=100-199"));
        assert_eq!(status_of(&reply), "HTTP/1.1 206 Partial Content");
        assert!(reply.head.contains("Content-Range: bytes 100-199/4096"));
        assert!(reply.head.contains("Accept-Ranges: bytes"));
        assert_eq!(body_of(&reply), &media[100..200]);

        let open = request(&server, "GET", &path, Some("bytes=4000-"));
        assert_eq!(status_of(&open), "HTTP/1.1 206 Partial Content");
        assert!(open.head.contains("Content-Range: bytes 4000-4095/4096"));
        assert_eq!(body_of(&open), &media[4000..]);

        let suffix = request(&server, "GET", &path, Some("bytes=-16"));
        assert!(suffix.head.contains("Content-Range: bytes 4080-4095/4096"));
        assert_eq!(body_of(&suffix), &media[4080..]);
    }

    #[test]
    fn an_impossible_range_is_416_and_a_whole_file_is_200() {
        let (_directory, server, _media) = server(64, Duration::from_secs(60), 1);
        let path = token_path(&server);
        for range in ["bytes=64-", "bytes=100-200", "bytes=-0", "bytes=9-4"] {
            let reply = request(&server, "GET", &path, Some(range));
            assert_eq!(
                status_of(&reply),
                "HTTP/1.1 416 Range Not Satisfiable",
                "{range}"
            );
            assert!(reply.head.contains("Content-Range: bytes */64"));
        }
        let whole = request(&server, "GET", &path, None);
        assert_eq!(status_of(&whole), "HTTP/1.1 200 OK");
        assert!(whole.head.contains("Content-Length: 64"));
        let multipart = request(&server, "GET", &path, Some("bytes=0-1,4-5"));
        assert_eq!(status_of(&multipart), "HTTP/1.1 200 OK");
    }

    #[test]
    fn a_head_request_answers_the_headers_without_the_media() {
        let (_directory, server, _media) = server(2_048, Duration::from_secs(60), 1);
        let path = token_path(&server);
        let reply = request(&server, "HEAD", &path, Some("bytes=0-9"));
        assert_eq!(status_of(&reply), "HTTP/1.1 206 Partial Content");
        assert!(reply.head.contains("Content-Length: 10"));
        assert!(reply.head.contains("Content-Type: video/mp4"));
        assert!(body_of(&reply).is_empty());
    }

    #[test]
    fn stopping_closes_the_port_and_drops_the_token() {
        let (_directory, mut server, _media) = server(256, Duration::from_secs(60), 1);
        let address = server.address();
        let path = token_path(&server);
        assert!(server.is_running());
        assert_eq!(
            status_of(&request(&server, "GET", &path, None)),
            "HTTP/1.1 200 OK"
        );
        server.stop();
        assert!(!server.is_running());
        assert!(
            TcpStream::connect_timeout(&address, Duration::from_millis(300)).is_err(),
            "the listener must be closed after stop"
        );
    }

    #[test]
    fn the_hard_lifetime_ends_the_endpoint_without_anyone_asking() {
        let (_directory, server, _media) = server(64, Duration::from_millis(150), 1);
        let address = server.address();
        std::thread::sleep(Duration::from_millis(350));
        assert!(!server.is_running());
        assert!(
            TcpStream::connect_timeout(&address, Duration::from_millis(300)).is_err(),
            "the lifetime must close the listener"
        );
    }

    #[test]
    fn a_second_client_is_refused_while_the_first_is_still_being_served() {
        let (_directory, server, _media) = server(8 * 1_024 * 1_024, Duration::from_secs(60), 1);
        let path = token_path(&server);
        let held = hold_stream(&server, &path);
        wait_for_clients(&server, 1);
        let second = request(&server, "GET", &path, None);
        assert_eq!(status_of(&second), "HTTP/1.1 503 Service Unavailable");
        let body = String::from_utf8_lossy(body_of(&second)).into_owned();
        assert!(
            body.contains("only one client is served"),
            "the refusal must state the real limit, got {body}"
        );
        drop(held);
        wait_for_clients(&server, 0);
        assert_eq!(
            status_of(&request(&server, "GET", &path, None)),
            "HTTP/1.1 200 OK"
        );
    }

    #[test]
    fn the_refusal_message_reports_the_configured_number_of_clients() {
        assert!(busy_message(1).contains("only one client"));
        let two = busy_message(2);
        assert!(two.contains("at most 2 clients"), "got {two}");
        assert!(!two.contains("only one"), "got {two}");
        assert!(busy_message(MAX_CLIENTS).contains("at most 4 clients"));
    }

    #[test]
    fn a_connection_that_never_finishes_its_request_takes_no_shared_slot() {
        let (_directory, server, media) = server(1_024, Duration::from_secs(60), 1);
        let path = token_path(&server);
        let mut stalled = TcpStream::connect(server.address()).expect("connect");
        stalled
            .write_all(b"GET /stream/partial HTTP/1.1\r\n")
            .expect("write");
        thread::sleep(Duration::from_millis(200));
        assert_eq!(
            server.active_clients(),
            0,
            "an unvalidated connection must not consume the shared budget"
        );
        let served = request(&server, "GET", &path, None);
        assert_eq!(
            status_of(&served),
            "HTTP/1.1 200 OK",
            "the legitimate token request must still be served"
        );
        assert_eq!(body_of(&served), &media[..]);
        drop(stalled);
    }

    #[test]
    fn a_trickled_request_header_is_cut_off_by_the_aggregate_deadline() {
        let (_directory, server, _media) = server(64, Duration::from_secs(60), 1);
        let mut peer = TcpStream::connect(server.address()).expect("connect");
        peer.set_read_timeout(Some(Duration::from_secs(5)))
            .expect("timeout");
        let started = Instant::now();
        peer.write_all(b"G").expect("write");
        thread::sleep(Duration::from_millis(1_900));
        let _ = peer.write_all(b"E");
        let mut scratch = [0_u8; 16];
        let read = peer.read(&mut scratch);
        assert!(
            matches!(read, Ok(0)) || read.is_err(),
            "the aggregate deadline must close the connection, got {read:?}"
        );
        assert!(
            started.elapsed() < Duration::from_secs(4),
            "the whole header budget is a couple of seconds, took {:?}",
            started.elapsed()
        );
        assert_eq!(server.active_clients(), 0);
    }

    #[test]
    fn stopping_closes_the_port_while_a_peer_is_still_trickling_a_request() {
        let (_directory, mut server, _media) = server(64, Duration::from_secs(60), 1);
        let address = server.address();
        let mut peer = TcpStream::connect(address).expect("connect");
        peer.set_read_timeout(Some(Duration::from_secs(2)))
            .expect("timeout");
        peer.write_all(b"GET /stream/partial HTTP/1.1\r\n")
            .expect("write");
        thread::sleep(Duration::from_millis(200));
        let started = Instant::now();
        server.stop();
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "stop must not wait for the header budget, took {:?}",
            started.elapsed()
        );
        assert!(!server.is_running());
        assert_eq!(server.active_clients(), 0);
        let mut scratch = [0_u8; 16];
        let read = peer.read(&mut scratch);
        assert!(
            matches!(read, Ok(0)) || read.is_err(),
            "the serving thread must be gone too, got {read:?}"
        );
        assert!(
            TcpStream::connect_timeout(&address, Duration::from_millis(300)).is_err(),
            "the listener must be closed after stop"
        );
    }

    #[test]
    fn an_unspecified_bind_address_is_refused_before_a_socket_exists() {
        let (_directory, path, _media) = fixture(16);
        assert!(matches!(
            StreamServer::start(
                path,
                "0.0.0.0:0".parse().expect("bind"),
                Duration::from_secs(60),
                1
            ),
            Err(CastError::Stream(_))
        ));
    }

    #[test]
    fn dropping_the_server_closes_it_even_without_an_explicit_stop() {
        let (_directory, server, _media) = server(32, Duration::from_secs(60), 1);
        let address = server.address();
        let mut peer = TcpStream::connect(address).expect("connect");
        peer.set_read_timeout(Some(Duration::from_secs(2)))
            .expect("timeout");
        peer.write_all(b"GET /stream/partial HTTP/1.1\r\n")
            .expect("write");
        thread::sleep(Duration::from_millis(200));
        drop(server);
        let mut scratch = [0_u8; 16];
        let read = peer.read(&mut scratch);
        assert!(
            matches!(read, Ok(0)) || read.is_err(),
            "Drop must end the serving thread, got {read:?}"
        );
        assert!(
            TcpStream::connect_timeout(&address, Duration::from_millis(300)).is_err(),
            "Drop must close the listener"
        );
    }
}
