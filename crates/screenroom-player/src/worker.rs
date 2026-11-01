use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::render::{MAX_DIMENSION, SoftwareRenderer, UPDATE_FRAME, target_size};
use crate::{Frame, LibMpv, Mpv, PlayerError, PlayerState, WakeSignal, event};

fn tracing() -> bool {
    use std::sync::OnceLock;
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("SCREENROOM_PLAYER_TRACE").is_some())
}

macro_rules! trace {
    ($($arg:tt)*) => {
        if tracing() {
            eprintln!("[player] {}", format_args!($($arg)*));
        }
    };
}

const ID_TIME_POS: u64 = 1;
const ID_DURATION: u64 = 2;
const ID_PAUSE: u64 = 3;
const ID_EOF_REACHED: u64 = 4;
const ID_AUDIO_PTS: u64 = 5;
const ID_AVSYNC: u64 = 6;
const ID_TOTAL_AVSYNC_CHANGE: u64 = 7;
const ID_MISTIMED_FRAMES: u64 = 8;
const ID_VOLUME: u64 = 9;
const ID_MUTE: u64 = 10;
const ID_TRACK_LIST: u64 = 11;
const ID_HWDEC_CURRENT: u64 = 12;
const ID_DWIDTH: u64 = 13;
const ID_DHEIGHT: u64 = 14;

fn is_load_time_property(id: u64) -> bool {
    matches!(
        id,
        ID_TIME_POS | ID_DURATION | ID_DWIDTH | ID_DHEIGHT | ID_HWDEC_CURRENT | ID_TRACK_LIST
    )
}

const FORMAT_FLAG: i32 = 3;
const FORMAT_INT64: i32 = 4;
const FORMAT_DOUBLE: i32 = 5;
const FORMAT_NODE: i32 = 6;
const FORMAT_STRING: i32 = 1;

#[repr(C)]
struct RawProperty {
    name: *const std::ffi::c_char,
    format: i32,
    data: *mut std::ffi::c_void,
}

#[repr(C)]
struct RawEndFile {
    reason: i32,
    error: i32,
    playlist_entry_id: i64,
}

#[repr(C)]
struct RawStartFile {
    playlist_entry_id: i64,
}

mod end_reason {
    pub const EOF: i32 = 0;
    pub const ERROR: i32 = 4;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerFailure {
    pub code: String,

    pub message: String,
}

#[derive(Debug, Clone)]
pub struct PlayerSnapshot {
    pub load_id: u64,

    pub position_ready: bool,

    pub seek_serial: u64,

    pub state: PlayerState,

    pub position: f64,

    pub duration: f64,

    pub frame: Option<Frame>,

    pub failure: Option<PlayerFailure>,

    pub ended: bool,

    pub loaded: bool,

    pub tracks: Vec<MediaTrack>,

    pub audio_pts: Option<f64>,

    pub avsync: Option<f64>,

    pub total_avsync_change: Option<f64>,

    pub mistimed_frames: Option<i64>,

    pub subtitle_scale: Option<f64>,

    pub subtitle_position: Option<f64>,

    pub volume: Option<f64>,

    pub muted: Option<bool>,

    pub hardware_decoder: Option<String>,
}

impl Default for PlayerSnapshot {
    fn default() -> Self {
        Self {
            load_id: 0,
            position_ready: false,
            seek_serial: 0,
            state: PlayerState::Idle,
            position: 0.0,
            duration: 0.0,
            frame: None,
            failure: None,
            ended: false,
            loaded: false,
            tracks: Vec::new(),
            audio_pts: None,
            avsync: None,
            total_avsync_change: None,
            mistimed_frames: None,
            subtitle_scale: None,
            subtitle_position: None,
            volume: None,
            muted: None,
            hardware_decoder: None,
        }
    }
}

impl PlayerSnapshot {
    pub fn position_ms(&self) -> i64 {
        if !self.position.is_finite() || self.position < 0.0 {
            return 0;
        }
        (self.position * 1000.0).round() as i64
    }

