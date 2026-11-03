use std::fmt;
use std::io::{BufReader, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::time::Duration;

use rustls::pki_types::ServerName;
use rustls::{ClientConfig, ClientConnection, RootCertStore, StreamOwned};

use super::provider_http::{
    DEFAULT_CONNECT_TIMEOUT, Deadline, HttpMethod, HttpRequest, HttpResponse, HttpTarget,
    HttpTransport, MAX_TRANSFER_BYTES, TcpTransport, TransportError, parse_target_with,
    read_response, url_scheme,
};

pub const MAX_REDIRECTS: usize = 3;

const HTTPS_PORT: u16 = 443;

pub struct HttpsTransport {
    connect_timeout: Duration,
    config: Arc<ClientConfig>,
    plaintext: TcpTransport,
}

impl fmt::Debug for HttpsTransport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HttpsTransport")
            .field("connect_timeout", &self.connect_timeout)
            .field("max_redirects", &MAX_REDIRECTS)
            .finish_non_exhaustive()
    }
}

impl Default for HttpsTransport {
    fn default() -> Self {
        Self::new(DEFAULT_CONNECT_TIMEOUT)
    }
}

impl HttpsTransport {
    pub fn new(connect_timeout: Duration) -> Self {
        let mut roots = RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        Self::from_roots(connect_timeout, roots)
    }

    #[cfg(test)]
    fn trusting(
        connect_timeout: Duration,
        certificate: rustls::pki_types::CertificateDer<'static>,
    ) -> Self {
        let mut roots = RootCertStore::empty();
        roots.add(certificate).expect("a usable test anchor");
        Self::from_roots(connect_timeout, roots)
    }

    fn from_roots(connect_timeout: Duration, roots: RootCertStore) -> Self {
        let config = ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        Self {
            connect_timeout,
            config: Arc::new(config),
            plaintext: TcpTransport::new(connect_timeout),
        }
    }

    fn connect(
        &self,
        target: &HttpTarget,
        deadline: &Deadline,
    ) -> Result<TcpStream, TransportError> {
        let mut failure: Option<std::io::Error> = None;
        for address in (target.host.as_str(), target.port).to_socket_addrs()? {
            let Some(remaining) = deadline.remaining() else {
                return Err(expired());
            };
            let budget = self.connect_timeout.min(remaining);
            match TcpStream::connect_timeout(&address, budget) {
                Ok(socket) => {
                    let _ = socket.set_nodelay(true);
                    return Ok(socket);
                }
                Err(error) => failure = Some(error),
            }
        }
        Err(match failure {
            Some(error) => TransportError::Io(error),
            None => TransportError::InvalidUrl,
        })
    }

    fn exchange(
        &self,
        url: &str,
        request: &HttpRequest,
        deadline: &Deadline,
    ) -> Result<HttpResponse, TransportError> {
        let target = parse_target_with(url, "https", HTTPS_PORT)?;
        let limit = request.max_bytes.min(MAX_TRANSFER_BYTES);
        let socket = self.connect(&target, deadline)?;
        let control = socket.try_clone()?;
        let name =
            ServerName::try_from(target.host.clone()).map_err(|_| TransportError::InvalidUrl)?;
        let connection = ClientConnection::new(self.config.clone(), name).map_err(|_| {
            TransportError::InvalidRequest("the secure connection could not be prepared")
        })?;
        let mut stream =
            DeadlineStream::new(StreamOwned::new(connection, socket), control, deadline);

        let mut head = format!(
            "{} {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nAccept-Encoding: identity\r\n",
            request.method.as_str(),
            target.path,
            authority_of(&target),
        );
        for (name, value) in &request.headers {
            head.push_str(&format!("{name}: {value}\r\n"));
        }
        if !request.body.is_empty() {
            head.push_str(&format!("Content-Length: {}\r\n", request.body.len()));
        }
        head.push_str("\r\n");
        stream.write_all(head.as_bytes())?;
        if !request.body.is_empty() {
            stream.write_all(&request.body)?;
        }
        stream.flush()?;

        let mut reader = BufReader::new(stream);
        read_response(&mut reader, limit)
    }
}

