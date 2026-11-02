use std::collections::HashMap;

use iced::overlay::menu::Style as MenuStyle;
use iced::widget::{
    Id, Space, button, column, container, image, mouse_area, operation, pick_list, row, scrollable,
    stack, text, text_input,
};
use iced::{
    Alignment, Background, Border, Color, ContentFit, Element, Length, Padding, Shadow, Task,
    Theme, mouse,
};
use screenroom_core::LibraryItem;

use crate::app::{KindFilter, Message, SortChoice, WatchFilter};
use crate::design;
use crate::ui::components::poster::{CARD_GAP, CARD_WIDTH, card_columns, truncate};
use crate::ui::icons::{self, Icon};
use crate::ui::route::{FocusPos, Nav};
use crate::ui::theme::{self, Breakpoint, Gap, Radius};

pub const PAGE_TILES: usize = 24;

const TILE_TITLE_CHARS: usize = 26;

const CAPTION_CHARS: usize = 34;

const ART_RATIO: f32 = 9.0 / 16.0;

const PROGRESS_HEIGHT: f32 = 2.0;

const SEARCH_WIDTH: f32 = 250.0;

const ROW_CHROME: f32 = 52.0;

const ANY_GENRE: &str = "Any genre";

const SORTS: [SortChoice; 4] = [
    SortChoice::Title,
    SortChoice::Added,
    SortChoice::Year,
    SortChoice::Progress,
];

pub type Sort = SortChoice;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FilterChips {
    pub kind: KindFilter,

    pub watch: WatchFilter,

    pub available_only: bool,

    pub watchlist_only: bool,

    pub favorites_only: bool,
}

impl FilterChips {
    pub fn none() -> Self {
        Self {
            kind: KindFilter::All,
            watch: WatchFilter::Any,
            available_only: false,
            watchlist_only: false,
            favorites_only: false,
        }
    }

