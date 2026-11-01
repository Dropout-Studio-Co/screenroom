use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;

use screenroom_core::{
    Clock, DownloadedSubtitle, HttpMethod, HttpRequest, HttpResponse, HttpTransport, Ingest,
    Library, LibraryQuery, MediaKind, OpenSubtitlesProvider, SubtitleProvider,
    SubtitleProviderConfig, SubtitleSearch, TcpTransport, TransportError, parse_filename,
    subtitle_language_codes, subtitle_language_name,
};
use tempfile::TempDir;

enum Reply {
    Answer(HttpResponse),
    Fail(TransportError),
}

struct Shared {
    replies: RefCell<VecDeque<Reply>>,
    requests: RefCell<Vec<HttpRequest>>,
}

impl Shared {
    fn new(replies: Vec<Reply>) -> Rc<Self> {
        Rc::new(Self {
            replies: RefCell::new(replies.into()),
            requests: RefCell::new(Vec::new()),
        })
    }

    fn request_count(&self) -> usize {
        self.requests.borrow().len()
    }

    fn last_request(&self) -> HttpRequest {
        self.requests
            .borrow()
            .last()
            .expect("the provider made no request")
            .clone()
    }

    fn requests(&self) -> Vec<HttpRequest> {
        self.requests.borrow().clone()
    }
}

struct FakeTransport(Rc<Shared>);

impl HttpTransport for FakeTransport {
    fn send(&self, request: &HttpRequest) -> Result<HttpResponse, TransportError> {
        let shared = &self.0;
        shared.requests.borrow_mut().push(request.clone());
        match shared.replies.borrow_mut().pop_front() {
            Some(Reply::Answer(response)) => Ok(response),
            Some(Reply::Fail(error)) => Err(error),
            None => panic!(
                "the provider made an unexpected request to {}",
                request.redacted_url()
            ),
        }
    }
}

struct TlsTransport(Rc<Shared>);

impl HttpTransport for TlsTransport {
    fn send(&self, request: &HttpRequest) -> Result<HttpResponse, TransportError> {
        FakeTransport(self.0.clone()).send(request)
    }

    fn supports_scheme(&self, scheme: &str) -> bool {
        scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https")
    }
}

#[derive(Clone)]
struct FakeTime {
    now: Rc<Cell<i64>>,
    sleeps: Rc<RefCell<Vec<i64>>>,
}

impl FakeTime {
    fn new() -> Self {
        Self {
            now: Rc::new(Cell::new(1_700_000_000_000)),
            sleeps: Rc::new(RefCell::new(Vec::new())),
        }
    }

    fn sleeps(&self) -> Vec<i64> {
        self.sleeps.borrow().clone()
    }
}

impl Clock for FakeTime {
    fn now_ms(&self) -> i64 {
        self.now.get()
    }
}

impl screenroom_core::Sleeper for FakeTime {
    fn sleep_ms(&self, millis: i64) {
        self.sleeps.borrow_mut().push(millis);
        self.now.set(self.now.get() + millis);
    }
}

fn json(body: &str) -> HttpResponse {
    HttpResponse::new(200, body.as_bytes().to_vec())
}

fn config() -> SubtitleProviderConfig {
    SubtitleProviderConfig::unconfigured()
        .with_api_key("unit-test-key")
        .with_base_url("http://api.invalid/api/v1")
        .with_budget_per_minute(600)
}

fn provider_with(
    config: SubtitleProviderConfig,
    shared: Rc<Shared>,
) -> (OpenSubtitlesProvider, FakeTime) {
    let time = FakeTime::new();
    let provider = OpenSubtitlesProvider::with_parts(
        config,
        Box::new(FakeTransport(shared)),
        Box::new(time.clone()),
        Box::new(time.clone()),
    );
    (provider, time)
}

fn srt() -> Vec<u8> {
    b"1\n00:00:01,000 --> 00:00:03,000\n\xe0\xa6\xb6\xe0\xa7\x81\xe0\xa6\xad \xe0\xa6\xb8\xe0\xa6\xa8\xe0\xa7\x8d\xe0\xa6\xa7\xe0\xa7\x8d\xe0\xa6\xaf\xe0\xa6\xbe\n".to_vec()
}

