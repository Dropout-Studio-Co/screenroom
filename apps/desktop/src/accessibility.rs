use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use screenroom_core::LibraryItem;

pub const CHECK_ENV: &str = "SCREENROOM_AX_CHECK";

pub const CHECK_SCREEN_ENV: &str = "SCREENROOM_AX_CHECK_SCREEN";

const MAX_PENDING: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    Home,
    Browse,
    Detail,
    Player,
    Settings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Button,
    CheckBox,
    StaticText,
    Heading,
}

impl Role {
    pub fn ax_name(self) -> &'static str {
        match self {
            Self::Button => "AXButton",
            Self::CheckBox => "AXCheckBox",
            Self::StaticText => "AXStaticText",
            Self::Heading => "AXHeading",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    GoHome,
    GoLibrary,
    GoSettings,
    Refresh,
    Close,
    ChooseFolder,
    AddFolder,
    PreviousPage,
    NextPage,
    ToggleWatchlistOnly,
    ToggleFavoritesOnly,
    ToggleAvailableOnly,
    Open(usize),
    PlayDetail,
    RestartDetail,
    ToggleFavourite,
    ToggleWatchlist,
    ToggleWatched,
    HideFromContinue,
    Settings(usize),
    TogglePlay,
    SeekBack,
    SeekForward,
    ToggleMute,
    VolumeDown,
    VolumeUp,
    ToggleFullscreen,
    MiniPlayer,
    BackFromPlayer,
    PlayNext,
    CancelNext,
    SkipSegment,
    SelectAudio(usize),
    SelectSubtitle(usize),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Control {
    pub key: String,
    pub role: Role,
    pub label: String,
    pub value: Option<String>,
    pub enabled: bool,
    pub focused: bool,
    pub action: Option<Action>,
    pub frame: Option<Frame>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Snapshot {
    pub title: String,
    pub controls: Vec<Control>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Page {
    pub index: usize,
    pub count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Filters {
    pub watchlist: bool,
    pub favourites: bool,
    pub available: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Flags {
    pub favourite: bool,
    pub watchlist: bool,
    pub watched: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Player {
    pub title: String,
    pub state: String,
    pub play: String,
    pub position: f64,
    pub duration: f64,
    pub muted: bool,
    pub volume: u8,
    pub fullscreen: bool,
    pub up_next: Option<String>,
    pub skip: Option<String>,
    pub audio: Vec<(String, bool)>,
    pub subtitles: Vec<(String, bool)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScreenFacts<'a> {
    pub surface: Surface,
    pub title: String,
    pub items: Vec<&'a LibraryItem>,
    pub focus: Option<usize>,
    pub page: Option<Page>,
    pub filters: Option<Filters>,
    pub flags: Option<Flags>,
    pub settings: Vec<String>,
    pub settings_focus: usize,
    pub player: Option<Player>,
    pub status: Option<String>,
    pub error: Option<String>,
}

impl<'a> ScreenFacts<'a> {
    pub fn new(surface: Surface, title: impl Into<String>) -> Self {
        Self {
            surface,
            title: title.into(),
            items: Vec::new(),
            focus: None,
            page: None,
            filters: None,
            flags: None,
            settings: Vec::new(),
            settings_focus: 0,
            player: None,
            status: None,
            error: None,
        }
    }
}

static SNAPSHOT: RwLock<Option<Arc<Snapshot>>> = RwLock::new(None);

static PENDING: Mutex<Vec<Action>> = Mutex::new(Vec::new());

static CHECK_ARMED: AtomicBool = AtomicBool::new(false);

pub fn install() {
    if std::env::var_os(CHECK_ENV).is_some() {
        CHECK_ARMED.store(true, Ordering::Release);
    }
    #[cfg(target_os = "macos")]
    appkit::install();
}

pub fn current() -> Arc<Snapshot> {
    match SNAPSHOT.read() {
        Ok(guard) => guard.clone().unwrap_or_default(),
        Err(poisoned) => poisoned.into_inner().clone().unwrap_or_default(),
    }
}

pub fn children() -> Vec<Control> {
    current().controls.clone()
}

pub fn drain() -> Vec<Action> {
    match PENDING.lock() {
        Ok(mut queue) => std::mem::take(&mut *queue),
        Err(poisoned) => std::mem::take(&mut *poisoned.into_inner()),
    }
}

pub fn pending() -> usize {
    match PENDING.lock() {
        Ok(queue) => queue.len(),
        Err(poisoned) => poisoned.into_inner().len(),
    }
}

pub fn enqueue(action: Action) {
    let mut queue = match PENDING.lock() {
        Ok(queue) => queue,
        Err(poisoned) => poisoned.into_inner(),
    };
    if queue.len() < MAX_PENDING {
        queue.push(action);
    }
}

pub fn publish(facts: ScreenFacts<'_>) {
    let snapshot = Arc::new(build(&facts));
    store(&snapshot);
    #[cfg(target_os = "macos")]
    {
        let guarded = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if CHECK_ARMED.swap(false, Ordering::AcqRel) {
                appkit::run_check(&snapshot);
            }
            appkit::attach(&snapshot);
        }));
        if guarded.is_err() {
            eprintln!("screenroom: the accessibility bridge stopped after a failed update");
            appkit::detach();
        }
    }
}

pub fn shutdown() {
    clear_snapshot();
    match PENDING.lock() {
        Ok(mut queue) => queue.clear(),
        Err(poisoned) => poisoned.into_inner().clear(),
    }
    #[cfg(target_os = "macos")]
    appkit::detach();
}

fn clear_snapshot() {
    match SNAPSHOT.write() {
        Ok(mut guard) => *guard = None,
        Err(poisoned) => *poisoned.into_inner() = None,
    }
}

fn store(snapshot: &Arc<Snapshot>) {
    let mut guard = match SNAPSHOT.write() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    if guard.as_ref().is_some_and(|held| **held == **snapshot) {
        return;
    }
    *guard = Some(Arc::clone(snapshot));
}

fn button(key: &str, label: impl Into<String>, action: Action) -> Control {
    Control {
        key: key.to_string(),
        role: Role::Button,
        label: label.into(),
        value: None,
        enabled: true,
        focused: false,
        action: Some(action),
        frame: None,
    }
}

fn toggle(key: &str, label: impl Into<String>, on: bool, action: Action) -> Control {
    Control {
        key: key.to_string(),
        role: Role::CheckBox,
        label: label.into(),
        value: Some(if on { "on" } else { "off" }.to_string()),
        enabled: true,
        focused: false,
        action: Some(action),
        frame: None,
    }
}

fn text(key: &str, role: Role, label: impl Into<String>, value: Option<String>) -> Control {
    Control {
        key: key.to_string(),
        role,
        label: label.into(),
        value,
        enabled: true,
        focused: false,
        action: None,
        frame: None,
    }
}

fn item_label(item: &LibraryItem) -> String {
    if item.title.trim().is_empty() {
        item.filename.clone()
    } else {
        item.title.clone()
    }
}

fn item_value(item: &LibraryItem) -> String {
    let mut parts: Vec<String> = Vec::new();
    let episode = item.episode_label();
    if !episode.is_empty() {
        parts.push(episode);
    } else if let Some(year) = item.year {
        parts.push(year.to_string());
    }
    if item.duration_ms > 0 && item.position_ms > 0 {
        let ratio = (item.position_ms as f64 / item.duration_ms as f64).clamp(0.0, 1.0);
        parts.push(format!(
            "{:.0}% watched, at {}",
            ratio * 100.0,
            crate::app::clock(item.position_ms as f64 / 1000.0)
        ));
    } else if item.position_ms > 0 {
        parts.push(format!(
            "resume at {}",
            crate::app::clock(item.position_ms as f64 / 1000.0)
        ));
    } else if item.watched {
        parts.push("watched".to_string());
    } else if !item.available {
        parts.push("unavailable".to_string());
    } else {
        parts.push("not started".to_string());
    }
    parts.join(" · ")
}

fn item_controls(facts: &ScreenFacts<'_>, controls: &mut Vec<Control>) {
    for (index, item) in facts.items.iter().enumerate() {
        let mut control = button(
            &format!("item:{index}"),
            item_label(item),
            Action::Open(index),
        );
        control.value = Some(item_value(item));
        control.enabled = item.available;
        control.focused = facts.focus == Some(index);
        controls.push(control);
    }
}

fn detail_controls(facts: &ScreenFacts<'_>, controls: &mut Vec<Control>) {
    let Some(item) = facts.items.first() else {
        return;
    };
    let resume = item.position_ms > 0;
    let mut play = button(
        "detail:play",
        if resume { "Resume" } else { "Play" },
        Action::PlayDetail,
    );
    play.enabled = item.available;
    controls.push(play);
    let mut restart = button("detail:restart", "Start over", Action::RestartDetail);
    restart.enabled = item.available;
    controls.push(restart);
    if let Some(flags) = facts.flags {
        controls.push(toggle(
            "detail:favourite",
            if flags.favourite {
                "Favourited"
            } else {
                "Favourite"
            },
            flags.favourite,
            Action::ToggleFavourite,
        ));
        controls.push(toggle(
            "detail:watchlist",
            if flags.watchlist {
                "On watchlist"
            } else {
                "Watchlist"
            },
            flags.watchlist,
            Action::ToggleWatchlist,
        ));
        controls.push(toggle(
            "detail:watched",
            if flags.watched {
                "Watched"
            } else {
                "Mark watched"
            },
            flags.watched,
            Action::ToggleWatched,
        ));
    }
    controls.push(button(
        "detail:hide-continue",
        "Hide from Continue",
        Action::HideFromContinue,
    ));
}

fn browse_controls(facts: &ScreenFacts<'_>, controls: &mut Vec<Control>) {
    if let Some(page) = facts.page {
        let mut previous = button("browse:previous", "Previous", Action::PreviousPage);
        previous.enabled = page.index > 0;
        previous.value = Some(format!("page {} of {}", page.index + 1, page.count.max(1)));
        controls.push(previous);
        let mut next = button("browse:next", "Next", Action::NextPage);
        next.enabled = page.index + 1 < page.count.max(1);
        controls.push(next);
    }
    if let Some(filters) = facts.filters {
        controls.push(toggle(
            "browse:watchlist",
            "Watchlist",
            filters.watchlist,
            Action::ToggleWatchlistOnly,
        ));
        controls.push(toggle(
            "browse:favourites",
            "Favourites",
            filters.favourites,
            Action::ToggleFavoritesOnly,
        ));
        controls.push(toggle(
            "browse:available",
            "Available only",
            filters.available,
            Action::ToggleAvailableOnly,
        ));
    }
}

fn player_controls(facts: &ScreenFacts<'_>, controls: &mut Vec<Control>) {
    let Some(player) = &facts.player else {
        return;
    };
    controls.push(button(
        "player:play",
        player.play.clone(),
        Action::TogglePlay,
    ));
    let position = format!(
        "{} of {}",
        crate::app::clock(player.position),
        crate::app::clock(player.duration)
    );
    let mut seek_back = button("player:seek-back", "Back 10 seconds", Action::SeekBack);
    seek_back.value = Some(position.clone());
    controls.push(seek_back);
    controls.push(button(
        "player:seek-forward",
        "Forward 10 seconds",
        Action::SeekForward,
    ));
    controls.push(button(
        "player:mute",
        if player.muted { "Unmute" } else { "Mute" },
        Action::ToggleMute,
    ));
    let mut quieter = button("player:volume-down", "Volume down", Action::VolumeDown);
    quieter.value = Some(format!("{}%", player.volume));
    controls.push(quieter);
    let mut louder = button("player:volume-up", "Volume up", Action::VolumeUp);
    louder.value = Some(format!("{}%", player.volume));
    controls.push(louder);
    controls.push(button(
        "player:fullscreen",
        if player.fullscreen {
            "Windowed"
        } else {
            "Fullscreen"
        },
        Action::ToggleFullscreen,
    ));
    if !player.fullscreen {
        controls.push(button("player:mini", "Mini player", Action::MiniPlayer));
        controls.push(button("player:back", "Back", Action::BackFromPlayer));
    }
    if let Some(title) = &player.up_next {
        let mut now = button("player:next", "Play now", Action::PlayNext);
        now.value = Some(title.clone());
        controls.push(now);
        controls.push(button("player:cancel-next", "Cancel", Action::CancelNext));
    }
    if let Some(skip) = &player.skip {
        controls.push(button("player:skip", skip.clone(), Action::SkipSegment));
    }
    for (index, (label, selected)) in player.audio.iter().enumerate() {
        let mut track = button(
            &format!("player:audio:{index}"),
            format!("Audio: {label}"),
            Action::SelectAudio(index),
        );
        track.value = Some(
            if *selected {
                "selected"
            } else {
                "not selected"
            }
            .to_string(),
        );
        controls.push(track);
    }
    for (index, (label, selected)) in player.subtitles.iter().enumerate() {
        let mut track = button(
            &format!("player:subtitles:{index}"),
            format!("Subtitles: {label}"),
            Action::SelectSubtitle(index),
        );
        track.value = Some(
            if *selected {
                "selected"
            } else {
                "not selected"
            }
            .to_string(),
        );
        controls.push(track);
    }
}

fn build(facts: &ScreenFacts<'_>) -> Snapshot {
    let mut controls: Vec<Control> = Vec::new();
    let fullscreen_player =
        facts.surface == Surface::Player && facts.player.as_ref().is_some_and(|p| p.fullscreen);

    if !fullscreen_player {
        controls.push(button("nav:home", "Home", Action::GoHome));
        controls.push(button("nav:library", "Library", Action::GoLibrary));
        controls.push(button("nav:settings", "Settings", Action::GoSettings));
        controls.push(button("nav:refresh", "Refresh", Action::Refresh));
        controls.push(button("nav:close", "Close", Action::Close));
    }

    controls.push(text(
        "screen:heading",
        Role::Heading,
        facts.title.clone(),
        None,
    ));

    match facts.surface {
        Surface::Home => {
            item_controls(facts, &mut controls);
            if facts.items.is_empty() {
                controls.push(button(
                    "home:choose-folder",
                    "Choose a folder…",
                    Action::ChooseFolder,
                ));
            }
        }
        Surface::Browse => {
            browse_controls(facts, &mut controls);
            item_controls(facts, &mut controls);
        }
        Surface::Detail => {
            detail_controls(facts, &mut controls);
            item_controls(facts, &mut controls);
        }
        Surface::Settings => {
            for (index, label) in facts.settings.iter().enumerate() {
                let mut action =
                    button(&format!("settings:{index}"), label, Action::Settings(index));
                action.focused = index == facts.settings_focus;
                controls.push(action);
            }
        }
        Surface::Player => {
            if let Some(player) = &facts.player {
                controls.push(text(
                    "player:title",
                    Role::Heading,
                    player.title.clone(),
                    Some(player.state.clone()),
                ));
                controls.push(text(
                    "player:position",
                    Role::StaticText,
                    "Position",
                    Some(format!(
                        "{} of {}",
                        crate::app::clock(player.position),
                        crate::app::clock(player.duration)
                    )),
                ));
            }
            player_controls(facts, &mut controls);
        }
    }

    if let Some(error) = &facts.error {
        controls.push(text("status:error", Role::StaticText, error.clone(), None));
    } else if let Some(status) = &facts.status {
        controls.push(text("status", Role::StaticText, status.clone(), None));
    }

    let mut snapshot = Snapshot {
        title: facts.title.clone(),
        controls,
    };
    if facts.surface == Surface::Home
        || facts.surface == Surface::Browse
        || facts.surface == Surface::Detail
    {
        let mut seen = false;
        for control in snapshot
            .controls
            .iter_mut()
            .filter(|c| c.key.starts_with("item:"))
        {
            if control.focused && !seen {
                seen = true;
            } else {
                control.focused = false;
            }
        }
    }
    snapshot
}

#[cfg(target_os = "macos")]
mod appkit {
    use std::ffi::{CStr, c_char, c_void};
    use std::path::PathBuf;
    use std::ptr;
    use std::sync::{Mutex, OnceLock};

    use objc2::encode::{Encode, Encoding};
    use objc2::msg_send;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject, Bool, ClassBuilder, Sel};

    use super::{CHECK_ENV, Control, Snapshot, enqueue, pending};

    #[repr(C)]
    #[derive(Clone, Copy, Default, Debug)]
    pub(crate) struct ScreenRect {
        pub x: f64,
        pub y: f64,
        pub width: f64,
        pub height: f64,
    }

    unsafe impl Encode for ScreenRect {
        const ENCODING: Encoding = Encoding::Struct(
            "CGRect",
            &[
                Encoding::Struct("CGPoint", &[Encoding::Double, Encoding::Double]),
                Encoding::Struct("CGSize", &[Encoding::Double, Encoding::Double]),
            ],
        );
    }

    struct Live {
        view: *mut AnyObject,
        elements: Vec<Retained<AnyObject>>,
        controls: Vec<Control>,
    }

    unsafe impl Send for Live {}

    static LIVE: Mutex<Option<Live>> = Mutex::new(None);

    static ELEMENT_CLASS: OnceLock<Option<&'static AnyClass>> = OnceLock::new();

    #[derive(Debug, Clone)]
    pub(crate) struct Reading {
        pub role: String,
        pub label: String,
        pub value: Option<String>,
        pub enabled: bool,
        pub focused: bool,
        pub actions: Vec<String>,
        pub frame: Option<ScreenRect>,
    }

    fn nullable_class(name: &CStr) -> Option<&'static AnyClass> {
        AnyClass::get(name)
    }

    fn class(name: &CStr) -> Option<&'static AnyClass> {
        nullable_class(name)
    }

    unsafe fn string(text: &str) -> *mut AnyObject {
        let Some(class) = class(c"NSString") else {
            return ptr::null_mut();
        };
        let bytes = text.as_bytes();
        let allocated: *mut AnyObject = unsafe { msg_send![class, alloc] };
        if allocated.is_null() {
            return ptr::null_mut();
        }
        let encoding = 4usize;
        unsafe {
            msg_send![
                allocated,
                initWithBytes: bytes.as_ptr() as *const c_void,
                length: bytes.len(),
                encoding: encoding
            ]
        }
    }

    unsafe fn read_string(object: *mut AnyObject) -> Option<String> {
        if object.is_null() {
            return None;
        }
        let bytes: *const c_char = unsafe { msg_send![object, UTF8String] };
        if bytes.is_null() {
            return None;
        }
        Some(
            unsafe { CStr::from_ptr(bytes) }
                .to_string_lossy()
                .into_owned(),
        )
    }

    unsafe fn control_of(element: *mut AnyObject) -> Option<Control> {
        let live = match LIVE.lock() {
            Ok(live) => live,
            Err(poisoned) => poisoned.into_inner(),
        };
        let live = live.as_ref()?;
        let index = live
            .elements
            .iter()
            .position(|candidate| Retained::as_ptr(candidate) == element)?;
        live.controls.get(index).cloned()
    }

    unsafe extern "C-unwind" fn is_focused(element: *mut AnyObject, _: Sel) -> Bool {
        let control = unsafe { control_of(element) };
        Bool::from(control.is_some_and(|control| control.focused))
    }

    unsafe extern "C-unwind" fn is_enabled(element: *mut AnyObject, _: Sel) -> Bool {
        let control = unsafe { control_of(element) };
        Bool::from(control.is_none_or(|control| control.enabled))
    }

    unsafe extern "C-unwind" fn action_names(element: *mut AnyObject, _: Sel) -> *mut AnyObject {
        let Some(array_class) = class(c"NSArray") else {
            return ptr::null_mut();
        };
        let actionable =
            unsafe { control_of(element) }.is_some_and(|control| control.action.is_some());
        if !actionable {
            let empty: *mut AnyObject = unsafe { msg_send![array_class, array] };
            return empty;
        }
        let press = unsafe { string("AXPress") };
        unsafe {
            let array: *mut AnyObject = msg_send![array_class, arrayWithObject: press];
            array
        }
    }

    unsafe extern "C-unwind" fn perform_press(element: *mut AnyObject, _: Sel) -> Bool {
        match unsafe { control_of(element) }.and_then(|control| control.action) {
            Some(action) => {
                enqueue(action);
                Bool::YES
            }
            None => Bool::NO,
        }
    }

    fn element_class() -> Option<&'static AnyClass> {
        *ELEMENT_CLASS.get_or_init(|| {
            if let Some(existing) = nullable_class(c"ScreenroomAccessibilityElement") {
                return Some(existing);
            }
            let superclass = nullable_class(c"NSAccessibilityElement")?;
            let mut builder = ClassBuilder::new(c"ScreenroomAccessibilityElement", superclass)?;
            unsafe {
                builder.add_method::<AnyObject, _>(
                    Sel::register(c"isAccessibilityFocused"),
                    is_focused as unsafe extern "C-unwind" fn(*mut AnyObject, Sel) -> Bool,
                );
                builder.add_method::<AnyObject, _>(
                    Sel::register(c"accessibilityEnabled"),
                    is_enabled as unsafe extern "C-unwind" fn(*mut AnyObject, Sel) -> Bool,
                );
                builder.add_method::<AnyObject, _>(
                    Sel::register(c"accessibilityActionNames"),
                    action_names
                        as unsafe extern "C-unwind" fn(*mut AnyObject, Sel) -> *mut AnyObject,
                );
                builder.add_method::<AnyObject, _>(
                    Sel::register(c"accessibilityPerformPress"),
                    perform_press as unsafe extern "C-unwind" fn(*mut AnyObject, Sel) -> Bool,
                );
            }
            Some(builder.register())
        })
    }

