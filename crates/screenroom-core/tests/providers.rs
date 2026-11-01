use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;

use screenroom_core::{
    ApiCredential, ArtworkRetention, CatalogueMetadataProvider, CatalogueWork, Clock,
    EnrichmentState, EnrichmentTarget, HttpRequest, HttpResponse, HttpTransport, ImageSize, Ingest,
    Library, LibraryQuery, MediaKind, MetadataOrigin, MetadataQuery, ProviderConfig, ProviderImage,
    ReconcileMode, Secret, Sleeper, TcpTransport, TmdbProvider, TransportError, parse_filename,
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

    fn push(&self, reply: Reply) {
        self.replies.borrow_mut().push_back(reply);
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

    fn now_ms(&self) -> i64 {
        self.now.get()
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

impl Sleeper for FakeTime {
    fn sleep_ms(&self, millis: i64) {
        self.sleeps.borrow_mut().push(millis);
        self.now.set(self.now.get() + millis);
    }
}

fn json(body: &str) -> HttpResponse {
    HttpResponse::new(200, body.as_bytes().to_vec())
}

fn refuses(status: u16) -> HttpResponse {
    HttpResponse::new(status, Vec::new())
}

fn configured() -> ProviderConfig {
    ProviderConfig::configured(ApiCredential::ApiKey(Secret::new("unit-test-key")))
        .with_base_url("http://provider.invalid/3")
        .with_image_base_url("http://images.invalid/t/p")
        .with_budget_per_minute(600)
}

fn https_config() -> ProviderConfig {
    ProviderConfig::configured(ApiCredential::ApiKey(Secret::new("unit-test-key")))
}

fn provider_with(config: ProviderConfig, shared: Rc<Shared>) -> (TmdbProvider, FakeTime) {
    let time = FakeTime::new();
    let provider = TmdbProvider::with_parts(
        config,
        Box::new(FakeTransport(shared)),
        Box::new(time.clone()),
        Box::new(time.clone()),
    );
    (provider, time)
}

fn library_with(names: &[&str]) -> (TempDir, Library) {
    let directory = TempDir::new().expect("temp dir");
    let library = Library::open_in_memory().expect("library");
    let source = library.add_source(directory.path(), None).expect("source");
    for name in names {
        let path = directory.path().join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("folders");
        }
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
    }
    (directory, library)
}

fn work_id(library: &Library, relative: &str) -> String {
    library
        .items(&LibraryQuery {
            text: relative.to_string(),
            ..LibraryQuery::page(10)
        })
        .expect("items")
        .into_iter()
        .next()
        .expect("an item")
        .work_id
}

fn jpeg(width: u16, height: u16) -> Vec<u8> {
    let mut bytes = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x04, 0x00, 0x00];
    bytes.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x0B, 0x08]);
    bytes.extend_from_slice(&height.to_be_bytes());
    bytes.extend_from_slice(&width.to_be_bytes());
    bytes.extend_from_slice(&[0x01, 0x01, 0x11, 0x00]);
    bytes.extend_from_slice(&[0x00; 8]);
    bytes
}

#[test]
fn a_tmdb_search_reads_the_documented_shape_and_asks_for_title_and_year() {
    let shared = Shared::new(vec![Reply::Answer(json(include_str!(
        "fixtures/providers/tmdb_search_movie.json"
    )))]);
    let (provider, _time) = provider_with(configured(), shared.clone());

    let matches = provider
        .search(&MetadataQuery {
            title: "The Lantern Keeper".to_string(),
            year: Some(1998),
            kind: MediaKind::Movie,
        })
        .expect("search");

    assert_eq!(matches.len(), 2);
    assert_eq!(matches[0].external_id, "55001");
    assert_eq!(matches[0].title, "The Lantern Keeper");
    assert_eq!(matches[0].year, Some(1998));
    assert_eq!(
        matches[0].poster_path.as_deref(),
        Some("/poster-lantern.jpg")
    );
    assert_eq!(
        matches[0].backdrop_path.as_deref(),
        Some("/backdrop-lantern.jpg")
    );
    assert_eq!(
        matches[0].overview,
        "A keeper tends a light that never goes out."
    );
    assert_eq!(matches[1].year, Some(2004));

    let request = shared.last_request();
    assert!(
        request
            .url
            .starts_with("http://provider.invalid/3/search/movie?"),
        "{}",
        request.url
    );
    assert!(request.url.contains("query=The%20Lantern%20Keeper"));
    assert!(request.url.contains("year=1998"));
    assert!(request.url.contains("language=en-US"));
}