    pub fn narrowed(&self) -> bool {
        self.kind != KindFilter::All
            || self.watch != WatchFilter::Any
            || self.available_only
            || self.watchlist_only
            || self.favorites_only
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chip {
    All,

    Films,

    Series,

    Unwatched,

    AvailableOffline,

    Watchlist,

    Favourites,
}

impl Chip {
    pub const ALL: [Chip; 7] = [
        Chip::All,
        Chip::Films,
        Chip::Series,
        Chip::Unwatched,
        Chip::AvailableOffline,
        Chip::Watchlist,
        Chip::Favourites,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Chip::All => "All",
            Chip::Films => "Films",
            Chip::Series => "Series",
            Chip::Unwatched => "Unwatched",
            Chip::AvailableOffline => "Available offline",
            Chip::Watchlist => "Watchlist",
            Chip::Favourites => "Favourites",
        }
    }

    pub fn kind_group(self) -> bool {
        matches!(self, Chip::All | Chip::Films | Chip::Series)
    }

    pub fn active(self, filters: FilterChips) -> bool {
        match self {
            Chip::All => filters.kind == KindFilter::All,
            Chip::Films => filters.kind == KindFilter::Movies,
            Chip::Series => filters.kind == KindFilter::Series,
            Chip::Unwatched => filters.watch == WatchFilter::Unwatched,
            Chip::AvailableOffline => filters.available_only,
            Chip::Watchlist => filters.watchlist_only,
            Chip::Favourites => filters.favorites_only,
        }
    }

    pub fn message(self, active: bool) -> Message {
        match self {
            Chip::All => Message::KindChanged(KindFilter::All),
            Chip::Films => Message::KindChanged(if active {
                KindFilter::All
            } else {
                KindFilter::Movies
            }),
            Chip::Series => Message::KindChanged(if active {
                KindFilter::All
            } else {
                KindFilter::Series
            }),
            Chip::Unwatched => Message::WatchChanged(if active {
                WatchFilter::Any
            } else {
                WatchFilter::Unwatched
            }),
            Chip::AvailableOffline => Message::ToggleAvailableOnly(!active),
            Chip::Watchlist => Message::ToggleWatchlistOnly(!active),
            Chip::Favourites => Message::ToggleFavoritesOnly(!active),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChipRows {
    Single,

    Stacked,
}

pub fn chip_rows(width: f32) -> ChipRows {
    match Breakpoint::at(capped_window(width)) {
        Breakpoint::Narrow => ChipRows::Stacked,
        _ => ChipRows::Single,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeaderLayout {
    Inline,

    Stacked,
}

pub fn header_layout(width: f32) -> HeaderLayout {
    match Breakpoint::at(capped_window(width)) {
        Breakpoint::Narrow => HeaderLayout::Stacked,
        _ => HeaderLayout::Inline,
    }
}

pub struct Library<'a> {
    pub items: &'a [LibraryItem],

    pub total: usize,

    pub page: u32,

    pub pages: u32,

    pub query: &'a str,

    pub filters: FilterChips,

    pub sort: Sort,

    pub source_line: String,

    pub nav: &'a Nav,

    pub genres: &'a [String],

    pub genre: Option<&'a str>,

    pub art: &'a HashMap<String, image::Handle>,

    pub width: f32,
}

impl<'a> Library<'a> {
    pub fn view(self) -> Element<'a, Message> {
        let body = if self.items.is_empty() {
            self.empty()
        } else {
            self.grid()
        };

        let mut page = column![self.header(), self.chips(), body]
            .spacing(theme::gap(Gap::Rail))
            .width(Length::Fill);

        if self.pages > 1 {
            page = page.push(self.pager());
        }

        let framed = container(
            scrollable(page)
                .id(grid_id())
                .direction(theme::scrollbar_direction())
                .width(Length::Fill)
                .height(Length::Fill)
                .style(theme::scrollbar),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(Padding {
            top: theme::gap(Gap::Section),
            bottom: theme::gap(Gap::Section),
            left: 0.0,
            right: 0.0,
        });

        match Breakpoint::at(self.width).content_cap() {
            Some(cap) => container(container(framed).width(Length::Fill).max_width(cap))
                .width(Length::Fill)
                .height(Length::Fill)
                .center_x(Length::Fill)
                .into(),
            None => framed.into(),
        }
    }

    fn header(&self) -> Element<'a, Message> {
        let tokens = design::tokens();

        let heading = row![
            text("Library")
                .size(tokens.type_scale.title)
                .font(theme::semibold())
                .color(theme::ink()),
            container(
                text(count_line(self.total))
                    .size(tokens.type_scale.caption)
                    .font(theme::regular())
                    .color(theme::muted()),
            )
            .padding(Padding {
                top: 0.0,
                bottom: theme::gap(Gap::Tight),
                left: 0.0,
                right: 0.0,
            }),
        ]
        .spacing(theme::gap(Gap::Group))
        .align_y(Alignment::End);

        let sort = pick_list(SORTS, Some(self.sort), Message::SortChanged)
            .text_size(tokens.type_scale.label)
            .font(theme::regular())
            .padding([theme::gap(Gap::Card), theme::gap(Gap::Group)])
            .style(field_pick_list)
            .menu_style(dark_menu);

        match header_layout(self.width) {
            HeaderLayout::Stacked => column![
                row![heading, Space::new().width(Length::Fill), sort]
                    .spacing(theme::gap(Gap::Group))
                    .align_y(Alignment::Center)
                    .width(Length::Fill),
                self.search_field(Length::Fill),
            ]
            .spacing(theme::gap(Gap::Card))
            .width(Length::Fill)
            .into(),
            HeaderLayout::Inline => row![
                heading,
                Space::new().width(Length::Fill),
                self.search_field(Length::Fixed(SEARCH_WIDTH)),
                sort,
            ]
            .spacing(theme::gap(Gap::Group))
            .align_y(Alignment::Center)
            .width(Length::Fill)
            .into(),
        }
    }

    fn search_field(&self, width: Length) -> Element<'a, Message> {
        let tokens = design::tokens();

        container(
            row![
                icons::icon(Icon::Search, 16.0, theme::muted()),
                text_input("Search titles", self.query)
                    .on_input(Message::SearchChanged)
                    .size(tokens.type_scale.label)
                    .font(theme::regular())
                    .padding(0)
                    .width(Length::Fill)
                    .style(field_input),
            ]
            .spacing(theme::gap(Gap::Card))
            .align_y(Alignment::Center),
        )
        .width(width)
        .padding(Padding {
            top: theme::gap(Gap::Card),
            bottom: theme::gap(Gap::Card),
            left: theme::gap(Gap::Group),
            right: theme::gap(Gap::Group),
        })
        .style(field_frame)
        .into()
    }

    fn chips(&self) -> Element<'a, Message> {
        let tokens = design::tokens();

        let mut kinds = row![]
            .spacing(theme::gap(Gap::Card))
            .align_y(Alignment::Center);

        let mut toggles = row![]
            .spacing(theme::gap(Gap::Card))
            .align_y(Alignment::Center);

        for chip in Chip::ALL {
            let active = chip.active(self.filters);
            let pressed = button(
                text(chip.label())
                    .size(tokens.type_scale.label)
                    .font(if active {
                        theme::medium()
                    } else {
                        theme::regular()
                    })
                    .color(if active {
                        theme::ground()
                    } else {
                        theme::ink()
                    }),
            )
            .on_press(chip.message(active))
            .padding([theme::gap(Gap::Card), theme::gap(Gap::Group)])
            .style(chip_style(active));

            if chip.kind_group() {
                kinds = kinds.push(pressed);
            } else {
                toggles = toggles.push(pressed);
            }
        }

        let genre = (!self.genres.is_empty()).then(|| {
            let mut options = vec![ANY_GENRE.to_string()];
            options.extend(self.genres.iter().cloned());
            let active = self.genre.is_some();

            let pick = pick_list(options, self.genre.map(str::to_string), |choice: String| {
                Message::GenreChanged((choice != ANY_GENRE).then_some(choice))
            })
            .placeholder("Genre")
            .text_size(tokens.type_scale.label)
            .font(theme::regular())
            .padding([theme::gap(Gap::Card), theme::gap(Gap::Group)])
            .style(chip_pick_list(active))
            .menu_style(dark_menu);

            Element::<Message>::from(pick)
        });

        let rule = || {
            container(
                Space::new()
                    .width(Length::Fixed(1.0))
                    .height(Length::Fixed(18.0)),
            )
            .height(Length::Fixed(18.0))
            .style(|_theme| container::Style {
                background: Some(Background::Color(theme::hair())),
                ..container::Style::default()
            })
        };

        let strip: Element<'a, Message> = match chip_rows(self.width) {
            ChipRows::Single => {
                let mut strip = row![kinds, rule(), toggles]
                    .spacing(theme::gap(Gap::Card))
                    .align_y(Alignment::Center);

                if let Some(genre) = genre {
                    strip = strip.push(genre);
                }

                strip.into()
            }
            ChipRows::Stacked => {
                let mut rows = column![kinds, toggles].spacing(theme::gap(Gap::Card));

                if let Some(genre) = genre {
                    rows = rows.push(row![genre]);
                }

                rows.into()
            }
        };

        let line = text(self.source_line.clone())
            .size(tokens.type_scale.caption)
            .font(theme::regular())
            .color(theme::muted());

        match source_placement(self.width) {
            SourcePlacement::BesideChips => row![strip, Space::new().width(Length::Fill), line,]
                .spacing(theme::gap(Gap::Group))
                .align_y(Alignment::Center)
                .width(Length::Fill)
                .into(),
            SourcePlacement::BeneathChips => column![
                row![strip, Space::new().width(Length::Fill)]
                    .width(Length::Fill)
                    .align_y(Alignment::Center),
                line,
            ]
            .spacing(theme::gap(Gap::Card))
            .width(Length::Fill)
            .into(),
        }
    }

    fn grid(&self) -> Element<'a, Message> {
        let width = tile_width(self.width);
        let columns = Breakpoint::at(capped_window(self.width)).columns().max(1);
        let focus = self.focus_index();

        let mut grid = column![].spacing(theme::gap(Gap::Rail)).width(Length::Fill);

        for (row_index, chunk) in visible_tiles(self.items, self.nav.focus())
            .chunks(columns)
            .enumerate()
        {
            let mut cells = row![].spacing(CARD_GAP).align_y(Alignment::Start);

            for (column_index, item) in chunk.iter().enumerate() {
                let index = row_index * columns + column_index;
                cells = cells.push(tile(
                    item,
                    self.art.get(&item.work_id),
                    focus == Some(index),
                    width,
                ));
            }

            grid = grid.push(cells);
        }

        grid.into()
    }

