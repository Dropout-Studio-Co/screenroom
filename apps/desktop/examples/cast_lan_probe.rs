use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use screenroom_desktop::cast_session::{SessionRequest, connect};
use screenroom_desktop::cast_stream::{StreamServer, local_address_towards};
use screenroom_desktop::casting::{self, MediaFacts, Receiver};
use serde_json::{Value, json};

const ROLE: &str = "drive the real casting modules against the first-party test receiver";

struct Options {
    media: PathBuf,
    evidence: PathBuf,
    mode: String,
    ready: Option<PathBuf>,
    go: Option<PathBuf>,
    wait: Duration,
    expect: String,
    container: String,
    video: String,
    audio: String,
    title: String,
}

impl Options {
    fn parse() -> Result<Options, String> {
        let mut options = Options {
            media: PathBuf::new(),
            evidence: PathBuf::from("."),
            mode: "full".to_string(),
            ready: None,
            go: None,
            wait: Duration::from_secs(30),
            expect: "Screenroom Test Receiver".to_string(),
            container: "mp4".to_string(),
            video: "h264".to_string(),
            audio: "aac".to_string(),
            title: "Screenroom casting verification clip".to_string(),
        };
        let mut arguments = std::env::args().skip(1);
        while let Some(name) = arguments.next() {
            let mut value = || {
                arguments
                    .next()
                    .ok_or_else(|| format!("{name} needs a value"))
            };
            match name.as_str() {
                "--media" => options.media = PathBuf::from(value()?),
                "--evidence" => options.evidence = PathBuf::from(value()?),
                "--mode" => options.mode = value()?,
                "--ready" => options.ready = Some(PathBuf::from(value()?)),
                "--go" => options.go = Some(PathBuf::from(value()?)),
                "--wait" => {
                    let seconds = value()?
                        .parse::<u64>()
                        .map_err(|error| format!("--wait: {error}"))?;
                    options.wait = Duration::from_secs(seconds);
                }
                "--expect" => options.expect = value()?,
                "--container" => options.container = value()?,
                "--video" => options.video = value()?,
                "--audio" => options.audio = value()?,
                "--title" => options.title = value()?,
                other => return Err(format!("unknown option {other}")),
            }
        }
        if options.media.as_os_str().is_empty() {
            return Err("--media is required".to_string());
        }
        std::fs::create_dir_all(&options.evidence)
            .map_err(|error| format!("cannot create the evidence directory: {error}"))?;
        Ok(options)
    }

    fn facts(&self) -> MediaFacts {
        MediaFacts::video(&self.container, &self.video, &self.audio)
    }
}

struct Reply {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Reply {
    fn header(&self, name: &str) -> String {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.clone())
            .unwrap_or_default()
    }
}

fn split_url(url: &str) -> Result<(String, u16, String), String> {
    let rest = url
        .strip_prefix("http://")
        .ok_or_else(|| format!("{url} is not an http address"))?;
    let (authority, target) = match rest.find('/') {
        Some(index) => (&rest[..index], &rest[index..]),
        None => (rest, "/"),
    };
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) => (
            host.to_string(),
            port.parse::<u16>()
                .map_err(|error| format!("{url} has no usable port: {error}"))?,
        ),
        None => (authority.to_string(), 80),
    };
    Ok((host, port, target.to_string()))
}

fn get(url: &str, range: Option<&str>) -> Result<Reply, String> {
    let (host, port, target) = split_url(url)?;
    let address: SocketAddr = format!("{host}:{port}")
        .parse()
        .or_else(|_| {
            std::net::ToSocketAddrs::to_socket_addrs(&(host.as_str(), port))
                .map_err(|error| std::io::Error::other(error.to_string()))
                .and_then(|mut list| {
                    list.next()
                        .ok_or_else(|| std::io::Error::other("no address"))
                })
        })
        .map_err(|error| format!("cannot resolve {host}: {error}"))?;
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(5))
        .map_err(|error| format!("cannot reach {host}:{port}: {error}"))?;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(10)));
    let mut head = format!(
        "GET {target} HTTP/1.1\r\nHost: {host}:{port}\r\nAccept: */*\r\nConnection: close\r\n\
         User-Agent: ScreenroomCastProbe\r\n"
    );
    if let Some(range) = range {
        head.push_str(&format!("Range: {range}\r\n"));
    }
    head.push_str("\r\n");
    stream
        .write_all(head.as_bytes())
        .map_err(|error| format!("the endpoint dropped the request: {error}"))?;
    let mut raw = Vec::new();
    stream
        .read_to_end(&mut raw)
        .map_err(|error| format!("the endpoint stopped answering: {error}"))?;
    let split = raw
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(|| "the endpoint answered without headers".to_string())?;
    let text = String::from_utf8_lossy(&raw[..split]).into_owned();
    let mut lines = text.lines();
    let status = lines
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(|| "the endpoint answered without a status".to_string())?;
    let headers = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(key, value)| (key.trim().to_string(), value.trim().to_string()))
        .collect();
    Ok(Reply {
        status,
        headers,
        body: raw[split + 4..].to_vec(),
    })
}

