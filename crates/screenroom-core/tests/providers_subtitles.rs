use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::rc::Rc;
use std::sync::{Mutex, MutexGuard};
use std::thread;

use screenroom_core::{
    Clock, DOWNLOAD_HOST_DOMAINS, DownloadedSubtitle, HttpMethod, HttpRequest, HttpResponse,
    HttpTransport, Ingest, Library, LibraryQuery, MediaKind, Secret, SubtitleProvider,
    SubtitleProviderConfig, SubtitleSearch, TSDB_API_KEY_ENVIRONMENT, TSDB_BASE_URL_ENVIRONMENT,
    TSDB_CONFIG_DIR_ENVIRONMENT, TSDB_CONFIG_ENVIRONMENT, TSDB_KEY_FILE_NAME, TSDB_PROVIDER,
    TcpTransport, TransportError, TsdbProvider, parse_filename, subtitle_language_codes,
    subtitle_language_name,
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

fn missing() -> HttpResponse {
    HttpResponse::new(404, Vec::new())
}

fn config() -> SubtitleProviderConfig {
    SubtitleProviderConfig::unconfigured()
        .with_base_url("http://api.invalid")
        .with_budget_per_minute(600)
}

fn keyed_config() -> SubtitleProviderConfig {
    config()
        .with_base_url("https://api.invalid")
        .with_api_key("unit-test-key")
}

fn provider_with(config: SubtitleProviderConfig, shared: Rc<Shared>) -> (TsdbProvider, FakeTime) {
    let time = FakeTime::new();
    let provider = TsdbProvider::with_parts(
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
fn a_tmdb_movie_lookup_parses_the_items_it_keeps() {
    let shared = Shared::new(vec![Reply::Answer(json(include_str!(
        "fixtures/providers/tsdb_movie_lookup.json"
    )))]);
    let (provider, _time) = provider_with(config(), shared.clone());

    let matches = provider
        .search(&SubtitleSearch::movie("Harbour Lights", Some(2014), "bn").with_tmdb(76341))
        .expect("search");

    assert_eq!(matches.len(), 2);
    assert_eq!(matches[0].id, 918273);
    assert_eq!(matches[0].language, "bn");
    assert_eq!(matches[0].language_name, "Bengali");
    assert_eq!(matches[0].format, "srt");
    assert_eq!(matches[0].cues, 640);
    assert_eq!(matches[0].encoding, "utf-8-sig");
    assert!(!matches[0].hearing_impaired);
    assert!(!matches[0].ai_transcribed);
    assert_eq!(matches[0].release_name, "Harbour.Lights.2014.1080p.WEB-DL");
    assert_eq!(matches[1].id, 918276);
    assert_eq!(matches[1].format, "ass");
    assert!(matches[1].hearing_impaired);
    assert!(matches[1].ai_transcribed);

    let request = shared.last_request();
    assert!(
        request
            .url
            .starts_with("http://api.invalid/v1/by-tmdb/movie/76341?"),
        "{}",
        request.url
    );
    assert!(request.url.contains("lang=bn%2Cben"), "{}", request.url);
    assert!(request.url.contains("sort=cues"), "{}", request.url);
    assert!(request.url.contains("limit=100"), "{}", request.url);
    assert_eq!(request.method, HttpMethod::Get);
    assert!(
        !request
            .headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("authorization")),
        "anonymous lookups must not carry a key"
    );
}

#[test]
fn a_tmdb_episode_lookup_drills_into_the_episode() {
    let shared = Shared::new(vec![Reply::Answer(json(include_str!(
        "fixtures/providers/tsdb_episode_lookup.json"
    )))]);
    let (provider, _time) = provider_with(config(), shared.clone());

    let matches = provider
        .search(&SubtitleSearch::episode("Harbour Lights", Some(2014), 4, 2, "bn").with_tmdb(1399))
        .expect("search");

    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].id, 1414527232);
    assert_eq!(matches[0].language, "bn");
    assert_eq!(matches[0].cues, 720);
    let request = shared.last_request();
    assert!(
        request
            .url
            .starts_with("http://api.invalid/v1/by-tmdb/tv/1399/season/4/episode/2?"),
        "{}",
        request.url
    );
}

#[test]
fn a_series_season_without_an_episode_falls_through_instead_of_taking_a_season_pack() {
    let shared = Shared::new(vec![Reply::Answer(json(include_str!(
        "fixtures/providers/tsdb_episode_lookup.json"
    )))]);
    let (provider, _time) = provider_with(config(), shared.clone());

    let matches = provider
        .search(&SubtitleSearch {
            title: "Harbour Lights".to_string(),
            year: Some(2014),
            kind: MediaKind::Series,
            season: Some(4),
            episode: None,
            language: "bn".to_string(),
            tmdb_id: Some(1399),
            imdb_id: Some("tt0944947".to_string()),
            release_name: None,
        })
        .expect("search");

    assert_eq!(matches.len(), 1);
    assert_eq!(shared.request_count(), 1);
    let request = shared.last_request();
    assert!(
        request
            .url
            .starts_with("http://api.invalid/v1/by-imdb/tt0944947?"),
        "{}",
        request.url
    );
    assert!(!request.url.contains("/season/"), "{}", request.url);
    assert!(!request.url.contains("/episode/"), "{}", request.url);
}