    pub(crate) fn install() {
        let _ = element_class();
    }

    unsafe fn application() -> *mut AnyObject {
        let Some(class) = class(c"NSApplication") else {
            return ptr::null_mut();
        };
        unsafe { msg_send![class, sharedApplication] }
    }

    unsafe fn is_window(object: *mut AnyObject) -> bool {
        let Some(window_class) = class(c"NSWindow") else {
            return false;
        };
        if object.is_null() {
            return false;
        }
        let is_kind: Bool = unsafe { msg_send![object, isKindOfClass: window_class] };
        is_kind.as_bool()
    }

    unsafe fn content_view() -> Option<(*mut AnyObject, *mut AnyObject)> {
        let application = unsafe { application() };
        if application.is_null() {
            return None;
        }
        let windows: *mut AnyObject = unsafe { msg_send![application, windows] };
        if windows.is_null() {
            return None;
        }
        let winit_view = class(c"WinitView");
        let count: isize = unsafe { msg_send![windows, count] };
        let mut fallback: Option<(*mut AnyObject, *mut AnyObject)> = None;
        for index in 0..count {
            let window: *mut AnyObject = unsafe { msg_send![windows, objectAtIndex: index] };
            if !unsafe { is_window(window) } {
                continue;
            }
            let view: *mut AnyObject = unsafe { msg_send![window, contentView] };
            if view.is_null() {
                continue;
            }
            if let Some(winit_view) = winit_view {
                let is_winit: Bool = unsafe { msg_send![view, isKindOfClass: winit_view] };
                if is_winit.as_bool() {
                    return Some((window, view));
                }
            }
            if fallback.is_none() {
                fallback = Some((window, view));
            }
        }
        fallback
    }

