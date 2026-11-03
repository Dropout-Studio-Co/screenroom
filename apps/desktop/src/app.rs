use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::Duration;

use iced::widget::{
    Id, Space, button, column, container, image, operation, pick_list, row, text, text_input,
};
use iced::{Alignment, Element, Length, Subscription, Task, keyboard};
use screenroom_core::{
    ArtworkCandidate, ArtworkRetention, CatalogueMetadataProvider, ConversionJob,
    ConversionProfile, ConversionStatus, CoreError, DownloadedSubtitle, EnrichmentDrainReport,
    Library, LibraryItem, LibraryQuery, LibraryScanner, MediaKind, OutputFacts, Preferences,
    Profile, ProviderCapability, ProviderConfig, Recommendation, ScanToken, SegmentMarker, Sort,
    Source, SourceState, StoredSubtitle, SubtitleProvider, SubtitleProviderConfig, SubtitleSearch,
    TmdbProvider, TsdbProvider, verify_output,
};
use screenroom_player::{Player, PlayerCapability, PlayerSnapshot, TrackKind};

use crate::artwork::{ArtworkEvent, Extractor};
use crate::conversion::{ConversionEvent, ConversionOutcome, Runner};
use crate::design::{self, Level};
use crate::playback::PlaybackTracker;
use crate::preview::{PREVIEW_DELAY, Preview, may_preview};
use crate::probe::{DurationPass, spawn_duration_pass};
use crate::ui;
use crate::ui::motion::{self, MotionState};
use crate::ui::route::{FocusPos, Nav};
use crate::video_surface::Frame;

const PLAYER_TICK: Duration = Duration::from_millis(16);

const STATUS_TICK: Duration = Duration::from_millis(250);

const INITIAL_WINDOW_WIDTH: f32 = 1280.0;

const MIN_WINDOW_WIDTH: f32 = 640.0;

const MIN_WINDOW_HEIGHT: f32 = 480.0;

const PAGE_ROWS: usize = 6;

const PAGE_SIZE: i64 = (DEFAULT_COLUMNS * PAGE_ROWS) as i64;

const DEFAULT_COLUMNS: usize = 4;

const RAIL: i64 = 8;

const SEEK_STEP: f64 = 10.0;

const DETAIL_POSTER: (f32, f32) = (210.0, 315.0);

const ARTWORK_IDLE_BATCH: i64 = 4;

const SUGGESTIONS: i64 = 8;

const QUIT_ROUTING_BUDGET: u8 = 40;

const MODE_SETTLE: Duration = Duration::from_millis(1_500);

const TERMINATE_CHECK_DELAY: Duration = Duration::from_millis(1_800);

const SUBTITLE_PRIORITY_NOTE: &str = "A subtitle track inside the film always comes first, and a \
                                      subtitle file sitting beside the film comes next. The \
                                      Subtitle DB is asked only when the film has neither, so \
                                      nothing online is consulted for a film that already has one.";

fn hides_its_window() -> bool {
    crate::play_check::requests_hidden_window()
}

pub fn run() -> iced::Result {
    if let Some(error) = crate::play_check::startup_error() {
        eprintln!("play check refused: {error}");
        return Ok(());
    }
    let window = iced::window::Settings {
        visible: !hides_its_window(),
        size: verification_size(),
        min_size: Some(minimum_window_size()),
        ..Default::default()
    };

    let mut application = iced::application(App::new, App::update, App::view)
        .title("Screenroom")
        .theme(App::theme)
        .subscription(App::subscription)
        .window(window)
        .exit_on_close_request(false)
        .default_font(crate::ui::theme::regular());

    for font in crate::ui::theme::font_data() {
        application = application.font(font);
    }

    application.run()
}

pub fn minimum_window_size() -> iced::Size {
    iced::Size::new(MIN_WINDOW_WIDTH, MIN_WINDOW_HEIGHT)
}

fn verification_size() -> iced::Size {
    window_size_from(
        std::env::var("SCREENROOM_WINDOW_WIDTH").ok().as_deref(),
        std::env::var("SCREENROOM_WINDOW_HEIGHT").ok().as_deref(),
    )
}

fn window_size_from(width: Option<&str>, height: Option<&str>) -> iced::Size {
    let read = |value: Option<&str>, minimum: f32, fallback: f32| {
        value
            .and_then(|value| value.trim().parse::<f32>().ok())
            .filter(|value| value.is_finite() && *value > 0.0)
            .map(|value| value.max(minimum))
            .unwrap_or(fallback)
    };
    iced::Size::new(
        read(width, MIN_WINDOW_WIDTH, INITIAL_WINDOW_WIDTH),
        read(height, MIN_WINDOW_HEIGHT, 800.0),
    )
}

fn library_path() -> Option<PathBuf> {
    match std::env::var_os("SCREENROOM_LIBRARY") {
        Some(path) => Some(PathBuf::from(path)),
        None => default_library_path(),
    }
}

pub fn default_library_path() -> Option<PathBuf> {
    let base = if cfg!(target_os = "windows") {
        std::env::var_os("APPDATA")
            .or_else(|| std::env::var_os("LOCALAPPDATA"))
            .map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|home| PathBuf::from(home).join("Library/Application Support"))
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share"))
            })
    }?;
    Some(base.join("Screenroom").join("library.db"))
}

pub fn default_backup_path() -> String {
    let Some(library) = default_library_path() else {
        return String::new();
    };
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default();
    library
        .with_file_name(format!("screenroom-backup-{stamp}.db"))
        .to_string_lossy()
        .to_string()
}

fn default_conversion_folder() -> String {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/Users"));
    home.join("Movies")
        .join("Screenroom Converted")
        .to_string_lossy()
        .to_string()
}

fn file_modified_ms(metadata: &std::fs::Metadata) -> Option<i64> {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|elapsed| elapsed.as_millis() as i64)
}

fn discard(path: &Path) {
    if path.is_file() {
        let _ = std::fs::remove_file(path);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Home,

    Browse,

    Detail,

    Player,

    Settings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KindFilter {
    All,

    Movies,

    Series,
}

impl std::fmt::Display for KindFilter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

impl KindFilter {
    fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Movies => "Movies",
            Self::Series => "Series",
        }
    }

    fn kind(self) -> Option<String> {
        match self {
            Self::All => None,
            Self::Movies => Some(MediaKind::Movie.as_str().to_string()),
            Self::Series => Some(MediaKind::Series.as_str().to_string()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchFilter {
    Any,

    Unwatched,

    Watched,
}

impl WatchFilter {
    fn watched(self) -> Option<bool> {
        match self {
            Self::Any => None,
            Self::Unwatched => Some(false),
            Self::Watched => Some(true),
        }
    }
}

impl std::fmt::Display for WatchFilter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Any => "Watched or not",
            Self::Unwatched => "Unwatched",
            Self::Watched => "Watched",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortChoice {
    Title,

    Added,

    Year,

    Progress,
}

impl std::fmt::Display for SortChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

impl SortChoice {
    pub const ALL: [Self; 4] = [Self::Title, Self::Added, Self::Year, Self::Progress];

    fn label(self) -> &'static str {
        match self {
            Self::Title => "Title",
            Self::Added => "Recently added",
            Self::Year => "Year",
            Self::Progress => "Recently watched",
        }
    }

    fn sort(self) -> Sort {
        match self {
            Self::Title => Sort::Title,
            Self::Added => Sort::Added,
            Self::Year => Sort::Year,
            Self::Progress => Sort::Progress,
        }
    }
}

struct Browse {
    text: String,
    kind: KindFilter,
    genre: Option<String>,
    watch: WatchFilter,
    sort: SortChoice,
    watchlist_only: bool,
    favorites_only: bool,
    available_only: bool,
    page: usize,
    items: Vec<LibraryItem>,
    total: i64,
    genres: Vec<String>,
}

impl Browse {
    fn new() -> Self {
        Self {
            text: String::new(),
            kind: KindFilter::All,
            genre: None,
            watch: WatchFilter::Any,
            sort: SortChoice::Title,
            watchlist_only: false,
            favorites_only: false,
            available_only: false,
            page: 0,
            items: Vec::new(),
            total: 0,
            genres: Vec::new(),
        }
    }

    fn page_count(&self) -> usize {
        if self.total <= 0 {
            1
        } else {
            ((self.total as usize).div_ceil(PAGE_SIZE as usize)).max(1)
        }
    }

    #[cfg(test)]
    fn narrowed(&self) -> bool {
        !self.text.trim().is_empty()
            || self.kind != KindFilter::All
            || self.genre.is_some()
            || self.watch != WatchFilter::Any
            || self.watchlist_only
            || self.favorites_only
            || self.available_only
    }
}

struct Detail {
    item: LibraryItem,

    episodes: Vec<LibraryItem>,

    shown: Vec<LibraryItem>,

    seasons: Vec<i64>,

    season: i64,

    origin: (Screen, usize),
}

impl Detail {
    fn compute_shown(&self) -> Vec<LibraryItem> {
        let held: Vec<LibraryItem> = self
            .episodes
            .iter()
            .filter(|episode| episode.season.unwrap_or(self.season) == self.season)
            .cloned()
            .collect();

        if held.is_empty() {
            self.episodes.clone()
        } else {
            held
        }
    }

    fn refresh_shown(&mut self) {
        self.shown = self.compute_shown();
    }

    fn season_episodes(&self) -> &[LibraryItem] {
        &self.shown
    }
}

struct Active {
    tracker: PlaybackTracker,
    file_id: String,
    title: String,
    year: Option<i64>,
    season: Option<i64>,
    episode: Option<i64>,
    resume_from_ms: i64,

    origin: Screen,

    load_id: u64,

    seek_serial: u64,
}

struct UpNext {
    file_id: String,
    title: String,
    starts_at: std::time::Instant,
}

struct SubtitleDownload {
    receiver: Receiver<Result<DownloadedSubtitle, String>>,
    file_id: String,
    load_id: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SubtitleSource {
    Track { label: String, external: bool },
    Sidecar(PathBuf),
    Online,
}

#[derive(Debug, Clone)]
struct ConversionRun {
    job_id: String,

    title: String,

    source_path: PathBuf,

    source_size: i64,

    source_modified_ms: i64,

    source_duration_ms: i64,

    source_has_audio: bool,

    output_path: PathBuf,
}

const SCAN_REFRESH_INTERVAL_MS: i64 = 500;

const UP_NEXT_DELAY: Duration = Duration::from_secs(5);

#[derive(Debug)]
enum ScanEvent {
    Progress {
        visited: u64,
        imported: u64,
        issues: usize,
        done: bool,
        complete: bool,
        enriched: u64,
    },

    Settling(Vec<String>),

    Failed(String),
}

pub struct App {
    library: Arc<Library>,

    library_label: String,
    profile_id: String,
    capability: PlayerCapability,

    screen: Screen,

    focus: usize,

    nav: Nav,

    settings_focus: usize,

    settings_section: ui::screens::settings::Section,

    home: Vec<LibraryItem>,

    suggestions: Vec<Recommendation>,
    browse: Browse,
    detail: Option<Detail>,

    artwork: HashMap<String, image::Handle>,

    artwork_aspect: HashMap<String, f32>,

    artwork_queued: HashSet<String>,

    artwork_failed: HashSet<String>,

    artwork_failures: usize,
    extractor: Option<Extractor>,

    durations: Option<DurationPass>,

    probe_paused: Arc<AtomicBool>,

    background_work: bool,

    play_check: Option<crate::play_check::Run>,

    player: Option<Player>,
    snapshot: PlayerSnapshot,
    frame: Option<Frame>,
    active: Option<Active>,
    scrub: Option<f64>,

    volume_drag: Option<f64>,

    up_next: Option<UpNext>,

    markers: Vec<SegmentMarker>,
    subtitles: Vec<StoredSubtitle>,

    receivers: Vec<crate::casting::Receiver>,
    cast_search: Option<std::sync::mpsc::Receiver<Result<Vec<crate::casting::Receiver>, String>>>,
    cast_searched: bool,

    preview: Option<Preview>,
    preview_frame: Option<Frame>,
    preview_shown: Option<String>,
    preview_dwell: Option<(String, std::time::Instant)>,
    preview_blocked: HashSet<String>,

    mark_pending: Option<(String, i64)>,

    auto_skipped: Option<String>,

    fullscreen: bool,

    motion: MotionState,

    window_width: f32,

    window_height: f32,

    last_input: std::time::Instant,

    player_chrome: bool,

    pointer_inside: bool,

    track_menu_open: Option<TrackKind>,

    pointer_buttons: std::collections::HashSet<iced::mouse::Button>,

    keyboard_reveal_at: Option<std::time::Instant>,

    preferences: Preferences,

    profiles: Vec<Profile>,

    sources: Vec<Source>,

    new_profile: String,

    relink_target: Option<String>,
    relink_path: String,

    backup_path: String,

    bookmarks: Vec<screenroom_core::Bookmark>,
    bookmark_label: String,
    bookmark_address: String,
    bookmark_access: String,

    restore_path: String,
    armed_restore: Option<String>,
    backup_job: Option<crate::backup_job::BackupJob>,
    restore_job: Option<crate::backup_job::RestoreJob>,
    queued_backup: Option<PathBuf>,

    conversion_folder: String,
    conversions: Vec<ConversionJob>,
    conversion_runner: Option<Runner>,
    conversion_run: Option<ConversionRun>,
    conversion_progress: u8,

    subtitle_input: String,
    audio_input: String,

    armed_remove: Option<String>,

    armed_profile: Option<String>,

    new_root: String,
    scan_token: Option<ScanToken>,

    scan_queue: crate::watch::Queue,
    scan_events: Option<Receiver<ScanEvent>>,

    scan_visited: u64,
    scan_imported: u64,

    scan_started_ms: Option<i64>,

    scan_quiet: bool,

    scan_refresh_ms: i64,
    status: Option<String>,
    error: Option<String>,

    metadata_capability: ProviderCapability,
    subtitle_capability: ProviderCapability,
    artwork_retention: ArtworkRetention,
    enrichment: Option<Receiver<Result<EnrichmentDrainReport, String>>>,
    subtitle_results: Vec<screenroom_core::SubtitleMatch>,
    subtitle_results_for: Option<String>,
    subtitle_note: Option<String>,

    subtitle_note_error: bool,
    subtitle_focus: usize,
    subtitle_search: Option<Receiver<Result<Vec<screenroom_core::SubtitleMatch>, String>>>,
    subtitle_download: Option<SubtitleDownload>,
    cast_session: Option<crate::cast_session::CastSession>,
    cast_connect: Option<Receiver<Result<crate::cast_session::CastSession, String>>>,
    cast_note: Option<String>,

    quit_routing_budget: u8,
    quit_routing: crate::quit::QuitRouting,

    quit_check: Option<PathBuf>,

    mode_change: Option<(iced::window::Mode, std::time::Instant)>,

    terminate_check: Option<PathBuf>,
    terminate_routing: crate::quit::TerminateRouting,
    terminate_deadline: Option<std::time::Instant>,

    auto_discovery: bool,
    watchers: crate::watch::WatchSet,
    watch_tick: std::time::Instant,
    watch_wall: Duration,
    watch_last_arrival_check: std::time::Instant,
    watch_reconciled: bool,
    watch_reconcile_at: std::time::Instant,
    watch_deferred: Vec<DeferredPath>,
}

#[derive(Debug, Clone)]
struct DeferredPath {
    at: std::time::Instant,
    source: String,
    root: PathBuf,
    path: PathBuf,
}

const MAX_DEFERRED: usize = 512;

const DEFERRED_RETRY: Duration = Duration::from_secs(3);

const INGEST_STABLE_AGE: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsField {
    ProfileName,
    SubtitleLanguage,
    AudioLanguage,
    FolderPath,
    BackupPath,
    RestorePath,
    ConversionFolder,
    BookmarkLabel,
    BookmarkAddress,
    BookmarkAccess,
}

impl SettingsField {
    fn id(self) -> &'static str {
        match self {
            Self::ProfileName => "settings-profile-name",
            Self::SubtitleLanguage => "settings-subtitle-language",
            Self::AudioLanguage => "settings-audio-language",
            Self::FolderPath => "settings-folder-path",
            Self::BackupPath => "settings-backup-path",
            Self::RestorePath => "settings-restore-path",
            Self::ConversionFolder => "settings-conversion-folder",
            Self::BookmarkLabel => "settings-bookmark-label",
            Self::BookmarkAddress => "settings-bookmark-address",
            Self::BookmarkAccess => "settings-bookmark-access",
        }
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    GoHome,

    GoBrowse,

    Back,

    Refresh,

    SearchChanged(String),

    KindChanged(KindFilter),

    GenreChanged(Option<String>),

    WatchChanged(WatchFilter),

    SortChanged(SortChoice),

    ToggleWatchlistOnly(bool),

    ToggleFavoritesOnly(bool),

    ToggleAvailableOnly(bool),

    NextPage,

    PreviousPage,

    Move(Focus),

    OpenDetail(String),

    SeasonChosen(i64),

    Play(String),

    Restart(String),

    ToggleFavorite(String),

    ToggleWatchlist(String),

    ToggleWatched(String),

    HideFromContinue(String),

    NewRootChanged(String),

    AddFolder,

    ChooseFolder,

    ChooseRelinkFolder,

    ChooseBackupPath,

    ChooseRestoreFile,

    BookmarkLabelChanged(String),

    BookmarkAddressChanged(String),

    BookmarkAccessChanged(String),

    AddBookmark,

    RemoveBookmark(String),

    OpenBookmark(String),

    ImportSubtitle,

    SubtitleChosen(String),

    UseSubtitle(String),

    RemoveSubtitle(String),

    LookForReceivers,

    CancelScan,

    RescanSource(String),

    RemoveSource(String),

    CancelArmed,

    RelinkTargetChosen(String),

    RelinkPathChanged(String),

    RelinkSource,

    GoSettings,

    SettingsSection(ui::screens::settings::Section),

    ProfileChosen(String),

    NewProfileChanged(String),

    AddProfile,

    RemoveProfile(String),

    ToggleAutoplay(bool),

    ToggleAutoSkip(bool),

    ToggleReducedMotion(bool),

    HoverCard(String),

    SkipSegment,

    FocusSettingsField(SettingsField),

    ApplySubtitleLanguage,

    ApplyAudioLanguage,

    SubtitleLanguageChanged(String),

    AudioLanguageChanged(String),

    BackupPathChanged(String),

    BackupNow,

    RestorePathChanged(String),

    RestoreNow,

    InspectRestore,

    ChooseConversionFolder,

    ConversionFolderChanged(String),

    StartConversion(String, ConversionProfile),

    CancelConversion,

    ClearStatus,

    BackFromPlayer,

    ModeChanged(iced::window::Mode),

    PlayNextNow,

    CancelNext,

    TogglePlay,

    Scrub(f64),

    SeekCommit,

    Volume(f64),

    VolumeCommit,

    ToggleMute,

    ToggleFullscreen,

    SeekStep(f64),

    SelectTrack(TrackKind, i64),

    Resized(iced::Size),

    SubtitleScale(f64),

    SubtitlePosition(f64),

    Tick,

    MotionTick(std::time::Instant),

    RailScrolled {
        label: String,
        offset: f32,
    },

    PlayerWake,

    PointerInside(bool),

    TrackMenuOpened(TrackKind),

    TrackMenuClosed(TrackKind),

    PointerMoved {
        x: f32,
        y: f32,
    },

    PointerButton {
        button: iced::mouse::Button,
        pressed: bool,
    },

    Keyboard(keyboard::Event),

    RequestTerminate,

    CancelBackup,

    EnrichmentNow,

    ToggleArtworkRetention,

    FindSubtitles,

    SubtitleDownload {
        id: i64,
        format: String,
    },

    CastTo(String),

    CastPause,

    CastStop,

    CastDisconnect,

    Close,
}

impl Screen {
    fn depth(self) -> i8 {
        match self {
            Self::Home => 0,
            Self::Browse | Self::Settings => 1,
            Self::Detail => 2,
            Self::Player => 3,
        }
    }
}

fn screen_depth(screen: Screen) -> i8 {
    screen.depth()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Left,

    Right,

    Up,

    Down,
}

#[derive(Clone, Hash)]
struct TickKey {
    poll: Duration,
    idle: Duration,
    signals: Vec<screenroom_player::FrameTicks>,
}

fn ticks(
    poll: Duration,
    idle: Duration,
    signals: Vec<screenroom_player::FrameTicks>,
) -> Subscription<Message> {
    use iced::futures::StreamExt;
    use iced::futures::channel::mpsc;

    let key = TickKey {
        poll,
        idle,
        signals,
    };
    Subscription::run_with(key, |key| {
        let key = key.clone();
        let (sender, receiver) = mpsc::unbounded::<()>();
        std::thread::spawn(move || {
            let mut seen: Vec<u64> = key.signals.iter().map(|signal| signal.current()).collect();
            let mut last = std::time::Instant::now();
            loop {
                std::thread::sleep(key.poll);
                let advanced = key
                    .signals
                    .iter()
                    .zip(seen.iter_mut())
                    .any(|(signal, seen)| {
                        let now = signal.current();
                        let advanced = now != *seen;
                        *seen = now;
                        advanced
                    });
                if advanced || last.elapsed() >= key.idle {
                    last = std::time::Instant::now();
                    if sender.unbounded_send(()).is_err() {
                        break;
                    }
                }
            }
        });
        receiver.map(|()| Message::Tick)
    })
}

pub fn pointer_event(event: &iced::Event) -> Option<Message> {
    match event {
        iced::Event::Mouse(iced::mouse::Event::CursorEntered) => Some(Message::PointerInside(true)),
        iced::Event::Mouse(iced::mouse::Event::CursorLeft) => Some(Message::PointerInside(false)),
        iced::Event::Mouse(iced::mouse::Event::CursorMoved { position }) => {
            Some(Message::PointerMoved {
                x: position.x,
                y: position.y,
            })
        }
        iced::Event::Mouse(iced::mouse::Event::ButtonPressed(button)) => {
            Some(Message::PointerButton {
                button: *button,
                pressed: true,
            })
        }
        iced::Event::Mouse(iced::mouse::Event::ButtonReleased(button)) => {
            Some(Message::PointerButton {
                button: *button,
                pressed: false,
            })
        }
        _ => None,
    }
}

fn motion_ticks() -> Subscription<Message> {
    use iced::futures::StreamExt;
    use iced::futures::channel::mpsc;

    Subscription::run_with((), |()| {
        let (sender, receiver) = mpsc::unbounded::<std::time::Instant>();
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(motion::TICK);
                if sender.unbounded_send(std::time::Instant::now()).is_err() {
                    break;
                }
            }
        });
        receiver.map(Message::MotionTick)
    })
}

fn epoch_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or_default()
}

fn ago_phrase(count: i64, singular: &str, plural: &str) -> String {
    let count = count.max(1);
    let unit = if count == 1 { singular } else { plural };
    format!("{count} {unit} ago")
}