#[test]
fn an_imdb_lookup_runs_when_the_tmdb_route_misses() {
    let shared = Shared::new(vec![
        Reply::Answer(missing()),
        Reply::Answer(json(include_str!(
            "fixtures/providers/tsdb_movie_lookup.json"
        ))),
    ]);
    let (provider, _time) = provider_with(config(), shared.clone());

    let matches = provider
        .search(
            &SubtitleSearch::movie("Harbour Lights", Some(2014), "bn")
                .with_tmdb(76341)
                .with_imdb("tt1454468"),
        )
        .expect("search");

    assert_eq!(matches.len(), 2);
    let requests = shared.requests();
    assert_eq!(requests.len(), 2);
    assert!(
        requests[1]
            .url
            .starts_with("http://api.invalid/v1/by-imdb/tt1454468?"),
        "{}",
        requests[1].url
    );
}

#[test]
fn a_release_name_is_tried_after_the_identifiers_miss() {
    let shared = Shared::new(vec![
        Reply::Answer(missing()),
        Reply::Answer(missing()),
        Reply::Answer(json(include_str!(
            "fixtures/providers/tsdb_title_match.json"
        ))),
    ]);
    let (provider, _time) = provider_with(config(), shared.clone());

    let matches = provider
        .search(
            &SubtitleSearch::movie("The Lantern Keeper", Some(1998), "en")
                .with_tmdb(597)
                .with_imdb("tt0120338")
                .with_release("The.Lantern.Keeper.1998.Remastered"),
        )
        .expect("search");

    assert_eq!(matches.len(), 1);
    let requests = shared.requests();
    assert_eq!(requests.len(), 3);
    assert!(
        requests[2].url.starts_with(
            "http://api.invalid/v1/by-releasename?release=The.Lantern.Keeper.1998.Remastered"
        ),
        "{}",
        requests[2].url
    );
    assert!(requests[2].url.contains("lang=en"), "{}", requests[2].url);
}

#[test]
fn a_title_lookup_is_the_last_anonymous_strategy() {
    let shared = Shared::new(vec![Reply::Answer(json(include_str!(
        "fixtures/providers/tsdb_title_match.json"
    )))]);
    let (provider, _time) = provider_with(config(), shared.clone());

    let matches = provider
        .search(&SubtitleSearch::movie(
            "The Lantern Keeper",
            Some(1998),
            "en",
        ))
        .expect("search");

    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].id, 771001);
    let request = shared.last_request();
    assert!(
        request
            .url
            .starts_with("http://api.invalid/v1/by-title?q=The%20Lantern%20Keeper&type=movie"),
        "{}",
        request.url
    );
    assert_eq!(shared.request_count(), 1);
}

#[test]
fn a_not_found_on_every_strategy_is_an_empty_answer() {
    let shared = Shared::new(vec![
        Reply::Answer(missing()),
        Reply::Answer(missing()),
        Reply::Answer(missing()),
        Reply::Answer(missing()),
    ]);
    let (provider, _time) = provider_with(config(), shared.clone());

    let matches = provider
        .search(
            &SubtitleSearch::movie("No Such Film", Some(1970), "en")
                .with_tmdb(1)
                .with_imdb("tt0000001")
                .with_release("No.Such.Film.1970"),
        )
        .expect("search");

    assert!(matches.is_empty());
    let requests = shared.requests();
    assert_eq!(requests.len(), 4);
    assert!(
        requests
            .iter()
            .all(|request| !request.url.contains("/v1/by-search")),
        "an anonymous provider must never call keyed search"
    );
}

#[test]
fn an_empty_item_list_is_a_miss_that_falls_through() {
    let shared = Shared::new(vec![
        Reply::Answer(json(include_str!("fixtures/providers/tsdb_empty.json"))),
        Reply::Answer(json(include_str!("fixtures/providers/tsdb_empty.json"))),
    ]);
    let (provider, _time) = provider_with(config(), shared.clone());

    let matches = provider
        .search(&SubtitleSearch::movie("No Such Film", Some(1970), "en").with_tmdb(9))
        .expect("search");

    assert!(matches.is_empty());
    assert_eq!(shared.request_count(), 2);
}

#[test]
fn a_title_for_another_media_type_is_a_miss() {
    let shared = Shared::new(vec![
        Reply::Answer(json(include_str!(
            "fixtures/providers/tsdb_movie_lookup.json"
        ))),
        Reply::Answer(missing()),
    ]);
    let (provider, _time) = provider_with(config(), shared.clone());

    let matches = provider
        .search(&SubtitleSearch::episode("Harbour Lights", Some(2014), 1, 1, "bn").with_tmdb(1399))
        .expect("search");

    assert!(matches.is_empty());
    let requests = shared.requests();
    assert_eq!(requests.len(), 2);
    assert!(
        requests[0].url.contains("/season/1/episode/1"),
        "{}",
        requests[0].url
    );
    assert!(
        requests[1]
            .url
            .starts_with("http://api.invalid/v1/by-title?"),
        "{}",
        requests[1].url
    );
}

#[test]
fn a_year_that_differs_by_more_than_one_is_a_miss() {
    let shared = Shared::new(vec![
        Reply::Answer(json(include_str!(
            "fixtures/providers/tsdb_movie_lookup.json"
        ))),
        Reply::Answer(missing()),
    ]);
    let (provider, _time) = provider_with(config(), shared.clone());
    let matches = provider
        .search(&SubtitleSearch::movie("Harbour Lights", Some(1990), "bn").with_tmdb(76341))
        .expect("search");
    assert!(matches.is_empty());

    let shared = Shared::new(vec![Reply::Answer(json(include_str!(
        "fixtures/providers/tsdb_movie_lookup.json"
    )))]);
    let (provider, _time) = provider_with(config(), shared.clone());
    let matches = provider
        .search(&SubtitleSearch::movie("Harbour Lights", Some(2015), "bn").with_tmdb(76341))
        .expect("search");
    assert_eq!(matches.len(), 2);
}

