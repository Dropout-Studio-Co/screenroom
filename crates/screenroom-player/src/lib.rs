use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::path::{Path, PathBuf};
use std::ptr::NonNull;
use std::sync::{Arc, Condvar, Mutex};

use libloading::Library;

mod frame;
mod render;
mod worker;

#[cfg(target_os = "macos")]
pub mod gl_render;

#[cfg(target_os = "macos")]
pub mod surface;

pub use frame::{Frame, OutputDescription, OutputTransfer, PixelFormat, ToneMapping, half_to_f32};

#[cfg(target_os = "macos")]
pub use gl_render::GlRenderer;

#[cfg(target_os = "macos")]
pub use surface::SurfaceFrame;

pub use worker::{
    HdrMode, HwdecMode, MediaTrack, NO_TRACK, Player, PlayerCommand, PlayerFailure, PlayerSnapshot,
    SUB_POSITION_RANGE, SUB_SCALE_RANGE, TrackKind,
};

pub const DEFAULT_EVENT_TIMEOUT: f64 = 5.0;

#[derive(Clone, Debug)]
pub struct FrameTicks(Arc<std::sync::atomic::AtomicU64>);

impl FrameTicks {
    pub(crate) fn new() -> Self {
        Self(Arc::new(std::sync::atomic::AtomicU64::new(0)))
    }

    pub fn current(&self) -> u64 {
        self.0.load(std::sync::atomic::Ordering::Acquire)
    }

    pub(crate) fn bump(&self) {
        self.0.fetch_add(1, std::sync::atomic::Ordering::Release);
    }
}