    unsafe fn main_screen_height() -> f64 {
        let Some(screen_class) = class(c"NSScreen") else {
            return 0.0;
        };
        let screens: *mut AnyObject = unsafe { msg_send![screen_class, screens] };
        if screens.is_null() {
            return 0.0;
        }
        let count: isize = unsafe { msg_send![screens, count] };
        if count == 0 {
            return 0.0;
        }
        let first: isize = 0;
        let screen: *mut AnyObject = unsafe { msg_send![screens, objectAtIndex: first] };
        let frame: ScreenRect = unsafe { msg_send![screen, frame] };
        frame.height
    }

    unsafe fn screen_frame(window: *mut AnyObject, view: *mut AnyObject) -> ScreenRect {
        let bounds: ScreenRect = unsafe { msg_send![view, bounds] };
        let no_view: *mut AnyObject = ptr::null_mut();
        let in_window: ScreenRect =
            unsafe { msg_send![view, convertRect: bounds, toView: no_view] };
        let on_screen: ScreenRect = unsafe { msg_send![window, convertRectToScreen: in_window] };
        let height = unsafe { main_screen_height() };
        ScreenRect {
            x: on_screen.x,
            y: height - (on_screen.y + on_screen.height),
            width: on_screen.width,
            height: on_screen.height,
        }
    }

