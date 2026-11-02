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
    RadioButton,
    Slider,
    StaticText,
    Heading,
}

impl Role {
    pub fn ax_name(self) -> &'static str {
        match self {
            Self::Button => "AXButton",
            Self::CheckBox => "AXCheckBox",
            Self::RadioButton => "AXRadioButton",
            Self::Slider => "AXSlider",
            Self::StaticText => "AXStaticText",
            Self::Heading => "AXHeading",
        }
    }

    pub fn presses(self) -> bool {
        matches!(self, Self::Button | Self::CheckBox | Self::RadioButton)
    }

    pub fn adjusts(self) -> bool {
        self == Self::Slider
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    GoHome,
    GoLibrary,
    GoSettings,
    Refresh,
    Search,
    ChooseFolder,
    AddFolder,
    PreviousPage,
    NextPage,
    FilterAll,
    FilterFilms,
    FilterSeries,
    ToggleUnwatched,
    ToggleWatchlistOnly,
    ToggleFavoritesOnly,
    ToggleAvailableOnly,
    Sort(usize),
    Genre(usize),
    Open(usize),
    PlayHero,
    OpenHero,
    PlayDetail,
    RestartDetail,
    BackFromDetail,
    ToggleFavourite,
    ToggleWatchlist,
    ToggleWatched,
    HideFromContinue,
    Season(usize),
    SettingsSection(usize),
    Settings(usize),
    SettingsStep(usize, bool),
    Extra(usize),
    TogglePlay,
    SeekBack,
    SeekForward,
    ToggleMute,
    VolumeDown,
    VolumeUp,
    ToggleFullscreen,
    CancelScan,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Adjust {
    pub increase: Action,
    pub decrease: Action,
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
    pub adjust: Option<Adjust>,
    pub frame: Option<Frame>,
}

impl Control {
    pub fn press(
        key: impl Into<String>,
        label: impl Into<String>,
        action: Option<Action>,
        enabled: bool,
    ) -> Self {
        Self {
            key: key.into(),
            role: Role::Button,
            label: label.into(),
            value: None,
            enabled,
            focused: false,
            action,
            adjust: None,
            frame: None,
        }
    }

    pub fn state(
        key: impl Into<String>,
        label: impl Into<String>,
        on: bool,
        action: Option<Action>,
    ) -> Self {
        Self {
            key: key.into(),
            role: Role::CheckBox,
            label: label.into(),
            value: Some(mark(on)),
            enabled: true,
            focused: false,
            action,
            adjust: None,
            frame: None,
        }
    }

    pub fn choice(
        key: impl Into<String>,
        label: impl Into<String>,
        chosen: bool,
        action: Option<Action>,
        enabled: bool,
    ) -> Self {
        Self {
            key: key.into(),
            role: Role::RadioButton,
            label: label.into(),
            value: Some(mark(chosen)),
            enabled,
            focused: false,
            action,
            adjust: None,
            frame: None,
        }
    }

    pub fn measure(
        key: impl Into<String>,
        label: impl Into<String>,
        value: impl Into<String>,
        adjust: Option<Adjust>,
    ) -> Self {
        Self {
            key: key.into(),
            role: Role::Slider,
            label: label.into(),
            value: Some(value.into()),
            enabled: adjust.is_some(),
            focused: false,
            action: None,
            adjust,
            frame: None,
        }
    }

    pub fn note(key: impl Into<String>, label: impl Into<String>, value: Option<String>) -> Self {
        Self {
            key: key.into(),
            role: Role::StaticText,
            label: label.into(),
            value,
            enabled: true,
            focused: false,
            action: None,
            adjust: None,
            frame: None,
        }
    }

    pub fn heading(key: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            role: Role::Heading,
            label: label.into(),
            value: None,
            enabled: true,
            focused: false,
            action: None,
            adjust: None,
            frame: None,
        }
    }

    fn with_value(mut self, value: impl Into<String>) -> Self {
        self.value = Some(value.into());
        self
    }

    fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}