fn library_with_film() -> (TempDir, Library, String, String) {
    let directory = TempDir::new().expect("temp dir");
    let library = Library::open_in_memory().expect("library");
    let source = library.add_source(directory.path(), None).expect("source");
    let name = "Harbour.Lights.S01E02.mkv";
    let path = directory.path().join(name);
    std::fs::write(&path, b"video").expect("write");
    let parsed = parse_filename(name, None);
    library
        .ingest_file(Ingest {
            source_id: &source.id,
            relative_path: name,
            fingerprint: name,
            size: 5,
            modified_ms: 1,
            scan_id: "scan-1",
            parsed: Some(&parsed),
            expected_root: Some(&source.root_path),
        })
        .expect("ingest");
    let item = library
        .items(&LibraryQuery::page(10))
        .expect("items")
        .remove(0);
    (
        directory,
        library,
        item.id,
        path.to_string_lossy().to_string(),
    )
}

#[test]
fn a_hit_carries_language_release_format_and_a_stable_id() {
    let shared = Shared::new(vec![Reply::Answer(json(include_str!(
        "fixtures/providers/opensubtitles_search_hit.json"
    )))]);
    let (provider, _time) = provider_with(config(), shared.clone());

    let matches = provider
        .search(&SubtitleSearch::episode(
            "Harbour Lights",
            Some(2014),
            1,
            2,
            "bn",
        ))
        .expect("search");

    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].id, "643001");
    assert_eq!(matches[0].file_id, 987654);
    assert_eq!(matches[0].language, "bn");
    assert_eq!(matches[0].language_name, "Bengali");
    assert_eq!(matches[0].format, "srt");
    assert_eq!(
        matches[0].release_name,
        "Harbour.Lights.S01E02.1080p.WEB-DL"
    );
    assert_eq!(matches[0].downloads, 42);
    assert!(!matches[0].hearing_impaired);

    let request = shared.last_request();
    assert!(
        request
            .url
            .starts_with("http://api.invalid/api/v1/subtitles?"),
        "{}",
        request.url
    );
    assert!(request.url.contains("query=Harbour%20Lights"));
    assert!(request.url.contains("languages=bn%2Cben"));
    assert!(request.url.contains("season_number=1"));
    assert!(request.url.contains("episode_number=2"));
    assert!(request.url.contains("type=episode"));
    assert!(request.url.contains("year=2014"));
}

#[test]
fn bengali_is_requested_under_both_of_its_codes() {
    assert_eq!(
        subtitle_language_codes("bn"),
        vec!["bn".to_string(), "ben".to_string()]
    );
    assert_eq!(
        subtitle_language_codes("Bengali"),
        vec!["bn".to_string(), "ben".to_string()]
    );
    assert_eq!(
        subtitle_language_codes("bangla"),
        vec!["bn".to_string(), "ben".to_string()]
    );
    assert_eq!(subtitle_language_codes("en"), vec!["en".to_string()]);
    assert_eq!(subtitle_language_codes("en-US"), vec!["en".to_string()]);
    assert!(subtitle_language_codes("  ").is_empty());
    assert_eq!(subtitle_language_name("ben"), "Bengali");
    assert_eq!(subtitle_language_name("BN"), "Bengali");
}

#[test]
fn a_miss_is_an_empty_answer_rather_than_an_error() {
    let shared = Shared::new(vec![Reply::Answer(json(include_str!(
        "fixtures/providers/opensubtitles_search_miss.json"
    )))]);
    let (provider, _time) = provider_with(config(), shared.clone());

    let matches = provider
        .search(&SubtitleSearch::movie("No Such Film", Some(1970), "bn"))
        .expect("search");

    assert!(matches.is_empty());
    assert_eq!(shared.request_count(), 1);
}

#[test]
fn a_quota_answer_is_its_own_human_state() {
    let shared = Shared::new(vec![Reply::Answer(HttpResponse::new(406, Vec::new()))]);
    let (provider, _time) = provider_with(config(), shared.clone());

    let error = provider
        .search(&SubtitleSearch::movie("Harbour Lights", Some(2014), "bn"))
        .expect_err("a quota answer is an error");

    assert_eq!(error.code(), "provider_quota");
    assert!(error.message().contains("quota"), "{}", error.message());
    assert!(!error.retryable());
}