#[test]
fn a_keyed_search_picks_the_best_title_then_reads_its_subtitles() {
    let shared = Shared::new(vec![
        Reply::Answer(missing()),
        Reply::Answer(json(include_str!(
            "fixtures/providers/tsdb_search_keyed.json"
        ))),
        Reply::Answer(json(include_str!(
            "fixtures/providers/tsdb_movie_lookup.json"
        ))),
    ]);
    let (provider, _time) = provider_with(keyed_config(), shared.clone());

    let matches = provider
        .search(&SubtitleSearch::movie("Harbour Lights", Some(2014), "bn"))
        .expect("search");

    assert_eq!(matches.len(), 2);
    let requests = shared.requests();
    assert_eq!(requests.len(), 3);
    assert!(
        requests[0]
            .url
            .starts_with("https://api.invalid/v1/by-title?"),
        "{}",
        requests[0].url
    );
    assert!(
        requests[1]
            .url
            .starts_with("https://api.invalid/v1/by-search?q=Harbour%20Lights&type=movie"),
        "{}",
        requests[1].url
    );
    assert!(
        requests[1].headers.iter().any(|(name, value)| {
            name.eq_ignore_ascii_case("authorization") && value == "Bearer unit-test-key"
        }),
        "keyed search must carry the key"
    );
    assert!(
        requests[2]
            .url
            .starts_with("https://api.invalid/v1/by-tmdb/movie/76342?"),
        "the title with the most Bengali subtitles is the one to read: {}",
        requests[2].url
    );
    assert!(
        !requests[2]
            .headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("authorization")),
        "a lookup after the search must not carry the key"
    );
}

#[test]
fn a_keyed_search_that_matches_no_title_is_a_miss() {
    let shared = Shared::new(vec![
        Reply::Answer(missing()),
        Reply::Answer(json(
            "{\"total\": 1, \"results\": [{\"name\": \"Harbour Lights Returns\", \
             \"media_type\": \"movie\", \"year\": 2014, \"tmdb_id\": 76343, \
             \"language_counts\": {\"bn\": 40}}]}",
        )),
    ]);
    let (provider, _time) = provider_with(keyed_config(), shared.clone());

    let matches = provider
        .search(&SubtitleSearch::movie("Harbour Lights", Some(2014), "bn"))
        .expect("search");

    assert!(matches.is_empty());
    assert_eq!(shared.request_count(), 2);
}

#[test]
fn a_search_answer_without_a_results_list_is_reported() {
    let shared = Shared::new(vec![
        Reply::Answer(missing()),
        Reply::Answer(json("{\"total\": 0}")),
    ]);
    let (provider, _time) = provider_with(keyed_config(), shared.clone());

    let error = provider
        .search(&SubtitleSearch::movie("Harbour Lights", Some(2014), "bn"))
        .expect_err("a search without results is an error");

    assert_eq!(error.code(), "provider_invalid_response");
}

#[test]
fn an_anonymous_provider_reports_lookups_and_never_calls_keyed_search() {
    let shared = Shared::new(vec![Reply::Answer(missing())]);
    let (provider, _time) = provider_with(config(), shared.clone());

    assert!(!provider.search_is_keyed());
    let capability = provider.capability();
    assert!(capability.is_configured());
    assert_eq!(capability.provider, TSDB_PROVIDER);
    assert!(
        capability.detail.contains("without a key"),
        "{}",
        capability.detail
    );
    assert!(
        capability.detail.contains("keyed title search"),
        "{}",
        capability.detail
    );

    let matches = provider
        .search(&SubtitleSearch::movie("Harbour Lights", Some(2014), "bn"))
        .expect("search");
    assert!(matches.is_empty());
    assert_eq!(shared.request_count(), 1);
    assert!(!shared.last_request().url.contains("/v1/by-search"));
}

#[test]
fn a_malformed_json_body_is_reported_rather_than_swallowed() {
    let shared = Shared::new(vec![Reply::Answer(json("{"))]);
    let (provider, _time) = provider_with(config(), shared.clone());

    let error = provider
        .search(&SubtitleSearch::movie("Harbour Lights", Some(2014), "bn").with_tmdb(76341))
        .expect_err("malformed JSON is an error");

    assert_eq!(error.code(), "provider_invalid_response");
}

#[test]
fn a_body_without_a_title_or_a_subtitle_list_is_reported() {
    for body in [
        "{\"subtitles\": {\"items\": []}}",
        include_str!("fixtures/providers/tsdb_error.json"),
        "{\"title\": {\"media_type\": \"movie\", \"year\": 2014}}",
    ] {
        let shared = Shared::new(vec![Reply::Answer(json(body))]);
        let (provider, _time) = provider_with(config(), shared.clone());
        let error = provider
            .search(&SubtitleSearch::movie("Harbour Lights", Some(2014), "bn").with_tmdb(76341))
            .expect_err("a body without the expected objects is an error");
        assert_eq!(error.code(), "provider_invalid_response", "{body}");
    }
}