impl PartialEq for FrameTicks {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for FrameTicks {}

impl std::hash::Hash for FrameTicks {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        Arc::as_ptr(&self.0).hash(state);
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PlayerError {
    #[error("libmpv is not available on this device: {0}")]
    Unavailable(String),

    #[error("{context} failed: {message} (code {code})")]
    Mpv {
        context: String,

        message: String,

        code: i32,
    },

    #[error("invalid argument: {0}")]
    Argument(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderBridge {
    Software,

    OpenGl,

    Direct3D11,
}

pub fn render_bridge() -> RenderBridge {
    match ACTIVE_BRIDGE.load(std::sync::atomic::Ordering::Relaxed) {
        1 => RenderBridge::OpenGl,
        2 => RenderBridge::Direct3D11,
        _ => RenderBridge::Software,
    }
}

static ACTIVE_BRIDGE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

pub(crate) fn set_render_bridge(bridge: RenderBridge) {
    let value = match bridge {
        RenderBridge::Software => 0,
        RenderBridge::OpenGl => 1,
        RenderBridge::Direct3D11 => 2,
    };
    ACTIVE_BRIDGE.store(value, std::sync::atomic::Ordering::Relaxed);
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerCapability {
    pub available: bool,

    pub reason: String,

    pub library_path: Option<String>,

    pub api_version: Option<String>,
}

fn candidates() -> &'static [&'static str] {
    if cfg!(windows) {
        &["mpv-2.dll", "libmpv-2.dll", "libmpv.dll"]
    } else if cfg!(target_os = "macos") {
        &[
            "libmpv.2.dylib",
            "libmpv.dylib",
            "/opt/homebrew/lib/libmpv.2.dylib",
            "/usr/local/lib/libmpv.2.dylib",
            "/opt/homebrew/lib/libmpv.dylib",
            "/usr/local/lib/libmpv.dylib",
        ]
    } else {
        &["libmpv.so.2", "libmpv.so.1", "libmpv.so"]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BundleLayout {
    MacOsAppBundle,

    WindowsPortable,

    SystemOnly,
}

impl BundleLayout {
    pub fn host() -> Self {
        if cfg!(target_os = "macos") {
            Self::MacOsAppBundle
        } else if cfg!(windows) {
            Self::WindowsPortable
        } else {
            Self::SystemOnly
        }
    }
}

pub fn system_search_consults_working_directory() -> bool {
    cfg!(windows)
}

fn system_candidates() -> Vec<PathBuf> {
    candidates().iter().map(PathBuf::from).collect()
}

fn library_search_order(
    system: &[PathBuf],
    bundled: &[PathBuf],
    system_search_consults_working_directory: bool,
) -> Vec<PathBuf> {
    let mut order = Vec::with_capacity(system.len() + bundled.len());
    if system_search_consults_working_directory {
        order.extend_from_slice(bundled);
        order.extend_from_slice(system);
    } else {
        order.extend_from_slice(system);
        order.extend_from_slice(bundled);
    }
    order
}

fn bundled_names() -> &'static [&'static str] {
    if cfg!(windows) {
        &["mpv-2.dll", "libmpv-2.dll", "libmpv.dll"]
    } else if cfg!(target_os = "macos") {
        &["libmpv.2.dylib", "libmpv.dylib"]
    } else {
        &[]
    }
}

fn bundled_libmpv_paths(binary_dir: &Path, names: &[&str], layout: BundleLayout) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    match layout {
        BundleLayout::MacOsAppBundle => {
            let contents = binary_dir.parent().map(Path::to_path_buf);
            for name in names {
                if let Some(contents) = &contents {
                    paths.push(contents.join("Frameworks").join(name));
                }

                paths.push(binary_dir.join(name));
            }
        }
        BundleLayout::WindowsPortable => {
            for name in names {
                paths.push(binary_dir.join(name));

                paths.push(binary_dir.join("bin").join(name));
                paths.push(binary_dir.join("lib").join(name));
            }
        }
        BundleLayout::SystemOnly => {}
    }
    paths
}

pub fn libmpv_search_plan() -> Vec<PathBuf> {
    library_search_order(
        &system_candidates(),
        &packaged_candidates(),
        system_search_consults_working_directory(),
    )
}

fn packaged_candidates() -> Vec<PathBuf> {
    let Ok(executable) = std::env::current_exe() else {
        return Vec::new();
    };
    let Some(binary_dir) = executable.parent() else {
        return Vec::new();
    };
    bundled_libmpv_paths(binary_dir, bundled_names(), BundleLayout::host())
}

pub(crate) type MpvHandle = *mut c_void;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub(crate) struct RawEvent {
    pub(crate) event_id: c_int,
    pub(crate) error: c_int,
    pub(crate) reply_userdata: u64,
    pub(crate) data: *mut c_void,
}

#[derive(Clone, Copy)]
pub(crate) struct RenderSymbols {
    create: unsafe extern "C" fn(*mut *mut c_void, MpvHandle, *mut c_void) -> c_int,
    free: unsafe extern "C" fn(*mut c_void),
    render: unsafe extern "C" fn(*mut c_void, *mut c_void) -> c_int,
    update: unsafe extern "C" fn(*mut c_void) -> u64,
    #[cfg(target_os = "macos")]
    report_swap: Option<unsafe extern "C" fn(*mut c_void)>,
    set_update_callback:
        unsafe extern "C" fn(*mut c_void, Option<unsafe extern "C" fn(*mut c_void)>, *mut c_void),
}

type FnCreate = unsafe extern "C" fn() -> MpvHandle;
type FnInitialize = unsafe extern "C" fn(MpvHandle) -> c_int;
type FnTerminateDestroy = unsafe extern "C" fn(MpvHandle);
type FnSetOptionString = unsafe extern "C" fn(MpvHandle, *const c_char, *const c_char) -> c_int;
type FnCommand = unsafe extern "C" fn(MpvHandle, *const *const c_char) -> c_int;
type FnGetProperty = unsafe extern "C" fn(MpvHandle, *const c_char, c_int, *mut c_void) -> c_int;

type FnGetPropertyString = unsafe extern "C" fn(MpvHandle, *const c_char) -> *mut c_char;
type FnSetPropertyString = unsafe extern "C" fn(MpvHandle, *const c_char, *const c_char) -> c_int;
type FnFree = unsafe extern "C" fn(*mut c_void);
type FnErrorString = unsafe extern "C" fn(c_int) -> *const c_char;
type FnWaitEvent = unsafe extern "C" fn(MpvHandle, f64) -> *mut RawEvent;
type FnSetWakeupCallback =
    unsafe extern "C" fn(MpvHandle, Option<unsafe extern "C" fn(*mut c_void)>, *mut c_void);
type FnClientApiVersion = unsafe extern "C" fn() -> u64;
type FnObserveProperty = unsafe extern "C" fn(MpvHandle, u64, *const c_char, c_int) -> c_int;

struct Symbols {
    create: FnCreate,
    initialize: FnInitialize,
    terminate_destroy: FnTerminateDestroy,
    set_option_string: FnSetOptionString,
    command: FnCommand,
    get_property: FnGetProperty,
    get_property_string: FnGetPropertyString,
    set_property_string: FnSetPropertyString,
    free: FnFree,
    error_string: FnErrorString,
    wait_event: FnWaitEvent,
    set_wakeup_callback: FnSetWakeupCallback,
    client_api_version: FnClientApiVersion,
    observe_property: FnObserveProperty,
    render: Option<RenderSymbols>,
}

pub struct LibMpv {
    path: String,
    symbols: Symbols,

    _library: Library,
}

impl std::fmt::Debug for LibMpv {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LibMpv")
            .field("path", &self.path)
            .field("api_version", &self.api_version())
            .finish()
    }
}

impl LibMpv {
    pub fn load() -> Result<Arc<Self>, PlayerError> {
        let order = library_search_order(
            &system_candidates(),
            &packaged_candidates(),
            system_search_consults_working_directory(),
        );
        let mut attempted = Vec::new();
        for candidate in &order {
            match Self::load_path(candidate) {
                Ok(library) => return Ok(Arc::new(library)),
                Err(error) => attempted.push(format!("{} ({error})", candidate.display())),
            }
        }
        Err(PlayerError::Unavailable(format!(
            "no libmpv shared library found; tried {}",
            attempted.join(", ")
        )))
    }

    pub fn load_path(path: impl AsRef<Path>) -> Result<Self, PlayerError> {
        let path = path.as_ref();
        let library = unsafe { Library::new(path) }
            .map_err(|error| PlayerError::Unavailable(format!("{}: {error}", path.display())))?;
        unsafe {
            let symbols = Symbols {
                create: *library
                    .get(b"mpv_create\0")
                    .map_err(|error| missing(path, "mpv_create", error))?,
                initialize: *library
                    .get(b"mpv_initialize\0")
                    .map_err(|error| missing(path, "mpv_initialize", error))?,
                terminate_destroy: *library
                    .get(b"mpv_terminate_destroy\0")
                    .map_err(|error| missing(path, "mpv_terminate_destroy", error))?,
                set_option_string: *library
                    .get(b"mpv_set_option_string\0")
                    .map_err(|error| missing(path, "mpv_set_option_string", error))?,
                command: *library
                    .get(b"mpv_command\0")
                    .map_err(|error| missing(path, "mpv_command", error))?,
                get_property: *library
                    .get(b"mpv_get_property\0")
                    .map_err(|error| missing(path, "mpv_get_property", error))?,
                get_property_string: *library
                    .get(b"mpv_get_property_string\0")
                    .map_err(|error| missing(path, "mpv_get_property_string", error))?,
                set_property_string: *library
                    .get(b"mpv_set_property_string\0")
                    .map_err(|error| missing(path, "mpv_set_property_string", error))?,
                free: *library
                    .get(b"mpv_free\0")
                    .map_err(|error| missing(path, "mpv_free", error))?,
                error_string: *library
                    .get(b"mpv_error_string\0")
                    .map_err(|error| missing(path, "mpv_error_string", error))?,
                wait_event: *library
                    .get(b"mpv_wait_event\0")
                    .map_err(|error| missing(path, "mpv_wait_event", error))?,
                set_wakeup_callback: *library
                    .get(b"mpv_set_wakeup_callback\0")
                    .map_err(|error| missing(path, "mpv_set_wakeup_callback", error))?,
                client_api_version: *library
                    .get(b"mpv_client_api_version\0")
                    .map_err(|error| missing(path, "mpv_client_api_version", error))?,
                observe_property: *library
                    .get(b"mpv_observe_property\0")
                    .map_err(|error| missing(path, "mpv_observe_property", error))?,
                render: render_symbols(&library),
            };
            Ok(Self {
                path: path.display().to_string(),
                symbols,
                _library: library,
            })
        }
    }

    pub fn api_version(&self) -> String {
        let raw = unsafe { (self.symbols.client_api_version)() };
        format!("{}.{}", raw >> 16, raw & 0xffff)
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub(crate) fn render_symbols(&self) -> Option<RenderSymbols> {
        self.symbols.render
    }

    pub(crate) fn message(&self, code: c_int) -> String {
        unsafe {
            let pointer = (self.symbols.error_string)(code);
            if pointer.is_null() {
                format!("mpv error {code}")
            } else {
                CStr::from_ptr(pointer).to_string_lossy().to_string()
            }
        }
    }
}

fn render_symbols(library: &Library) -> Option<RenderSymbols> {
    unsafe {
        Some(RenderSymbols {
            create: *library.get(b"mpv_render_context_create\0").ok()?,
            free: *library.get(b"mpv_render_context_free\0").ok()?,
            render: *library.get(b"mpv_render_context_render\0").ok()?,
            update: *library.get(b"mpv_render_context_update\0").ok()?,
            #[cfg(target_os = "macos")]
            report_swap: library
                .get(b"mpv_render_context_report_swap\0")
                .ok()
                .map(|symbol| *symbol),
            set_update_callback: *library
                .get(b"mpv_render_context_set_update_callback\0")
                .ok()?,
        })
    }
}

fn missing(path: &Path, symbol: &str, error: libloading::Error) -> PlayerError {
    PlayerError::Unavailable(format!(
        "{} does not export {symbol}: {error}",
        path.display()
    ))
}

pub fn capability() -> PlayerCapability {
    match LibMpv::load() {
        Ok(library) => describe(&library),
        Err(error) => PlayerCapability {
            available: false,
            reason: error.to_string(),
            library_path: None,
            api_version: None,
        },
    }
}

pub fn capability_at(path: impl AsRef<Path>) -> PlayerCapability {
    match LibMpv::load_path(path) {
        Ok(library) => describe(&Arc::new(library)),
        Err(error) => PlayerCapability {
            available: false,
            reason: error.to_string(),
            library_path: None,
            api_version: None,
        },
    }
}

fn describe(library: &Arc<LibMpv>) -> PlayerCapability {
    let version = library.api_version();
    let path = library.path().to_string();
    match Mpv::new(library) {
        Ok(_core) => PlayerCapability {
            available: true,
            reason: format!("libmpv {version} loaded"),
            library_path: Some(path),
            api_version: Some(version),
        },
        Err(error) => PlayerCapability {
            available: false,
            reason: format!("libmpv loaded but could not initialise: {error}"),
            library_path: Some(path),
            api_version: Some(version),
        },
    }
}

pub struct Mpv {
    handle: NonNull<c_void>,
    library: Arc<LibMpv>,
    wakeup_data: Option<Arc<WakeSignal>>,
}

pub(crate) struct WakeSignal {
    sequence: Mutex<u64>,
    changed: Condvar,
}

impl WakeSignal {
    pub(crate) fn new() -> Self {
        Self {
            sequence: Mutex::new(0),
            changed: Condvar::new(),
        }
    }

    pub(crate) fn notify(&self) {
        let mut sequence = self
            .sequence
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        *sequence = sequence.wrapping_add(1);
        self.changed.notify_one();
    }

    pub(crate) fn current(&self) -> u64 {
        *self
            .sequence
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }

    pub(crate) fn wait_after(&self, observed: u64) {
        let mut sequence = self
            .sequence
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        while *sequence == observed {
            sequence = self
                .changed
                .wait(sequence)
                .unwrap_or_else(|error| error.into_inner());
        }
    }
}

unsafe extern "C" fn wakeup_callback(userdata: *mut c_void) {
    let signal = unsafe { &*userdata.cast::<WakeSignal>() };
    signal.notify();
}

unsafe impl Send for Mpv {}
unsafe impl Sync for Mpv {}

impl std::fmt::Debug for Mpv {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Mpv").finish_non_exhaustive()
    }
}

impl Drop for Mpv {
    fn drop(&mut self) {
        if self.wakeup_data.is_some() {
            unsafe {
                (self.library.symbols.set_wakeup_callback)(
                    self.handle.as_ptr(),
                    None,
                    std::ptr::null_mut(),
                )
            };
        }
        unsafe { (self.library.symbols.terminate_destroy)(self.handle.as_ptr()) }

        self.wakeup_data.take();
    }
}

impl Mpv {
    pub fn new(library: &Arc<LibMpv>) -> Result<Self, PlayerError> {
        Self::new_with_options(library, &[])
    }

    pub fn new_with_options(
        library: &Arc<LibMpv>,
        options: &[(&str, &str)],
    ) -> Result<Self, PlayerError> {
        let handle = unsafe { (library.symbols.create)() };
        let handle = NonNull::new(handle)
            .ok_or_else(|| PlayerError::Unavailable("mpv_create returned null".into()))?;
        let core = Self {
            handle,
            library: Arc::clone(library),
            wakeup_data: None,
        };
        for (name, value) in options {
            core.set_option(name, value)?;
        }
        let code = unsafe { (library.symbols.initialize)(handle.as_ptr()) };
        core.check(code, "mpv_initialize")?;
        Ok(core)
    }

    pub(crate) fn library(&self) -> &Arc<LibMpv> {
        &self.library
    }

    pub(crate) fn raw_handle(&self) -> MpvHandle {
        self.handle.as_ptr()
    }

    pub(crate) fn set_wakeup_signal(&mut self, signal: Arc<WakeSignal>) {
        let pointer = Arc::as_ptr(&signal).cast_mut().cast::<c_void>();

        unsafe {
            (self.library.symbols.set_wakeup_callback)(
                self.handle.as_ptr(),
                Some(wakeup_callback),
                pointer,
            )
        };
        self.wakeup_data = Some(signal);
    }

    pub fn observe(&self, name: &str, format: i32, userdata: u64) -> Result<(), PlayerError> {
        let name = to_cstring(name)?;
        let code = unsafe {
            (self.library.symbols.observe_property)(
                self.handle.as_ptr(),
                userdata,
                name.as_ptr(),
                format,
            )
        };
        self.check(
            code,
            &format!("mpv_observe_property({})", name.to_string_lossy()),
        )
    }

    pub fn get_i64(&self, name: &str) -> Result<i64, PlayerError> {
        let name = to_cstring(name)?;
        let mut value: i64 = 0;
        let code = unsafe {
            (self.library.symbols.get_property)(
                self.handle.as_ptr(),
                name.as_ptr(),
                4,
                (&mut value as *mut i64).cast(),
            )
        };
        self.check(
            code,
            &format!("mpv_get_property({})", name.to_string_lossy()),
        )?;
        Ok(value)
    }

    pub(crate) fn wait_event_raw(&mut self, timeout_seconds: f64) -> Option<RawEvent> {
        let event =
            unsafe { (self.library.symbols.wait_event)(self.handle.as_ptr(), timeout_seconds) };
        if event.is_null() {
            return None;
        }

        let event = unsafe { *event };
        if event.event_id == event::NONE {
            return None;
        }
        Some(event)
    }

    fn check(&self, code: c_int, context: &str) -> Result<(), PlayerError> {
        if code < 0 {
            return Err(PlayerError::Mpv {
                context: context.to_string(),
                message: self.library.message(code),
                code,
            });
        }
        Ok(())
    }

    pub fn set_option(&self, name: &str, value: &str) -> Result<(), PlayerError> {
        let name = to_cstring(name)?;
        let value = to_cstring(value)?;
        let code = unsafe {
            (self.library.symbols.set_option_string)(
                self.handle.as_ptr(),
                name.as_ptr(),
                value.as_ptr(),
            )
        };
        self.check(
            code,
            &format!("mpv_set_option_string({})", name.to_string_lossy()),
        )
    }

    pub fn command(&self, arguments: &[&str]) -> Result<(), PlayerError> {
        if arguments.is_empty() {
            return Err(PlayerError::Argument("empty command".into()));
        }
        let owned = arguments
            .iter()
            .map(|argument| to_cstring(argument))
            .collect::<Result<Vec<_>, _>>()?;
        let mut pointers: Vec<*const c_char> = owned.iter().map(|value| value.as_ptr()).collect();
        pointers.push(std::ptr::null());
        let code =
            unsafe { (self.library.symbols.command)(self.handle.as_ptr(), pointers.as_ptr()) };
        self.check(code, &format!("mpv_command({})", arguments[0]))
    }

    pub fn set_property(&self, name: &str, value: &str) -> Result<(), PlayerError> {
        let name = to_cstring(name)?;
        let value = to_cstring(value)?;
        let code = unsafe {
            (self.library.symbols.set_property_string)(
                self.handle.as_ptr(),
                name.as_ptr(),
                value.as_ptr(),
            )
        };
        self.check(code, "mpv_set_property_string")
    }

    pub fn get_string(&self, name: &str) -> Result<Option<String>, PlayerError> {
        let name = to_cstring(name)?;
        let pointer = unsafe {
            (self.library.symbols.get_property_string)(self.handle.as_ptr(), name.as_ptr())
        };
        if pointer.is_null() {
            return Ok(None);
        }

        let text = unsafe { CStr::from_ptr(pointer) }
            .to_string_lossy()
            .into_owned();
        unsafe { (self.library.symbols.free)(pointer.cast()) };
        Ok(Some(text))
    }

    pub fn get_flag(&self, name: &str) -> Result<bool, PlayerError> {
        let name = to_cstring(name)?;
        let mut value: c_int = 0;
        let code = unsafe {
            (self.library.symbols.get_property)(
                self.handle.as_ptr(),
                name.as_ptr(),
                3,
                (&mut value as *mut c_int).cast(),
            )
        };
        self.check(
            code,
            &format!("mpv_get_property({})", name.to_string_lossy()),
        )?;
        Ok(value != 0)
    }

    pub fn get_f64(&self, name: &str) -> Result<f64, PlayerError> {
        let name = to_cstring(name)?;
        let mut value: f64 = 0.0;
        let code = unsafe {
            (self.library.symbols.get_property)(
                self.handle.as_ptr(),
                name.as_ptr(),
                5,
                (&mut value as *mut f64).cast(),
            )
        };
        self.check(
            code,
            &format!("mpv_get_property({})", name.to_string_lossy()),
        )?;
        Ok(value)
    }

    pub fn wait_event(&mut self, timeout_seconds: f64) -> i32 {
        let event =
            unsafe { (self.library.symbols.wait_event)(self.handle.as_ptr(), timeout_seconds) };
        if event.is_null() {
            return 0;
        }
        unsafe { (*event).event_id }
    }
}

fn to_cstring(value: &str) -> Result<CString, PlayerError> {
    CString::new(value).map_err(|_| PlayerError::Argument(value.to_string()))
}

pub mod event {

    pub const NONE: i32 = 0;

    pub const SHUTDOWN: i32 = 1;

    pub const START_FILE: i32 = 6;

    pub const END_FILE: i32 = 7;

    pub const FILE_LOADED: i32 = 8;

    pub const PROPERTY_CHANGE: i32 = 22;

    pub const VIDEO_RECONFIG: i32 = 17;

    pub const PLAYBACK_RESTART: i32 = 21;

    pub const SEEK: i32 = 20;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerState {
    Idle,

    Resolving,

    Preparing,

    Playing,

    Paused,

    Ended,

    Failed,
}

impl PlayerState {
    pub fn accepts_progress(self) -> bool {
        matches!(self, Self::Playing | Self::Paused | Self::Ended)
    }

    pub fn transition(self, next: PlayerState) -> PlayerState {
        use PlayerState::*;
        match (self, next) {
            (_, Failed) => Failed,
            (Failed, _) => Failed,
            (Ended, _) => Ended,
            (_, Ended) => Ended,
            (Idle, Resolving) => Resolving,
            (Idle, _) => self,
            (Resolving, Preparing) => Preparing,
            (Resolving, Playing) => Playing,
            (Resolving, _) => self,
            (Preparing, Playing) => Playing,
            (Preparing, _) => self,
            (Playing, Paused) => Paused,
            (Paused, Playing) => Playing,
            (Playing, Playing) | (Paused, Paused) => self,

            (_, _) => self,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_is_only_meaningful_after_playing() {
        assert!(!PlayerState::Idle.accepts_progress());
        assert!(!PlayerState::Resolving.accepts_progress());
        assert!(!PlayerState::Preparing.accepts_progress());
        assert!(PlayerState::Playing.accepts_progress());
        assert!(PlayerState::Paused.accepts_progress());
        assert!(PlayerState::Ended.accepts_progress());
    }

    #[test]
    fn prepare_zero_position_cannot_become_playing() {
        assert_eq!(
            PlayerState::Preparing.transition(PlayerState::Paused),
            PlayerState::Preparing
        );
        assert_eq!(
            PlayerState::Preparing.transition(PlayerState::Playing),
            PlayerState::Playing
        );
    }

    #[test]
    fn ended_and_failed_are_terminal() {
        assert_eq!(
            PlayerState::Ended.transition(PlayerState::Playing),
            PlayerState::Ended
        );
        assert_eq!(
            PlayerState::Failed.transition(PlayerState::Playing),
            PlayerState::Failed
        );
        assert_eq!(
            PlayerState::Playing.transition(PlayerState::Failed),
            PlayerState::Failed
        );
    }

    #[test]
    fn a_windows_install_finds_its_own_libmpv_beside_the_executable() {
        let install = Path::new(r"C:\Program Files\Screenroom");
        let names = ["mpv-2.dll", "libmpv.dll"];
        let paths = bundled_libmpv_paths(install, &names, BundleLayout::WindowsPortable);

        assert_eq!(
            paths,
            vec![
                install.join("mpv-2.dll"),
                install.join("bin").join("mpv-2.dll"),
                install.join("lib").join("mpv-2.dll"),
                install.join("libmpv.dll"),
                install.join("bin").join("libmpv.dll"),
                install.join("lib").join("libmpv.dll"),
            ]
        );

        assert!(paths[0].ends_with("mpv-2.dll"));
        assert_eq!(paths[0].parent(), Some(install));

        for path in &paths {
            assert!(
                path.starts_with(install),
                "a bundled candidate must be anchored to the install folder: {}",
                path.display()
            );
            assert!(
                path.file_name().is_some(),
                "a candidate must name a file: {}",
                path.display()
            );
        }
    }

    #[test]
    fn the_windows_name_list_starts_with_the_real_dll_name() {
        let names = bundled_names();
        if cfg!(windows) {
            assert_eq!(names, &["mpv-2.dll", "libmpv-2.dll", "libmpv.dll"]);
        } else if cfg!(target_os = "macos") {
            assert_eq!(names, &["libmpv.2.dylib", "libmpv.dylib"]);
        } else {
            assert!(
                names.is_empty(),
                "Linux bundles nothing and resolves nothing"
            );
        }
    }

    #[test]
    fn a_macos_bundle_is_unchanged_by_the_windows_layout() {
        let names = ["libmpv.2.dylib"];
        let paths = bundled_libmpv_paths(
            Path::new("/Applications/Screenroom.app/Contents/MacOS"),
            &names,
            BundleLayout::MacOsAppBundle,
        );
        let rendered: Vec<String> = paths.iter().map(|p| p.display().to_string()).collect();
        assert_eq!(
            rendered,
            vec![
                "/Applications/Screenroom.app/Contents/Frameworks/libmpv.2.dylib",
                "/Applications/Screenroom.app/Contents/MacOS/libmpv.2.dylib",
            ]
        );
    }

    #[test]
    fn a_system_only_layout_bundles_nothing() {
        let names = ["libmpv.so.2", "libmpv.so.1"];
        assert!(
            bundled_libmpv_paths(Path::new("/usr/bin"), &names, BundleLayout::SystemOnly)
                .is_empty()
        );
    }

    #[test]
    fn the_working_directory_is_searched_last_on_windows_and_never_first() {
        let system = vec![PathBuf::from("mpv-2.dll")];
        let bundled = vec![PathBuf::from(r"C:\App\mpv-2.dll")];

        let windows = library_search_order(&system, &bundled, true);
        assert_eq!(windows[0], bundled[0]);
        assert_eq!(windows[1], system[0]);

        let macos = library_search_order(&system, &bundled, false);
        assert_eq!(macos[0], system[0]);
        assert_eq!(macos[1], bundled[0]);

        assert_eq!(system_search_consults_working_directory(), cfg!(windows));
        assert_eq!(BundleLayout::host(), {
            if cfg!(target_os = "macos") {
                BundleLayout::MacOsAppBundle
            } else if cfg!(windows) {
                BundleLayout::WindowsPortable
            } else {
                BundleLayout::SystemOnly
            }
        });
    }

    #[test]
    fn capability_is_reported_without_panicking() {
        let capability = capability();
        assert!(!capability.reason.is_empty());
        assert_eq!(capability.available, capability.api_version.is_some());
        assert_eq!(capability.available, capability.library_path.is_some());
    }

    #[test]
    fn the_reported_bridge_is_the_one_the_player_actually_uses() {
        assert_eq!(render_bridge(), RenderBridge::Software);
    }
}
