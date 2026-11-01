pub mod domain;
pub mod error;
pub mod filename;
pub mod fingerprint;
pub mod library;
pub mod paths;
pub mod scanner;
mod schema;

pub use domain::{
    BackgroundJob, LibraryItem, MediaKind, PlaybackSession, Profile, Recommendation, SegmentMarker,
    Source, SourceState,
};
pub use error::{CoreError, Result};
pub use filename::{ParsedName, parse_filename};
pub use fingerprint::media_fingerprint;
pub use library::{
    Artwork, ArtworkCandidate, BackupInfo, BackupReport, Checkpoint, ConversionJob, ConversionPlan,
    ConversionProfile, ConversionStatus, Ingest, Library, LibraryQuery, OutputFacts, PathSemantics,
    Preferences, RestoreReport, Sort, UnprobedFile, verify_output,
};
pub use paths::{PathStyle, location_key};
pub use scanner::{
    LibraryScanner, MEDIA_EXTENSIONS, ScanIssue, ScanSummary, ScanToken, ScanUpdate,
};

pub const SCHEMA_VERSION: i64 = 5;

pub const BACKUP_FORMAT_VERSION: i64 = 2;