#[test]
fn a_server_error_is_retried_and_honours_retry_after() {
    let shared = Shared::new(vec![
        Reply::Answer(HttpResponse::new(503, Vec::new()).with_header("Retry-After", "3")),
        Reply::Answer(json(include_str!(
            "fixtures/providers/tsdb_movie_lookup.json"
        ))),
    ]);
    let (provider, time) = provider_with(config(), shared.clone());

    let matches = provider
        .search(&SubtitleSearch::movie("Harbour Lights", Some(2014), "bn").with_tmdb(76341))
        .expect("retried");

    assert_eq!(matches.len(), 2);
    assert_eq!(time.sleeps(), vec![3_000]);
    assert_eq!(shared.request_count(), 2);
}

#[test]
fn a_server_error_without_retry_after_uses_the_backoff_and_then_gives_up() {
    let shared = Shared::new(vec![
        Reply::Answer(HttpResponse::new(503, Vec::new())),
        Reply::Answer(HttpResponse::new(503, Vec::new())),
        Reply::Answer(HttpResponse::new(503, Vec::new())),
    ]);
    let (provider, time) = provider_with(config(), shared.clone());

    let error = provider
        .search(&SubtitleSearch::movie("Harbour Lights", Some(2014), "bn").with_tmdb(76341))
        .expect_err("three server errors are still an error");

    assert_eq!(error.code(), "provider_server");
    assert_eq!(time.sleeps(), vec![500, 1_000]);
    assert_eq!(shared.request_count(), 3);
}

#[test]
fn a_rate_limited_answer_waits_for_retry_after_then_succeeds() {
    let shared = Shared::new(vec![
        Reply::Answer(HttpResponse::new(429, Vec::new()).with_header("Retry-After", "5")),
        Reply::Answer(json(include_str!(
            "fixtures/providers/tsdb_movie_lookup.json"
        ))),
    ]);
    let (provider, time) = provider_with(config(), shared.clone());

    let matches = provider
        .search(&SubtitleSearch::movie("Harbour Lights", Some(2014), "bn").with_tmdb(76341))
        .expect("retried");

    assert_eq!(matches.len(), 2);
    assert_eq!(time.sleeps(), vec![5_000]);
    assert_eq!(shared.request_count(), 2);
}

#[test]
fn an_unauthorized_or_rejected_answer_is_returned_rather_than_skipped() {
    let shared = Shared::new(vec![Reply::Answer(HttpResponse::new(401, Vec::new()))]);
    let (provider, _time) = provider_with(config(), shared.clone());
    let error = provider
        .search(
            &SubtitleSearch::movie("Harbour Lights", Some(2014), "bn")
                .with_tmdb(76341)
                .with_imdb("tt1454468"),
        )
        .expect_err("an unauthorized answer is an error");
    assert_eq!(error.code(), "provider_unauthorized");
    assert_eq!(shared.request_count(), 1);

    let shared = Shared::new(vec![Reply::Answer(HttpResponse::new(400, Vec::new()))]);
    let (provider, _time) = provider_with(config(), shared.clone());
    let error = provider
        .search(
            &SubtitleSearch::movie("Harbour Lights", Some(2014), "bn")
                .with_tmdb(76341)
                .with_imdb("tt1454468"),
        )
        .expect_err("a bad request is an error");
    assert_eq!(error.code(), "provider_rejected");
    assert_eq!(shared.request_count(), 1);
}

#[test]
fn an_oversized_page_keeps_its_valid_items_without_paging() {
    let shared = Shared::new(vec![Reply::Answer(json(include_str!(
        "fixtures/providers/tsdb_oversized_page.json"
    )))]);
    let (provider, _time) = provider_with(config(), shared.clone());

    let matches = provider
        .search(&SubtitleSearch::movie("Harbour Lights", Some(2014), "bn").with_tmdb(76341))
        .expect("search");

    assert_eq!(matches.len(), 100);
    assert_eq!(matches[0].id, 1_000_000);
    assert_eq!(matches[99].id, 1_000_099);
    assert_eq!(shared.request_count(), 1);
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
fn a_blank_language_never_touches_the_network() {
    let shared = Shared::new(Vec::new());
    let (provider, _time) = provider_with(config(), shared.clone());

    let matches = provider
        .search(&SubtitleSearch::movie("Harbour Lights", Some(2014), "  ").with_tmdb(76341))
        .expect("search");

    assert!(matches.is_empty());
    assert_eq!(shared.request_count(), 0);
}

#[test]
fn a_keyed_download_carries_the_key_and_the_allowlisted_redirect_hosts() {
    let shared = Shared::new(vec![Reply::Answer(HttpResponse::new(200, srt()))]);
    let (provider, _time) = provider_with(
        SubtitleProviderConfig::unconfigured()
            .with_api_key("unit-test-key")
            .with_budget_per_minute(600),
        shared.clone(),
    );

    let downloaded = provider
        .download(1414527232, "bn", "SRT")
        .expect("download");

    assert_eq!(downloaded.id, 1414527232);
    assert_eq!(downloaded.language, "bn");
    assert_eq!(downloaded.language_name, "Bengali");
    assert_eq!(downloaded.format, "srt");
    assert_eq!(downloaded.bytes, srt());
    assert!(downloaded.release_name.is_empty());

    let request = shared.last_request();
    assert_eq!(request.url, "https://api.thesubtitledb.org/get/1414527232");
    assert_eq!(request.max_bytes, 4 * 1024 * 1024);
    assert_eq!(request.redirect_hosts, DOWNLOAD_HOST_DOMAINS);
    assert!(
        request.headers.iter().any(|(name, value)| {
            name.eq_ignore_ascii_case("authorization") && value == "Bearer unit-test-key"
        }),
        "a keyed download carries the key to the provider"
    );
}

#[test]
fn an_anonymous_download_carries_no_key_and_keeps_a_known_format_hint() {
    let shared = Shared::new(vec![Reply::Answer(HttpResponse::new(200, srt()))]);
    let (provider, _time) = provider_with(
        SubtitleProviderConfig::unconfigured().with_budget_per_minute(600),
        shared.clone(),
    );

    let downloaded = provider
        .download(1414527232, "en", "VTT")
        .expect("download");

    assert_eq!(downloaded.format, "vtt");
    assert!(
        !shared
            .last_request()
            .headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("authorization"))
    );

    let shared = Shared::new(vec![Reply::Answer(HttpResponse::new(200, srt()))]);
    let (provider, _time) = provider_with(
        SubtitleProviderConfig::unconfigured().with_budget_per_minute(600),
        shared.clone(),
    );
    let downloaded = provider
        .download(1414527232, "en", "exe")
        .expect("download");
    assert_eq!(downloaded.format, "srt");
}

