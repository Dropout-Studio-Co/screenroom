use std::sync::Mutex;

pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

pub const CHECK_INTERVAL_SECONDS: u64 = 86_400;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    NotDownloaded,
    Downloaded,
    Installing,
}

impl Stage {
    fn from_name(name: &str) -> Option<Self> {
        match name {
            "not_downloaded" => Some(Self::NotDownloaded),
            "downloaded" => Some(Self::Downloaded),
            "installing" => Some(Self::Installing),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvailableUpdate {
    pub version: String,
    pub display_version: String,
    pub notes: String,
    pub critical: bool,
    pub informational: bool,
    pub feed_signing: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UpdateSettings {
    pub automatic_checks: bool,
    pub automatic_downloads: bool,
    pub allows_automatic_updates: bool,
}

impl Default for UpdateSettings {
    fn default() -> Self {
        Self {
            automatic_checks: true,
            automatic_downloads: true,
            allows_automatic_updates: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum UpdateStatus {
    Unavailable { reason: String },
    Idle,
    Checking,
    UpToDate,
    Available,
    Downloading { received: u64, expected: u64 },
    Extracting { progress: f64 },
    Ready { deferred_to_quit: bool },
    Installing,
    Installed,
    Failed { message: String },
}

impl UpdateStatus {
    pub fn is_busy(&self) -> bool {
        matches!(
            self,
            Self::Checking | Self::Downloading { .. } | Self::Extracting { .. } | Self::Installing
        )
    }

    pub fn is_ready(&self) -> bool {
        matches!(self, Self::Ready { .. })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    Settings {
        automatic_checks: bool,
        automatic_downloads: bool,
        allows_automatic_updates: bool,
    },
    Checking,
    Found {
        update: AvailableUpdate,
        stage: Stage,
        user_initiated: bool,
    },
    Downloading {
        received: u64,
        expected: u64,
    },
    Extracting {
        progress: f64,
    },
    Ready {
        update: AvailableUpdate,
        deferred_to_quit: bool,
    },
    Deferred,
    Installing,
    Installed {
        relaunched: bool,
    },
    UpToDate {
        user_initiated: bool,
    },
    Failed {
        message: String,
    },
    Dismissed,
    Scheduled {
        delay: f64,
    },
    InstallOnQuit,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    CheckNow,
    InstallNow,
    InstallOnQuit,
    CancelDownload,
    Dismiss,
    SetAutomaticChecks(bool),
    SetAutomaticDownloads(bool),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Playback {
    Idle,
    Playing,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Updater {
    status: UpdateStatus,
    update: Option<AvailableUpdate>,
    settings: UpdateSettings,
    scheduled_delay: Option<f64>,
    last_failure: Option<String>,
    quit_install_ready: bool,
}

impl Default for Updater {
    fn default() -> Self {
        Self::new()
    }
}

impl Updater {
    pub fn new() -> Self {
        Self {
            status: UpdateStatus::Idle,
            update: None,
            settings: UpdateSettings::default(),
            scheduled_delay: None,
            last_failure: None,
            quit_install_ready: false,
        }
    }

    pub fn unavailable(reason: impl Into<String>) -> Self {
        Self {
            status: UpdateStatus::Unavailable {
                reason: reason.into(),
            },
            ..Self::new()
        }
    }

    pub fn status(&self) -> &UpdateStatus {
        &self.status
    }

    pub fn update(&self) -> Option<&AvailableUpdate> {
        self.update.as_ref()
    }

    pub fn settings(&self) -> UpdateSettings {
        self.settings
    }

    pub fn scheduled_delay(&self) -> Option<f64> {
        self.scheduled_delay
    }

    pub fn last_failure(&self) -> Option<&str> {
        self.last_failure.as_deref()
    }

    pub fn quit_install_ready(&self) -> bool {
        self.quit_install_ready
    }

    pub fn is_unavailable(&self) -> bool {
        matches!(self.status, UpdateStatus::Unavailable { .. })
    }

    pub fn headline(&self) -> Option<String> {
        match &self.status {
            UpdateStatus::Unavailable { .. } | UpdateStatus::Idle => None,
            UpdateStatus::Checking => Some("Checking for a new version…".to_string()),
            UpdateStatus::UpToDate => Some(format!(
                "Screenroom {CURRENT_VERSION} is the newest version."
            )),
            UpdateStatus::Available => self
                .update
                .as_ref()
                .map(|update| format!("Screenroom {} is available.", update.display_version)),
            UpdateStatus::Downloading { received, expected } => {
                Some(download_label(*received, *expected))
            }
            UpdateStatus::Extracting { progress } => {
                Some(format!("Preparing the update… {}%", percent(*progress)))
            }
            UpdateStatus::Ready {
                deferred_to_quit: true,
            } => self.update.as_ref().map(|update| {
                format!(
                    "Screenroom {} installs when you quit, or now if you ask.",
                    update.display_version
                )
            }),
            UpdateStatus::Ready { .. } => self.update.as_ref().map(|update| {
                format!("Screenroom {} is ready to install.", update.display_version)
            }),
            UpdateStatus::Installing => {
                Some("Installing the update; Screenroom is closing.".to_string())
            }
            UpdateStatus::Installed => {
                Some("The update installed. The next launch uses it.".to_string())
            }
            UpdateStatus::Failed { message } => Some(message.clone()),
        }
    }

    pub fn primary_action(&self) -> Option<Action> {
        match &self.status {
            UpdateStatus::Available | UpdateStatus::Ready { .. } => Some(Action::InstallNow),
            UpdateStatus::Downloading { .. } => Some(Action::CancelDownload),
            UpdateStatus::Unavailable { .. } | UpdateStatus::Installing => None,
            _ => Some(Action::CheckNow),
        }
    }

    pub fn request_dismiss(&mut self) -> Vec<Action> {
        if self.update.is_none() {
            return Vec::new();
        }
        Vec::from([Action::Dismiss])
    }

    pub fn may_install(&self, playback: Playback) -> bool {
        if playback == Playback::Playing {
            return false;
        }
        matches!(
            self.status,
            UpdateStatus::Available | UpdateStatus::Ready { .. }
        )
    }

    pub fn request_check(&mut self, playback: Playback) -> Vec<Action> {
        if self.is_unavailable() || self.status.is_busy() {
            return Vec::new();
        }
        if playback == Playback::Playing {
            return Vec::new();
        }
        Vec::from([Action::CheckNow])
    }

    pub fn request_install(&mut self, playback: Playback) -> Vec<Action> {
        if !self.may_install(playback) {
            return Vec::new();
        }
        Vec::from([Action::InstallNow])
    }

    pub fn request_install_on_quit(&mut self, playback: Playback) -> Vec<Action> {
        if playback == Playback::Playing || !self.status.is_ready() {
            return Vec::new();
        }
        Vec::from([Action::InstallOnQuit])
    }

    pub fn request_automatic_checks(&mut self, enabled: bool) -> Vec<Action> {
        if self.is_unavailable() {
            return Vec::new();
        }
        Vec::from([Action::SetAutomaticChecks(enabled)])
    }

    pub fn request_automatic_downloads(&mut self, enabled: bool) -> Vec<Action> {
        if self.is_unavailable() {
            return Vec::new();
        }
        Vec::from([Action::SetAutomaticDownloads(enabled)])
    }

    pub fn feed(&mut self, event: Event) -> Vec<Action> {
        match event {
            Event::Settings {
                automatic_checks,
                automatic_downloads,
                allows_automatic_updates,
            } => {
                self.settings = UpdateSettings {
                    automatic_checks,
                    automatic_downloads,
                    allows_automatic_updates,
                };
            }
            Event::Checking => {
                self.status = UpdateStatus::Checking;
                self.last_failure = None;
            }
            Event::Found {
                update,
                stage,
                user_initiated,
            } => {
                let version_is_newer = compare_versions(&update.version, CURRENT_VERSION) > 0;
                if !version_is_newer && !user_initiated {
                    self.status = UpdateStatus::UpToDate;
                    return Vec::new();
                }
                self.update = Some(update);
                self.status = if stage == Stage::Installing {
                    UpdateStatus::Installing
                } else {
                    UpdateStatus::Available
                };
                self.last_failure = None;
            }
            Event::Downloading { received, expected } => {
                if self.update.is_none() {
                    return Vec::new();
                }
                self.status = UpdateStatus::Downloading { received, expected };
            }
            Event::Extracting { progress } => {
                self.status = UpdateStatus::Extracting {
                    progress: progress.clamp(0.0, 1.0),
                };
            }
            Event::Ready {
                update,
                deferred_to_quit,
            } => {
                let version_is_newer = compare_versions(&update.version, CURRENT_VERSION) > 0;
                if !version_is_newer {
                    self.status = UpdateStatus::UpToDate;
                    return Vec::new();
                }
                self.update = Some(update);
                self.status = UpdateStatus::Ready { deferred_to_quit };
                self.last_failure = None;
            }
            Event::Deferred => {
                if self.status.is_ready() {
                    self.status = UpdateStatus::Ready {
                        deferred_to_quit: true,
                    };
                }
            }
            Event::Installing => {
                self.status = UpdateStatus::Installing;
            }
            Event::Installed { relaunched } => {
                self.status = if relaunched {
                    UpdateStatus::Idle
                } else {
                    UpdateStatus::Installed
                };
            }
            Event::UpToDate { user_initiated } => {
                self.update = None;
                self.status = if user_initiated {
                    UpdateStatus::UpToDate
                } else {
                    UpdateStatus::Idle
                };
            }
            Event::Failed { message } => {
                self.last_failure = Some(message.clone());
                self.status = UpdateStatus::Failed { message };
            }
            Event::Dismissed | Event::Unknown => {}
            Event::Scheduled { delay } => {
                self.scheduled_delay = Some(delay);
            }
            Event::InstallOnQuit => {
                self.quit_install_ready = true;
            }
        }
        Vec::new()
    }

    pub fn decode(document: &str) -> Option<Event> {
        let value: serde_json::Value = serde_json::from_str(document).ok()?;
        let name = value.get("event")?.as_str()?;
        let text = |key: &str| {
            value
                .get(key)
                .and_then(|item| item.as_str())
                .unwrap_or_default()
                .to_string()
        };
        let flag = |key: &str| {
            value
                .get(key)
                .and_then(|item| item.as_bool())
                .unwrap_or(false)
        };
        let number = |key: &str| value.get(key).and_then(|item| item.as_u64()).unwrap_or(0);
        let update = || AvailableUpdate {
            version: text("version"),
            display_version: {
                let display = text("display_version");
                if display.is_empty() {
                    text("version")
                } else {
                    display
                }
            },
            notes: text("notes"),
            critical: flag("critical"),
            informational: flag("informational"),
            feed_signing: text("feed_signing"),
        };
        let event = match name {
            "settings" => Event::Settings {
                automatic_checks: flag("automatic_checks"),
                automatic_downloads: flag("automatic_downloads"),
                allows_automatic_updates: flag("allows_automatic_updates"),
            },
            "checking" => Event::Checking,
            "found" => Event::Found {
                update: update(),
                stage: Stage::from_name(&text("stage")).unwrap_or(Stage::NotDownloaded),
                user_initiated: flag("user_initiated"),
            },
            "downloading" => Event::Downloading {
                received: number("received"),
                expected: number("expected"),
            },
            "extracting" => Event::Extracting {
                progress: value
                    .get("progress")
                    .and_then(|item| item.as_f64())
                    .unwrap_or(0.0),
            },
            "ready" => Event::Ready {
                update: update(),
                deferred_to_quit: false,
            },
            "deferred" => Event::Deferred,
            "installing" => Event::Installing,
            "installed" => Event::Installed {
                relaunched: flag("relaunched"),
            },
            "up_to_date" => Event::UpToDate {
                user_initiated: flag("user_initiated"),
            },
            "failed" => Event::Failed {
                message: {
                    let message = text("message");
                    if message.is_empty() {
                        "The update failed without a description.".to_string()
                    } else {
                        message
                    }
                },
            },
            "dismissed" => Event::Dismissed,
            "scheduled" => Event::Scheduled {
                delay: value
                    .get("delay")
                    .and_then(|item| item.as_f64())
                    .unwrap_or(0.0),
            },
            "install_on_quit" => Event::InstallOnQuit,
            "idle" | "unscheduled" | "quit_install_started" => Event::Unknown,
            _ => Event::Unknown,
        };
        Some(event)
    }
}

pub fn compare_versions(left: &str, right: &str) -> i32 {
    let parse = |value: &str| -> Vec<u64> {
        value
            .split(['.', '-', '+'])
            .take(4)
            .map(|part| {
                part.chars()
                    .take_while(|character| character.is_ascii_digit())
                    .collect::<String>()
                    .parse::<u64>()
                    .unwrap_or(0)
            })
            .collect()
    };
    let left = parse(left);
    let right = parse(right);
    let length = left.len().max(right.len());
    for index in 0..length {
        let a = left.get(index).copied().unwrap_or(0);
        let b = right.get(index).copied().unwrap_or(0);
        if a != b {
            return if a > b { 1 } else { -1 };
        }
    }
    0
}

fn percent(progress: f64) -> u64 {
    (progress.clamp(0.0, 1.0) * 100.0).round() as u64
}

fn download_label(received: u64, expected: u64) -> String {
    if expected == 0 {
        return format!("Downloading the update… {} MB", received / (1024 * 1024));
    }
    let progress = (received.min(expected) as f64) / (expected as f64);
    format!(
        "Downloading the update… {}% of {} MB",
        percent(progress),
        expected.max(1) / (1024 * 1024)
    )
}

pub fn current_version() -> &'static str {
    CURRENT_VERSION
}

static EVENT_QUEUE: Mutex<Vec<String>> = Mutex::new(Vec::new());

pub fn drain_events() -> Vec<String> {
    match EVENT_QUEUE.lock() {
        Ok(mut queue) => std::mem::take(&mut *queue),
        Err(poisoned) => {
            let mut queue = poisoned.into_inner();
            std::mem::take(&mut *queue)
        }
    }
}

pub trait Backend {
    fn perform(&mut self, action: Action) -> bool;

    fn confirm_quit_install(&mut self) {}

    fn reason(&self) -> Option<&str>;
}

pub struct UnavailableBackend {
    reason: String,
}

impl UnavailableBackend {
    pub fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
        }
    }
}

impl Backend for UnavailableBackend {
    fn perform(&mut self, _action: Action) -> bool {
        false
    }

    fn reason(&self) -> Option<&str> {
        Some(&self.reason)
    }
}

#[cfg(all(target_os = "macos", screenroom_updater))]
mod sparkle {
    use std::ffi::{CStr, CString, c_char, c_int, c_void};

    use super::{Action, Backend, EVENT_QUEUE};

    const CHOICE_DISMISS: c_int = 1;

    type Handle = *mut c_void;

    type EventCallback = unsafe extern "C" fn(*const c_char);

    unsafe extern "C" {
        fn screenroom_updater_create(
            configuration_json: *const c_char,
            callback: EventCallback,
        ) -> Handle;

        fn screenroom_updater_destroy(handle: Handle);

        fn screenroom_updater_check_now(handle: Handle) -> bool;

        fn screenroom_updater_install_now(handle: Handle) -> bool;

        fn screenroom_updater_defer_to_quit(handle: Handle) -> bool;

        fn screenroom_updater_cancel_download(handle: Handle) -> bool;

        fn screenroom_updater_choose(handle: Handle, choice: c_int) -> bool;

        fn screenroom_updater_install_pending_on_quit(handle: Handle) -> bool;

        fn screenroom_updater_set_automatic_checks(handle: Handle, enabled: bool) -> bool;

        fn screenroom_updater_set_automatic_downloads(handle: Handle, enabled: bool) -> bool;
    }

    unsafe extern "C" fn receive_event(json: *const c_char) {
        if json.is_null() {
            return;
        }
        let text = unsafe { CStr::from_ptr(json) }
            .to_string_lossy()
            .into_owned();
        if let Ok(mut queue) = EVENT_QUEUE.lock() {
            queue.push(text);
        }
    }

    pub struct SparkleBackend {
        handle: Handle,
    }

    unsafe impl Send for SparkleBackend {}

    impl SparkleBackend {
        pub fn open(feed_override: Option<&str>) -> Result<Self, String> {
            let configuration = match feed_override {
                Some(url) => format!(
                    "{{\"feed_url\":{}}}",
                    serde_json::Value::String(url.to_string())
                ),
                None => "{}".to_string(),
            };
            let configuration = CString::new(configuration).map_err(|error| error.to_string())?;
            let handle =
                unsafe { screenroom_updater_create(configuration.as_ptr(), receive_event) };
            if handle.is_null() {
                return Err(
                    "the update framework did not start, so this build cannot check for a new \
                     version"
                        .to_string(),
                );
            }
            Ok(Self { handle })
        }

        fn call(&self, action: Action) -> bool {
            unsafe {
                match action {
                    Action::CheckNow => screenroom_updater_check_now(self.handle),
                    Action::InstallNow => screenroom_updater_install_now(self.handle),
                    Action::InstallOnQuit => screenroom_updater_defer_to_quit(self.handle),
                    Action::CancelDownload => screenroom_updater_cancel_download(self.handle),
                    Action::Dismiss => screenroom_updater_choose(self.handle, CHOICE_DISMISS),
                    Action::SetAutomaticChecks(enabled) => {
                        screenroom_updater_set_automatic_checks(self.handle, enabled)
                    }
                    Action::SetAutomaticDownloads(enabled) => {
                        screenroom_updater_set_automatic_downloads(self.handle, enabled)
                    }
                }
            }
        }
    }

    impl Backend for SparkleBackend {
        fn perform(&mut self, action: Action) -> bool {
            let accepted = self.call(action);
            if !accepted && action == Action::InstallNow {
                return self.call(Action::InstallOnQuit);
            }
            accepted
        }

        fn confirm_quit_install(&mut self) {
            unsafe { screenroom_updater_install_pending_on_quit(self.handle) };
        }

        fn reason(&self) -> Option<&str> {
            None
        }
    }

    impl Drop for SparkleBackend {
        fn drop(&mut self) {
            unsafe { screenroom_updater_destroy(self.handle) };
        }
    }

    pub use self::SparkleBackend as PlatformBackend;

    pub fn open(feed_override: Option<&str>) -> Result<PlatformBackend, String> {
        PlatformBackend::open(feed_override)
    }
}

#[cfg(not(all(target_os = "macos", screenroom_updater)))]
mod sparkle {
    pub struct PlatformBackend;

    pub fn open(_feed_override: Option<&str>) -> Result<PlatformBackend, String> {
        Err(if cfg!(target_os = "macos") {
            "this build carries no update framework, so it cannot look for a new version"
                .to_string()
        } else {
            "automatic updates exist on macOS only".to_string()
        })
    }

    impl super::Backend for PlatformBackend {
        fn perform(&mut self, _action: super::Action) -> bool {
            false
        }

        fn reason(&self) -> Option<&str> {
            Some("no update framework in this build")
        }
    }
}

pub fn open(feed_override: Option<&str>) -> (Updater, Box<dyn Backend>) {
    match sparkle::open(feed_override) {
        Ok(backend) => (Updater::new(), Box::new(backend)),
        Err(reason) => (
            Updater::unavailable(reason.clone()),
            Box::new(UnavailableBackend::new(reason)),
        ),
    }
}

pub fn feed_override_from_environment() -> Option<String> {
    std::env::var("SCREENROOM_UPDATE_FEED")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

pub const DEFAULT_FEED_URL: &str =
    "https://github.com/Dropout-Studio-Co/screenroom/releases/latest/download/appcast.xml";

pub fn feed_label() -> String {
    feed_override_from_environment().unwrap_or_else(|| DEFAULT_FEED_URL.to_string())
}

pub fn describe_feed_signing(state: &str) -> String {
    match state {
        "succeeded" => "The signed feed and the archive signature both verified.".to_string(),
        "failed" => "The feed signature did not verify, so this update is refused.".to_string(),
        "skipped" => "No signature was checked for this item; this build requires one.".to_string(),
        _ => "Not reported yet.".to_string(),
    }
}

pub const CHECK_ENV: &str = "SCREENROOM_UPDATE_CHECK";

pub const INSTALL_ENV: &str = "SCREENROOM_UPDATE_INSTALL";

pub const TIMEOUT_ENV: &str = "SCREENROOM_UPDATE_TIMEOUT";

pub const DEFAULT_TIMEOUT_SECONDS: u64 = 180;

pub fn check_requested() -> bool {
    std::env::var_os(CHECK_ENV).is_some()
}

impl UpdateStatus {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Unavailable { .. } => "unavailable",
            Self::Idle => "idle",
            Self::Checking => "checking",
            Self::UpToDate => "up_to_date",
            Self::Available => "available",
            Self::Downloading { .. } => "downloading",
            Self::Extracting { .. } => "extracting",
            Self::Ready {
                deferred_to_quit: true,
            } => "ready_on_quit",
            Self::Ready { .. } => "ready",
            Self::Installing => "installing",
            Self::Installed => "installed",
            Self::Failed { .. } => "failed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallMode {
    Stop,
    Now,
    Quit,
}

pub struct CheckRun {
    receipt: std::path::PathBuf,
    mode: InstallMode,
    started: std::time::Instant,
    deadline: std::time::Duration,
    begun: bool,
    requested_download: bool,
    attempts: u8,
    last_attempt: Option<std::time::Instant>,
    requested_install: bool,
    settle_until: Option<std::time::Instant>,
    last: Option<&'static str>,
    finished: bool,
}

impl CheckRun {
    pub fn from_environment() -> Option<Self> {
        let path = std::path::PathBuf::from(std::env::var_os(CHECK_ENV)?);
        let mode = match std::env::var(INSTALL_ENV).unwrap_or_default().as_str() {
            "now" => InstallMode::Now,
            "quit" => InstallMode::Quit,
            _ => InstallMode::Stop,
        };
        let seconds = std::env::var(TIMEOUT_ENV)
            .ok()
            .and_then(|value| value.trim().parse::<u64>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(DEFAULT_TIMEOUT_SECONDS);
        Some(Self {
            receipt: path,
            mode,
            started: std::time::Instant::now(),
            deadline: std::time::Duration::from_secs(seconds),
            begun: false,
            requested_download: false,
            attempts: 0,
            last_attempt: None,
            requested_install: false,
            settle_until: None,
            last: None,
            finished: false,
        })
    }

    pub fn finished(&self) -> bool {
        self.finished
    }

    pub fn step(&mut self, updater: &Updater) -> Option<Action> {
        if self.finished {
            return None;
        }
        let status = updater.status();
        let name = status.name();
        if self.last != Some(name) {
            self.last = Some(name);
            let version = updater
                .update()
                .map(|update| update.version.clone())
                .unwrap_or_default();
            self.append(&format!(
                "\"status\":\"{name}\",\"version\":\"{}\",\"current\":\"{}\"",
                escape(&version),
                CURRENT_VERSION
            ));
        }
        if self.started.elapsed() > self.deadline {
            self.append("\"status\":\"timed_out\"");
            self.finished = true;
            return None;
        }
        if !self.begun && !updater.is_unavailable() && !status.is_busy() {
            self.begun = true;
            self.append("\"status\":\"check_requested\"");
            return Some(Action::CheckNow);
        }
        let waiting = matches!(status, UpdateStatus::Idle)
            && !matches!(status, UpdateStatus::Unavailable { .. });
        let due = self
            .last_attempt
            .map(|attempt| attempt.elapsed() >= std::time::Duration::from_secs(1))
            .unwrap_or(true);
        if waiting && self.attempts < 10 && due && self.begun {
            self.attempts += 1;
            self.last_attempt = Some(std::time::Instant::now());
            self.append("\"status\":\"check_retry\"");
            return Some(Action::CheckNow);
        }
        let download_first = self.mode != InstallMode::Stop
            && matches!(status, UpdateStatus::Available)
            && !self.requested_download;
        if download_first {
            self.requested_download = true;
            self.append("\"status\":\"download_requested\"");
            return Some(Action::InstallNow);
        }
        if self.mode == InstallMode::Now && status.is_ready() && !self.requested_install {
            self.requested_install = true;
            self.append("\"status\":\"install_requested\"");
            return Some(Action::InstallNow);
        }
        if self.mode == InstallMode::Quit && status.is_ready() && !self.requested_install {
            self.requested_install = true;
            self.settle_until = Some(std::time::Instant::now() + std::time::Duration::from_secs(3));
            self.append("\"status\":\"quit_requested\"");
            return Some(Action::InstallOnQuit);
        }
        if self.mode == InstallMode::Quit
            && self
                .settle_until
                .is_some_and(|until| std::time::Instant::now() >= until)
        {
            self.append("\"status\":\"quit_ready\"");
            self.finished = true;
        }
        if matches!(status, UpdateStatus::Failed { .. }) {
            self.finished = true;
            return None;
        }
        if self.mode == InstallMode::Stop
            && (matches!(status, UpdateStatus::UpToDate)
                || matches!(status, UpdateStatus::Available)
                || matches!(status, UpdateStatus::Ready { .. }))
        {
            self.finished = true;
        }
        None
    }

    fn append(&self, fields: &str) {
        let line = format!(
            "{{\"at_ms\":{},\"elapsed_ms\":{},{}}}\n",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|elapsed| elapsed.as_millis() as u64)
                .unwrap_or_default(),
            self.started.elapsed().as_millis() as u64,
            fields
        );
        if let Some(parent) = self.receipt.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        use std::io::Write;
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.receipt)
        {
            let _ = file.write_all(line.as_bytes());
        }
    }
}

fn escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(version: &str, user_initiated: bool) -> Event {
        Event::Found {
            update: AvailableUpdate {
                version: version.to_string(),
                display_version: version.to_string(),
                notes: String::new(),
                critical: false,
                informational: false,
                feed_signing: "succeeded".to_string(),
            },
            stage: Stage::NotDownloaded,
            user_initiated,
        }
    }

    #[test]
    fn a_newer_version_is_offered_and_an_older_one_is_not() {
        let mut updater = Updater::new();
        updater.feed(found("99.0.0", false));
        assert_eq!(updater.status(), &UpdateStatus::Available);

        let mut outdated = Updater::new();
        outdated.feed(found("0.0.1", false));
        assert_eq!(outdated.status(), &UpdateStatus::UpToDate);
        assert!(outdated.update().is_none());
    }

    #[test]
    fn a_user_initiated_check_still_reports_the_same_version() {
        let mut updater = Updater::new();
        updater.feed(found(CURRENT_VERSION, true));
        assert_eq!(updater.status(), &UpdateStatus::Available);
    }

    #[test]
    fn versions_compare_numerically_not_alphabetically() {
        assert_eq!(compare_versions("0.10.0", "0.9.0"), 1);
        assert_eq!(compare_versions("1.0", "1.0.0"), 0);
        assert_eq!(compare_versions("2.0.0", "10.0.0"), -1);
        assert_eq!(compare_versions("1.2.3-beta", "1.2.3"), 0);
        assert_eq!(compare_versions("", ""), 0);
    }

    #[test]
    fn nothing_installs_while_a_film_is_playing() {
        let mut updater = Updater::new();
        updater.feed(found("9.9.9", true));
        assert!(updater.may_install(Playback::Idle));
        assert!(!updater.may_install(Playback::Playing));
        assert!(updater.request_install(Playback::Playing).is_empty());
        assert!(
            updater
                .request_install_on_quit(Playback::Playing)
                .is_empty()
        );
    }

    #[test]
    fn a_manual_check_is_refused_while_playing_and_while_one_is_running() {
        let mut updater = Updater::new();
        assert!(updater.request_check(Playback::Playing).is_empty());
        updater.feed(Event::Checking);
        assert!(updater.request_check(Playback::Idle).is_empty());
    }

    #[test]
    fn an_unavailable_updater_never_asks_the_backend_for_anything() {
        let mut updater = Updater::unavailable("no framework");
        assert!(updater.request_check(Playback::Idle).is_empty());
        assert!(updater.request_install(Playback::Idle).is_empty());
        assert!(updater.request_automatic_checks(true).is_empty());
        assert_eq!(updater.headline(), None);
        assert_eq!(updater.primary_action(), None);
    }

    #[test]
    fn download_progress_and_extraction_reach_the_headline() {
        let mut updater = Updater::new();
        updater.feed(found("9.9.9", true));
        updater.feed(Event::Downloading {
            received: 5 * 1024 * 1024,
            expected: 10 * 1024 * 1024,
        });
        assert_eq!(
            updater.status(),
            &UpdateStatus::Downloading {
                received: 5 * 1024 * 1024,
                expected: 10 * 1024 * 1024
            }
        );
        assert!(
            updater
                .headline()
                .unwrap_or_default()
                .contains("50% of 10 MB")
        );
        updater.feed(Event::Extracting { progress: 0.42 });
        assert!(updater.headline().unwrap_or_default().contains("42%"));
    }

    #[test]
    fn a_ready_update_records_whether_it_waits_for_quit() {
        let mut updater = Updater::new();
        updater.feed(Event::Ready {
            update: AvailableUpdate {
                version: "9.9.9".to_string(),
                display_version: "9.9.9".to_string(),
                notes: String::new(),
                critical: false,
                informational: false,
                feed_signing: "succeeded".to_string(),
            },
            deferred_to_quit: false,
        });
        assert!(updater.status().is_ready());
        updater.feed(Event::Deferred);
        assert_eq!(
            updater.status(),
            &UpdateStatus::Ready {
                deferred_to_quit: true
            }
        );
        assert!(updater.headline().unwrap_or_default().contains("quit"));
    }

    #[test]
    fn a_failure_is_reported_without_touching_the_rest_of_the_state() {
        let mut updater = Updater::new();
        updater.feed(Event::Failed {
            message: "the download was cut off".to_string(),
        });
        assert_eq!(updater.last_failure(), Some("the download was cut off"));
        assert!(matches!(updater.status(), UpdateStatus::Failed { .. }));
        assert!(!updater.status().is_busy());
    }

    #[test]
    fn a_failure_without_a_description_still_says_something() {
        let event = Updater::decode("{\"event\":\"failed\"}").expect("a failed event");
        assert_eq!(
            event,
            Event::Failed {
                message: "The update failed without a description.".to_string()
            }
        );
    }

    #[test]
    fn the_bridge_json_decodes_into_the_expected_events() {
        let settings = Updater::decode(
            "{\"event\":\"settings\",\"automatic_checks\":true,\"automatic_downloads\":false,\
             \"allows_automatic_updates\":true}",
        )
        .expect("settings");
        assert_eq!(
            settings,
            Event::Settings {
                automatic_checks: true,
                automatic_downloads: false,
                allows_automatic_updates: true
            }
        );

        let found = Updater::decode(
            "{\"event\":\"found\",\"version\":\"0.2.0\",\"display_version\":\"0.2.0\",\
             \"stage\":\"not_downloaded\",\"user_initiated\":false,\"critical\":false,\
             \"informational\":false,\"feed_signing\":\"succeeded\",\"notes\":\"Faster scans.\"}",
        )
        .expect("found");
        match found {
            Event::Found { update, stage, .. } => {
                assert_eq!(update.version, "0.2.0");
                assert_eq!(update.notes, "Faster scans.");
                assert_eq!(update.feed_signing, "succeeded");
                assert_eq!(stage, Stage::NotDownloaded);
            }
            other => panic!("expected a found event, got {other:?}"),
        }

        assert_eq!(
            Updater::decode("{\"event\":\"extracting\",\"progress\":0.75}"),
            Some(Event::Extracting { progress: 0.75 })
        );
        assert_eq!(
            Updater::decode("{\"event\":\"installed\",\"relaunched\":false}"),
            Some(Event::Installed { relaunched: false })
        );
        assert_eq!(
            Updater::decode("{\"event\":\"something-new\"}"),
            Some(Event::Unknown)
        );
        assert_eq!(Updater::decode("not json"), None);
    }

    #[test]
    fn settings_from_the_framework_replace_the_local_copy() {
        let mut updater = Updater::new();
        assert!(updater.settings().automatic_checks);
        updater.feed(Event::Settings {
            automatic_checks: false,
            automatic_downloads: false,
            allows_automatic_updates: false,
        });
        assert!(!updater.settings().automatic_checks);
        assert!(!updater.settings().automatic_downloads);
        assert!(!updater.settings().allows_automatic_updates);
    }

    #[test]
    fn the_primary_action_follows_the_state() {
        let mut updater = Updater::new();
        assert_eq!(updater.primary_action(), Some(Action::CheckNow));
        updater.feed(found("9.9.9", true));
        assert_eq!(updater.primary_action(), Some(Action::InstallNow));
        updater.feed(Event::Downloading {
            received: 1,
            expected: 2,
        });
        assert_eq!(updater.primary_action(), Some(Action::CancelDownload));
        updater.feed(Event::Installing);
        assert_eq!(updater.primary_action(), None);
    }

    #[test]
    fn a_background_check_that_is_already_current_stays_quiet() {
        let mut updater = Updater::new();
        updater.feed(found("9.9.9", false));
        updater.feed(Event::UpToDate {
            user_initiated: false,
        });
        assert_eq!(updater.status(), &UpdateStatus::Idle);
        assert_eq!(updater.headline(), None);
    }

    #[test]
    fn the_scheduled_delay_is_kept_for_the_settings_row() {
        let mut updater = Updater::new();
        updater.feed(Event::Scheduled { delay: 86_400.0 });
        assert_eq!(updater.scheduled_delay(), Some(86_400.0));
    }
}