#[test]
fn a_login_required_answer_is_its_own_state() {
    let shared = Shared::new(vec![Reply::Answer(HttpResponse::new(407, Vec::new()))]);
    let (provider, _time) = provider_with(config(), shared.clone());

    let error = provider
        .search(&SubtitleSearch::movie("Harbour Lights", Some(2014), "bn"))
        .expect_err("a login answer is an error");

    assert_eq!(error.code(), "provider_login_required");
    assert!(error.message().contains("signed-in"));
}

#[test]
fn a_rate_limited_answer_waits_for_retry_after_then_succeeds() {
    let shared = Shared::new(vec![
        Reply::Answer(HttpResponse::new(429, Vec::new()).with_header("Retry-After", "5")),
        Reply::Answer(json(include_str!(
            "fixtures/providers/opensubtitles_search_hit.json"
        ))),
    ]);
    let (provider, time) = provider_with(config(), shared.clone());

    let matches = provider
        .search(&SubtitleSearch::movie("Harbour Lights", Some(2014), "bn"))
        .expect("retried");

    assert_eq!(matches.len(), 1);
    assert_eq!(time.sleeps(), vec![5_000]);
    assert_eq!(shared.request_count(), 2);
}

#[test]
fn a_malformed_search_body_is_reported_rather_than_swallowed() {
    let shared = Shared::new(vec![Reply::Answer(json("{"))]);
    let (provider, _time) = provider_with(config(), shared.clone());

    let error = provider
        .search(&SubtitleSearch::movie("Harbour Lights", Some(2014), "bn"))
        .expect_err("malformed JSON is an error");

    assert_eq!(error.code(), "provider_invalid_response");
}

#[test]
fn a_body_without_the_data_list_is_reported_too() {
    let shared = Shared::new(vec![Reply::Answer(json("{\"total_count\": 1}"))]);
    let (provider, _time) = provider_with(config(), shared.clone());

    let error = provider
        .search(&SubtitleSearch::movie("Harbour Lights", Some(2014), "bn"))
        .expect_err("a missing list is an error");

    assert_eq!(error.code(), "provider_invalid_response");
}

#[test]
fn a_malformed_download_body_is_reported() {
    let shared = Shared::new(vec![Reply::Answer(json("{"))]);
    let (provider, _time) = provider_with(config(), shared.clone());

    let error = provider
        .download(987654, "bn", "srt")
        .expect_err("malformed JSON is an error");

    assert_eq!(error.code(), "provider_invalid_response");
}

#[test]
fn an_unconfigured_subtitle_provider_says_so_and_never_touches_the_network() {
    let shared = Shared::new(Vec::new());
    let (provider, _time) = provider_with(SubtitleProviderConfig::unconfigured(), shared.clone());

    let capability = provider.capability();
    assert!(!capability.is_configured());
    assert!(capability.detail.contains("not configured"));
    assert_eq!(capability.provider, "opensubtitles");

    let error = provider
        .search(&SubtitleSearch::movie("Anything", None, "bn"))
        .expect_err("an unconfigured provider must refuse");
    assert_eq!(error.code(), "provider_not_configured");
    assert_eq!(shared.request_count(), 0);
}

#[test]
fn a_transport_failure_while_fetching_the_file_is_reported() {
    let shared = Shared::new(vec![
        Reply::Answer(json(include_str!(
            "fixtures/providers/opensubtitles_download.json"
        ))),
        Reply::Fail(TransportError::Io(std::io::Error::new(
            std::io::ErrorKind::ConnectionReset,
            "reset",
        ))),
        Reply::Fail(TransportError::Io(std::io::Error::new(
            std::io::ErrorKind::ConnectionReset,
            "reset",
        ))),
        Reply::Fail(TransportError::Io(std::io::Error::new(
            std::io::ErrorKind::ConnectionReset,
            "reset",
        ))),
    ]);
    let (provider, time) = provider_with(config(), shared.clone());

    let error = provider
        .download(987654, "bn", "srt")
        .expect_err("a reset connection is an error");

    assert_eq!(error.code(), "provider_transport");
    assert_eq!(time.sleeps(), vec![500, 1_000]);
    assert_eq!(shared.request_count(), 4);
}