#[test]
fn a_download_address_that_is_not_secure_or_not_the_provider_is_refused() {
    let shared = Shared::new(Vec::new());
    let (provider, _time) = provider_with(config(), shared.clone());
    let error = provider
        .download(1414527232, "bn", "srt")
        .expect_err("a plain http download address is refused");
    assert_eq!(error.code(), "provider_invalid_response");
    assert!(
        error.message().contains("not secure"),
        "{}",
        error.message()
    );
    assert_eq!(shared.request_count(), 0);

    let shared = Shared::new(Vec::new());
    let (provider, _time) = provider_with(
        SubtitleProviderConfig::unconfigured()
            .with_base_url("https://127.0.0.1:9/api")
            .with_budget_per_minute(600),
        shared.clone(),
    );
    let error = provider
        .download(1414527232, "bn", "srt")
        .expect_err("a loopback base host is refused");
    assert_eq!(error.code(), "provider_invalid_response");
    assert!(
        error.message().contains("outside the configured provider"),
        "{}",
        error.message()
    );
    assert_eq!(shared.request_count(), 0);

    let shared = Shared::new(vec![Reply::Answer(HttpResponse::new(200, srt()))]);
    let (provider, _time) = provider_with(
        SubtitleProviderConfig::unconfigured()
            .with_base_url("https://mirror.example.test/api")
            .with_budget_per_minute(600),
        shared.clone(),
    );
    provider
        .download(1414527232, "bn", "srt")
        .expect("the configured base host is acceptable");
    assert_eq!(
        shared.last_request().url,
        "https://mirror.example.test/api/get/1414527232"
    );

    let shared = Shared::new(Vec::new());
    let (provider, _time) = provider_with(config(), shared.clone());
    let error = provider
        .download(0, "bn", "srt")
        .expect_err("a zero handle is refused");
    assert_eq!(error.code(), "provider_invalid_response");
    assert_eq!(shared.request_count(), 0);
}

#[test]
fn a_transport_failure_while_fetching_the_file_is_retried_then_reported() {
    let failures = (0..3)
        .map(|_| {
            Reply::Fail(TransportError::Io(std::io::Error::new(
                std::io::ErrorKind::ConnectionReset,
                "reset",
            )))
        })
        .collect();
    let shared = Shared::new(failures);
    let (provider, time) = provider_with(
        SubtitleProviderConfig::unconfigured().with_budget_per_minute(600),
        shared.clone(),
    );

    let error = provider
        .download(1414527232, "bn", "srt")
        .expect_err("a reset connection is an error");

    assert_eq!(error.code(), "provider_transport");
    assert_eq!(time.sleeps(), vec![500, 1_000]);
    assert_eq!(shared.request_count(), 3);
}

#[test]
fn a_four_mebibyte_body_is_accepted_and_an_eight_mebibyte_body_is_refused() {
    let length = 4 * 1024 * 1024;
    let (url, handle) = spawn_body_server(length, length);
    let answer = TcpTransport::default()
        .send(&HttpRequest::get(url).with_max_bytes(length))
        .expect("a four mebibyte body fits the subtitle budget");
    assert_eq!(answer.body.len(), length);
    handle.join().expect("the server thread");

    let (url, handle) = spawn_body_server(8 * 1024 * 1024, 8 * 1024 * 1024);
    let error = TcpTransport::default()
        .send(&HttpRequest::get(url).with_max_bytes(length))
        .expect_err("a body over the subtitle budget is refused");
    assert!(
        matches!(error, TransportError::TooLarge { limit } if limit == length),
        "{error:?}"
    );
    handle.join().expect("the server thread");

    let (url, handle) = spawn_body_server(8 * 1024 * 1024 + 1, 0);
    let error = TcpTransport::default()
        .send(&HttpRequest::get(url).with_max_bytes(usize::MAX))
        .expect_err("the hard transfer ceiling is eight mebibytes");
    assert!(
        matches!(error, TransportError::TooLarge { limit } if limit == 8 * 1024 * 1024),
        "{error:?}"
    );
    handle.join().expect("the server thread");
}