fn soap_call(control_url: &str, action: &str, inner: &str) -> Result<(u16, String), String> {
    let (host, port, target) = split_url(control_url)?;
    let address: SocketAddr = format!("{host}:{port}")
        .parse()
        .map_err(|error| format!("{control_url} is not usable: {error}"))?;
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(5))
        .map_err(|error| format!("cannot reach {host}:{port}: {error}"))?;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(10)));
    let body = format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
         <s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\"><s:Body>\
         <u:{action} xmlns:u=\"{}\">{inner}</u:{action}></s:Body></s:Envelope>",
        casting::AV_TRANSPORT_SERVICE
    );
    let soap_action = format!("{}#{action}", casting::AV_TRANSPORT_SERVICE);
    let head = format!(
        "POST {target} HTTP/1.1\r\nHost: {host}:{port}\r\nSOAPACTION: \"{soap_action}\"\r\n\
         Content-Type: text/xml; charset=\"utf-8\"\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream
        .write_all(head.as_bytes())
        .map_err(|error| format!("the receiver dropped the request: {error}"))?;
    let mut raw = Vec::new();
    stream
        .read_to_end(&mut raw)
        .map_err(|error| format!("the receiver stopped answering: {error}"))?;
    let split = raw
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(|| "the receiver answered without headers".to_string())?;
    let head = String::from_utf8_lossy(&raw[..split]).into_owned();
    let status = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(|| "the receiver answered without a status".to_string())?;
    Ok((
        status,
        String::from_utf8_lossy(&raw[split + 4..]).into_owned(),
    ))
}

fn field_of(body: &str, name: &str) -> String {
    let open = format!("<{name}>");
    let close = format!("</{name}>");
    body.find(&open)
        .and_then(|start| {
            let rest = &body[start + open.len()..];
            rest.find(&close).map(|end| rest[..end].to_string())
        })
        .unwrap_or_default()
}

fn listeners_of(pid: u32) -> Vec<String> {
    let output = std::process::Command::new("/usr/sbin/lsof")
        .args(["-nP", "-a", "-p", &pid.to_string(), "-i"])
        .output();
    match output {
        Ok(output) => String::from_utf8_lossy(&output.stdout)
            .lines()
            .skip(1)
            .map(|line| line.trim().to_string())
            .filter(|line| !line.is_empty())
            .collect(),
        Err(error) => vec![format!("lsof could not run: {error}")],
    }
}

fn redact_address(address: &str) -> String {
    if let Ok(ip) = address.parse::<IpAddr>() {
        return match ip {
            IpAddr::V4(value) => {
                let octets = value.octets();
                format!("{}.{}.{}.x", octets[0], octets[1], octets[2])
            }
            IpAddr::V6(value) => {
                let segments = value.segments();
                format!("{}-{}-{}::x", segments[0], segments[1], segments[2])
            }
        };
    }
    address.to_string()
}

fn redact_url(url: &str) -> String {
    let (host, port, target) = match split_url(url) {
        Ok(parts) => parts,
        Err(_) => return "<unparsable address>".to_string(),
    };
    let mut pieces: Vec<&str> = target.split('/').collect();
    if pieces.len() > 2 && pieces[1] == "stream" {
        pieces[2] = "<redacted>";
        return format!(
            "http://{}:{}/{}",
            redact_address(&host),
            port,
            pieces[1..].join("/")
        );
    }
    format!("http://{}:{}{target}", redact_address(&host), port)
}

fn stream_token(url: &str) -> String {
    split_url(url)
        .ok()
        .and_then(|(_, _, target)| {
            let pieces: Vec<&str> = target.split('/').collect();
            (pieces.len() > 2 && pieces[1] == "stream").then(|| pieces[2].to_string())
        })
        .unwrap_or_default()
}