impl HttpTransport for HttpsTransport {
    fn send(&self, request: &HttpRequest) -> Result<HttpResponse, TransportError> {
        request.validate()?;
        if !url_scheme(&request.url).is_some_and(|scheme| scheme == "https") {
            return self.plaintext.send(request);
        }
        let start = parse_target_with(&request.url, "https", HTTPS_PORT)?;
        let deadline = Deadline::after(request.timeout);
        let mut url = request.url.clone();
        let mut followed = 0_usize;
        loop {
            let hop = request_for_hop(request, &start.host, &url)?;
            let response = self
                .exchange(&url, &hop, &deadline)
                .map_err(classify)
                .map_err(|error| error.into_timeout(request.timeout))?;
            if !is_redirect(response.status) || request.method != HttpMethod::Get {
                return Ok(response);
            }
            let Some(location) = response.header("location").map(str::to_string) else {
                return Ok(response);
            };
            if followed == MAX_REDIRECTS {
                return Err(TransportError::InvalidRequest(
                    "the answer redirected more often than the transport follows",
                ));
            }
            url = redirect_target(&url, &location, &request.redirect_hosts)?;
            followed += 1;
        }
    }

    fn supports_scheme(&self, scheme: &str) -> bool {
        scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https")
    }
}

struct DeadlineStream<'a, S> {
    inner: S,
    socket: TcpStream,
    deadline: &'a Deadline,
}

impl<'a, S> DeadlineStream<'a, S> {
    fn new(inner: S, socket: TcpStream, deadline: &'a Deadline) -> Self {
        Self {
            inner,
            socket,
            deadline,
        }
    }

    fn arm(&self) -> std::io::Result<()> {
        let Some(remaining) = self.deadline.remaining() else {
            return Err(deadline_passed());
        };
        let remaining = remaining.max(Duration::from_millis(1));
        self.socket.set_read_timeout(Some(remaining))?;
        self.socket.set_write_timeout(Some(remaining))
    }
}

impl<S: Read> Read for DeadlineStream<'_, S> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        self.arm()?;
        self.inner.read(buffer)
    }
}

impl<S: Write> Write for DeadlineStream<'_, S> {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.arm()?;
        self.inner.write(buffer)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.arm()?;
        self.inner.flush()
    }
}

fn deadline_passed() -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::TimedOut, "the request deadline passed")
}

fn expired() -> TransportError {
    TransportError::Io(deadline_passed())
}

fn classify(error: TransportError) -> TransportError {
    match error {
        TransportError::Io(io) => {
            match io
                .get_ref()
                .and_then(|inner| inner.downcast_ref::<rustls::Error>())
            {
                Some(rustls::Error::InvalidCertificate(_)) => TransportError::CertificateNotTrusted,
                _ => TransportError::Io(io),
            }
        }
        other => other,
    }
}

fn is_redirect(status: u16) -> bool {
    matches!(status, 301 | 302 | 303 | 307 | 308)
}

fn redirect_target(
    current: &str,
    location: &str,
    allow_hosts: &[String],
) -> Result<String, TransportError> {
    let location = location.trim();
    if location.is_empty() || location.chars().any(char::is_control) {
        return Err(TransportError::InvalidRequest(
            "the answer offered a redirect that cannot be followed",
        ));
    }
    let target = if location.starts_with("https://") {
        location.to_string()
    } else if location.starts_with('/') {
        format!("{}{location}", origin_of(current)?)
    } else if location.starts_with("http://") {
        return Err(TransportError::InvalidRequest(
            "the answer redirected to a plain http address, which is refused",
        ));
    } else {
        return Err(TransportError::InvalidRequest(
            "the answer offered a redirect that cannot be followed",
        ));
    };
    let target = target.split('#').next().unwrap_or(&target).to_string();
    let current = parse_target_with(current, "https", HTTPS_PORT)?;
    let next = parse_target_with(&target, "https", HTTPS_PORT)?;
    if current.host.eq_ignore_ascii_case(&next.host) && current.port == next.port {
        return Ok(target);
    }
    if redirect_host_allowed(&next.host, allow_hosts) {
        return Ok(target);
    }
    Err(TransportError::InvalidRequest(
        "the answer redirected to a different host, which the transport does not follow",
    ))
}