    pub fn duration_ms(&self) -> i64 {
        if !self.duration.is_finite() || self.duration <= 0.0 {
            return 0;
        }
        (self.duration * 1000.0).round() as i64
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackKind {
    Video,

    Audio,

    Subtitle,
}

impl TrackKind {
    pub fn as_str(self) -> &'static str {
        match self {
            TrackKind::Video => "video",
            TrackKind::Audio => "audio",
            TrackKind::Subtitle => "subtitle",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaTrack {
    pub id: i64,

    pub kind: TrackKind,

    pub title: Option<String>,

    pub language: Option<String>,

    pub codec: Option<String>,

    pub selected: bool,

    pub external: bool,
}

impl MediaTrack {
    pub fn label(&self) -> String {
        let codec = self
            .codec
            .clone()
            .unwrap_or_else(|| self.kind.as_str().to_string());
        match (&self.title, &self.language) {
            (Some(title), Some(language)) => format!("{title} · {language} · {codec}"),
            (Some(title), None) => format!("{title} · {codec}"),
            (None, Some(language)) => format!("{language} · {codec}"),
            (None, None) => codec,
        }
    }
}

pub const NO_TRACK: i64 = 0;

#[derive(Debug, Clone)]
pub enum PlayerCommand {
    Load {
        load_id: u64,

        path: PathBuf,

        resume: Option<f64>,
    },

    Seek {
        seconds: f64,
        serial: u64,
    },

    SetPaused(bool),

    Stop,

    SelectTrack {
        kind: TrackKind,

        id: i64,
    },

    SetSubtitleStyle {
        scale: f64,

        position: f64,
    },

    SetVolume(f64),

    SetMuted(bool),

    SubAdd {
        path: PathBuf,
    },

    Shutdown,
}

pub const SUB_SCALE_RANGE: (f64, f64) = (0.1, 10.0);

pub const SUB_POSITION_RANGE: (f64, f64) = (0.0, 150.0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HwdecMode {
    No,

    VideoToolboxCopy,

    AutoCopy,
}

impl Default for HwdecMode {
    fn default() -> Self {
        if cfg!(target_os = "macos") {
            Self::AutoCopy
        } else {
            Self::No
        }
    }
}

impl HwdecMode {
    fn as_option(self) -> &'static str {
        match self {
            Self::No => "no",
            Self::VideoToolboxCopy => "videotoolbox-copy",
            Self::AutoCopy => "auto-copy",
        }
    }

    fn from_environment() -> Result<Self, PlayerError> {
        match std::env::var("SCREENROOM_HWDEC").as_deref() {
            Ok("no") => Ok(Self::No),
            Ok("videotoolbox-copy") => Ok(Self::VideoToolboxCopy),
            Ok("auto-copy") => Ok(Self::AutoCopy),
            Ok(value) => Err(PlayerError::Argument(format!(
                "unsupported SCREENROOM_HWDEC value {value:?}; use no, videotoolbox-copy, or auto-copy"
            ))),
            Err(std::env::VarError::NotPresent) => Ok(Self::default()),
            Err(error) => Err(PlayerError::Argument(format!(
                "could not read SCREENROOM_HWDEC: {error}"
            ))),
        }
    }
}

pub struct Player {
    shared: Arc<Mutex<PlayerSnapshot>>,
    commands: Sender<PlayerCommand>,
    thread: Option<JoinHandle<()>>,
    wake: Arc<WakeSignal>,
    ticks: crate::FrameTicks,
    next_load_id: std::sync::atomic::AtomicU64,
    next_seek_serial: std::sync::atomic::AtomicU64,
}

impl std::fmt::Debug for Player {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Player").finish_non_exhaustive()
    }
}

impl Player {
    pub fn start() -> Result<Self, PlayerError> {
        Self::start_with_hwdec(HwdecMode::from_environment()?)
    }

    pub fn start_with_hwdec(hwdec: HwdecMode) -> Result<Self, PlayerError> {
        Self::start_with_limit(hwdec, MAX_DIMENSION)
    }

    pub fn start_with_max_dimension(max_dimension: u32) -> Result<Self, PlayerError> {
        Self::start_with_limit(HwdecMode::from_environment()?, max_dimension)
    }

    fn start_with_limit(hwdec: HwdecMode, max_dimension: u32) -> Result<Self, PlayerError> {
        let shared = Arc::new(Mutex::new(PlayerSnapshot::default()));
        let (commands, queue) = mpsc::channel();
        let (ready_sender, ready) = mpsc::channel();
        let wake = Arc::new(WakeSignal::new());
        let thread_wake = Arc::clone(&wake);
        let thread_shared = Arc::clone(&shared);
        let ticks = crate::FrameTicks::new();
        let thread_ticks = ticks.clone();

        let handle = thread::Builder::new()
            .name("screenroom-player".to_string())
            .spawn(move || {
                run(
                    thread_shared,
                    queue,
                    ready_sender,
                    thread_wake,
                    thread_ticks,
                    hwdec,
                    max_dimension,
                )
            })
            .map_err(|error| {
                PlayerError::Unavailable(format!("could not start player: {error}"))
            })?;

        match ready.recv_timeout(Duration::from_secs(10)) {
            Ok(Ok(())) => Ok(Self {
                shared,
                commands,
                thread: Some(handle),
                wake,
                ticks,
                next_load_id: std::sync::atomic::AtomicU64::new(0),
                next_seek_serial: std::sync::atomic::AtomicU64::new(0),
            }),
            Ok(Err(error)) => {
                let _ = handle.join();
                Err(error)
            }
            Err(_) => Err(PlayerError::Unavailable(
                "the player thread did not become ready".to_string(),
            )),
        }
    }

    pub fn snapshot(&self) -> PlayerSnapshot {
        self.shared
            .lock()
            .map(|snapshot| snapshot.clone())
            .unwrap_or_default()
    }

    pub fn wake_notifications(&self) -> u64 {
        self.wake.current()
    }

    pub fn frame_ticks(&self) -> crate::FrameTicks {
        self.ticks.clone()
    }

    pub fn load(&self, path: impl AsRef<Path>) {
        self.load_at(path, None);
    }

    pub fn load_at(&self, path: impl AsRef<Path>, resume: Option<f64>) -> u64 {
        let load_id = self
            .next_load_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            + 1;
        self.send(PlayerCommand::Load {
            load_id,
            path: path.as_ref().to_path_buf(),
            resume,
        });
        load_id
    }

    pub fn seek(&self, seconds: f64) -> u64 {
        let serial = self
            .next_seek_serial
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            + 1;
        self.send(PlayerCommand::Seek { seconds, serial });
        serial
    }

    pub fn set_paused(&self, paused: bool) {
        self.send(PlayerCommand::SetPaused(paused));
    }

    pub fn select_track(&self, kind: TrackKind, id: i64) {
        self.send(PlayerCommand::SelectTrack { kind, id });
    }

    pub fn set_subtitle_style(&self, scale: f64, position: f64) {
        self.send(PlayerCommand::SetSubtitleStyle { scale, position });
    }

    pub fn set_volume(&self, percent: f64) {
        if !percent.is_finite() {
            return;
        }
        self.send(PlayerCommand::SetVolume(percent.clamp(0.0, 100.0)));
    }

    pub fn set_muted(&self, muted: bool) {
        self.send(PlayerCommand::SetMuted(muted));
    }

    pub fn sub_add(&self, path: impl AsRef<Path>) {
        self.send(PlayerCommand::SubAdd {
            path: path.as_ref().to_path_buf(),
        });
    }

    pub fn stop(&self) {
        self.send(PlayerCommand::Stop);
    }

    fn send(&self, command: PlayerCommand) {
        if self.commands.send(command).is_ok() {
            self.wake.notify();
        }
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        let _ = self.commands.send(PlayerCommand::Shutdown);
        self.wake.notify();
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

fn run(
    shared: Arc<Mutex<PlayerSnapshot>>,
    commands: Receiver<PlayerCommand>,
    ready: Sender<Result<(), PlayerError>>,
    wake: Arc<WakeSignal>,
    ticks: crate::FrameTicks,
    hwdec: HwdecMode,
    max_dimension: u32,
) {
    let mut runner = match Runner::new(hwdec, Arc::clone(&wake), ticks, max_dimension) {
        Ok(runner) => {
            let _ = ready.send(Ok(()));
            runner
        }
        Err(error) => {
            let message = error.to_string();
            publish(&shared, |snapshot| {
                snapshot.state = PlayerState::Failed;
                snapshot.failure = Some(PlayerFailure {
                    code: "player_unavailable".to_string(),
                    message,
                });
            });
            let _ = ready.send(Err(error));
            return;
        }
    };
    runner.play_loop(&commands, &shared, &wake);
}

struct Runner {
    renderer: SoftwareRenderer,
    max_dimension: u32,
    core: Mpv,
    library: Arc<LibMpv>,
    state: PlayerState,
    paused: bool,
    position: f64,
    duration: f64,
    loaded: bool,

    tracks: Vec<MediaTrack>,

    audio_pts: Option<f64>,

    avsync: Option<f64>,

    total_avsync_change: Option<f64>,

    mistimed_frames: Option<i64>,

    subtitle_scale: Option<f64>,

    subtitle_position: Option<f64>,

    volume: Option<f64>,

    muted: Option<bool>,
    hardware_decoder: Option<String>,
    load_id: u64,
    position_ready: bool,
    current_entry_id: Option<i64>,
    expected_entry_id: Option<i64>,
    seek_serial: u64,
    pending_seek_serial: Option<u64>,
    queued_seek: Option<(f64, u64)>,
    seek_started: bool,
    video_width: u32,
    video_height: u32,
    last_time_pos: Option<f64>,
    awaiting_position: bool,
    metadata_dirty: bool,
    ticks: crate::FrameTicks,
}

impl Runner {
    fn new(
        hwdec: HwdecMode,
        wake: Arc<WakeSignal>,
        ticks: crate::FrameTicks,
        max_dimension: u32,
    ) -> Result<Self, PlayerError> {
        let library = LibMpv::load()?;

        let mut options = vec![
            ("vo", "libmpv"),
            ("idle", "yes"),
            ("keep-open", "no"),
            ("config", "no"),
            ("terminal", "no"),
            ("osc", "no"),
            ("input-default-bindings", "no"),
            ("input-vo-keyboard", "no"),
            ("hwdec", hwdec.as_option()),
            ("audio-client-name", "Screenroom"),
            ("video-sync", "audio"),
        ];
        if std::env::var_os("SCREENROOM_PLAYER_LOG").is_some() {
            options.push(("terminal", "yes"));
            options.push(("msg-level", "all=debug"));
        }
        let mut core = Mpv::new_with_options(&library, &options)?;
        core.set_wakeup_signal(Arc::clone(&wake));
        let renderer = SoftwareRenderer::new(&core, Arc::clone(&wake))?;

        core.observe("time-pos", FORMAT_DOUBLE, ID_TIME_POS)?;
        core.observe("duration", FORMAT_DOUBLE, ID_DURATION)?;
        core.observe("pause", FORMAT_FLAG, ID_PAUSE)?;
        core.observe("eof-reached", FORMAT_FLAG, ID_EOF_REACHED)?;

        core.observe("audio-pts", FORMAT_DOUBLE, ID_AUDIO_PTS)?;

        core.observe("avsync", FORMAT_DOUBLE, ID_AVSYNC)?;
        core.observe("total-avsync-change", FORMAT_DOUBLE, ID_TOTAL_AVSYNC_CHANGE)?;
        core.observe("mistimed-frame-count", FORMAT_INT64, ID_MISTIMED_FRAMES)?;

        core.observe("volume", FORMAT_DOUBLE, ID_VOLUME)?;
        core.observe("mute", FORMAT_FLAG, ID_MUTE)?;
        core.observe("track-list", FORMAT_NODE, ID_TRACK_LIST)?;
        core.observe("hwdec-current", FORMAT_STRING, ID_HWDEC_CURRENT)?;
        core.observe("dwidth", FORMAT_INT64, ID_DWIDTH)?;
        core.observe("dheight", FORMAT_INT64, ID_DHEIGHT)?;

        Ok(Self {
            renderer,
            max_dimension,
            core,
            library,
            state: PlayerState::Idle,
            paused: false,
            position: 0.0,
            duration: 0.0,
            loaded: false,
            tracks: Vec::new(),
            audio_pts: None,
            avsync: None,
            total_avsync_change: None,
            mistimed_frames: None,
            subtitle_scale: None,
            subtitle_position: None,
            volume: None,
            muted: None,
            hardware_decoder: None,
            load_id: 0,
            position_ready: false,
            current_entry_id: None,
            expected_entry_id: None,
            seek_serial: 0,
            pending_seek_serial: None,
            queued_seek: None,
            seek_started: false,
            video_width: 0,
            video_height: 0,
            last_time_pos: None,
            awaiting_position: false,
            metadata_dirty: false,
            ticks,
        })
    }

    fn play_loop(
        &mut self,
        commands: &Receiver<PlayerCommand>,
        shared: &Arc<Mutex<PlayerSnapshot>>,
        wake: &Arc<WakeSignal>,
    ) {
        loop {
            let observed = wake.current();
            let loop_began = std::time::Instant::now();
            let phase = std::time::Instant::now();
            loop {
                match commands.try_recv() {
                    Ok(PlayerCommand::Shutdown) => return,
                    Ok(command) => self.handle_command(command, shared),
                    Err(TryRecvError::Empty) => break,

                    Err(TryRecvError::Disconnected) => return,
                }
            }
            let command_time = phase.elapsed();
            let phase = std::time::Instant::now();

            let mut event_wait = Duration::ZERO;
            let mut event_handle = Duration::ZERO;
            let mut event_count = 0_u64;
            loop {
                let wait_began = std::time::Instant::now();
                let raw = self.core.wait_event_raw(0.0);
                event_wait += wait_began.elapsed();
                let Some(raw) = raw else { break };
                let event_id = raw.event_id;
                let handle_began = std::time::Instant::now();
                self.handle_event(raw, shared);
                let handled = handle_began.elapsed();
                if handled >= Duration::from_millis(5) {
                    trace!("slow event {event_id} took {handled:?}");
                }
                event_handle += handled;
                event_count += 1;
            }
            let event_time = phase.elapsed();
            let phase = std::time::Instant::now();

            if self.pending_seek_serial.is_none()
                && self.loaded
                && let Some((seconds, serial)) = self.queued_seek.take()
            {
                self.seek(seconds, serial, shared);
            }

            let had_frame = self.renderer.take_dirty();
            if had_frame {
                self.present(shared);
            }
            if !had_frame && self.metadata_dirty {
                self.refresh_metadata(shared);
            }
            let render_time = phase.elapsed();
            let phase = std::time::Instant::now();
            wake.wait_after(observed);
            let wait_time = phase.elapsed();
            let total = loop_began.elapsed();
            if total >= Duration::from_millis(40) {
                trace!(
                    "slow loop {total:?}: commands {command_time:?}, events {event_time:?} ({event_count} events: wait {event_wait:?}, handle {event_handle:?}), render {render_time:?}, wait {wait_time:?}"
                );
            }
        }
    }

    fn handle_command(&mut self, command: PlayerCommand, shared: &Arc<Mutex<PlayerSnapshot>>) {
        match command {
            PlayerCommand::Load {
                load_id,
                path,
                resume,
            } => self.load(load_id, &path, resume, shared),
            PlayerCommand::Seek { seconds, serial } => {
                if self.pending_seek_serial.is_some() {
                    self.queued_seek = Some((seconds, serial));
                } else {
                    self.seek(seconds, serial, shared);
                }
            }
            PlayerCommand::SetPaused(paused) => self.set_paused(paused, shared),
            PlayerCommand::SelectTrack { kind, id } => self.select_track(kind, id, shared),
            PlayerCommand::SetSubtitleStyle { scale, position } => {
                self.set_subtitle_style(scale, position, shared)
            }
            PlayerCommand::SetVolume(percent) => self.set_volume(percent, shared),
            PlayerCommand::SetMuted(muted) => self.set_muted(muted, shared),
            PlayerCommand::SubAdd { path } => self.sub_add(&path, shared),
            PlayerCommand::Stop => self.stop(shared),
            PlayerCommand::Shutdown => {}
        }
    }

    fn load(
        &mut self,
        load_id: u64,
        path: &Path,
        resume: Option<f64>,
        shared: &Arc<Mutex<PlayerSnapshot>>,
    ) {
        self.load_id = load_id;
        self.position_ready = false;
        self.current_entry_id = None;
        self.expected_entry_id = None;
        self.pending_seek_serial = None;
        self.queued_seek = None;
        self.seek_started = false;
        self.position = 0.0;
        self.duration = 0.0;
        self.loaded = false;
        self.hardware_decoder = None;
        self.tracks.clear();
        self.video_width = 0;
        self.video_height = 0;
        self.last_time_pos = None;
        self.awaiting_position = false;
        self.metadata_dirty = false;
        self.audio_pts = None;
        self.avsync = None;
        self.total_avsync_change = None;
        self.mistimed_frames = None;
        let load_id_for_snapshot = self.load_id;
        publish(shared, |snapshot| {
            snapshot.load_id = load_id_for_snapshot;
            snapshot.position_ready = false;
            snapshot.loaded = false;
            snapshot.position = 0.0;
            snapshot.duration = 0.0;
            snapshot.tracks.clear();
            snapshot.audio_pts = None;
            snapshot.avsync = None;
            snapshot.total_avsync_change = None;
            snapshot.mistimed_frames = None;
            snapshot.hardware_decoder = None;
            snapshot.failure = None;
            snapshot.ended = false;
            snapshot.frame = None;
        });

        if !path.is_file() {
            self.fail(
                "file_missing",
                format!("{} is no longer on disk.", path.display()),
                shared,
            );
            return;
        }
        let Some(path) = path.to_str() else {
            self.fail(
                "invalid_path",
                "The file path is not valid UTF-8.".to_string(),
                shared,
            );
            return;
        };
        let resume = resume.filter(|seconds| seconds.is_finite() && *seconds > 0.5);

        self.state = PlayerState::Idle;
        self.set_state(PlayerState::Resolving, shared);
        let command = if let Some(position) = resume {
            let options = format!("start={position:.3}");
            self.core
                .command(&["loadfile", path, "replace", "-1", &options])
        } else {
            self.core.command(&["loadfile", path, "replace"])
        };
        if let Err(error) = command {
            self.fail("load_failed", error.to_string(), shared);
            return;
        }
        self.expected_entry_id = self.core.get_i64("playlist/0/id").ok();
    }

    fn seek(&mut self, seconds: f64, serial: u64, shared: &Arc<Mutex<PlayerSnapshot>>) {
        if !seconds.is_finite() || seconds < 0.0 {
            self.confirm_seek(serial, shared);
            return;
        }
        if self.loaded && self.current_entry_id.is_none() {
            self.confirm_seek(serial, shared);
            return;
        }
        if !self.loaded {
            self.queued_seek = Some((seconds, serial));
            return;
        }
        let target = if self.duration > 0.0 {
            seconds.min(self.duration)
        } else {
            seconds
        };
        self.pending_seek_serial = None;
        self.seek_started = false;
        if let Err(error) = self
            .core
            .command(&["seek", &format!("{target:.3}"), "absolute"])
        {
            self.queued_seek = None;
            self.fail("seek_failed", error.to_string(), shared);
            self.confirm_seek(serial, shared);
        } else {
            self.pending_seek_serial = Some(serial);
        }
    }

    fn confirm_seek(&mut self, serial: u64, shared: &Arc<Mutex<PlayerSnapshot>>) {
        self.seek_serial = self.seek_serial.max(serial);
        self.publish_snapshot(shared);
    }

    fn set_paused(&mut self, paused: bool, shared: &Arc<Mutex<PlayerSnapshot>>) {
        let value = if paused { "yes" } else { "no" };
        if let Err(error) = self.core.command(&["set", "pause", value]) {
            self.fail("pause_failed", error.to_string(), shared);
        } else {
            self.paused = paused;
        }
    }

    fn set_subtitle_style(
        &mut self,
        scale: f64,
        position: f64,
        shared: &Arc<Mutex<PlayerSnapshot>>,
    ) {
        for (property, value, (low, high)) in [
            ("sub-scale", scale, SUB_SCALE_RANGE),
            ("sub-pos", position, SUB_POSITION_RANGE),
        ] {
            if !value.is_finite() {
                continue;
            }
            let value = value.clamp(low, high).to_string();
            if let Err(error) = self.core.command(&["set", property, &value]) {
                self.fail("subtitle_style_failed", error.to_string(), shared);
                return;
            }
        }

        self.subtitle_scale = self
            .core
            .get_f64("sub-scale")
            .ok()
            .filter(|v| v.is_finite());
        self.subtitle_position = self.core.get_f64("sub-pos").ok().filter(|v| v.is_finite());
        self.publish_snapshot(shared);
    }

    fn select_track(&mut self, kind: TrackKind, id: i64, shared: &Arc<Mutex<PlayerSnapshot>>) {
        if !self.loaded {
            return;
        }
        let property = match kind {
            TrackKind::Video => "vid",
            TrackKind::Audio => "aid",
            TrackKind::Subtitle => "sid",
        };
        let value = if id == NO_TRACK {
            "no".to_string()
        } else {
            id.to_string()
        };
        if let Err(error) = self.core.command(&["set", property, &value]) {
            self.fail("track_failed", error.to_string(), shared);
            return;
        }
        self.tracks = read_tracks(&self.core);
        let tracks = self.tracks.clone();
        publish(shared, |snapshot| snapshot.tracks = tracks);
    }

    fn sub_add(&mut self, path: &Path, shared: &Arc<Mutex<PlayerSnapshot>>) {
        if !self.loaded {
            return;
        }
        let address = path.to_string_lossy().to_string();
        if let Err(error) = self.core.command(&["sub-add", &address]) {
            self.fail("subtitle_add_failed", error.to_string(), shared);
            return;
        }
        self.tracks = read_tracks(&self.core);
        let tracks = self.tracks.clone();
        publish(shared, |snapshot| snapshot.tracks = tracks);
    }

    fn stop(&mut self, shared: &Arc<Mutex<PlayerSnapshot>>) {
        let _ = self.core.command(&["stop"]);
        self.state = PlayerState::Idle;
        self.loaded = false;
        self.position_ready = false;
        self.pending_seek_serial = None;
        self.queued_seek = None;
        self.seek_started = false;
        self.position = 0.0;
        self.duration = 0.0;
        self.tracks.clear();
        self.audio_pts = None;
        self.avsync = None;
        self.total_avsync_change = None;
        self.mistimed_frames = None;
        self.hardware_decoder = None;
        self.video_width = 0;
        self.video_height = 0;
        self.last_time_pos = None;
        self.awaiting_position = false;
        self.metadata_dirty = false;
        publish(shared, |snapshot| {
            snapshot.load_id = self.load_id;
            snapshot.position_ready = false;
            snapshot.state = PlayerState::Idle;
            snapshot.loaded = false;
            snapshot.position = 0.0;
            snapshot.duration = 0.0;
            snapshot.ended = false;
            snapshot.frame = None;
            snapshot.tracks = Vec::new();
            snapshot.audio_pts = None;
            snapshot.avsync = None;
            snapshot.total_avsync_change = None;
            snapshot.mistimed_frames = None;
            snapshot.hardware_decoder = None;
        });
    }

    fn handle_event(&mut self, raw: crate::RawEvent, shared: &Arc<Mutex<PlayerSnapshot>>) {
        match raw.event_id {
            event::PROPERTY_CHANGE => self.apply_property(raw, shared),
            event::START_FILE => {
                if !raw.data.is_null() {
                    let entry_id =
                        unsafe { (*(raw.data as *const RawStartFile)).playlist_entry_id };
                    if self
                        .expected_entry_id
                        .is_some_and(|expected| expected != entry_id)
                    {
                        return;
                    }
                    self.expected_entry_id = None;
                    self.current_entry_id = Some(entry_id);
                    self.position_ready = false;
                    self.last_time_pos = None;
                    self.set_state(PlayerState::Resolving, shared);
                }
            }
            event::FILE_LOADED if self.current_entry_id.is_some() => self.on_loaded(shared),
            event::END_FILE => self.on_end_file(raw, shared),
            event::SEEK => {
                if self.pending_seek_serial.is_some() {
                    self.seek_started = true;
                }
            }
            event::PLAYBACK_RESTART if self.loaded => {
                match self.last_time_pos {
                    Some(position) => {
                        self.position = position;
                        self.position_ready = true;
                        self.awaiting_position = false;
                    }
                    None => self.awaiting_position = true,
                }
                if self.seek_started {
                    if let Some(serial) = self.pending_seek_serial.take() {
                        self.seek_serial = serial;
                    }
                    self.seek_started = false;
                }
                self.publish_snapshot(shared);
            }
            event::VIDEO_RECONFIG if self.loaded => self.update_target(),
            event::SHUTDOWN => {}
            _ => {}
        }
    }

    fn apply_property(&mut self, raw: crate::RawEvent, shared: &Arc<Mutex<PlayerSnapshot>>) {
        if self.current_entry_id.is_none() && !matches!(raw.reply_userdata, ID_VOLUME | ID_MUTE) {
            return;
        }
        if !self.loaded
            && !matches!(raw.reply_userdata, ID_PAUSE | ID_VOLUME | ID_MUTE)
            && !is_load_time_property(raw.reply_userdata)
        {
            return;
        }
        if raw.data.is_null() {
            return;
        }

        let property = unsafe { &*(raw.data as *const RawProperty) };
        trace!(
            "property id={} present={}",
            raw.reply_userdata,
            !property.data.is_null()
        );
        if property.data.is_null() {
            if raw.reply_userdata == ID_HWDEC_CURRENT {
                self.hardware_decoder = None;
                self.publish_snapshot(shared);
            }
            return;
        }
        match raw.reply_userdata {
            ID_TIME_POS => {
                let value = unsafe { *(property.data as *const f64) };
                if value.is_finite() && value >= 0.0 {
                    self.last_time_pos = Some(value);
                    if self.awaiting_position {
                        self.position = value;
                        self.position_ready = true;
                        self.awaiting_position = false;
                    }
                    if self.position_ready {
                        self.position = value;
                    }
                }
            }
            ID_DWIDTH | ID_DHEIGHT => {
                let value = unsafe { *(property.data as *const i64) }.max(0) as u32;
                if raw.reply_userdata == ID_DWIDTH {
                    self.video_width = value;
                } else {
                    self.video_height = value;
                }
                self.update_target();
            }
            ID_DURATION => {
                let value = unsafe { *(property.data as *const f64) };
                if value.is_finite() && value > 0.0 {
                    self.duration = value;
                }
            }
            ID_PAUSE => {
                let paused = unsafe { *(property.data as *const i32) } != 0;
                self.paused = paused;
                if self.loaded {
                    self.set_state(
                        if paused {
                            PlayerState::Paused
                        } else {
                            PlayerState::Playing
                        },
                        shared,
                    );
                }
            }
            ID_EOF_REACHED => {
                let ended = unsafe { *(property.data as *const i32) } != 0;
                if ended {
                    self.set_state(PlayerState::Ended, shared);
                    publish(shared, |snapshot| snapshot.ended = true);
                }
            }
            ID_AUDIO_PTS => {
                let value = unsafe { *(property.data as *const f64) };

                self.audio_pts = value.is_finite().then_some(value);
            }
            ID_AVSYNC => {
                let value = unsafe { *(property.data as *const f64) };
                self.avsync = value.is_finite().then_some(value);
            }
            ID_TOTAL_AVSYNC_CHANGE => {
                let value = unsafe { *(property.data as *const f64) };
                self.total_avsync_change = value.is_finite().then_some(value);
            }
            ID_MISTIMED_FRAMES => {
                let value = unsafe { *(property.data as *const i64) };
                self.mistimed_frames = Some(value);
            }
            ID_VOLUME => {
                let value = unsafe { *(property.data as *const f64) };
                self.volume = value.is_finite().then_some(value);
            }
            ID_MUTE => {
                self.muted = Some(unsafe { *(property.data as *const i32) } != 0);
            }
            ID_TRACK_LIST => self.metadata_dirty = true,
            ID_HWDEC_CURRENT => {
                let value = unsafe { *(property.data.cast::<*const std::ffi::c_char>()) };
                self.hardware_decoder = (!value.is_null()).then(|| {
                    unsafe { std::ffi::CStr::from_ptr(value) }
                        .to_string_lossy()
                        .into_owned()
                });
            }
            _ => {}
        }
        self.publish_snapshot(shared);
    }

    fn set_volume(&mut self, percent: f64, shared: &Arc<Mutex<PlayerSnapshot>>) {
        let value = percent.clamp(0.0, 100.0).to_string();
        if let Err(error) = self.core.command(&["set", "volume", &value]) {
            self.fail("volume_failed", error.to_string(), shared);
            return;
        }
        self.volume = self.core.get_f64("volume").ok().filter(|v| v.is_finite());
        self.publish_snapshot(shared);
    }

    fn set_muted(&mut self, muted: bool, shared: &Arc<Mutex<PlayerSnapshot>>) {
        let value = if muted { "yes" } else { "no" };
        if let Err(error) = self.core.command(&["set", "mute", value]) {
            self.fail("mute_failed", error.to_string(), shared);
            return;
        }
        self.muted = self.core.get_flag("mute").ok();
        self.publish_snapshot(shared);
    }

    fn on_loaded(&mut self, shared: &Arc<Mutex<PlayerSnapshot>>) {
        trace!("event: file loaded");
        self.loaded = true;
        self.metadata_dirty = true;
        self.update_target();

        self.set_state(
            if self.paused {
                PlayerState::Paused
            } else {
                PlayerState::Playing
            },
            shared,
        );
        self.publish_snapshot(shared);
    }

    fn refresh_metadata(&mut self, shared: &Arc<Mutex<PlayerSnapshot>>) {
        self.metadata_dirty = false;
        self.tracks = read_tracks(&self.core);
        if self.duration <= 0.0
            && let Ok(duration) = self.core.get_f64("duration")
            && duration.is_finite()
            && duration > 0.0
        {
            self.duration = duration;
        }
        if self.hardware_decoder.is_none() {
            self.hardware_decoder = self.core.get_string("hwdec-current").ok().flatten();
        }
        self.publish_snapshot(shared);
    }

    fn update_target(&mut self) {
        if self.video_width == 0 || self.video_height == 0 {
            return;
        }
        let (width, height) = (self.video_width, self.video_height);
        let limit = std::env::var("SCREENROOM_RENDER_MAX")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(self.max_dimension);
        let (width, height) = target_size(width, height, limit);
        self.renderer.set_target(width, height);
    }

    fn on_end_file(&mut self, raw: crate::RawEvent, shared: &Arc<Mutex<PlayerSnapshot>>) {
        let (reason, error, playlist_entry_id) = if raw.data.is_null() {
            (
                end_reason::EOF,
                0,
                self.current_entry_id.unwrap_or_default(),
            )
        } else {
            let end = unsafe { &*(raw.data as *const RawEndFile) };
            (end.reason, end.error, end.playlist_entry_id)
        };
        if self.current_entry_id != Some(playlist_entry_id) {
            return;
        }
        self.current_entry_id = None;

        let outstanding = self
            .pending_seek_serial
            .take()
            .into_iter()
            .chain(self.queued_seek.take().map(|(_, serial)| serial))
            .max();
        self.seek_started = false;
        if let Some(serial) = outstanding {
            self.seek_serial = self.seek_serial.max(serial);
        }
        self.hardware_decoder = None;
        match reason {
            end_reason::EOF => {
                self.set_state(PlayerState::Ended, shared);
                publish(shared, |snapshot| snapshot.ended = true);
            }
            end_reason::ERROR => {
                let message = self.library.message(error);
                self.fail("decode_failed", message, shared);
            }

            _ => {}
        }
        self.publish_snapshot(shared);
    }

    fn present(&mut self, shared: &Arc<Mutex<PlayerSnapshot>>) {
        if !self.loaded || self.current_entry_id.is_none() {
            return;
        }
        trace!("present: checking update flags");
        let flags = match self.renderer.update_flags() {
            Ok(flags) => flags,
            Err(error) => {
                self.fail("render_failed", error.to_string(), shared);
                return;
            }
        };
        if flags & UPDATE_FRAME == 0 {
            trace!("present: flags {flags} carry no frame");
            return;
        }
        trace!("present: rendering frame");
        let began = std::time::Instant::now();
        match self.renderer.render() {
            Ok(Some(frame)) => {
                trace!(
                    "present: frame {}x{} gen {} in {:?}",
                    frame.width,
                    frame.height,
                    frame.generation,
                    began.elapsed()
                );
                let load_id = self.load_id;
                publish(shared, |snapshot| {
                    snapshot.load_id = load_id;
                    snapshot.frame = Some(frame);
                });
                self.ticks.bump();
            }
            Ok(None) => {}
            Err(error) => self.fail("render_failed", error.to_string(), shared),
        }
    }

    fn set_state(&mut self, next: PlayerState, shared: &Arc<Mutex<PlayerSnapshot>>) {
        let previous = self.state;
        self.state = self.state.transition(next);
        if self.state != previous {
            let (load_id, position_ready, state) = (self.load_id, self.position_ready, self.state);
            publish(shared, |snapshot| {
                snapshot.load_id = load_id;
                snapshot.position_ready = position_ready;
                snapshot.state = state;
            });
        }
    }

    fn fail(&mut self, code: &str, message: String, shared: &Arc<Mutex<PlayerSnapshot>>) {
        self.state = PlayerState::Failed;
        let (load_id, position_ready, position, duration, loaded) = (
            self.load_id,
            self.position_ready,
            self.position,
            self.duration,
            self.loaded,
        );
        publish(shared, |snapshot| {
            snapshot.load_id = load_id;
            snapshot.position_ready = position_ready;
            snapshot.position = if position_ready { position } else { 0.0 };
            snapshot.duration = duration;
            snapshot.loaded = loaded;
            snapshot.state = PlayerState::Failed;
            snapshot.failure = Some(PlayerFailure {
                code: code.to_string(),
                message,
            });
        });
    }

    fn publish_snapshot(&self, shared: &Arc<Mutex<PlayerSnapshot>>) {
        let (position, duration, state, loaded) =
            (self.position, self.duration, self.state, self.loaded);
        let (tracks, audio_pts) = (self.tracks.clone(), self.audio_pts);
        let (avsync, total_avsync_change, mistimed_frames) =
            (self.avsync, self.total_avsync_change, self.mistimed_frames);
        let (subtitle_scale, subtitle_position) = (self.subtitle_scale, self.subtitle_position);
        let (volume, muted) = (self.volume, self.muted);
        let hardware_decoder = self.hardware_decoder.clone();
        let (load_id, position_ready, seek_serial) =
            (self.load_id, self.position_ready, self.seek_serial);
        publish(shared, |snapshot| {
            snapshot.load_id = load_id;
            snapshot.position_ready = position_ready;
            snapshot.seek_serial = seek_serial;
            snapshot.position = if position_ready { position } else { 0.0 };
            snapshot.duration = duration;
            snapshot.state = state;
            snapshot.loaded = loaded;
            snapshot.tracks = tracks;
            snapshot.audio_pts = audio_pts;
            snapshot.avsync = avsync;
            snapshot.total_avsync_change = total_avsync_change;
            snapshot.mistimed_frames = mistimed_frames;
            snapshot.subtitle_scale = subtitle_scale;
            snapshot.subtitle_position = subtitle_position;
            snapshot.volume = volume;
            snapshot.muted = muted;
            snapshot.hardware_decoder = hardware_decoder;
        });
    }
}

fn read_tracks(core: &Mpv) -> Vec<MediaTrack> {
    let count = core.get_i64("track-list/count").unwrap_or(0).clamp(0, 64);
    let mut tracks = Vec::new();
    for index in 0..count {
        let field = |name: &str| {
            core.get_string(&format!("track-list/{index}/{name}"))
                .ok()
                .flatten()
        };
        let kind = match field("type").as_deref() {
            Some("video") => TrackKind::Video,
            Some("audio") => TrackKind::Audio,
            Some("sub") => TrackKind::Subtitle,

            _ => continue,
        };

        if kind == TrackKind::Video && field("image").as_deref() == Some("yes") {
            continue;
        }
        let Some(id) = core.get_i64(&format!("track-list/{index}/id")).ok() else {
            continue;
        };
        tracks.push(MediaTrack {
            id,
            kind,
            title: field("title").filter(|value| !value.trim().is_empty()),
            language: field("lang").filter(|value| !value.trim().is_empty()),
            codec: field("codec").filter(|value| !value.trim().is_empty()),
            selected: core
                .get_flag(&format!("track-list/{index}/selected"))
                .unwrap_or(false),
            external: core
                .get_flag(&format!("track-list/{index}/external"))
                .unwrap_or(false),
        });
    }
    tracks
}

fn publish(shared: &Arc<Mutex<PlayerSnapshot>>, update: impl FnOnce(&mut PlayerSnapshot)) {
    if let Ok(mut snapshot) = shared.lock() {
        update(&mut snapshot);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_converts_seconds_to_milliseconds() {
        let snapshot = PlayerSnapshot {
            position: 92.4,
            duration: 3600.0,
            ..PlayerSnapshot::default()
        };
        assert_eq!(snapshot.position_ms(), 92_400);
        assert_eq!(snapshot.duration_ms(), 3_600_000);
    }

    #[test]
    fn nonsense_positions_become_zero_not_negative_progress() {
        let cases = [f64::NAN, f64::INFINITY, -5.0];
        for position in cases {
            let snapshot = PlayerSnapshot {
                position,
                duration: 0.0,
                ..PlayerSnapshot::default()
            };
            assert_eq!(snapshot.position_ms(), 0);
            assert_eq!(snapshot.duration_ms(), 0);
        }
    }

    #[test]
    fn a_missing_file_is_reported_without_touching_libmpv() {
        let shared = Arc::new(Mutex::new(PlayerSnapshot::default()));
        publish(&shared, |snapshot| {
            snapshot.state = PlayerState::Failed;
            snapshot.failure = Some(PlayerFailure {
                code: "file_missing".into(),
                message: "gone".into(),
            });
        });
        let snapshot = shared.lock().unwrap().clone();
        assert_eq!(snapshot.state, PlayerState::Failed);
        assert_eq!(snapshot.failure.unwrap().code, "file_missing");
    }
}