fn mark(on: bool) -> String {
    if on { "on" } else { "off" }.to_string()
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
    pub kind: usize,
    pub unwatched: bool,
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
pub enum Listing<'a> {
    None,
    Flat(Vec<&'a LibraryItem>),
    Sectioned(Vec<(String, Vec<&'a LibraryItem>)>),
}

impl Listing<'_> {
    pub fn is_empty(&self) -> bool {
        match self {
            Self::None => true,
            Self::Flat(items) => items.is_empty(),
            Self::Sectioned(sections) => sections.iter().all(|(_, items)| items.is_empty()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hero {
    pub title: String,
    pub eyebrow: String,
    pub detail: String,
    pub primary: String,
    pub secondary: String,
    pub playable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryFacts {
    pub total_line: String,
    pub query: String,
    pub sorts: Vec<String>,
    pub sort: usize,
    pub genres: Vec<String>,
    pub genre: Option<usize>,
    pub source_line: String,
    pub empty: Option<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetailFacts {
    pub kind: String,
    pub metadata: String,
    pub synopsis: String,
    pub facts: Vec<String>,
    pub available: bool,
    pub seasons: Vec<i64>,
    pub season: i64,
    pub episode_line: String,
    pub shows_episodes: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SettingRowKind {
    Toggle(bool),
    Slider {
        value: String,
        adjust: Option<Adjust>,
    },
    Value(String),
    Action(bool),
}

#[derive(Debug, Clone, PartialEq)]
pub struct SettingRow {
    pub label: String,
    pub hint: Option<String>,
    pub kind: SettingRowKind,
    pub action: Option<Action>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SettingsFacts {
    pub sections: Vec<String>,
    pub current: usize,
    pub rows: Vec<SettingRow>,
    pub focus: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Player {
    pub title: String,
    pub play: String,
    pub position: f64,
    pub duration: f64,
    pub muted: bool,
    pub volume: u8,
    pub fullscreen: bool,
    pub chrome: bool,
    pub preparing: bool,
    pub width: f64,
    pub episode: Option<String>,
    pub output: Option<String>,
    pub failure: Option<String>,
    pub up_next: Option<String>,
    pub skip: Option<String>,
    pub skip_note: Option<String>,
    pub resume_note: Option<String>,
    pub audio: Vec<(String, bool)>,
    pub subtitles: Vec<(String, bool)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScreenFacts<'a> {
    pub surface: Surface,
    pub title: String,
    pub heading: String,
    pub hero: Option<Hero>,
    pub listing: Listing<'a>,
    pub subject: Option<&'a LibraryItem>,
    pub focus: Option<usize>,
    pub page: Option<Page>,
    pub filters: Option<Filters>,
    pub library: Option<LibraryFacts>,
    pub detail: Option<DetailFacts>,
    pub flags: Option<Flags>,
    pub settings: Option<SettingsFacts>,
    pub player: Option<Player>,
    pub scan: Option<String>,
    pub status: Option<String>,
    pub error: Option<String>,
    pub extras: Vec<Control>,
}

impl<'a> ScreenFacts<'a> {
    pub fn new(surface: Surface, title: impl Into<String>) -> Self {
        let title = title.into();
        Self {
            surface,
            heading: title.clone(),
            title,
            hero: None,
            listing: Listing::None,
            subject: None,
            focus: None,
            page: None,
            filters: None,
            library: None,
            detail: None,
            flags: None,
            settings: None,
            player: None,
            scan: None,
            status: None,
            error: None,
            extras: Vec::new(),
        }
    }
}

static SNAPSHOT: RwLock<Option<Arc<Snapshot>>> = RwLock::new(None);

static PENDING: Mutex<Vec<Action>> = Mutex::new(Vec::new());

static CHECK_ARMED: AtomicBool = AtomicBool::new(false);

static SYSTEM_REDUCE_MOTION: std::sync::OnceLock<bool> = std::sync::OnceLock::new();

pub fn system_reduce_motion() -> bool {
    *SYSTEM_REDUCE_MOTION.get_or_init(read_system_reduce_motion)
}

#[cfg(target_os = "macos")]
fn read_system_reduce_motion() -> bool {
    use objc2::msg_send;
    use objc2::runtime::{AnyClass, AnyObject};

    let Some(class) = AnyClass::get(c"NSWorkspace") else {
        return false;
    };
    let workspace: *mut AnyObject = unsafe { msg_send![class, sharedWorkspace] };
    if workspace.is_null() {
        return false;
    }
    let reduce: bool = unsafe { msg_send![workspace, accessibilityDisplayShouldReduceMotion] };
    reduce
}

#[cfg(not(target_os = "macos"))]
fn read_system_reduce_motion() -> bool {
    false
}

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

pub fn snapshot(facts: &ScreenFacts<'_>) -> Arc<Snapshot> {
    Arc::new(build(facts))
}

pub fn publish_snapshot(snapshot: Arc<Snapshot>) {
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

pub fn item_label(item: &LibraryItem) -> String {
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

fn item_control(index: usize, item: &LibraryItem, focused: bool) -> Control {
    Control::press(
        format!("item:{index}"),
        item_label(item),
        Some(Action::Open(index)),
        true,
    )
    .with_value(item_value(item))
    .enabled(item.available)
    .focused(focused)
}

fn listing_controls<'a>(
    listing: &'a Listing<'a>,
    focus: Option<usize>,
    controls: &mut Vec<Control>,
) {
    match listing {
        Listing::None => {}
        Listing::Flat(items) => {
            for (index, item) in items.iter().enumerate() {
                controls.push(item_control(index, item, focus == Some(index)));
            }
        }
        Listing::Sectioned(sections) => {
            let mut offset = 0;
            for (rail, (label, items)) in sections.iter().enumerate() {
                controls.push(
                    Control::heading(format!("rail:{rail}"), label.clone())
                        .with_value(items.len().to_string()),
                );
                for (column, item) in items.iter().enumerate() {
                    let position = offset + column;
                    controls.push(item_control(position, item, focus == Some(position)));
                }
                offset += items.len();
            }
        }
    }
}

fn home_controls(facts: &ScreenFacts<'_>, controls: &mut Vec<Control>) {
    match &facts.hero {
        Some(hero) => {
            controls.push(Control::heading("home:hero:title", hero.title.clone()));
            if !hero.eyebrow.trim().is_empty() {
                controls.push(Control::note(
                    "home:hero:reason",
                    hero.eyebrow.clone(),
                    None,
                ));
            }
            if !hero.detail.trim().is_empty() {
                controls.push(Control::note("home:hero:detail", hero.detail.clone(), None));
            }
            controls.push(Control::press(
                "home:hero:play",
                hero.primary.clone(),
                Some(Action::PlayHero),
                hero.playable,
            ));
            controls.push(Control::press(
                "home:hero:details",
                hero.secondary.clone(),
                Some(Action::OpenHero),
                true,
            ));
        }
        None => controls.push(Control::heading("home:title", facts.heading.clone())),
    }

    listing_controls(&facts.listing, facts.focus, controls);

    if facts.hero.is_none() && facts.listing.is_empty() {
        controls.push(Control::press(
            "home:choose-folder",
            "Choose a folder…",
            Some(Action::ChooseFolder),
            true,
        ));
        controls.push(Control::press(
            "home:refresh",
            "Refresh",
            Some(Action::Refresh),
            true,
        ));
    }
}

fn library_controls(facts: &ScreenFacts<'_>, controls: &mut Vec<Control>) {
    controls.push(Control::heading("library:title", facts.heading.clone()));

    if let Some(library) = &facts.library {
        controls.push(Control::note(
            "library:count",
            library.total_line.clone(),
            None,
        ));
        controls.push(Control::note(
            "library:search",
            "Search",
            Some(if library.query.trim().is_empty() {
                "empty".to_string()
            } else {
                library.query.clone()
            }),
        ));
        for (index, sort) in library.sorts.iter().enumerate() {
            controls.push(Control::choice(
                format!("library:sort:{index}"),
                format!("Sort by {}", sort.to_lowercase()),
                index == library.sort,
                Some(Action::Sort(index)),
                true,
            ));
        }
    }

    if let Some(filters) = &facts.filters {
        for (index, label) in ["All", "Films", "Series"].iter().enumerate() {
            controls.push(Control::choice(
                format!("library:kind:{index}"),
                *label,
                filters.kind == index,
                Some(match index {
                    0 => Action::FilterAll,
                    1 => Action::FilterFilms,
                    _ => Action::FilterSeries,
                }),
                true,
            ));
        }
        controls.push(Control::state(
            "library:unwatched",
            "Unwatched",
            filters.unwatched,
            Some(Action::ToggleUnwatched),
        ));
        controls.push(Control::state(
            "library:available",
            "Available offline",
            filters.available,
            Some(Action::ToggleAvailableOnly),
        ));
        controls.push(Control::state(
            "library:watchlist",
            "Watchlist",
            filters.watchlist,
            Some(Action::ToggleWatchlistOnly),
        ));
        controls.push(Control::state(
            "library:favourites",
            "Favourites",
            filters.favourites,
            Some(Action::ToggleFavoritesOnly),
        ));
    }

    if let Some(library) = &facts.library
        && !library.genres.is_empty()
    {
        controls.push(Control::choice(
            "library:genre:0",
            GENRE_ANY,
            library.genre.is_none(),
            Some(Action::Genre(0)),
            true,
        ));
        for (index, genre) in library.genres.iter().enumerate() {
            controls.push(Control::choice(
                format!("library:genre:{}", index + 1),
                genre.clone(),
                library.genre == Some(index),
                Some(Action::Genre(index + 1)),
                true,
            ));
        }
    }

    if let Some(library) = &facts.library {
        controls.push(Control::note(
            "library:source",
            library.source_line.clone(),
            None,
        ));
    }

    listing_controls(&facts.listing, facts.focus, controls);

    if let Some((headline, detail)) = facts
        .library
        .as_ref()
        .and_then(|library| library.empty.as_ref())
    {
        controls.push(Control::note("library:empty", headline.clone(), None));
        controls.push(Control::note("library:empty-detail", detail.clone(), None));
    }

    if let Some(page) = facts.page
        && page.count > 1
    {
        let shown = format!("page {} of {}", page.index + 1, page.count);
        controls.push(
            Control::press(
                "library:previous",
                "Previous",
                Some(Action::PreviousPage),
                page.index > 0,
            )
            .with_value(shown.clone()),
        );
        controls.push(
            Control::press(
                "library:next",
                "Next",
                Some(Action::NextPage),
                page.index + 1 < page.count,
            )
            .with_value(shown),
        );
    }
}

const GENRE_ANY: &str = "Any genre";

fn detail_controls(facts: &ScreenFacts<'_>, controls: &mut Vec<Control>) {
    controls.push(Control::press(
        "detail:back",
        "Back to library",
        Some(Action::BackFromDetail),
        true,
    ));

    if let Some(detail) = &facts.detail {
        controls.push(Control::note("detail:kind", detail.kind.clone(), None));
    }

    controls.push(Control::heading("detail:title", facts.heading.clone()));

    if let Some(detail) = &facts.detail
        && !detail.metadata.trim().is_empty()
    {
        controls.push(Control::note(
            "detail:metadata",
            detail.metadata.clone(),
            None,
        ));
    }

    if let Some(item) = facts.subject {
        let (label, _) = crate::ui::screens::detail::primary_action(item);
        controls.push(Control::press(
            "detail:play",
            label,
            Some(Action::PlayDetail),
            item.available,
        ));
    }

    if let Some(flags) = facts.flags {
        controls.push(Control::state(
            "detail:watchlist",
            "Watchlist",
            flags.watchlist,
            Some(Action::ToggleWatchlist),
        ));
        controls.push(Control::state(
            "detail:watched",
            "Watched",
            flags.watched,
            Some(Action::ToggleWatched),
        ));
    }

    if let Some(detail) = &facts.detail {
        if !detail.synopsis.trim().is_empty() {
            controls.push(Control::note(
                "detail:synopsis",
                detail.synopsis.clone(),
                None,
            ));
        }
        for (index, fact) in detail.facts.iter().enumerate() {
            controls.push(Control::note(
                format!("detail:fact:{index}"),
                fact.clone(),
                None,
            ));
        }
        if !detail.available {
            controls.push(Control::note("detail:unavailable", UNREADABLE, None));
        }
        if detail.shows_episodes {
            controls.push(Control::heading(
                "detail:season",
                format!("Season {}", detail.season),
            ));
            controls.push(Control::note(
                "detail:episode-count",
                detail.episode_line.clone(),
                None,
            ));
            if detail.seasons.len() > 1 {
                for (index, season) in detail.seasons.iter().enumerate() {
                    let current = *season == detail.season;
                    controls.push(Control::choice(
                        format!("detail:season:{index}"),
                        format!("Season {season}"),
                        current,
                        (!current).then_some(Action::Season(index)),
                        !current,
                    ));
                }
            }
        }
    }

    listing_controls(&facts.listing, facts.focus, controls);
}

const UNREADABLE: &str = "This file is not readable right now. Relink its source and rescan.";

fn settings_controls(facts: &ScreenFacts<'_>, controls: &mut Vec<Control>) {
    controls.push(Control::heading("settings:title", facts.heading.clone()));

    let Some(settings) = &facts.settings else {
        return;
    };

    for (index, label) in settings.sections.iter().enumerate() {
        let current = index == settings.current;
        let mut section = Control::press(
            format!("settings:section:{index}"),
            label.clone(),
            (!current).then_some(Action::SettingsSection(index)),
            !current,
        );
        if current {
            section = section.with_value("current");
        }
        controls.push(section);
    }

    if let Some(label) = settings.sections.get(settings.current) {
        controls.push(Control::heading("settings:section-title", label.clone()));
    }

    for (index, row) in settings.rows.iter().enumerate() {
        let focused = index == settings.focus;
        let key = format!("settings:{index}");
        let control = match &row.kind {
            SettingRowKind::Toggle(on) => Control::state(key, row.label.clone(), *on, row.action),
            SettingRowKind::Slider { value, adjust } => {
                Control::measure(key, row.label.clone(), value.clone(), *adjust)
            }
            SettingRowKind::Value(value) => {
                Control::note(key, row.label.clone(), Some(value.clone()))
            }
            SettingRowKind::Action(enabled) => {
                Control::press(key, row.label.clone(), row.action, *enabled)
            }
        };
        controls.push(control.focused(focused));
        if let Some(hint) = &row.hint
            && !hint.trim().is_empty()
        {
            controls.push(Control::note(
                format!("settings:{index}:hint"),
                hint.clone(),
                None,
            ));
        }
    }
}

fn player_controls(facts: &ScreenFacts<'_>, controls: &mut Vec<Control>) {
    let Some(player) = &facts.player else {
        controls.push(Control::heading("player:title", facts.heading.clone()));
        return;
    };

    controls.push(Control::press(
        "player:back",
        "Back",
        Some(Action::BackFromPlayer),
        true,
    ));
    controls.push(Control::heading("player:title", player.title.clone()));

    if !player.chrome {
        return;
    }

    if let Some(episode) = &player.episode {
        controls.push(Control::note("player:episode", episode.clone(), None));
    }
    if let Some(output) = &player.output {
        controls.push(Control::note("player:output", output.clone(), None));
    }
    if let Some(failure) = &player.failure {
        controls.push(Control::note("player:failure", failure.clone(), None));
    } else if player.preparing {
        controls.push(Control::note(
            "player:preparing",
            "Preparing streams…",
            None,
        ));
    }

    controls.push(Control::measure(
        "player:position",
        "Position",
        format!(
            "{} of {}",
            crate::app::clock(player.position),
            crate::app::clock(player.duration)
        ),
        Some(Adjust {
            increase: Action::SeekForward,
            decrease: Action::SeekBack,
        }),
    ));
    controls.push(Control::press(
        "player:play",
        player.play.clone(),
        Some(Action::TogglePlay),
        true,
    ));
    controls.push(Control::press(
        "player:seek-back",
        "Back 10 seconds",
        Some(Action::SeekBack),
        true,
    ));
    controls.push(Control::press(
        "player:seek-forward",
        "Forward 10 seconds",
        Some(Action::SeekForward),
        true,
    ));

    if crate::ui::screens::player::control_visible(
        player.width as f32,
        crate::ui::screens::player::Control::Volume,
    ) {
        controls.push(Control::press(
            "player:mute",
            if player.muted { "Unmute" } else { "Mute" },
            Some(Action::ToggleMute),
            true,
        ));
        controls.push(Control::measure(
            "player:volume",
            "Volume",
            format!("{}%", player.volume),
            Some(Adjust {
                increase: Action::VolumeUp,
                decrease: Action::VolumeDown,
            }),
        ));
    }

    if crate::ui::screens::player::control_visible(
        player.width as f32,
        crate::ui::screens::player::Control::Subtitles,
    ) {
        if player.subtitles.is_empty() {
            controls.push(Control::note("player:subtitles", "Subtitles · none", None));
        } else {
            for (index, (label, selected)) in player.subtitles.iter().enumerate() {
                controls.push(Control::choice(
                    format!("player:subtitles:{index}"),
                    format!("Subtitles: {label}"),
                    *selected,
                    (!*selected).then_some(Action::SelectSubtitle(index)),
                    !*selected,
                ));
            }
        }
    }

    if crate::ui::screens::player::control_visible(
        player.width as f32,
        crate::ui::screens::player::Control::Audio,
    ) {
        if player.audio.is_empty() {
            controls.push(Control::note("player:audio", "Audio · none", None));
        } else {
            for (index, (label, selected)) in player.audio.iter().enumerate() {
                controls.push(Control::choice(
                    format!("player:audio:{index}"),
                    format!("Audio: {label}"),
                    *selected,
                    (!*selected).then_some(Action::SelectAudio(index)),
                    !*selected,
                ));
            }
        }
    }

    controls.push(Control::press(
        "player:fullscreen",
        if player.fullscreen {
            "Windowed"
        } else {
            "Fullscreen"
        },
        Some(Action::ToggleFullscreen),
        true,
    ));
    controls.push(Control::note(
        "player:mark-hint",
        crate::ui::screens::player::MARK_HINT,
        None,
    ));

    if let Some(up) = &player.up_next {
        controls.push(Control::note(
            "player:up-next",
            format!("Up next: {up}"),
            None,
        ));
        controls.push(Control::press(
            "player:next",
            "Play now",
            Some(Action::PlayNext),
            true,
        ));
        controls.push(Control::press(
            "player:cancel-next",
            "Cancel",
            Some(Action::CancelNext),
            true,
        ));
    }

    if let Some(skip) = &player.skip {
        controls.push(Control::press(
            "player:skip",
            skip.clone(),
            Some(Action::SkipSegment),
            true,
        ));
        if let Some(note) = &player.skip_note {
            controls.push(Control::note("player:skip-note", note.clone(), None));
        }
    }

    if let Some(note) = &player.resume_note {
        controls.push(Control::note("player:resumed", note.clone(), None));
    }
}

fn announcement(snapshot: &Snapshot) -> Option<&str> {
    snapshot
        .controls
        .iter()
        .find(|control| control.key == "status" || control.key == "status:error")
        .map(|control| control.label.as_str())
        .filter(|label| !label.trim().is_empty())
        .filter(|label| !label.starts_with(SCAN_PROGRESS))
}

const SCAN_PROGRESS: &str = "Scanning\u{2026}";

fn build(facts: &ScreenFacts<'_>) -> Snapshot {
    let mut controls: Vec<Control> = Vec::new();

    if facts.surface != Surface::Player {
        for (key, label, action, screen) in [
            ("nav:home", "Home", Action::GoHome, Surface::Home),
            ("nav:library", "Library", Action::GoLibrary, Surface::Browse),
            (
                "nav:settings",
                "Settings",
                Action::GoSettings,
                Surface::Settings,
            ),
        ] {
            let mut destination = Control::press(key, label, Some(action), true);
            if facts.surface == screen {
                destination = destination.with_value("current");
            }
            controls.push(destination);
        }
        controls.push(Control::press(
            "nav:refresh",
            "Refresh",
            Some(Action::Refresh),
            true,
        ));
        controls.push(Control::press(
            "nav:search",
            "Search",
            Some(Action::Search),
            true,
        ));
    }

    if let Some(progress) = &facts.scan {
        controls.push(Control::note(
            "scan:progress",
            "Scanning",
            Some(progress.clone()),
        ));
        controls.push(Control::press(
            "scan:cancel",
            "Cancel scan",
            Some(Action::CancelScan),
            true,
        ));
    }

    match facts.surface {
        Surface::Home => home_controls(facts, &mut controls),
        Surface::Browse => library_controls(facts, &mut controls),
        Surface::Detail => detail_controls(facts, &mut controls),
        Surface::Settings => settings_controls(facts, &mut controls),
        Surface::Player => player_controls(facts, &mut controls),
    }

    controls.extend(facts.extras.iter().cloned());

    if let Some(error) = &facts.error {
        controls.push(Control::note("status:error", error.clone(), None));
    } else if let Some(status) = &facts.status {
        controls.push(Control::note("status", status.clone(), None));
    }

    let mut snapshot = Snapshot {
        title: facts.title.clone(),
        controls,
    };
    let mut claimed = false;
    for control in snapshot.controls.iter_mut() {
        if control.focused && claimed {
            control.focused = false;
        } else if control.focused {
            claimed = true;
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

    #[link(name = "AppKit", kind = "framework")]
    unsafe extern "C" {
        static NSAccessibilityAnnouncementRequestedNotification: *mut AnyObject;
        fn NSAccessibilityPostNotificationWithUserInfo(
            element: *mut AnyObject,
            notification: *mut AnyObject,
            user_info: *mut AnyObject,
        );
    }

    static ANNOUNCED: Mutex<String> = Mutex::new(String::new());

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
        let Some(array_class) = class(c"NSMutableArray") else {
            return ptr::null_mut();
        };
        let array: *mut AnyObject = unsafe { msg_send![array_class, array] };
        if array.is_null() {
            return array;
        }
        let Some(control) = (unsafe { control_of(element) }) else {
            return array;
        };
        if control.action.is_some() {
            let press = unsafe { string("AXPress") };
            if !press.is_null() {
                unsafe {
                    let _: () = msg_send![array, addObject: &*press];
                }
            }
        }
        if control.adjust.is_some() {
            for name in ["AXIncrement", "AXDecrement"] {
                let action = unsafe { string(name) };
                if !action.is_null() {
                    unsafe {
                        let _: () = msg_send![array, addObject: &*action];
                    }
                }
            }
        }
        array
    }

    unsafe extern "C-unwind" fn increment(element: *mut AnyObject, _: Sel) -> Bool {
        match unsafe { control_of(element) }.and_then(|control| control.adjust.map(|a| a.increase))
        {
            Some(action) => {
                enqueue(action);
                Bool::YES
            }
            None => Bool::NO,
        }
    }

    unsafe extern "C-unwind" fn decrement(element: *mut AnyObject, _: Sel) -> Bool {
        match unsafe { control_of(element) }.and_then(|control| control.adjust.map(|a| a.decrease))
        {
            Some(action) => {
                enqueue(action);
                Bool::YES
            }
            None => Bool::NO,
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
                builder.add_method::<AnyObject, _>(
                    Sel::register(c"accessibilityIncrement"),
                    increment as unsafe extern "C-unwind" fn(*mut AnyObject, Sel) -> Bool,
                );
                builder.add_method::<AnyObject, _>(
                    Sel::register(c"accessibilityDecrement"),
                    decrement as unsafe extern "C-unwind" fn(*mut AnyObject, Sel) -> Bool,
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

    fn announce(snapshot: &Snapshot, view: *mut AnyObject) {
        let Some(text) = super::announcement(snapshot) else {
            return;
        };
        let mut last = match ANNOUNCED.lock() {
            Ok(last) => last,
            Err(poisoned) => poisoned.into_inner(),
        };
        if *last == text {
            return;
        }
        last.clear();
        last.push_str(text);
        drop(last);
        unsafe { post_announcement(text, view) };
    }

    unsafe fn post_announcement(text: &str, view: *mut AnyObject) {
        let Some(dictionary_class) = class(c"NSDictionary") else {
            return;
        };
        let message = unsafe { string(text) };
        let key = unsafe { string("AXAnnouncement") };
        if message.is_null() || key.is_null() {
            return;
        }
        let dictionary: *mut AnyObject =
            unsafe { msg_send![dictionary_class, dictionaryWithObject: &*message, forKey: &*key] };
        if dictionary.is_null() {
            return;
        }
        let notification = unsafe { NSAccessibilityAnnouncementRequestedNotification };
        if notification.is_null() {
            return;
        }
        unsafe { NSAccessibilityPostNotificationWithUserInfo(view, notification, dictionary) };
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
        announce(snapshot, view);
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
        let values_match = readings.len() == snapshot.controls.len()
            && readings
                .iter()
                .zip(snapshot.controls.iter())
                .all(|(reading, control)| reading.value == control.value);
        let enabled_match = readings
            .iter()
            .zip(snapshot.controls.iter())
            .all(|(reading, control)| reading.enabled == control.enabled);
        let press_match =
            readings
                .iter()
                .zip(snapshot.controls.iter())
                .all(|(reading, control)| {
                    reading.actions.iter().any(|name| name == "AXPress") == control.action.is_some()
                });
        let adjust_match =
            readings
                .iter()
                .zip(snapshot.controls.iter())
                .all(|(reading, control)| {
                    let increment = reading.actions.iter().any(|name| name == "AXIncrement");
                    let decrement = reading.actions.iter().any(|name| name == "AXDecrement");
                    increment == control.adjust.is_some() && decrement == control.adjust.is_some()
                });
        let focused_count = readings.iter().filter(|reading| reading.focused).count();
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
            format!("values_match {values_match}"),
            format!("enabled_match {enabled_match}"),
            format!("press_actions_match {press_match}"),
            format!("adjust_actions_match {adjust_match}"),
            format!("focus_match {focus_match}"),
            format!("focused_count {focused_count}"),
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

    fn keys(snapshot: &Snapshot) -> Vec<&str> {
        snapshot
            .controls
            .iter()
            .map(|control| control.key.as_str())
            .collect()
    }

    fn find<'a>(snapshot: &'a Snapshot, key: &str) -> &'a Control {
        snapshot
            .controls
            .iter()
            .find(|control| control.key == key)
            .unwrap_or_else(|| panic!("{key} was not announced"))
    }

    fn position(snapshot: &Snapshot, key: &str) -> usize {
        snapshot
            .controls
            .iter()
            .position(|control| control.key == key)
            .unwrap_or_else(|| panic!("{key} was not announced"))
    }

    fn home() -> Snapshot {
        let items = [item(1, "First"), item(2, "Second"), item(3, "Third")];
        let mut facts = ScreenFacts::new(Surface::Home, "Home");
        facts.heading = "First".to_string();
        facts.hero = Some(Hero {
            title: "First".to_string(),
            eyebrow: "CONTINUE WATCHING".to_string(),
            detail: "2000 · Film".to_string(),
            primary: "Resume".to_string(),
            secondary: "Details".to_string(),
            playable: true,
        });
        facts.listing = Listing::Sectioned(vec![
            ("Continue watching".to_string(), vec![&items[0]]),
            ("Films".to_string(), vec![&items[1], &items[2]]),
        ]);
        facts.focus = Some(1);
        build(&facts)
    }

    fn library() -> Snapshot {
        let items = [item(1, "First"), item(2, "Second")];
        let mut facts = ScreenFacts::new(Surface::Browse, "Library");
        facts.heading = "Library".to_string();
        facts.listing = Listing::Flat(items.iter().collect());
        facts.focus = Some(0);
        facts.page = Some(Page { index: 0, count: 1 });
        facts.filters = Some(Filters {
            kind: 1,
            unwatched: true,
            watchlist: false,
            favourites: false,
            available: false,
        });
        facts.library = Some(LibraryFacts {
            empty: None,
            total_line: "2 titles".to_string(),
            query: "the".to_string(),
            sorts: vec!["Title".to_string(), "Recently added".to_string()],
            sort: 1,
            genres: vec!["Drama".to_string()],
            genre: Some(0),
            source_line: "1 folder · 2 files · never scanned".to_string(),
        });
        build(&facts)
    }

    fn detail() -> Snapshot {
        let mut subject = item(9, "Aardvark");
        subject.kind = MediaKind::Series;
        subject.season = Some(1);
        subject.episode = Some(1);
        let episodes = [item(1, "Aardvark"), item(2, "Aardvark")];
        let mut facts = ScreenFacts::new(Surface::Detail, "Details");
        facts.heading = "Aardvark".to_string();
        facts.subject = Some(&subject);
        facts.listing = Listing::Flat(episodes.iter().collect());
        facts.focus = Some(1);
        facts.flags = Some(Flags {
            favourite: false,
            watchlist: true,
            watched: false,
        });
        facts.detail = Some(DetailFacts {
            kind: "SERIES".to_string(),
            metadata: "2000 · 1 season · 2 episodes".to_string(),
            synopsis: "A synopsis.".to_string(),
            facts: vec!["series".to_string(), "S01E01".to_string()],
            available: true,
            seasons: vec![1, 2],
            season: 1,
            episode_line: "2 episodes".to_string(),
            shows_episodes: true,
        });
        facts.extras = vec![
            Control::press(
                "detail:restart",
                "Start over",
                Some(Action::RestartDetail),
                true,
            ),
            Control::state(
                "detail:favourite",
                "Favourite",
                false,
                Some(Action::ToggleFavourite),
            ),
            Control::press(
                "detail:hide-continue",
                "Hide from Continue",
                Some(Action::HideFromContinue),
                true,
            ),
        ];
        build(&facts)
    }

    fn settings() -> Snapshot {
        let mut facts = ScreenFacts::new(Surface::Settings, "Settings");
        facts.heading = "Settings".to_string();
        facts.settings = Some(SettingsFacts {
            sections: vec!["Playback".to_string(), "Data and backups".to_string()],
            current: 0,
            focus: 1,
            rows: vec![
                SettingRow {
                    label: "Play the next episode automatically".to_string(),
                    hint: Some("A gap in a season stops the chain.".to_string()),
                    kind: SettingRowKind::Toggle(true),
                    action: Some(Action::Settings(0)),
                },
                SettingRow {
                    label: "Subtitle size".to_string(),
                    hint: None,
                    kind: SettingRowKind::Slider {
                        value: "125%".to_string(),
                        adjust: Some(Adjust {
                            increase: Action::Settings(1),
                            decrease: Action::Settings(1),
                        }),
                    },
                    action: None,
                },
                SettingRow {
                    label: "Library".to_string(),
                    hint: None,
                    kind: SettingRowKind::Value("5 files".to_string()),
                    action: None,
                },
            ],
        });
        build(&facts)
    }

    fn player(width: f64) -> Snapshot {
        let mut facts = ScreenFacts::new(Surface::Player, "Player");
        facts.heading = "Film".to_string();
        facts.player = Some(Player {
            title: "Film".to_string(),
            play: "Pause".to_string(),
            position: 12.0,
            duration: 60.0,
            muted: false,
            volume: 50,
            fullscreen: false,
            chrome: true,
            preparing: false,
            width,
            episode: Some("S01E01".to_string()),
            output: Some("1080p".to_string()),
            failure: None,
            up_next: Some("Next film".to_string()),
            skip: Some("Skip intro".to_string()),
            skip_note: Some("A segment you marked starts here.".to_string()),
            resume_note: Some("resumed from 0:12".to_string()),
            audio: vec![("English".to_string(), true), ("French".to_string(), false)],
            subtitles: vec![("Off".to_string(), true)],
        });
        build(&facts)
    }

    #[test]
    fn nothing_is_published_before_the_application_does() {
        clear_snapshot();
        assert!(children().is_empty());
        assert!(current().controls.is_empty());
    }

    #[test]
    fn every_published_control_carries_a_label_and_a_unique_key() {
        for snapshot in [home(), library(), detail(), settings(), player(1440.0)] {
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
            let mut seen: Vec<&str> = keys(&snapshot);
            seen.sort_unstable();
            let unique = seen.len();
            seen.dedup();
            assert_eq!(
                unique,
                seen.len(),
                "{} announced a key twice",
                snapshot.title
            );
        }
    }

    #[test]
    fn a_role_that_presses_carries_a_press_and_a_slider_carries_none() {
        for snapshot in [home(), library(), detail(), settings(), player(1440.0)] {
            for control in &snapshot.controls {
                if control.action.is_some() {
                    assert!(
                        control.role.presses(),
                        "{} gave {} an action its role cannot perform",
                        snapshot.title,
                        control.key
                    );
                }
                if control.adjust.is_some() {
                    assert!(
                        control.role.adjusts(),
                        "{} gave {} an adjustment its role cannot perform",
                        snapshot.title,
                        control.key
                    );
                }
                if matches!(control.role, Role::StaticText | Role::Heading) {
                    assert_eq!(
                        control.action, None,
                        "{} is a control but is not drawn as one",
                        control.key
                    );
                    assert_eq!(control.adjust, None);
                }
            }
        }
    }

    #[test]
    fn the_navigation_is_announced_once_and_carries_no_close_it_does_not_draw() {
        for snapshot in [home(), library(), detail(), settings()] {
            for key in [
                "nav:home",
                "nav:library",
                "nav:settings",
                "nav:refresh",
                "nav:search",
            ] {
                let matching = snapshot.controls.iter().filter(|c| c.key == key).count();
                assert_eq!(matching, 1, "{key} was not announced exactly once");
            }
            assert!(
                snapshot.controls.iter().all(|c| c.key != "nav:close"),
                "{} announced a Close button the shell never draws",
                snapshot.title
            );
        }

        let snapshot = library();
        let current = find(&snapshot, "nav:library");
        assert_eq!(current.value.as_deref(), Some("current"));
        assert_eq!(find(&snapshot, "nav:home").value, None);
    }

    #[test]
    fn the_player_announces_no_shell_because_it_draws_none() {
        let snapshot = player(1440.0);
        assert!(snapshot.controls.iter().all(|c| !c.key.starts_with("nav:")));
        assert!(snapshot.controls.iter().any(|c| c.key == "player:play"));
    }

    #[test]
    fn home_announces_the_hero_action_the_rails_and_their_titles_in_order() {
        let snapshot = home();
        let hero = position(&snapshot, "home:hero:play");
        let first_rail = position(&snapshot, "rail:0");
        let first_card = position(&snapshot, "item:0");
        let second_rail = position(&snapshot, "rail:1");

        assert!(hero < first_rail, "the hero is drawn above the rails");
        assert!(
            first_rail < first_card,
            "a rail names itself before its cards"
        );
        assert!(first_card < second_rail);

        assert_eq!(find(&snapshot, "home:hero:play").role, Role::Button);
        assert_eq!(find(&snapshot, "home:hero:play").label, "Resume");
        assert_eq!(
            find(&snapshot, "home:hero:details").action,
            Some(Action::OpenHero)
        );
        assert_eq!(find(&snapshot, "rail:0").label, "Continue watching");
        assert_eq!(find(&snapshot, "rail:1").value.as_deref(), Some("2"));
        assert_eq!(find(&snapshot, "item:2").label, "Third");
        assert_eq!(find(&snapshot, "item:2").action, Some(Action::Open(2)));
    }

    #[test]
    fn an_empty_home_still_offers_the_two_things_it_draws() {
        let facts = ScreenFacts::new(Surface::Home, "Home");
        let snapshot = build(&facts);
        assert_eq!(
            find(&snapshot, "home:choose-folder").action,
            Some(Action::ChooseFolder)
        );
        assert_eq!(
            find(&snapshot, "home:refresh").action,
            Some(Action::Refresh)
        );
    }

    #[test]
    fn the_library_announces_the_chips_it_draws_and_the_pager_it_does_not() {
        let snapshot = library();
        assert_eq!(
            find(&snapshot, "library:kind:1").value.as_deref(),
            Some("on")
        );
        assert_eq!(
            find(&snapshot, "library:kind:0").value.as_deref(),
            Some("off")
        );
        assert_eq!(
            find(&snapshot, "library:unwatched").value.as_deref(),
            Some("on")
        );
        assert_eq!(
            find(&snapshot, "library:sort:1").value.as_deref(),
            Some("on")
        );
        assert_eq!(
            find(&snapshot, "library:sort:0").value.as_deref(),
            Some("off")
        );
        assert_eq!(
            find(&snapshot, "library:search").value.as_deref(),
            Some("the")
        );
        assert_eq!(
            find(&snapshot, "library:source").label,
            "1 folder · 2 files · never scanned"
        );
        assert!(
            snapshot
                .controls
                .iter()
                .all(|c| !c.key.starts_with("library:previous")),
            "a one-page library draws no pager"
        );

        let chips = position(&snapshot, "library:kind:0");
        let grid = position(&snapshot, "item:0");
        assert!(chips < grid, "the chips sit above the grid");
    }

    #[test]
    fn a_paged_library_announces_the_pager_after_the_grid() {
        let items = [item(1, "First")];
        let mut facts = ScreenFacts::new(Surface::Browse, "Library");
        facts.listing = Listing::Flat(items.iter().collect());
        facts.page = Some(Page { index: 1, count: 4 });
        let snapshot = build(&facts);
        assert!(position(&snapshot, "item:0") < position(&snapshot, "library:next"));
        assert!(find(&snapshot, "library:previous").enabled);
        assert!(find(&snapshot, "library:next").enabled);
    }

    #[test]
    fn the_detail_announces_the_back_affordance_and_the_title_before_the_actions() {
        let snapshot = detail();
        assert!(position(&snapshot, "detail:back") < position(&snapshot, "detail:title"));
        assert!(position(&snapshot, "detail:title") < position(&snapshot, "detail:play"));
        assert!(position(&snapshot, "detail:play") < position(&snapshot, "detail:synopsis"));
        assert!(position(&snapshot, "detail:season") < position(&snapshot, "item:0"));
        assert!(
            position(&snapshot, "item:1") < position(&snapshot, "detail:restart"),
            "the extra actions are drawn below the episode list"
        );
        assert_eq!(
            find(&snapshot, "detail:back").action,
            Some(Action::BackFromDetail)
        );
        assert_eq!(find(&snapshot, "detail:title").role, Role::Heading);
        assert_eq!(
            find(&snapshot, "detail:watchlist").value.as_deref(),
            Some("on")
        );
        assert_eq!(
            find(&snapshot, "detail:favourite").value.as_deref(),
            Some("off")
        );
        assert_eq!(
            find(&snapshot, "detail:season:1").action,
            Some(Action::Season(1))
        );
        assert!(!find(&snapshot, "detail:season:0").enabled);
    }

    #[test]
    fn a_film_detail_announces_no_episode_it_does_not_list() {
        let subject = item(1, "Zulu");
        let mut facts = ScreenFacts::new(Surface::Detail, "Details");
        facts.subject = Some(&subject);
        facts.listing = Listing::None;
        facts.detail = Some(DetailFacts {
            kind: "FILM".to_string(),
            metadata: "2000 · 100 min".to_string(),
            synopsis: String::new(),
            facts: Vec::new(),
            available: true,
            seasons: Vec::new(),
            season: 0,
            episode_line: String::new(),
            shows_episodes: false,
        });
        let snapshot = build(&facts);
        assert!(
            snapshot
                .controls
                .iter()
                .all(|c| !c.key.starts_with("item:")),
            "a film has no episode list to announce"
        );
        assert!(snapshot.controls.iter().all(|c| c.key != "detail:season"));
    }

    #[test]
    fn settings_announces_the_rail_the_rows_and_what_each_control_is() {
        let snapshot = settings();
        assert!(position(&snapshot, "settings:section:0") < position(&snapshot, "settings:0"));
        assert_eq!(
            find(&snapshot, "settings:section:0").value.as_deref(),
            Some("current")
        );
        assert!(!find(&snapshot, "settings:section:0").enabled);
        assert!(find(&snapshot, "settings:section:1").enabled);
        assert_eq!(find(&snapshot, "settings:0").role, Role::CheckBox);
        assert_eq!(find(&snapshot, "settings:0").value.as_deref(), Some("on"));
        assert_eq!(
            find(&snapshot, "settings:0:hint").label,
            "A gap in a season stops the chain."
        );
        assert_eq!(find(&snapshot, "settings:1").role, Role::Slider);
        assert_eq!(find(&snapshot, "settings:1").value.as_deref(), Some("125%"));
        assert_eq!(find(&snapshot, "settings:2").role, Role::StaticText);
        assert_eq!(
            find(&snapshot, "settings:2").action,
            None,
            "a value is not a button"
        );
        assert_eq!(
            find(&snapshot, "settings:2").value.as_deref(),
            Some("5 files")
        );
        assert!(find(&snapshot, "settings:1").focused);
    }

    #[test]
    fn the_player_announces_its_two_sliders_and_no_step_button_it_does_not_draw() {
        let snapshot = player(1440.0);
        assert_eq!(find(&snapshot, "player:position").role, Role::Slider);
        assert_eq!(
            find(&snapshot, "player:position").value.as_deref(),
            Some("0:12 of 1:00")
        );
        assert_eq!(find(&snapshot, "player:volume").role, Role::Slider);
        assert_eq!(
            find(&snapshot, "player:volume").value.as_deref(),
            Some("50%")
        );
        for phantom in ["player:volume-up", "player:volume-down"] {
            assert!(
                snapshot.controls.iter().all(|c| c.key != phantom),
                "{phantom} is announced but never drawn"
            );
        }
        assert_eq!(
            find(&snapshot, "player:audio:0").value.as_deref(),
            Some("on")
        );
        assert_eq!(find(&snapshot, "player:audio:1").role, Role::RadioButton);
        assert!(find(&snapshot, "player:audio:1").enabled);
        assert_eq!(find(&snapshot, "player:audio:0").action, None);
        assert_eq!(find(&snapshot, "player:skip").label, "Skip intro");
    }

    #[test]
    fn the_player_drops_exactly_the_controls_the_window_drops() {
        let wide = player(1440.0);
        let standard = player(1000.0);
        let narrow = player(800.0);

        assert!(wide.controls.iter().any(|c| c.key == "player:subtitles:0"));
        assert!(
            standard
                .controls
                .iter()
                .all(|c| c.key != "player:subtitles:0")
        );
        assert!(standard.controls.iter().any(|c| c.key == "player:audio:0"));
        assert!(narrow.controls.iter().all(|c| c.key != "player:audio:0"));
        assert!(narrow.controls.iter().all(|c| c.key != "player:volume"));
        assert!(narrow.controls.iter().any(|c| c.key == "player:position"));
    }

    #[test]
    fn a_receded_chrome_announces_the_film_and_no_control_it_hides() {
        let mut facts = ScreenFacts::new(Surface::Player, "Player");
        facts.player = Some(Player {
            title: "Film".to_string(),
            play: "Pause".to_string(),
            position: 0.0,
            duration: 60.0,
            muted: false,
            volume: 50,
            fullscreen: false,
            chrome: false,
            preparing: false,
            width: 1440.0,
            episode: None,
            output: None,
            failure: None,
            up_next: None,
            skip: None,
            skip_note: None,
            resume_note: None,
            audio: Vec::new(),
            subtitles: Vec::new(),
        });
        let snapshot = build(&facts);
        assert_eq!(
            snapshot.controls.len(),
            2,
            "back and the title, nothing else"
        );
        assert_eq!(keys(&snapshot), vec!["player:back", "player:title"]);
    }

    #[test]
    fn the_focused_control_is_the_one_the_application_marks() {
        let snapshot = home();
        let focused: Vec<&Control> = snapshot.controls.iter().filter(|c| c.focused).collect();
        assert_eq!(focused.len(), 1);
        assert_eq!(focused[0].key, "item:1");
        assert_eq!(focused[0].label, "Second");
    }

    #[test]
    fn a_focus_index_past_the_end_marks_nothing() {
        let items = [item(1, "First")];
        let mut facts = ScreenFacts::new(Surface::Browse, "Library");
        facts.listing = Listing::Flat(items.iter().collect());
        facts.focus = Some(9);
        let snapshot = build(&facts);
        assert!(snapshot.controls.iter().all(|control| !control.focused));
    }

    #[test]
    fn a_second_focus_cannot_be_published() {
        let items = [item(1, "First")];
        let mut facts = ScreenFacts::new(Surface::Detail, "Details");
        facts.subject = Some(&items[0]);
        facts.listing = Listing::Flat(items.iter().collect());
        facts.focus = Some(0);
        facts.extras = vec![
            Control::press(
                "detail:restart",
                "Start over",
                Some(Action::RestartDetail),
                true,
            )
            .focused(true),
        ];
        let snapshot = build(&facts);
        assert_eq!(snapshot.controls.iter().filter(|c| c.focused).count(), 1);
    }

    #[test]
    fn the_status_line_is_published_last_and_only_when_it_has_something_to_say() {
        let mut facts = ScreenFacts::new(Surface::Browse, "Library");
        assert!(facts.status.is_none());
        facts.status = Some("Scanned 2 folders".to_string());
        let snapshot = build(&facts);
        assert_eq!(
            snapshot.controls.last().map(|c| c.key.as_str()),
            Some("status")
        );

        facts.status = None;
        facts.error = Some("scan_failed: no permission".to_string());
        let snapshot = build(&facts);
        assert_eq!(
            snapshot.controls.last().map(|c| c.key.as_str()),
            Some("status:error")
        );
    }

    #[test]
    fn the_status_line_is_the_one_thing_worth_announcing() {
        let mut facts = ScreenFacts::new(Surface::Browse, "Library");
        assert_eq!(announcement(&build(&facts)), None);

        facts.status = Some("Scanned 2 folders".to_string());
        assert_eq!(announcement(&build(&facts)), Some("Scanned 2 folders"));

        facts.error = Some("backup_failed: disk full".to_string());
        assert_eq!(
            announcement(&build(&facts)),
            Some("backup_failed: disk full"),
            "an error is announced instead of the status it replaced"
        );

        facts.error = Some("   ".to_string());
        assert_eq!(
            announcement(&build(&facts)),
            None,
            "a blank line is silence"
        );

        facts.error = None;
        facts.status = Some("Scanning… 240 entries, 240 imported".to_string());
        assert_eq!(
            announcement(&build(&facts)),
            None,
            "a scan that counts up is read from the strip, not announced twice a second"
        );

        facts.status = Some("Scan finished: 1,200 entries".to_string());
        assert_eq!(
            announcement(&build(&facts)),
            Some("Scan finished: 1,200 entries")
        );
    }

    #[test]
    fn a_press_is_queued_and_drained_once() {
        assert_eq!(pending(), 0);
        enqueue(Action::GoHome);
        assert_eq!(pending(), 1);
        assert_eq!(drain(), vec![Action::GoHome]);
        assert_eq!(pending(), 0);
    }
}