    unsafe fn create_element(
        superclass: &AnyClass,
        control: &Control,
        view: *mut AnyObject,
        frame: ScreenRect,
    ) -> Option<Retained<AnyObject>> {
        let allocated: *mut AnyObject = unsafe { msg_send![superclass, alloc] };
        if allocated.is_null() {
            return None;
        }
        let initialized: *mut AnyObject = unsafe { msg_send![allocated, init] };
        let element = unsafe { Retained::from_raw(initialized) }?;
        unsafe { fill(&element, control, view, frame) };
        Some(element)
    }

    unsafe fn fill(
        element: &AnyObject,
        control: &Control,
        view: *mut AnyObject,
        frame: ScreenRect,
    ) {
        let role = unsafe { string(control.role.ax_name()) };
        let label = unsafe { string(&control.label) };
        unsafe {
            let _: () = msg_send![element, setAccessibilityRole: role];
            let _: () = msg_send![element, setAccessibilityLabel: label];
            let _: () = msg_send![element, setAccessibilityFrame: frame];
            let _: () = msg_send![element, setAccessibilityEnabled: Bool::from(control.enabled)];
            let _: () = msg_send![element, setAccessibilityParent: view];
            let identifier = string(&control.key);
            let _: () = msg_send![element, setAccessibilityIdentifier: identifier];
            match &control.value {
                Some(value) => {
                    let value = string(value);
                    let _: () = msg_send![element, setAccessibilityValue: value];
                }
                None => {
                    let no_value: *mut AnyObject = ptr::null_mut();
                    let _: () = msg_send![element, setAccessibilityValue: no_value];
                }
            }
        }
    }