    fn focus_index(&self) -> Option<usize> {
        let focus = self.nav.focus();

        if focus.rail != 0 {
            return None;
        }

        let start = page_start(focus, self.items.len());
        let visible = self.items.len().saturating_sub(start).min(PAGE_TILES);
        let index = focus.column.checked_sub(start)?;

        (index < visible).then_some(index)
    }

    fn empty(&self) -> Element<'a, Message> {
        let tokens = design::tokens();
        let narrowed =
            !self.query.trim().is_empty() || self.filters.narrowed() || self.genre.is_some();

        let (headline, detail) = if narrowed {
            (
                "Nothing matches these filters.",
                "Clear the search and switch the other filters off to see the whole library.",
            )
        } else {
            (
                "No films or episodes yet.",
                "Add a folder in Settings and Screenroom reads it in place.",
            )
        };

        column![
            text(headline)
                .size(tokens.type_scale.body_lg)
                .font(theme::medium())
                .color(theme::ink()),
            text(detail)
                .size(tokens.type_scale.caption)
                .font(theme::regular())
                .color(theme::muted()),
        ]
        .spacing(theme::gap(Gap::Card))
        .width(Length::Fill)
        .into()
    }

    fn pager(&self) -> Element<'a, Message> {
        let tokens = design::tokens();
        let pages = self.pages.max(1);
        let first = (self.page as usize * PAGE_TILES + 1).min(self.total.max(1));
        let last = ((self.page as usize + 1) * PAGE_TILES).min(self.total);
        let caption = tokens.type_scale.caption;

        row![
            text(format!("{first}\u{2013}{last} of {}", self.total))
                .size(caption)
                .font(theme::regular())
                .color(theme::muted()),
            Space::new().width(Length::Fill),
            button(text("Previous").size(caption).font(theme::regular()))
                .on_press_maybe((self.page > 0).then_some(Message::PreviousPage))
                .padding([theme::gap(Gap::Tight), theme::gap(Gap::Card)])
                .style(quiet_button),
            text(format!("Page {} of {pages}", self.page + 1))
                .size(caption)
                .font(theme::regular())
                .color(theme::muted()),
            button(text("Next").size(caption).font(theme::regular()))
                .on_press_maybe((self.page + 1 < pages).then_some(Message::NextPage),)
                .padding([theme::gap(Gap::Tight), theme::gap(Gap::Card)])
                .style(quiet_button),
        ]
        .spacing(theme::gap(Gap::Group))
        .align_y(Alignment::Center)
        .width(Length::Fill)
        .into()
    }
}