#[test]
fn a_download_writes_into_the_bounded_store_and_leaves_the_media_file_alone() {
    let (_directory, library, file_id, media_path) = library_with_film();
    let shared = Shared::new(vec![
        Reply::Answer(json(include_str!(
            "fixtures/providers/opensubtitles_download.json"
        ))),
        Reply::Answer(HttpResponse::new(200, srt())),
    ]);
    let (provider, _time) = provider_with(config(), shared.clone());

    let downloaded = provider.download(987654, "bn", "srt").expect("download");
    assert_eq!(downloaded.format, "srt");
    assert_eq!(downloaded.language_name, "Bengali");
    assert_eq!(downloaded.remaining_downloads, Some(4));
    assert_eq!(downloaded.bytes, srt());

    let requests = shared.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].method, HttpMethod::Post);
    assert!(requests[0].url.ends_with("/api/v1/download"));
    assert!(
        requests[0].headers.iter().any(|(name, value)| {
            name.eq_ignore_ascii_case("api-key") && value == "unit-test-key"
        })
    );
    assert_eq!(
        requests[1].url,
        "http://api.invalid/api/v1/download/file/987654"
    );
    assert!(
        !requests[1]
            .headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("api-key")),
        "the key must not travel to the download host"
    );

    let stored = library
        .import_downloaded_subtitle(&file_id, &downloaded)
        .expect("stored");
    assert_eq!(stored.language, "Bengali");
    assert_eq!(stored.format, "srt");
    assert_eq!(stored.bytes, srt().len() as i64);
    assert!(stored.provenance.contains("downloaded from OpenSubtitles"));
    assert!(stored.label.contains("Harbour.Lights.S01E02.bn.srt"));

    let listed = library.subtitles_for(&file_id).expect("listed");
    assert_eq!(listed, vec![stored.clone()]);
    let bytes = library
        .subtitle_bytes(&stored.id)
        .expect("bytes")
        .expect("present");
    assert_eq!(bytes, srt());
    assert_eq!(std::fs::read(&media_path).expect("media"), b"video");
}

#[test]
fn an_oversized_binary_or_wrongly_typed_download_is_refused() {
    let (_directory, library, file_id, _media) = library_with_film();

    let oversized = DownloadedSubtitle {
        file_id: 1,
        language: "bn".to_string(),
        language_name: "Bengali".to_string(),
        release_name: "huge".to_string(),
        format: "srt".to_string(),
        bytes: vec![b'a'; 4 * 1024 * 1024 + 1],
        remaining_downloads: None,
    };
    assert_eq!(
        library
            .import_downloaded_subtitle(&file_id, &oversized)
            .unwrap_err()
            .code(),
        "subtitle_size"
    );

    let binary = DownloadedSubtitle {
        bytes: vec![0, 0, 0, 0],
        ..oversized
    };
    assert_eq!(
        library
            .import_downloaded_subtitle(&file_id, &binary)
            .unwrap_err()
            .code(),
        "subtitle_binary"
    );

    let wrong_format = DownloadedSubtitle {
        format: "exe".to_string(),
        bytes: srt(),
        ..binary
    };
    assert_eq!(
        library
            .import_downloaded_subtitle(&file_id, &wrong_format)
            .unwrap_err()
            .code(),
        "subtitle_format"
    );

    let unknown_file = DownloadedSubtitle {
        format: "srt".to_string(),
        ..wrong_format
    };
    assert_eq!(
        library
            .import_downloaded_subtitle("missing", &unknown_file)
            .unwrap_err()
            .code(),
        "subtitle_file"
    );

    assert!(library.subtitles_for(&file_id).expect("listed").is_empty());
}