fn scan_recency_phrase(elapsed_seconds: i64) -> String {
    match elapsed_seconds.max(0) {
        0..=90 => "just now".to_string(),
        seconds if seconds < 3_600 => ago_phrase(seconds / 60, "minute", "minutes"),
        seconds if seconds < 86_400 => ago_phrase(seconds / 3_600, "hour", "hours"),
        seconds => ago_phrase(seconds / 86_400, "day", "days"),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CastAction {
    Pause,
    Stop,
    Disconnect,
}

fn contained_media_path(root: &Path, relative: &str) -> Option<PathBuf> {
    let root = std::fs::canonicalize(root).ok()?;
    let path = std::fs::canonicalize(root.join(relative)).ok()?;
    (path.starts_with(&root) && path.is_file()).then_some(path)
}

fn snapshot_is_current(active: &Active, snapshot: &PlayerSnapshot) -> bool {
    snapshot.loaded
        && snapshot.position_ready
        && snapshot.load_id == active.load_id
        && snapshot.seek_serial >= active.seek_serial
}

fn opening_profile(library: &Library, preferences: &Preferences) -> String {
    let profiles = library.profiles().unwrap_or_default();
    preferences
        .default_profile
        .as_ref()
        .filter(|id| profiles.iter().any(|profile| &profile.id == *id))
        .cloned()
        .or_else(|| profiles.into_iter().next().map(|profile| profile.id))
        .unwrap_or_default()
}

fn screen_first(candidates: Vec<ArtworkCandidate>, on_screen: &[String]) -> Vec<ArtworkCandidate> {
    let rank = |candidate: &ArtworkCandidate| {
        on_screen
            .iter()
            .position(|work_id| work_id == &candidate.work_id)
            .unwrap_or(usize::MAX)
    };
    let mut ordered: Vec<(usize, ArtworkCandidate)> = candidates
        .into_iter()
        .map(|candidate| (rank(&candidate), candidate))
        .collect();
    ordered.sort_by_key(|(place, _)| *place);
    ordered
        .into_iter()
        .map(|(_, candidate)| candidate)
        .collect()
}

impl App {
    fn new() -> (Self, Task<Message>) {
        let (mut app, task) = Self::launch();
        app.quit_routing_budget = QUIT_ROUTING_BUDGET;
        app.quit_check = crate::quit::check_path();
        app.terminate_check = crate::quit::terminate_check_path();
        app.terminate_routing = crate::quit::install_terminate_routing();
        app.terminate_deadline = app
            .terminate_check
            .as_ref()
            .map(|_| std::time::Instant::now() + TERMINATE_CHECK_DELAY);
        app.background_work = true;

        app.open_requested_screen();
        app.ensure_artwork();
        app.start_durations();
        (app, task)
    }

    fn launch() -> (Self, Task<Message>) {
        let (path, label) = match std::env::var_os("SCREENROOM_LIBRARY") {
            Some(path) => (PathBuf::from(path), String::from("SCREENROOM_LIBRARY")),
            None => match library_path() {
                Some(path) => {
                    let label = path.display().to_string();
                    (path, label)
                }
                None => (PathBuf::from("<in-memory>"), String::from("in-memory")),
            },
        };

        if label == "in-memory" {
            return (
                Self::with_library(
                    Library::open_in_memory().expect("in-memory library"),
                    None,
                    label,
                ),
                Task::none(),
            );
        }

        match Library::open(&path) {
            Ok(library) => (Self::with_library(library, None, label), Task::none()),
            Err(error) => (
                Self::with_library(
                    Library::open_in_memory().expect("in-memory library"),
                    Some(format!("{}: {}", error.code(), error)),
                    label,
                ),
                Task::none(),
            ),
        }
    }

    pub fn with_library(library: Library, error: Option<String>, library_label: String) -> Self {
        let library = Arc::new(library);

        let preferences = library.preferences().unwrap_or_default();
        let metadata_capability =
            TmdbProvider::new(ProviderConfig::from_environment()).capability();
        let subtitle_provider = TsdbProvider::new(SubtitleProviderConfig::from_environment());
        let subtitle_capability = subtitle_provider.capability();
        let artwork_retention = library
            .artwork_retention()
            .unwrap_or(ArtworkRetention::DoNotStore);
        let conversion_folder = preferences
            .conversion_folder
            .clone()
            .unwrap_or_else(default_conversion_folder);
        let profile_id = opening_profile(&library, &preferences);
        let mut app = Self {
            library,
            library_label,
            profile_id,
            capability: screenroom_player::capability(),
            play_check: crate::play_check::Run::from_environment(),
            screen: Screen::Home,
            focus: 0,
            nav: Nav::new(Screen::Home),
            home: Vec::new(),
            suggestions: Vec::new(),
            browse: Browse::new(),
            detail: None,
            artwork: HashMap::new(),
            artwork_aspect: HashMap::new(),
            artwork_queued: HashSet::new(),
            artwork_failed: HashSet::new(),
            artwork_failures: 0,
            extractor: None,
            durations: None,
            probe_paused: Arc::new(AtomicBool::new(false)),
            background_work: false,
            player: None,
            snapshot: PlayerSnapshot::default(),
            frame: None,
            active: None,
            scrub: None,
            volume_drag: None,
            fullscreen: false,
            markers: Vec::new(),
            subtitles: Vec::new(),
            receivers: Vec::new(),
            cast_search: None,
            cast_searched: false,
            preview: None,
            preview_frame: None,
            preview_shown: None,
            preview_dwell: None,
            preview_blocked: HashSet::new(),
            mark_pending: None,
            up_next: None,
            auto_skipped: None,
            motion: MotionState::new(
                preferences.reduced_motion || crate::motion::system_reduce_motion(),
            ),
            window_width: verification_size().width,
            window_height: verification_size().height,
            last_input: std::time::Instant::now(),
            player_chrome: true,
            pointer_inside: true,
            track_menu_open: None,
            pointer_buttons: std::collections::HashSet::new(),
            keyboard_reveal_at: None,
            preferences,
            profiles: Vec::new(),
            settings_focus: 0,
            settings_section: ui::screens::settings::Section::default(),
            sources: Vec::new(),
            new_profile: String::new(),
            relink_target: None,
            relink_path: String::new(),
            backup_path: default_backup_path(),
            bookmarks: Vec::new(),
            bookmark_label: String::new(),
            bookmark_address: String::new(),
            bookmark_access: String::new(),
            restore_path: String::new(),
            armed_restore: None,
            backup_job: None,
            restore_job: None,
            queued_backup: None,
            conversion_folder,
            conversions: Vec::new(),
            conversion_runner: None,
            conversion_run: None,
            conversion_progress: 0,
            subtitle_input: String::new(),
            audio_input: String::new(),
            armed_remove: None,
            armed_profile: None,
            new_root: String::new(),
            scan_token: None,
            scan_queue: crate::watch::Queue::new(),
            scan_events: None,
            scan_visited: 0,
            scan_imported: 0,
            scan_started_ms: None,
            scan_quiet: false,
            scan_refresh_ms: 0,
            status: None,
            error,

            metadata_capability,
            subtitle_capability,
            artwork_retention,
            enrichment: None,
            subtitle_results: Vec::new(),
            subtitle_results_for: None,
            subtitle_note: None,
            subtitle_note_error: false,
            subtitle_focus: 0,
            subtitle_search: None,
            subtitle_download: None,
            cast_session: None,
            cast_connect: None,
            cast_note: None,

            quit_routing_budget: 0,
            quit_routing: crate::quit::QuitRouting::Unsupported,
            quit_check: None,
            mode_change: None,
            terminate_check: None,
            terminate_routing: crate::quit::TerminateRouting::Unsupported,
            terminate_deadline: None,

            auto_discovery: !cfg!(test),
            watchers: crate::watch::WatchSet::new(),
            watch_tick: std::time::Instant::now(),
            watch_wall: crate::watch::clock(),
            watch_last_arrival_check: std::time::Instant::now(),
            watch_reconciled: false,
            watch_reconcile_at: std::time::Instant::now(),
            watch_deferred: Vec::new(),
        };
        app.sync_language_inputs();
        app.reload();
        app
    }

    fn open_requested_screen(&mut self) {
        let Some(requested) = std::env::var_os("SCREENROOM_OPEN_SCREEN") else {
            return;
        };
        match requested.to_string_lossy().as_ref() {
            "library" => self.navigate(Screen::Browse),
            "settings" => self.navigate(Screen::Settings),
            "detail" => {
                if let Some(file_id) = self.browse.items.first().map(|item| item.id.clone()) {
                    self.open_detail(&file_id);
                }
            }
            "player" => {
                if let Some(file_id) = self.browse.items.first().map(|item| item.id.clone()) {
                    self.open_detail(&file_id);
                    self.open_player(&file_id, true);
                }
            }
            _ => {}
        }
    }

    fn sync_language_inputs(&mut self) {
        self.subtitle_input = self
            .preferences
            .subtitle_language
            .clone()
            .unwrap_or_default();
        self.audio_input = self.preferences.audio_language.clone().unwrap_or_default();
    }

    fn reload(&mut self) {
        if self.restore_job.is_some() {
            return;
        }
        self.reload_home();
        self.reload_browse();
        self.reload_administration();
        self.ensure_artwork();
    }

    fn reload_administration(&mut self) {
        match self.library.profiles() {
            Ok(profiles) => self.profiles = profiles,
            Err(error) => self.report(&error),
        }
        match self.library.sources() {
            Ok(sources) => self.sources = sources,
            Err(error) => self.report(&error),
        }
        if self
            .relink_target
            .as_ref()
            .is_some_and(|id| !self.sources.iter().any(|source| &source.id == id))
        {
            self.relink_target = None;
        }
        match self.library.recent_conversions(8) {
            Ok(jobs) => self.conversions = jobs,
            Err(error) => self.report(&error),
        }
        self.reload_bookmarks();
    }

    fn reload_bookmarks(&mut self) {
        match self.library.bookmarks() {
            Ok(bookmarks) => self.bookmarks = bookmarks,
            Err(error) => self.report(&error),
        }
    }

    fn add_bookmark(&mut self) {
        let draft = screenroom_core::BookmarkDraft {
            label: self.bookmark_label.clone(),
            address: self.bookmark_address.clone(),
            access: self.bookmark_access.clone(),
        };
        match self.library.add_bookmark(&draft) {
            Ok(bookmark) => {
                self.bookmark_label.clear();
                self.bookmark_address.clear();
                self.bookmark_access.clear();
                self.reload_bookmarks();
                self.status = Some(format!("Saved {}.", bookmark.label));
            }
            Err(error) => self.report(&error),
        }
    }

    fn remove_bookmark(&mut self, id: &str) {
        match self.library.remove_bookmark(id) {
            Ok(true) => {
                self.reload_bookmarks();
                self.status = Some("Saved source removed.".to_string());
            }
            Ok(false) => {}
            Err(error) => self.report(&error),
        }
    }

    fn open_bookmark(&mut self, id: &str) {
        let Some(bookmark) = self
            .bookmarks
            .iter()
            .find(|bookmark| bookmark.id == id)
            .cloned()
        else {
            return;
        };
        match crate::external::open_url(&bookmark.address) {
            Ok(()) => {
                self.status = Some(format!("Handed {} to your browser.", bookmark.label));
            }
            Err(error) => {
                self.error = Some(format!(
                    "Could not hand {} to the system: {error}",
                    bookmark.label
                ));
            }
        }
    }

    fn reload_home(&mut self) {
        match self.library.continue_watching(&self.profile_id, RAIL) {
            Ok(items) => self.home = items,
            Err(error) => self.report(&error),
        }
        match self.library.recommendations(&self.profile_id, SUGGESTIONS) {
            Ok(suggestions) => self.suggestions = suggestions,
            Err(error) => self.report(&error),
        }
        self.ensure_artwork();
    }

    fn reload_browse(&mut self) {
        let base = LibraryQuery {
            profile_id: Some(self.profile_id.clone()),
            text: self.browse.text.clone(),
            kind: self.browse.kind.kind(),
            genre: self.browse.genre.clone(),
            watched: self.browse.watch.watched(),
            available_only: self.browse.available_only,

            group_by_work: true,
            watchlist_only: self.browse.watchlist_only,
            favorites_only: self.browse.favorites_only,
            sort: self.browse.sort.sort(),
            ..LibraryQuery::page(PAGE_SIZE)
        };

        match self.library.count(&base) {
            Ok(total) => self.browse.total = total,
            Err(error) => {
                self.report(&error);
                return;
            }
        }

        let pages = self.browse.page_count();
        if self.browse.page >= pages {
            self.browse.page = pages - 1;
        }

        let query = LibraryQuery {
            offset: (self.browse.page * PAGE_SIZE as usize) as i64,
            ..base
        };
        match self.library.items(&query) {
            Ok(items) => self.browse.items = items,
            Err(error) => self.report(&error),
        }

        if let Ok(genres) = self.library.known_genres() {
            if let Some(selected) = &self.browse.genre
                && !genres.contains(selected)
            {
                self.browse.genre = None;
            }
            self.browse.genres = genres;
        }
        self.clamp_focus();
        self.ensure_artwork();
    }

    fn visible_work_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = Vec::new();
        let mut add = |work_id: &str| {
            if !ids.iter().any(|known| known == work_id) {
                ids.push(work_id.to_string());
            }
        };
        for item in &self.home {
            add(&item.work_id);
        }
        for item in &self.browse.items {
            add(&item.work_id);
        }
        if let Some(detail) = &self.detail {
            add(&detail.item.work_id);
            for episode in &detail.episodes {
                add(&episode.work_id);
            }
        }
        for suggestion in &self.suggestions {
            add(&suggestion.item.work_id);
        }
        ids
    }

    fn on_screen_work_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = Vec::new();
        let mut add = |work_id: &str| {
            if !ids.iter().any(|known| known == work_id) {
                ids.push(work_id.to_string());
            }
        };
        match self.screen {
            Screen::Home => {
                for item in &self.home {
                    add(&item.work_id);
                }
                for suggestion in &self.suggestions {
                    add(&suggestion.item.work_id);
                }
            }
            Screen::Browse => {
                for item in &self.browse.items {
                    add(&item.work_id);
                }
            }
            Screen::Detail => {
                if let Some(detail) = &self.detail {
                    add(&detail.item.work_id);
                    for episode in &detail.episodes {
                        add(&episode.work_id);
                    }
                }
            }
            Screen::Player | Screen::Settings => {}
        }
        ids
    }

    fn ensure_artwork(&mut self) {
        self.queue_artwork(false);
    }

    fn queue_artwork(&mut self, visible_only: bool) {
        if self.screen == Screen::Player {
            return;
        }
        let ids = self.visible_work_ids();
        if ids.is_empty() {
            return;
        }

        let missing: Vec<String> = ids
            .iter()
            .filter(|work_id| {
                !self.artwork.contains_key(*work_id) && !self.artwork_failed.contains(*work_id)
            })
            .cloned()
            .collect();
        if !missing.is_empty() {
            match self.library.artworks_for(&missing) {
                Ok(found) => {
                    for art in found {
                        if art.width > 0 && art.height > 0 {
                            self.artwork_aspect
                                .insert(art.work_id.clone(), art.width as f32 / art.height as f32);
                        }
                        self.artwork
                            .entry(art.work_id)
                            .or_insert_with(|| image::Handle::from_bytes(art.bytes));
                    }
                }
                Err(error) => self.report(&error),
            }
        }

        if !self.background_work {
            return;
        }
        let mut candidates = match self.library.artwork_candidates_in(&ids) {
            Ok(candidates) => candidates,
            Err(error) => return self.report(&error),
        };
        if candidates.is_empty() {
            if visible_only {
                return;
            }

            candidates = match self.library.artwork_candidates(ARTWORK_IDLE_BATCH) {
                Ok(candidates) => candidates,
                Err(error) => return self.report(&error),
            };
        }
        candidates.retain(|candidate| {
            !self.artwork_queued.contains(&candidate.work_id)
                && !self.artwork_failed.contains(&candidate.work_id)
        });
        if candidates.is_empty() {
            return;
        }
        let on_screen = self.on_screen_work_ids();
        let candidates = screen_first(candidates, &on_screen);
        if self.extractor.is_none() {
            self.extractor = Some(Extractor::spawn(Arc::clone(&self.library)));
        }
        let Some(extractor) = &self.extractor else {
            return;
        };
        for candidate in candidates {
            let work_id = candidate.work_id.clone();

            if extractor.request(candidate) {
                self.artwork_queued.insert(work_id);
            }
        }
    }

    fn start_durations(&mut self) {
        if self.background_work && self.durations.is_none() {
            self.durations = Some(spawn_duration_pass(
                Arc::clone(&self.library),
                Arc::clone(&self.probe_paused),
            ));
        }
    }

    fn drain_durations(&mut self) {
        self.probe_paused
            .store(self.active.is_some(), Ordering::Relaxed);
        let Some(pass) = &self.durations else { return };
        let mut stored = false;
        let mut ended = false;
        loop {
            match pass.reports.try_recv() {
                Ok(count) => stored |= count > 0,
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    ended = true;
                    break;
                }
            }
        }
        if ended {
            self.durations = None;
        }

        if stored && self.screen != Screen::Player {
            self.reload_home();
            self.reload_browse();
            self.refresh_detail();
        }
    }

    fn drain_artwork(&mut self) -> usize {
        let Some(extractor) = &self.extractor else {
            return 0;
        };
        let mut handled = 0_usize;
        for event in extractor.poll() {
            handled += 1;
            match event {
                ArtworkEvent::Stored { work_id } => {
                    self.artwork_queued.remove(&work_id);
                    if let Ok(Some(art)) = self.library.artwork(&work_id) {
                        let aspect = if art.width > 0 && art.height > 0 {
                            art.width as f32 / art.height as f32
                        } else {
                            DETAIL_POSTER.0 / DETAIL_POSTER.1
                        };
                        self.artwork
                            .insert(work_id.clone(), image::Handle::from_bytes(art.bytes));
                        if aspect.is_finite() && aspect > 0.0 {
                            self.artwork_aspect.insert(work_id, aspect);
                        }
                    }
                }
                ArtworkEvent::Failed { work_id, reason } => {
                    self.artwork_queued.remove(&work_id);
                    self.artwork_failed.insert(work_id);
                    self.artwork_failures += 1;

                    if self.artwork_failures == 1 {
                        self.status = Some(format!("No poster could be drawn — {reason}"));
                    }
                }
            }
        }
        handled
    }

    fn sync_navigation(&mut self) {
        self.screen = self.nav.screen();
        self.focus = self.nav.focus().column;
    }

    fn navigate(&mut self, screen: Screen) {
        self.nav.set(screen);
        self.sync_navigation();
    }

    fn open_screen(&mut self, screen: Screen) {
        if self.nav.screen() == screen {
            self.nav.set(screen);
        } else {
            self.nav.push(screen);
        }
        self.sync_navigation();
    }

    fn focus_card(&mut self, column: usize) {
        self.nav.set_focus(FocusPos { rail: 0, column });
        self.sync_navigation();
    }

    fn clamp_focus(&mut self) {
        if self.screen == Screen::Home {
            let sections = self.home_sections();
            let current = self.nav.focus();
            let rail = current.rail.min(sections.len().saturating_sub(1));
            let held = sections.get(rail).map_or(0, |(_, items)| items.len());
            let column = if held == 0 {
                0
            } else {
                current.column.min(held - 1)
            };
            self.nav.set_focus(FocusPos { rail, column });
            self.sync_navigation();
            return;
        }

        let len = self.visible_len();
        self.focus_card(if len == 0 { 0 } else { self.focus.min(len - 1) });
    }

    fn home_sections(&self) -> Vec<(&'static str, Vec<&LibraryItem>)> {
        let suggested: Vec<&LibraryItem> = self
            .suggestions
            .iter()
            .map(|suggestion| &suggestion.item)
            .collect();
        ui::screens::home::rails_for(&self.home, &suggested)
    }

    fn home_selected_item(&self) -> Option<&LibraryItem> {
        let sections = self.home_sections();
        let focus = self.nav.focus();
        sections
            .get(focus.rail)
            .and_then(|(_, items)| items.get(focus.column))
            .copied()
    }

    fn home_flat_index(&self) -> Option<usize> {
        let sections = self.home_sections();
        let focus = self.nav.focus();
        let mut offset = 0;
        for (rail, (_, items)) in sections.iter().enumerate() {
            if rail == focus.rail {
                return (focus.column < items.len()).then_some(offset + focus.column);
            }
            offset += items.len();
        }
        None
    }

    #[cfg(test)]
    fn home_item(&self, index: usize) -> Option<&LibraryItem> {
        if index < self.home.len() {
            self.home.get(index)
        } else {
            self.suggestions
                .get(index - self.home.len())
                .map(|suggestion| &suggestion.item)
        }
    }

    fn visible_len(&self) -> usize {
        match self.screen {
            Screen::Home => self.home.len() + self.suggestions.len(),
            Screen::Browse => self.browse.items.len(),

            Screen::Settings | Screen::Player => 0,
            Screen::Detail => self
                .detail
                .as_ref()
                .map(|detail| detail.season_episodes().len().max(1))
                .unwrap_or(0),
        }
    }

    fn report(&mut self, error: &CoreError) {
        self.error = Some(format!("{}: {}", error.code(), error));
    }

    fn route_quit(&mut self) -> bool {
        if self.quit_routing_budget == 0 {
            return false;
        }
        let routing = crate::quit::route_quit_through_close();
        self.quit_routing = routing;

        let settled = if routing.is_graceful() {
            self.quit_routing_budget = 0;
            true
        } else {
            self.quit_routing_budget -= 1;
            if self.quit_routing_budget == 0 {
                eprintln!(
                    "screenroom: the menu bar's Quit could not be routed through the window \
                     close ({routing}); quitting may not store the position it is at."
                );
            }
            self.quit_routing_budget == 0
        };
        if !settled {
            return false;
        }

        let Some(path) = self.quit_check.clone() else {
            return false;
        };
        if let Err(error) = crate::quit::report_check(&path, routing) {
            eprintln!(
                "screenroom: could not write the quit-routing check to {}: {error}",
                path.display()
            );
        }
        true
    }

    fn open_detail(&mut self, file_id: &str) {
        let item = match self.library.item(file_id, Some(&self.profile_id)) {
            Ok(Some(item)) => item,
            Ok(None) => {
                self.error = Some("That file is no longer in the library.".to_string());
                return;
            }
            Err(error) => return self.report(&error),
        };

        let mut seasons = Vec::new();
        if item.kind == MediaKind::Series
            && let Ok(found) = self.library.seasons_for_work(&item.work_id)
        {
            seasons = found;
        }
        let episodes = match self.episodes_of(&item) {
            Ok(found) => found,
            Err(error) => return self.report(&error),
        };

        let origin = match (&self.detail, self.screen) {
            (Some(detail), Screen::Detail) => detail.origin,
            _ => (self.screen, self.focus),
        };

        let season = item
            .season
            .or_else(|| seasons.first().copied())
            .unwrap_or(1);
        let mut detail = Detail {
            item,
            episodes,
            shown: Vec::new(),
            seasons,
            season,
            origin,
        };
        detail.refresh_shown();
        let focus = detail
            .season_episodes()
            .iter()
            .position(|episode| episode.id == detail.item.id)
            .unwrap_or(0);
        self.detail = Some(detail);
        self.open_screen(Screen::Detail);
        self.focus_card(focus);
        self.ensure_artwork();
    }

    fn episodes_of(&self, item: &LibraryItem) -> Result<Vec<LibraryItem>, CoreError> {
        if item.kind != MediaKind::Series {
            return Ok(Vec::new());
        }
        self.library.items(&LibraryQuery {
            profile_id: Some(self.profile_id.clone()),
            work_id: Some(item.work_id.clone()),
            sort: Sort::Title,
            ..LibraryQuery::page(500)
        })
    }

    fn open_player(&mut self, file_id: &str, from_start: bool) {
        if self.active.is_some() {
            self.finish_active();
        }
        if self.subtitle_results_for.as_deref() != Some(file_id) {
            self.subtitle_results.clear();
            self.subtitle_results_for = None;
        }
        self.subtitle_note = None;
        self.subtitle_note_error = false;
        self.track_menu_open = None;
        self.pointer_buttons.clear();
        self.keyboard_reveal_at = None;
        let origin = match self.screen {
            Screen::Detail => Screen::Detail,
            _ => Screen::Browse,
        };
        let Some(item) = self
            .library
            .item(file_id, Some(&self.profile_id))
            .ok()
            .flatten()
        else {
            self.error = Some("That file is no longer in the library.".to_string());
            return;
        };

        if !item.available {
            self.error = Some(format!(
                "{} is not readable right now; relink its source and rescan.",
                item.filename
            ));
            return;
        }
        let Some(path) = self.media_path(&item) else {
            self.error = Some(format!(
                "{} cannot be read right now; relink its source and rescan.",
                item.filename
            ));
            return;
        };

        if self.player.is_none() {
            match Player::start() {
                Ok(player) => self.player = Some(player),
                Err(error) => {
                    self.error = Some(format!("Playback is unavailable: {error}"));
                    return;
                }
            }
        }

        let opened = match PlaybackTracker::open(&self.library, &self.profile_id, &item.id) {
            Ok(opened) => opened,
            Err(error) => return self.report(&error),
        };

        let resume = if from_start { 0 } else { opened.resume_from_ms };
        let player = self.player.as_ref().expect("player started above");

        player.set_subtitle_style(
            f64::from(self.preferences.subtitle_scale_percent) / 100.0,
            f64::from(self.preferences.subtitle_position_percent),
        );

        player.set_volume(f64::from(self.preferences.volume_percent));
        let load_id = player.load_at(&path, Some(resume as f64 / 1_000.0));
        self.active = Some(Active {
            tracker: opened.tracker,
            file_id: item.id.clone(),

            title: item.title.clone(),
            year: item.year,
            season: item.season,
            episode: item.episode,
            resume_from_ms: resume,
            origin,
            load_id,
            seek_serial: 0,
        });
        self.snapshot = player.snapshot();
        self.frame = None;
        self.scrub = None;

        self.markers.clear();
        self.reload_markers(&item.edition_id);
        self.reload_subtitles(&item.id);
        self.mark_pending = None;
        self.auto_skipped = None;
        self.open_screen(Screen::Player);
    }

    fn media_path(&self, item: &LibraryItem) -> Option<PathBuf> {
        let source = self.library.source(&item.source_id).ok()?;
        contained_media_path(Path::new(&source.root_path), &item.relative_path)
    }

    fn end_viewing(&mut self) -> Task<Message> {
        if self.active.is_none() && self.screen != Screen::Player {
            return Task::none();
        }
        self.stop_playback();
        if self.fullscreen {
            return self.toggle_fullscreen();
        }
        Task::none()
    }

    fn enrichment_now(&mut self) {
        if self.enrichment.is_some() {
            self.status = Some("An enrichment pass is already running.".to_string());
            return;
        }
        if !self.metadata_capability.is_configured() {
            self.error = None;
            self.status = Some(self.metadata_capability.detail.clone());
            return;
        }
        self.error = None;
        self.status = Some("Queueing the library for online metadata…".to_string());
        let library = Arc::clone(&self.library);
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let provider = TmdbProvider::new(ProviderConfig::from_environment());
            let result = library
                .enqueue_whole_library("en-US")
                .and_then(|queued| {
                    library
                        .drain_enrichment_queue(&provider, 40, epoch_ms())
                        .map(|report| (queued, report))
                })
                .map_err(|error| format!("{}: {error}", error.code()))
                .map(|(queued, report)| {
                    let _ = queued;
                    report
                });
            let _ = sender.send(result);
        });
        self.enrichment = Some(receiver);
    }

    fn toggle_artwork_retention(&mut self) {
        let next = if self.artwork_retention == ArtworkRetention::KeepLocally {
            ArtworkRetention::DoNotStore
        } else {
            ArtworkRetention::KeepLocally
        };
        match self.library.set_artwork_retention(next) {
            Ok(()) => {
                self.artwork_retention = next;
                self.error = None;
                self.status = Some(match next {
                    ArtworkRetention::KeepLocally => {
                        "Provider artwork will be kept on this machine within the provider's terms."
                            .to_string()
                    }
                    ArtworkRetention::DoNotStore => {
                        "Provider artwork will not be downloaded or kept.".to_string()
                    }
                });
            }
            Err(error) => self.report(&error),
        }
    }

    fn subtitle_language(&self) -> String {
        self.preferences
            .subtitle_language
            .clone()
            .filter(|code| !code.trim().is_empty())
            .unwrap_or_else(|| "bn".to_string())
    }

    fn tmdb_identity(&self, work_id: &str) -> Option<i64> {
        self.library
            .provider_identity(work_id, "tmdb")
            .ok()
            .flatten()
            .and_then(|id| id.trim().parse::<i64>().ok())
    }

    fn release_name_for(&self, item: &LibraryItem) -> Option<String> {
        let stem = self
            .media_path(item)
            .and_then(|path| {
                path.file_stem()
                    .map(|stem| stem.to_string_lossy().to_string())
            })
            .or_else(|| {
                Path::new(&item.filename)
                    .file_stem()
                    .map(|stem| stem.to_string_lossy().to_string())
            })?;
        let trimmed = stem.trim().to_string();
        (!trimmed.is_empty()).then_some(trimmed)
    }

    fn subtitle_source(&self) -> Option<SubtitleSource> {
        let active = self.active.as_ref()?;
        if self.snapshot.load_id == active.load_id
            && let Some(track) = self
                .snapshot
                .tracks
                .iter()
                .find(|track| track.kind == TrackKind::Subtitle)
        {
            return Some(SubtitleSource::Track {
                label: track.label(),
                external: track.external,
            });
        }
        Some(
            self.subtitle_sidecar(active)
                .map_or(SubtitleSource::Online, SubtitleSource::Sidecar),
        )
    }

    fn subtitle_sidecar(&self, active: &Active) -> Option<PathBuf> {
        let item = self
            .library
            .item(&active.file_id, Some(&self.profile_id))
            .ok()
            .flatten()?;
        let path = self.media_path(&item)?;
        crate::sidecar::subtitle_beside(&path)
    }

    fn note_subtitle(&mut self, note: impl Into<String>) {
        self.subtitle_note = Some(note.into());
        self.subtitle_note_error = false;
    }

    fn fail_subtitle(&mut self, message: impl Into<String>) {
        let message = message.into();
        self.subtitle_note = Some(message.clone());
        self.subtitle_note_error = true;
        self.error = Some(message);
    }

    fn subtitle_actions(&self) -> Vec<Message> {
        let (kept_rows, result_rows) = self.subtitle_row_split();
        let mut actions: Vec<Message> = self
            .subtitles
            .iter()
            .take(kept_rows)
            .map(|subtitle| Message::UseSubtitle(subtitle.id.clone()))
            .collect();
        actions.extend(
            self.subtitle_results
                .iter()
                .take(result_rows)
                .map(|result| Message::SubtitleDownload {
                    id: result.id,
                    format: result.format.clone(),
                }),
        );
        actions
    }

    fn subtitle_row_split(&self) -> (usize, usize) {
        let budget = self.subtitle_row_budget();
        if self.subtitle_results.is_empty() {
            return (self.subtitles.len().min(budget), 0);
        }
        let result_rows = budget.min(self.subtitle_results.len());
        let kept_rows = self.subtitles.len().min(budget - result_rows);
        (kept_rows, result_rows)
    }

    fn move_subtitle_focus(&mut self, delta: isize) {
        let count = self.subtitle_actions().len();
        if count == 0 {
            self.subtitle_focus = 0;
            return;
        }
        let current = self.subtitle_focus.min(count - 1) as isize;
        let next = (current + delta).rem_euclid(count as isize);
        self.subtitle_focus = next as usize;
    }

    fn subtitle_result_label(&self, result: &screenroom_core::SubtitleMatch) -> String {
        if self.window_width < 900.0 {
            return format!("{} · {} cues", result.language_name, result.cues);
        }
        let name = if result.release_name.trim().is_empty() {
            result.language_name.clone()
        } else {
            result.release_name.clone()
        };
        format!("{name} · {} cues", result.cues)
    }

    fn subtitle_enter_target(&self) -> Option<Message> {
        if self.up_next.is_some() {
            return None;
        }
        let actions = self.subtitle_actions();
        actions
            .get(self.subtitle_focus.min(actions.len().saturating_sub(1)))
            .cloned()
    }

    fn activate_subtitle(&mut self) -> Task<Message> {
        let actions = self.subtitle_actions();
        let Some(action) = actions.get(self.subtitle_focus.min(actions.len().saturating_sub(1)))
        else {
            return Task::none();
        };
        let action = action.clone();
        self.update(action)
    }

    fn subtitle_row_budget(&self) -> usize {
        const ROW_COST: f32 = 52.0;
        const RESERVED: f32 = ui::screens::player::BOTTOM_SCRIM_HEIGHT + 30.0 + 130.0 + 104.0;
        if !self.window_height.is_finite() {
            return 2;
        }
        let free = (self.window_height - RESERVED).max(ROW_COST);
        ((free / ROW_COST).floor() as i64).clamp(1, 6) as usize
    }

    fn find_subtitles(&mut self) {
        if self.subtitle_search.is_some() {
            self.status = Some("A subtitle search is already running.".to_string());
            self.note_subtitle("A subtitle search is already running.");
            return;
        }
        let Some(active) = self.active.as_ref() else {
            self.status = Some("Play something first, then find its subtitles.".to_string());
            self.note_subtitle("Play something first, then find its subtitles.");
            return;
        };
        if self.snapshot.load_id != active.load_id || !self.snapshot.loaded {
            self.error = None;
            self.status = Some(
                "The player has not reported this film's tracks yet, so a subtitle track inside \
                 it cannot be ruled out. Nothing online was asked; try again once it is playing."
                    .to_string(),
            );
            self.note_subtitle(
                "The player has not reported this film's tracks yet, so a track inside it cannot \
                 be ruled out. Nothing online was asked; try again once it is playing.",
            );
            return;
        }
        let Some(source) = self.subtitle_source() else {
            self.status = Some("Play something first, then find its subtitles.".to_string());
            self.note_subtitle("Play something first, then find its subtitles.");
            return;
        };
        match source {
            SubtitleSource::Track { label, external } => {
                self.report_existing_track(&label, external)
            }
            SubtitleSource::Sidecar(path) => self.use_sidecar(&path),
            SubtitleSource::Online => self.find_subtitles_online(),
        }
    }

    fn report_existing_track(&mut self, label: &str, external: bool) {
        self.error = None;
        self.subtitle_results.clear();
        let note = if external {
            format!(
                "A kept subtitle ({label}) is already showing for this film, so nothing online was \
                 asked."
            )
        } else {
            format!(
                "This file carries its own subtitle track ({label}). Nothing online was asked, \
                 because a track inside the file always comes first."
            )
        };
        self.status = Some(note.clone());
        self.note_subtitle(note);
    }

    fn use_sidecar(&mut self, path: &Path) {
        let Some(file_id) = self.active.as_ref().map(|active| active.file_id.clone()) else {
            return;
        };
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string_lossy().to_string());
        match self.library.import_subtitle(&file_id, path) {
            Ok(stored) => {
                self.reload_subtitles(&file_id);
                self.subtitle_results.clear();
                self.error = None;
                self.use_subtitle(&stored.id);
                if self.error.is_none() {
                    let note = format!(
                        "Chose {name}, the subtitle file beside this film. Nothing online was asked."
                    );
                    self.status = Some(note.clone());
                    self.note_subtitle(note);
                }
            }
            Err(error) => {
                self.fail_subtitle(error.to_string());
                self.report(&error);
            }
        }
    }

    fn find_subtitles_online(&mut self) {
        if !self.subtitle_capability.is_configured() {
            self.error = None;
            self.status = Some(self.subtitle_capability.detail.clone());
            self.note_subtitle(self.subtitle_capability.detail.clone());
            return;
        }
        let Some(active) = self.active.as_ref() else {
            self.status = Some("Play something first, then find its subtitles.".to_string());
            self.note_subtitle("Play something first, then find its subtitles.");
            return;
        };
        let title = active.title.clone();
        let year = active.year;
        let season = active.season;
        let episode = active.episode;
        let file_id = active.file_id.clone();
        let language = self.subtitle_language();
        let mut request = match (season, episode) {
            (Some(season), Some(episode)) => {
                SubtitleSearch::episode(&title, year, season, episode, &language)
            }
            _ => SubtitleSearch::movie(&title, year, &language),
        };
        let item = self
            .library
            .item(&file_id, Some(&self.profile_id))
            .ok()
            .flatten();
        if let Some(item) = &item
            && let Some(tmdb) = self.tmdb_identity(&item.work_id)
        {
            request = request.with_tmdb(tmdb);
        }
        if let Some(item) = &item
            && let Some(release) = self.release_name_for(item)
        {
            request = request.with_release(release);
        }
        self.error = None;
        self.subtitle_results.clear();
        self.subtitle_results_for = Some(file_id.clone());
        self.subtitle_focus = 0;
        self.status = Some("Asking The Subtitle DB…".to_string());
        self.note_subtitle("Asking The Subtitle DB…");
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let provider = TsdbProvider::new(SubtitleProviderConfig::from_environment());
            let result = provider
                .search(&request)
                .map_err(|error| format!("{}: {}", error.code(), error.message()));
            let _ = sender.send(result);
        });
        self.subtitle_search = Some(receiver);
    }

    fn subtitle_download(&mut self, id: i64, format: &str) {
        if self.subtitle_download.is_some() {
            self.status = Some("A subtitle download is already running.".to_string());
            return;
        }
        let Some(active) = self.active.as_ref() else {
            self.status = Some("Play something first.".to_string());
            return;
        };
        let file_id = active.file_id.clone();
        let load_id = active.load_id;
        if self.subtitle_results_for.as_deref() != Some(file_id.as_str()) {
            self.status = Some(
                "Those results were found for a different film. Find subtitles again for this \
                 one."
                    .to_string(),
            );
            self.note_subtitle(
                "Those results were found for a different film. Find subtitles again for this one.",
            );
            return;
        }
        let language = self.subtitle_language();
        let format = format.to_string();
        self.error = None;
        self.status = Some("Downloading the subtitle…".to_string());
        self.note_subtitle("Downloading the subtitle…");
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let provider = TsdbProvider::new(SubtitleProviderConfig::from_environment());
            let result = provider
                .download(id, &language, &format)
                .map_err(|error| format!("{}: {}", error.code(), error.message()));
            let _ = sender.send(result);
        });
        self.subtitle_download = Some(SubtitleDownload {
            receiver,
            file_id,
            load_id,
        });
    }

    fn cast_to(&mut self, receiver_id: &str) {
        let Some(receiver) = self
            .receivers
            .iter()
            .find(|candidate| candidate.id == receiver_id)
            .cloned()
        else {
            return;
        };
        if self.cast_session.is_some() {
            self.status = Some("A cast session is already running. Stop it first.".to_string());
            return;
        }
        let Some(active) = self.active.as_ref() else {
            self.status = Some("Play something first, then choose a receiver.".to_string());
            return;
        };
        let Some(path) = self
            .library
            .item(&active.file_id, Some(&self.profile_id))
            .ok()
            .flatten()
            .and_then(|item| self.media_path(&item))
        else {
            self.status = Some("That file is not on an available source any more.".to_string());
            return;
        };
        let title = active.title.clone();
        let extension = path
            .extension()
            .map(|value| value.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        let facts = crate::casting::MediaFacts {
            container: extension,
            video_codec: None,
            audio_codec: None,
        };
        self.error = None;
        self.cast_note = Some(format!(
            "Connecting to {} — this machine serves that one file from a temporary endpoint.",
            receiver.name
        ));
        let mut request = crate::cast_session::SessionRequest::new(receiver, path, facts, title);
        request.allow_unverified = true;
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let result = crate::cast_session::connect(request).map_err(|error| format!("{error}"));
            let _ = sender.send(result);
        });
        self.cast_connect = Some(receiver);
    }

    fn cast_action(&mut self, action: CastAction) {
        if action == CastAction::Disconnect {
            let Some(session) = self.cast_session.take() else {
                return;
            };
            self.cast_note = Some(match session.disconnect() {
                Ok(_) => "The receiver was told to stop, and the endpoint is closed.".to_string(),
                Err(error) => {
                    format!("The session closed, but the receiver did not confirm: {error}")
                }
            });
            return;
        }
        let Some(session) = self.cast_session.as_mut() else {
            return;
        };
        let outcome = match action {
            CastAction::Pause => session.pause().map(|state| format!("Receiver: {state:?}")),
            CastAction::Stop => session.stop().map(|state| format!("Receiver: {state:?}")),
            CastAction::Disconnect => unreachable!(),
        };
        match outcome {
            Ok(note) => self.cast_note = Some(note),
            Err(error) => self.cast_note = Some(format!("{error}")),
        }
        if let Some(session) = self.cast_session.as_ref() {
            self.cast_note = Some(session.summary());
        }
    }

    fn drain_enrichment(&mut self) {
        let Some(receiver) = &self.enrichment else {
            return;
        };
        match receiver.try_recv() {
            Ok(Ok(report)) => {
                self.enrichment = None;
                self.error = None;
                self.status = Some(format!(
                    "Enrichment: {} attempted, {} updated, {} failed, {} waiting for the network, {} still queued.",
                    report.attempted,
                    report.succeeded,
                    report.failed,
                    report.waiting_network,
                    report.remaining
                ));
                self.reload();
            }
            Ok(Err(message)) => {
                self.enrichment = None;
                self.error = Some(message);
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.enrichment = None;
                self.error = Some("The enrichment pass stopped unexpectedly.".to_string());
            }
        }
    }

    fn drain_subtitles(&mut self) {
        if let Some(receiver) = &self.subtitle_search {
            match receiver.try_recv() {
                Ok(Ok(results)) => {
                    self.subtitle_search = None;
                    self.error = None;
                    let still_watching = self.active.as_ref().is_some_and(|active| {
                        self.subtitle_results_for.as_deref() == Some(active.file_id.as_str())
                    });
                    if still_watching {
                        let note = match results.len() {
                            0 => "No subtitle on The Subtitle DB matched this file.".to_string(),
                            count => format!("{count} subtitles matched on The Subtitle DB."),
                        };
                        self.status = Some(note.clone());
                        self.note_subtitle(note);
                        self.subtitle_results = results;
                        self.subtitle_focus = 0;
                    } else {
                        self.subtitle_results_for = None;
                        let note = "Those results were for a film that is no longer playing, so \
                                    they were dropped. Find subtitles again for this one.";
                        self.status = Some(note.to_string());
                        self.note_subtitle(note);
                    }
                }
                Ok(Err(message)) => {
                    self.subtitle_search = None;
                    self.fail_subtitle(message);
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    self.subtitle_search = None;
                    self.fail_subtitle("The subtitle search stopped unexpectedly.");
                }
            }
        }
        if let Some(pending) = &self.subtitle_download {
            match pending.receiver.try_recv() {
                Ok(Ok(downloaded)) => {
                    let file_id = pending.file_id.clone();
                    let load_id = pending.load_id;
                    self.subtitle_download = None;
                    let still_watching = self.active.as_ref().is_some_and(|active| {
                        active.file_id == file_id && active.load_id == load_id
                    });
                    if !still_watching {
                        self.error = None;
                        let note = "That subtitle finished downloading after the film changed, so \
                                    it was not kept or shown.";
                        self.status = Some(note.to_string());
                        self.note_subtitle(note);
                    } else {
                        match self
                            .library
                            .import_downloaded_subtitle(&file_id, &downloaded)
                        {
                            Ok(stored) => {
                                self.error = None;
                                let note = format!("Downloaded {}.", stored.language);
                                self.status = Some(note.clone());
                                self.note_subtitle(note);
                                self.reload_subtitles(&file_id);
                                let _ = self.update(Message::UseSubtitle(stored.id.clone()));
                            }
                            Err(error) => {
                                self.fail_subtitle(error.to_string());
                                self.report(&error);
                            }
                        }
                    }
                }
                Ok(Err(message)) => {
                    self.subtitle_download = None;
                    self.fail_subtitle(message);
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    self.subtitle_download = None;
                    self.error = Some("The subtitle download stopped unexpectedly.".to_string());
                }
            }
        }
    }

    fn drain_cast(&mut self) {
        let Some(receiver) = &self.cast_connect else {
            return;
        };
        match receiver.try_recv() {
            Ok(Ok(session)) => {
                self.cast_connect = None;
                self.cast_note = Some(session.summary());
                self.cast_session = Some(session);
            }
            Ok(Err(message)) => {
                self.cast_connect = None;
                self.cast_note = Some(message);
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.cast_connect = None;
                self.cast_note = Some("The cast attempt stopped unexpectedly.".to_string());
            }
        }
    }

    fn casting_note(&self) -> String {
        if self.cast_search.is_some() {
            return "Looking for cast receivers on this network…".to_string();
        }
        if !self.cast_searched {
            return "Nothing has been looked for yet. Discovery runs only when you press the \
                    button below."
                .to_string();
        }
        crate::casting::summary(self.receivers.len())
    }

    fn look_for_receivers(&mut self) {
        if self.cast_search.is_some() {
            self.status = Some("A discovery run is already in progress.".to_string());
            return;
        }
        self.cast_searched = true;
        self.receivers.clear();
        let (sender, receiver) = std::sync::mpsc::channel();
        self.cast_search = Some(receiver);
        self.status = Some("Looking for cast receivers on this network…".to_string());
        std::thread::spawn(move || {
            let outcome = crate::casting::discover(Duration::from_millis(1_200), 12);
            let _ = sender.send(outcome);
        });
    }

    fn drain_cast_discovery(&mut self) {
        let Some(receiver) = self.cast_search.take() else {
            return;
        };
        match receiver.try_recv() {
            Ok(Ok(found)) => {
                self.receivers = found;
                self.cast_searched = true;
                self.status = Some(self.casting_note());
            }
            Ok(Err(error)) => {
                self.cast_searched = false;
                self.receivers.clear();
                self.error = Some(error);
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                self.cast_search = Some(receiver);
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {}
        }
    }

    fn reload_subtitles(&mut self, file_id: &str) {
        match self.library.subtitles_for(file_id) {
            Ok(subtitles) => self.subtitles = subtitles,
            Err(error) => {
                self.subtitles.clear();
                self.report(&error);
            }
        }
    }

    fn import_subtitle(&mut self) {
        let start = self
            .active
            .as_ref()
            .and_then(|active| {
                self.library
                    .item(&active.file_id, Some(&self.profile_id))
                    .ok()
                    .flatten()
            })
            .and_then(|item| self.media_path(&item))
            .and_then(|path| path.parent().map(Path::to_path_buf));
        match crate::picker::choose_subtitle(start.as_deref()) {
            crate::picker::Picked::Path(path) => {
                self.subtitle_chosen(path.to_string_lossy().as_ref());
            }
            crate::picker::Picked::Cancelled => {}
            crate::picker::Picked::Unavailable(reason) => self.status = Some(reason.to_string()),
        }
    }

    fn subtitle_chosen(&mut self, path: &str) {
        let Some(file_id) = self.active.as_ref().map(|active| active.file_id.clone()) else {
            return;
        };
        match self.library.import_subtitle(&file_id, Path::new(path)) {
            Ok(stored) => {
                self.reload_subtitles(&file_id);
                self.status = Some(format!(
                    "Kept {} for this film; the file you picked was not changed.",
                    stored.label
                ));
            }
            Err(error) => self.report(&error),
        }
    }

    fn use_subtitle(&mut self, id: &str) {
        let Some(player) = &self.player else {
            return;
        };
        match self.library.subtitle_path_for(id) {
            Ok(Some(path)) => {
                player.sub_add(&path);
                self.status = Some("Subtitle handed to the player.".to_string());
            }
            Ok(None) => {
                self.fail_subtitle("That stored subtitle is no longer here.");
            }
            Err(error) => {
                self.fail_subtitle(error.to_string());
                self.report(&error);
            }
        }
    }

    fn remove_subtitle(&mut self, id: &str) {
        match self.library.remove_subtitle(id) {
            Ok(true) => {
                if let Some(file_id) = self.active.as_ref().map(|active| active.file_id.clone()) {
                    self.reload_subtitles(&file_id);
                }
                self.status = Some("Stored subtitle removed.".to_string());
            }
            Ok(false) => {}
            Err(error) => self.report(&error),
        }
    }

    fn leave_player(&mut self) {
        let origin = self
            .active
            .as_ref()
            .map(|active| active.origin)
            .unwrap_or(Screen::Browse);

        self.stop_playback();
        self.navigate(origin);
        self.reload();
        self.refresh_detail();
    }

    fn refresh_detail(&mut self) {
        let Some(file_id) = self.detail.as_ref().map(|detail| detail.item.id.clone()) else {
            return;
        };
        if let Ok(Some(item)) = self.library.item(&file_id, Some(&self.profile_id)) {
            let episodes = self.episodes_of(&item).ok();
            if let Some(detail) = self.detail.as_mut() {
                detail.item = item;
                if let Some(episodes) = episodes {
                    detail.episodes = episodes;
                }
                detail.refresh_shown();
            }
        }
    }

    #[cfg(test)]
    fn playing_snapshot(position: f64, duration: f64) -> PlayerSnapshot {
        PlayerSnapshot {
            state: screenroom_player::PlayerState::Playing,
            position,
            duration,
            loaded: true,
            position_ready: true,
            ..PlayerSnapshot::default()
        }
    }

    fn save(&mut self, active: &mut Active, snapshot: &PlayerSnapshot, ended: bool, force: bool) {
        if !snapshot_is_current(active, snapshot) || snapshot.duration_ms() <= 0 {
            return;
        }
        match active.tracker.record(
            &self.library,
            snapshot.position_ms(),
            snapshot.duration_ms(),
            ended,
            force,
        ) {
            Ok(_) => {}
            Err(error) => self.report(&error),
        }
    }

    fn drive_play_check(&mut self) -> bool {
        let Some(mut run) = self.play_check.take() else {
            return false;
        };
        let done = self.advance_play_check(&mut run);
        if !done {
            self.play_check = Some(run);
        }
        done
    }

    fn advance_play_check(&mut self, run: &mut crate::play_check::Run) -> bool {
        let now = std::time::Instant::now();
        if !run.started() {
            let player = match Player::start() {
                Ok(player) => player,
                Err(error) => {
                    run.fail(format!("the player could not start: {error}"));
                    let _ = run.report(now);
                    return true;
                }
            };
            player.load_at(run.config().media(), None);
            self.player = Some(player);
            self.open_screen(Screen::Player);
            run.begin(now);
            return false;
        }
        run.note_tick();
        let decoded = self
            .snapshot
            .frame
            .as_ref()
            .map_or(0, |frame| frame.generation);
        let delivered = self.frame.as_ref().map(|frame| frame.generation);
        if let Some(frame) = self.frame.as_ref() {
            run.note_size((frame.width, frame.height));
        }
        run.note(now, delivered, crate::play_check::uploads());
        run.note_drawn(crate::play_check::draws(), now);
        if run.sample_due(now) {
            run.sample(
                now,
                (
                    decoded,
                    delivered.unwrap_or(0),
                    crate::play_check::uploads(),
                ),
            );
        }
        if run.failure().is_none() && self.frame.is_none() && run.elapsed(now) > 8.0 {
            run.fail("no decoded frame reached the application within eight seconds".to_string());
        }
        if run.failure().is_none() && run.elapsed(now) > run.config().seconds() + 30.0 {
            run.fail("the run did not finish inside its deadline".to_string());
        }
        if run.finished(now) {
            let _ = run.report(now);
            return true;
        }
        false
    }

    fn poll(&mut self) {
        if self
            .mode_change
            .is_some_and(|(_, since)| since.elapsed() >= MODE_SETTLE)
        {
            self.mode_change = None;
        }
        if self
            .terminate_deadline
            .is_some_and(|at| std::time::Instant::now() >= at)
        {
            self.terminate_deadline = None;
            crate::quit::request_termination();
        }

        self.drain_backup();
        self.drain_enrichment();
        self.drain_subtitles();
        self.drain_cast();
        if self.restore_job.is_some() {
            return;
        }
        self.drain_scan();
        self.watch_step();
        if self.scan_token.is_none()
            && let Some(destination) = self.queued_backup.take()
        {
            self.start_backup(destination);
        }
        self.drain_durations();
        self.drain_conversion();
        self.drain_preview();
        self.drain_cast_discovery();

        if self.drain_artwork() > 0 {
            self.queue_artwork(true);
        }
        let Some(player) = &self.player else { return };
        let snapshot = player.snapshot();
        let changed_frame = snapshot.frame.as_ref().is_some_and(|frame| {
            self.frame.as_ref().map(|held| held.generation) != Some(frame.generation)
        });
        if changed_frame {
            self.frame = snapshot.frame.clone();
        }
        let ended = snapshot.ended && !self.snapshot.ended;
        self.snapshot = snapshot;

        if let Some(mut active) = self.active.take() {
            let snapshot = self.snapshot.clone();
            self.save(&mut active, &snapshot, ended, false);
            self.active = Some(active);
        }

        if ended {
            self.advance_to_next_episode();
        }
        if self
            .up_next
            .as_ref()
            .is_some_and(|up| std::time::Instant::now() >= up.starts_at)
        {
            self.play_up_next();
        }

        if !ended {
            self.maybe_auto_skip_intro();
        }
    }

    fn toggle_play(&mut self) {
        let Some(player) = &self.player else { return };
        match play_action(self.snapshot.state) {
            PlayAction::Replay => self.replay(),
            PlayAction::Pause => player.set_paused(true),
            PlayAction::Resume => player.set_paused(false),
        }
    }

    fn replay(&mut self) {
        let Some(file_id) = self.active.as_ref().map(|active| active.file_id.clone()) else {
            return;
        };
        let origin = self.active.as_ref().map(|active| active.origin);
        self.finish_active();
        self.open_player(&file_id, true);

        if let (Some(origin), Some(active)) = (origin, self.active.as_mut()) {
            active.origin = origin;
        }
    }

    fn stop_playback(&mut self) {
        self.up_next = None;

        let snapshot = self
            .player
            .as_ref()
            .map(|player| player.snapshot())
            .unwrap_or_else(|| self.snapshot.clone());
        if let Some(mut active) = self.active.take() {
            self.save(&mut active, &snapshot, snapshot.ended, true);
            if let Err(error) = active.tracker.close(&self.library) {
                self.report(&error);
            }
        }
        if let Some(player) = &self.player {
            player.stop();
        }
        self.subtitles.clear();
        self.subtitle_results.clear();
        self.subtitle_results_for = None;
        self.subtitle_note = None;
        self.subtitle_note_error = false;
        self.subtitle_focus = 0;
        self.track_menu_open = None;
        self.pointer_buttons.clear();
        self.keyboard_reveal_at = None;
        self.frame = None;
        self.scrub = None;
        self.snapshot = PlayerSnapshot::default();
    }

    fn finish_active(&mut self) {
        self.up_next = None;
        if let Some(mut active) = self.active.take() {
            let snapshot = self.player.as_ref().map(|player| player.snapshot());
            if let Some(snapshot) = snapshot {
                self.save(&mut active, &snapshot, snapshot.ended, true);
            }
            if let Err(error) = active.tracker.close(&self.library) {
                self.report(&error);
            }
        }
        if let Some(player) = &self.player {
            player.stop();
        }
        self.frame = None;
        self.scrub = None;
        self.snapshot = PlayerSnapshot::default();
    }

    fn next_episode_after(&self, file_id: &str) -> Result<Option<LibraryItem>, CoreError> {
        if !self.preferences.autoplay_next {
            return Ok(None);
        }
        self.library.next_episode(file_id, Some(&self.profile_id))
    }

    fn advance_to_next_episode(&mut self) {
        let Some(finished) = self.active.as_ref().map(|active| active.file_id.clone()) else {
            return;
        };
        let next = match self.next_episode_after(&finished) {
            Ok(next) => next,
            Err(error) => return self.report(&error),
        };
        let Some(next) = next else {
            return;
        };
        let title = self
            .active
            .as_ref()
            .map(|active| active.title.clone())
            .unwrap_or_default();
        if !next.available {
            self.status = Some(format!(
                "{title} finished. The next episode cannot be read right now; relink its folder."
            ));
            return;
        }

        self.status = Some(format!("Finished {title}."));
        self.up_next = Some(UpNext {
            file_id: next.id,
            title: next.title,
            starts_at: std::time::Instant::now() + UP_NEXT_DELAY,
        });
    }

    fn play_up_next(&mut self) {
        let Some(next) = self.up_next.take() else {
            return;
        };
        let origin = self.active.as_ref().map(|active| active.origin);
        self.finish_active();
        self.open_player(&next.file_id, false);
        if let (Some(origin), Some(active)) = (origin, self.active.as_mut()) {
            active.origin = origin;
        }
        if self.screen == Screen::Player {
            self.status = Some(format!("Playing next: {}", next.title));
        }
    }

    fn commit_seek(&mut self) {
        let Some(seconds) = self.scrub.take() else {
            return;
        };
        self.seek_to(seconds);
    }

    fn seek_to(&mut self, seconds: f64) {
        let Some(player) = &self.player else { return };

        if let Some(active) = &mut self.active
            && let Err(error) = active.tracker.seek(&self.library)
        {
            self.report(&error);
            return;
        }
        let serial = player.seek(seconds);
        if let Some(active) = &mut self.active {
            active.seek_serial = serial;
        }
    }

    fn active_marker(&self) -> Option<SegmentMarker> {
        let position = self.snapshot.position_ms();
        self.markers
            .iter()
            .find(|marker| position >= marker.start_ms && position < marker.end_ms)
            .cloned()
    }

    fn skip_segment(&mut self) {
        let Some(marker) = self.active_marker() else {
            self.status = Some("No intro or credits marker covers this position yet.".to_string());
            return;
        };
        self.seek_to(marker.end_ms as f64 / 1_000.0);
        self.status = Some(
            if marker.kind == "intro" {
                "Skipped the intro."
            } else {
                "Skipped the credits."
            }
            .to_string(),
        );
    }

    fn toggle_segment_mark(&mut self, kind: &str) {
        if self
            .mark_pending
            .as_ref()
            .is_some_and(|(pending, _)| pending == kind)
        {
            self.finish_segment_mark();
        } else {
            self.start_segment_mark(kind);
        }
    }

    fn start_segment_mark(&mut self, kind: &str) {
        let position = self.snapshot.position_ms();
        if position <= 0 {
            self.status =
                Some("Play into the segment first, then pin where it starts.".to_string());
            return;
        }
        self.mark_pending = Some((kind.to_string(), position));
        self.status = Some(format!(
            "{kind} start pinned at {} — press again where the segment ends.",
            clock(position as f64 / 1_000.0)
        ));
    }

    fn finish_segment_mark(&mut self) {
        let Some((kind, start_ms)) = self.mark_pending.take() else {
            return;
        };
        let Some(file_id) = self.active.as_ref().map(|active| active.file_id.clone()) else {
            return;
        };
        let edition_id = match self.library.item(&file_id, Some(&self.profile_id)) {
            Ok(Some(item)) => item.edition_id,
            Ok(None) => {
                self.status = Some("That file is no longer in the library.".to_string());
                return;
            }
            Err(error) => return self.report(&error),
        };
        let end_ms = self.snapshot.position_ms().max(start_ms);
        match self
            .library
            .mark_segment(&edition_id, &kind, start_ms, end_ms)
        {
            Ok(marker) => {
                self.reload_markers(&marker.edition_id);
                self.status = Some(format!(
                    "{kind} marked {} – {}; it is offered next time this file plays.",
                    clock(marker.start_ms as f64 / 1_000.0),
                    clock(marker.end_ms as f64 / 1_000.0)
                ));
            }
            Err(error) => self.report(&error),
        }
    }

    fn reload_markers(&mut self, edition_id: &str) {
        match self.library.markers(edition_id) {
            Ok(markers) => self.markers = markers,
            Err(error) => self.report(&error),
        }
    }

    fn maybe_auto_skip_intro(&mut self) {
        if !self.preferences.auto_skip_intro {
            return;
        }

        if !self
            .active
            .as_ref()
            .is_some_and(|active| snapshot_is_current(active, &self.snapshot))
        {
            return;
        }
        let position = self.snapshot.position_ms();
        let found = self
            .markers
            .iter()
            .find(|marker| {
                marker.kind == "intro" && position >= marker.start_ms && position < marker.end_ms
            })
            .map(|marker| (marker.id.clone(), marker.end_ms));
        let Some((id, end_ms)) = found else {
            return;
        };
        if self.auto_skipped.as_deref() == Some(id.as_str()) {
            return;
        }
        self.auto_skipped = Some(id);
        self.seek_to(end_ms as f64 / 1_000.0);
        self.status = Some("Skipped the intro.".to_string());
    }

    fn choose_profile(&mut self, profile_id: String) {
        if !self.profiles.iter().any(|profile| profile.id == profile_id) {
            self.error = Some("That profile is no longer in the library.".to_string());
            return;
        }
        self.profile_id = profile_id.clone();
        self.focus_card(0);
        self.detail = None;

        let preferences = Preferences {
            default_profile: Some(profile_id),
            ..self.preferences.clone()
        };
        self.save_preferences(preferences);
        self.reload();
    }

    fn add_profile(&mut self) {
        let name = self.new_profile.trim().to_string();
        if name.is_empty() {
            self.error = Some("Type a name for the new profile first.".to_string());
            return;
        }
        match self.library.add_profile(&name) {
            Ok(profile) => {
                self.new_profile.clear();
                self.reload_administration();
                self.status = Some(format!("Added {}", profile.name));
                self.choose_profile(profile.id);
            }
            Err(error) => self.report(&error),
        }
    }

    fn remove_profile(&mut self, profile_id: String) {
        let name = self
            .profiles
            .iter()
            .find(|profile| profile.id == profile_id)
            .map(|profile| profile.name.clone())
            .unwrap_or_else(|| "that profile".to_string());
        if self.profiles.len() > 1 && self.armed_profile.as_deref() != Some(profile_id.as_str()) {
            self.armed_profile = Some(profile_id);
            self.status = Some(format!(
                "Press Remove again to delete {name} with its progress, favourites and watchlist."
            ));
            return;
        }
        self.armed_profile = None;
        if let Err(error) = self.library.remove_profile(&profile_id) {
            return self.report(&error);
        }
        let removed_active = self.profile_id == profile_id;
        if removed_active {
            self.profile_id = opening_profile(&self.library, &Preferences::default());
        }

        if removed_active
            || self.preferences.default_profile.as_deref() == Some(profile_id.as_str())
        {
            let preferences = Preferences {
                default_profile: Some(self.profile_id.clone()).filter(|id| !id.is_empty()),
                ..self.preferences.clone()
            };
            self.save_preferences(preferences);
        }
        self.status = Some(format!("Removed {name} and its watch history."));
        self.reload();
    }

    fn save_preferences(&mut self, preferences: Preferences) {
        match self.library.set_preferences(&preferences) {
            Ok(()) => self.preferences = preferences,
            Err(error) => self.report(&error),
        }
    }

    fn volume_percent(&self) -> f64 {
        self.volume_drag
            .or(self.snapshot.volume)
            .unwrap_or(f64::from(self.preferences.volume_percent))
            .clamp(0.0, 100.0)
    }

    fn drag_volume(&mut self, percent: f64) {
        if !percent.is_finite() {
            return;
        }
        let percent = percent.clamp(0.0, 100.0);
        self.volume_drag = Some(percent);
        if let Some(player) = &self.player {
            player.set_volume(percent);
        }
    }

    fn commit_volume(&mut self) {
        let Some(percent) = self.volume_drag.take() else {
            return;
        };
        let stored = round_percent(percent, 0, 100);
        if stored == self.preferences.volume_percent {
            return;
        }
        let preferences = Preferences {
            volume_percent: stored,
            ..self.preferences.clone()
        };
        self.save_preferences(preferences);
    }

    fn toggle_mute(&mut self) {
        let Some(player) = &self.player else {
            return;
        };
        let muted = !self.snapshot.muted.unwrap_or(false);
        player.set_muted(muted);
        self.status = Some(if muted { "Muted." } else { "Sound on." }.to_string());
    }

    fn apply_subtitle_style(&mut self, scale_percent: u16, position_percent: u16) {
        let preferences = Preferences {
            subtitle_scale_percent: scale_percent,
            subtitle_position_percent: position_percent,
            ..self.preferences.clone()
        };
        if preferences == self.preferences {
            return;
        }
        let before_ok = self.error.is_none();
        self.save_preferences(preferences);
        if before_ok && self.error.is_none() {
            self.status = Some("Subtitle style saved.".to_string());
        }
        self.push_subtitle_style();
    }

    fn push_subtitle_style(&self) {
        let Some(player) = &self.player else {
            return;
        };
        player.set_subtitle_style(
            f64::from(self.preferences.subtitle_scale_percent) / 100.0,
            f64::from(self.preferences.subtitle_position_percent),
        );
    }

    fn apply_language(&mut self, subtitle: bool) {
        let (value, current) = if subtitle {
            (
                self.subtitle_input.clone(),
                self.preferences.subtitle_language.clone(),
            )
        } else {
            (
                self.audio_input.clone(),
                self.preferences.audio_language.clone(),
            )
        };
        let trimmed = value.trim();
        let next = (!trimmed.is_empty()).then(|| trimmed.to_string());
        if next == current {
            return;
        }
        let preferences = if subtitle {
            Preferences {
                subtitle_language: next,
                ..self.preferences.clone()
            }
        } else {
            Preferences {
                audio_language: next,
                ..self.preferences.clone()
            }
        };
        let before_ok = self.error.is_none();
        self.save_preferences(preferences);
        if before_ok && self.error.is_none() {
            self.status = Some("Preference saved.".to_string());
        }
    }

    fn rescan_source(&mut self, source_id: String) {
        if let Err(error) = self
            .library
            .set_source_state(&source_id, SourceState::Available)
        {
            return self.report(&error);
        }
        self.reload_administration();
        self.start_scan(source_id);
    }

    fn relink_source(&mut self) {
        let Some(source_id) = self.relink_target.clone() else {
            self.error = Some("Choose the folder to relink first.".to_string());
            return;
        };
        let raw = self.relink_path.trim().to_string();
        if raw.is_empty() {
            self.error = Some("Type the folder's new location first.".to_string());
            return;
        }
        if let Err(error) = self.library.relink_source(&source_id, PathBuf::from(&raw)) {
            return self.report(&error);
        }
        self.relink_path.clear();
        self.status = Some("Folder relinked — rescanning.".to_string());
        self.reload_administration();
        self.start_scan(source_id);
    }

    fn remove_source(&mut self, source_id: String) {
        if self.armed_remove.as_deref() != Some(source_id.as_str()) {
            self.armed_remove = Some(source_id);
            self.status = Some(
                "Press Remove again to hide this folder's files. Nothing on disk is deleted."
                    .to_string(),
            );
            return;
        }
        self.armed_remove = None;
        match self.library.remove_source(&source_id) {
            Ok(()) => {
                self.scan_queue.drop_source(&source_id);
                self.arm_watchers();
                self.status = Some(
                    "Folder removed from the library. Its files are untouched on disk.".to_string(),
                );
                self.reload();
            }
            Err(error) => self.report(&error),
        }
    }

    fn backup_now(&mut self) {
        let raw = self.backup_path.trim().to_string();
        if raw.is_empty() {
            self.error = Some("Choose where the backup should be written.".to_string());
            return;
        }
        if self.backup_job.is_some() || self.restore_job.is_some() {
            self.status = Some(
                "A backup or a restore is already running. Wait for it to finish.".to_string(),
            );
            return;
        }
        let destination = PathBuf::from(&raw);
        if self.scan_token.is_some() {
            self.queued_backup = Some(destination);
            self.status = Some("The backup will start when this scan finishes.".to_string());
            return;
        }
        self.start_backup(destination);
    }

    fn start_backup(&mut self, destination: PathBuf) {
        self.error = None;
        self.status = Some(format!(
            "Backing up to {} — the window stays usable while it writes.",
            destination.display()
        ));
        self.backup_job = Some(crate::backup_job::BackupJob::start(
            Arc::clone(&self.library),
            destination,
        ));
    }

    fn cancel_backup(&mut self) {
        let Some(job) = &self.backup_job else {
            return;
        };
        job.cancel();
        self.status = Some("Stopping the backup…".to_string());
    }

    fn drain_backup(&mut self) {
        let mut finished = None;
        if let Some(job) = &mut self.backup_job {
            if let Some(written) = job.take_progress() {
                self.status = Some(format!(
                    "Backing up — {} written so far.",
                    crate::backup_job::megabytes(written)
                ));
            }
            if let Some(outcome) = job.take_outcome() {
                finished = Some((job.destination().to_path_buf(), outcome));
            }
        }
        if let Some((destination, outcome)) = finished {
            self.backup_job = None;
            match outcome {
                Ok(report) => {
                    self.error = None;
                    self.status = Some(format!(
                        "Backed up {} KB to {} — {}.",
                        report.bytes / 1024,
                        destination.display(),
                        report.info
                    ));
                }
                Err(error) => self.report(&error),
            }
        }

        let mut restored = None;
        if let Some(job) = &mut self.restore_job
            && let Some(outcome) = job.take_outcome()
        {
            restored = Some(outcome);
        }
        if let Some(outcome) = restored {
            self.restore_job = None;
            match outcome {
                Ok(report) => self.after_restore(report),
                Err(error) => self.report(&error),
            }
        }
    }

    fn after_restore(&mut self, report: screenroom_core::RestoreReport) {
        self.preferences = self.library.preferences().unwrap_or_default();
        self.sync_language_inputs();

        if !self
            .library
            .profiles()
            .map(|profiles| profiles.iter().any(|profile| profile.id == self.profile_id))
            .unwrap_or(false)
        {
            self.profile_id = opening_profile(&self.library, &self.preferences);
        }

        self.artwork.clear();
        self.artwork_aspect.clear();
        self.artwork_queued.clear();
        self.artwork_failed.clear();
        self.artwork_failures = 0;
        self.detail = None;
        self.reload();
        let safety = report
            .previous_saved_to
            .map(|path| format!(" The previous library is at {path}."))
            .unwrap_or_default();
        self.status = Some(format!("Restored {} entries.{safety}", report.info.works));
    }

    fn inspect_restore(&mut self) {
        let raw = self.restore_path.trim().to_string();
        if raw.is_empty() {
            self.error = Some("Choose the backup to restore from.".to_string());
            return;
        }
        self.armed_restore = None;
        match Library::inspect_backup(PathBuf::from(&raw)) {
            Ok(info) => {
                self.error = None;
                self.status = Some(format!("That backup holds {info}"));
            }
            Err(error) => self.report(&error),
        }
    }

    fn restore_now(&mut self) {
        let raw = self.restore_path.trim().to_string();
        if raw.is_empty() {
            self.error = Some("Choose the backup to restore from.".to_string());
            return;
        }
        if self.armed_restore.as_deref() != Some(raw.as_str()) {
            self.armed_restore = Some(raw.clone());
            let summary = Library::inspect_backup(PathBuf::from(&raw))
                .map(|info| info.to_string())
                .unwrap_or_else(|error| format!("unreadable ({})", error.code()));
            self.status = Some(format!(
                "Press Restore again to replace this library with {summary}. \
                 Your current library is copied aside first."
            ));
            return;
        }
        self.armed_restore = None;
        if self.backup_job.is_some() || self.restore_job.is_some() {
            self.status = Some(
                "A backup or a restore is already running. Wait for it to finish.".to_string(),
            );
            return;
        }
        self.error = None;
        self.status = Some(
            "Restoring — the window keeps drawing, and reading the library waits until it ends."
                .to_string(),
        );
        self.restore_job = Some(crate::backup_job::RestoreJob::start(
            Arc::clone(&self.library),
            PathBuf::from(&raw),
        ));
    }

    fn stop_preview(&mut self) {
        self.preview = None;
        self.preview_frame = None;
        self.preview_shown = None;
        self.preview_dwell = None;
    }

    fn release_preview(&mut self) {
        if let Some(preview) = &self.preview
            && self.preview_blocked.len() < 4_096
        {
            self.preview_blocked.insert(preview.file_id().to_string());
        }
        self.preview = None;
        self.preview_dwell = None;
    }

    fn drain_preview(&mut self) {
        let playing = self.player.is_some() || self.active.is_some();
        if self.screen != Screen::Browse {
            if self.preview.is_some() || self.preview_frame.is_some() {
                self.stop_preview();
            }
            return;
        }
        let mut ended = false;
        let mut presented = None;
        if let Some(preview) = self.preview.as_mut() {
            let snapshot = preview.tick();
            presented = snapshot.frame.clone();
            ended = snapshot.ended;
        }
        if presented.is_some() {
            self.preview_frame = presented;
        }
        let focused = self
            .browse
            .items
            .get(self.focus)
            .map(|item| (item.id.clone(), item.duration_ms, item.available));
        let mismatch = match (&self.preview, &focused) {
            (Some(preview), Some((id, ..))) => preview.file_id() != id.as_str(),
            (Some(_), None) => true,
            (None, _) => false,
        };
        let expired = self.preview.as_ref().is_some_and(Preview::expired);
        if playing || mismatch || self.conversion_runner.is_some() {
            self.stop_preview();
        } else if expired || ended {
            self.release_preview();
        }
        if self.preview.is_some() {
            return;
        }
        let Some((id, duration_ms, available)) = focused else {
            return;
        };
        if may_preview(
            self.preferences.reduced_motion,
            playing,
            available,
            self.capability.available,
        )
        .is_err()
            || self.preview_blocked.contains(&id)
        {
            return;
        }
        match &self.preview_dwell {
            Some((dwell_id, since)) if *dwell_id == id => {
                if since.elapsed() < PREVIEW_DELAY {
                    return;
                }
            }
            _ => {
                self.preview_dwell = Some((id, std::time::Instant::now()));
                return;
            }
        }
        let Some(item) = self.browse.items.get(self.focus).cloned() else {
            return;
        };
        let Some(path) = self.media_path(&item) else {
            self.preview_blocked.insert(id);
            return;
        };
        match Preview::start(&id, &path, duration_ms) {
            Ok(preview) => {
                self.preview = Some(preview);
                self.preview_shown = Some(id);
                self.preview_frame = None;
            }
            Err(_) => {
                self.preview_blocked.insert(id);
            }
        }
    }

    fn start_conversion(&mut self, file_id: String, profile: ConversionProfile) {
        if self.conversion_runner.is_some() {
            self.error =
                Some("One conversion runs at a time. Stop the running one first.".to_string());
            return;
        }
        let folder = PathBuf::from(self.conversion_folder.trim());
        let plan = match self.library.plan_conversion(&file_id, profile, &folder) {
            Ok(plan) => plan,
            Err(error) => return self.report(&error),
        };
        let facts = crate::probe::probe(&plan.source_path).unwrap_or_default();
        let job = match self.library.begin_conversion(&plan) {
            Ok(job) => job,
            Err(error) => return self.report(&error),
        };
        match Runner::spawn(plan.clone()) {
            Ok(runner) => {
                self.conversion_runner = Some(runner);
                self.conversion_run = Some(ConversionRun {
                    job_id: job.id,
                    title: plan.title.clone(),
                    source_path: plan.source_path.clone(),
                    source_size: plan.source_size,
                    source_modified_ms: plan.source_modified_ms,
                    source_duration_ms: facts.duration_ms,
                    source_has_audio: facts.has_audio,
                    output_path: plan.output_path.clone(),
                });
                self.conversion_progress = 0;
                self.error = None;
                self.status = Some(format!(
                    "Converting {} — the copy is written to {}.",
                    plan.title,
                    plan.output_path.display()
                ));
                self.reload_administration();
            }
            Err(reason) => {
                let _ =
                    self.library
                        .finish_conversion(&job.id, ConversionStatus::Failed, &reason, 0);
                self.error = Some(reason);
                self.reload_administration();
            }
        }
    }

    fn drain_conversion(&mut self) {
        let Some(runner) = &self.conversion_runner else {
            return;
        };
        let events = runner.poll();
        if events.is_empty() {
            return;
        }
        let Some(run) = self.conversion_run.clone() else {
            return;
        };
        let mut finished = false;
        for event in events {
            match event {
                ConversionEvent::Progress(percent) => {
                    if percent > self.conversion_progress {
                        self.conversion_progress = percent;
                        let _ = self.library.set_conversion_progress(&run.job_id, percent);
                    }
                }
                ConversionEvent::Finished(outcome) => {
                    finished = true;
                    self.apply_conversion_outcome(&run, outcome);
                }
            }
        }
        if finished {
            self.conversion_runner = None;
            self.conversion_run = None;
            self.conversion_progress = 0;
            self.reload_administration();
        }
    }

    fn apply_conversion_outcome(&mut self, run: &ConversionRun, outcome: ConversionOutcome) {
        match outcome {
            ConversionOutcome::Succeeded {
                bytes,
                duration_ms,
                has_video,
                has_audio,
                seconds,
            } => {
                let facts = OutputFacts {
                    duration_ms,
                    has_video,
                    has_audio,
                };
                let checked = verify_output(run.source_duration_ms, run.source_has_audio, &facts);
                let intact = std::fs::metadata(&run.source_path).is_ok_and(|metadata| {
                    metadata.len() as i64 == run.source_size
                        && file_modified_ms(&metadata) == Some(run.source_modified_ms)
                });
                if let Err(error) = checked {
                    discard(&run.output_path);
                    let _ = self.library.finish_conversion(
                        &run.job_id,
                        ConversionStatus::Failed,
                        &error.to_string(),
                        bytes as i64,
                    );
                    self.error = Some(error.to_string());
                    return;
                }
                if !intact {
                    discard(&run.output_path);
                    let reason = "The original file changed while it was being converted, so the \
                                  copy no longer proves anything and was removed.";
                    let _ = self.library.finish_conversion(
                        &run.job_id,
                        ConversionStatus::Failed,
                        reason,
                        bytes as i64,
                    );
                    self.error = Some(reason.to_string());
                    return;
                }
                let _ = self.library.finish_conversion(
                    &run.job_id,
                    ConversionStatus::Done,
                    "",
                    bytes as i64,
                );
                self.error = None;
                self.status = Some(format!(
                    "Converted {} in {} s and checked the copy. It is at {}. The original is \
                     unchanged; add that folder to the library to play it.",
                    run.title,
                    seconds,
                    run.output_path.display()
                ));
            }
            ConversionOutcome::Failed(reason) => {
                discard(&run.output_path);
                let _ = self.library.finish_conversion(
                    &run.job_id,
                    ConversionStatus::Failed,
                    &reason,
                    0,
                );
                self.error = Some(format!(
                    "Converting {} stopped: {reason} The original is untouched.",
                    run.title
                ));
            }
            ConversionOutcome::Cancelled => {
                discard(&run.output_path);
                let _ = self.library.finish_conversion(
                    &run.job_id,
                    ConversionStatus::Cancelled,
                    "Stopped at your request.",
                    0,
                );
                self.status = Some(
                    "Conversion stopped and the partial copy was removed. The original is \
                     untouched."
                        .to_string(),
                );
            }
        }
    }

    fn choose_conversion_folder(&mut self) {
        let start = Path::new(&self.conversion_folder)
            .parent()
            .map(Path::to_path_buf);
        match crate::picker::choose_folder(start.as_deref()) {
            crate::picker::Picked::Path(folder) => {
                self.conversion_folder = folder.to_string_lossy().to_string();
                let preferences = Preferences {
                    conversion_folder: Some(self.conversion_folder.clone()),
                    ..self.preferences.clone()
                };
                self.save_preferences(preferences);
            }
            crate::picker::Picked::Cancelled => {}
            crate::picker::Picked::Unavailable(reason) => self.status = Some(reason.to_string()),
        }
    }

    fn detail_conversion_row(&self, item: &LibraryItem) -> Element<'_, Message> {
        let tokens = design::tokens();
        let job = self.conversions.iter().find(|job| job.file_id == item.id);
        let running = job.is_some_and(ConversionJob::is_active);
        let mut buttons = row![]
            .spacing(tokens.spacing.field_x)
            .align_y(Alignment::Center);
        for profile in ConversionProfile::ALL {
            buttons = buttons.push(
                button(
                    text(format!("Convert · {}", profile.label())).size(tokens.type_scale.label),
                )
                .on_press_maybe(
                    (item.available && !running)
                        .then(|| Message::StartConversion(item.id.clone(), profile)),
                )
                .style(design::quiet_style(false))
                .padding([tokens.spacing.field_y, tokens.spacing.field_x]),
            );
        }
        if running {
            buttons = buttons.push(
                button(text("Stop converting").size(tokens.type_scale.label))
                    .on_press(Message::CancelConversion)
                    .style(design::danger_style(true))
                    .padding([tokens.spacing.field_y, tokens.spacing.field_x]),
            );
        }
        let mut panel = column![
            text("Convert a copy".to_string()).size(tokens.type_scale.label),
            buttons,
        ]
        .spacing(tokens.spacing.field_y);
        if let Some(job) = job {
            panel = panel.push(text(job.summary()).size(tokens.type_scale.caption).color(
                if job.status == ConversionStatus::Failed {
                    tokens.palette.destructive
                } else {
                    tokens.palette.ink_muted
                },
            ));
            panel = panel.push(
                text(job.output_path.clone())
                    .size(tokens.type_scale.micro)
                    .color(tokens.palette.ink_muted),
            );
        } else {
            panel = panel.push(
                text(format!(
                    "Writes a new file into {}. The original is never renamed, replaced or \
                     deleted.",
                    self.conversion_folder
                ))
                .size(tokens.type_scale.caption)
                .color(tokens.palette.ink_muted),
            );
        }
        panel.into()
    }

    fn conversion_panel(&self) -> Element<'_, Message> {
        let tokens = design::tokens();
        let mut panel = column![
            row![
                button(text("Choose…").size(tokens.type_scale.label))
                    .on_press(Message::ChooseConversionFolder)
                    .style(design::quiet_style(false))
                    .padding([tokens.spacing.field_y, tokens.spacing.field_x]),
                text_input("Folder for converted copies", &self.conversion_folder)
                    .id(SettingsField::ConversionFolder.id())
                    .on_input(Message::ConversionFolderChanged)
                    .style(design::input_style())
                    .padding(tokens.spacing.field_y)
                    .width(Length::Fill),
            ]
            .spacing(tokens.spacing.field_x)
            .align_y(Alignment::Center),
            settings_note(
                "A conversion writes one new file into this folder. The original is never \
                 written to, renamed or deleted, and a read-only drive is refused.",
            ),
        ]
        .spacing(tokens.spacing.field_y);

        if self.conversion_runner.is_some() {
            panel = panel.push(
                row![
                    text(format!("Working · {}%", self.conversion_progress))
                        .size(tokens.type_scale.label),
                    button(text("Stop").size(tokens.type_scale.label))
                        .on_press(Message::CancelConversion)
                        .style(design::danger_style(true))
                        .padding([tokens.spacing.field_y, tokens.spacing.field_x]),
                ]
                .spacing(tokens.spacing.field_x)
                .align_y(Alignment::Center),
            );
        }

        if self.conversions.is_empty() {
            panel = panel.push(
                text("Nothing has been converted yet. Open a film and press Convert a copy.")
                    .size(tokens.type_scale.caption)
                    .color(tokens.palette.ink_muted),
            );
        } else {
            for job in self.conversions.iter().take(5) {
                panel = panel.push(text(job.summary()).size(tokens.type_scale.caption).color(
                    if job.status == ConversionStatus::Failed {
                        tokens.palette.destructive
                    } else {
                        tokens.palette.ink_muted
                    },
                ));
                panel = panel.push(
                    text(job.output_path.clone())
                        .size(tokens.type_scale.micro)
                        .color(tokens.palette.ink_muted),
                );
            }
        }
        panel.into()
    }

    fn choose_folder(&mut self) {
        let start = self
            .sources
            .first()
            .map(|source| PathBuf::from(&source.root_path));
        match crate::picker::choose_folder(start.as_deref()) {
            crate::picker::Picked::Path(folder) => {
                self.new_root = folder.to_string_lossy().to_string();
                self.add_root(folder.to_string_lossy().to_string());
            }
            crate::picker::Picked::Cancelled => {}
            crate::picker::Picked::Unavailable(reason) => {
                self.status = Some(reason.to_string());
            }
        }
    }

    fn choose_relink_folder(&mut self) {
        if self.relink_target.is_none() {
            self.status = Some("Choose the folder to relink first.".to_string());
            return;
        }
        match crate::picker::choose_folder(None) {
            crate::picker::Picked::Path(folder) => {
                self.relink_path = folder.to_string_lossy().to_string();
                self.relink_source();
            }
            crate::picker::Picked::Cancelled => {}
            crate::picker::Picked::Unavailable(reason) => {
                self.status = Some(reason.to_string());
            }
        }
    }

    fn choose_backup_path(&mut self) {
        let name = self
            .backup_path
            .rsplit('/')
            .next()
            .filter(|name| !name.is_empty())
            .unwrap_or("screenroom-backup.db")
            .to_string();
        let start = Path::new(&self.backup_path).parent().map(Path::to_path_buf);
        match crate::picker::choose_backup_path(start.as_deref(), &name) {
            crate::picker::Picked::Path(path) => {
                self.backup_path = path.to_string_lossy().to_string();
            }
            crate::picker::Picked::Cancelled => {}
            crate::picker::Picked::Unavailable(reason) => self.status = Some(reason.to_string()),
        }
    }

    fn choose_restore_file(&mut self) {
        let start = Path::new(&self.restore_path)
            .parent()
            .map(Path::to_path_buf);
        match crate::picker::choose_restore_file(start.as_deref()) {
            crate::picker::Picked::Path(path) => {
                self.restore_path = path.to_string_lossy().to_string();
                self.armed_restore = None;
            }
            crate::picker::Picked::Cancelled => {}
            crate::picker::Picked::Unavailable(reason) => self.status = Some(reason.to_string()),
        }
    }

    fn add_folder(&mut self) {
        let raw = self.new_root.trim().to_string();
        if raw.is_empty() {
            self.error = Some("Type or paste an absolute folder path first.".to_string());
            return;
        }
        self.add_root(raw);
    }

    fn add_root(&mut self, raw: String) {
        let root = PathBuf::from(&raw);
        if !root.is_absolute() {
            self.error = Some("Choose an absolute folder path.".to_string());
            return;
        }

        let source = match self
            .library
            .sources()
            .map(|sources| sources.into_iter().find(|source| source.root_path == raw))
        {
            Ok(Some(source)) => Ok(source),
            Ok(None) => self.library.add_source(&root, None),
            Err(error) => Err(error),
        };
        let source = match source {
            Ok(source) => source,
            Err(error) => return self.report(&error),
        };

        self.new_root.clear();
        self.error = None;
        self.start_scan(source.id);
    }

    fn start_scan(&mut self, source_id: String) {
        self.submit_job(crate::watch::Job::Scan {
            source: source_id,
            quiet: false,
        });
    }

    fn submit_job(&mut self, job: crate::watch::Job) {
        if let Some(next) = self.scan_queue.submit(job) {
            self.start_job(next);
            return;
        }
        self.status = Some(format!(
            "A scan is running; {} more queued.",
            self.scan_queue.queued()
        ));
    }

    fn start_job(&mut self, job: crate::watch::Job) {
        let token = ScanToken::new();
        self.scan_token = Some(token.clone());
        self.scan_visited = 0;
        self.scan_imported = 0;
        self.scan_started_ms = Some(now_ms());

        self.scan_refresh_ms = 0;
        self.scan_quiet = job.is_quiet();
        if !self.scan_quiet {
            self.status = Some("Scanning…".to_string());
        }
        let (sender, receiver) = std::sync::mpsc::channel();
        self.scan_events = Some(receiver);
        let library = Arc::clone(&self.library);
        let progress_sender = sender.clone();
        std::thread::spawn(move || {
            let mut progress = |update: &screenroom_core::ScanUpdate| {
                let _ = progress_sender.send(ScanEvent::Progress {
                    visited: update.visited,
                    imported: update.imported,
                    issues: update.errors.len(),
                    done: false,
                    complete: false,
                    enriched: 0,
                });
            };
            let source_id = job.source().to_string();
            let outcome = match &job {
                crate::watch::Job::Scan { source, .. } => LibraryScanner::new(&library)
                    .with_limits(250_000, 64, Duration::from_secs(2))
                    .scan(source, &token, &mut progress),
                crate::watch::Job::Ingest { root, paths, .. } => LibraryScanner::new(&library)
                    .with_limits(250_000, 64, INGEST_STABLE_AGE)
                    .scan_paths(root, paths, &token, &mut progress),
                crate::watch::Job::Withdraw { root, paths, .. } => LibraryScanner::new(&library)
                    .with_limits(250_000, 64, INGEST_STABLE_AGE)
                    .withdraw_paths(root, paths, &token)
                    .map(|withdrawn| screenroom_core::ScanSummary {
                        visited: withdrawn as u64,
                        imported: 0,
                        issues: Vec::new(),
                        cancelled: false,
                        complete: true,
                    }),
            };
            match outcome {
                Ok(summary) => {
                    let enriched = library
                        .enrich_from_sidecars(Some(&source_id))
                        .map(|enriched| enriched.works)
                        .unwrap_or(0);
                    let settling: Vec<String> = summary
                        .issues
                        .iter()
                        .filter(|issue| issue.code == "still_downloading")
                        .map(|issue| issue.relative_path.clone())
                        .collect();
                    if !settling.is_empty() {
                        let _ = sender.send(ScanEvent::Settling(settling));
                    }
                    let _ = sender.send(ScanEvent::Progress {
                        visited: summary.visited,
                        imported: summary.imported,
                        issues: summary.issues.len(),
                        done: true,
                        complete: summary.complete && !summary.cancelled,
                        enriched,
                    });
                }
                Err(error) => {
                    let _ = sender.send(ScanEvent::Failed(format!("{}: {}", error.code(), error)));
                }
            }
        });
    }

    fn drain_scan(&mut self) {
        let Some(receiver) = self.scan_events.as_ref() else {
            return;
        };
        let quiet = self.scan_quiet;
        let ingest_job = self.scan_queue.running().is_some_and(|job| !job.is_scan());
        let mut finished = false;
        let mut failure = None;
        let mut status = None;
        let mut refresh = false;
        let mut settling = Vec::new();
        loop {
            match receiver.try_recv() {
                Ok(ScanEvent::Progress {
                    visited,
                    imported,
                    issues,
                    done,
                    complete,
                    enriched,
                }) => {
                    self.scan_visited = visited;
                    self.scan_imported = imported;
                    if done {
                        finished = true;
                        let message = if ingest_job {
                            format!(
                                "Added {imported} {}.",
                                if imported == 1 { "file" } else { "files" }
                            )
                        } else if complete && enriched > 0 {
                            format!(
                                "Scan finished — {visited} entries, {imported} imported, \
                                 details from {enriched} local NFO file(s)."
                            )
                        } else if complete {
                            format!("Scan finished — {visited} entries, {imported} imported.")
                        } else {
                            format!(
                                "Scan ended early — {visited} entries, {imported} imported, \
                                 {issues} issue(s). Nothing was deleted."
                            )
                        };
                        status = (!quiet).then_some(message);
                    } else if !quiet {
                        status = Some(format!("Scanning… {visited} entries, {imported} imported"));

                        if now_ms() - self.scan_refresh_ms >= SCAN_REFRESH_INTERVAL_MS {
                            self.scan_refresh_ms = now_ms();
                            refresh = true;
                        }
                    }
                }
                Ok(ScanEvent::Settling(paths)) => settling.extend(paths),
                Ok(ScanEvent::Failed(message)) => {
                    finished = true;
                    failure = Some(message);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    finished = true;
                    break;
                }
            }
        }
        if let Some(status) = status {
            self.status = Some(status);
        }
        if let Some(failure) = failure {
            self.error = Some(failure);
        }

        if refresh && !finished {
            self.reload_home();
            self.reload_browse();
        }
        if finished {
            let finished_job = self.scan_queue.running().cloned();
            self.scan_events = None;
            self.scan_token = None;
            self.scan_started_ms = None;
            self.scan_quiet = false;
            self.reload();

            self.start_durations();
            if let Some(crate::watch::Job::Ingest { source, root, .. }) = finished_job {
                self.defer_settling(&source, &root, settling);
            }
            if let Some(next) = self.scan_queue.finish() {
                self.start_job(next);
            }
        }
    }

    fn defer_settling(&mut self, source: &str, root: &Path, settling: Vec<String>) {
        for relative in settling {
            if self.watch_deferred.len() >= MAX_DEFERRED {
                self.submit_job(crate::watch::Job::Scan {
                    source: source.to_string(),
                    quiet: true,
                });
                return;
            }
            let path = root.join(&relative);
            if self
                .watch_deferred
                .iter()
                .any(|deferred| deferred.source == source && deferred.path == path)
            {
                continue;
            }
            self.watch_deferred.push(DeferredPath {
                at: std::time::Instant::now() + DEFERRED_RETRY,
                source: source.to_string(),
                root: root.to_path_buf(),
                path,
            });
        }
    }

    fn drain_deferred(&mut self) {
        if self.watch_deferred.is_empty() {
            return;
        }
        let now = std::time::Instant::now();
        let mut due: std::collections::BTreeMap<(String, PathBuf), Vec<PathBuf>> =
            std::collections::BTreeMap::new();
        let mut index = 0;
        while index < self.watch_deferred.len() {
            if self.watch_deferred[index].at <= now {
                let entry = self.watch_deferred.swap_remove(index);
                due.entry((entry.source, entry.root))
                    .or_default()
                    .push(entry.path);
            } else {
                index += 1;
            }
        }
        for ((source, root), paths) in due {
            self.submit_job(crate::watch::Job::Ingest {
                source,
                root,
                paths,
            });
        }
    }

    fn drain_watch(&mut self) {
        for event in self.watchers.poll() {
            self.handle_watched(event);
        }
    }

    fn handle_watched(&mut self, event: crate::watch::Watched) {
        match event {
            crate::watch::Watched::Changes { root, changes } => {
                let work = crate::watch::work_for(&root, &changes);
                self.apply_work(&root, work);
            }
            crate::watch::Watched::Rescan { root, reason } => {
                self.apply_work(
                    &root,
                    crate::watch::Work {
                        walk: true,
                        ..crate::watch::Work::default()
                    },
                );
                self.status = Some(format!("{} — {reason}. Checking it again.", root.display()));
            }
            crate::watch::Watched::Stopped { root, reason } => {
                self.status = Some(format!(
                    "Stopped watching {} ({reason}). The library re-checks on its own.",
                    root.display()
                ));
            }
        }
    }

    fn apply_work(&mut self, root: &Path, work: crate::watch::Work) {
        if work.is_empty() {
            return;
        }
        let Some((source, stored_root)) = self.source_for_root(root) else {
            return;
        };
        if work.walk {
            self.submit_job(crate::watch::Job::Scan {
                source,
                quiet: true,
            });
            return;
        }
        if !work.withdraw.is_empty() {
            self.submit_job(crate::watch::Job::Withdraw {
                source: source.clone(),
                paths: self.stored_paths(root, &stored_root, &work.withdraw),
                root: stored_root.clone(),
            });
        }
        if !work.ingest.is_empty() {
            self.submit_job(crate::watch::Job::Ingest {
                source,
                paths: self.stored_paths(root, &stored_root, &work.ingest),
                root: stored_root,
            });
        }
    }

    fn stored_paths(&self, root: &Path, stored_root: &Path, paths: &[PathBuf]) -> Vec<PathBuf> {
        paths
            .iter()
            .map(|path| match path.strip_prefix(root) {
                Ok(relative) => stored_root.join(relative),
                Err(_) => path.clone(),
            })
            .collect()
    }

    fn source_for_root(&self, root: &Path) -> Option<(String, PathBuf)> {
        if let Some(source) = self
            .sources
            .iter()
            .find(|source| Path::new(&source.root_path) == root)
        {
            return Some((source.id.clone(), PathBuf::from(&source.root_path)));
        }
        let canonical = std::fs::canonicalize(root).ok()?;
        self.sources
            .iter()
            .find(|source| {
                std::fs::canonicalize(&source.root_path).is_ok_and(|stored| stored == canonical)
            })
            .map(|source| (source.id.clone(), PathBuf::from(&source.root_path)))
    }

    fn arm_watchers(&mut self) -> Vec<PathBuf> {
        let roots: Vec<PathBuf> = self
            .sources
            .iter()
            .map(|source| PathBuf::from(&source.root_path))
            .filter(|root| root.is_dir())
            .collect();
        self.watchers.sync(&roots)
    }

    fn reconcile(&mut self) {
        self.watch_reconcile_at = std::time::Instant::now();
        self.watch_reconciled = true;
        self.watchers.clear_retries();
        self.arm_watchers();
        for source in self.sources.clone() {
            if !Path::new(&source.root_path).is_dir() {
                continue;
            }
            self.submit_job(crate::watch::Job::Scan {
                source: source.id,
                quiet: true,
            });
        }
    }

    fn catch_up_arrivals(&mut self) {
        for root in self.arm_watchers() {
            let Some((source, _)) = self.source_for_root(&root) else {
                continue;
            };
            let walking = self
                .scan_queue
                .running()
                .is_some_and(|job| job.is_scan() && job.source() == source)
                || self.scan_queue.is_scan_queued(&source);
            if walking {
                continue;
            }
            self.submit_job(crate::watch::Job::Scan {
                source,
                quiet: true,
            });
        }
    }

    fn watch_step(&mut self) {
        if !self.auto_discovery {
            return;
        }
        self.drain_watch();
        self.drain_deferred();
        let wall = crate::watch::clock();
        let wall_gap = wall.saturating_sub(self.watch_wall);
        let monotonic_gap = self.watch_tick.elapsed();
        self.watch_wall = wall;
        self.watch_tick = std::time::Instant::now();
        let first = !self.watch_reconciled;
        let woke = crate::watch::slept(wall_gap, monotonic_gap);
        let due = self.watch_reconcile_at.elapsed() >= crate::watch::RECONCILE_INTERVAL;
        if first || woke || due {
            self.reconcile();
        } else if self.watch_last_arrival_check.elapsed() >= crate::watch::ARRIVAL_RETRY {
            self.watch_last_arrival_check = std::time::Instant::now();
            self.catch_up_arrivals();
        }
    }

    fn scan_strip(&self) -> Option<Element<'_, Message>> {
        self.scan_token.as_ref()?;
        if self.scan_quiet {
            return None;
        }
        let tokens = design::tokens();
        let elapsed = self
            .scan_started_ms
            .map(|started| (now_ms() - started).max(0) / 1_000)
            .unwrap_or(0);
        Some(
            container(
                row![
                    text("Scanning").size(tokens.type_scale.label),
                    text(format!(
                        "{} entries · {} imported · {}s",
                        self.scan_visited, self.scan_imported, elapsed
                    ))
                    .size(tokens.type_scale.caption)
                    .color(tokens.palette.ink_muted),
                    Space::new().width(Length::Fill),
                    button(text("Cancel scan").size(tokens.type_scale.caption))
                        .on_press(Message::CancelScan)
                        .style(design::quiet_style(false))
                        .padding([tokens.spacing.field_y, tokens.spacing.field_x]),
                ]
                .spacing(tokens.spacing.field_x)
                .align_y(Alignment::Center),
            )
            .style(|_theme| design::container_style(Level::Panel, false))
            .padding(tokens.spacing.field_y)
            .width(Length::Fill)
            .into(),
        )
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        let before = self.screen;
        let task = self.dispatch(message);
        if self.screen != before {
            self.motion.flush_rails();
            self.motion.clear_lifts();
            let direction = (screen_depth(self.screen) - screen_depth(before)).signum();
            self.motion.arrive(direction as f32);
        }
        self.sync_motion();
        task
    }

    fn reduced_motion(&self) -> bool {
        self.preferences.reduced_motion || crate::motion::system_reduce_motion()
    }

    fn motion_focus_index(&self) -> Option<usize> {
        match self.screen {
            Screen::Home => self.home_flat_index(),
            Screen::Browse => (self.focus < self.browse.items.len()).then_some(self.focus),
            Screen::Detail | Screen::Settings | Screen::Player => None,
        }
    }

    fn player_chrome_held(&self) -> bool {
        self.track_menu_open.is_some() || !self.pointer_buttons.is_empty()
    }

    fn set_chrome(&mut self, visible: bool) {
        if self.player_chrome == visible {
            return;
        }
        self.player_chrome = visible;
        self.motion
            .place_chrome(visible && self.screen == Screen::Player);
    }

    fn sync_motion(&mut self) {
        self.motion.set_reduced(self.reduced_motion());
        match self.motion_focus_index() {
            Some(index) => self.motion.focus(index),
            None => self.motion.release_focus(),
        }
        self.motion
            .set_chrome(self.player_chrome && self.screen == Screen::Player);
    }

    fn rail_viewport(&self) -> f32 {
        let width = if self.window_width.is_finite() {
            self.window_width
        } else {
            INITIAL_WINDOW_WIDTH
        };
        ui::screens::home::rail_span(width).max(1.0)
    }

    fn scroll_focused_rail(&mut self) -> Task<Message> {
        let Some((label, count)) = self
            .home_sections()
            .get(self.nav.focus().rail)
            .map(|(label, items)| (*label, items.len()))
        else {
            return Task::none();
        };
        let column = self.nav.focus().column;
        let card = ui::theme::Breakpoint::at(self.window_width).card();
        let extent = ui::components::rail::extent(column, true, count, card, self.rail_viewport());
        self.motion.scroll_rail(
            label,
            ui::components::rail::focus_offset(column, card),
            extent,
        );
        Task::none()
    }

    fn drive_motion(&mut self) -> Task<Message> {
        let mut tasks = Vec::new();
        for (label, offset) in self.motion.moving_rails() {
            tasks.push(ui::components::rail::scroll_task(&label, offset));
        }
        if !self.motion.page_settled() {
            tasks.push(ui::screens::library::scroll_offset_task(
                self.motion.page_offset(),
            ));
        }
        Task::batch(tasks)
    }

    fn dispatch(&mut self, message: Message) -> Task<Message> {
        if !matches!(message, Message::Tick | Message::MotionTick(_)) {
            self.last_input = std::time::Instant::now();
        }
        if self.route_quit() {
            return iced::exit();
        }
        match message {
            Message::GoHome => {
                let leave = self.end_viewing();
                self.navigate(Screen::Home);
                self.focus_card(0);
                self.reload_home();
                return leave;
            }
            Message::GoBrowse => {
                let leave = self.end_viewing();
                self.navigate(Screen::Browse);
                self.focus_card(0);
                self.reload_browse();
                return leave;
            }
            Message::Back => self.back(),
            Message::Refresh => {
                self.reload();
                self.refresh_detail();
            }

            Message::SearchChanged(text) => {
                self.browse.text = text;
                self.browse.page = 0;
                self.reload_browse();
            }
            Message::KindChanged(kind) => {
                self.browse.kind = kind;
                self.browse.page = 0;
                self.reload_browse();
            }
            Message::GenreChanged(genre) => {
                self.browse.genre = genre;
                self.browse.page = 0;
                self.reload_browse();
            }
            Message::WatchChanged(watch) => {
                self.browse.watch = watch;
                self.browse.page = 0;
                self.reload_browse();
            }
            Message::SortChanged(sort) => {
                self.browse.sort = sort;
                self.browse.page = 0;
                self.reload_browse();
            }
            Message::ToggleWatchlistOnly(value) => {
                self.browse.watchlist_only = value;
                self.browse.page = 0;
                self.reload_browse();
            }
            Message::ToggleFavoritesOnly(value) => {
                self.browse.favorites_only = value;
                self.browse.page = 0;
                self.reload_browse();
            }
            Message::ToggleAvailableOnly(value) => {
                self.browse.available_only = value;
                self.browse.page = 0;
                self.reload_browse();
            }
            Message::NextPage => {
                if self.browse.page + 1 < self.browse.page_count() {
                    self.browse.page += 1;
                    self.focus_card(0);
                    self.motion.place_page(0.0);
                    self.reload_browse();
                    return ui::screens::library::scroll_offset_task(0.0);
                }
            }
            Message::PreviousPage => {
                if self.browse.page > 0 {
                    self.browse.page -= 1;
                    self.focus_card(0);
                    self.motion.place_page(0.0);
                    self.reload_browse();
                    return ui::screens::library::scroll_offset_task(0.0);
                }
            }

            Message::Move(direction) => return self.move_focus(direction),
            Message::OpenDetail(file_id) => self.open_detail(&file_id),
            Message::SeasonChosen(season) => {
                if let Some(detail) = self.detail.as_mut() {
                    detail.season = season;
                    detail.refresh_shown();
                }
                self.focus_card(0);
            }
            Message::SettingsSection(section) => {
                self.settings_section = section;
                self.settings_focus = 0;
            }
            Message::Play(file_id) => self.open_player(&file_id, false),
            Message::Restart(file_id) => self.open_player(&file_id, true),
            Message::ToggleFavorite(work_id) => {
                self.set_flag(&work_id, Flag::Favorite);
            }
            Message::ToggleWatchlist(work_id) => {
                self.set_flag(&work_id, Flag::Watchlist);
            }
            Message::ToggleWatched(edition_id) => {
                let watched = self
                    .detail
                    .as_ref()
                    .map(|detail| detail.item.watched)
                    .unwrap_or(false);
                if let Err(error) =
                    self.library
                        .mark_watched(&self.profile_id, &edition_id, !watched)
                {
                    self.report(&error);
                } else {
                    self.reload();
                    self.refresh_detail();
                }
            }
            Message::HideFromContinue(edition_id) => {
                if let Err(error) = self.library.hide_continue(&self.profile_id, &edition_id) {
                    self.report(&error);
                } else {
                    self.status = Some("Removed from Continue Watching.".to_string());
                    self.reload();
                }
            }

            Message::NewRootChanged(value) => self.new_root = value,
            Message::AddFolder => self.add_folder(),
            Message::ChooseFolder => self.choose_folder(),
            Message::ChooseRelinkFolder => self.choose_relink_folder(),
            Message::ChooseBackupPath => self.choose_backup_path(),
            Message::ChooseRestoreFile => self.choose_restore_file(),
            Message::RescanSource(source_id) => self.rescan_source(source_id),
            Message::RemoveSource(source_id) => self.remove_source(source_id),
            Message::CancelArmed => {
                self.armed_remove = None;
                self.armed_restore = None;
                self.status = None;
            }
            Message::RelinkTargetChosen(source_id) => self.relink_target = Some(source_id),
            Message::RelinkPathChanged(value) => self.relink_path = value,
            Message::RelinkSource => self.relink_source(),

            Message::GoSettings => {
                let leave = self.end_viewing();
                self.navigate(Screen::Settings);
                self.focus_card(0);
                self.armed_remove = None;
                self.armed_restore = None;
                self.armed_profile = None;
                self.reload_administration();
                return leave;
            }
            Message::ProfileChosen(profile_id) => self.choose_profile(profile_id),
            Message::NewProfileChanged(value) => self.new_profile = value,
            Message::AddProfile => self.add_profile(),
            Message::RemoveProfile(profile_id) => self.remove_profile(profile_id),
            Message::ToggleAutoplay(enabled) => {
                let preferences = Preferences {
                    autoplay_next: enabled,
                    ..self.preferences.clone()
                };
                self.save_preferences(preferences);
            }
            Message::ToggleAutoSkip(enabled) => {
                let preferences = Preferences {
                    auto_skip_intro: enabled,
                    ..self.preferences.clone()
                };
                self.save_preferences(preferences);
            }
            Message::ToggleReducedMotion(enabled) => {
                self.motion
                    .set_reduced(enabled || crate::motion::system_reduce_motion());
                let preferences = Preferences {
                    reduced_motion: enabled,
                    ..self.preferences.clone()
                };
                self.save_preferences(preferences);
                if enabled {
                    self.stop_preview();
                }
            }
            Message::HoverCard(file_id) => {
                if self.screen == Screen::Browse
                    && let Some(index) =
                        self.browse.items.iter().position(|item| item.id == file_id)
                {
                    self.focus_card(index);
                }
            }
            Message::SkipSegment => self.skip_segment(),
            Message::SubtitleLanguageChanged(value) => self.subtitle_input = value,
            Message::AudioLanguageChanged(value) => self.audio_input = value,
            Message::Resized(size) => {
                self.track_menu_open = None;
                self.pointer_buttons.clear();
                if size.width.is_finite() && size.width > 0.0 {
                    self.window_width = size.width;
                }
                if size.height.is_finite() && size.height > 0.0 {
                    self.window_height = size.height;
                }

                return iced::window::latest()
                    .and_then(iced::window::mode)
                    .map(Message::ModeChanged);
            }
            Message::SubtitleScale(value) => self.apply_subtitle_style(
                round_percent(
                    value,
                    Preferences::SUBTITLE_SCALE_MIN_PERCENT,
                    Preferences::SUBTITLE_SCALE_MAX_PERCENT,
                ),
                self.preferences.subtitle_position_percent,
            ),
            Message::SubtitlePosition(value) => self.apply_subtitle_style(
                self.preferences.subtitle_scale_percent,
                round_percent(
                    value,
                    Preferences::SUBTITLE_POSITION_MIN_PERCENT,
                    Preferences::SUBTITLE_POSITION_MAX_PERCENT,
                ),
            ),
            Message::FocusSettingsField(field) => {
                return operation::focus(Id::new(field.id()));
            }
            Message::ApplySubtitleLanguage => self.apply_language(true),
            Message::ApplyAudioLanguage => self.apply_language(false),
            Message::BackupPathChanged(value) => self.backup_path = value,
            Message::BookmarkLabelChanged(value) => self.bookmark_label = value,
            Message::BookmarkAddressChanged(value) => self.bookmark_address = value,
            Message::BookmarkAccessChanged(value) => self.bookmark_access = value,
            Message::AddBookmark => self.add_bookmark(),
            Message::RemoveBookmark(id) => self.remove_bookmark(&id),
            Message::OpenBookmark(id) => self.open_bookmark(&id),
            Message::ImportSubtitle => self.import_subtitle(),
            Message::SubtitleChosen(path) => self.subtitle_chosen(&path),
            Message::UseSubtitle(id) => self.use_subtitle(&id),
            Message::RemoveSubtitle(id) => self.remove_subtitle(&id),
            Message::LookForReceivers => self.look_for_receivers(),
            Message::BackupNow => self.backup_now(),
            Message::EnrichmentNow => self.enrichment_now(),
            Message::ToggleArtworkRetention => self.toggle_artwork_retention(),
            Message::FindSubtitles => self.find_subtitles(),
            Message::SubtitleDownload { id, format } => self.subtitle_download(id, &format),
            Message::CastTo(id) => self.cast_to(&id),
            Message::CastPause => self.cast_action(CastAction::Pause),
            Message::CastStop => self.cast_action(CastAction::Stop),
            Message::CastDisconnect => self.cast_action(CastAction::Disconnect),
            Message::CancelBackup => self.cancel_backup(),
            Message::RestorePathChanged(value) => {
                self.restore_path = value;

                self.armed_restore = None;
            }
            Message::InspectRestore => self.inspect_restore(),
            Message::RestoreNow => self.restore_now(),
            Message::ChooseConversionFolder => self.choose_conversion_folder(),
            Message::ConversionFolderChanged(value) => self.conversion_folder = value,
            Message::StartConversion(file_id, profile) => self.start_conversion(file_id, profile),
            Message::CancelConversion => {
                if let Some(runner) = &self.conversion_runner {
                    runner.cancel();
                    self.status = Some("Stopping the conversion…".to_string());
                }
                if let Some(run) = &self.conversion_run {
                    let _ = self.library.cancel_conversion(&run.job_id);
                }
            }
            Message::ClearStatus => {
                self.status = None;
                self.error = None;
            }
            Message::CancelScan => {
                self.scan_queue.cancel_pending();
                if let Some(token) = &self.scan_token {
                    token.cancel();
                    self.status = Some("Stopping the scan…".to_string());
                }
            }

            Message::BackFromPlayer => self.leave_player(),
            Message::PlayNextNow => self.play_up_next(),
            Message::ModeChanged(mode) => {
                self.fullscreen = mode == iced::window::Mode::Fullscreen;
                self.mode_change = None;
            }
            Message::CancelNext => {
                if self.up_next.take().is_some() {
                    self.status = Some("Autoplay cancelled.".to_string());
                }
            }
            Message::TogglePlay => self.toggle_play(),
            Message::Volume(percent) => self.drag_volume(percent),
            Message::VolumeCommit => self.commit_volume(),
            Message::ToggleMute => self.toggle_mute(),
            Message::ToggleFullscreen => return self.toggle_fullscreen(),
            Message::Scrub(seconds) => self.scrub = Some(seconds),
            Message::SeekCommit => self.commit_seek(),
            Message::SeekStep(delta) => self.seek_step(delta),
            Message::SelectTrack(kind, id) => {
                if self.track_menu_open == Some(kind) {
                    self.track_menu_open = None;
                }
                if let Some(player) = &self.player {
                    player.select_track(kind, id);
                }
            }
            Message::PlayerWake => {
                self.set_chrome(true);
                return Task::none();
            }
            Message::PointerInside(inside) => {
                self.pointer_inside = inside;
                if inside {
                    self.last_input = std::time::Instant::now();
                    self.set_chrome(true);
                } else if !self.player_chrome_held() {
                    self.set_chrome(false);
                }
                return Task::none();
            }
            Message::TrackMenuOpened(kind) => {
                self.track_menu_open = Some(kind);
                self.set_chrome(true);
                return Task::none();
            }
            Message::TrackMenuClosed(kind) => {
                if self.track_menu_open == Some(kind) {
                    self.track_menu_open = None;
                }
                return Task::none();
            }
            Message::PointerMoved { x, y } => {
                let inside = self.window_width.is_finite()
                    && self.window_height.is_finite()
                    && x >= 0.0
                    && y >= 0.0
                    && x <= self.window_width
                    && y <= self.window_height;
                if inside != self.pointer_inside {
                    self.pointer_inside = inside;
                    if inside {
                        self.last_input = std::time::Instant::now();
                        self.set_chrome(true);
                    } else if !self.player_chrome_held() {
                        self.set_chrome(false);
                    }
                }
                return Task::none();
            }
            Message::PointerButton { button, pressed } => {
                if pressed {
                    self.pointer_buttons.insert(button);
                } else {
                    self.pointer_buttons.remove(&button);
                }
                return Task::none();
            }
            Message::MotionTick(now) => {
                self.motion.step(now);
                return self.drive_motion();
            }
            Message::RailScrolled { label, offset } => {
                self.motion.adopt_rail(&label, offset);
            }
            Message::Tick => {
                self.poll();
                let playing =
                    matches!(self.snapshot.state, screenroom_player::PlayerState::Playing)
                        && self.up_next.is_none();
                let recent = self
                    .keyboard_reveal_at
                    .is_some_and(|at| at.elapsed() < ui::screens::player::CHROME_IDLE_AFTER);
                let visible = self.player_chrome_held()
                    || ((self.pointer_inside || recent)
                        && ui::screens::player::chrome_visible(playing, self.last_input.elapsed()));
                self.set_chrome(visible);
                if self.drive_play_check() {
                    return iced::exit();
                }
                return Task::none();
            }
            Message::Keyboard(event) => {
                self.keyboard_reveal_at = Some(std::time::Instant::now());
                return self.handle_key(event);
            }
            Message::RequestTerminate => {
                crate::quit::request_termination();
            }
            Message::Close => {
                let reached = self
                    .player
                    .as_ref()
                    .map(|player| player.snapshot().position);
                self.stop_playback();
                self.record_terminate_check(reached);
                return iced::exit();
            }
        }
        Task::none()
    }

    fn move_focus(&mut self, direction: Focus) -> Task<Message> {
        if self.screen == Screen::Home {
            let rails: Vec<(&'static str, usize)> = self
                .home_sections()
                .iter()
                .map(|(label, items)| (*label, items.len()))
                .collect();
            let counts: Vec<usize> = rails.iter().map(|(_, count)| *count).collect();
            if counts.is_empty() {
                self.focus_card(0);
                return Task::none();
            }
            self.nav.move_focus(
                match direction {
                    Focus::Left => ui::route::Focus::Left,
                    Focus::Right => ui::route::Focus::Right,
                    Focus::Up => ui::route::Focus::Up,
                    Focus::Down => ui::route::Focus::Down,
                },
                &counts,
                ui::theme::Breakpoint::at(self.window_width).columns(),
            );
            self.sync_navigation();
            return self.scroll_focused_rail();
        }

        let len = self.visible_len();
        if len == 0 {
            self.focus_card(0);
            return Task::none();
        }
        let current = self.focus.min(len - 1);

        let step = match self.screen {
            Screen::Detail => 1,
            _ => ui::theme::Breakpoint::at(self.window_width).columns(),
        };
        let next = match direction {
            Focus::Left => current.checked_sub(1),
            Focus::Right => (current + 1 < len).then_some(current + 1),
            Focus::Up => current.checked_sub(step),
            Focus::Down => {
                let candidate = current + step;
                (candidate < len).then_some(candidate)
            }
        };
        self.focus_card(next.unwrap_or(current));

        if self.screen == Screen::Browse {
            let target = ui::screens::library::focus_offset(self.nav.focus(), self.window_width);
            self.motion.scroll_page(target);
        }
        Task::none()
    }

    fn activate(&mut self) {
        let file_id = match self.screen {
            Screen::Home => self.home_selected_item().map(|item| item.id.clone()),
            Screen::Browse => self
                .browse
                .items
                .get(self.focus)
                .map(|item| item.id.clone()),

            Screen::Detail => self.detail.as_ref().map(|detail| {
                detail
                    .season_episodes()
                    .get(self.focus)
                    .map(|episode| episode.id.clone())
                    .unwrap_or_else(|| detail.item.id.clone())
            }),
            Screen::Player | Screen::Settings => None,
        };
        match (self.screen, file_id) {
            (Screen::Detail, Some(file_id)) => self.open_player(&file_id, false),
            (_, Some(file_id)) => self.open_detail(&file_id),
            _ => {}
        }
    }

    fn back(&mut self) {
        match self.screen {
            Screen::Player => self.leave_player(),
            Screen::Detail => {
                let (screen, column) = self
                    .detail
                    .take()
                    .map(|detail| detail.origin)
                    .unwrap_or((Screen::Browse, 0));
                if self.nav.pop().is_none() {
                    self.navigate(if screen == Screen::Home {
                        Screen::Home
                    } else {
                        Screen::Browse
                    });
                    self.focus_card(column);
                }
                self.sync_navigation();
                self.reload();
            }
            Screen::Browse | Screen::Settings => self.navigate(Screen::Home),
            Screen::Home => {}
        }
    }

    fn set_flag(&mut self, work_id: &str, flag: Flag) {
        let current = self
            .detail
            .as_ref()
            .map(|detail| match flag {
                Flag::Favorite => detail.item.favorite,
                Flag::Watchlist => detail.item.watchlisted,
            })
            .unwrap_or(false);
        let result = match flag {
            Flag::Favorite => self
                .library
                .set_favorite(&self.profile_id, work_id, !current),
            Flag::Watchlist => self
                .library
                .set_watchlisted(&self.profile_id, work_id, !current),
        };
        match result {
            Ok(()) => {
                self.reload();
                self.refresh_detail();
            }
            Err(error) => self.report(&error),
        }
    }

    fn handle_key(&mut self, event: keyboard::Event) -> Task<Message> {
        use iced::keyboard::Key;
        use iced::keyboard::key::Named;
        let keyboard::Event::KeyPressed { key, modifiers, .. } = event else {
            return Task::none();
        };

        if modifiers.command() || modifiers.control() {
            match key {
                Key::Character(ref character) if character.as_str() == "1" => {
                    let _ = self.update(Message::GoHome);
                }
                Key::Character(ref character) if character.as_str() == "2" => {
                    let _ = self.update(Message::GoBrowse);
                }
                Key::Character(ref character) if character.as_str() == "3" => {
                    let _ = self.update(Message::GoSettings);
                }
                _ => {}
            }
            return Task::none();
        }

        match self.screen {
            Screen::Player => {
                self.set_chrome(true);
                match key {
                    Key::Named(Named::Escape) => {
                        if self.up_next.take().is_some() {
                            self.status = Some("Autoplay cancelled.".to_string());
                            return Task::none();
                        }

                        if self.fullscreen
                            || self.pending_mode() == Some(iced::window::Mode::Fullscreen)
                        {
                            return self.leave_fullscreen();
                        }
                        self.leave_player();
                    }
                    Key::Named(Named::Space) => self.toggle_play(),
                    Key::Named(Named::ArrowUp) if !self.subtitle_actions().is_empty() => {
                        self.move_subtitle_focus(-1);
                    }
                    Key::Named(Named::ArrowDown) if !self.subtitle_actions().is_empty() => {
                        self.move_subtitle_focus(1);
                    }
                    Key::Named(Named::Enter) if self.subtitle_enter_target().is_some() => {
                        return self.activate_subtitle();
                    }
                    Key::Named(Named::Enter) if self.up_next.is_some() => self.play_up_next(),
                    Key::Named(Named::ArrowLeft) => self.seek_step(-SEEK_STEP),
                    Key::Named(Named::ArrowRight) => self.seek_step(SEEK_STEP),
                    Key::Character(character) => match character.as_str() {
                        "f" => return self.toggle_fullscreen(),
                        "m" => self.toggle_mute(),
                        "s" => self.skip_segment(),
                        "t" => self.find_subtitles(),
                        "i" => self.toggle_segment_mark("intro"),
                        "c" => self.toggle_segment_mark("credits"),
                        _ => {}
                    },
                    _ => {}
                }
            }
            Screen::Detail => match key {
                Key::Named(Named::Escape) => self.back(),
                Key::Named(Named::Enter) => self.activate(),
                Key::Named(Named::ArrowUp) => return self.move_focus(Focus::Up),
                Key::Named(Named::ArrowDown) => return self.move_focus(Focus::Down),
                Key::Character(character) => match character.as_str() {
                    "f" => self.toggle_focused_flag(Flag::Favorite),
                    "w" => self.toggle_focused_flag(Flag::Watchlist),
                    "m" => self.toggle_focused_watched(),
                    _ => {}
                },
                _ => {}
            },

            Screen::Settings => match key {
                Key::Named(Named::Escape) => self.back(),
                Key::Named(Named::Enter) => return self.run_settings_action(),
                Key::Named(Named::ArrowUp) => self.move_settings_focus(false),
                Key::Named(Named::ArrowDown) => self.move_settings_focus(true),
                Key::Named(Named::ArrowLeft) => self.step_settings_section(false),
                Key::Named(Named::ArrowRight) => self.step_settings_section(true),
                _ => {}
            },
            _ => match key {
                Key::Named(Named::Escape) => self.back(),
                Key::Named(Named::Enter) => self.activate(),
                Key::Named(Named::ArrowLeft) => return self.move_focus(Focus::Left),
                Key::Named(Named::ArrowRight) => return self.move_focus(Focus::Right),
                Key::Named(Named::ArrowUp) => return self.move_focus(Focus::Up),
                Key::Named(Named::ArrowDown) => return self.move_focus(Focus::Down),
                _ => {}
            },
        }
        Task::none()
    }

    fn pending_mode(&self) -> Option<iced::window::Mode> {
        self.mode_change
            .filter(|(_, since)| since.elapsed() < MODE_SETTLE)
            .map(|(mode, _)| mode)
    }

    fn request_mode(&mut self, mode: iced::window::Mode) -> Task<Message> {
        self.fullscreen = mode == iced::window::Mode::Fullscreen;
        self.mode_change = Some((mode, std::time::Instant::now()));

        iced::window::latest().and_then(move |id| iced::window::set_mode(id, mode))
    }

    fn toggle_fullscreen(&mut self) -> Task<Message> {
        if self.pending_mode().is_some() {
            return Task::none();
        }
        let mode = if self.fullscreen {
            iced::window::Mode::Windowed
        } else {
            iced::window::Mode::Fullscreen
        };
        self.request_mode(mode)
    }

    fn leave_fullscreen(&mut self) -> Task<Message> {
        if self.pending_mode() == Some(iced::window::Mode::Windowed) {
            return Task::none();
        }
        self.request_mode(iced::window::Mode::Windowed)
    }

    fn record_terminate_check(&self, reached_seconds: Option<f64>) {
        let Some(path) = &self.terminate_check else {
            return;
        };
        let reached = match reached_seconds {
            Some(seconds) if seconds.is_finite() && seconds > 0.0 => format!("{seconds:.3}"),
            _ => "-".to_string(),
        };
        let text = format!("closed {} {reached}\n", self.terminate_routing);
        if let Err(error) = std::fs::write(path, text) {
            eprintln!(
                "screenroom: could not write the terminate check to {}: {error}",
                path.display()
            );
        }
    }

    fn seek_step(&mut self, delta: f64) {
        if !(self.snapshot.duration.is_finite() && self.snapshot.duration > 0.0) {
            return;
        }
        let target = (self.snapshot.position + delta).clamp(0.0, self.snapshot.duration);
        self.seek_to(target);
    }

    fn toggle_focused_flag(&mut self, flag: Flag) {
        let Some(work_id) = self
            .detail
            .as_ref()
            .map(|detail| detail.item.work_id.clone())
        else {
            return;
        };
        self.set_flag(&work_id, flag);
    }

    fn toggle_focused_watched(&mut self) {
        let Some(edition_id) = self
            .detail
            .as_ref()
            .map(|detail| detail.item.edition_id.clone())
        else {
            return;
        };
        let _ = self.update(Message::ToggleWatched(edition_id));
    }

    fn subscription(&self) -> Subscription<Message> {
        let mut subscriptions = vec![
            iced::keyboard::listen().map(Message::Keyboard),
            iced::window::close_requests().map(|_| Message::Close),
            iced::window::resize_events().map(|(_id, size)| Message::Resized(size)),
            iced::event::listen_with(|event, _status, _id| crate::app::pointer_event(&event)),
        ];
        let interval = if self.screen == Screen::Player || self.preview.is_some() {
            PLAYER_TICK
        } else {
            STATUS_TICK
        };
        subscriptions.push(ticks(interval, STATUS_TICK, self.frame_signals()));
        if !self.motion.idle() {
            subscriptions.push(motion_ticks());
        }
        Subscription::batch(subscriptions)
    }

    fn frame_signals(&self) -> Vec<screenroom_player::FrameTicks> {
        let mut signals = Vec::new();
        if let Some(player) = &self.player
            && self.screen == Screen::Player
        {
            signals.push(player.frame_ticks());
        }
        if let Some(preview) = &self.preview {
            signals.push(preview.frame_ticks());
        }
        signals
    }

    fn theme(&self) -> iced::Theme {
        design::theme()
    }

    fn view(&self) -> Element<'_, Message> {
        let body = match self.screen {
            Screen::Home => self.home_screen(),
            Screen::Browse => self.library_screen(),
            Screen::Detail => self.detail_screen(),
            Screen::Player => self.player_screen(),
            Screen::Settings => self.settings_screen(),
        };

        if self.fullscreen && self.screen == Screen::Player {
            return container(body)
                .style(|_theme| design::page_style())
                .padding(0)
                .width(Length::Fill)
                .height(Length::Fill)
                .into();
        }

        let mut content = body;
        if let Some(strip) = self.scan_strip() {
            content = column![self.gutter(strip), content]
                .spacing(design::tokens().spacing.content_pad)
                .width(Length::Fill)
                .into();
        }

        let framed = crate::ui::shell::frame(
            content,
            Some(self.screen),
            self.status.as_deref(),
            self.error.as_deref(),
            self.window_width,
        );

        self.stage(framed)
    }

    fn stage<'a>(&self, surface: Element<'a, Message>) -> Element<'a, Message> {
        let scrim = self.motion.arrival_scrim();
        let offset = self.motion.arrival_offset();

        let staged: Element<'a, Message> = if scrim > 0.001 {
            let veil = container(Space::new())
                .width(Length::Fill)
                .height(Length::Fill)
                .style(move |_theme| container::Style {
                    background: Some(iced::Background::Color(motion::with_alpha(
                        ui::theme::ground(),
                        scrim,
                    ))),
                    ..container::Style::default()
                });
            iced::widget::stack![surface, veil]
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
        } else {
            surface
        };

        if offset.abs() < 0.01 {
            return staged;
        }

        iced::widget::Float::new(staged)
            .translate(move |_bounds, _viewport| iced::Vector::new(offset, 0.0))
            .style(|_theme| iced::widget::float::Style::default())
            .into()
    }

    fn gutter<'a>(&self, body: Element<'a, Message>) -> Element<'a, Message> {
        container(body)
            .width(Length::Fill)
            .padding(iced::Padding {
                top: 0.0,
                bottom: 0.0,
                left: crate::ui::theme::PAD,
                right: crate::ui::theme::PAD,
            })
            .into()
    }

    fn home_screen(&self) -> Element<'_, Message> {
        let suggested: Vec<&LibraryItem> = self
            .suggestions
            .iter()
            .map(|suggestion| &suggestion.item)
            .collect();
        let resume = self
            .home
            .first()
            .or_else(|| self.suggestions.first().map(|suggestion| &suggestion.item));

        ui::screens::home::Home {
            continue_watching: &self.home,
            suggested,
            resume,
            nav: &self.nav,
            empty: false,
            art: &self.artwork,
            waiting: &self.artwork_queued,
            motion: &self.motion,
            width: self.window_width,
        }
        .view()
    }

    fn library_screen(&self) -> Element<'_, Message> {
        ui::screens::library::Library {
            items: &self.browse.items,
            total: self.browse.total.max(0) as usize,
            page: self.browse.page as u32,
            pages: self.browse.page_count() as u32,
            query: &self.browse.text,
            filters: self.filter_chips(),
            sort: self.browse.sort,
            source_line: ui::screens::library::source_line(
                self.sources.len(),
                self.browse.total.max(0) as usize,
                self.last_scan_phrase().as_deref(),
            ),
            nav: &self.nav,
            genres: &self.browse.genres,
            genre: self.browse.genre.as_deref(),
            art: &self.artwork,
            motion: &self.motion,
            width: self.window_width,
        }
        .view()
    }

    fn filter_chips(&self) -> ui::screens::library::FilterChips {
        ui::screens::library::FilterChips {
            kind: self.browse.kind,
            watch: self.browse.watch,
            available_only: self.browse.available_only,
            watchlist_only: self.browse.watchlist_only,
            favorites_only: self.browse.favorites_only,
        }
    }

    fn last_scan_phrase(&self) -> Option<String> {
        let newest = self
            .sources
            .iter()
            .filter_map(|source| source.last_scan_ms)
            .max()?;
        let elapsed = (epoch_ms() - newest).max(0) / 1_000;
        Some(scan_recency_phrase(elapsed))
    }

    fn detail_screen(&self) -> Element<'_, Message> {
        let Some(detail) = &self.detail else {
            return self.empty_note("Nothing selected.", "Open something from the library.");
        };

        ui::screens::detail::Detail {
            item: &detail.item,
            episodes: detail.season_episodes(),
            seasons: &detail.seasons,
            season: detail.season,
            nav: &self.nav,
            facts: self.detail_facts(&detail.item),
            art: &self.artwork,
            width: self.window_width,
            extra: Some(self.detail_extra(&detail.item)),
        }
        .view()
    }

    fn detail_facts(&self, item: &LibraryItem) -> String {
        let mut lines: Vec<String> = vec![
            item.kind.as_str().to_string(),
            item.year.map(|year| year.to_string()).unwrap_or_default(),
            item.episode_label(),
            if item.duration_ms > 0 {
                format!(
                    "{} / {}",
                    clock(item.position_ms as f64 / 1_000.0),
                    clock(item.duration_ms as f64 / 1_000.0)
                )
            } else {
                "duration reported by the player on open".to_string()
            },
            if item.genres.is_empty() {
                String::new()
            } else {
                item.genres.join(", ")
            },
            if item.metadata_origin == "nfo" {
                "details from a local NFO file".to_string()
            } else {
                String::new()
            },
            item.relative_path.clone(),
        ]
        .into_iter()
        .filter(|line| !line.trim().is_empty())
        .collect();

        if !item.available {
            lines.push(
                "This file is not readable right now. Relink its source and rescan.".to_string(),
            );
        }

        lines.join("\n")
    }

    fn detail_extra(&self, item: &LibraryItem) -> Element<'_, Message> {
        let tokens = design::tokens();
        let secondary = |label: String, message: Option<Message>| -> Element<'_, Message> {
            button(text(label).size(tokens.type_scale.label))
                .on_press_maybe(message)
                .style(design::quiet_style(false))
                .padding([tokens.spacing.field_y, tokens.spacing.field_x])
                .into()
        };

        column![
            row![
                secondary(
                    "Start over".to_string(),
                    item.available.then(|| Message::Restart(item.id.clone())),
                ),
                secondary(
                    if item.favorite {
                        "Favourited".to_string()
                    } else {
                        "Favourite".to_string()
                    },
                    Some(Message::ToggleFavorite(item.work_id.clone())),
                ),
                secondary(
                    "Hide from Continue".to_string(),
                    Some(Message::HideFromContinue(item.edition_id.clone())),
                ),
            ]
            .spacing(tokens.spacing.field_x)
            .align_y(Alignment::Center),
            self.detail_conversion_row(item),
        ]
        .spacing(tokens.spacing.field_y)
        .width(Length::Fill)
        .into()
    }

    fn player_screen(&self) -> Element<'_, Message> {
        let title = self
            .active
            .as_ref()
            .map(|active| active.title.clone())
            .unwrap_or_else(|| "Player".to_string());
        let episode =
            self.active
                .as_ref()
                .and_then(|active| match (active.season, active.episode) {
                    (Some(season), Some(episode)) => Some(format!("S{season:02}E{episode:02}")),
                    _ => None,
                });
        let output = self.snapshot.output.label();

        let reveal = self.motion.chrome();
        let shown = self.player_chrome || reveal > 0.001;

        ui::screens::player::Player {
            snapshot: &self.snapshot,
            title,
            episode,
            output: Some(output.clone()),
            nav: &self.nav,
            width: self.window_width,
            position: self.scrub.unwrap_or(self.snapshot.position),
            volume: self.volume_percent(),
            chrome: shown,
            reveal,
            overlay: shown.then(|| self.player_overlay(reveal)),
            compact: self.window_height < 620.0,
        }
        .view()
    }

    fn player_overlay(&self, reveal: f32) -> Element<'_, Message> {
        let tokens = design::tokens();
        let ink = |color: iced::Color| motion::with_alpha(color, reveal);
        let action = move |_theme: &iced::Theme, status| {
            ui::theme::veiled(design::action_style(false), reveal)(_theme, status)
        };
        let quiet = move |_theme: &iced::Theme, status| {
            ui::theme::veiled(design::quiet_style(false), reveal)(_theme, status)
        };
        let mut block = column![].spacing(tokens.spacing.field_y);

        if let Some(up) = &self.up_next {
            let left = up
                .starts_at
                .saturating_duration_since(std::time::Instant::now())
                .as_secs_f32()
                .ceil();
            block = block.push(
                row![
                    text(format!("Up next: {} in {left:.0} s", up.title))
                        .size(tokens.type_scale.label)
                        .color(ink(tokens.palette.foreground)),
                    button(text("Play now").size(tokens.type_scale.label))
                        .on_press(Message::PlayNextNow)
                        .style(action)
                        .padding([tokens.spacing.field_y, tokens.spacing.field_x]),
                    button(text("Cancel").size(tokens.type_scale.label))
                        .on_press(Message::CancelNext)
                        .style(quiet)
                        .padding([tokens.spacing.field_y, tokens.spacing.field_x]),
                ]
                .spacing(tokens.spacing.field_x)
                .align_y(Alignment::Center),
            );
        }

        if let Some(marker) = self.active_marker() {
            let label = if marker.kind == "intro" {
                "Skip intro"
            } else {
                "Skip credits"
            };
            block = block.push(
                row![
                    button(text(label).size(tokens.type_scale.label))
                        .on_press(Message::SkipSegment)
                        .style(action)
                        .padding([tokens.spacing.field_y, tokens.spacing.field_x]),
                    text(if marker.kind == "intro" {
                        "A segment you marked starts here."
                    } else {
                        "A credits segment you marked starts here."
                    })
                    .size(tokens.type_scale.micro)
                    .color(ink(tokens.palette.ink_muted)),
                ]
                .spacing(tokens.spacing.field_x)
                .align_y(Alignment::Center),
            );
        }

        if self.active.is_some() {
            let mut panel = column![].spacing(tokens.spacing.field_y);
            panel = panel.push(
                row![
                    text("Subtitles")
                        .size(tokens.type_scale.label)
                        .color(ink(tokens.palette.foreground)),
                    Space::new().width(Length::Fill),
                    button(text("Find online").size(tokens.type_scale.micro))
                        .on_press(Message::FindSubtitles)
                        .style(action)
                        .padding([tokens.spacing.field_y, tokens.spacing.field_x]),
                    button(text("Import…").size(tokens.type_scale.micro))
                        .on_press(Message::ImportSubtitle)
                        .style(quiet)
                        .padding([tokens.spacing.field_y, tokens.spacing.field_x]),
                ]
                .spacing(tokens.spacing.field_x)
                .align_y(Alignment::Center),
            );
            let budget = self.subtitle_row_budget();
            if budget >= 2 && !self.subtitle_actions().is_empty() {
                panel = panel.push(
                    text("↑ ↓ pick a subtitle · Enter takes it")
                        .size(tokens.type_scale.micro)
                        .color(ink(tokens.palette.ink_muted)),
                );
            }
            if let Some(error) = &self.error
                && Some(error) != self.subtitle_note.as_ref()
            {
                panel = panel.push(
                    text(error.clone())
                        .size(tokens.type_scale.micro)
                        .color(ink(ui::theme::danger())),
                );
            }
            if let Some(message) = self.subtitle_note.clone() {
                let color = if self.subtitle_note_error {
                    ui::theme::danger()
                } else {
                    tokens.palette.ink_muted
                };
                panel = panel.push(
                    text(message)
                        .size(tokens.type_scale.micro)
                        .color(ink(color)),
                );
            }
            let (kept_rows, result_rows) = self.subtitle_row_split();
            let focus = self
                .subtitle_focus
                .min(self.subtitle_actions().len().saturating_sub(1));
            for (index, subtitle) in self.subtitles.iter().take(kept_rows).enumerate() {
                let marker = if index == focus { "› " } else { "  " };
                panel = panel.push(
                    row![
                        text(format!("{marker}{} · {}", subtitle.label, subtitle.format))
                            .size(tokens.type_scale.micro)
                            .color(ink(tokens.palette.foreground)),
                        Space::new().width(Length::Fill),
                        button(text("Use").size(tokens.type_scale.micro))
                            .on_press(Message::UseSubtitle(subtitle.id.clone()))
                            .style(quiet)
                            .padding([tokens.spacing.field_y, tokens.spacing.field_x]),
                        button(text("Remove").size(tokens.type_scale.micro))
                            .on_press(Message::RemoveSubtitle(subtitle.id.clone()))
                            .style(quiet)
                            .padding([tokens.spacing.field_y, tokens.spacing.field_x]),
                    ]
                    .spacing(tokens.spacing.field_x)
                    .align_y(Alignment::Center),
                );
            }
            if budget >= 2 && self.subtitles.len() > kept_rows {
                panel = panel.push(
                    text(format!(
                        "{} more kept subtitle(s) for this film",
                        self.subtitles.len() - kept_rows
                    ))
                    .size(tokens.type_scale.micro)
                    .color(ink(tokens.palette.ink_muted)),
                );
            }
            for (offset, result) in self.subtitle_results.iter().take(result_rows).enumerate() {
                let index = kept_rows + offset;
                let marker = if index == focus { "› " } else { "  " };
                let label = self.subtitle_result_label(result);
                panel = panel.push(
                    row![
                        text(format!("{marker}{label}"))
                            .size(tokens.type_scale.micro)
                            .color(ink(tokens.palette.foreground)),
                        Space::new().width(Length::Fill),
                        button(text("Download").size(tokens.type_scale.micro))
                            .on_press(Message::SubtitleDownload {
                                id: result.id,
                                format: result.format.clone(),
                            })
                            .style(action)
                            .padding([tokens.spacing.field_y, tokens.spacing.field_x]),
                    ]
                    .spacing(tokens.spacing.field_x)
                    .align_y(Alignment::Center),
                );
            }
            if budget >= 2 && self.subtitle_results.len() > result_rows {
                panel = panel.push(
                    text(format!(
                        "{} more match(es), best first",
                        self.subtitle_results.len() - result_rows
                    ))
                    .size(tokens.type_scale.micro)
                    .color(ink(tokens.palette.ink_muted)),
                );
            }
            block = block.push(panel.width(Length::Fill));
        }

        block = block.push(
            row![
                text(
                    self.active
                        .as_ref()
                        .filter(|active| active.resume_from_ms > 0 && !self.snapshot.ended)
                        .map(|active| format!(
                            "resumed from {}",
                            clock(active.resume_from_ms as f64 / 1_000.0)
                        ))
                        .unwrap_or_default()
                )
                .size(tokens.type_scale.micro)
                .color(ink(tokens.palette.ink_muted)),
                Space::new().width(Length::Fill),
            ]
            .spacing(tokens.spacing.field_x)
            .align_y(Alignment::Center),
        );

        container(block.width(Length::Fill))
            .width(Length::Fill)
            .padding(tokens.spacing.field_x)
            .style(move |_theme| container::Style {
                background: Some(iced::Background::Color(motion::with_alpha(
                    tokens.palette.surface2,
                    0.66 * reveal,
                ))),
                border: iced::Border {
                    radius: tokens.radius.overlay.into(),
                    ..iced::Border::default()
                },
                ..container::Style::default()
            })
            .into()
    }

    fn settings_screen(&self) -> Element<'_, Message> {
        ui::screens::settings::Settings {
            section: self.settings_section,
            rows: self.settings_rows(),
            nav: &self.nav,
            width: self.window_width,
            extra: Some(self.settings_detail()),
        }
        .view()
    }

    fn add_folder_row(&self) -> Element<'_, Message> {
        let tokens = design::tokens();
        let first_run = self.sources.is_empty();
        container(
            column![
                text(if first_run {
                    "Choose a folder of your own media"
                } else {
                    "Add another folder"
                })
                .size(tokens.type_scale.label),
                row![
                    button(text("Choose a folder…").size(tokens.type_scale.label))
                        .on_press(Message::ChooseFolder)
                        .style(design::action_style(true))
                        .padding([tokens.spacing.field_y, tokens.spacing.field_x]),
                    text_input("…or type an absolute path", &self.new_root)
                        .on_input(Message::NewRootChanged)
                        .on_submit(Message::AddFolder)
                        .style(design::input_style())
                        .padding(tokens.spacing.field_y)
                        .width(Length::Fill),
                    button(text("Add and scan").size(tokens.type_scale.label))
                        .on_press(Message::AddFolder)
                        .style(design::quiet_style(false))
                        .padding([tokens.spacing.field_y, tokens.spacing.field_x]),
                ]
                .spacing(tokens.spacing.field_x)
                .align_y(Alignment::Center),
                text(if first_run {
                    "Movies and episodes stay where they are. Screenroom reads them in place, \
                     keeps posters and positions in its own library, and needs no network."
                } else {
                    "Files are read in place and never moved, renamed or deleted."
                })
                .size(tokens.type_scale.micro)
                .color(tokens.palette.ink_muted),
                if self.artwork_failures > 0 {
                    text(format!(
                        "{} title(s) had no readable frame, so they keep a title-only card.",
                        self.artwork_failures
                    ))
                    .size(tokens.type_scale.micro)
                    .color(tokens.palette.destructive)
                } else {
                    text(String::new()).size(tokens.type_scale.micro)
                },
            ]
            .spacing(tokens.spacing.field_y),
        )
        .style(|_theme| design::container_style(Level::Panel, false))
        .padding(tokens.spacing.content_pad)
        .width(Length::Fill)
        .into()
    }

    fn empty_note(&self, title: &str, detail: &str) -> Element<'_, Message> {
        let tokens = design::tokens();
        container(
            column![
                text(title.to_string())
                    .size(tokens.type_scale.body_lg)
                    .color(tokens.palette.foreground),
                text(detail.to_string())
                    .size(tokens.type_scale.label)
                    .color(tokens.palette.ink_muted),
            ]
            .spacing(tokens.spacing.field_y),
        )
        .padding(tokens.spacing.content_pad)
        .width(Length::Fill)
        .into()
    }

    fn settings_rows(&self) -> Vec<ui::screens::settings::Row<'_>> {
        use ui::screens::settings::Row;

        let preferences = &self.preferences;
        let mut rows: Vec<Row<'_>> = Vec::new();

        match self.settings_section {
            ui::screens::settings::Section::Playback => {
                rows.push(Row::Toggle {
                    label: "Play the next episode automatically",
                    hint: Some("A gap in a season stops the chain."),
                    on: preferences.autoplay_next,
                    message: Message::ToggleAutoplay(!preferences.autoplay_next),
                });
                rows.push(Row::Toggle {
                    label: "Skip marked intros automatically",
                    hint: Some("Only a segment you marked is skipped."),
                    on: preferences.auto_skip_intro,
                    message: Message::ToggleAutoSkip(!preferences.auto_skip_intro),
                });
                rows.push(Row::Slider {
                    label: "Subtitle size",
                    hint: None,
                    value: f64::from(preferences.subtitle_scale_percent),
                    range: f64::from(Preferences::SUBTITLE_SCALE_MIN_PERCENT)
                        ..=f64::from(Preferences::SUBTITLE_SCALE_MAX_PERCENT),
                    message: Message::SubtitleScale,
                });
                rows.push(Row::Slider {
                    label: "Subtitle position",
                    hint: Some("Measured from the top of the picture, so 100% is the bottom."),
                    value: f64::from(preferences.subtitle_position_percent),
                    range: f64::from(Preferences::SUBTITLE_POSITION_MIN_PERCENT)
                        ..=f64::from(Preferences::SUBTITLE_POSITION_MAX_PERCENT),
                    message: Message::SubtitlePosition,
                });
            }
            ui::screens::settings::Section::Casting => {
                rows.push(Row::Value {
                    label: "Capability",
                    value: crate::casting::CAPABILITY_TEXT.to_string(),
                });
                rows.push(Row::Value {
                    label: "Cast session",
                    value: if self.cast_session.is_some() {
                        "A receiver is connected.".to_string()
                    } else {
                        "No receiver connected.".to_string()
                    },
                });
                rows.push(Row::Action {
                    label: "Look for receivers",
                    hint: Some("Only when you press this; nothing scans in the background."),
                    action: "Look now",
                    on_press: Some(Message::LookForReceivers),
                });
                for receiver in &self.receivers {
                    rows.push(Row::Action {
                        label: receiver.name.as_str(),
                        hint: Some(receiver.address.as_str()),
                        action: "Cast",
                        on_press: Some(Message::CastTo(receiver.id.clone())),
                    });
                }
                if let Some(note) = &self.cast_note {
                    rows.push(Row::Value {
                        label: "Last result",
                        value: note.clone(),
                    });
                }
                if self.cast_session.is_some() {
                    for (label, message) in [
                        ("Pause the cast", Message::CastPause),
                        ("Stop the cast", Message::CastStop),
                        ("Disconnect the cast", Message::CastDisconnect),
                    ] {
                        rows.push(Row::Action {
                            label,
                            hint: None,
                            action: "Run",
                            on_press: Some(message),
                        });
                    }
                }
            }
            ui::screens::settings::Section::Profiles => {
                for profile in &self.profiles {
                    let active = profile.id == self.profile_id;
                    if active {
                        rows.push(Row::Value {
                            label: profile.name.as_str(),
                            value: "Active profile".to_string(),
                        });
                    } else {
                        rows.push(Row::Action {
                            label: profile.name.as_str(),
                            hint: Some("Keeps its own progress, favourites and watchlist."),
                            action: "Use",
                            on_press: Some(Message::ProfileChosen(profile.id.clone())),
                        });
                    }
                }
                if self.profiles.len() > 1 {
                    for profile in &self.profiles {
                        let armed = self.armed_profile.as_deref() == Some(profile.id.as_str());
                        rows.push(Row::Action {
                            label: profile.name.as_str(),
                            hint: Some("Removing a profile cannot be undone."),
                            action: if armed { "Confirm remove" } else { "Remove" },
                            on_press: Some(Message::RemoveProfile(profile.id.clone())),
                        });
                    }
                }
            }
            ui::screens::settings::Section::Folders => {
                rows.push(Row::Action {
                    label: "Choose a folder of your own media",
                    hint: Some("Files are read in place and never moved or renamed."),
                    action: "Choose…",
                    on_press: Some(Message::ChooseFolder),
                });
                for source in &self.sources {
                    rows.push(Row::Value {
                        label: source.label.as_str(),
                        value: source.root_path.clone(),
                    });
                    rows.push(Row::Action {
                        label: source.label.as_str(),
                        hint: Some(source_state_label(source.state)),
                        action: "Rescan",
                        on_press: Some(Message::RescanSource(source.id.clone())),
                    });
                    let armed = self.armed_remove.as_deref() == Some(source.id.as_str());
                    rows.push(Row::Action {
                        label: source.label.as_str(),
                        hint: Some("Nothing on disk is changed."),
                        action: if armed {
                            "Press again to remove"
                        } else {
                            "Remove"
                        },
                        on_press: Some(Message::RemoveSource(source.id.clone())),
                    });
                }
            }
            ui::screens::settings::Section::Online => {
                rows.push(Row::Value {
                    label: "Metadata provider",
                    value: self.metadata_capability.detail.clone(),
                });
                rows.push(Row::Toggle {
                    label: "Keep provider artwork on this machine",
                    hint: Some("Only permitted image bytes are stored, never a remote URL."),
                    on: self.artwork_retention == ArtworkRetention::KeepLocally,
                    message: Message::ToggleArtworkRetention,
                });
                rows.push(Row::Action {
                    label: "Enrich the library now",
                    hint: Some("A manual match is never overwritten."),
                    action: "Enrich",
                    on_press: Some(Message::EnrichmentNow),
                });
            }
            ui::screens::settings::Section::Subtitles => {
                rows.push(Row::Value {
                    label: "Online provider",
                    value: self.subtitle_capability.detail.clone(),
                });
                rows.push(Row::Value {
                    label: "Where subtitles are found",
                    value: "In the player, while a film is playing".to_string(),
                });
            }
            ui::screens::settings::Section::Accessibility => {
                rows.push(Row::Toggle {
                    label: "Reduce motion: no browse previews",
                    hint: Some("A card keeps its poster and no preview decoder is opened."),
                    on: preferences.reduced_motion,
                    message: Message::ToggleReducedMotion(!preferences.reduced_motion),
                });
                rows.push(Row::Value {
                    label: "Subtitle size",
                    value: format!("{}%", preferences.subtitle_scale_percent),
                });
                rows.push(Row::Value {
                    label: "Subtitle position",
                    value: format!("{}%", preferences.subtitle_position_percent),
                });
            }
            ui::screens::settings::Section::Data => {
                rows.push(Row::Value {
                    label: "Backup file",
                    value: self.backup_path.clone(),
                });
                rows.push(Row::Action {
                    label: "Back up now",
                    hint: Some("Profiles, progress and lists, in one file."),
                    action: "Back up",
                    on_press: Some(Message::BackupNow),
                });
                if self.backup_job.is_some() {
                    rows.push(Row::Action {
                        label: "Cancel the backup that is running",
                        hint: None,
                        action: "Cancel",
                        on_press: Some(Message::CancelBackup),
                    });
                }
                rows.push(Row::Action {
                    label: "Converted copies folder",
                    hint: Some(
                        "A conversion writes a new file here and leaves the original alone.",
                    ),
                    action: "Choose…",
                    on_press: Some(Message::ChooseConversionFolder),
                });
                rows.push(Row::Value {
                    label: "Restore file",
                    value: self.restore_path.clone(),
                });
                rows.push(Row::Action {
                    label: "Check the restore file",
                    hint: Some("Nothing is replaced until you press again."),
                    action: "Check",
                    on_press: Some(Message::InspectRestore),
                });
                rows.push(Row::Action {
                    label: "Restore from that file",
                    hint: Some("The current library is copied to a .before-restore file first."),
                    action: if self.armed_restore.is_some() {
                        "Press again to restore"
                    } else {
                        "Restore"
                    },
                    on_press: Some(Message::RestoreNow),
                });
                if self.conversion_runner.is_some() {
                    rows.push(Row::Action {
                        label: "Stop the conversion",
                        hint: None,
                        action: "Stop",
                        on_press: Some(Message::CancelConversion),
                    });
                }
            }
            ui::screens::settings::Section::About => {
                rows.push(Row::Value {
                    label: "Playback",
                    value: if self.capability.available {
                        format!(
                            "libmpv {}",
                            self.capability.api_version.as_deref().unwrap_or("?")
                        )
                    } else {
                        format!("unavailable — {}", self.capability.reason)
                    },
                });
                rows.push(Row::Value {
                    label: "Picture",
                    value: self.snapshot.output.label(),
                });
                rows.push(Row::Value {
                    label: "Library",
                    value: self.library_label.clone(),
                });
                rows.push(Row::Value {
                    label: "Design tokens",
                    value: design::provenance().to_string(),
                });
            }
        }

        rows
    }

    fn settings_detail(&self) -> Element<'_, Message> {
        use ui::screens::settings::Section;
        let tokens = design::tokens();
        let note = |label: String| -> Element<'_, Message> {
            text(label)
                .size(tokens.type_scale.caption)
                .color(tokens.palette.ink_muted)
                .into()
        };

        match self.settings_section {
            Section::Playback => column![
                row![
                    text("Subtitle language").size(tokens.type_scale.label),
                    text_input("e.g. en", &self.subtitle_input)
                        .id(SettingsField::SubtitleLanguage.id())
                        .on_input(Message::SubtitleLanguageChanged)
                        .on_submit(Message::ApplySubtitleLanguage)
                        .style(design::input_style())
                        .padding(tokens.spacing.field_y)
                        .width(140.0),
                    button(text("Apply").size(tokens.type_scale.label))
                        .on_press(Message::ApplySubtitleLanguage)
                        .style(design::quiet_style(false))
                        .padding([tokens.spacing.field_y, tokens.spacing.field_x]),
                ]
                .spacing(tokens.spacing.field_x)
                .align_y(Alignment::Center),
                row![
                    text("Audio language").size(tokens.type_scale.label),
                    text_input("e.g. en", &self.audio_input)
                        .id(SettingsField::AudioLanguage.id())
                        .on_input(Message::AudioLanguageChanged)
                        .on_submit(Message::ApplyAudioLanguage)
                        .style(design::input_style())
                        .padding(tokens.spacing.field_y)
                        .width(140.0),
                    button(text("Apply").size(tokens.type_scale.label))
                        .on_press(Message::ApplyAudioLanguage)
                        .style(design::quiet_style(false))
                        .padding([tokens.spacing.field_y, tokens.spacing.field_x]),
                ]
                .spacing(tokens.spacing.field_x)
                .align_y(Alignment::Center),
                note(
                    "Press Enter to apply. Leave a field empty and press Enter to let the player \
                     choose."
                        .to_string(),
                ),
            ]
            .spacing(tokens.spacing.field_y)
            .width(Length::Fill)
            .into(),
            Section::Casting => column![
                note(crate::casting::CAPABILITY_TEXT.to_string()),
                note(
                    "Receivers are looked for on this network only when you press the button. A \
                     cast session starts only when you choose one and a film is playing: this \
                     machine then serves that one file from a temporary endpoint that closes \
                     itself, and sends AVTransport controls to the receiver."
                        .to_string(),
                ),
            ]
            .spacing(tokens.spacing.field_y)
            .width(Length::Fill)
            .into(),
            Section::Profiles => column![
                row![
                    text_input("New profile name", &self.new_profile)
                        .id(SettingsField::ProfileName.id())
                        .on_input(Message::NewProfileChanged)
                        .on_submit(Message::AddProfile)
                        .style(design::input_style())
                        .padding(tokens.spacing.field_y)
                        .width(220.0),
                    button(text("Add profile").size(tokens.type_scale.label))
                        .on_press(Message::AddProfile)
                        .style(design::action_style(false))
                        .padding([tokens.spacing.field_y, tokens.spacing.field_x]),
                ]
                .spacing(tokens.spacing.field_x)
                .align_y(Alignment::Center),
                note("Each profile keeps its own progress, favourites and watchlist.".to_string()),
            ]
            .spacing(tokens.spacing.field_y)
            .width(Length::Fill)
            .into(),
            Section::Folders => {
                let mut body = column![self.add_folder_row()]
                    .spacing(tokens.spacing.field_y)
                    .width(Length::Fill);

                if !self.sources.is_empty() {
                    let choices: Vec<SourceChoice> = self
                        .sources
                        .iter()
                        .map(|source| SourceChoice {
                            id: source.id.clone(),
                            label: source.label.clone(),
                        })
                        .collect();
                    let chosen = self
                        .relink_target
                        .as_ref()
                        .and_then(|id| choices.iter().find(|choice| &choice.id == id).cloned());
                    body = body
                        .push(text("Relink a folder").size(tokens.type_scale.label))
                        .push(
                            row![
                                pick_list(choices, chosen, |choice: SourceChoice| {
                                    Message::RelinkTargetChosen(choice.id)
                                })
                                .placeholder("Choose a folder"),
                                button(text("Choose…").size(tokens.type_scale.label))
                                    .on_press(Message::ChooseRelinkFolder)
                                    .style(design::quiet_style(false))
                                    .padding([tokens.spacing.field_y, tokens.spacing.field_x]),
                                text_input(
                                    "New location, for example /Volumes/Media",
                                    &self.relink_path
                                )
                                .id(SettingsField::FolderPath.id())
                                .on_input(Message::RelinkPathChanged)
                                .on_submit(Message::RelinkSource)
                                .style(design::input_style())
                                .padding(tokens.spacing.field_y)
                                .width(Length::Fill),
                                button(text("Relink and rescan").size(tokens.type_scale.label))
                                    .on_press(Message::RelinkSource)
                                    .style(design::action_style(false))
                                    .padding([tokens.spacing.field_y, tokens.spacing.field_x]),
                            ]
                            .spacing(tokens.spacing.field_x)
                            .align_y(Alignment::Center),
                        )
                        .push(note(
                            "Use this when a removable drive mounts somewhere new. Nothing on \
                             disk is changed."
                                .to_string(),
                        ));
                }

                body.into()
            }
            Section::Online => column![
                note(self.metadata_capability.detail.clone()),
                note(
                    "A manual match is never overwritten, and a value from a local NFO beats a \
                     provider unless you ask for the provider's version. Nothing is fetched while \
                     no key is configured."
                        .to_string(),
                ),
            ]
            .spacing(tokens.spacing.field_y)
            .width(Length::Fill)
            .into(),
            Section::Subtitles => column![
                note(self.subtitle_capability.detail.clone()),
                note(SUBTITLE_PRIORITY_NOTE.to_string()),
                note(
                    "These controls live in the player, because a film has to be playing for a \
                     subtitle to belong to it: while it plays, press t or move the pointer to \
                     show the player's subtitle row, then use Find online, Import…, Use or \
                     Remove there. Opening Settings stops the film, so nothing here can act on \
                     it."
                    .to_string(),
                ),
            ]
            .spacing(tokens.spacing.field_y)
            .width(Length::Fill)
            .into(),
            Section::Accessibility => column![note(
                "Reduce motion also stops browse previews, so no preview decoder is opened."
                    .to_string()
            )]
            .width(Length::Fill)
            .into(),
            Section::Data => column![
                row![
                    button(text("Choose…").size(tokens.type_scale.label))
                        .on_press(Message::ChooseBackupPath)
                        .style(design::quiet_style(false))
                        .padding([tokens.spacing.field_y, tokens.spacing.field_x]),
                    text_input("Backup file path", &self.backup_path)
                        .id(SettingsField::BackupPath.id())
                        .on_input(Message::BackupPathChanged)
                        .on_submit(Message::BackupNow)
                        .style(design::input_style())
                        .padding(tokens.spacing.field_y)
                        .width(Length::Fill),
                    button(text("Back up now").size(tokens.type_scale.label))
                        .on_press(Message::BackupNow)
                        .style(design::action_style(false))
                        .padding([tokens.spacing.field_y, tokens.spacing.field_x]),
                ]
                .spacing(tokens.spacing.field_x)
                .align_y(Alignment::Center),
                row![
                    button(text("Choose…").size(tokens.type_scale.label))
                        .on_press(Message::ChooseRestoreFile)
                        .style(design::quiet_style(false))
                        .padding([tokens.spacing.field_y, tokens.spacing.field_x]),
                    text_input("Backup file to restore from", &self.restore_path)
                        .id(SettingsField::RestorePath.id())
                        .on_input(Message::RestorePathChanged)
                        .on_submit(Message::InspectRestore)
                        .style(design::input_style())
                        .padding(tokens.spacing.field_y)
                        .width(Length::Fill),
                    button(text("Check file").size(tokens.type_scale.label))
                        .on_press(Message::InspectRestore)
                        .style(design::quiet_style(false))
                        .padding([tokens.spacing.field_y, tokens.spacing.field_x]),
                ]
                .spacing(tokens.spacing.field_x)
                .align_y(Alignment::Center),
                note(
                    "Restoring replaces profiles, progress and lists with the backup's. Your \
                     current library is copied to a .before-restore file first."
                        .to_string(),
                ),
                note("Saved sources".to_string()),
                row![
                    text_input("Name, for example an ISP portal", &self.bookmark_label)
                        .id(SettingsField::BookmarkLabel.id())
                        .on_input(Message::BookmarkLabelChanged)
                        .style(design::input_style())
                        .padding(tokens.spacing.field_y)
                        .width(Length::Fill),
                    text_input("https://…", &self.bookmark_address)
                        .id(SettingsField::BookmarkAddress.id())
                        .on_input(Message::BookmarkAddressChanged)
                        .on_submit(Message::AddBookmark)
                        .style(design::input_style())
                        .padding(tokens.spacing.field_y)
                        .width(Length::Fill),
                    text_input(
                        "Access note, optional: account, ISP, plan",
                        &self.bookmark_access
                    )
                    .id(SettingsField::BookmarkAccess.id())
                    .on_input(Message::BookmarkAccessChanged)
                    .style(design::input_style())
                    .padding(tokens.spacing.field_y)
                    .width(Length::Fill),
                    button(text("Save source").size(tokens.type_scale.label))
                        .on_press(Message::AddBookmark)
                        .style(design::action_style(false))
                        .padding([tokens.spacing.field_y, tokens.spacing.field_x]),
                ]
                .spacing(tokens.spacing.field_x)
                .align_y(Alignment::Center),
                self.bookmark_rows(),
                self.conversion_panel(),
                note(
                    "Folders are read in place. Nothing here needs a network, an account or a \
                     server."
                        .to_string(),
                ),
            ]
            .spacing(tokens.spacing.field_y)
            .width(Length::Fill)
            .into(),
            Section::About => column![
                note(format!("Design tokens: {}", design::provenance())),
                note(
                    "Folders are read in place. Nothing here needs a network, an account or a \
                     server."
                        .to_string(),
                ),
            ]
            .spacing(tokens.spacing.field_y)
            .width(Length::Fill)
            .into(),
        }
    }

    fn bookmark_rows(&self) -> Element<'_, Message> {
        let tokens = design::tokens();
        let mut rows = column![].spacing(tokens.spacing.field_y);
        if self.bookmarks.is_empty() {
            rows = rows.push(
                text("No source saved yet.")
                    .size(tokens.type_scale.caption)
                    .color(tokens.palette.ink_muted),
            );
        }
        for bookmark in &self.bookmarks {
            rows = rows.push(
                row![
                    text(bookmark.label.clone()).size(tokens.type_scale.body),
                    text(format!("{} · {}", bookmark.protocol, bookmark.address))
                        .size(tokens.type_scale.micro)
                        .color(tokens.palette.ink_muted),
                    Space::new().width(Length::Fill),
                    button(text("Open").size(tokens.type_scale.label))
                        .on_press(Message::OpenBookmark(bookmark.id.clone()))
                        .style(design::action_style(false))
                        .padding([tokens.spacing.field_y, tokens.spacing.field_x]),
                    button(text("Remove").size(tokens.type_scale.label))
                        .on_press(Message::RemoveBookmark(bookmark.id.clone()))
                        .style(design::danger_style(false))
                        .padding([tokens.spacing.field_y, tokens.spacing.field_x]),
                ]
                .spacing(tokens.spacing.field_x)
                .align_y(Alignment::Center),
            );
        }
        rows.into()
    }

    fn step_settings_section(&mut self, forward: bool) {
        use ui::screens::settings::Section;
        let all = Section::ALL;
        let current = all
            .iter()
            .position(|section| *section == self.settings_section)
            .unwrap_or(0);
        let next = if forward {
            (current + 1) % all.len()
        } else {
            (current + all.len() - 1) % all.len()
        };
        self.settings_section = all[next];
        self.settings_focus = 0;
    }

    fn move_settings_focus(&mut self, down: bool) {
        let len = self.settings_activations().len();
        if len == 0 {
            self.settings_focus = 0;
            return;
        }
        let current = self.settings_focus.min(len - 1);
        self.settings_focus = if down {
            (current + 1).min(len - 1)
        } else {
            current.saturating_sub(1)
        };
    }

    fn run_settings_action(&mut self) -> Task<Message> {
        let actions = self.settings_activations();
        let Some(Some(message)) = actions.get(self.settings_focus).cloned() else {
            return Task::none();
        };
        self.update(message)
    }

    #[cfg(test)]
    fn settings_labels(&self) -> Vec<String> {
        use ui::screens::settings::Row;
        self.settings_rows()
            .into_iter()
            .map(|row| match row {
                Row::Toggle { label, .. }
                | Row::Value { label, .. }
                | Row::Slider { label, .. }
                | Row::Action { label, .. } => label.to_string(),
            })
            .collect()
    }

    fn settings_activations(&self) -> Vec<Option<Message>> {
        use ui::screens::settings::Row;
        self.settings_rows()
            .into_iter()
            .map(|row| match row {
                Row::Toggle { message, .. } => Some(message),
                Row::Action { on_press, .. } => on_press,
                Row::Value { .. } | Row::Slider { .. } => None,
            })
            .collect()
    }
}