pub fn grid_id() -> Id {
    Id::from("library-grid")
}

pub fn capped_window(width: f32) -> f32 {
    if !width.is_finite() || width <= 0.0 {
        return CARD_WIDTH;
    }

    match Breakpoint::at(width).content_cap() {
        Some(cap) => (cap + 2.0 * theme::PAD).min(width),
        None => width,
    }
}

pub fn tile_width(width: f32) -> f32 {
    if !width.is_finite() || width <= 0.0 {
        return CARD_WIDTH;
    }

    let width = capped_window(width);
    let columns = card_columns(width) as f32;
    let available = (width - 2.0 * theme::PAD - theme::scroll_reserve()).max(0.0);
    let tile = (available - CARD_GAP * (columns - 1.0)) / columns;

    if tile.is_finite() && tile > 0.0 {
        tile
    } else {
        CARD_WIDTH
    }
}

pub fn focus_row(focus: FocusPos, width: f32) -> usize {
    let columns = card_columns(capped_window(width)).max(1);
    focus.column / columns
}

pub fn scroll_task(focus: FocusPos, width: f32) -> Task<Message> {
    let step = tile_width(width) * ART_RATIO + ROW_CHROME + theme::gap(Gap::Rail);

    operation::scroll_to::<Message>(
        grid_id(),
        operation::AbsoluteOffset {
            x: None,
            y: Some(focus_row(focus, width) as f32 * step),
        },
    )
}

pub fn visible_tiles(items: &[LibraryItem], focused: FocusPos) -> Vec<&LibraryItem> {
    let start = page_start(focused, items.len());

    items.iter().skip(start).take(PAGE_TILES).collect()
}

fn page_start(focused: FocusPos, len: usize) -> usize {
    if len <= PAGE_TILES {
        0
    } else {
        (focused.column / PAGE_TILES) * PAGE_TILES
    }
}