    pub(crate) fn attach(snapshot: &Snapshot) {
        let Some(superclass) = element_class() else {
            return;
        };
        let Some((window, view)) = (unsafe { content_view() }) else {
            return;
        };
        let frame = unsafe { screen_frame(window, view) };
        let mut elements: Vec<Retained<AnyObject>> = {
            let mut live = match LIVE.lock() {
                Ok(live) => live,
                Err(poisoned) => poisoned.into_inner(),
            };
            match live.take() {
                Some(previous) if previous.view == view => previous.elements,
                _ => Vec::new(),
            }
        };
        let wanted = snapshot.controls.len();
        while elements.len() < wanted {
            let Some(control) = snapshot.controls.get(elements.len()) else {
                break;
            };
            let Some(element) = (unsafe { create_element(superclass, control, view, frame) })
            else {
                break;
            };
            elements.push(element);
        }
        for (control, element) in snapshot.controls.iter().zip(elements.iter()) {
            unsafe { fill(element, control, view, frame) };
        }
        let shown = wanted.min(elements.len());
        if let Some(array_class) = class(c"NSMutableArray") {
            unsafe {
                let array: *mut AnyObject = msg_send![array_class, array];
                if !array.is_null() {
                    for element in elements.iter().take(shown) {
                        let _: () = msg_send![array, addObject: &**element];
                    }
                    let _: () = msg_send![view, setAccessibilityChildren: array];
                }
            }
        }
        let mut live = match LIVE.lock() {
            Ok(live) => live,
            Err(poisoned) => poisoned.into_inner(),
        };
        *live = Some(Live {
            view,
            elements,
            controls: snapshot.controls.clone(),
        });
    }