fn redirect_host_allowed(host: &str, allow_hosts: &[String]) -> bool {
    let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
    if host.is_empty() {
        return false;
    }
    allow_hosts.iter().any(|entry| {
        let entry = entry.trim().trim_end_matches('.').to_ascii_lowercase();
        !entry.is_empty() && (host == entry || host.ends_with(&format!(".{entry}")))
    })
}

fn request_for_hop(
    request: &HttpRequest,
    start_host: &str,
    url: &str,
) -> Result<HttpRequest, TransportError> {
    let target = parse_target_with(url, "https", HTTPS_PORT)?;
    if target.host.eq_ignore_ascii_case(start_host) {
        return Ok(request.clone());
    }
    let mut hop = request.clone();
    hop.headers
        .retain(|(name, _)| !name.eq_ignore_ascii_case("authorization"));
    Ok(hop)
}

fn origin_of(url: &str) -> Result<String, TransportError> {
    let (_, rest) = url.split_once("://").ok_or(TransportError::InvalidUrl)?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.is_empty() {
        return Err(TransportError::InvalidUrl);
    }
    Ok(format!("https://{authority}"))
}

fn authority_of(target: &HttpTarget) -> String {
    if target.port == HTTPS_PORT {
        target.host.clone()
    } else {
        format!("{}:{}", target.host, target.port)
    }
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, Write};
    use std::net::TcpListener;
    use std::sync::Mutex;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Instant;

    use rcgen::{CertifiedKey, generate_simple_self_signed};
    use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
    use rustls::{ServerConfig, ServerConnection};

    use super::*;

    struct TestServer {
        base_url: String,
        certificate: CertificateDer<'static>,
        heads: Arc<Mutex<Vec<String>>>,
        handle: thread::JoinHandle<()>,
    }

    impl TestServer {
        fn heads(&self) -> Vec<String> {
            self.heads.lock().expect("the head log").clone()
        }
    }

    fn response(status: &str, headers: &[(&str, &str)], body: &[u8]) -> Vec<u8> {
        let mut bytes = format!("HTTP/1.1 {status}\r\n").into_bytes();
        for (name, value) in headers {
            bytes.extend_from_slice(format!("{name}: {value}\r\n").as_bytes());
        }
        bytes.extend_from_slice(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes());
        bytes.extend_from_slice(body);
        bytes
    }

    fn start_server(replies: Vec<Vec<u8>>) -> TestServer {
        start_server_named(vec!["127.0.0.1".to_string()], replies)
    }

    fn start_server_named(names: Vec<String>, replies: Vec<Vec<u8>>) -> TestServer {
        let CertifiedKey { cert, signing_key } =
            generate_simple_self_signed(names).expect("a self-signed certificate");
        start_secured(
            &cert.der().clone(),
            &signing_key.serialize_der(),
            "127.0.0.1",
            "127.0.0.1",
            replies,
        )
    }

    fn start_secured(
        certificate: &CertificateDer<'static>,
        signing_key_der: &[u8],
        bind: &str,
        name: &str,
        replies: Vec<Vec<u8>>,
    ) -> TestServer {
        let certificate = certificate.clone();
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(signing_key_der.to_vec()));
        let config = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![certificate.clone()], key)
            .expect("a server configuration");
        let listener = TcpListener::bind((bind, 0)).expect("a loopback listener");
        listener
            .set_nonblocking(true)
            .expect("a non-blocking listener");
        let address = listener.local_addr().expect("a bound address");
        let heads = Arc::new(Mutex::new(Vec::new()));
        let recorded = heads.clone();
        let handle = thread::spawn(move || {
            let give_up = Instant::now() + Duration::from_secs(5);
            for reply in replies {
                let accepted = loop {
                    match listener.accept() {
                        Ok((socket, _)) => break socket,
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            if Instant::now() >= give_up {
                                return;
                            }
                            thread::sleep(Duration::from_millis(5));
                        }
                        Err(_) => return,
                    }
                };
                let _ = accepted.set_nonblocking(false);
                let Ok(connection) = ServerConnection::new(Arc::new(config.clone())) else {
                    continue;
                };
                let mut stream = StreamOwned::new(connection, accepted);
                recorded
                    .lock()
                    .expect("the head log")
                    .push(read_head(&mut stream));
                let _ = stream.write_all(&reply);
                let _ = stream.flush();
                stream.conn.send_close_notify();
                let _ = stream.flush();
            }
        });
        TestServer {
            base_url: format!("https://{name}:{}", address.port()),
            certificate,
            heads,
            handle,
        }
    }

    fn read_head(stream: &mut StreamOwned<ServerConnection, TcpStream>) -> String {
        let mut head = String::new();
        let mut reader = BufReader::new(stream);
        loop {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) | Err(_) => return head,
                Ok(_) if line == "\r\n" || line == "\n" => return head,
                Ok(_) => head.push_str(&line),
            }
        }
    }

    fn trusting(server: &TestServer) -> HttpsTransport {
        HttpsTransport::trusting(DEFAULT_CONNECT_TIMEOUT, server.certificate.clone())
    }

    #[test]
    fn a_secure_answer_arrives_with_its_body() {
        let server = start_server(vec![response(
            "200 OK",
            &[("Content-Type", "application/json")],
            b"{\"id\":55001}",
        )]);
        let answer = trusting(&server)
            .send(&HttpRequest::get(format!(
                "{}/movie/55001",
                server.base_url
            )))
            .expect("a secure answer");
        assert_eq!(answer.status, 200);
        assert_eq!(answer.body, b"{\"id\":55001}");
        assert_eq!(answer.header("content-type"), Some("application/json"));
        server.handle.join().expect("the server thread");
    }

    #[test]
    fn a_secure_missing_entry_is_reported_as_its_status() {
        let server = start_server(vec![response("404 Not Found", &[], b"missing")]);
        let answer = trusting(&server)
            .send(&HttpRequest::get(format!("{}/movie/0", server.base_url)))
            .expect("a secure answer");
        assert_eq!(answer.status, 404);
        assert_eq!(answer.body, b"missing");
        server.handle.join().expect("the server thread");
    }

    #[test]
    fn a_declared_body_over_the_budget_is_refused_before_it_is_read() {
        let body = vec![b'a'; 4_096];
        let server = start_server(vec![response("200 OK", &[], &body)]);
        let error = trusting(&server)
            .send(&HttpRequest::get(server.base_url.clone()).with_max_bytes(1_024))
            .expect_err("a body over the budget must be refused");
        assert!(
            matches!(error, TransportError::TooLarge { limit: 1_024 }),
            "{error:?}"
        );
        server.handle.join().expect("the server thread");
    }

    #[test]
    fn an_undelimited_body_over_the_budget_is_refused_while_it_streams() {
        let mut bytes = b"HTTP/1.1 200 OK\r\n\r\n".to_vec();
        bytes.extend(std::iter::repeat_n(b'a', 4_096));
        let server = start_server(vec![bytes]);
        let error = trusting(&server)
            .send(&HttpRequest::get(server.base_url.clone()).with_max_bytes(1_024))
            .expect_err("a streamed body over the budget must be refused");
        assert!(
            matches!(error, TransportError::TooLarge { limit: 1_024 }),
            "{error:?}"
        );
        server.handle.join().expect("the server thread");
    }

    #[test]
    fn a_server_that_never_answers_is_cut_off_by_the_deadline() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback listener");
        let address = listener.local_addr().expect("a bound address");
        let (finished_tx, finished_rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            if let Ok((socket, _)) = listener.accept() {
                let _ = finished_rx.recv_timeout(Duration::from_secs(10));
                drop(socket);
            }
        });
        let mut request = HttpRequest::get(format!("https://{address}/silence"));
        request.timeout = Duration::from_millis(300);
        let started = Instant::now();
        let error = HttpsTransport::new(DEFAULT_CONNECT_TIMEOUT)
            .send(&request)
            .expect_err("a silent server must not hold the caller open");
        let elapsed = started.elapsed();
        assert!(matches!(error, TransportError::Timeout { .. }), "{error:?}");
        assert!(elapsed < Duration::from_secs(5), "{elapsed:?}");
        let _ = finished_tx.send(());
        handle.join().expect("the server thread");
    }

    #[test]
    fn a_redirect_to_a_plain_address_is_refused() {
        let server = start_server(vec![response(
            "302 Found",
            &[("Location", "http://127.0.0.1:9/plain.srt")],
            b"",
        )]);
        let error = trusting(&server)
            .send(&HttpRequest::get(server.base_url.clone()))
            .expect_err("a downgrade must be refused");
        match error {
            TransportError::InvalidRequest(reason) => {
                assert!(reason.contains("plain http"), "{reason}");
            }
            other => panic!("{other:?}"),
        }
        server.handle.join().expect("the server thread");
    }

    #[test]
    fn a_redirect_to_another_host_is_refused() {
        let server = start_server(vec![response(
            "302 Found",
            &[("Location", "https://elsewhere.invalid/plain.srt")],
            b"",
        )]);
        let error = trusting(&server)
            .send(&HttpRequest::get(server.base_url.clone()))
            .expect_err("a cross-host redirect must be refused");
        match error {
            TransportError::InvalidRequest(reason) => {
                assert!(reason.contains("different host"), "{reason}");
            }
            other => panic!("{other:?}"),
        }
        server.handle.join().expect("the server thread");
    }

    #[test]
    fn a_same_host_redirect_is_followed_and_then_answered() {
        let server = start_server(vec![
            response("302 Found", &[("Location", "/final.srt")], b""),
            response(
                "200 OK",
                &[("Content-Type", "application/x-subrip")],
                b"1\n00:00:01,000 --> 00:00:02,000\nhello\n",
            ),
        ]);
        let answer = trusting(&server)
            .send(&HttpRequest::get(format!("{}/start.srt", server.base_url)))
            .expect("a followed redirect");
        assert_eq!(answer.status, 200);
        assert_eq!(answer.body, b"1\n00:00:01,000 --> 00:00:02,000\nhello\n");
        server.handle.join().expect("the server thread");
    }

    #[test]
    fn an_allowlisted_cross_host_redirect_is_followed_without_the_authorization_header() {
        let CertifiedKey { cert, signing_key } =
            generate_simple_self_signed(vec!["127.0.0.1".to_string(), "localhost".to_string()])
                .expect("a self-signed certificate");
        let certificate: CertificateDer<'static> = cert.der().clone();
        let key = signing_key.serialize_der();
        let second = start_secured(
            &certificate,
            &key,
            "127.0.0.1",
            "localhost",
            vec![response(
                "200 OK",
                &[("Content-Type", "application/x-subrip")],
                b"1\n00:00:01,000 --> 00:00:02,000\nhello\n",
            )],
        );
        let location = format!("{}/files/1", second.base_url);
        let first = start_secured(
            &certificate,
            &key,
            "127.0.0.1",
            "127.0.0.1",
            vec![response("302 Found", &[("Location", &location)], b"")],
        );
        let request = HttpRequest::get(format!("{}/get/1", first.base_url))
            .with_header("Authorization", "Bearer unit-test-key")
            .with_redirect_hosts(["localhost"]);
        let answer = trusting(&first)
            .send(&request)
            .expect("an allowlisted redirect is followed");
        assert_eq!(answer.status, 200);
        assert_eq!(answer.body, b"1\n00:00:01,000 --> 00:00:02,000\nhello\n");
        let first_head = first.heads().remove(0);
        assert!(
            first_head
                .to_ascii_lowercase()
                .contains("authorization: bearer unit-test-key"),
            "{first_head}"
        );
        let second_head = second.heads().remove(0);
        assert!(
            !second_head.to_ascii_lowercase().contains("authorization"),
            "{second_head}"
        );
        second.handle.join().expect("the server thread");
        first.handle.join().expect("the server thread");
    }

    #[test]
    fn a_cross_host_redirect_without_an_allowlist_entry_is_refused() {
        let CertifiedKey { cert, signing_key } =
            generate_simple_self_signed(vec!["127.0.0.1".to_string(), "localhost".to_string()])
                .expect("a self-signed certificate");
        let certificate: CertificateDer<'static> = cert.der().clone();
        let key = signing_key.serialize_der();
        let second = start_secured(&certificate, &key, "127.0.0.1", "localhost", Vec::new());
        let location = format!("{}/files/1", second.base_url);
        let first = start_secured(
            &certificate,
            &key,
            "127.0.0.1",
            "127.0.0.1",
            vec![response("302 Found", &[("Location", &location)], b"")],
        );
        let error = trusting(&first)
            .send(&HttpRequest::get(format!("{}/get/1", first.base_url)))
            .expect_err("a cross-host redirect needs an allowlist entry");
        match error {
            TransportError::InvalidRequest(reason) => {
                assert!(reason.contains("different host"), "{reason}");
            }
            other => panic!("{other:?}"),
        }
        first.handle.join().expect("the server thread");
        second.handle.join().expect("the server thread");
    }

    #[test]
    fn an_allowlist_entry_covers_its_subdomains_but_not_a_lookalike_name() {
        let allow = vec!["thesubtitledb.org".to_string()];
        assert!(redirect_host_allowed("thesubtitledb.org", &allow));
        assert!(redirect_host_allowed("files.thesubtitledb.org", &allow));
        assert!(redirect_host_allowed("FILES.TheSubtitleDB.org.", &allow));
        assert!(!redirect_host_allowed("notthedbsubtitledb.org", &allow));
        assert!(!redirect_host_allowed("thesubtitledb.org.invalid", &allow));
        assert!(!redirect_host_allowed("", &allow));
    }

    #[test]
    fn a_redirect_loop_stops_at_the_configured_limit() {
        let replies = (0..MAX_REDIRECTS + 1)
            .map(|_| response("302 Found", &[("Location", "/again")], b""))
            .collect();
        let server = start_server(replies);
        let error = trusting(&server)
            .send(&HttpRequest::get(server.base_url.clone()))
            .expect_err("an endless redirect must be refused");
        match error {
            TransportError::InvalidRequest(reason) => {
                assert!(reason.contains("redirected more often"), "{reason}");
            }
            other => panic!("{other:?}"),
        }
        server.handle.join().expect("the server thread");
    }

    #[test]
    fn an_untrusted_certificate_is_refused() {
        let server = start_server(vec![response("200 OK", &[], b"never")]);
        let error = HttpsTransport::default()
            .send(&HttpRequest::get(server.base_url.clone()))
            .expect_err("a self-signed certificate must not be trusted");
        assert!(
            matches!(error, TransportError::CertificateNotTrusted),
            "{error:?}"
        );
        assert!(error.to_string().contains("certificate"), "{error}");
        server.handle.join().expect("the server thread");
    }

    #[test]
    fn a_trusted_certificate_for_another_name_is_refused() {
        let server = start_server_named(
            vec!["elsewhere.invalid".to_string()],
            vec![response("200 OK", &[], b"never")],
        );
        let error = trusting(&server)
            .send(&HttpRequest::get(server.base_url.clone()))
            .expect_err("a certificate for another name must not be trusted");
        assert!(
            matches!(error, TransportError::CertificateNotTrusted),
            "{error:?}"
        );
        server.handle.join().expect("the server thread");
    }

    #[test]
    fn a_plain_http_address_is_carried_by_the_plaintext_path() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback listener");
        let address = listener.local_addr().expect("a bound address");
        let handle = thread::spawn(move || {
            if let Ok((mut socket, _)) = listener.accept() {
                let mut request = [0_u8; 2_048];
                let _ = socket.read(&mut request);
                let _ = socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok");
                let _ = socket.flush();
            }
        });
        let answer = HttpsTransport::default()
            .send(&HttpRequest::get(format!("http://{address}/plain")))
            .expect("a plain answer");
        assert_eq!(answer.status, 200);
        assert_eq!(answer.body, b"ok");
        handle.join().expect("the server thread");
    }

    #[test]
    fn the_transport_reports_the_schemes_it_can_carry() {
        let transport = HttpsTransport::default();
        assert!(transport.supports_scheme("https"));
        assert!(transport.supports_scheme("HTTPS"));
        assert!(transport.supports_scheme("http"));
        assert!(!transport.supports_scheme("ftp"));
    }
}