fn settings_note(label: &str) -> Element<'static, Message> {
    text(label.to_string())
        .size(design::tokens().type_scale.micro)
        .color(design::tokens().palette.ink_muted)
        .into()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceChoice {
    pub id: String,

    pub label: String,
}

impl std::fmt::Display for SourceChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}

fn source_state_label(state: SourceState) -> &'static str {
    match state {
        SourceState::Available => "Available",
        SourceState::Disconnected => {
            "Drive not attached — reconnect it, then Rescan. Your history is kept."
        }
        SourceState::PermissionLost => "Access was withdrawn — grant access to the folder again.",
        SourceState::Missing => "Folder is gone — relink it to its new location.",
        SourceState::Removed => "Removed from the library. Files on disk are untouched.",
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayAction {
    Pause,

    Resume,

    Replay,
}

impl PlayAction {
    pub fn label(self) -> &'static str {
        match self {
            Self::Pause => "Pause",
            Self::Resume => "Play",
            Self::Replay => "Replay",
        }
    }
}

pub fn play_action(state: screenroom_player::PlayerState) -> PlayAction {
    use screenroom_player::PlayerState;
    match state {
        PlayerState::Paused => PlayAction::Resume,
        PlayerState::Ended => PlayAction::Replay,
        _ => PlayAction::Pause,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Flag {
    Favorite,

    Watchlist,
}

pub fn browse_empty_copy(has_sources: bool, narrowed: bool) -> (&'static str, &'static str) {
    match (has_sources, narrowed) {
        (false, _) => (
            "No media yet.",
            "Add a folder on Home and Screenroom reads it where it is — \
             nothing is moved, renamed or copied.",
        ),
        (true, true) => (
            "Nothing matches those filters.",
            "Clear the search or turn the filters off; the folder itself is fine.",
        ),
        (true, false) => (
            "Nothing imported from your folders yet.",
            "Rescan a folder in Settings. If it looks empty, check that the drive \
             is connected.",
        ),
    }
}

#[cfg(test)]
impl App {
    fn columns(&self) -> usize {
        grid_columns(self.window_width)
    }
}

pub fn grid_columns(window_width: f32) -> usize {
    ui::theme::Breakpoint::at(window_width).columns()
}

pub fn round_percent(value: f64, min: u16, max: u16) -> u16 {
    if !value.is_finite() {
        return min;
    }
    (value.round().max(0.0) as u16).clamp(min, max)
}

#[cfg(test)]
fn truncate(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        return value.to_string();
    }
    let mut out: String = value.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or_default()
}

pub fn clock(seconds: f64) -> String {
    if !seconds.is_finite() || seconds < 0.0 {
        return "0:00".to_string();
    }
    let total = seconds.round() as u64;
    let (hours, minutes, seconds) = (total / 3600, (total % 3600) / 60, total % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

#[cfg(test)]
mod acceptance;

#[cfg(test)]
mod tests {
    use super::*;
    use screenroom_core::{Checkpoint, Ingest, parse_filename};

    #[test]
    fn clock_formats_short_and_long_media() {
        assert_eq!(clock(0.0), "0:00");
        assert_eq!(clock(9.4), "0:09");
        assert_eq!(clock(92.0), "1:32");
        assert_eq!(clock(3_661.0), "1:01:01");
        assert_eq!(clock(-4.0), "0:00");
        assert_eq!(clock(f64::NAN), "0:00");
    }

    #[test]
    fn a_single_unit_never_reads_as_a_plural() {
        assert_eq!(scan_recency_phrase(0), "just now");
        assert_eq!(scan_recency_phrase(90), "just now");
        assert_eq!(scan_recency_phrase(91), "1 minute ago");
        assert_eq!(scan_recency_phrase(120), "2 minutes ago");
        assert_eq!(scan_recency_phrase(3_599), "59 minutes ago");
        assert_eq!(scan_recency_phrase(3_600), "1 hour ago");
        assert_eq!(scan_recency_phrase(7_200), "2 hours ago");
        assert_eq!(scan_recency_phrase(86_399), "23 hours ago");
        assert_eq!(scan_recency_phrase(86_400), "1 day ago");
        assert_eq!(scan_recency_phrase(172_800), "2 days ago");
        assert_eq!(scan_recency_phrase(-5), "just now");
    }

    #[test]
    fn every_unit_phrases_both_of_its_counts() {
        let units = [
            (91_i64, 120_i64, "minute"),
            (3_600, 7_200, "hour"),
            (86_400, 172_800, "day"),
        ];

        for (one, two, unit) in units {
            assert_eq!(scan_recency_phrase(one), format!("1 {unit} ago"));
            assert_eq!(scan_recency_phrase(two), format!("2 {unit}s ago"));
        }

        for seconds in 0..200_000 {
            let phrase = scan_recency_phrase(seconds);
            for unit in ["minute", "hour", "day"] {
                assert!(
                    !phrase.starts_with(&format!("1 {unit}s")),
                    "{phrase} at {seconds}s"
                );
            }
        }
    }

    #[test]
    fn the_grid_shows_more_cards_as_the_window_widens() {
        let laptop = grid_columns(1280.0);
        let ultrawide = grid_columns(3440.0);
        assert!(
            laptop >= DEFAULT_COLUMNS,
            "a 1280-wide window must not lose columns"
        );
        assert!(
            ultrawide > laptop,
            "an ultrawide must show more cards, not wider ones: {ultrawide} vs {laptop}"
        );

        let usable = 3440.0 - 2.0 * design::tokens().spacing.shell_pad;
        let card = usable / ultrawide as f32;
        assert!(
            card < 600.0,
            "a card on the ultrawide is {card:.0}px wide, which is a billboard"
        );
    }

    #[test]
    fn the_window_has_a_minimum_it_can_actually_draw() {
        let minimum = minimum_window_size();
        assert!(
            minimum.width >= 600.0,
            "a window narrower than {}px clips the navigation and the library header",
            minimum.width
        );
        assert!(
            minimum.height >= 480.0,
            "a window this short hides the grid"
        );
        assert_eq!(
            ui::theme::Breakpoint::at(minimum.width),
            ui::theme::Breakpoint::Narrow,
            "the minimum width has to be a width the narrow layout was drawn for"
        );
        assert_eq!(
            ui::screens::library::header_layout(minimum.width),
            ui::screens::library::HeaderLayout::Stacked,
            "the header must stack at the narrowest window it allows"
        );
    }

    #[test]
    fn a_requested_window_size_is_raised_to_the_minimum_the_layout_supports() {
        assert_eq!(
            window_size_from(Some("320"), Some("240")),
            minimum_window_size(),
            "a request below the minimum opens at the minimum, not at the default"
        );
        assert_eq!(
            window_size_from(Some("1280"), Some("800")),
            iced::Size::new(1280.0, 800.0)
        );
        assert_eq!(
            window_size_from(None, Some(" ")),
            iced::Size::new(INITIAL_WINDOW_WIDTH, 800.0)
        );
    }

    #[test]
    fn the_column_count_is_monotonic_and_always_usable() {
        let mut previous = 0;
        for width in [
            320.0_f32, 800.0, 1024.0, 1280.0, 1920.0, 2560.0, 3440.0, 5120.0,
        ] {
            let columns = grid_columns(width);
            assert!(
                (2..=8).contains(&columns),
                "{width}px produced {columns} columns, which is outside the range"
            );
            assert!(
                columns >= previous,
                "widening the window reduced the column count: {width}px gave {columns}"
            );
            previous = columns;
        }

        for width in [0.0_f32, -100.0, 10.0, f32::NAN, f32::INFINITY] {
            let columns = grid_columns(width);
            assert!(columns >= 2, "{width} produced {columns} columns");
        }
    }

    #[test]
    fn keyboard_rows_move_by_the_columns_the_grid_was_drawn_with() {
        let (mut app, _directory) = app_with(24);

        for width in [900.0_f32, 1100.0, 1280.0, 1440.0, 1920.0, 3440.0] {
            app.window_width = width;
            let drawn =
                ui::theme::Breakpoint::at(ui::screens::library::capped_window(width)).columns();

            app.focus_card(0);
            let _ = app.move_focus(Focus::Down);
            assert_eq!(
                app.focus, drawn,
                "at {width}px the library draws {drawn} columns, so Down must move {drawn}"
            );
            let _ = app.move_focus(Focus::Up);
            assert_eq!(
                app.focus, 0,
                "Up must return to the same column at {width}px"
            );

            let _ = app.move_focus(Focus::Right);
            assert_eq!(app.focus, 1);
        }
    }

    #[test]
    fn only_real_pointer_events_move_the_presence_latch() {
        use iced::mouse;

        assert!(matches!(
            pointer_event(&iced::Event::Mouse(mouse::Event::CursorEntered)),
            Some(Message::PointerInside(true))
        ));
        assert!(matches!(
            pointer_event(&iced::Event::Mouse(mouse::Event::CursorLeft)),
            Some(Message::PointerInside(false))
        ));
        assert!(matches!(
            pointer_event(&iced::Event::Mouse(mouse::Event::CursorMoved {
                position: iced::Point::ORIGIN
            })),
            Some(Message::PointerMoved { x: 0.0, y: 0.0 })
        ));
        assert!(matches!(
            pointer_event(&iced::Event::Mouse(mouse::Event::ButtonPressed(
                mouse::Button::Left
            ))),
            Some(Message::PointerButton {
                button: mouse::Button::Left,
                pressed: true
            })
        ));
        assert!(matches!(
            pointer_event(&iced::Event::Mouse(mouse::Event::ButtonReleased(
                mouse::Button::Left
            ))),
            Some(Message::PointerButton {
                button: mouse::Button::Left,
                pressed: false
            })
        ));
        assert!(
            pointer_event(&iced::Event::Mouse(mouse::Event::WheelScrolled {
                delta: mouse::ScrollDelta::Lines { x: 0.0, y: 1.0 }
            }))
            .is_none(),
            "a wheel scroll is not a boundary and must not touch presence"
        );
    }

    #[test]
    fn a_resize_event_that_has_no_usable_width_is_ignored() {
        let (mut app, _directory) = app_with(4);
        app.window_width = 1920.0;
        for width in [0.0_f32, -1.0, f32::NAN, f32::INFINITY] {
            let _ = app.update(Message::Resized(iced::Size::new(width, 800.0)));
            assert_eq!(
                app.window_width, 1920.0,
                "a width of {width} must not replace a usable one"
            );
        }
        let _ = app.update(Message::Resized(iced::Size::new(3440.0, 1440.0)));
        assert_eq!(app.window_width, 3440.0);
    }

    #[test]
    fn an_empty_browse_screen_names_the_situation_it_is_actually_in() {
        let first_run = browse_empty_copy(false, false);
        assert!(first_run.0.contains("No media"));
        assert!(first_run.1.contains("Add a folder"));
        assert!(!first_run.1.contains("filter"));

        assert_eq!(browse_empty_copy(false, true), first_run);

        let unread = browse_empty_copy(true, false);
        assert!(unread.1.contains("Rescan"));
        assert!(!unread.1.contains("Add a folder"));

        let filtered = browse_empty_copy(true, true);
        assert!(filtered.0.contains("filters"));
        assert!(filtered.1.contains("Clear the search"));

        for (title, detail) in [first_run, unread, filtered] {
            assert!(!title.is_empty() && !detail.is_empty());
            assert!(detail.ends_with('.'), "copy should be a sentence: {detail}");
        }
    }

    #[test]
    fn sort_order_alone_does_not_count_as_narrowing_the_library() {
        let mut browse = Browse::new();
        browse.sort = SortChoice::Progress;
        assert!(!browse.narrowed());

        for narrow in [
            |b: &mut Browse| b.text = "x".into(),
            |b: &mut Browse| b.kind = KindFilter::Movies,
            |b: &mut Browse| b.genre = Some("Drama".into()),
            |b: &mut Browse| b.watchlist_only = true,
            |b: &mut Browse| b.favorites_only = true,
            |b: &mut Browse| b.available_only = true,
        ] {
            let mut browse = Browse::new();
            narrow(&mut browse);
            assert!(browse.narrowed(), "a narrowing filter went unnoticed");
        }

        let mut browse = Browse::new();
        browse.text = "   ".into();
        assert!(!browse.narrowed());
    }

    #[test]
    fn a_slider_position_becomes_a_whole_percent_inside_the_offered_range() {
        let (min, max) = (
            Preferences::SUBTITLE_SCALE_MIN_PERCENT,
            Preferences::SUBTITLE_SCALE_MAX_PERCENT,
        );
        assert_eq!(round_percent(125.4, min, max), 125);
        assert_eq!(round_percent(125.6, min, max), 126);

        assert_eq!(round_percent(-8.0, min, max), min);
        assert_eq!(round_percent(9_999.0, min, max), max);

        assert_eq!(round_percent(f64::NAN, min, max), min);
        assert_eq!(round_percent(f64::INFINITY, min, max), min);
    }

    #[test]
    fn the_offered_subtitle_range_is_one_the_core_accepts() {
        let (scale_low, scale_high) = screenroom_player::SUB_SCALE_RANGE;
        assert!(scale_low <= f64::from(Preferences::SUBTITLE_SCALE_MIN_PERCENT) / 100.0);
        assert!(scale_high >= f64::from(Preferences::SUBTITLE_SCALE_MAX_PERCENT) / 100.0);
        let (position_low, position_high) = screenroom_player::SUB_POSITION_RANGE;
        assert!(position_low <= f64::from(Preferences::SUBTITLE_POSITION_MIN_PERCENT));
        assert!(position_high >= f64::from(Preferences::SUBTITLE_POSITION_MAX_PERCENT));
    }

    #[test]
    fn fullscreen_is_a_window_mode_the_player_can_leave_with_escape() {
        use iced::keyboard::key::Named;
        use iced::keyboard::{Event, Key, Modifiers};

        let (mut app, _directory) = app_with(2);
        let press = |key: Key| Event::KeyPressed {
            key,
            modified_key: Key::Unidentified,
            physical_key: iced::keyboard::key::Physical::Code(iced::keyboard::key::Code::Escape),
            location: iced::keyboard::Location::Standard,
            modifiers: Modifiers::empty(),
            text: None,
            repeat: false,
        };

        assert!(!app.fullscreen);
        let _ = app.update(Message::ToggleFullscreen);
        assert!(
            app.fullscreen,
            "the flag follows the mode that was asked for"
        );

        app.navigate(Screen::Player);
        let _ = app.handle_key(press(Key::Named(Named::Escape)));
        assert!(
            !app.fullscreen,
            "Escape leaves fullscreen before it leaves the film"
        );
        assert_eq!(app.screen, Screen::Player, "the film is still open");

        let _ = app.handle_key(press(Key::Named(Named::Escape)));
        assert_ne!(
            app.screen,
            Screen::Player,
            "a second Escape, now that the film is out of fullscreen, leaves it"
        );
    }

    #[test]
    fn one_escape_cancels_a_fullscreen_entry_that_has_not_landed_yet() {
        use iced::keyboard::key::Named;
        use iced::keyboard::{Event, Key, Modifiers};

        let (mut app, _directory) = app_with(2);
        app.navigate(Screen::Player);
        let press = |key: Key| Event::KeyPressed {
            key,
            modified_key: Key::Unidentified,
            physical_key: iced::keyboard::key::Physical::Code(iced::keyboard::key::Code::Escape),
            location: iced::keyboard::Location::Standard,
            modifiers: Modifiers::empty(),
            text: None,
            repeat: false,
        };

        let _ = app.update(Message::ToggleFullscreen);
        assert!(app.fullscreen);
        assert_eq!(app.pending_mode(), Some(iced::window::Mode::Fullscreen));

        let _ = app.handle_key(press(Key::Named(Named::Escape)));
        assert!(
            !app.fullscreen,
            "one Escape cancels the entry instead of dismissing the film"
        );
        assert_eq!(app.screen, Screen::Player, "and the film is untouched");
        assert_eq!(app.pending_mode(), Some(iced::window::Mode::Windowed));
    }

    #[test]
    fn a_second_fullscreen_request_while_one_is_in_flight_is_ignored() {
        let (mut app, _directory) = app_with(2);
        app.navigate(Screen::Player);

        let _ = app.update(Message::ToggleFullscreen);
        assert!(
            app.fullscreen,
            "the flag follows the mode that was asked for"
        );
        assert_eq!(app.pending_mode(), Some(iced::window::Mode::Fullscreen));

        let _ = app.update(Message::ToggleFullscreen);
        assert!(
            app.fullscreen,
            "a second f must not flip back before the window has answered the first"
        );

        let _ = app.update(Message::ModeChanged(iced::window::Mode::Fullscreen));
        assert_eq!(
            app.pending_mode(),
            None,
            "the window settling clears the in-flight request"
        );

        let _ = app.update(Message::ToggleFullscreen);
        assert!(!app.fullscreen, "after the window settles, f toggles again");
    }

    #[test]
    fn a_late_fullscreen_report_does_not_dismiss_a_film_that_was_being_left() {
        use iced::keyboard::key::Named;
        use iced::keyboard::{Event, Key, Modifiers};

        let (mut app, _directory) = app_with(2);
        app.navigate(Screen::Player);
        let press = |key: Key| Event::KeyPressed {
            key,
            modified_key: Key::Unidentified,
            physical_key: iced::keyboard::key::Physical::Code(iced::keyboard::key::Code::Escape),
            location: iced::keyboard::Location::Standard,
            modifiers: Modifiers::empty(),
            text: None,
            repeat: false,
        };

        let _ = app.update(Message::ToggleFullscreen);
        let _ = app.handle_key(press(Key::Named(Named::Escape)));

        let _ = app.update(Message::ModeChanged(iced::window::Mode::Fullscreen));
        assert_eq!(
            app.screen,
            Screen::Player,
            "a fullscreen report that lands after the exit began must not dismiss the film"
        );

        let _ = app.update(Message::ModeChanged(iced::window::Mode::Windowed));
        assert!(!app.fullscreen);
        assert_eq!(app.pending_mode(), None);
        assert_eq!(app.screen, Screen::Player, "and the film is still open");
    }

    #[test]
    fn closing_the_window_records_that_the_terminate_path_ran() {
        let (mut app, directory) = app_with(2);
        let report = directory.path().join("terminate.txt");
        app.terminate_check = Some(report.clone());
        app.terminate_routing = crate::quit::TerminateRouting::Routed;
        app.navigate(Screen::Player);

        let _ = app.update(Message::Close);

        let text = std::fs::read_to_string(&report).expect("the close path wrote its receipt");
        assert!(
            text.starts_with("closed routed"),
            "the receipt names the routing that was installed: {text:?}"
        );
    }

    #[test]
    fn the_volume_control_shows_the_player_then_the_stored_level() {
        let (mut app, _directory) = app_with(2);
        app.preferences.volume_percent = 40;
        assert_eq!(
            app.volume_percent(),
            40.0,
            "a player that has not reported a volume must still start at the stored one"
        );

        app.snapshot.volume = Some(62.0);
        assert_eq!(
            app.volume_percent(),
            62.0,
            "the control must follow the player rather than the request"
        );

        app.drag_volume(77.0);
        assert_eq!(
            app.volume_percent(),
            77.0,
            "the drag wins while it is in flight"
        );

        app.snapshot.volume = Some(62.0);
        assert_eq!(app.volume_percent(), 77.0);
    }

    #[test]
    fn a_volume_drag_is_stored_once_when_it_ends() {
        let (mut app, _directory) = app_with(2);
        app.drag_volume(55.6);
        assert_eq!(
            app.library.preferences().unwrap().volume_percent,
            Preferences::default().volume_percent,
            "a drag must not write the database on every step"
        );

        app.commit_volume();
        assert_eq!(app.library.preferences().unwrap().volume_percent, 56);
        assert_eq!(app.volume_drag, None, "the drag is over");
        assert_eq!(app.volume_percent(), 56.0);

        app.drag_volume(-30.0);
        app.commit_volume();
        assert_eq!(app.library.preferences().unwrap().volume_percent, 0);
        app.drag_volume(f64::NAN);
        assert_eq!(app.volume_percent(), 0.0, "a non-finite drag is ignored");
    }

    #[test]
    fn truncation_keeps_cards_a_fixed_height() {
        assert_eq!(truncate("short", 10), "short");
        assert_eq!(truncate("exactly-ten", 11), "exactly-ten");
        let long = truncate("a-very-long-title-that-would-reflow-a-card", 12);
        assert_eq!(long.chars().count(), 12);
        assert!(long.ends_with('…'));
    }

    #[test]
    fn a_finished_file_offers_a_replay_instead_of_a_dead_pause() {
        use screenroom_player::PlayerState;
        assert_eq!(play_action(PlayerState::Ended), PlayAction::Replay);
        assert_eq!(play_action(PlayerState::Paused), PlayAction::Resume);
        assert_eq!(play_action(PlayerState::Playing), PlayAction::Pause);

        for state in [
            PlayerState::Idle,
            PlayerState::Resolving,
            PlayerState::Preparing,
            PlayerState::Playing,
            PlayerState::Paused,
            PlayerState::Ended,
            PlayerState::Failed,
        ] {
            assert!(!play_action(state).label().is_empty());
        }
    }

    #[test]
    fn the_page_size_is_a_whole_number_of_reference_rows() {
        assert_eq!(PAGE_SIZE, (DEFAULT_COLUMNS * PAGE_ROWS) as i64);
        assert_eq!(PAGE_SIZE % DEFAULT_COLUMNS as i64, 0);
    }

    #[test]
    fn kind_filters_map_onto_the_stored_kinds() {
        assert_eq!(KindFilter::All.kind(), None);
        assert_eq!(KindFilter::Movies.kind().as_deref(), Some("movie"));
        assert_eq!(KindFilter::Series.kind().as_deref(), Some("series"));
        for filter in [KindFilter::All, KindFilter::Movies, KindFilter::Series] {
            assert!(!filter.label().is_empty());
        }
    }

    #[test]
    fn every_sort_choice_is_offered_and_labelled() {
        assert_eq!(SortChoice::ALL.len(), 4);
        for choice in SortChoice::ALL {
            assert!(!choice.label().is_empty());
        }
        assert_eq!(SortChoice::Added.sort(), Sort::Added);
    }

    fn library_with(count: usize) -> (Library, tempfile::TempDir) {
        let directory = tempfile::TempDir::new().expect("temp dir");
        let library = Library::open_in_memory().expect("library");
        let source = library.add_source(directory.path(), None).expect("source");
        for index in 0..count {
            let name = format!("Title.{index:03}.2020.mkv");
            std::fs::write(directory.path().join(&name), b"not really a video")
                .expect("write placeholder");
            let parsed = parse_filename(&name, None);
            library
                .ingest_file(Ingest {
                    source_id: &source.id,
                    relative_path: &name,
                    fingerprint: &format!("fingerprint-{index}"),
                    size: 18,
                    modified_ms: 1,
                    scan_id: "test",
                    parsed: Some(&parsed),
                    expected_root: Some(&source.root_path),
                })
                .expect("ingest");
        }
        (library, directory)
    }

    fn app_with(count: usize) -> (App, tempfile::TempDir) {
        let (library, directory) = library_with(count);
        let mut app = App::with_library(library, None, "test".into());
        app.navigate(Screen::Browse);
        app.reload_browse();
        (app, directory)
    }

    fn settle_jobs(app: &mut App) {
        for _ in 0..600 {
            app.drain_backup();
            if app.backup_job.is_none() && app.restore_job.is_none() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("a backup or a restore did not finish within six seconds");
    }

    #[test]
    fn a_retained_poster_is_cached_for_its_card() {
        let (library, _directory) = library_with(2);
        let item = library.items(&LibraryQuery::page(1)).expect("items")[0].clone();
        library
            .store_artwork(
                &item.work_id,
                None,
                &[0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10],
                400,
                600,
                1_000,
            )
            .expect("artwork");

        let mut app = App::with_library(library, None, "test".into());
        app.navigate(Screen::Browse);
        app.reload_browse();
        assert!(
            app.artwork.contains_key(&item.work_id),
            "a stored poster must be handed to the card"
        );
    }

    #[test]
    fn a_test_built_app_never_starts_an_extraction() {
        let (mut app, _directory) = app_with(4);
        app.navigate(Screen::Browse);
        app.reload_browse();
        assert!(app.extractor.is_none());
        assert!(app.artwork_queued.is_empty());
    }

    fn poster_candidate(work_id: &str, title: &str) -> ArtworkCandidate {
        ArtworkCandidate {
            work_id: work_id.to_string(),
            file_id: format!("file-{work_id}"),
            title: title.to_string(),
            duration_ms: 600_000,
            path: std::path::PathBuf::from(format!("/media/{work_id}.mkv")),
        }
    }

    #[test]
    fn the_rail_on_screen_is_drawn_before_the_shelves_behind_it() {
        let candidates = vec![
            poster_candidate("behind-1", "Aardvark"),
            poster_candidate("third", "Third on screen"),
            poster_candidate("behind-2", "Zebra"),
            poster_candidate("first", "First on screen"),
            poster_candidate("second", "Second on screen"),
        ];
        let on_screen = vec![
            "first".to_string(),
            "second".to_string(),
            "third".to_string(),
        ];

        let ordered = screen_first(candidates, &on_screen);
        let order: Vec<&str> = ordered
            .iter()
            .map(|candidate| candidate.work_id.as_str())
            .collect();
        assert_eq!(order, ["first", "second", "third", "behind-1", "behind-2"]);
    }

    #[test]
    fn a_screen_with_nothing_of_its_own_keeps_the_queue_order_it_had() {
        let candidates = vec![poster_candidate("b", "B"), poster_candidate("a", "A")];
        let ordered = screen_first(candidates, &[]);
        let order: Vec<&str> = ordered
            .iter()
            .map(|candidate| candidate.work_id.as_str())
            .collect();
        assert_eq!(order, ["b", "a"]);
    }

    #[test]
    fn a_screen_asks_for_its_own_cards_before_what_sits_behind_it() {
        let (mut app, _directory) = app_with(6);
        app.navigate(Screen::Browse);
        app.reload_browse();
        app.reload_home();

        let on_screen = app.on_screen_work_ids();
        assert_eq!(
            on_screen.len(),
            app.browse.items.len(),
            "a library page must name exactly the cards it shows"
        );
        for item in &app.browse.items {
            assert!(
                on_screen.contains(&item.work_id),
                "a card on the library page was left out of the queue order"
            );
        }
        let visible = app.visible_work_ids();
        assert!(
            visible.len() >= on_screen.len(),
            "the whole visible set must still be offered to the extractor"
        );
    }

    #[test]
    fn home_names_the_hero_and_its_rails_before_the_library_behind_them() {
        let (mut app, _directory) = app_with(8);
        app.navigate(Screen::Home);
        app.reload_home();

        let on_screen = app.on_screen_work_ids();
        let expected: Vec<String> = app
            .home
            .iter()
            .chain(app.suggestions.iter().map(|suggestion| &suggestion.item))
            .map(|item| item.work_id.clone())
            .collect();
        assert_eq!(on_screen, expected);
        assert!(!on_screen.is_empty(), "home offered no card to draw");
    }

    #[test]
    fn a_marker_covers_only_the_interval_it_describes() {
        let (mut app, _directory) = app_with(1);
        app.markers = vec![SegmentMarker {
            id: "marker".to_string(),
            edition_id: "edition".to_string(),
            kind: "intro".to_string(),
            start_ms: 60_000,
            end_ms: 120_000,
            duration_ms: 3_600_000,
        }];

        app.snapshot = App::playing_snapshot(30.0, 3_600.0);
        assert!(app.active_marker().is_none());
        app.snapshot = App::playing_snapshot(60.0, 3_600.0);
        assert_eq!(
            app.active_marker().map(|marker| marker.kind),
            Some("intro".to_string())
        );
        app.snapshot = App::playing_snapshot(120.0, 3_600.0);
        assert!(app.active_marker().is_none());
    }

    #[test]
    fn automatic_skipping_only_happens_once_and_only_when_asked_for() {
        let (mut app, _directory) = app_with(1);
        app.markers = vec![SegmentMarker {
            id: "marker".to_string(),
            edition_id: "edition".to_string(),
            kind: "intro".to_string(),
            start_ms: 60_000,
            end_ms: 120_000,
            duration_ms: 3_600_000,
        }];
        let file_id = app.browse.items[0].id.clone();
        let opened =
            PlaybackTracker::open(&app.library, &app.profile_id, &file_id).expect("a session");
        app.active = Some(Active {
            tracker: opened.tracker,
            file_id,

            title: "Film".to_string(),
            year: Some(2020),
            season: None,
            episode: None,
            resume_from_ms: 1_500_000,
            origin: Screen::Browse,
            load_id: 1,
            seek_serial: 0,
        });

        app.preferences.auto_skip_intro = true;
        app.markers[0].start_ms = 0;
        app.snapshot = PlayerSnapshot {
            position_ready: false,
            load_id: 1,
            ..App::playing_snapshot(0.0, 3_600.0)
        };
        app.maybe_auto_skip_intro();
        app.snapshot = App::playing_snapshot(90.0, 3_600.0);
        app.maybe_auto_skip_intro();
        assert!(
            app.auto_skipped.is_none(),
            "a skip before the resume point landed would replace it"
        );
        app.preferences.auto_skip_intro = false;
        app.snapshot = PlayerSnapshot {
            load_id: 1,
            ..App::playing_snapshot(90.0, 3_600.0)
        };

        app.maybe_auto_skip_intro();
        assert!(app.auto_skipped.is_none());
        assert_ne!(app.status.as_deref(), Some("Skipped the intro."));

        app.preferences.auto_skip_intro = true;
        app.maybe_auto_skip_intro();
        assert_eq!(app.auto_skipped.as_deref(), Some("marker"));
        assert_eq!(app.status.as_deref(), Some("Skipped the intro."));

        app.status = None;
        app.maybe_auto_skip_intro();
        assert!(app.status.is_none());
    }

    #[test]
    fn the_home_cursor_walks_continue_watching_before_suggestions() {
        let (mut app, _directory) = app_with(2);
        let all = app.library.items(&LibraryQuery::page(10)).expect("items");
        app.suggestions = vec![Recommendation {
            item: all[0].clone(),
            reason: "Recently added".to_string(),
        }];
        app.home = vec![all[1].clone()];
        app.navigate(Screen::Home);
        app.focus_card(0);

        assert_eq!(app.visible_len(), 2, "both cards are reachable");
        assert_eq!(
            app.home_item(0).map(|item| item.id.clone()),
            Some(all[1].id.clone()),
            "the resume card is the first thing on the page"
        );
        assert_eq!(
            app.home_item(1).map(|item| item.id.clone()),
            Some(all[0].id.clone()),
            "a suggestion sits below it"
        );
        assert!(app.home_item(2).is_none());
    }

    #[test]
    fn a_test_built_app_never_reaches_for_the_menu_bar() {
        let (mut app, _directory) = app_with(1);
        assert_eq!(app.quit_routing_budget, 0, "not armed by construction");
        let _ = app.update(Message::Refresh);
        assert_eq!(app.quit_routing_budget, 0, "and never armed by an update");
        assert_eq!(app.quit_routing, crate::quit::QuitRouting::Unsupported);
    }

    #[test]
    fn opening_an_unknown_file_changes_nothing() {
        let library = Library::open_in_memory().expect("library");
        let mut app = App::with_library(library, None, "test".into());
        app.open_detail("no-such-file");
        assert_eq!(app.screen, Screen::Home);
        assert!(app.detail.is_none());
        assert!(app.error.is_some(), "an unknown id must be reported");
    }

    #[test]
    fn an_unreadable_item_is_refused_with_a_recovery_hint() {
        let directory = tempfile::TempDir::new().expect("temp dir");
        let library = Library::open_in_memory().expect("library");
        let source = library.add_source(directory.path(), None).expect("source");
        let parsed = parse_filename("Gone.2020.mkv", None);
        let file_id = library
            .ingest_file(Ingest {
                source_id: &source.id,
                relative_path: "Gone.2020.mkv",
                fingerprint: "f",
                size: 10,
                modified_ms: 1,
                scan_id: "s",
                parsed: Some(&parsed),
                expected_root: Some(&source.root_path),
            })
            .expect("ingest");
        let mut app = App::with_library(library, None, "test".into());
        app.open_player(&file_id, false);
        assert_eq!(app.screen, Screen::Home);
        assert!(
            app.error
                .as_deref()
                .is_some_and(|error| error.contains("relink")),
            "an unavailable file must name the recovery"
        );
        assert!(app.active.is_none());
    }

    #[test]
    fn focus_walks_the_grid_and_stops_at_the_edges() {
        let (mut app, _directory) = app_with(10);
        let columns = app.columns();

        assert!(columns >= 2, "the default window shows a grid");
        assert_eq!(app.focus, 0);
        let _ = app.move_focus(Focus::Left);
        assert_eq!(app.focus, 0, "focus must not leave the first card");
        let _ = app.move_focus(Focus::Right);
        assert_eq!(app.focus, 1);
        let _ = app.move_focus(Focus::Up);
        assert_eq!(app.focus, 1, "there is no row above the first");
        let _ = app.move_focus(Focus::Down);
        assert_eq!(app.focus, 1 + columns, "down moves a whole row");

        for _ in 0..20 {
            let _ = app.move_focus(Focus::Right);
            let _ = app.move_focus(Focus::Down);
        }
        assert!(app.focus < app.visible_len());
        assert_eq!(app.focus, app.visible_len() - 1);
    }

    #[test]
    fn down_the_episode_list_moves_one_episode_not_one_grid_row() {
        let (mut app, _directory) = app_with(8);
        let episodes = app.browse.items[..3].to_vec();
        let mut detail = Detail {
            item: episodes[0].clone(),
            episodes,
            shown: Vec::new(),
            seasons: Vec::new(),
            season: 1,
            origin: (Screen::Browse, 0),
        };
        detail.refresh_shown();
        app.detail = Some(detail);
        app.navigate(Screen::Detail);
        app.focus_card(0);
        assert!(
            app.columns() > 1,
            "this test only means something when the grid is wider than the list"
        );

        let _ = app.move_focus(Focus::Down);
        assert_eq!(app.focus, 1, "one arrow press moves one episode");
        let _ = app.move_focus(Focus::Down);
        assert_eq!(app.focus, 2, "so the third episode is reachable");
        let _ = app.move_focus(Focus::Down);
        assert_eq!(app.focus, 2, "and the list stops at its last episode");
        let _ = app.move_focus(Focus::Up);
        assert_eq!(app.focus, 1, "and comes back one");

        app.navigate(Screen::Browse);
        app.focus_card(0);
        let _ = app.move_focus(Focus::Down);
        assert_eq!(
            app.focus,
            app.columns(),
            "a grid still moves by the columns it was drawn with"
        );
    }

    #[test]
    fn focus_is_clamped_when_a_page_shrinks() {
        let (mut app, _directory) = app_with(10);
        app.focus_card(9);
        app.browse.text = "Title.000".to_string();
        app.reload_browse();
        assert_eq!(app.visible_len(), 1);
        assert_eq!(app.focus, 0, "a shrunken page must pull focus back in");
    }

    #[test]
    fn paging_is_bounded_and_never_runs_off_the_end() {
        let (mut app, _directory) = app_with(30);
        assert_eq!(app.browse.page_count(), 2);
        let _ = app.update(Message::PreviousPage);
        assert_eq!(app.browse.page, 0, "there is no page before the first");
        let _ = app.update(Message::NextPage);
        assert_eq!(app.browse.page, 1);
        assert_eq!(
            app.browse.items.len(),
            6,
            "the last page holds the remainder"
        );
        let _ = app.update(Message::NextPage);
        assert_eq!(app.browse.page, 1, "there is no page past the last");
    }

    #[test]
    fn search_is_literal_and_survives_punctuation() {
        let (mut app, _directory) = app_with(12);
        app.browse.text = "Title.00".to_string();
        app.reload_browse();
        assert_eq!(
            app.browse.total, 10,
            "the prefix matches Title.000 through Title.009"
        );

        app.browse.text = "O'Brien%'".to_string();
        app.reload_browse();
        assert_eq!(app.browse.total, 0);
        assert!(app.error.is_none(), "a literal search must not error");
    }

    #[test]
    fn a_finished_edition_is_marked_watched_and_leaves_continue_watching() {
        let (mut app, _directory) = app_with(2);
        let item = app.browse.items[0].clone();
        app.open_detail(&item.id);
        assert_eq!(app.screen, Screen::Detail);
        let _ = app.update(Message::ToggleWatched(item.edition_id.clone()));
        let detail = app.detail.as_ref().expect("detail");
        assert!(detail.item.watched);
        let _ = app.update(Message::ToggleWatched(item.edition_id.clone()));
        let detail = app.detail.as_ref().expect("detail");
        assert!(!detail.item.watched);
    }

    #[test]
    fn favourite_and_watchlist_survive_a_reload() {
        let (mut app, _directory) = app_with(2);
        let item = app.browse.items[0].clone();
        app.open_detail(&item.id);
        let _ = app.update(Message::ToggleFavorite(item.work_id.clone()));
        let _ = app.update(Message::ToggleWatchlist(item.work_id.clone()));
        let detail = app.detail.as_ref().expect("detail");
        assert!(detail.item.favorite);
        assert!(detail.item.watchlisted);
        app.reload();
        assert!(app.browse.items.iter().any(|row| row.favorite));
    }

    #[test]
    fn the_watchlist_filter_restricts_the_library() {
        let (mut app, _directory) = app_with(4);
        let item = app.browse.items[0].clone();
        app.library
            .set_watchlisted(&app.profile_id, &item.work_id, true)
            .expect("watchlist");
        let _ = app.update(Message::ToggleWatchlistOnly(true));
        assert_eq!(app.browse.total, 1);
        assert_eq!(app.browse.items[0].id, item.id);
        let _ = app.update(Message::ToggleWatchlistOnly(false));
        assert_eq!(app.browse.total, 4);
    }

    #[test]
    fn back_walks_home_browse_detail_and_stops_at_home() {
        let (mut app, _directory) = app_with(2);
        let _ = app.update(Message::GoBrowse);
        assert_eq!(app.screen, Screen::Browse);
        let item = app.browse.items[0].clone();
        app.open_detail(&item.id);
        assert_eq!(app.screen, Screen::Detail);
        app.back();
        assert_eq!(app.screen, Screen::Browse);
        app.back();
        assert_eq!(app.screen, Screen::Home);
        app.back();
        assert_eq!(app.screen, Screen::Home, "Home is the top of the hierarchy");
    }

    #[test]
    fn adding_a_folder_requires_an_absolute_path() {
        let library = Library::open_in_memory().expect("library");
        let mut app = App::with_library(library, None, "test".into());
        app.new_root = "relative/media".to_string();
        let _ = app.update(Message::AddFolder);
        assert!(
            app.error
                .as_deref()
                .is_some_and(|error| error.contains("absolute")),
            "a relative path must be refused with the reason"
        );
        assert!(app.scan_token.is_none());
    }

    #[test]
    fn the_default_library_path_is_absolute_and_named() {
        let path = default_library_path().expect("a data directory on this host");
        assert!(path.is_absolute());
        assert!(path.ends_with("Screenroom/library.db"));
    }

    fn series_app() -> (App, Vec<String>, tempfile::TempDir) {
        let directory = tempfile::TempDir::new().expect("temp dir");
        let library = Library::open_in_memory().expect("library");
        let source = library.add_source(directory.path(), None).expect("source");
        let mut ids = Vec::new();
        for name in ["Show.S01E01.Pilot.mkv", "Show.S01E02.Second.mkv"] {
            std::fs::write(directory.path().join(name), b"not really a video").expect("write");
            let parsed = parse_filename(name, None);
            ids.push(
                library
                    .ingest_file(Ingest {
                        source_id: &source.id,
                        relative_path: name,
                        fingerprint: &format!("fingerprint-{name}"),
                        size: 18,
                        modified_ms: 1,
                        scan_id: "test",
                        parsed: Some(&parsed),
                        expected_root: Some(&source.root_path),
                    })
                    .expect("ingest"),
            );
        }
        let app = App::with_library(library, None, "test".into());
        (app, ids, directory)
    }

    #[test]
    fn detail_starts_on_the_opened_episode_and_back_returns_to_its_card() {
        let (mut app, ids, _directory) = series_app();
        app.navigate(Screen::Browse);
        app.reload_browse();
        assert_eq!(
            app.browse.items.len(),
            1,
            "a series is one card, not one per file"
        );
        assert_eq!(
            app.browse.items[0].id, ids[0],
            "represented by its first episode"
        );
        app.focus_card(0);
        app.activate();
        assert_eq!(app.screen, Screen::Detail);
        let detail = app.detail.as_ref().expect("detail");
        assert_eq!(detail.episodes.len(), 2, "Detail lists every episode");
        assert_eq!(
            detail.episodes[app.focus].id, ids[0],
            "the cursor starts on it"
        );

        app.open_detail(&ids[1]);
        let detail = app.detail.as_ref().expect("detail");
        assert_eq!(detail.episodes[app.focus].id, ids[1]);
        assert_eq!(detail.origin, (Screen::Browse, 0), "the way back is kept");

        app.back();
        assert_eq!(app.screen, Screen::Browse);
        assert_eq!(app.focus, 0, "Back keeps the viewer's place in the grid");
    }

    #[test]
    fn back_from_detail_returns_to_the_card_and_the_watch_filter_narrows() {
        let (mut app, _directory) = app_with(4);
        app.focus_card(2);
        let opened = app.browse.items[2].id.clone();
        app.activate();
        assert_eq!(app.detail.as_ref().map(|d| d.item.id.clone()), Some(opened));
        app.back();
        assert_eq!((app.screen, app.focus), (Screen::Browse, 2));

        let first = app.browse.items[0].clone();
        app.library
            .mark_watched(&app.profile_id, &first.edition_id, true)
            .expect("mark watched");
        let _ = app.update(Message::WatchChanged(WatchFilter::Watched));
        assert_eq!(app.browse.total, 1);
        assert!(app.browse.narrowed());
        let _ = app.update(Message::WatchChanged(WatchFilter::Unwatched));
        assert_eq!(app.browse.total, 3);
        let _ = app.update(Message::WatchChanged(WatchFilter::Any));
        assert_eq!(app.browse.total, 4);
    }

    #[test]
    fn a_finished_episode_offers_the_next_with_a_cancelable_countdown() {
        let (mut app, ids, _directory) = series_app();
        let opened =
            PlaybackTracker::open(&app.library, &app.profile_id, &ids[0]).expect("a session");
        app.active = Some(Active {
            year: Some(2020),
            season: None,
            episode: None,
            tracker: opened.tracker,
            file_id: ids[0].clone(),
            title: "Pilot".to_string(),
            resume_from_ms: 0,
            origin: Screen::Detail,
            load_id: 0,
            seek_serial: 0,
        });
        app.navigate(Screen::Player);

        app.advance_to_next_episode();
        let up = app.up_next.as_ref().expect("the next episode is offered");
        assert_eq!(up.file_id, ids[1]);
        assert!(
            up.starts_at > std::time::Instant::now(),
            "it waits rather than starting at once"
        );
        assert_eq!(
            app.active.as_ref().map(|active| active.file_id.as_str()),
            Some(ids[0].as_str()),
            "the finished episode stays open during the countdown"
        );

        let _ = app.update(Message::CancelNext);
        assert!(app.up_next.is_none(), "Cancel withdraws the offer");

        app.advance_to_next_episode();
        assert!(app.up_next.is_some());
        let _ = app.update(Message::GoHome);
        assert!(app.up_next.is_none(), "leaving the player drops the offer");

        let opened =
            PlaybackTracker::open(&app.library, &app.profile_id, &ids[0]).expect("a session");
        app.active = Some(Active {
            year: Some(2020),
            season: None,
            episode: None,
            tracker: opened.tracker,
            file_id: ids[0].clone(),
            title: "Pilot".to_string(),
            resume_from_ms: 0,
            origin: Screen::Detail,
            load_id: 0,
            seek_serial: 0,
        });
        app.navigate(Screen::Player);
        app.advance_to_next_episode();
        assert!(app.up_next.is_some());
        app.replay();
        assert!(
            app.up_next.is_none(),
            "a replay must not be replaced by the next episode five seconds later"
        );
    }

    #[test]
    fn autoplay_follows_the_next_verified_episode() {
        let (app, ids, _directory) = series_app();
        let next = app
            .next_episode_after(&ids[0])
            .expect("lookup")
            .expect("the second episode follows the first");
        assert_eq!(next.id, ids[1]);
        assert!(
            app.next_episode_after(&ids[1]).unwrap().is_none(),
            "the last episode has no successor to guess at"
        );
    }

    #[test]
    fn autoplay_stands_down_when_the_preference_is_off() {
        let (mut app, ids, _directory) = series_app();
        let _ = app.update(Message::ToggleAutoplay(false));
        assert!(!app.preferences.autoplay_next);
        assert!(
            app.next_episode_after(&ids[0]).unwrap().is_none(),
            "a viewer who turned autoplay off must not have it happen anyway"
        );

        assert!(!app.library.preferences().unwrap().autoplay_next);
    }

    #[test]
    fn a_movie_has_no_next_episode() {
        let (app, _directory) = app_with(1);
        let item = app.browse.items[0].clone();
        assert!(
            app.next_episode_after(&item.id).unwrap().is_none(),
            "autoplay must not invent an episode after a film"
        );
    }

    #[test]
    fn switching_a_profile_keeps_each_viewers_data_separate() {
        let (mut app, _directory) = app_with(3);
        let work = app.browse.items[0].work_id.clone();
        let first = app.profile_id.clone();
        app.library.set_favorite(&first, &work, true).unwrap();

        let _ = app.update(Message::NewProfileChanged("Guest".to_string()));
        let _ = app.update(Message::AddProfile);
        let guest = app.profile_id.clone();
        assert_ne!(guest, first, "adding a profile switches to it");
        assert_eq!(app.profiles.len(), 2);
        assert!(
            !app.browse.items.iter().any(|item| item.favorite),
            "the new profile must not inherit the first profile's favourites"
        );

        assert_eq!(
            app.library.preferences().unwrap().default_profile,
            Some(guest.clone())
        );

        let _ = app.update(Message::ProfileChosen(first));
        assert_eq!(
            app.library
                .count(&LibraryQuery {
                    profile_id: Some(app.profile_id.clone()),
                    favorites_only: true,
                    ..LibraryQuery::page(PAGE_SIZE)
                })
                .unwrap(),
            1,
            "the first viewer still has their own favourite"
        );
        assert_eq!(
            app.library
                .count(&LibraryQuery {
                    profile_id: Some(guest),
                    favorites_only: true,
                    ..LibraryQuery::page(PAGE_SIZE)
                })
                .unwrap(),
            0,
            "and the other viewer has none of it"
        );
    }

    #[test]
    fn the_last_profile_is_kept() {
        let (mut app, _directory) = app_with(1);
        assert_eq!(app.profiles.len(), 1);
        let only = app.profile_id.clone();
        let _ = app.update(Message::RemoveProfile(only.clone()));
        assert_eq!(app.profiles.len(), 1, "a device always keeps one profile");
        assert_eq!(app.profile_id, only);
        assert!(
            app.error
                .as_deref()
                .is_some_and(|e| e.contains("one profile")),
            "the refusal is explained"
        );
    }

    #[test]
    fn an_invalid_language_tag_is_reported_and_not_stored() {
        let (mut app, _directory) = app_with(1);
        let _ = app.update(Message::SubtitleLanguageChanged(
            "en; drop table".to_string(),
        ));
        let _ = app.update(Message::ApplySubtitleLanguage);
        assert_eq!(app.library.preferences().unwrap().subtitle_language, None);
        assert!(app.error.is_some(), "the refusal is shown");

        let _ = app.update(Message::SubtitleLanguageChanged("fr".to_string()));
        let _ = app.update(Message::ApplySubtitleLanguage);
        assert_eq!(
            app.library
                .preferences()
                .unwrap()
                .subtitle_language
                .as_deref(),
            Some("fr")
        );
    }

    #[test]
    fn removing_a_folder_asks_first_and_keeps_the_watch_history() {
        let (mut app, _directory) = app_with(2);
        let source = app.sources[0].clone();
        let item = app.browse.items[0].clone();
        app.library
            .set_favorite(&app.profile_id, &item.work_id, true)
            .unwrap();

        let _ = app.update(Message::RemoveSource(source.id.clone()));
        assert_eq!(
            app.armed_remove.as_deref(),
            Some(source.id.as_str()),
            "the first press asks rather than acts"
        );
        assert_eq!(app.sources.len(), 1, "nothing has happened yet");

        let _ = app.update(Message::RemoveSource(source.id.clone()));
        assert!(app.armed_remove.is_none());
        assert_eq!(app.browse.total, 0, "the folder's files leave the library");
        assert!(
            app.library.sources().unwrap().is_empty(),
            "the folder leaves the visible list"
        );
        assert!(
            app.library.source(&source.id).is_ok(),
            "but its row survives, so recovery copy can still name it"
        );

        assert_eq!(
            app.library
                .count(&LibraryQuery {
                    profile_id: Some(app.profile_id.clone()),
                    favorites_only: true,
                    ..LibraryQuery::page(PAGE_SIZE)
                })
                .unwrap(),
            0,
            "a removed folder's entries are hidden, not offered"
        );
        app.library
            .set_source_state(&source.id, SourceState::Available)
            .expect("re-admit");
        app.reload();
        assert_eq!(
            app.library
                .count(&LibraryQuery {
                    profile_id: Some(app.profile_id.clone()),
                    favorites_only: true,
                    ..LibraryQuery::page(PAGE_SIZE)
                })
                .unwrap(),
            1,
            "the favourite was kept through the removal"
        );
    }

    #[test]
    fn a_backup_and_restore_round_trip_through_the_settings_screen() {
        let directory = tempfile::TempDir::new().expect("temp dir");
        let library = Library::open(directory.path().join("library.db")).expect("library");
        let media = directory.path().join("media");
        std::fs::create_dir_all(&media).unwrap();
        let source = library.add_source(&media, None).expect("source");
        let name = "Heat.1995.mkv";
        std::fs::write(media.join(name), b"not really a video").unwrap();
        let parsed = parse_filename(name, None);
        let file_id = library
            .ingest_file(Ingest {
                source_id: &source.id,
                relative_path: name,
                fingerprint: "f",
                size: 18,
                modified_ms: 1,
                scan_id: "test",
                parsed: Some(&parsed),
                expected_root: Some(&source.root_path),
            })
            .expect("ingest");
        let mut app = App::with_library(library, None, "test".into());
        app.profile_id = app.library.profiles().unwrap().remove(0).id;
        let backup = directory.path().join("backup.db");
        app.backup_path = backup.to_string_lossy().to_string();
        let _ = app.update(Message::BackupNow);
        settle_jobs(&mut app);
        assert!(backup.is_file(), "the backup is written: {:?}", app.error);
        assert!(app.error.is_none(), "unexpected error: {:?}", app.error);

        app.library
            .set_favorite(&app.profile_id, "anything", true)
            .ok();
        let session = app
            .library
            .start_session(&app.profile_id, &file_id)
            .unwrap();
        app.library
            .checkpoint(screenroom_core::Checkpoint {
                session: &session,
                position_ms: 5_000,
                duration_ms: 60_000,
                sequence: 1,
                seek_generation: 0,
                ended: false,
            })
            .unwrap();
        app.library.end_session(&session).unwrap();
        assert_eq!(
            app.library
                .continue_watching(&app.profile_id, 5)
                .unwrap()
                .len(),
            1
        );

        app.restore_path = backup.to_string_lossy().to_string();
        let _ = app.update(Message::RestoreNow);
        settle_jobs(&mut app);
        assert!(app.armed_restore.is_some(), "restore asks before replacing");
        assert_eq!(
            app.library
                .continue_watching(&app.profile_id, 5)
                .unwrap()
                .len(),
            1,
            "asking changes nothing"
        );

        let _ = app.update(Message::RestoreNow);
        settle_jobs(&mut app);
        assert!(app.armed_restore.is_none());
        assert!(app.error.is_none(), "unexpected error: {:?}", app.error);
        assert_eq!(
            app.library
                .continue_watching(&app.profile_id, 5)
                .unwrap()
                .len(),
            0,
            "the restored state is the backup's, not the later change"
        );
        let safety = app
            .status
            .clone()
            .expect("the result names the safety copy");
        assert!(safety.contains("before-restore"), "status was: {safety}");
    }

    #[test]
    fn a_restore_brings_back_the_backups_preferences_and_keeps_them() {
        let (mut app, _directory) = app_with(1);
        let directory = tempfile::TempDir::new().expect("temp dir");
        let backup = directory.path().join("prefs.db");
        let _ = app.update(Message::SubtitleLanguageChanged("bn".to_string()));
        let _ = app.update(Message::ApplySubtitleLanguage);
        app.library.backup_to(&backup).expect("backup");
        let _ = app.update(Message::SubtitleLanguageChanged("fr".to_string()));
        let _ = app.update(Message::ApplySubtitleLanguage);

        app.restore_path = backup.to_string_lossy().to_string();
        let _ = app.update(Message::RestoreNow);
        let _ = app.update(Message::RestoreNow);
        settle_jobs(&mut app);
        assert!(app.error.is_none(), "unexpected error: {:?}", app.error);
        assert_eq!(app.preferences.subtitle_language.as_deref(), Some("bn"));
        assert_eq!(
            app.subtitle_input, "bn",
            "the field shows the restored value"
        );

        let _ = app.update(Message::ToggleAutoplay(false));
        assert_eq!(
            app.library
                .preferences()
                .unwrap()
                .subtitle_language
                .as_deref(),
            Some("bn")
        );
    }

    #[test]
    fn removing_a_profile_asks_first_and_the_choice_survives_a_reopen() {
        let (mut app, _directory) = app_with(1);
        let first = app.profile_id.clone();
        app.new_profile = "Second".to_string();
        let _ = app.update(Message::AddProfile);
        let second = app.profile_id.clone();
        assert_ne!(first, second, "adding a profile switches to it");
        assert_eq!(
            opening_profile(&app.library, &app.library.preferences().unwrap()),
            second,
            "a reopened library starts with the chosen profile"
        );

        let _ = app.update(Message::RemoveProfile(first.clone()));
        assert_eq!(app.profiles.len(), 2, "the first press only asks");
        assert_eq!(app.armed_profile.as_deref(), Some(first.as_str()));
        let _ = app.update(Message::RemoveProfile(first.clone()));
        assert_eq!(app.profiles.len(), 1, "the second press removes it");
        assert_eq!(app.profile_id, second, "the active viewer is untouched");
        assert_eq!(
            app.library
                .preferences()
                .unwrap()
                .default_profile
                .as_deref(),
            Some(second.as_str()),
            "removing another viewer leaves the stored choice alone"
        );
    }

    #[test]
    fn inspecting_a_backup_reports_what_it_holds_without_restoring() {
        let (mut app, _directory) = app_with(2);
        let directory = tempfile::TempDir::new().expect("temp dir");
        let backup = directory.path().join("copy.db");
        app.library.backup_to(&backup).expect("backup");
        app.restore_path = backup.to_string_lossy().to_string();
        let _ = app.update(Message::InspectRestore);
        let status = app.status.clone().expect("a summary");
        assert!(status.contains("2 works"), "status was: {status}");
        assert!(
            app.armed_restore.is_none(),
            "checking a file is not a restore"
        );
        assert_eq!(app.browse.total, 2, "and it changed nothing");
    }

    #[test]
    fn the_settings_screen_is_reachable_and_leaves_again() {
        let (mut app, _directory) = app_with(2);
        let _ = app.update(Message::GoSettings);
        assert_eq!(app.screen, Screen::Settings);
        assert!(!app.profiles.is_empty());
        assert!(!app.sources.is_empty());
        app.back();
        assert_eq!(app.screen, Screen::Home);
    }

    #[test]
    fn quitting_with_a_file_open_saves_the_final_position() {
        let (mut app, _directory) = app_with(1);
        let item = app.browse.items[0].clone();

        let opened =
            PlaybackTracker::open(&app.library, &app.profile_id, &item.id).expect("a session");
        app.active = Some(Active {
            year: Some(2020),
            season: None,
            episode: None,
            tracker: opened.tracker,
            file_id: item.id.clone(),
            title: item.title.clone(),
            resume_from_ms: opened.resume_from_ms,
            origin: Screen::Browse,
            load_id: 0,
            seek_serial: 0,
        });
        app.snapshot = App::playing_snapshot(42.5, 100.0);

        app.stop_playback();

        assert!(app.active.is_none(), "quitting ends the viewing");
        assert_eq!(
            app.snapshot.position, 0.0,
            "and clears the player state it was showing"
        );
        let watching = app
            .library
            .continue_watching(&app.profile_id, 5)
            .expect("continue watching");
        assert_eq!(watching.len(), 1, "the viewing is offered to resume");
        assert_eq!(
            watching[0].position_ms, 42_500,
            "the position at quit is what was stored, to the tenth of a second"
        );

        let reopened =
            PlaybackTracker::open(&app.library, &app.profile_id, &item.id).expect("a new session");
        assert_eq!(reopened.resume_from_ms, 42_500);
    }

    #[test]
    fn leaving_the_player_by_the_header_stores_the_viewing() {
        for go in [Message::GoHome, Message::GoBrowse, Message::GoSettings] {
            let (mut app, _directory) = app_with(1);
            let item = app.browse.items[0].clone();
            let opened =
                PlaybackTracker::open(&app.library, &app.profile_id, &item.id).expect("a session");
            app.active = Some(Active {
                year: Some(2020),
                season: None,
                episode: None,
                tracker: opened.tracker,
                file_id: item.id.clone(),
                title: item.title.clone(),
                resume_from_ms: opened.resume_from_ms,
                origin: Screen::Browse,
                load_id: 0,
                seek_serial: 0,
            });
            app.navigate(Screen::Player);
            app.snapshot = App::playing_snapshot(30.0, 100.0);

            let _ = app.update(go);

            assert!(app.active.is_none(), "navigating away ends the viewing");
            assert_ne!(app.screen, Screen::Player);
            let watching = app
                .library
                .continue_watching(&app.profile_id, 5)
                .expect("continue watching");
            assert_eq!(watching.len(), 1);
            assert_eq!(watching[0].position_ms, 30_000);
        }
    }

    #[test]
    fn only_the_current_loads_settled_position_is_stored() {
        let (app, _directory) = app_with(1);
        let item = app.browse.items[0].clone();
        let opened =
            PlaybackTracker::open(&app.library, &app.profile_id, &item.id).expect("a session");
        let active = Active {
            tracker: opened.tracker,
            file_id: item.id.clone(),

            title: item.title.clone(),
            year: item.year,
            season: item.season,
            episode: item.episode,
            resume_from_ms: 0,
            origin: Screen::Browse,
            load_id: 7,
            seek_serial: 3,
        };
        let current = PlayerSnapshot {
            load_id: 7,
            seek_serial: 3,
            ..App::playing_snapshot(10.0, 100.0)
        };
        assert!(snapshot_is_current(&active, &current));

        assert!(!snapshot_is_current(
            &active,
            &PlayerSnapshot {
                load_id: 6,
                ..current.clone()
            }
        ));

        assert!(!snapshot_is_current(
            &active,
            &PlayerSnapshot {
                position_ready: false,
                ..current.clone()
            }
        ));

        assert!(!snapshot_is_current(
            &active,
            &PlayerSnapshot {
                seek_serial: 2,
                ..current
            }
        ));
    }

    #[test]
    fn playback_never_resolves_outside_the_granted_folder() {
        let directory = tempfile::TempDir::new().expect("temp dir");
        let root = directory.path().join("granted");
        write_settled(&root.join("Film.2020.mkv"), b"inside");
        write_settled(&directory.path().join("secret.mkv"), b"outside");
        assert!(contained_media_path(&root, "Film.2020.mkv").is_some());
        assert!(contained_media_path(&root, "../secret.mkv").is_none());
        assert!(contained_media_path(&root, "missing.mkv").is_none());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(directory.path().join("secret.mkv"), root.join("link.mkv"))
                .expect("link");
            assert!(contained_media_path(&root, "link.mkv").is_none());
        }
    }

    #[test]
    fn the_window_mode_read_back_overrides_what_the_app_last_asked_for() {
        let (mut app, _directory) = app_with(1);
        app.fullscreen = true;

        let _ = app.update(Message::ModeChanged(iced::window::Mode::Windowed));
        assert!(!app.fullscreen);
        let _ = app.update(Message::ModeChanged(iced::window::Mode::Fullscreen));
        assert!(app.fullscreen);
    }

    #[test]
    fn choosing_an_unknown_profile_changes_nothing() {
        let (mut app, _directory) = app_with(1);
        let before = app.profile_id.clone();
        let _ = app.update(Message::ProfileChosen("no-such-profile".to_string()));
        assert_eq!(app.profile_id, before);
        assert!(app.error.is_some());
    }

    fn finish_scan(app: &mut App) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        while app.scan_token.is_some() {
            assert!(
                std::time::Instant::now() < deadline,
                "the scan never finished"
            );
            let _ = app.update(Message::Tick);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }

        let _ = app.update(Message::Tick);
    }

    #[test]
    fn a_second_scan_waits_for_the_first_instead_of_orphaning_it() {
        let directory = tempfile::TempDir::new().expect("temp dir");
        for (folder, name) in [("a", "First.Film.2001.mkv"), ("b", "Second.Film.2002.mkv")] {
            write_settled(
                &directory.path().join(folder).join(name),
                b"not really a video",
            );
        }
        let library = Library::open_in_memory().expect("library");
        let mut app = App::with_library(library, None, "test".into());
        app.new_root = directory.path().join("a").to_string_lossy().to_string();
        let _ = app.update(Message::AddFolder);
        assert!(app.scan_token.is_some());
        app.new_root = directory.path().join("b").to_string_lossy().to_string();
        let _ = app.update(Message::AddFolder);

        assert_eq!(app.scan_queue.queued(), 1, "the second scan is queued");
        assert!(app.scan_token.is_some());
        finish_scan(&mut app);
        assert_eq!(app.scan_queue.queued(), 0);
        assert_eq!(
            app.library.count(&LibraryQuery::default()).expect("count"),
            2,
            "both folders were scanned"
        );
    }

    #[test]
    fn a_watched_arrival_reaches_the_library_without_a_button() {
        let directory = tempfile::TempDir::new().expect("temp dir");
        let media = directory.path().join("media");
        write_settled(
            &media.join("Arrival (2016)/Arrival (2016).mkv"),
            b"not really a video",
        );
        let library = Library::open_in_memory().expect("library");
        let mut app = App::with_library(library, None, "test".into());
        add_folder(&mut app, &media);
        let _ = app.update(Message::GoBrowse);
        assert_eq!(app.browse.items.len(), 1);

        let arrival = media.join("Dune (2021)/Dune (2021).mkv");
        write_settled(&arrival, b"another not really a video");
        app.handle_watched(crate::watch::Watched::Changes {
            root: media.clone(),
            changes: vec![screenroom_core::MediaChange::Added(arrival.clone())],
        });
        assert!(app.scan_token.is_some(), "an arrival must start an ingest");
        finish_scan(&mut app);

        assert_eq!(app.error, None);
        assert_eq!(
            app.browse.items.len(),
            2,
            "the arrival is in the library without a rescan button"
        );
        assert!(
            app.browse.items.iter().any(|item| item.title == "Dune"),
            "the arrival was parsed into the catalog"
        );
    }

    #[test]
    fn a_watched_removal_withdraws_the_path_it_names() {
        let directory = tempfile::TempDir::new().expect("temp dir");
        let media = directory.path().join("media");
        let gone = media.join("Gone (2001).mkv");
        write_settled(&gone, b"not really a video");
        let library = Library::open_in_memory().expect("library");
        let mut app = App::with_library(library, None, "test".into());
        add_folder(&mut app, &media);
        let _ = app.update(Message::GoBrowse);
        assert_eq!(app.browse.items.len(), 1);

        std::fs::remove_file(&gone).expect("remove");
        app.handle_watched(crate::watch::Watched::Changes {
            root: media.clone(),
            changes: vec![screenroom_core::MediaChange::Removed(gone.clone())],
        });

        let running = app
            .scan_queue
            .running()
            .cloned()
            .expect("a withdrawal was queued");
        assert!(
            matches!(running, crate::watch::Job::Withdraw { .. }),
            "a removal asked for a walk instead of withdrawing one path: {running:?}"
        );
        assert_eq!(running.paths().len(), 1);
        assert_eq!(
            running.paths()[0].file_name(),
            gone.file_name(),
            "the withdrawal names a different file"
        );
        assert!(
            running.is_quiet(),
            "a removal does not take the status line"
        );

        finish_scan(&mut app);

        assert_eq!(app.error, None);
        assert_eq!(
            app.library.count(&LibraryQuery::default()).expect("count"),
            1,
            "a withdrawal deleted the row"
        );
        assert!(
            !app.library.items(&LibraryQuery::page(10)).expect("items")[0].available,
            "the removed file is still available"
        );
    }

    #[test]
    fn a_sleep_gap_on_the_tick_triggers_the_catch_up_walk() {
        let (mut app, _directory) = app_with(1);
        app.auto_discovery = true;
        app.watch_reconciled = true;
        app.watch_reconcile_at = std::time::Instant::now();
        app.watch_wall = crate::watch::clock() - Duration::from_secs(3_600);

        let _ = app.update(Message::Tick);

        let running = app
            .scan_queue
            .running()
            .cloned()
            .expect("a sleep gap must start a walk");
        assert!(running.is_scan());
        finish_scan(&mut app);
    }

    #[test]
    fn a_reconcile_walks_each_attached_folder_once() {
        let directory = tempfile::TempDir::new().expect("temp dir");
        let media = directory.path().join("media");
        write_settled(&media.join("Arrival (2016).mkv"), b"not really a video");
        let library = Library::open_in_memory().expect("library");
        let mut app = App::with_library(library, None, "test".into());
        add_folder(&mut app, &media);
        assert!(app.scan_queue.running().is_none());

        app.reconcile();

        assert!(
            app.scan_queue.running().is_some(),
            "a reconcile starts a walk"
        );
        assert!(
            app.scan_queue.running().is_some_and(|job| job.is_quiet()),
            "a background reconcile stays out of the status line"
        );
        finish_scan(&mut app);
        assert_eq!(app.scan_queue.queued(), 0);
        assert!(app.scan_queue.running().is_none());
    }

    fn add_folder(app: &mut App, root: &std::path::Path) {
        app.new_root = root.to_string_lossy().to_string();
        let _ = app.update(Message::AddFolder);
        assert!(
            app.scan_token.is_some(),
            "adding a folder must start its scan"
        );
        finish_scan(app);
    }

    fn write_settled(path: &std::path::Path, contents: &[u8]) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("folder");
        }
        std::fs::write(path, contents).expect("write");
        let file = std::fs::File::options()
            .write(true)
            .open(path)
            .expect("open");
        file.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(3_600))
            .expect("backdate");
    }

    fn folder_fingerprint(
        root: &std::path::Path,
    ) -> Vec<(String, u64, Option<std::time::SystemTime>)> {
        let mut entries = Vec::new();
        let mut pending = vec![root.to_path_buf()];
        while let Some(path) = pending.pop() {
            let Ok(listing) = std::fs::read_dir(&path) else {
                continue;
            };
            for entry in listing.flatten() {
                let path = entry.path();
                let metadata = entry.metadata().expect("metadata");
                let relative = path
                    .strip_prefix(root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .to_string();
                entries.push((relative, metadata.len(), metadata.modified().ok()));
                if metadata.is_dir() {
                    pending.push(path);
                }
            }
        }
        entries.sort();
        entries
    }

    #[test]
    fn a_first_run_scan_builds_the_library_and_writes_nothing_to_the_media_folder() {
        let directory = tempfile::TempDir::new().expect("temp dir");
        let media = directory.path().join("media");
        write_settled(
            &media.join("Arrival (2016)/Arrival (2016).mkv"),
            b"not really a video",
        );
        let before = folder_fingerprint(&media);

        let library = Library::open_in_memory().expect("library");
        let mut app = App::with_library(library, None, "test".into());
        assert!(app.sources.is_empty(), "a first run starts with no folder");
        assert_eq!(app.home.len(), 0, "and nothing to watch");

        add_folder(&mut app, &media);

        assert_eq!(app.error, None, "a scan of a readable folder must not fail");
        assert_eq!(app.sources.len(), 1, "the folder was granted");
        let _ = app.update(Message::GoBrowse);
        assert_eq!(app.browse.items.len(), 1, "the film is in the library");
        let item = &app.browse.items[0];
        assert_eq!(item.title, "Arrival", "the title came from the filename");
        assert_eq!(item.year, Some(2016));
        assert_eq!(item.kind, MediaKind::Movie);

        assert_eq!(
            before,
            folder_fingerprint(&media),
            "the scan must not create, change or remove anything in the media folder"
        );
    }

    #[test]
    fn a_folder_that_moved_keeps_its_history_until_it_is_relinked() {
        let directory = tempfile::TempDir::new().expect("temp dir");
        let media = directory.path().join("media");
        write_settled(&media.join("Show.S01E01.Pilot.mkv"), b"not really a video");

        let library = Library::open_in_memory().expect("library");
        let mut app = App::with_library(library, None, "test".into());
        add_folder(&mut app, &media);
        let source_id = app.sources[0].id.clone();
        let item = app
            .library
            .items(&LibraryQuery::page(10))
            .expect("items")
            .remove(0);

        let session = app
            .library
            .start_session(&app.profile_id, &item.id)
            .expect("session");
        app.library
            .checkpoint(Checkpoint {
                session: &session,
                position_ms: 240_000,
                duration_ms: 1_800_000,
                sequence: 1,
                seek_generation: 0,
                ended: false,
            })
            .expect("checkpoint");
        app.library.end_session(&session).expect("end");

        let moved = directory.path().join("media 1");
        std::fs::rename(&media, &moved).expect("move the folder");
        let _ = app.update(Message::RescanSource(source_id.clone()));
        finish_scan(&mut app);

        assert_eq!(
            app.library.count(&LibraryQuery::default()).expect("count"),
            1,
            "an unreadable folder must not empty the library"
        );
        let offline = app
            .library
            .item(&item.id, Some(&app.profile_id))
            .expect("item")
            .expect("the row is still there");
        assert!(!offline.available, "its file cannot be read right now");
        assert_eq!(
            offline.position_ms, 240_000,
            "the resume point survived the folder disappearing"
        );

        let _ = app.update(Message::RelinkTargetChosen(source_id.clone()));
        let _ = app.update(Message::RelinkPathChanged(
            moved.to_string_lossy().to_string(),
        ));
        let _ = app.update(Message::RelinkSource);
        finish_scan(&mut app);

        assert_eq!(
            app.error, None,
            "a relink to a readable folder must not fail"
        );
        let online = app
            .library
            .item(&item.id, Some(&app.profile_id))
            .expect("item")
            .expect("the same row, not a new one");
        assert!(
            online.available,
            "the episode plays again from the new path"
        );
        assert_eq!(
            online.position_ms, 240_000,
            "and resumes where the viewer stopped"
        );
        assert_eq!(
            app.library.count(&LibraryQuery::default()).expect("count"),
            1,
            "relinking must not duplicate the episode"
        );
    }

    #[test]
    fn a_restore_from_a_damaged_backup_changes_nothing() {
        let directory = tempfile::TempDir::new().expect("temp dir");
        let library = Library::open_in_memory().expect("library");
        let source = library.add_source(directory.path(), None).expect("source");
        std::fs::write(
            directory.path().join("Film.2020.mkv"),
            b"not really a video",
        )
        .expect("write");
        let parsed = parse_filename("Film.2020.mkv", None);
        let file_id = library
            .ingest_file(Ingest {
                source_id: &source.id,
                relative_path: "Film.2020.mkv",
                fingerprint: "fingerprint-film",
                size: 18,
                modified_ms: 1,
                scan_id: "test",
                parsed: Some(&parsed),
                expected_root: Some(&source.root_path),
            })
            .expect("ingest");
        let mut app = App::with_library(library, None, "test".into());
        let session = app
            .library
            .start_session(&app.profile_id, &file_id)
            .expect("session");
        app.library
            .checkpoint(Checkpoint {
                session: &session,
                position_ms: 120_000,
                duration_ms: 600_000,
                sequence: 1,
                seek_generation: 0,
                ended: false,
            })
            .expect("checkpoint");
        app.library.end_session(&session).expect("end");
        let profiles_before = app.library.profiles().expect("profiles").len();
        let count_before = app.library.count(&LibraryQuery::default()).expect("count");

        let damaged = directory.path().join("not-a-backup.json");
        std::fs::write(&damaged, b"this is not a Screenroom backup").expect("write");
        let _ = app.update(Message::RestorePathChanged(
            damaged.to_string_lossy().to_string(),
        ));

        let _ = app.update(Message::RestoreNow);
        let _ = app.update(Message::RestoreNow);
        settle_jobs(&mut app);

        let error = app
            .error
            .clone()
            .expect("a damaged backup must be reported");
        assert!(
            error.contains("backup") || error.contains("readable"),
            "the reason must name the backup, got {error:?}"
        );
        assert_eq!(
            app.library.count(&LibraryQuery::default()).expect("count"),
            count_before,
            "a refused restore must not change the catalogue"
        );
        assert_eq!(
            app.library.profiles().expect("profiles").len(),
            profiles_before
        );
        let item = app
            .library
            .item(&file_id, Some(&app.profile_id))
            .expect("item")
            .expect("the row");
        assert_eq!(item.position_ms, 120_000, "progress is untouched");
    }

    #[test]
    fn the_home_cursor_walks_rails_and_keeps_the_column() {
        let (mut app, _directory) = app_with(4);
        let items = app.library.items(&LibraryQuery::page(10)).expect("items");
        app.home = items;
        app.navigate(Screen::Home);
        app.focus_card(0);

        let _ = app.update(Message::Move(Focus::Right));
        assert_eq!(
            app.nav.focus(),
            FocusPos { rail: 0, column: 1 },
            "right moves one card inside the rail"
        );
        let selected = app.home_selected_item().map(|item| item.id.clone());
        assert!(selected.is_some(), "the focused card resolves to an item");

        let _ = app.update(Message::Move(Focus::Down));
        assert_eq!(
            app.nav.focus(),
            FocusPos { rail: 1, column: 1 },
            "down moves to the next rail and keeps the column"
        );
        assert_eq!(
            app.home_selected_item().map(|item| item.id.clone()),
            app.home_sections()[1].1[1].id.clone().into(),
            "the selected item is the one under the cursor"
        );

        let _ = app.update(Message::Move(Focus::Left));
        let _ = app.update(Message::Move(Focus::Left));
        let _ = app.update(Message::Move(Focus::Left));
        assert_eq!(
            app.nav.focus(),
            FocusPos { rail: 1, column: 0 },
            "left clamps at the first card instead of wrapping"
        );
    }

    #[test]
    fn choosing_a_season_changes_the_listed_episodes_without_leaving_the_page() {
        let (mut app, _directory) = app_with(4);
        let mut episodes = app.library.items(&LibraryQuery::page(10)).expect("items");
        for (index, episode) in episodes.iter_mut().enumerate() {
            episode.kind = MediaKind::Series;
            episode.season = Some(if index < 2 { 1 } else { 2 });
            episode.episode = Some((index % 2 + 1) as i64);
        }
        let mut detail = Detail {
            item: episodes[0].clone(),
            episodes,
            shown: Vec::new(),
            seasons: vec![1, 2],
            season: 1,
            origin: (Screen::Browse, 0),
        };
        detail.refresh_shown();
        app.detail = Some(detail);
        app.navigate(Screen::Detail);
        app.focus_card(1);

        let _ = app.update(Message::SeasonChosen(2));

        let detail = app.detail.as_ref().expect("the page stays open");
        assert_eq!(detail.season, 2);
        assert_eq!(detail.season_episodes().len(), 2);
        assert!(
            detail
                .season_episodes()
                .iter()
                .all(|episode| episode.season == Some(2)),
            "the list holds only the chosen season"
        );
        assert_eq!(app.screen, Screen::Detail);
        assert_eq!(app.focus, 0, "the cursor returns to the first episode");
    }

    #[test]
    fn casting_and_profiles_are_populated_from_what_the_app_already_holds() {
        use ui::screens::settings::{Row, Section};

        let (mut app, _directory) = app_with(1);
        app.receivers.push(crate::casting::Receiver {
            id: "receiver-1".to_string(),
            name: "Samsung TV".to_string(),
            kind: "tv".to_string(),
            address: "192.0.2.10".to_string(),
            from: "ssdp".to_string(),
        });
        app.profiles = vec![
            Profile {
                id: "me".to_string(),
                name: "Rahi".to_string(),
            },
            Profile {
                id: "guest".to_string(),
                name: "Guest".to_string(),
            },
        ];
        app.profile_id = "me".to_string();

        app.settings_section = Section::Casting;
        let rows = app.settings_rows();
        assert!(
            rows.iter().any(|row| matches!(
                row,
                Row::Action { label, action: "Cast", on_press: Some(_), .. } if *label == "Samsung TV"
            )),
            "the receiver is offered as a cast target"
        );
        assert!(
            rows.iter().any(|row| matches!(
                row,
                Row::Action {
                    action: "Look now",
                    on_press: Some(Message::LookForReceivers),
                    ..
                }
            )),
            "discovery stays the viewer's choice"
        );

        app.settings_section = Section::Profiles;
        let rows = app.settings_rows();
        assert!(
            rows.iter().any(|row| matches!(
                row,
                Row::Value { label, .. } if *label == "Rahi"
            )),
            "the active profile is stated, not offered"
        );
        assert!(
            rows.iter().any(|row| matches!(
                row,
                Row::Action { label, action: "Use", on_press: Some(_), .. } if *label == "Guest"
            )),
            "another profile can be chosen"
        );
    }
}