#[test]
fn a_movie_search_does_not_send_episode_fields() {
    let shared = Shared::new(vec![Reply::Answer(json(include_str!(
        "fixtures/providers/opensubtitles_search_miss.json"
    )))]);
    let (provider, _time) = provider_with(config(), shared.clone());

    provider
        .search(&SubtitleSearch {
            title: "The Lantern Keeper".to_string(),
            year: Some(1998),
            kind: MediaKind::Movie,
            season: None,
            episode: None,
            language: "en".to_string(),
            file_hash: Some("abcdef0123456789".to_string()),
        })
        .expect("search");

    let request = shared.last_request();
    assert!(request.url.contains("type=movie"));
    assert!(!request.url.contains("season_number"));
    assert!(request.url.contains("moviehash=abcdef0123456789"));
}

fn download_reply(link: &str) -> HttpResponse {
    json(&format!(
        "{{\"link\": \"{link}\", \"file_name\": \"Harbour.Lights.bn.srt\", \"remaining\": 1}}"
    ))
}

fn https_config() -> SubtitleProviderConfig {
    SubtitleProviderConfig::unconfigured().with_api_key("unit-test-key")
}

#[test]
fn a_download_link_to_loopback_or_an_unrelated_host_is_refused() {
    for link in [
        "http://127.0.0.1:8080/subs/file.srt",
        "https://127.0.0.1/file.srt",
        "http://[::1]/file.srt",
        "http://localhost/file.srt",
        "http://unrelated.invalid/file.srt",
        "https://dl.opensubtitles.org@unrelated.invalid/file.srt",
        "file:///etc/passwd",
    ] {
        let shared = Shared::new(vec![Reply::Answer(download_reply(link))]);
        let (provider, _time) = provider_with(config(), shared.clone());
        let error = provider
            .download(987654, "bn", "srt")
            .expect_err("a link outside the provider must be refused");
        assert_eq!(error.code(), "provider_invalid_response", "{link}");
        assert_eq!(
            shared.request_count(),
            1,
            "the refused link must never be fetched: {link}"
        );
    }
}

#[test]
fn a_provider_cdn_link_still_downloads() {
    let link = "https://dl.opensubtitles.org/en/download/file/987654";
    let shared = Shared::new(vec![
        Reply::Answer(download_reply(link)),
        Reply::Answer(HttpResponse::new(200, srt())),
    ]);
    let (provider, _time) = provider_with(config(), shared.clone());

    let downloaded = provider.download(987654, "bn", "srt").expect("download");

    assert_eq!(downloaded.bytes, srt());
    let requests = shared.requests();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[1].url, link);
    assert!(
        !requests[1]
            .headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("api-key")),
        "the key must not travel to the download host"
    );
}

#[test]
fn a_download_link_to_the_configured_provider_host_is_allowed() {
    let shared = Shared::new(vec![
        Reply::Answer(download_reply(
            "http://api.invalid/api/v1/download/file/987654",
        )),
        Reply::Answer(HttpResponse::new(200, srt())),
    ]);
    let (provider, _time) = provider_with(config(), shared.clone());

    provider.download(987654, "bn", "srt").expect("download");

    assert_eq!(
        shared.last_request().url,
        "http://api.invalid/api/v1/download/file/987654"
    );
}

#[test]
fn subtitle_capability_fails_closed_when_the_base_address_needs_a_transport_it_does_not_have() {
    let capability =
        OpenSubtitlesProvider::with_system_clock(https_config(), Box::new(TcpTransport::default()))
            .capability();
    assert!(!capability.is_configured());
    assert!(capability.detail.contains("https"), "{}", capability.detail);
    assert_eq!(capability.provider, "opensubtitles");

    let shared = Shared::new(Vec::new());
    let (guarded, _time) = provider_with(https_config(), shared.clone());
    assert!(!guarded.capability().is_configured());
    assert_eq!(shared.request_count(), 0);

    let (plain, _time) = provider_with(config(), shared);
    assert!(plain.capability().is_configured());
}

#[test]
fn subtitle_capability_reports_configured_when_a_tls_capable_transport_is_injected() {
    let time = FakeTime::new();
    let provider = OpenSubtitlesProvider::with_parts(
        https_config(),
        Box::new(TlsTransport(Shared::new(Vec::new()))),
        Box::new(time.clone()),
        Box::new(time),
    );

    let capability = provider.capability();

    assert!(capability.is_configured());
    assert_eq!(capability.provider, "opensubtitles");
}