pub fn source_line(folders: usize, files: usize, scanned: Option<&str>) -> String {
    let folders = match folders {
        0 => "no folders".to_string(),
        1 => "1 folder".to_string(),
        count => format!("{count} folders"),
    };

    let files = match files {
        0 => "no files".to_string(),
        1 => "1 file".to_string(),
        count => format!("{count} files"),
    };

    let recency = match scanned.map(str::trim).filter(|when| !when.is_empty()) {
        Some(when) => format!("scanned {when}"),
        None => "never scanned".to_string(),
    };

    format!("{folders} \u{00b7} {files} \u{00b7} {recency}")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourcePlacement {
    BesideChips,

    BeneathChips,
}

pub fn source_placement(width: f32) -> SourcePlacement {
    match Breakpoint::at(capped_window(width)) {
        Breakpoint::Narrow | Breakpoint::Compact => SourcePlacement::BeneathChips,
        _ => SourcePlacement::BesideChips,
    }
}

pub fn count_line(total: usize) -> String {
    match total {
        0 => "no titles".to_string(),
        1 => "1 title".to_string(),
        count => format!("{count} titles"),
    }
}

pub fn tile_title(title: &str) -> &str {
    truncate(title, TILE_TITLE_CHARS)
}

fn tile<'a>(
    item: &'a LibraryItem,
    art: Option<&'a image::Handle>,
    focused: bool,
    width: f32,
) -> Element<'a, Message> {
    let tokens = design::tokens();
    let art_height = (width * ART_RATIO).round();

    let face: Element<'a, Message> = match art {
        Some(handle) => image(handle.clone())
            .width(Length::Fill)
            .height(Length::Fill)
            .content_fit(ContentFit::Cover)
            .into(),
        None => container(Space::new().width(Length::Fill).height(Length::Fill)).into(),
    };

    let framed = container(face)
        .width(Length::Fixed(width))
        .height(Length::Fixed(art_height))
        .clip(true)
        .style(move |_theme| art_frame(focused));

    let mut layers = stack![framed];

    if let Some(progress) = progress_of(item) {
        layers = stack![layers, progress_bar(width, progress)];
    }

    let title = text(tile_title(&item.title).to_string())
        .size(tokens.type_scale.label)
        .font(theme::medium())
        .color(theme::ink())
        .width(Length::Fill);

    let caption = text(truncate(&caption_of(item), CAPTION_CHARS).to_string())
        .size(tokens.type_scale.caption)
        .font(theme::regular())
        .color(theme::muted())
        .width(Length::Fill);

    let card = column![layers, title, caption]
        .spacing(theme::gap(Gap::Tight))
        .width(Length::Fixed(width));

    mouse_area(
        button(card)
            .on_press(Message::OpenDetail(item.id.clone()))
            .padding(0)
            .style(tile_button),
    )
    .on_enter(Message::HoverCard(item.id.clone()))
    .interaction(mouse::Interaction::Pointer)
    .into()
}

fn caption_of(item: &LibraryItem) -> String {
    let mut parts = Vec::with_capacity(2);
    let episode = item.episode_label();

    if !episode.is_empty() {
        parts.push(episode);
    } else if let Some(year) = item.year {
        parts.push(year.to_string());
    }

    if item.duration_ms > 0 {
        parts.push(runtime(item.duration_ms));
    }

    if parts.is_empty() {
        parts.push(item.kind.as_str().to_string());
    }

    parts.join(" \u{00b7} ")
}

fn runtime(ms: i64) -> String {
    let minutes = (ms.max(0) + 30_000) / 60_000;

    if minutes >= 60 {
        format!("{}h {}m", minutes / 60, minutes % 60)
    } else {
        format!("{minutes}m")
    }
}

fn progress_of(item: &LibraryItem) -> Option<f32> {
    if item.watched || item.duration_ms <= 0 || item.position_ms <= 0 {
        return None;
    }

    let ratio = item.position_ms as f32 / item.duration_ms as f32;

    (ratio.is_finite() && ratio > 0.0 && ratio < 1.0).then_some(ratio)
}

fn progress_bar(width: f32, progress: f32) -> Element<'static, Message> {
    let filled = width * progress.clamp(0.0, 1.0);

    let bar = container(
        Space::new()
            .width(Length::Fixed(filled))
            .height(Length::Fixed(PROGRESS_HEIGHT)),
    )
    .width(Length::Fixed(filled))
    .height(Length::Fixed(PROGRESS_HEIGHT))
    .style(|_theme| container::Style {
        background: Some(Background::Color(theme::signal())),
        ..container::Style::default()
    });

    column![
        Space::new().height(Length::Fill),
        row![bar, Space::new().width(Length::Fill)].width(Length::Fill),
    ]
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

