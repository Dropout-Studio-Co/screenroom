pub mod domain;
pub mod error;
pub mod filename;
pub mod fingerprint;
pub mod library;
pub mod nfo;
pub mod paths;
pub mod scanner;
mod schema;
pub mod volume;
pub mod watch;

pub use domain::{
    BackgroundJob, LibraryItem, MediaKind, PlaybackSession, Profile, Recommendation, SegmentMarker,
    Source, SourceState,
};
pub use error::{CoreError, Result};
pub use filename::{ParsedName, parse_filename};
pub use fingerprint::media_fingerprint;
pub use library::{
    ARTWORK_RETENTION_SETTING, ApiCredential, Artwork, ArtworkCandidate, ArtworkRetention,
    BENGALI_CODES, BackupCancel, BackupInfo, BackupProgress, BackupProgressFn, BackupReport,
    Bookmark, BookmarkDraft, CatalogueEpisode, CatalogueMatch, CatalogueMetadataProvider,
    CatalogueSeason, CatalogueSeasonSummary, CatalogueSeries, CatalogueWork, Checkpoint, Clock,
    ConversionJob, ConversionPlan, ConversionProfile, ConversionStatus, DOWNLOAD_HOST_DOMAINS,
    DownloadedSubtitle, EnrichmentDrainReport, EnrichmentQueueDepth, EnrichmentState,
    EnrichmentSummary, EnrichmentTarget, EnrichmentTask, HttpMethod, HttpRequest, HttpResponse,
    HttpTransport, HttpsTransport, ImageSize, Ingest, Library, LibraryQuery, MAX_REDIRECTS,
    MAX_TRANSFER_BYTES, METADATA_ORIGIN_PROVIDER, MetadataOrigin, MetadataPatch, MetadataQuery,
    MetadataSnapshot, OutputFacts, PathSemantics, Preferences, ProviderCapability, ProviderConfig,
    ProviderError, ProviderImage, ReconcileDecision, ReconcileMode, RestoreReport, Secret, Sleeper,
    Sort, StoredSubtitle, SubtitleMatch, SubtitleProvider, SubtitleProviderConfig, SubtitleSearch,
    SystemClock, TMDB_PROVIDER, TSDB_API_KEY_ENVIRONMENT, TSDB_BASE_URL, TSDB_BASE_URL_ENVIRONMENT,
    TSDB_CONFIG_DIR_ENVIRONMENT, TSDB_CONFIG_ENVIRONMENT, TSDB_KEY_FILE_NAME, TSDB_PROVIDER,
    TSDB_USER_AGENT, TcpTransport, ThreadSleeper, TmdbProvider, TransportError, TsdbProvider,
    UnprobedFile, describe_enrichment_target, reconcile, replacement_allowed,
    subtitle_language_codes, subtitle_language_name, verify_output,
};
pub use paths::{PathStyle, location_key};
pub use scanner::{
    LibraryScanner, MEDIA_EXTENSIONS, ScanIssue, ScanSummary, ScanToken, ScanUpdate,
};
pub use volume::VolumeIdentity;
pub use watch::{MAX_PLAN_ENTRIES, MediaChange, Plan};

pub const SCHEMA_VERSION: i64 = 10;

pub const BACKUP_FORMAT_VERSION: i64 = 2;