#[test]
fn a_tmdb_movie_detail_carries_genres_runtime_and_resolved_artwork_paths() {
    let shared = Shared::new(vec![Reply::Answer(json(include_str!(
        "fixtures/providers/tmdb_movie_detail.json"
    )))]);
    let (provider, _time) = provider_with(configured(), shared.clone());

    let work = provider.work("55001", MediaKind::Movie).expect("detail");

    assert_eq!(work.external_id, "55001");
    assert_eq!(work.title, "The Lantern Keeper");
    assert_eq!(work.original_title, "The Lantern Keeper");
    assert_eq!(work.year, Some(1998));
    assert_eq!(work.genres, vec!["Drama", "Mystery"]);
    assert_eq!(work.runtime_ms, 112 * 60_000);
    assert!(work.overview.starts_with("A keeper tends a light"));

    assert_eq!(
        provider.image_url("/poster-lantern.jpg", ImageSize::Poster),
        Some("http://images.invalid/t/p/w500/poster-lantern.jpg".to_string())
    );
    assert_eq!(
        provider.image_url("/poster-lantern.jpg", ImageSize::Thumbnail),
        Some("http://images.invalid/t/p/w185/poster-lantern.jpg".to_string())
    );
    assert!(
        provider
            .image_url("https://other.invalid/x.jpg", ImageSize::Poster)
            .is_none()
    );
    assert!(
        provider
            .image_url("/../private", ImageSize::Poster)
            .is_none()
    );
    assert!(provider.image_url("", ImageSize::Poster).is_none());
}

#[test]
fn series_season_and_episode_details_parse_from_the_documented_shapes() {
    let shared = Shared::new(vec![
        Reply::Answer(json(include_str!("fixtures/providers/tmdb_tv_detail.json"))),
        Reply::Answer(json(include_str!(
            "fixtures/providers/tmdb_season_detail.json"
        ))),
        Reply::Answer(json(include_str!(
            "fixtures/providers/tmdb_episode_detail.json"
        ))),
    ]);
    let (provider, _time) = provider_with(configured(), shared.clone());

    let series = provider.series("9001").expect("series");
    assert_eq!(series.title, "Harbour Lights");
    assert_eq!(series.year, Some(2014));
    assert_eq!(series.genres, vec!["Drama"]);
    assert_eq!(series.seasons.len(), 2);
    assert_eq!(series.seasons[0].number, 1);
    assert_eq!(series.seasons[0].episode_count, 8);
    assert_eq!(series.seasons[1].year, Some(2015));

    let season = provider.season("9001", 1).expect("season");
    assert_eq!(season.number, 1);
    assert_eq!(season.title, "Season 1");
    assert_eq!(season.episodes.len(), 2);
    assert_eq!(season.episodes[0].title, "The First Tide");
    assert_eq!(season.episodes[1].number, 2);
    assert_eq!(season.episodes[1].runtime_ms, 49 * 60_000);

    let episode = provider.episode("9001", 1, 2).expect("episode");
    assert_eq!(episode.number, 2);
    assert_eq!(episode.title, "Low Water");
    assert_eq!(episode.season, 1);

    let requested: Vec<String> = shared
        .requests()
        .into_iter()
        .map(|request| request.url)
        .collect();
    assert!(requested[0].contains("/tv/9001?"));
    assert!(requested[1].contains("/tv/9001/season/1?"));
    assert!(requested[2].contains("/tv/9001/season/1/episode/2?"));
}