    pub(crate) fn detach() {
        let previous = {
            let mut live = match LIVE.lock() {
                Ok(live) => live,
                Err(poisoned) => poisoned.into_inner(),
            };
            live.take()
        };
        let Some(previous) = previous else {
            return;
        };
        let Some(array_class) = class(c"NSMutableArray") else {
            return;
        };
        unsafe {
            let array: *mut AnyObject = msg_send![array_class, array];
            if !array.is_null() {
                let _: () = msg_send![previous.view, setAccessibilityChildren: array];
            }
        }
    }

    unsafe fn responds(object: *mut AnyObject, selector: &CStr) -> bool {
        if object.is_null() {
            return false;
        }
        let selector = Sel::register(selector);
        let responds: Bool = unsafe { msg_send![object, respondsToSelector: selector] };
        responds.as_bool()
    }

    #[derive(Clone, Copy)]
    enum Attribute {
        Role,
        Label,
        Value,
        Actions,
    }

    unsafe fn read_object(object: *mut AnyObject, attribute: Attribute) -> *mut AnyObject {
        let selector = match attribute {
            Attribute::Role => c"accessibilityRole",
            Attribute::Label => c"accessibilityLabel",
            Attribute::Value => c"accessibilityValue",
            Attribute::Actions => c"accessibilityActionNames",
        };
        if !unsafe { responds(object, selector) } {
            return ptr::null_mut();
        }
        match attribute {
            Attribute::Role => unsafe { msg_send![object, accessibilityRole] },
            Attribute::Label => unsafe { msg_send![object, accessibilityLabel] },
            Attribute::Value => unsafe { msg_send![object, accessibilityValue] },
            Attribute::Actions => unsafe { msg_send![object, accessibilityActionNames] },
        }
    }

    unsafe fn read_attribute(object: *mut AnyObject, attribute: Attribute) -> Option<String> {
        let value = unsafe { read_object(object, attribute) };
        unsafe { read_string(value) }
    }

    #[derive(Clone, Copy)]
    enum Flag {
        Enabled,
        Focused,
    }

    unsafe fn read_flag(object: *mut AnyObject, flag: Flag) -> Option<bool> {
        let selector = match flag {
            Flag::Enabled => c"accessibilityEnabled",
            Flag::Focused => c"isAccessibilityFocused",
        };
        if !unsafe { responds(object, selector) } {
            return None;
        }
        match flag {
            Flag::Enabled => {
                let value: Bool = unsafe { msg_send![object, accessibilityEnabled] };
                Some(value.as_bool())
            }
            Flag::Focused => {
                let value: Bool = unsafe { msg_send![object, isAccessibilityFocused] };
                Some(value.as_bool())
            }
        }
    }

    pub(crate) fn read_children() -> (Option<String>, Vec<Reading>) {
        let Some(view) = (unsafe { content_view() }).map(|(_, view)| view) else {
            return (None, Vec::new());
        };
        let class_name = match class(c"WinitView") {
            Some(winit_view) => {
                let is_winit: Bool = unsafe { msg_send![view, isKindOfClass: winit_view] };
                Some(if is_winit.as_bool() { "winit" } else { "other" }.to_string())
            }
            None => None,
        };
        let children: *mut AnyObject = unsafe { msg_send![view, accessibilityChildren] };
        let mut readings = Vec::new();
        if children.is_null() {
            return (class_name, readings);
        }
        let count: isize = unsafe { msg_send![children, count] };
        for index in 0..count {
            let element: *mut AnyObject = unsafe { msg_send![children, objectAtIndex: index] };
            if element.is_null() {
                continue;
            }
            let role = unsafe { read_attribute(element, Attribute::Role) };
            let label = unsafe { read_attribute(element, Attribute::Label) };
            let value = unsafe { read_attribute(element, Attribute::Value) };
            let enabled = unsafe { read_flag(element, Flag::Enabled) }.unwrap_or(true);
            let focused = unsafe { read_flag(element, Flag::Focused) }.unwrap_or(false);
            let names = unsafe { read_object(element, Attribute::Actions) };
            let frame = if unsafe { responds(element, c"accessibilityFrame") } {
                Some(unsafe { msg_send![element, accessibilityFrame] })
            } else {
                None
            };
            let mut actions = Vec::new();
            if !names.is_null() {
                let action_count: isize = unsafe { msg_send![names, count] };
                for action_index in 0..action_count {
                    let action: *mut AnyObject =
                        unsafe { msg_send![names, objectAtIndex: action_index] };
                    if let Some(name) = unsafe { read_string(action) } {
                        actions.push(name);
                    }
                }
            }
            readings.push(Reading {
                role: role.unwrap_or_default(),
                label: label.unwrap_or_default(),
                value,
                enabled,
                focused,
                actions,
                frame,
            });
        }
        (class_name, readings)
    }

