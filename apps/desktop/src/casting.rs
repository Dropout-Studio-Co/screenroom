use std::net::{SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

pub const MAX_RECEIVERS: usize = 12;

pub const MAX_PROBE: Duration = Duration::from_secs(3);

pub const SEARCH_TARGET: &str = "urn:schemas-upnp-org:device:MediaRenderer:1";

const MULTICAST_ADDRESS: &str = "239.255.255.250:1900";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Receiver {
    pub id: String,

    pub name: String,

    pub kind: String,

    pub address: String,

    pub from: String,
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

pub fn parse_response(text: &str, from: SocketAddr) -> Option<Receiver> {
    if text.len() > 8_192 || !text.starts_with("HTTP/1.1 200") {
        return None;
    }
    let target = header(text, "ST").or_else(|| header(text, "NT"))?;
    if !target.contains("MediaRenderer") {
        return None;
    }
    let location = header(text, "LOCATION")?;
    if !(location.starts_with("http://") || location.starts_with("https://")) {
        return None;
    }
    if location.len() > 2_048 {
        return None;
    }
    let id = header(text, "USN")
        .or_else(|| header(text, "UUID"))
        .map(|value| clamp(value, 120))
        .unwrap_or_else(|| clamp(location, 120));
    let name = header(text, "FRIENDLYNAME")
        .or_else(|| header(text, "SERVER"))
        .map(|value| clamp(value, 80))
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| format!("A device at {}", from.ip()));
    Some(Receiver {
        id,
        name,
        kind: "DLNA MediaRenderer".to_string(),
        address: clamp(location, 2_048),
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
        .map_err(|error| format!("Cannot send the discovery request: {error}"))?;

    let deadline = Instant::now() + timeout.min(MAX_PROBE);
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
    Ok(found)
}

pub fn summary(count: usize) -> String {
    match count {
        0 => "No receiver answered. Discovery is not the same as casting, and nothing was sent."
            .to_string(),
        1 => "One receiver answered. That is discovery only: this build cannot cast to it yet, \
              and no stream has been sent."
            .to_string(),
        count => format!(
            "{count} receivers answered. That is discovery only: this build cannot cast to them \
             yet, and no stream has been sent."
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RESPONSE: &str = "HTTP/1.1 200 OK\r\nCACHE-CONTROL: max-age=1800\r\nST: urn:schemas-upnp-org:device:MediaRenderer:1\r\nUSN: uuid:1111::urn:schemas-upnp-org:device:MediaRenderer:1\r\nLOCATION: http://192.168.1.20:9197/dmr\r\nSERVER: Linux/4.0 UPnP/1.0 TestRenderer/1.0\r\n\r\n";

    fn from() -> SocketAddr {
        "192.168.1.20:1900".parse().expect("addr")
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
}