fn spawn_body_server(declared: usize, sent: usize) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback listener");
    let address = listener.local_addr().expect("a bound address");
    let handle = thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut request = [0_u8; 2_048];
            let _ = stream.read(&mut request);
            let head = format!("HTTP/1.1 200 OK\r\nContent-Length: {declared}\r\n\r\n");
            if stream.write_all(head.as_bytes()).is_ok() {
                let chunk = vec![b'a'; 8_192];
                let mut written = 0_usize;
                while written < sent {
                    let take = chunk.len().min(sent - written);
                    if stream.write_all(&chunk[..take]).is_err() {
                        return;
                    }
                    written += take;
                }
                let _ = stream.flush();
            }
        }
    });
    (format!("http://{address}/body.srt"), handle)
}

#[test]
fn a_download_is_stored_with_its_own_provenance_and_leaves_the_media_alone() {
    let (_directory, library, file_id, media_path) = library_with_film();
    let media_before = std::fs::read(&media_path).expect("media");
    let downloaded = DownloadedSubtitle {
        id: 1414527232,
        language: "bn".to_string(),
        language_name: "Bengali".to_string(),
        release_name: String::new(),
        format: "srt".to_string(),
        bytes: srt(),
    };

    let stored = library
        .import_downloaded_subtitle(&file_id, &downloaded)
        .expect("stored");

    assert_eq!(stored.language, "Bengali");
    assert_eq!(stored.format, "srt");
    assert_eq!(stored.bytes, srt().len() as i64);
    assert_eq!(stored.provenance, "downloaded from The Subtitle DB");
    assert_eq!(stored.label, "Bengali · downloaded");

    let listed = library.subtitles_for(&file_id).expect("listed");
    assert_eq!(listed, vec![stored.clone()]);
    let bytes = library
        .subtitle_bytes(&stored.id)
        .expect("bytes")
        .expect("present");
    assert_eq!(bytes, srt());
    assert_eq!(std::fs::read(&media_path).expect("media"), media_before);
    assert_eq!(std::fs::read(&media_path).expect("media"), b"video");
}

#[test]
fn an_empty_binary_or_wrongly_typed_download_is_refused() {
    let (_directory, library, file_id, _media) = library_with_film();

    let oversized = DownloadedSubtitle {
        id: 1,
        language: "bn".to_string(),
        language_name: "Bengali".to_string(),
        release_name: String::new(),
        format: "srt".to_string(),
        bytes: vec![b'a'; 4 * 1024 * 1024 + 1],
    };
    assert_eq!(
        library
            .import_downloaded_subtitle(&file_id, &oversized)
            .unwrap_err()
            .code(),
        "subtitle_size"
    );

    let empty = DownloadedSubtitle {
        bytes: Vec::new(),
        ..oversized.clone()
    };
    assert_eq!(
        library
            .import_downloaded_subtitle(&file_id, &empty)
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
fn a_broken_configuration_refuses_search_and_download_instead_of_going_anonymous() {
    let broken = SubtitleProviderConfig {
        configuration_error: Some("The subtitle key file was rejected.".to_string()),
        ..SubtitleProviderConfig::unconfigured()
    };
    let shared = Shared::new(Vec::new());
    let (provider, _time) = provider_with(broken, shared.clone());

    let error = provider
        .search(&SubtitleSearch::movie("Harbour Lights", Some(2014), "en"))
        .expect_err("a broken configuration must not fall back to anonymous lookups");
    assert_eq!(error.code(), "provider_not_configured");

    let error = provider
        .download(1414527232, "en", "srt")
        .expect_err("a broken configuration must not download anonymously");
    assert_eq!(error.code(), "provider_not_configured");

    assert_eq!(shared.request_count(), 0);
}

#[test]
fn a_key_and_a_request_never_show_the_key_when_debugged() {
    let key = "hidden-key-value-9f3";
    let shared = Shared::new(Vec::new());
    let (provider, _time) = provider_with(
        SubtitleProviderConfig::unconfigured()
            .with_api_key(key)
            .with_budget_per_minute(600),
        shared.clone(),
    );

    assert!(!format!("{provider:?}").contains(key));
    let secret = Secret::new(key);
    assert!(!format!("{secret:?}").contains(key));
    assert!(!format!("{secret}").contains(key));
    let request =
        HttpRequest::get("https://api.thesubtitledb.org/get/1").with_header("Authorization", key);
    assert!(!format!("{request:?}").contains(key));
    assert!(!provider.capability().detail.contains(key));
}

#[test]
fn capability_fails_closed_for_an_error_or_an_unsupported_base_address() {
    let shared = Shared::new(Vec::new());
    let config = SubtitleProviderConfig::unconfigured().with_base_url("ftp://api.invalid");
    let (provider, _time) = provider_with(config, shared.clone());
    let capability = provider.capability();
    assert!(!capability.is_configured());
    assert!(capability.detail.contains("ftp"), "{}", capability.detail);
    assert_eq!(capability.provider, TSDB_PROVIDER);

    let capability = TsdbProvider::with_system_clock(
        SubtitleProviderConfig::unconfigured(),
        Box::new(TcpTransport::default()),
    )
    .capability();
    assert!(!capability.is_configured());
    assert!(capability.detail.contains("https"), "{}", capability.detail);

    let broken = SubtitleProviderConfig {
        configuration_error: Some("The subtitle key file cannot be read.".to_string()),
        ..SubtitleProviderConfig::unconfigured()
    };
    let capability = provider_with(broken, shared).0.capability();
    assert!(!capability.is_configured());
    assert!(
        capability.detail.contains("cannot be read"),
        "{}",
        capability.detail
    );
}

#[test]
fn a_keyed_provider_reports_both_lookups_and_search() {
    let time = FakeTime::new();
    let provider = TsdbProvider::with_parts(
        SubtitleProviderConfig::unconfigured().with_api_key("unit-test-key"),
        Box::new(TlsTransport(Shared::new(Vec::new()))),
        Box::new(time.clone()),
        Box::new(time),
    );

    let capability = provider.capability();

    assert!(capability.is_configured());
    assert_eq!(capability.provider, TSDB_PROVIDER);
    assert!(provider.search_is_keyed());
    assert!(
        capability.detail.contains("keyed title search"),
        "{}",
        capability.detail
    );
    assert_eq!(provider.provider_name(), "thesubtitledb");
}

static ENVIRONMENT_LOCK: Mutex<()> = Mutex::new(());

struct Environment {
    _guard: MutexGuard<'static, ()>,
}

impl Environment {
    fn take() -> Self {
        let guard = ENVIRONMENT_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let environment = Self { _guard: guard };
        for name in [
            TSDB_API_KEY_ENVIRONMENT,
            TSDB_BASE_URL_ENVIRONMENT,
            TSDB_CONFIG_ENVIRONMENT,
            TSDB_CONFIG_DIR_ENVIRONMENT,
        ] {
            environment.clear(name);
        }
        environment
    }

    fn set(&self, name: &str, value: impl AsRef<std::ffi::OsStr>) {
        unsafe { std::env::set_var(name, value) };
    }

    fn clear(&self, name: &str) {
        unsafe { std::env::remove_var(name) };
    }
}

fn write_key_file(directory: &Path, body: &str) {
    let path = directory.join(TSDB_KEY_FILE_NAME);
    std::fs::write(&path, body).expect("a key file");
    restrict_to_owner(&path);
}

fn write_open_key_file(directory: &Path, body: &str) {
    let path = directory.join(TSDB_KEY_FILE_NAME);
    std::fs::write(&path, body).expect("a key file");
    open_to_other_accounts(&path);
}

#[cfg(unix)]
fn restrict_to_owner(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).expect("a private key");
}

#[cfg(not(unix))]
fn restrict_to_owner(_path: &Path) {}

#[cfg(unix)]
fn open_to_other_accounts(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o644)).expect("a shared key");
}