fn art_frame(focused: bool) -> container::Style {
    container::Style {
        background: Some(Background::Color(theme::surface(2))),
        border: Border {
            color: if focused { theme::ink() } else { theme::hair() },
            width: if focused { 2.0 } else { 1.0 },
            radius: theme::radius(Radius::Small).into(),
        },
        text_color: Some(theme::ink()),
        shadow: Shadow::default(),
        snap: true,
    }
}

fn field_frame(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(theme::surface(1))),
        border: Border {
            color: theme::hair(),
            width: 1.0,
            radius: theme::radius(Radius::Control).into(),
        },
        text_color: Some(theme::ink()),
        shadow: Shadow::default(),
        snap: true,
    }
}

fn field_input(_theme: &Theme, _status: text_input::Status) -> text_input::Style {
    text_input::Style {
        background: Background::Color(Color::TRANSPARENT),
        border: Border::default(),
        icon: theme::muted(),
        placeholder: theme::muted(),
        value: theme::ink(),
        selection: theme::surface(3),
    }
}

fn field_pick_list(_theme: &Theme, _status: pick_list::Status) -> pick_list::Style {
    pick_list::Style {
        text_color: theme::ink(),
        placeholder_color: theme::muted(),
        handle_color: theme::muted(),
        background: Background::Color(theme::surface(1)),
        border: Border {
            color: theme::hair(),
            width: 1.0,
            radius: theme::radius(Radius::Control).into(),
        },
    }
}

fn chip_pick_list(active: bool) -> impl Fn(&Theme, pick_list::Status) -> pick_list::Style {
    move |_theme, _status| pick_list::Style {
        text_color: if active {
            theme::ground()
        } else {
            theme::ink()
        },
        placeholder_color: theme::ink(),
        handle_color: if active {
            theme::ground()
        } else {
            theme::ink()
        },
        background: Background::Color(if active {
            theme::signal()
        } else {
            theme::surface(1)
        }),
        border: Border {
            color: if active {
                theme::signal()
            } else {
                theme::hair()
            },
            width: 1.0,
            radius: theme::radius(Radius::Overlay).into(),
        },
    }
}

fn dark_menu(_theme: &Theme) -> MenuStyle {
    MenuStyle {
        background: Background::Color(theme::surface(2)),
        border: Border {
            color: theme::hair(),
            width: 1.0,
            radius: theme::radius(Radius::Control).into(),
        },
        text_color: theme::ink(),
        selected_text_color: theme::ground(),
        selected_background: Background::Color(theme::signal()),
        shadow: Shadow::default(),
    }
}

fn chip_style(active: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_theme, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);

        button::Style {
            background: Some(Background::Color(if active {
                theme::signal()
            } else if hovered {
                theme::surface(2)
            } else {
                theme::surface(1)
            })),
            text_color: if active {
                theme::ground()
            } else {
                theme::ink()
            },
            border: Border {
                color: if active {
                    theme::signal()
                } else {
                    theme::hair()
                },
                width: 1.0,
                radius: theme::radius(Radius::Overlay).into(),
            },
            shadow: Shadow::default(),
            snap: true,
        }
    }
}

fn tile_button(_theme: &Theme, _status: button::Status) -> button::Style {
    button::Style {
        background: None,
        text_color: theme::ink(),
        border: Border::default(),
        shadow: Shadow::default(),
        snap: false,
    }
}