fn record(options: &Options, name: &str, payload: &Value) {
    let path = options.evidence.join(format!("probe-{name}.json"));
    let text = serde_json::to_string_pretty(payload).unwrap_or_else(|_| "{}".to_string());
    let _ = std::fs::write(path, format!("{text}\n"));
}

fn state_of(session: &screenroom_desktop::cast_session::CastSession) -> String {
    session.state().to_string()
}

fn main() {
    let outcome = run();
    match outcome {
        Ok(()) => {}
        Err(error) => {
            eprintln!("CAST PROBE FAILED: {error}");
            std::process::exit(1);
        }
    }
}

fn run() -> Result<(), String> {
    let options = Options::parse()?;
    match options.mode.as_str() {
        "full" => full(&options),
        "dropped" => dropped(&options),
        other => Err(format!("unknown mode {other}")),
    }
}

fn choose(options: &Options) -> Result<Receiver, String> {
    let found = casting::discover(Duration::from_secs(3), casting::MAX_RECEIVERS)?;
    let names: Vec<String> = found
        .iter()
        .map(|receiver| format!("{} <{}>", receiver.name, redact_url(&receiver.address)))
        .collect();
    println!("discovery found {} receiver(s): {names:?}", found.len());
    found
        .into_iter()
        .find(|receiver| receiver.name.contains(&options.expect))
        .ok_or_else(|| {
            format!(
                "no receiver named like {:?} answered; found {names:?}",
                options.expect
            )
        })
}