#[test]
fn a_transport_failure_is_retried_with_backoff_and_then_reported() {
    let shared = Shared::new(vec![
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
    let (provider, time) = provider_with(configured(), shared.clone());

    let error = provider
        .work("55001", MediaKind::Movie)
        .expect_err("a reset connection is an error");

    assert_eq!(error.code(), "provider_transport");
    assert_eq!(time.sleeps(), vec![500, 1_000]);
    assert_eq!(shared.request_count(), 3);
}

#[test]
fn a_transport_failure_parks_the_request_until_the_network_returns() {
    let (_directory, library) = library_with(&["Lantern.Keeper.1998.mkv"]);
    let work = work_id(&library, "Lantern.Keeper.1998.mkv");
    let reset = || {
        Reply::Fail(TransportError::Io(std::io::Error::new(
            std::io::ErrorKind::ConnectionReset,
            "reset",
        )))
    };
    let shared = Shared::new(vec![reset(), reset(), reset()]);
    let (provider, time) = provider_with(configured(), shared.clone());
    let task = library
        .enqueue_enrichment(&work, EnrichmentTarget::Work, "en-US", 0)
        .expect("queued");
    let start = time.now_ms();

    let report = library
        .drain_enrichment_queue(&provider, 10, start)
        .expect("offline drain");

    assert_eq!(report.waiting_network, 1);
    assert_eq!(report.failed, 0);
    assert_eq!(report.remaining, 1);
    let stored = library
        .enrichment_task(&task)
        .expect("task")
        .expect("present");
    assert_eq!(stored.state, EnrichmentState::WaitingNetwork);
    assert_eq!(stored.next_attempt_ms, start + 500);
    assert!(
        stored
            .last_error
            .as_deref()
            .unwrap_or_default()
            .contains("could not be made")
    );

    shared.push(Reply::Answer(json(include_str!(
        "fixtures/providers/tmdb_search_movie.json"
    ))));
    shared.push(Reply::Answer(json(include_str!(
        "fixtures/providers/tmdb_movie_detail.json"
    ))));

    let recovered = library
        .drain_enrichment_queue(&provider, 10, stored.next_attempt_ms)
        .expect("recovered drain");

    assert_eq!(recovered.succeeded, 1);
    assert_eq!(recovered.remaining, 0);
    assert_eq!(
        library.work_metadata(&work).expect("metadata").title,
        "The Lantern Keeper"
    );
}

#[test]
fn an_unconfigured_provider_says_so_and_never_touches_the_network() {
    let shared = Shared::new(Vec::new());
    let (provider, _time) = provider_with(ProviderConfig::unconfigured(), shared.clone());

    let capability = provider.capability();
    assert!(!capability.is_configured());
    assert!(capability.detail.contains("not configured"));
    assert_eq!(capability.provider, "tmdb");

    let error = provider
        .search(&MetadataQuery {
            title: "Anything".to_string(),
            year: None,
            kind: MediaKind::Movie,
        })
        .expect_err("an unconfigured provider must refuse");
    assert_eq!(error.code(), "provider_not_configured");
    assert_eq!(shared.request_count(), 0);
}

#[test]
fn an_https_endpoint_is_refused_by_the_bare_transport_with_a_clear_reason() {
    let transport = TcpTransport::default();
    let request = HttpRequest::get("https://api.themoviedb.org/3/movie/55001");
    let error = transport
        .send(&request)
        .expect_err("https has no transport");
    assert!(matches!(error, TransportError::UnsupportedScheme(_)));
    let message = error.to_string();
    assert!(message.contains("TLS"), "{message}");
}

#[test]
fn a_rate_limited_answer_waits_for_retry_after_and_then_succeeds() {
    let shared = Shared::new(vec![
        Reply::Answer(refuses(429).with_header("Retry-After", "2")),
        Reply::Answer(json(include_str!(
            "fixtures/providers/tmdb_movie_detail.json"
        ))),
    ]);
    let (provider, time) = provider_with(configured(), shared.clone());

    let work = provider.work("55001", MediaKind::Movie).expect("retried");

    assert_eq!(work.title, "The Lantern Keeper");
    assert_eq!(time.sleeps(), vec![2_000]);
    assert_eq!(shared.request_count(), 2);
}

#[test]
fn retries_back_off_exponentially_and_stop_at_the_configured_maximum() {
    let shared = Shared::new(vec![
        Reply::Answer(refuses(500)),
        Reply::Answer(refuses(500)),
        Reply::Answer(refuses(500)),
    ]);
    let (provider, time) = provider_with(configured(), shared.clone());

    let error = provider
        .work("55001", MediaKind::Movie)
        .expect_err("a server error survives no retries");

    assert_eq!(error.code(), "provider_server");
    assert_eq!(time.sleeps(), vec![500, 1_000]);
    assert_eq!(shared.request_count(), 3);
}

#[test]
fn the_request_budget_makes_a_third_call_wait_out_the_minute() {
    let shared = Shared::new(vec![
        Reply::Answer(json(include_str!(
            "fixtures/providers/tmdb_movie_detail.json"
        ))),
        Reply::Answer(json(include_str!(
            "fixtures/providers/tmdb_movie_detail.json"
        ))),
        Reply::Answer(json(include_str!(
            "fixtures/providers/tmdb_movie_detail.json"
        ))),
    ]);
    let (provider, time) = provider_with(configured().with_budget_per_minute(2), shared.clone());

    for _ in 0..3 {
        provider.work("55001", MediaKind::Movie).expect("work");
    }

    assert_eq!(time.sleeps(), vec![60_000]);
    assert_eq!(shared.request_count(), 3);
}

#[test]
fn the_configured_key_never_reaches_an_error_a_debug_string_or_a_stored_row() {
    let key = "unit-test-secret-key-0123456789";
    let config = ProviderConfig::configured(ApiCredential::ApiKey(Secret::new(key)))
        .with_base_url("http://provider.invalid/3");
    let shared = Shared::new(vec![Reply::Answer(refuses(401))]);
    let (provider, _time) = provider_with(config, shared.clone());

    let error = provider
        .work("55001", MediaKind::Movie)
        .expect_err("a rejected key is an error");
    assert_eq!(error.code(), "provider_unauthorized");

    let rendered = format!("{error} {error:?} {provider:?}");
    assert!(!rendered.contains(key), "{rendered}");

    let request = shared.last_request();
    assert!(request.url.contains("api_key="));
    assert!(!request.redacted_url().contains(key));
    assert!(request.redacted_url().contains("api_key=redacted"));
    assert!(!format!("{request:?}").contains(key));
}

#[test]
fn provider_metadata_lands_as_provider_origin_and_records_the_external_identity() {
    let (_directory, library) = library_with(&["Lantern.Keeper.1998.mkv"]);
    let work = work_id(&library, "Lantern.Keeper.1998.mkv");
    let shared = Shared::new(vec![
        Reply::Answer(json(include_str!(
            "fixtures/providers/tmdb_search_movie.json"
        ))),
        Reply::Answer(json(include_str!(
            "fixtures/providers/tmdb_movie_detail.json"
        ))),
    ]);
    let (provider, time) = provider_with(configured(), shared.clone());
    let task = library
        .enqueue_enrichment(&work, EnrichmentTarget::Work, "en-US", 0)
        .expect("queued");

    let report = library
        .drain_enrichment_queue(&provider, 10, time.now_ms())
        .expect("drain");

    assert!(report.configured);
    assert_eq!(report.attempted, 1);
    assert_eq!(report.succeeded, 1);
    assert_eq!(report.failed, 0);
    assert_eq!(report.remaining, 0);

    let snapshot = library.work_metadata(&work).expect("metadata");
    assert_eq!(snapshot.title, "The Lantern Keeper");
    assert_eq!(snapshot.year, Some(1998));
    assert_eq!(snapshot.origin, MetadataOrigin::Provider);
    assert_eq!(snapshot.genres, vec!["Drama", "Mystery"]);
    assert_eq!(
        library
            .provider_identity(&work, "tmdb")
            .expect("identity")
            .as_deref(),
        Some("55001")
    );
    assert_eq!(
        library
            .enrichment_task(&task)
            .expect("task")
            .expect("present")
            .state,
        EnrichmentState::Done
    );
}

#[test]
fn a_manual_match_survives_a_drain_and_the_provider_is_not_asked() {
    let (_directory, library) = library_with(&["Chosen.2020.mkv"]);
    let work = work_id(&library, "Chosen.2020.mkv");
    library.set_manual_match(&work, true).expect("manual");
    let shared = Shared::new(Vec::new());
    let (provider, time) = provider_with(configured(), shared.clone());

    library
        .enqueue_enrichment(&work, EnrichmentTarget::Work, "en-US", 0)
        .expect("queued");
    let report = library
        .drain_enrichment_queue(&provider, 10, time.now_ms())
        .expect("drain");

    assert_eq!(report.succeeded, 1);
    assert_eq!(shared.request_count(), 0);
    let snapshot = library.work_metadata(&work).expect("metadata");
    assert_eq!(snapshot.origin, MetadataOrigin::Manual);
    assert_eq!(snapshot.title, "Chosen");
    assert_eq!(
        library.known_genres().expect("genres"),
        Vec::<String>::new()
    );
}

#[test]
fn an_nfo_title_beats_the_provider_in_the_background_and_yields_when_the_owner_asks() {
    let (directory, library) = library_with(&["Some.Release.2019.mkv"]);
    std::fs::write(
        directory.path().join("Some.Release.2019.nfo"),
        b"<movie><title>Sidecar Title</title><genre>Drama</genre></movie>",
    )
    .expect("sidecar");
    library
        .enrich_from_sidecars(None)
        .expect("sidecar enrichment");
    let work = work_id(&library, "Some.Release.2019.mkv");

    let candidate = CatalogueWork {
        external_id: "55001".to_string(),
        title: "Provider Title".to_string(),
        original_title: "Provider Title".to_string(),
        year: Some(2019),
        overview: "From the provider.".to_string(),
        genres: vec!["Mystery".to_string()],
        runtime_ms: 0,
        poster_path: None,
        backdrop_path: None,
    };

    let blocked = library
        .apply_catalogue_work(&work, &candidate, "tmdb", ReconcileMode::Background)
        .expect("background");
    assert!(!blocked.applied);
    assert_eq!(blocked.blocked_by, Some(MetadataOrigin::Nfo));
    assert_eq!(
        library.work_metadata(&work).expect("read").title,
        "Sidecar Title"
    );
    assert_eq!(
        library.known_genres().expect("genres"),
        vec!["Drama".to_string()]
    );

    let applied = library
        .apply_catalogue_work(&work, &candidate, "tmdb", ReconcileMode::OwnerRequested)
        .expect("owner");
    assert!(applied.applied);
    assert_eq!(applied.origin, MetadataOrigin::Provider);
    let snapshot = library.work_metadata(&work).expect("read");
    assert_eq!(snapshot.title, "Provider Title");
    assert_eq!(snapshot.genres, vec!["Mystery"]);
    assert_eq!(
        library
            .provider_identity(&work, "tmdb")
            .expect("identity")
            .as_deref(),
        Some("55001")
    );
}

#[test]
fn provider_artwork_is_kept_only_when_the_retention_setting_is_on() {
    let (_directory, library) = library_with(&["Lantern.Keeper.1998.mkv"]);
    let work = work_id(&library, "Lantern.Keeper.1998.mkv");
    let image = ProviderImage {
        bytes: jpeg(320, 180),
        mime: "image/jpeg".to_string(),
    };

    assert_eq!(
        library.artwork_retention().expect("retention"),
        ArtworkRetention::DoNotStore
    );
    assert!(!library.apply_provider_artwork(&work, &image).expect("gate"));
    assert!(library.artwork(&work).expect("read").is_none());

    library
        .set_artwork_retention(ArtworkRetention::KeepLocally)
        .expect("enable");
    assert!(
        library
            .apply_provider_artwork(&work, &image)
            .expect("store")
    );
    let stored = library.artwork(&work).expect("read").expect("present");
    assert_eq!(stored.bytes, image.bytes);
    assert_eq!((stored.width, stored.height), (320, 180));
    assert_eq!(stored.mime, "image/jpeg");
}

#[test]
fn a_drain_with_the_retention_setting_on_keeps_the_poster_bytes() {
    let (_directory, library) = library_with(&["Lantern.Keeper.1998.mkv"]);
    let work = work_id(&library, "Lantern.Keeper.1998.mkv");
    library
        .set_artwork_retention(ArtworkRetention::KeepLocally)
        .expect("enable");
    let shared = Shared::new(vec![
        Reply::Answer(json(include_str!(
            "fixtures/providers/tmdb_search_movie.json"
        ))),
        Reply::Answer(json(include_str!(
            "fixtures/providers/tmdb_movie_detail.json"
        ))),
        Reply::Answer(
            HttpResponse::new(200, jpeg(500, 750)).with_header("Content-Type", "image/jpeg"),
        ),
    ]);
    let (provider, time) = provider_with(configured(), shared.clone());

    library
        .enqueue_enrichment(&work, EnrichmentTarget::Work, "en-US", 0)
        .expect("queued");
    let report = library
        .drain_enrichment_queue(&provider, 10, time.now_ms())
        .expect("drain");

    assert_eq!(report.succeeded, 1);
    assert_eq!(shared.request_count(), 3);
    assert!(
        shared
            .last_request()
            .url
            .starts_with("http://images.invalid/t/p/w500/poster-lantern.jpg")
    );
    let stored = library.artwork(&work).expect("read").expect("present");
    assert_eq!((stored.width, stored.height), (500, 750));
}

#[test]
fn an_enqueued_request_survives_a_reopen_and_drains_later() {
    let directory = TempDir::new().expect("temp dir");
    let path = directory.path().join("library.db");
    let media = directory.path().join("media");
    std::fs::create_dir_all(&media).expect("media");

    let work = {
        let library = Library::open(&path).expect("library");
        let source = library.add_source(&media, None).expect("source");
        std::fs::write(media.join("Lantern.Keeper.1998.mkv"), b"video").expect("write");
        let parsed = parse_filename("Lantern.Keeper.1998.mkv", None);
        library
            .ingest_file(Ingest {
                source_id: &source.id,
                relative_path: "Lantern.Keeper.1998.mkv",
                fingerprint: "lantern",
                size: 5,
                modified_ms: 1,
                scan_id: "scan-1",
                parsed: Some(&parsed),
                expected_root: Some(&source.root_path),
            })
            .expect("ingest");
        let work = work_id(&library, "Lantern.Keeper.1998.mkv");
        library
            .enqueue_enrichment(&work, EnrichmentTarget::Work, "en-US", 0)
            .expect("queued");
        work
    };

    let library = Library::open(&path).expect("reopened");
    assert_eq!(library.enrichment_queue_depth().expect("depth").queued, 1);

    let shared = Shared::new(vec![
        Reply::Answer(json(include_str!(
            "fixtures/providers/tmdb_search_movie.json"
        ))),
        Reply::Answer(json(include_str!(
            "fixtures/providers/tmdb_movie_detail.json"
        ))),
    ]);
    let (provider, time) = provider_with(configured(), shared.clone());
    let report = library
        .drain_enrichment_queue(&provider, 10, time.now_ms())
        .expect("drain");

    assert_eq!(report.succeeded, 1);
    assert_eq!(
        library.work_metadata(&work).expect("metadata").title,
        "The Lantern Keeper"
    );
}

#[test]
fn the_whole_library_is_queued_and_a_favourite_comes_first() {
    let (_directory, library) = library_with(&["Plain.2001.mkv", "Favourite.2002.mkv"]);
    let favourite = work_id(&library, "Favourite.2002.mkv");
    let profile = library.profiles().expect("profiles").remove(0);
    library
        .set_favorite(&profile.id, &favourite, true)
        .expect("favourite");

    let queued = library.enqueue_whole_library("en-US").expect("queue");
    assert_eq!(queued, 2);

    let due = library
        .due_enrichment_tasks(i64::MAX, 10)
        .expect("due tasks");
    assert_eq!(due.len(), 2);
    assert_eq!(due[0].work_id, favourite);
    assert!(due[0].priority > due[1].priority);

    assert_eq!(library.enqueue_whole_library("en-US").expect("again"), 0);
}

#[test]
fn a_rate_limited_drain_stops_and_keeps_the_request_for_later() {
    let (_directory, library) = library_with(&["Lantern.Keeper.1998.mkv"]);
    let work = work_id(&library, "Lantern.Keeper.1998.mkv");
    let shared = Shared::new(vec![
        Reply::Answer(refuses(429).with_header("Retry-After", "30")),
        Reply::Answer(refuses(429).with_header("Retry-After", "30")),
        Reply::Answer(refuses(429).with_header("Retry-After", "30")),
    ]);
    let (provider, time) = provider_with(configured(), shared.clone());
    let task = library
        .enqueue_enrichment(&work, EnrichmentTarget::Work, "en-US", 0)
        .expect("queued");
    let start = time.now_ms();

    let report = library
        .drain_enrichment_queue(&provider, 10, start)
        .expect("drain");

    assert_eq!(report.rate_limited, 1);
    assert_eq!(report.succeeded, 0);
    assert_eq!(report.remaining, 1);
    let stored = library
        .enrichment_task(&task)
        .expect("task")
        .expect("present");
    assert_eq!(stored.state, EnrichmentState::RateLimited);
    assert_eq!(stored.next_attempt_ms, start + 30_000);
    assert_eq!(
        library.work_metadata(&work).expect("metadata").origin,
        MetadataOrigin::Legacy
    );

    let early = library
        .drain_enrichment_queue(&provider, 10, stored.next_attempt_ms - 1)
        .expect("early drain");
    assert_eq!(early.attempted, 0);
}

#[test]
fn a_failing_request_backs_off_then_gives_up_with_a_plain_reason() {
    let (_directory, library) = library_with(&["Lantern.Keeper.1998.mkv"]);
    let work = work_id(&library, "Lantern.Keeper.1998.mkv");
    let shared = Shared::new(vec![
        Reply::Answer(json("not json")),
        Reply::Answer(json("not json")),
        Reply::Answer(json("not json")),
    ]);
    let (provider, time) = provider_with(configured(), shared.clone());
    let task = library
        .enqueue_enrichment(&work, EnrichmentTarget::Work, "en-US", 0)
        .expect("queued");

    let first = library
        .drain_enrichment_queue(&provider, 10, time.now_ms())
        .expect("first");
    assert_eq!(first.failed, 1);
    let stored = library
        .enrichment_task(&task)
        .expect("task")
        .expect("present");
    assert_eq!(stored.state, EnrichmentState::Queued);
    assert_eq!(stored.attempts, 1);
    assert!(stored.next_attempt_ms > time.now_ms());
    assert!(
        stored
            .last_error
            .as_deref()
            .unwrap_or_default()
            .contains("unexpected answer")
    );

    let second = library
        .drain_enrichment_queue(&provider, 10, stored.next_attempt_ms)
        .expect("second");
    assert_eq!(second.failed, 1);
    let stored = library
        .enrichment_task(&task)
        .expect("task")
        .expect("present");
    assert_eq!(stored.attempts, 2);

    let third = library
        .drain_enrichment_queue(&provider, 10, stored.next_attempt_ms)
        .expect("third");
    assert_eq!(third.failed, 1);
    let stored = library
        .enrichment_task(&task)
        .expect("task")
        .expect("present");
    assert_eq!(stored.state, EnrichmentState::Failed);
    assert_eq!(stored.attempts, 3);
    assert_eq!(stored.next_attempt_ms, 0);
}

#[test]
fn an_unconfigured_drain_leaves_the_queue_alone_and_reports_why() {
    let (_directory, library) = library_with(&["Lantern.Keeper.1998.mkv"]);
    let work = work_id(&library, "Lantern.Keeper.1998.mkv");
    let shared = Shared::new(Vec::new());
    let (provider, time) = provider_with(ProviderConfig::unconfigured(), shared.clone());
    let task = library
        .enqueue_enrichment(&work, EnrichmentTarget::Work, "en-US", 0)
        .expect("queued");

    let report = library
        .drain_enrichment_queue(&provider, 10, time.now_ms())
        .expect("drain");

    assert!(!report.configured);
    assert_eq!(report.attempted, 0);
    assert_eq!(report.remaining, 1);
    assert_eq!(shared.request_count(), 0);
    assert_eq!(
        library
            .enrichment_task(&task)
            .expect("task")
            .expect("present")
            .state,
        EnrichmentState::Queued
    );
}

#[test]
fn a_series_season_task_names_the_season_and_its_episodes() {
    let (_directory, library) = library_with(&["Harbour.Lights.S01E02.mkv"]);
    let work = work_id(&library, "Harbour.Lights.S01E02.mkv");
    let shared = Shared::new(vec![
        Reply::Answer(json(include_str!("fixtures/providers/tmdb_search_tv.json"))),
        Reply::Answer(json(include_str!("fixtures/providers/tmdb_tv_detail.json"))),
        Reply::Answer(json(include_str!(
            "fixtures/providers/tmdb_season_detail.json"
        ))),
    ]);
    let (provider, time) = provider_with(configured(), shared.clone());
    let task = library
        .enqueue_enrichment(&work, EnrichmentTarget::Season { season: 1 }, "en-US", 0)
        .expect("queued");
    assert_eq!(
        screenroom_core::describe_enrichment_target(
            library
                .enrichment_task(&task)
                .expect("task")
                .expect("present")
                .target
        ),
        "season 1"
    );

    let report = library
        .drain_enrichment_queue(&provider, 10, time.now_ms())
        .expect("drain");

    assert_eq!(report.succeeded, 1);
    assert_eq!(
        library.work_metadata(&work).expect("metadata").title,
        "Harbour Lights"
    );
}

#[test]
fn reconcile_never_lets_a_provider_overwrite_a_manual_match() {
    let manual = screenroom_core::MetadataSnapshot {
        origin: MetadataOrigin::Manual,
        manual_match: true,
        title: "Mine".to_string(),
        year: Some(1998),
        overview: String::new(),
        genres: Vec::new(),
    };
    let patch = screenroom_core::MetadataPatch {
        title: Some("Theirs".to_string()),
        year: None,
        overview: None,
        genres: None,
    };
    let decision = screenroom_core::reconcile(&manual, &patch, ReconcileMode::OwnerRequested);
    assert!(!decision.applied);
    assert_eq!(decision.blocked_by, Some(MetadataOrigin::Manual));
}

fn exhausted_library() -> (TempDir, Library, String, Rc<Shared>, FakeTime) {
    let (directory, library) = library_with(&["Lantern.Keeper.1998.mkv"]);
    let work = work_id(&library, "Lantern.Keeper.1998.mkv");
    let shared = Shared::new(vec![
        Reply::Answer(json("not json")),
        Reply::Answer(json("not json")),
        Reply::Answer(json("not json")),
        Reply::Answer(json("not json")),
    ]);
    let (provider, time) = provider_with(configured(), shared.clone());
    let task = library
        .enqueue_enrichment(&work, EnrichmentTarget::Work, "en-US", 0)
        .expect("queued");
    let mut at = time.now_ms();
    for _ in 0..3 {
        library
            .drain_enrichment_queue(&provider, 10, at)
            .expect("drain");
        at = library
            .enrichment_task(&task)
            .expect("task")
            .expect("present")
            .next_attempt_ms;
    }
    let stored = library
        .enrichment_task(&task)
        .expect("task")
        .expect("present");
    assert_eq!(stored.state, EnrichmentState::Failed);
    assert_eq!(stored.attempts, 3);
    (directory, library, work, shared, time)
}

#[test]
fn a_task_that_exhausted_its_attempts_can_be_enqueued_again_and_restarts() {
    let (_directory, library, work, shared, time) = exhausted_library();
    assert!(
        library
            .due_enrichment_tasks(time.now_ms(), 10)
            .expect("due")
            .is_empty(),
        "an exhausted task must not be picked up as if it were due"
    );

    let rearmed = library
        .enqueue_enrichment(&work, EnrichmentTarget::Work, "en-US", 0)
        .expect("re-armed");
    let stored = library
        .enrichment_task(&rearmed)
        .expect("task")
        .expect("present");
    assert_eq!(stored.state, EnrichmentState::Queued);
    assert_eq!(stored.attempts, 0);
    assert_eq!(stored.next_attempt_ms, 0);
    assert!(stored.last_error.is_none());

    let due = library
        .due_enrichment_tasks(time.now_ms(), 10)
        .expect("due");
    assert_eq!(due.len(), 1);
    assert_eq!(due[0].id, rearmed);
    assert_eq!(library.enrichment_queue_depth().expect("depth").failed, 0);

    let (provider, _time) = provider_with(configured(), shared.clone());
    library
        .drain_enrichment_queue(&provider, 10, time.now_ms())
        .expect("drain after retry");
    let stored = library
        .enrichment_task(&rearmed)
        .expect("task")
        .expect("present");
    assert_eq!(
        stored.attempts, 1,
        "the retry budget must restart rather than resume spent attempts"
    );
    assert_eq!(stored.state, EnrichmentState::Queued);
}

#[test]
fn a_whole_library_pass_rearms_an_exhausted_task() {
    let (_directory, library, _work, _shared, _time) = exhausted_library();

    let queued = library.enqueue_whole_library("en-US").expect("queue");

    assert_eq!(queued, 1);
    let depth = library.enrichment_queue_depth().expect("depth");
    assert_eq!(depth.queued, 1);
    assert_eq!(depth.failed, 0);
}

#[test]
fn capability_fails_closed_when_the_base_address_needs_a_transport_it_does_not_have() {
    let capability = TmdbProvider::new(https_config()).capability();
    assert!(!capability.is_configured());
    assert!(capability.detail.contains("https"), "{}", capability.detail);
    assert_eq!(capability.provider, "tmdb");

    let shared = Shared::new(Vec::new());
    let (provider, _time) = provider_with(https_config(), shared.clone());
    let guarded = provider.capability();
    assert!(!guarded.is_configured());
    assert!(guarded.detail.contains("https"), "{}", guarded.detail);
    assert_eq!(shared.request_count(), 0);

    let (plain, _time) = provider_with(configured(), shared);
    assert!(plain.capability().is_configured());
}

#[test]
fn capability_reports_configured_when_a_tls_capable_transport_is_injected() {
    let shared = Shared::new(Vec::new());
    let time = FakeTime::new();
    let provider = TmdbProvider::with_parts(
        https_config(),
        Box::new(TlsTransport(shared.clone())),
        Box::new(time.clone()),
        Box::new(time),
    );

    let capability = provider.capability();
    assert!(capability.is_configured());
    assert_eq!(capability.language, "en-US");
    assert_eq!(shared.request_count(), 0);
}

#[test]
fn a_schema_eight_library_gains_the_queue_and_keeps_its_rows() {
    let directory = TempDir::new().expect("temp dir");
    let path = directory.path().join("library.db");
    let media = directory.path().join("media");
    std::fs::create_dir_all(&media).expect("media");

    let work = {
        let library = Library::open(&path).expect("fresh library");
        let source = library.add_source(&media, None).expect("source");
        std::fs::write(media.join("Lantern.Keeper.1998.mkv"), b"video").expect("write");
        let parsed = parse_filename("Lantern.Keeper.1998.mkv", None);
        library
            .ingest_file(Ingest {
                source_id: &source.id,
                relative_path: "Lantern.Keeper.1998.mkv",
                fingerprint: "lantern",
                size: 5,
                modified_ms: 1,
                scan_id: "scan-1",
                parsed: Some(&parsed),
                expected_root: Some(&source.root_path),
            })
            .expect("ingest");
        work_id(&library, "Lantern.Keeper.1998.mkv")
    };

    {
        let connection = rusqlite::Connection::open(&path).expect("open");
        connection
            .execute("DROP TABLE enrichment_queue", [])
            .expect("drop the queue table");
        connection
            .pragma_update(None, "user_version", 8)
            .expect("stamp version eight");
    }

    let library = Library::open(&path).expect("migrated");
    assert_eq!(
        library.work_metadata(&work).expect("kept work").title,
        "Lantern Keeper"
    );
    assert_eq!(library.enrichment_queue_depth().expect("depth").queued, 0);
    let task = library
        .enqueue_enrichment(&work, EnrichmentTarget::Work, "en-US", 0)
        .expect("the queue accepts work after the migration");
    let due = library.due_enrichment_tasks(i64::MAX, 10).expect("due");
    assert_eq!(due.len(), 1);
    assert_eq!(due[0].id, task);
    drop(library);

    let connection = rusqlite::Connection::open(&path).expect("reopen");
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("version");
    assert_eq!(version, screenroom_core::SCHEMA_VERSION);
    assert_eq!(version, 9);
    for name in [
        "enrichment_queue",
        "enrichment_queue_active",
        "enrichment_queue_due",
    ] {
        let found: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE name = ?1",
                [name],
                |row| row.get(0),
            )
            .expect("object lookup");
        assert_eq!(found, 1, "{name} is missing after the migration");
    }
    let rows: i64 = connection
        .query_row("SELECT COUNT(*) FROM files", [], |row| row.get(0))
        .expect("files");
    assert_eq!(rows, 1, "the pre-existing files must survive the migration");
    let identities: i64 = connection
        .query_row("SELECT COUNT(*) FROM works WHERE id = ?1", [&work], |row| {
            row.get(0)
        })
        .expect("works");
    assert_eq!(identities, 1);
}