#[cfg(not(unix))]
fn open_to_other_accounts(_path: &Path) {}

fn write_config_file(directory: &Path, body: &str) -> std::path::PathBuf {
    let path = directory.join("subtitles.conf");
    std::fs::write(&path, body).expect("a config file");
    path
}

#[test]
fn a_key_from_the_environment_wins_over_the_config_file_and_the_key_file() {
    let environment = Environment::take();
    let directory = TempDir::new().expect("temp dir");
    write_key_file(directory.path(), "key-file-value-1");
    let config_path = write_config_file(directory.path(), "api_key=config-file-key-1\n");
    environment.set(TSDB_API_KEY_ENVIRONMENT, "environment-key-1");
    environment.set(TSDB_CONFIG_ENVIRONMENT, &config_path);
    environment.set(TSDB_CONFIG_DIR_ENVIRONMENT, directory.path());

    let config = SubtitleProviderConfig::from_environment();

    assert_eq!(
        config.api_key.as_ref().map(Secret::expose),
        Some("environment-key-1")
    );
    assert!(config.configuration_error.is_none());
}

#[test]
fn a_key_from_the_config_file_wins_over_the_key_file() {
    let environment = Environment::take();
    let directory = TempDir::new().expect("temp dir");
    write_key_file(directory.path(), "key-file-value-1");
    let config_path = write_config_file(directory.path(), "api_key=config-file-key-1\n");
    environment.set(TSDB_CONFIG_ENVIRONMENT, &config_path);
    environment.set(TSDB_CONFIG_DIR_ENVIRONMENT, directory.path());

    let config = SubtitleProviderConfig::from_environment();

    assert_eq!(
        config.api_key.as_ref().map(Secret::expose),
        Some("config-file-key-1")
    );
    assert!(config.configuration_error.is_none());
}

#[test]
fn the_key_file_is_used_when_no_other_source_names_a_key() {
    let environment = Environment::take();
    let directory = TempDir::new().expect("temp dir");
    write_key_file(directory.path(), "  key-file-value-1\n");
    environment.set(TSDB_CONFIG_DIR_ENVIRONMENT, directory.path());

    let config = SubtitleProviderConfig::from_environment();

    assert_eq!(
        config.api_key.as_ref().map(Secret::expose),
        Some("key-file-value-1")
    );
    assert!(config.configuration_error.is_none());

    let explicit = SubtitleProviderConfig::from_configuration_dir(Some(directory.path()));
    assert_eq!(
        explicit.api_key.as_ref().map(Secret::expose),
        Some("key-file-value-1")
    );
}

#[test]
fn a_missing_key_file_leaves_lookups_anonymous_without_an_error() {
    let environment = Environment::take();
    let directory = TempDir::new().expect("temp dir");
    environment.set(TSDB_CONFIG_DIR_ENVIRONMENT, directory.path());

    let config = SubtitleProviderConfig::from_environment();
    assert!(config.api_key.is_none());
    assert!(config.configuration_error.is_none());

    let provider = TsdbProvider::new(config);
    assert!(provider.capability().is_configured());
    assert!(!provider.search_is_keyed());
}