fn full(options: &Options) -> Result<(), String> {
    let pid = std::process::id();
    let started = Instant::now();
    let receiver = choose(options)?;
    let description = casting::describe(&receiver.address, Duration::from_secs(5))
        .map_err(|error| format!("the description could not be read: {error}"))?;
    let services: Vec<Value> = description
        .services
        .iter()
        .map(|service| {
            json!({
                "type": service.service_type,
                "control": redact_url(&service.control_url),
                "scpd": redact_url(&service.scpd_url),
            })
        })
        .collect();
    println!(
        "described {} with {} service(s)",
        description.friendly_name,
        services.len()
    );

    let remote_ip: IpAddr = receiver
        .from
        .parse()
        .map_err(|error| format!("the receiver address is not usable: {error}"))?;
    let remote = SocketAddr::new(remote_ip, 1900);
    let local = local_address_towards(remote)
        .map_err(|error| format!("no local address towards the receiver: {error}"))?;
    let mut direct = StreamServer::start(
        options.media.clone(),
        SocketAddr::new(local, 0),
        Duration::from_secs(120),
        1,
    )
    .map_err(|error| format!("the direct stream server refused to start: {error}"))?;
    let direct_url = direct.url(local);
    let served = direct.bytes();
    let partial = get(&direct_url, Some("bytes=0-"))?;
    let bare_root = get(&format!("http://{local}:{}/", direct.port()), None)?;
    let library = get(&format!("http://{local}:{}/library", direct.port()), None)?;
    let wrong = get(
        &format!("http://{local}:{}/stream/not-the-token", direct.port()),
        None,
    )?;
    let direct_listeners = listeners_of(pid);
    let direct_running = direct.is_running();
    direct.stop();
    let direct_after = listeners_of(pid);
    println!(
        "direct endpoint: {} bytes, range {}, root {}, library {}, wrong token {}",
        served, partial.status, bare_root.status, library.status, wrong.status
    );

    let report = casting::codec_report(&options.facts());
    let request = SessionRequest::new(
        receiver.clone(),
        options.media.clone(),
        options.facts(),
        options.title.clone(),
    );
    let mut session =
        connect(request).map_err(|error| format!("the session could not start: {error}"))?;
    let after_connect = state_of(&session);
    let stream_url = session.stream_url().to_string();
    let session_listeners = listeners_of(pid);
    println!("session started; the receiver reports {after_connect}");

    let session_partial = get(&stream_url, Some("bytes=0-"))?;
    let session_root = get(
        &format!("http://{local}:{}/", direct_port(&stream_url)?),
        None,
    )?;
    let session_library = get(
        &format!("http://{local}:{}/library", direct_port(&stream_url)?),
        None,
    )?;

    let after_pause = session
        .pause()
        .map_err(|error| format!("pause failed: {error}"))?;
    let after_seek = session
        .seek(Duration::from_secs(10))
        .map_err(|error| format!("seek failed: {error}"))?;
    let av_control = session
        .description()
        .av_transport()
        .map_err(|error| format!("the description lost AVTransport: {error}"))?
        .control_url
        .clone();
    let paused_position = soap_call(&av_control, "GetPositionInfo", "<InstanceID>0</InstanceID>")
        .map(|(status, body)| (status, field_of(&body, "RelTime")))
        .unwrap_or((0, String::new()));
    let after_resume = session
        .resume()
        .map_err(|error| format!("resume failed: {error}"))?;
    std::thread::sleep(Duration::from_millis(600));
    let resumed_position = soap_call(&av_control, "GetPositionInfo", "<InstanceID>0</InstanceID>")
        .map(|(status, body)| (status, field_of(&body, "RelTime")))
        .unwrap_or((0, String::new()));
    let after_stop = session
        .stop()
        .map_err(|error| format!("stop failed: {error}"))?;
    let streaming_after_stop = session.is_streaming();
    let stop_listeners = listeners_of(pid);
    let summary = session.summary();
    let title = session.title().to_string();
    session
        .disconnect()
        .map_err(|error| format!("disconnect failed: {error}"))?;

    let token = stream_token(&stream_url);
    if !token.is_empty() {
        let _ = std::fs::write(options.evidence.join("token.txt"), format!("{token}\n"));
    }

    let evidence = json!({
        "role": ROLE,
        "target": "first-party Screenroom test receiver on this LAN; never a third-party device",
        "pid": pid,
        "elapsed_seconds": started.elapsed().as_secs_f64().round(),
        "capability": {
            "text": casting::CAPABILITY_TEXT,
            "can_start_a_session": casting::capability().can_start_a_session,
            "verified_on_a_receiver": casting::capability().verified_on_a_receiver,
        },
        "discovery": {
            "search": "urn:schemas-upnp-org:device:MediaRenderer:1",
            "name": receiver.name,
            "kind": receiver.kind,
            "address": redact_url(&receiver.address),
            "answered_from": redact_address(&receiver.from),
        },
        "description": {
            "friendly_name": description.friendly_name,
            "services": services,
        },
        "codec": {
            "compatibility": format!("{:?}", report.compatibility),
            "reason": report.reason,
        },
        "direct_endpoint": {
            "file_bytes": served,
            "range_status": partial.status,
            "range_bytes": partial.body.len(),
            "content_range": partial.header("Content-Range"),
            "root_status": bare_root.status,
            "library_status": library.status,
            "wrong_token_status": wrong.status,
            "running_before_stop": direct_running,
            "listeners_before_stop": direct_listeners.clone(),
            "listeners_after_stop": direct_after.clone(),
        },
        "session": {
            "title": title,
            "stream_url": redact_url(&stream_url),
            "listeners_before_stop": session_listeners.clone(),
        },
        "session_endpoint": {
            "range_status": session_partial.status,
            "range_bytes": session_partial.body.len(),
            "content_range": session_partial.header("Content-Range"),
            "root_status": session_root.status,
            "library_status": session_library.status,
        },
        "states": {
            "after_connect": after_connect,
            "after_pause": after_pause.to_string(),
            "after_seek": after_seek.to_string(),
            "after_resume": after_resume.to_string(),
            "after_stop": after_stop.to_string(),
        },
        "receiver_position": {
            "after_seek_status": paused_position.0,
            "after_seek": paused_position.1,
            "after_resume_status": resumed_position.0,
            "after_resume": resumed_position.1,
        },
        "shutdown": {
            "streaming_after_stop": streaming_after_stop,
            "summary": summary,
            "listeners_after_stop": stop_listeners.clone(),
            "listener_closed": stop_listeners.is_empty(),
        },
    });
    record(options, "full", &evidence);
    println!(
        "states: connect={after_connect} pause={after_pause} seek={after_seek} resume={after_resume} stop={after_stop}"
    );
    println!("summary: {summary}");
    println!(
        "listeners while streaming: {} ; after stop: {}",
        session_listeners.clone().len(),
        stop_listeners.clone().len()
    );

    let mut failures = Vec::new();
    if after_connect != "playing" {
        failures.push(format!(
            "connect reported {after_connect} instead of playing"
        ));
    }
    if after_pause.to_string() != "paused" {
        failures.push(format!("pause read back {after_pause}"));
    }
    if after_seek.to_string() != "paused" {
        failures.push(format!("seek read back {after_seek}"));
    }
    if after_resume.to_string() != "playing" {
        failures.push(format!("resume read back {after_resume}"));
    }
    if after_stop.to_string() != "stopped" {
        failures.push(format!("stop read back {after_stop}"));
    }
    if partial.status != 206 || !direct_after.is_empty() {
        failures.push("the direct endpoint did not serve a 206 range or did not close".to_string());
    }
    if bare_root.status != 404 || library.status != 404 || wrong.status != 404 {
        failures.push("a path other than the token answered something other than 404".to_string());
    }
    if session_partial.status != 206 {
        failures.push(format!(
            "the session endpoint answered {} to a range request",
            session_partial.status
        ));
    }
    if session_root.status != 404 || session_library.status != 404 {
        failures.push("the session endpoint exposed a path other than the token".to_string());
    }
    if streaming_after_stop {
        failures.push("the session still streamed after stop".to_string());
    }
    if !stop_listeners.is_empty() {
        failures.push("the app still held a listener after stop".to_string());
    }
    if paused_position.0 != 200 || paused_position.1 != "00:00:10" {
        failures.push(format!(
            "the receiver reported position {} on HTTP {} after a seek to ten seconds",
            paused_position.1, paused_position.0
        ));
    }
    if resumed_position.0 != 200 {
        failures.push("the receiver could not report its position after resume".to_string());
    }
    if !failures.is_empty() {
        return Err(failures.join("; "));
    }
    Ok(())
}

