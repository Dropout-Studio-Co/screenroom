use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::{Duration, Instant};

use screenroom_core::{HttpRequest, HttpTransport, TcpTransport, TransportError};

fn spawn_server(
    reply: impl FnOnce(&mut TcpStream) + Send + 'static,
) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("loopback listener");
    let address = listener.local_addr().expect("bound address");
    let handle = thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut request = [0_u8; 2_048];
            let _ = stream.read(&mut request);
            reply(&mut stream);
            let _ = stream.flush();
        }
    });
    (format!("http://{address}/subtitles.srt"), handle)
}

fn reply(stream: &mut TcpStream, bytes: &str) {
    stream
        .write_all(bytes.as_bytes())
        .expect("the client is still reading");
}

#[test]
fn a_sixteen_digit_chunk_size_is_refused_rather_than_overflowing() {
    for size in ["FFFFFFFFFFFFFFFF", "FFFFFFFFFFFFFFFE", "7FFFFFFFFFFFFFFF"] {
        let (url, handle) = spawn_server(move |stream| {
            reply(
                stream,
                &format!("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n{size}\r\n"),
            );
        });
        let error = TcpTransport::default()
            .send(&HttpRequest::get(url))
            .expect_err("a hostile chunk size must be refused");
        assert!(
            matches!(error, TransportError::TooLarge { .. }),
            "{size} produced {error:?}"
        );
        handle.join().expect("server thread");
    }
}

#[test]
fn a_hostile_chunk_size_after_a_read_body_is_refused_too() {
    let (url, handle) = spawn_server(|stream| {
        reply(
            stream,
            "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\nFFFFFFFFFFFFFFFF\r\n",
        );
    });
    let mut request = HttpRequest::get(url);
    request.max_bytes = 1_024;
    let error = TcpTransport::default()
        .send(&request)
        .expect_err("a hostile chunk size must be refused once bytes are buffered");
    assert!(
        matches!(error, TransportError::TooLarge { limit: 1_024 }),
        "{error:?}"
    );
    handle.join().expect("server thread");
}

#[test]
fn a_server_that_dribbles_bytes_past_the_deadline_is_abandoned_in_bounded_time() {
    let (url, handle) = spawn_server(|stream| {
        reply(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\r\n",
        );
        while stream.write_all(b"a").is_ok() {
            thread::sleep(Duration::from_millis(40));
        }
    });
    let mut request = HttpRequest::get(url);
    request.timeout = Duration::from_millis(300);
    let started = Instant::now();
    let error = TcpTransport::default()
        .send(&request)
        .expect_err("a dribbling server must not hold the caller open");
    let elapsed = started.elapsed();
    assert!(matches!(error, TransportError::Timeout { .. }), "{error:?}");
    assert!(elapsed < Duration::from_secs(3), "{elapsed:?}");
    handle.join().expect("server thread");
}

#[test]
fn an_endless_chunked_trailer_is_refused_rather_than_followed() {
    let (url, handle) = spawn_server(|stream| {
        reply(
            stream,
            "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n",
        );
        while stream.write_all(b"X-Trailer: endless\r\n").is_ok() {}
    });
    let mut request = HttpRequest::get(url);
    request.timeout = Duration::from_millis(300);
    let started = Instant::now();
    let error = TcpTransport::default()
        .send(&request)
        .expect_err("an endless trailer must be refused");
    let elapsed = started.elapsed();
    assert!(
        matches!(error, TransportError::InvalidRequest(_)),
        "{error:?}"
    );
    assert!(elapsed < Duration::from_secs(3), "{elapsed:?}");
    handle.join().expect("server thread");
}

#[test]
fn a_well_formed_chunked_answer_still_reads_in_full() {
    let (url, handle) = spawn_server(|stream| {
        reply(
            stream,
            "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n6\r\n world\r\n0\r\nX-Checksum: none\r\n\r\n",
        );
    });
    let response = TcpTransport::default()
        .send(&HttpRequest::get(url))
        .expect("a normal chunked answer");
    assert_eq!(response.status, 200);
    assert_eq!(response.body, b"hello world");
    assert!(response.header("x-checksum").is_none());
    handle.join().expect("server thread");
}