#[test]
fn a_malformed_key_file_is_reported_truthfully_instead_of_falling_back() {
    for body in ["short", "has a space in it", "\u{7f}hidden-key-value"] {
        let environment = Environment::take();
        let directory = TempDir::new().expect("temp dir");
        write_key_file(directory.path(), body);
        environment.set(TSDB_CONFIG_DIR_ENVIRONMENT, directory.path());

        let config = SubtitleProviderConfig::from_environment();

        assert!(config.api_key.is_none(), "{body:?}");
        let error = config
            .configuration_error
            .as_ref()
            .expect("a rejected key file is an error");
        assert!(error.contains("key file"), "{error}");
        let provider = TsdbProvider::new(config);
        let capability = provider.capability();
        assert!(!capability.is_configured());
        assert!(
            capability.detail.contains("key file"),
            "{}",
            capability.detail
        );
    }
}

#[test]
fn an_oversized_or_non_regular_key_file_is_refused() {
    let environment = Environment::take();
    let directory = TempDir::new().expect("temp dir");
    std::fs::write(directory.path().join(TSDB_KEY_FILE_NAME), "k".repeat(2_048))
        .expect("a key file");
    environment.set(TSDB_CONFIG_DIR_ENVIRONMENT, directory.path());

    let config = SubtitleProviderConfig::from_environment();
    assert!(config.api_key.is_none());
    let error = config.configuration_error.expect("an oversized key file");
    assert!(error.contains("1024"), "{error}");

    let directory = TempDir::new().expect("temp dir");
    std::fs::create_dir(directory.path().join(TSDB_KEY_FILE_NAME)).expect("a directory");
    environment.set(TSDB_CONFIG_DIR_ENVIRONMENT, directory.path());

    let config = SubtitleProviderConfig::from_environment();
    assert!(config.api_key.is_none());
    let error = config
        .configuration_error
        .expect("a directory is not a key file");
    assert!(error.contains("regular file"), "{error}");
}

#[test]
fn a_key_file_other_accounts_can_read_is_refused() {
    let environment = Environment::take();
    let directory = TempDir::new().expect("temp dir");
    write_open_key_file(directory.path(), "key-file-value-1");
    environment.set(TSDB_CONFIG_DIR_ENVIRONMENT, directory.path());

    let config = SubtitleProviderConfig::from_environment();

    if cfg!(unix) {
        assert!(config.api_key.is_none());
        let error = config.configuration_error.expect("a shared key file");
        assert!(error.contains("chmod 600"), "{error}");
    } else {
        assert_eq!(
            config.api_key.as_ref().map(Secret::expose),
            Some("key-file-value-1")
        );
    }
}

#[test]
fn an_environment_key_survives_a_config_file_that_cannot_be_read() {
    let environment = Environment::take();
    let directory = TempDir::new().expect("temp dir");
    environment.set(TSDB_API_KEY_ENVIRONMENT, "environment-key-1");
    environment.set(
        TSDB_CONFIG_ENVIRONMENT,
        directory.path().join("missing.conf"),
    );
    environment.set(TSDB_CONFIG_DIR_ENVIRONMENT, directory.path());

    let config = SubtitleProviderConfig::from_environment();

    assert_eq!(
        config.api_key.as_ref().map(Secret::expose),
        Some("environment-key-1")
    );
    assert!(config.configuration_error.is_none());
}

#[test]
fn a_key_is_never_sent_over_plain_http() {
    let shared = Shared::new(vec![Reply::Answer(missing())]);
    let plain = config().with_api_key("unit-test-key");
    let (provider, _time) = provider_with(plain, shared.clone());

    assert!(!provider.search_is_keyed());
    let capability = provider.capability();
    assert!(!capability.is_configured());
    assert!(capability.detail.contains("https"), "{}", capability.detail);

    let _ = provider
        .search(&SubtitleSearch::movie("Harbour Lights", Some(2014), "bn"))
        .expect("the anonymous lookup still answers");

    assert!(
        shared.requests().iter().all(|request| !request
            .headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("authorization"))),
        "a plain http request must carry no credential"
    );
}

#[test]
fn a_hostile_year_does_not_overflow_the_match_check() {
    let shared = Shared::new(vec![
        Reply::Answer(json(
            "{\"title\": {\"media_type\": \"movie\", \"name\": \"Harbour Lights\", \
             \"year\": -9223372036854775808}, \"subtitles\": {\"total\": 0, \"items\": []}}",
        )),
        Reply::Answer(missing()),
    ]);
    let (provider, _time) = provider_with(config(), shared.clone());

    let matches = provider
        .search(&SubtitleSearch::movie("Harbour Lights", Some(2014), "bn").with_tmdb(76341))
        .expect("a hostile year is a miss, not a panic");

    assert!(matches.is_empty());
}

#[test]
fn a_malformed_environment_key_is_reported_and_never_silently_anonymous() {
    let environment = Environment::take();
    let directory = TempDir::new().expect("temp dir");
    environment.set(TSDB_API_KEY_ENVIRONMENT, "bad");
    environment.set(TSDB_CONFIG_DIR_ENVIRONMENT, directory.path());

    let config = SubtitleProviderConfig::from_environment();

    assert!(config.api_key.is_none());
    let error = config.configuration_error.expect("a short key is rejected");
    assert!(error.contains("subtitle key"), "{error}");
}