    pub(crate) fn read_child_count() -> usize {
        read_children().1.len()
    }

    pub(crate) fn press(index: usize) -> bool {
        let held = {
            let live = match LIVE.lock() {
                Ok(live) => live,
                Err(poisoned) => poisoned.into_inner(),
            };
            let Some(live) = live.as_ref() else {
                return false;
            };
            let Some(element) = live.elements.get(index) else {
                return false;
            };
            unsafe { Retained::retain(Retained::as_ptr(element) as *mut AnyObject) }
        };
        let Some(element) = held else {
            return false;
        };
        let performed: Bool = unsafe { msg_send![&*element, accessibilityPerformPress] };
        performed.as_bool()
    }

    fn receipt_path() -> Option<PathBuf> {
        std::env::var_os(CHECK_ENV).map(PathBuf::from)
    }

    pub(crate) fn run_check(snapshot: &Snapshot) {
        let Some(path) = receipt_path() else {
            return;
        };
        let body = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| check_body(snapshot)));
        let (text, code) = match body {
            Ok(lines) => (lines, 0),
            Err(_) => (
                vec![
                    format!("surface {}", snapshot.title),
                    "check failed".to_string(),
                ],
                1,
            ),
        };
        let text = text.join("\n") + "\n";
        if let Err(error) = std::fs::write(&path, text) {
            eprintln!(
                "screenroom: could not write the accessibility check to {}: {error}",
                path.display()
            );
        }
        std::process::exit(code);
    }

    fn check_body(snapshot: &Snapshot) -> Vec<String> {
        let baseline = read_child_count();
        attach(snapshot);
        let (class_name, readings) = read_children();
        let all_labels = readings
            .iter()
            .all(|reading| !reading.label.trim().is_empty());
        let labels_match = readings.len() == snapshot.controls.len()
            && readings
                .iter()
                .zip(snapshot.controls.iter())
                .all(|(reading, control)| {
                    reading.label == control.label && reading.role == control.role.ax_name()
                });
        let focus_match = readings
            .iter()
            .zip(snapshot.controls.iter())
            .all(|(reading, control)| reading.focused == control.focused);
        let focused_label = readings
            .iter()
            .find(|reading| reading.focused)
            .map(|reading| reading.label.clone())
            .unwrap_or_default();
        let focused_key = snapshot
            .controls
            .iter()
            .find(|control| control.focused)
            .map(|control| control.key.clone())
            .unwrap_or_default();
        let focus_index = snapshot.controls.iter().position(|control| control.focused);
        let pressed = focus_index.map(press).unwrap_or(false);
        let queued = pending();
        super::drain();

        let mut lines = vec![
            format!("surface {}", snapshot.title),
            format!("baseline_children {baseline}"),
            format!("view_class {}", class_name.unwrap_or_default()),
            format!("children {}", readings.len()),
            format!("expected {}", snapshot.controls.len()),
            format!("all_labels_non_empty {all_labels}"),
            format!("labels_match {labels_match}"),
            format!("focus_match {focus_match}"),
            format!("focused_key {focused_key}"),
            format!("focused_label {focused_label}"),
            format!("pressed {pressed}"),
            format!("queued {queued}"),
        ];
        for reading in &readings {
            let frame = reading
                .frame
                .map(|frame| {
                    format!(
                        "{:.0},{:.0},{:.0},{:.0}",
                        frame.x, frame.y, frame.width, frame.height
                    )
                })
                .unwrap_or_default();
            lines.push(format!(
                "child {}|{}|{}|enabled={}|focused={}|actions={}|frame={}",
                reading.role,
                reading.label,
                reading.value.clone().unwrap_or_default(),
                reading.enabled,
                reading.focused,
                reading.actions.join(","),
                frame
            ));
        }
        super::shutdown();
        lines.push(format!("after_shutdown_children {}", read_child_count()));
        lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use screenroom_core::MediaKind;

    fn item(id: usize, title: &str) -> LibraryItem {
        LibraryItem {
            id: format!("file-{id}"),
            work_id: format!("work-{id}"),
            edition_id: format!("edition-{id}"),
            source_id: "source".to_string(),
            title: title.to_string(),
            filename: format!("{title}.mkv"),
            relative_path: format!("{title}.mkv"),
            kind: MediaKind::Movie,
            overview: String::new(),
            episode_title: String::new(),
            episode_overview: String::new(),
            genres: Vec::new(),
            year: Some(2000),
            season: None,
            episode: None,
            episode_end: None,
            duration_ms: 0,
            position_ms: 0,
            added_ms: 0,
            available: true,
            watched: false,
            favorite: false,
            watchlisted: false,
            manual_match: false,
            metadata_origin: String::new(),
        }
    }

    fn home() -> Snapshot {
        let items = [item(1, "First"), item(2, "Second"), item(3, "Third")];
        let mut facts = ScreenFacts::new(Surface::Home, "Home");
        facts.items = items.iter().collect();
        facts.focus = Some(1);
        build(&facts)
    }

    #[test]
    fn nothing_is_published_before_the_application_does() {
        clear_snapshot();
        assert!(children().is_empty());
        assert!(current().controls.is_empty());
    }

    #[test]
    fn every_published_control_carries_a_label() {
        let mut browse = ScreenFacts::new(Surface::Browse, "Library");
        let items = [item(1, "First")];
        browse.items = items.iter().collect();
        browse.page = Some(Page { index: 0, count: 3 });
        browse.filters = Some(Filters {
            watchlist: true,
            favourites: false,
            available: true,
        });

        let mut settings = ScreenFacts::new(Surface::Settings, "Settings");
        settings.settings = vec!["Back up now".to_string(), "Grant a folder".to_string()];

        let mut player = ScreenFacts::new(Surface::Player, "Player");
        player.player = Some(Player {
            title: "Film".to_string(),
            state: "Playing".to_string(),
            play: "Pause".to_string(),
            position: 12.0,
            duration: 60.0,
            muted: false,
            volume: 50,
            fullscreen: false,
            up_next: Some("Next film".to_string()),
            skip: Some("Skip intro".to_string()),
            audio: vec![("English".to_string(), true)],
            subtitles: vec![("None".to_string(), true)],
        });

        for snapshot in [home(), build(&browse), build(&settings), build(&player)] {
            assert!(
                !snapshot.controls.is_empty(),
                "{} published nothing",
                snapshot.title
            );
            for control in &snapshot.controls {
                assert!(
                    !control.label.trim().is_empty(),
                    "{} published a control with no label",
                    snapshot.title
                );
                assert!(!control.key.trim().is_empty());
            }
        }
    }

    #[test]
    fn the_published_items_are_the_ones_the_screen_shows() {
        let snapshot = home();
        let items: Vec<&Control> = snapshot
            .controls
            .iter()
            .filter(|control| control.key.starts_with("item:"))
            .collect();
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].label, "First");
        assert_eq!(items[2].label, "Third");
        assert_eq!(items[0].action, Some(Action::Open(0)));
        assert_eq!(items[2].action, Some(Action::Open(2)));
    }

    #[test]
    fn the_focused_item_is_the_one_the_application_marks() {
        let snapshot = home();
        let focused: Vec<&Control> = snapshot.controls.iter().filter(|c| c.focused).collect();
        assert_eq!(focused.len(), 1);
        assert_eq!(focused[0].label, "Second");
        assert_eq!(focused[0].action, Some(Action::Open(1)));
    }

    #[test]
    fn a_focus_index_past_the_end_marks_nothing() {
        let items = [item(1, "First")];
        let mut facts = ScreenFacts::new(Surface::Browse, "Library");
        facts.items = items.iter().collect();
        facts.focus = Some(9);
        let snapshot = build(&facts);
        assert!(snapshot.controls.iter().all(|control| !control.focused));
    }

    #[test]
    fn settings_follow_the_actions_the_screen_offers() {
        let labels = vec!["Back up now".to_string(), "Grant a folder".to_string()];
        let mut facts = ScreenFacts::new(Surface::Settings, "Settings");
        facts.settings = labels.clone();
        facts.settings_focus = 1;
        let snapshot = build(&facts);
        let published: Vec<&Control> = snapshot
            .controls
            .iter()
            .filter(|control| control.key.starts_with("settings:"))
            .collect();
        assert_eq!(published.len(), labels.len());
        assert_eq!(published[0].label, labels[0]);
        assert_eq!(published[0].action, Some(Action::Settings(0)));
        assert!(published[1].focused);
        assert!(!published[0].focused);
    }

    #[test]
    fn the_navigation_is_announced_once_per_screen() {
        let snapshot = home();
        for key in [
            "nav:home",
            "nav:library",
            "nav:settings",
            "nav:refresh",
            "nav:close",
        ] {
            let matching = snapshot.controls.iter().filter(|c| c.key == key).count();
            assert_eq!(matching, 1, "{key} was not announced exactly once");
        }
    }

    #[test]
    fn a_press_is_queued_and_drained_once() {
        assert_eq!(pending(), 0);
        enqueue(Action::GoHome);
        assert_eq!(pending(), 1);
        assert_eq!(drain(), vec![Action::GoHome]);
        assert_eq!(pending(), 0);
    }

    #[test]
    fn a_screen_with_nothing_on_it_still_announces_its_navigation() {
        let snapshot = build(&ScreenFacts::new(Surface::Browse, "Library"));
        assert_eq!(
            snapshot
                .controls
                .iter()
                .filter(|c| c.key.starts_with("item:"))
                .count(),
            0
        );
        assert!(snapshot.controls.iter().any(|c| c.key == "nav:home"));
    }

    #[test]
    fn a_fullscreen_player_does_not_announce_a_header_it_hides() {
        let mut facts = ScreenFacts::new(Surface::Player, "Player");
        facts.player = Some(Player {
            title: "Film".to_string(),
            state: "Playing".to_string(),
            play: "Pause".to_string(),
            position: 0.0,
            duration: 60.0,
            muted: false,
            volume: 40,
            fullscreen: true,
            up_next: None,
            skip: None,
            audio: Vec::new(),
            subtitles: Vec::new(),
        });
        let snapshot = build(&facts);
        assert!(snapshot.controls.iter().all(|c| !c.key.starts_with("nav:")));
        assert!(snapshot.controls.iter().any(|c| c.key == "player:play"));
    }

    #[test]
    fn a_stale_control_has_no_action_to_run() {
        let snapshot = home();
        let keys: Vec<&str> = snapshot.controls.iter().map(|c| c.key.as_str()).collect();
        assert!(!keys.contains(&"settings:99"));
        assert!(keys.contains(&"item:2"));
    }
}