fn direct_port(url: &str) -> Result<u16, String> {
    split_url(url).map(|(_, port, _)| port)
}

fn dropped(options: &Options) -> Result<(), String> {
    let pid = std::process::id();
    let receiver = choose(options)?;
    let request = SessionRequest::new(
        receiver.clone(),
        options.media.clone(),
        options.facts(),
        options.title.clone(),
    );
    let mut session =
        connect(request).map_err(|error| format!("the session could not start: {error}"))?;
    let after_connect = state_of(&session);
    let before = listeners_of(pid);
    println!("session started; the receiver reports {after_connect}; waiting for the drop");
    if let Some(ready) = &options.ready {
        std::fs::write(ready, format!("{after_connect}\n"))
            .map_err(|error| format!("cannot write the ready marker: {error}"))?;
    }
    if let Some(go) = &options.go {
        let deadline = Instant::now() + options.wait;
        while !go.exists() {
            if Instant::now() >= deadline {
                return Err(format!("no drop signal arrived within {:?}", options.wait));
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    } else {
        std::thread::sleep(options.wait);
    }
    let outcome = session.refresh();
    let reported = match &outcome {
        Ok(state) => format!("the receiver still answered {state}"),
        Err(error) => error.to_string(),
    };
    let state_after = state_of(&session);
    let streaming_after = session.is_streaming();
    let after = listeners_of(pid);
    let evidence = json!({
        "role": ROLE,
        "target": "first-party Screenroom test receiver on this LAN; never a third-party device",
        "pid": pid,
        "discovery": {
            "name": receiver.name,
            "kind": receiver.kind,
            "address": redact_url(&receiver.address),
        },
        "states": {
            "after_connect": after_connect,
            "after_drop": state_after,
        },
        "drop": {
            "refresh_ok": outcome.is_ok(),
            "reported": reported,
            "streaming_after_drop": streaming_after,
            "listeners_before_drop": before.clone(),
            "listeners_after_drop": after.clone(),
            "listener_closed": after.is_empty(),
        },
        "capability": {
            "text": casting::CAPABILITY_TEXT,
        },
    });
    record(options, "dropped", &evidence);
    println!("after the drop the app reported: {reported}");
    println!(
        "streaming after drop: {streaming_after}; listeners after drop: {}",
        after.clone().len()
    );
    if outcome.is_ok() {
        return Err(
            "the app kept reporting a healthy session after the receiver disappeared".to_string(),
        );
    }
    if streaming_after {
        return Err("the stream endpoint stayed open after the receiver disappeared".to_string());
    }
    if !after.is_empty() {
        return Err("the app still held a listener after the receiver disappeared".to_string());
    }
    Ok(())
}