fn quiet_button(_theme: &Theme, status: button::Status) -> button::Style {
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);

    button::Style {
        background: None,
        text_color: if hovered {
            theme::ink()
        } else {
            theme::muted()
        },
        border: Border::default(),
        shadow: Shadow::default(),
        snap: false,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use screenroom_core::{LibraryItem, MediaKind};

    use super::{
        Chip, ChipRows, FilterChips, HeaderLayout, Library, PAGE_TILES, SortChoice,
        SourcePlacement, capped_window, chip_rows, header_layout, source_line, source_placement,
        tile_title, tile_width, visible_tiles,
    };
    use crate::app::{KindFilter, Screen, WatchFilter};
    use crate::ui::components::poster::{CARD_GAP, card_columns};
    use crate::ui::route::{FocusPos, Nav};
    use crate::ui::theme;
    use crate::ui::theme::Breakpoint;

    fn item(index: usize) -> LibraryItem {
        LibraryItem {
            id: format!("file-{index}"),
            work_id: format!("work-{index}"),
            edition_id: format!("edition-{index}"),
            source_id: "source".to_string(),
            title: format!("Title {index}"),
            filename: format!("title-{index}.mkv"),
            relative_path: format!("films/title-{index}.mkv"),
            kind: MediaKind::Movie,
            overview: String::new(),
            episode_title: String::new(),
            episode_overview: String::new(),
            genres: Vec::new(),
            year: Some(2020),
            season: None,
            episode: None,
            episode_end: None,
            duration_ms: 6_360_000,
            position_ms: 0,
            added_ms: 0,
            available: true,
            watched: false,
            favorite: false,
            watchlisted: false,
            manual_match: false,
            metadata_origin: "filename".to_string(),
        }
    }

    fn many(count: usize) -> Vec<LibraryItem> {
        (0..count).map(item).collect()
    }

    #[test]
    fn a_ten_thousand_item_library_pages_instead_of_rendering_everything() {
        let items = many(10_000);
        let tiles = visible_tiles(&items[..24], FocusPos { rail: 0, column: 0 });
        assert_eq!(tiles.len(), 24, "only the page is built");
    }

    #[test]
    fn the_source_line_states_folders_files_and_recency() {
        assert!(source_line(2, 775, Some("2 hours ago")).contains("2 folders"));
        assert!(source_line(0, 0, None).contains("no folders"));
    }

    #[test]
    fn a_whole_library_is_bounded_to_one_page_of_tiles() {
        let items = many(10_000);
        let tiles = visible_tiles(&items, FocusPos { rail: 0, column: 0 });
        assert_eq!(tiles.len(), PAGE_TILES, "a page is a bounded slice");
        assert_eq!(tiles[0].id, "file-0");
    }

    #[test]
    fn a_very_long_title_truncates_instead_of_overflowing() {
        let long = "The Extraordinarily Long Title Of A Film That Nobody Would Ever Name This Way";
        let cut = tile_title(long);
        assert!(cut.chars().count() <= 26);
        assert!(long.starts_with(cut));

        let emoji = "ফিল্ম 🎬 café\u{0301} and more words than fit";
        let cut = tile_title(emoji);
        assert_eq!(cut.chars().count(), 26, "no char boundary may be split");
        assert!(emoji.starts_with(cut));
    }

    #[test]
    fn the_source_line_takes_its_own_row_before_it_can_wrap() {
        for width in [320.0_f32, 600.0, 899.0, 900.0, 960.0, 1_099.0, f32::NAN] {
            assert_eq!(
                source_placement(width),
                SourcePlacement::BeneathChips,
                "at {width} the source line must not squeeze the chips"
            );
        }

        for width in [1_100.0_f32, 1_280.0, 1_440.0, 3_440.0] {
            assert_eq!(
                source_placement(width),
                SourcePlacement::BesideChips,
                "at {width} the source line sits beside the chips"
            );
        }
    }

    #[test]
    fn the_source_line_never_claims_recency_it_does_not_have() {
        let never = source_line(2, 775, None).to_lowercase();
        assert!(!never.contains("ago"));
        assert!(!never.contains("just now"));
        assert!(!never.contains("minute"));
        assert!(never.contains("never scanned"));

        let blank = source_line(1, 1, Some("  ")).to_lowercase();
        assert!(!blank.contains("ago"));
        assert!(blank.contains("never scanned"));
        assert!(blank.contains("1 folder"));
        assert!(blank.contains("1 file"));
    }

    #[test]
    fn an_empty_library_still_builds_its_controls() {
        let nav = Nav::new(Screen::Browse);
        let art = HashMap::new();
        let genres: Vec<String> = Vec::new();

        let library = Library {
            items: &[],
            total: 0,
            page: 0,
            pages: 1,
            query: "",
            filters: FilterChips::none(),
            sort: SortChoice::Title,
            source_line: source_line(0, 0, None),
            nav: &nav,
            genres: &genres,
            genre: None,
            art: &art,
            width: 1440.0,
        };

        let _ = library.view();
    }

    #[test]
    fn an_ultrawide_caps_its_grid_instead_of_stretching_the_tiles() {
        assert_eq!(capped_window(3440.0), 1840.0);
        assert!(
            tile_width(3440.0) <= 268.0,
            "a tile may not grow past the widest rail card"
        );
        assert_eq!(capped_window(1440.0), 1440.0);
        assert_eq!(
            Breakpoint::at(capped_window(3440.0)).columns(),
            7,
            "the capped content keeps seven columns"
        );
    }

    #[test]
    fn the_grid_fills_the_gutters_exactly_and_leaves_the_scrollbar_its_strip() {
        for width in [900.0_f32, 1100.0, 1280.0, 1440.0, 1760.0, 3440.0] {
            let tile = tile_width(width);
            let columns = card_columns(capped_window(width)) as f32;
            let span = tile * columns
                + CARD_GAP * (columns - 1.0)
                + 2.0 * theme::PAD
                + theme::scroll_reserve();

            assert!(
                (span - capped_window(width)).abs() < 0.01,
                "at {width}px the grid must fill the window and leave the scrollbar its strip, \
                 but it spans {span}"
            );
        }

        let cite = capped_window(3440.0);
        assert!(
            (tile_width(3440.0) * 7.0
                + CARD_GAP * 6.0
                + 2.0 * theme::PAD
                + theme::scroll_reserve()
                - cite)
                .abs()
                < 0.01,
            "the capped ultrawide column must not push a tile under the scrollbar"
        );
    }

    #[test]
    fn a_page_of_tiles_builds_with_a_focused_card() {
        let items = many(PAGE_TILES);
        let mut nav = Nav::new(Screen::Browse);
        nav.set_focus(FocusPos { rail: 0, column: 5 });
        let art = HashMap::new();
        let genres = vec!["Drama".to_string()];

        let library = Library {
            items: &items,
            total: 10_000,
            page: 0,
            pages: 417,
            query: "the",
            filters: FilterChips {
                kind: KindFilter::Movies,
                watch: WatchFilter::Unwatched,
                available_only: false,
                watchlist_only: true,
                favorites_only: false,
            },
            sort: SortChoice::Added,
            source_line: source_line(2, 10_000, Some("2 hours ago")),
            nav: &nav,
            genres: &genres,
            genre: Some("Drama"),
            art: &art,
            width: 1440.0,
        };

        assert_eq!(library.focus_index(), Some(5));
        let _ = library.view();
    }

    #[test]
    fn each_chip_shows_only_its_own_filter_and_leaves_the_others_alone() {
        let films = FilterChips {
            kind: KindFilter::Movies,
            ..FilterChips::none()
        };
        let films_and_unwatched = FilterChips {
            watch: WatchFilter::Unwatched,
            ..films
        };

        assert!(
            Chip::Films.active(films_and_unwatched),
            "choosing Unwatched may not un-highlight the Films filter that is still applied"
        );
        assert!(Chip::Unwatched.active(films_and_unwatched));
        assert!(!Chip::All.active(films_and_unwatched));
        assert!(!Chip::Series.active(films_and_unwatched));

        let everything = FilterChips {
            watch: WatchFilter::Unwatched,
            available_only: true,
            watchlist_only: true,
            favorites_only: true,
            ..FilterChips::none()
        };
        assert!(Chip::All.active(everything));
        assert!(Chip::Unwatched.active(everything));
        assert!(Chip::AvailableOffline.active(everything));
        assert!(Chip::Watchlist.active(everything));
        assert!(Chip::Favourites.active(everything));

        assert!(everything.narrowed());
        assert!(!FilterChips::none().narrowed());
    }

    #[test]
    fn the_header_and_the_chips_stack_before_they_can_clip() {
        for width in [640.0_f32, 700.0, 899.0] {
            assert_eq!(
                header_layout(width),
                HeaderLayout::Stacked,
                "at {width}px the search field needs its own row"
            );
            assert_eq!(
                chip_rows(width),
                ChipRows::Stacked,
                "at {width}px the filter chips need two rows"
            );
        }

        for width in [900.0_f32, 1_024.0, 1_440.0, 3_440.0] {
            assert_eq!(header_layout(width), HeaderLayout::Inline);
            assert_eq!(chip_rows(width), ChipRows::Single);
        }
    }
}
