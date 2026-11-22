use crate::app::Message;
use crate::design;
use crate::ui::icons::{self, Icon};
use crate::ui::motion;
use crate::ui::theme::{self, Breakpoint, Gap, Radius};
use iced::border::Radius as CornerRadius;
use iced::widget::canvas::gradient::Linear;
use iced::widget::canvas::{self, Canvas, Program};
use iced::widget::text::Wrapping;
use iced::widget::{Space, button, column, container, row, scrollable, stack, text, text_input};
use iced::{
    Alignment, Background, Border, Color, Element, Length, Padding, Point, Rectangle, Renderer,
    Shadow, Size, Theme, mouse,
};
use screenroom_core::SourceState;

const FALLBACK_WIDTH: f32 = 1440.0;

const WELCOME_MEASURE: f32 = 1180.0;

const EMPTY_MEASURE: f32 = 980.0;

const BODY_MEASURE: f32 = 660.0;

const STEP_MEASURE: f32 = 980.0;

const RAIL_GAP: f32 = 26.0;

pub const VISIBLE_FOLDERS: usize = 200;

const HAIRLINE: f32 = 1.0;

const ELLIPSIS: &str = "…";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderIssue {
    Available,

    Missing,

    PermissionLost,

    Unsafe,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderEntry {
    pub relative_path: String,

    pub name: String,

    pub media: usize,

    pub subfolders: usize,

    pub issue: FolderIssue,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceCard {
    pub id: String,

    pub label: String,

    pub root_path: String,

    pub state: SourceState,

    pub folders: Vec<String>,

    pub folders_configured: bool,

    pub indexed: usize,

    pub total: usize,

    pub scanning: bool,

    pub last_scan: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Inclusion {
    Excluded,

    Selected,

    Included,
}

pub fn inclusion(selected: &[String], relative: &str) -> Inclusion {
    if selected.iter().any(|entry| entry == relative) {
        return Inclusion::Selected;
    }
    if selected
        .iter()
        .any(|entry| entry.is_empty() || relative.starts_with(&format!("{entry}/")))
    {
        return Inclusion::Included;
    }
    Inclusion::Excluded
}

pub fn normalize(selected: &[String]) -> Vec<String> {
    let mut cleaned: Vec<String> = selected
        .iter()
        .map(|entry| entry.trim_matches('/').to_string())
        .collect();
    if cleaned.iter().any(String::is_empty) {
        return vec![String::new()];
    }
    cleaned.sort();
    cleaned.dedup();
    let mut kept: Vec<String> = Vec::new();
    for entry in cleaned {
        if kept
            .iter()
            .any(|done| entry == *done || entry.starts_with(&format!("{done}/")))
        {
            continue;
        }
        kept.retain(|done| !done.starts_with(&format!("{entry}/")));
        kept.push(entry);
    }
    kept
}

pub fn parent_of(relative: &str) -> String {
    match relative.rsplit_once('/') {
        Some((parent, _)) => parent.to_string(),
        None => String::new(),
    }
}

pub fn folder_label(relative: &str) -> String {
    if relative.is_empty() {
        return "Everything in this folder".to_string();
    }
    relative.rsplit('/').next().unwrap_or(relative).to_string()
}

pub fn breadcrumb_for(relative: &str) -> Vec<(String, String)> {
    let mut trail = vec![("This location".to_string(), String::new())];
    if relative.is_empty() {
        return trail;
    }
    let mut path = String::new();
    for part in relative.split('/') {
        if part.is_empty() {
            continue;
        }
        if !path.is_empty() {
            path.push('/');
        }
        path.push_str(part);
        trail.push((part.to_string(), path.clone()));
    }
    trail
}

pub struct Welcome<'a> {
    pub width: f32,

    pub path: &'a str,

    pub busy: bool,

    pub error: Option<&'a str>,
}

pub struct EmptyLibrary {
    pub cards: Vec<SourceCard>,

    pub width: f32,

    pub scanning: bool,
}

pub struct Manager {
    pub cards: Vec<SourceCard>,

    pub selected: Option<String>,

    pub browser: Option<FolderBrowser>,

    pub width: f32,
}

pub struct FolderBrowser {
    pub source_id: String,

    pub source_label: String,

    pub source_root: String,

    pub state: SourceState,

    pub current: String,

    pub breadcrumb: Vec<(String, String)>,

    pub entries: Vec<FolderEntry>,

    pub media_here: usize,

    pub selected: Vec<String>,

    pub folders_configured: bool,

    pub relink_path: String,

    pub unreadable: Option<String>,

    pub focus: Option<usize>,
}

impl SourceCard {
    pub fn state_label(&self) -> &'static str {
        match self.state {
            SourceState::Available => {
                if self.indexed > 0 {
                    "Available"
                } else {
                    "No media indexed"
                }
            }
            SourceState::Disconnected => "Disconnected",
            SourceState::PermissionLost => "Access lost",
            SourceState::Missing => "Folder moved or missing",
            SourceState::Removed => "Removed",
        }
    }
}

fn span(width: f32) -> f32 {
    if width.is_finite() && width > 0.0 {
        width
    } else {
        FALLBACK_WIDTH
    }
}

fn page_pad(width: f32) -> f32 {
    theme::content_inset(span(width))
}

fn inner_width(width: f32) -> f32 {
    (span(width) - 2.0 * page_pad(width)).max(0.0)
}

fn welcome_measure(width: f32) -> f32 {
    (span(width) - 2.0 * theme::PAD).clamp(0.0, WELCOME_MEASURE)
}

fn display_size(width: f32) -> f32 {
    let base = design::tokens().type_scale.title_lg;
    let factor = match Breakpoint::at(width) {
        Breakpoint::Narrow => 1.05,
        Breakpoint::Compact => 1.2,
        Breakpoint::Standard => 1.34,
        Breakpoint::Wide => 1.46,
        Breakpoint::Ultra => 1.58,
    };
    (base * factor).min(96.0)
}

fn char_budget(room: f32, size: f32) -> usize {
    if !room.is_finite() || room <= 0.0 || !size.is_finite() || size <= 0.0 {
        return 0;
    }
    ((room / (size * 0.58)).floor() as usize).max(4)
}

fn end_ellipsis(value: &str, max_chars: usize) -> String {
    let count = value.chars().count();
    if count <= max_chars {
        return value.to_string();
    }
    if max_chars == 0 {
        return String::new();
    }
    if max_chars == 1 {
        return ELLIPSIS.to_string();
    }
    let mut kept: String = value.chars().take(max_chars - 1).collect();
    kept.push_str(ELLIPSIS);
    kept
}

fn middle_ellipsis(value: &str, max_chars: usize) -> String {
    let count = value.chars().count();
    if count <= max_chars {
        return value.to_string();
    }
    if max_chars == 0 {
        return String::new();
    }
    if max_chars == 1 {
        return ELLIPSIS.to_string();
    }
    let keep = max_chars - 1;
    let head = keep.div_ceil(2);
    let tail = keep - head;
    let mut kept: String = value.chars().take(head).collect();
    kept.push_str(ELLIPSIS);
    if tail > 0 {
        kept.extend(value.chars().skip(count - tail));
    }
    kept
}

fn fit(value: &str, room: f32, size: f32) -> String {
    let budget = char_budget(room, size);
    if budget == 0 {
        return String::new();
    }
    end_ellipsis(value, budget)
}

fn fit_path(value: &str, room: f32, size: f32) -> String {
    let budget = char_budget(room, size);
    if budget == 0 {
        return String::new();
    }
    middle_ellipsis(value, budget)
}

fn indexed_phrase(indexed: usize) -> String {
    match indexed {
        0 => "No media indexed".to_string(),
        1 => "1 title indexed".to_string(),
        count => format!("{count} titles indexed"),
    }
}

fn indexed_short(indexed: usize) -> String {
    match indexed {
        0 => "Nothing indexed".to_string(),
        1 => "1 title".to_string(),
        count => format!("{count} titles"),
    }
}

fn folders_phrase(selected: &[String], configured: bool) -> String {
    if selected.iter().any(|entry| entry.is_empty()) {
        return "Everything in this location".to_string();
    }
    match selected.len() {
        0 if configured => "No folders selected".to_string(),
        0 => "No folder selection saved yet".to_string(),
        1 => "1 folder included".to_string(),
        count => format!("{count} folders included"),
    }
}

fn folders_short(selected: &[String]) -> String {
    if selected.iter().any(|entry| entry.is_empty()) {
        return "Everything included".to_string();
    }
    match selected.len() {
        0 => "No folders chosen".to_string(),
        1 => "1 folder".to_string(),
        count => format!("{count} folders"),
    }
}

fn scan_phrase(card: &SourceCard) -> String {
    if card.scanning {
        return "Scanning now".to_string();
    }
    match card.last_scan.as_deref() {
        Some(phrase) if !phrase.is_empty() => format!("Scanned {phrase}"),
        _ => "Never scanned".to_string(),
    }
}

fn media_phrase(media: usize) -> String {
    match media {
        0 => "No indexed media inside".to_string(),
        1 => "1 indexed item inside".to_string(),
        count => format!("{count} indexed items inside"),
    }
}

fn scan_intent(selected: &[String], configured: bool) -> String {
    if !configured {
        return "This whole location is read until you save a folder selection.".to_string();
    }
    if selected.iter().any(|entry| entry.is_empty()) {
        return "A scan will read everything in this location.".to_string();
    }
    match selected.len() {
        0 => "Nothing selected yet. A scan will not add any media.".to_string(),
        1 => "A scan will read 1 selected folder.".to_string(),
        count => format!("A scan will read {count} selected folders."),
    }
}

fn selection_note(selected: &[String], configured: bool) -> Option<&'static str> {
    if !selected.is_empty() {
        return None;
    }
    if configured {
        Some("This location is saved with no folders selected, so it adds no media.")
    } else {
        Some("No folder selection has been saved yet. Choose folders below, then scan.")
    }
}

fn inclusion_label(state: Inclusion) -> &'static str {
    match state {
        Inclusion::Selected => "Selected",
        Inclusion::Included => "Included by a parent",
        Inclusion::Excluded => "Excluded",
    }
}

fn inclusion_action(state: Inclusion) -> &'static str {
    match state {
        Inclusion::Selected => "Exclude",
        Inclusion::Included => "Included above",
        Inclusion::Excluded => "Include",
    }
}

fn toggle_hint(state: Inclusion) -> &'static str {
    match state {
        Inclusion::Selected => "Press to exclude this folder",
        Inclusion::Included => "Included by a folder above",
        Inclusion::Excluded => "Press to include this folder",
    }
}

fn issue_label(issue: FolderIssue) -> Option<&'static str> {
    match issue {
        FolderIssue::Available => None,
        FolderIssue::Missing => Some("Missing"),
        FolderIssue::PermissionLost => Some("No access"),
        FolderIssue::Unsafe => Some("Unsafe link"),
    }
}

fn relink_note(state: SourceState) -> &'static str {
    match state {
        SourceState::Available => "This location is readable.",
        SourceState::Disconnected => {
            "This drive is not connected. Reconnect it, then relink if its path changed."
        }
        SourceState::PermissionLost => {
            "Access to this folder was withdrawn. Choose it again to restore access."
        }
        SourceState::Missing => {
            "Screenroom cannot find this folder. It may have been moved, renamed or unplugged."
        }
        SourceState::Removed => {
            "This location was removed from the library. Relink it to read it again."
        }
    }
}

fn manager_split(width: f32) -> bool {
    Breakpoint::at(width) != Breakpoint::Narrow
}

fn rail_width(width: f32) -> f32 {
    match Breakpoint::at(width) {
        Breakpoint::Narrow => inner_width(width),
        Breakpoint::Compact => 240.0,
        Breakpoint::Standard => 260.0,
        Breakpoint::Wide => 288.0,
        Breakpoint::Ultra => 320.0,
    }
}

fn panel_width(width: f32) -> f32 {
    let inner = inner_width(width);
    if manager_split(width) {
        (inner - rail_width(width) - RAIL_GAP).max(0.0)
    } else {
        inner
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Weight {
    Primary,

    Secondary,

    Quiet,

    Hazard,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tone {
    Signal,

    Muted,

    Danger,
}

fn label_colour(weight: Weight, enabled: bool) -> Color {
    if !enabled {
        return design::tokens().palette.disabled_foreground;
    }
    match weight {
        Weight::Primary => theme::signal_foreground(),
        Weight::Hazard => theme::danger(),
        Weight::Secondary | Weight::Quiet => theme::ink(),
    }
}

fn button_style(
    weight: Weight,
    enabled: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style + 'static {
    move |_theme, status| {
        let palette = &design::tokens().palette;
        let corner = theme::radius(Radius::Control).into();
        let (background, text_color, border) = if !enabled {
            (
                Some(Background::Color(palette.surface1)),
                palette.disabled_foreground,
                Border {
                    color: Color::TRANSPARENT,
                    width: 0.0,
                    radius: corner,
                },
            )
        } else {
            match weight {
                Weight::Primary => {
                    let fill = match status {
                        button::Status::Pressed => {
                            motion::mix(palette.signal, palette.ink_muted, 0.2)
                        }
                        button::Status::Hovered => {
                            motion::mix(palette.signal, palette.ink_muted, 0.1)
                        }
                        _ => palette.signal,
                    };
                    (
                        Some(Background::Color(fill)),
                        palette.signal_foreground,
                        Border {
                            color: Color::TRANSPARENT,
                            width: 0.0,
                            radius: corner,
                        },
                    )
                }
                Weight::Secondary => {
                    let fill = match status {
                        button::Status::Hovered | button::Status::Pressed => palette.surface3,
                        _ => palette.surface2,
                    };
                    (
                        Some(Background::Color(fill)),
                        palette.foreground,
                        Border {
                            color: palette.hair,
                            width: 1.0,
                            radius: corner,
                        },
                    )
                }
                Weight::Quiet => {
                    let fill = match status {
                        button::Status::Hovered => palette.surface2,
                        button::Status::Pressed => palette.surface3,
                        _ => Color::TRANSPARENT,
                    };
                    (
                        Some(Background::Color(fill)),
                        palette.foreground,
                        Border {
                            color: Color::TRANSPARENT,
                            width: 0.0,
                            radius: corner,
                        },
                    )
                }
                Weight::Hazard => {
                    let fill = match status {
                        button::Status::Hovered | button::Status::Pressed => {
                            motion::with_alpha(palette.destructive, 0.14)
                        }
                        _ => Color::TRANSPARENT,
                    };
                    (
                        Some(Background::Color(fill)),
                        palette.destructive,
                        Border {
                            color: Color::TRANSPARENT,
                            width: 0.0,
                            radius: corner,
                        },
                    )
                }
            }
        };
        button::Style {
            background,
            text_color,
            border,
            shadow: Shadow::default(),
            snap: false,
        }
    }
}

fn action<'a>(
    label: impl Into<String>,
    icon: Option<Icon>,
    message: Option<Message>,
    weight: Weight,
) -> Element<'a, Message> {
    let tokens = design::tokens();
    let enabled = message.is_some();
    let colour = label_colour(weight, enabled);
    let height = match weight {
        Weight::Primary => tokens.control.lg,
        _ => tokens.control.md,
    };
    let mut content = row![]
        .spacing(theme::gap(Gap::Card))
        .align_y(Alignment::Center);
    if let Some(icon) = icon {
        content = content.push(icons::icon(icon, 14.0, colour));
    }
    content = content.push(
        text(label.into())
            .size(tokens.type_scale.label)
            .font(theme::medium())
            .color(colour),
    );
    let control = button(crate::ui::components::centred_label(height, content))
        .height(Length::Fixed(height))
        .padding(Padding {
            top: 0.0,
            bottom: 0.0,
            left: theme::gap(Gap::Rail),
            right: theme::gap(Gap::Rail),
        })
        .style(button_style(weight, enabled));
    match message {
        Some(message) => control.on_press(message).into(),
        None => control.into(),
    }
}

fn micro_action<'a>(
    label: impl Into<String>,
    icon: Option<Icon>,
    message: Option<Message>,
    weight: Weight,
) -> Element<'a, Message> {
    let tokens = design::tokens();
    let enabled = message.is_some();
    let colour = label_colour(weight, enabled);
    let height = tokens.control.sm;
    let mut content = row![]
        .spacing(theme::gap(Gap::Tight))
        .align_y(Alignment::Center);
    if let Some(icon) = icon {
        content = content.push(icons::icon(icon, 11.0, colour));
    }
    content = content.push(
        text(label.into())
            .size(tokens.type_scale.caption)
            .font(theme::medium())
            .color(colour),
    );
    let control = button(crate::ui::components::centred_label(height, content))
        .height(Length::Fixed(height))
        .padding(Padding {
            top: 0.0,
            bottom: 0.0,
            left: theme::gap(Gap::Group),
            right: theme::gap(Gap::Group),
        })
        .style(button_style(weight, enabled));
    match message {
        Some(message) => control.on_press(message).into(),
        None => control.into(),
    }
}

fn tag(label: &str, icon: Option<Icon>, tone: Tone) -> Element<'static, Message> {
    let tokens = design::tokens();
    let (background, foreground) = match tone {
        Tone::Signal => (motion::with_alpha(theme::signal(), 0.1), theme::ink()),
        Tone::Muted => (motion::with_alpha(theme::ink(), 0.06), theme::muted()),
        Tone::Danger => (motion::with_alpha(theme::danger(), 0.14), theme::danger()),
    };
    let mut content = row![]
        .spacing(theme::gap(Gap::Tight))
        .align_y(Alignment::Center);
    if let Some(icon) = icon {
        content = content.push(icons::icon(icon, 11.0, foreground));
    }
    content = content.push(
        text(label.to_string())
            .size(tokens.type_scale.micro)
            .font(theme::medium())
            .color(foreground),
    );
    container(content)
        .padding(Padding {
            top: 3.0,
            bottom: 3.0,
            left: theme::gap(Gap::Card),
            right: theme::gap(Gap::Card),
        })
        .style(move |_theme| container::Style {
            background: Some(Background::Color(background)),
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: CornerRadius::from(theme::radius(Radius::Full)),
            },
            ..container::Style::default()
        })
        .into()
}

fn inclusion_tag(state: Inclusion) -> Element<'static, Message> {
    match state {
        Inclusion::Selected => tag(inclusion_label(state), Some(Icon::Check), Tone::Signal),
        Inclusion::Included => tag(inclusion_label(state), Some(Icon::Check), Tone::Muted),
        Inclusion::Excluded => tag(inclusion_label(state), None, Tone::Muted),
    }
}

fn state_tone(state: SourceState) -> Tone {
    match state {
        SourceState::Available => Tone::Muted,
        SourceState::Disconnected | SourceState::PermissionLost | SourceState::Missing => {
            Tone::Danger
        }
        SourceState::Removed => Tone::Danger,
    }
}

fn source_state_tag(card: &SourceCard) -> Element<'static, Message> {
    if card.scanning {
        return tag("Scanning now", None, Tone::Signal);
    }
    tag(card.state_label(), None, state_tone(card.state))
}

fn hairline() -> Element<'static, Message> {
    container(
        Space::new()
            .width(Length::Fill)
            .height(Length::Fixed(HAIRLINE)),
    )
    .width(Length::Fill)
    .height(Length::Fixed(HAIRLINE))
    .style(|_theme| container::Style {
        background: Some(Background::Color(theme::hair())),
        ..container::Style::default()
    })
    .into()
}

fn dot() -> Element<'static, Message> {
    text("·")
        .size(design::tokens().type_scale.caption)
        .color(theme::muted())
        .into()
}

fn panel_style() -> container::Style {
    container::Style {
        background: Some(Background::Color(theme::surface(1))),
        text_color: Some(theme::ink()),
        border: Border {
            color: theme::hair(),
            width: HAIRLINE,
            radius: CornerRadius::from(theme::radius(Radius::Surface)),
        },
        shadow: Shadow::default(),
        snap: true,
    }
}

fn raised_style() -> container::Style {
    container::Style {
        background: Some(Background::Color(theme::surface(2))),
        text_color: Some(theme::ink()),
        border: Border {
            color: theme::hair(),
            width: HAIRLINE,
            radius: CornerRadius::from(theme::radius(Radius::Surface)),
        },
        shadow: Shadow::default(),
        snap: true,
    }
}

fn danger_panel_style() -> container::Style {
    container::Style {
        background: Some(Background::Color(motion::with_alpha(theme::danger(), 0.1))),
        text_color: Some(theme::ink()),
        border: Border {
            color: motion::with_alpha(theme::danger(), 0.32),
            width: HAIRLINE,
            radius: CornerRadius::from(theme::radius(Radius::Surface)),
        },
        shadow: Shadow::default(),
        snap: true,
    }
}

#[derive(Clone, Copy)]
enum Mood {
    Welcome,

    Calm,
}

struct Backdrop {
    mood: Mood,
}

impl Program<Message> for Backdrop {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let size = bounds.size();
        if !size.width.is_finite()
            || !size.height.is_finite()
            || size.width <= 0.0
            || size.height <= 0.0
        {
            return vec![frame.into_geometry()];
        }

        let ground = theme::ground();
        let lifted = theme::surface(1);
        let scrim = |alpha: f32| Color { a: alpha, ..ground };

        frame.fill_rectangle(
            Point::ORIGIN,
            size,
            canvas::Fill::from(canvas::Gradient::from(
                Linear::new(Point::ORIGIN, Point::new(0.0, size.height))
                    .add_stop(0.0, lifted)
                    .add_stop(0.52, ground)
                    .add_stop(1.0, ground),
            )),
        );

        let glow_center = Point::new(size.width * 0.66, size.height * 0.2);
        let glow_radius = size.width.min(size.height)
            * match self.mood {
                Mood::Welcome => 0.72,
                Mood::Calm => 0.46,
            };
        let layers = match self.mood {
            Mood::Welcome => 26,
            Mood::Calm => 14,
        };
        for step in 0..layers {
            let progress = step as f32 / layers as f32;
            let radius = glow_radius * (1.0 - progress * 0.92);
            let alpha = match self.mood {
                Mood::Welcome => 0.012,
                Mood::Calm => 0.007,
            };
            if radius <= 0.5 {
                continue;
            }
            frame.fill(
                &canvas::Path::circle(glow_center, radius),
                motion::with_alpha(theme::signal(), alpha),
            );
        }

        let shelf_top = size.height * 0.62;
        let spill_width = size.width * 0.82;
        let spill_left = (size.width - spill_width) / 2.0;
        frame.fill_rectangle(
            Point::new(spill_left, shelf_top),
            Size::new(spill_width, (size.height * 0.003).max(1.0)),
            canvas::Fill::from(canvas::Gradient::from(
                Linear::new(
                    Point::new(spill_left, shelf_top),
                    Point::new(spill_left + spill_width, shelf_top),
                )
                .add_stop(0.0, motion::with_alpha(theme::signal(), 0.0))
                .add_stop(0.5, motion::with_alpha(theme::signal(), 0.16))
                .add_stop(1.0, motion::with_alpha(theme::signal(), 0.0)),
            )),
        );

        if matches!(self.mood, Mood::Welcome) {
            let shelves = [
                (0.1_f32, 0.16_f32, 0.1_f32),
                (0.34_f32, 0.22_f32, 0.08_f32),
                (0.62_f32, 0.14_f32, 0.12_f32),
                (0.82_f32, 0.18_f32, 0.09_f32),
            ];
            for (offset, height, width) in shelves {
                let block_width = size.width * width;
                let block_height = size.height * height;
                frame.fill(
                    &canvas::Path::rounded_rectangle(
                        Point::new(
                            size.width * offset,
                            shelf_top + (size.height * 0.14) - block_height,
                        ),
                        Size::new(block_width, block_height),
                        CornerRadius::from(2.0),
                    ),
                    motion::with_alpha(theme::surface(2), 0.22),
                );
            }
        }

        frame.fill_rectangle(
            Point::ORIGIN,
            Size::new(size.width, size.height * 0.36),
            canvas::Fill::from(canvas::Gradient::from(
                Linear::new(Point::ORIGIN, Point::new(0.0, size.height * 0.36))
                    .add_stop(
                        0.0,
                        scrim(match self.mood {
                            Mood::Welcome => 0.34,
                            Mood::Calm => 0.18,
                        }),
                    )
                    .add_stop(1.0, scrim(0.0)),
            )),
        );

        frame.fill_rectangle(
            Point::new(0.0, size.height * 0.42),
            Size::new(size.width, size.height * 0.58),
            canvas::Fill::from(canvas::Gradient::from(
                Linear::new(
                    Point::new(0.0, size.height * 0.42),
                    Point::new(0.0, size.height),
                )
                .add_stop(0.0, scrim(0.0))
                .add_stop(
                    1.0,
                    scrim(match self.mood {
                        Mood::Welcome => 0.92,
                        Mood::Calm => 0.66,
                    }),
                ),
            )),
        );

        let side = size.width * 0.22;
        frame.fill_rectangle(
            Point::ORIGIN,
            Size::new(side, size.height),
            canvas::Fill::from(canvas::Gradient::from(
                Linear::new(Point::ORIGIN, Point::new(side, 0.0))
                    .add_stop(0.0, scrim(0.46))
                    .add_stop(1.0, scrim(0.0)),
            )),
        );
        frame.fill_rectangle(
            Point::new(size.width - side, 0.0),
            Size::new(side, size.height),
            canvas::Fill::from(canvas::Gradient::from(
                Linear::new(
                    Point::new(size.width - side, 0.0),
                    Point::new(size.width, 0.0),
                )
                .add_stop(0.0, scrim(0.0))
                .add_stop(1.0, scrim(0.46)),
            )),
        );

        frame.fill_rectangle(
            Point::ORIGIN,
            Size::new(size.width, 1.0),
            motion::with_alpha(theme::signal(), 0.06),
        );

        vec![frame.into_geometry()]
    }
}

fn backdrop(mood: Mood) -> Element<'static, Message> {
    Canvas::new(Backdrop { mood })
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

pub fn welcome<'a>(view: Welcome<'a>) -> Element<'a, Message> {
    let tokens = design::tokens();
    let width = span(view.width);
    let measure = welcome_measure(width);
    let prose = measure.min(BODY_MEASURE);

    let wordmark = row![
        container(
            Space::new()
                .width(Length::Fixed(8.0))
                .height(Length::Fixed(8.0))
        )
        .style(|_theme| container::Style {
            background: Some(Background::Color(theme::signal())),
            border: Border {
                radius: CornerRadius::from(theme::radius(Radius::Small)),
                ..Border::default()
            },
            ..container::Style::default()
        }),
        text("SCREENROOM")
            .size(tokens.type_scale.micro)
            .font(theme::semibold())
            .color(theme::ink()),
        text("PERSONAL EDITION")
            .size(tokens.type_scale.micro)
            .font(theme::regular())
            .color(theme::muted()),
    ]
    .spacing(theme::gap(Gap::Card))
    .align_y(Alignment::Center);

    let intro = column![
        wordmark,
        text("Your films, where they already live.")
            .size(display_size(width))
            .font(theme::semibold())
            .color(theme::ink())
            .width(Length::Fill),
        text(
            "Point Screenroom at the folders and drives your films already sit in. It \
             reads them in place: nothing is moved, renamed or uploaded, and nothing \
             leaves this machine."
        )
        .size(tokens.type_scale.lead)
        .font(theme::regular())
        .color(theme::muted())
        .width(Length::Fixed(prose)),
    ]
    .spacing(theme::gap(Gap::Rail))
    .width(Length::Fill);

    let input = text_input("…or type an absolute path", view.path)
        .on_input(Message::NewRootChanged)
        .on_submit(Message::AddFolder)
        .style(design::input_style())
        .padding(tokens.spacing.field_y)
        .width(Length::Fill);

    let typed = column![
        text("Or type an absolute folder path")
            .size(tokens.type_scale.caption)
            .font(theme::regular())
            .color(theme::muted()),
        row![
            input,
            action(
                "Add this folder",
                None,
                Some(Message::AddFolder),
                Weight::Secondary
            ),
        ]
        .spacing(theme::gap(Gap::Card))
        .align_y(Alignment::Center),
    ]
    .spacing(theme::gap(Gap::Card))
    .width(Length::Fill);

    let actions = column![
        action(
            "Add a media location",
            Some(Icon::Plus),
            Some(Message::ChooseFolder),
            Weight::Primary
        ),
        typed,
    ]
    .spacing(theme::gap(Gap::Rail))
    .width(Length::Fill);

    let mut state_lines: Vec<Element<'a, Message>> = Vec::new();
    if view.busy {
        state_lines.push(
            text("Adding this location. A large folder can take a moment.")
                .size(tokens.type_scale.caption)
                .font(theme::regular())
                .color(theme::muted())
                .into(),
        );
    }
    if let Some(error) = view.error {
        state_lines.push(
            container(
                column![
                    text("COULD NOT ADD THIS LOCATION")
                        .size(tokens.type_scale.micro)
                        .font(theme::semibold())
                        .color(theme::ink()),
                    text(error.to_string())
                        .size(tokens.type_scale.caption)
                        .font(theme::regular())
                        .color(theme::ink()),
                ]
                .spacing(theme::gap(Gap::Tight)),
            )
            .style(|_theme| danger_panel_style())
            .padding(Padding {
                top: theme::gap(Gap::Card),
                bottom: theme::gap(Gap::Card),
                left: theme::gap(Gap::Group),
                right: theme::gap(Gap::Group),
            })
            .into(),
        );
    }
    let status = column(state_lines).spacing(theme::gap(Gap::Tight));

    let facts = text("Reads files in place  ·  Nothing is uploaded  ·  No account needed")
        .size(tokens.type_scale.micro)
        .font(theme::regular())
        .color(theme::muted());

    let body = column![intro, actions, status, steps(measure), hairline(), facts]
        .spacing(theme::gap(Gap::Section))
        .width(Length::Fixed(measure));

    let page = container(body)
        .width(Length::Fill)
        .center_x(Length::Fill)
        .padding(Padding {
            top: theme::gap(Gap::Section) * 1.3,
            bottom: theme::gap(Gap::Section),
            left: 0.0,
            right: 0.0,
        });

    let scroller = scrollable(page)
        .height(Length::Fill)
        .width(Length::Fill)
        .style(theme::scrollbar);

    container(stack![backdrop(Mood::Welcome), scroller])
        .width(Length::Fill)
        .height(Length::Fill)
        .clip(true)
        .into()
}

fn steps(measure: f32) -> Element<'static, Message> {
    let tokens = design::tokens();
    let span = measure.min(STEP_MEASURE);
    let wide = span >= 880.0;
    let step_width = if wide {
        ((span - 2.0 * theme::gap(Gap::Section)) / 3.0).max(0.0)
    } else {
        span
    };

    let first = step(
        "01",
        "Choose a location",
        "A folder, an external drive, or storage already connected to this Mac.",
        step_width,
    );
    let second = step(
        "02",
        "Include the folders you want",
        "Only the folders you include are scanned. Everything else stays out of the way.",
        step_width,
    );
    let third = step(
        "03",
        "Scan once",
        "Titles, posters and your place in each film are kept in Screenroom's own library.",
        step_width,
    );

    let columns: Element<'static, Message> = if wide {
        row![first, second, third]
            .spacing(theme::gap(Gap::Section))
            .align_y(Alignment::Start)
            .into()
    } else {
        column![first, hairline(), second, hairline(), third]
            .spacing(theme::gap(Gap::Rail))
            .width(Length::Fill)
            .into()
    };

    container(
        column![
            text("HOW IT WORKS")
                .size(tokens.type_scale.micro)
                .font(theme::semibold())
                .color(theme::muted()),
            columns,
        ]
        .spacing(theme::gap(Gap::Group)),
    )
    .width(Length::Fill)
    .into()
}

fn step(
    number: &'static str,
    title: &'static str,
    body: &'static str,
    width: f32,
) -> Element<'static, Message> {
    let tokens = design::tokens();
    container(
        column![
            text(number)
                .size(tokens.type_scale.micro)
                .font(theme::semibold())
                .color(theme::muted()),
            text(title)
                .size(tokens.type_scale.label)
                .font(theme::semibold())
                .color(theme::ink()),
            text(body)
                .size(tokens.type_scale.caption)
                .font(theme::regular())
                .color(theme::muted()),
        ]
        .spacing(theme::gap(Gap::Card))
        .width(Length::Fill),
    )
    .width(Length::Fixed(width))
    .into()
}

pub fn empty_library(view: EmptyLibrary) -> Element<'static, Message> {
    let EmptyLibrary {
        cards,
        width,
        scanning,
    } = view;
    let tokens = design::tokens();
    let page = span(width);
    let measure = inner_width(page).min(EMPTY_MEASURE);
    let prose = measure.min(BODY_MEASURE);

    let (headline_text, copy) = empty_state_copy(&cards, scanning);
    let headline = text(headline_text)
        .size(tokens.type_scale.title)
        .font(theme::semibold())
        .color(theme::ink())
        .width(Length::Fill);

    let lead = column![
        text("LIBRARY")
            .size(tokens.type_scale.micro)
            .font(theme::semibold())
            .color(theme::muted()),
        headline,
        text(copy)
            .size(tokens.type_scale.body_lg)
            .font(theme::regular())
            .color(theme::muted())
            .width(Length::Fixed(prose)),
    ]
    .spacing(theme::gap(Gap::Group))
    .width(Length::Fill);

    let mut list = column![].spacing(theme::gap(Gap::Card));
    if cards.is_empty() {
        list = list.push(
            text("No locations are connected yet. Add one to begin.")
                .size(tokens.type_scale.body)
                .font(theme::regular())
                .color(theme::muted()),
        );
    } else {
        for card in &cards {
            list = list.push(empty_card(card, measure));
        }
    }

    let first = cards.first();
    let review = action(
        match first {
            Some(card) => format!(
                "Review folders in {}",
                fit(&card.label, measure * 0.4, tokens.type_scale.label)
            ),
            None => "Review folders".to_string(),
        },
        None,
        first.map(|card| Message::SourceChosen(card.id.clone())),
        Weight::Primary,
    );
    let add = action(
        "Add another location",
        Some(Icon::Plus),
        Some(Message::ChooseFolder),
        Weight::Secondary,
    );
    let scan = action(
        if scanning {
            "Scanning locations…"
        } else {
            "Scan all locations"
        },
        None,
        (!scanning).then_some(Message::ScanAllSources),
        Weight::Quiet,
    );

    let actions: Element<'static, Message> = if measure < 700.0 {
        column![
            review,
            row![add, scan]
                .spacing(theme::gap(Gap::Group))
                .align_y(Alignment::Center),
        ]
        .spacing(theme::gap(Gap::Card))
        .width(Length::Fill)
        .into()
    } else {
        row![review, add, scan]
            .spacing(theme::gap(Gap::Group))
            .align_y(Alignment::Center)
            .into()
    };

    let settings = micro_action(
        "Relink or remove a location in Settings",
        Some(Icon::Settings),
        Some(Message::GoSettings),
        Weight::Quiet,
    );

    let body = column![
        lead,
        list,
        actions,
        hairline(),
        text("Media stays where it is. Screenroom only reads the folders you include.")
            .size(tokens.type_scale.micro)
            .font(theme::regular())
            .color(theme::muted()),
        settings,
    ]
    .spacing(theme::gap(Gap::Section))
    .width(Length::Fixed(measure));

    let content = container(body)
        .width(Length::Fill)
        .center_x(Length::Fill)
        .padding(Padding {
            top: theme::gap(Gap::Section) * 1.3,
            bottom: theme::gap(Gap::Section),
            left: 0.0,
            right: 0.0,
        });

    let scroller = scrollable(content)
        .height(Length::Fill)
        .width(Length::Fill)
        .style(theme::scrollbar);

    container(stack![backdrop(Mood::Calm), scroller])
        .width(Length::Fill)
        .height(Length::Fill)
        .clip(true)
        .into()
}

fn empty_state_copy(cards: &[SourceCard], scanning: bool) -> (String, String) {
    let unreachable = cards
        .iter()
        .any(|card| card.state != SourceState::Available);
    let configured = cards
        .iter()
        .any(|card| card.folders_configured && !card.folders.is_empty());
    let scanned = cards.iter().any(|card| card.last_scan.is_some());
    let headline = if unreachable {
        "Your media is offline"
    } else {
        "No playable media in these folders yet"
    };
    let mut copy = if unreachable {
        String::from(
            "Screenroom cannot read a location right now, so its titles are not available. \
             The indexed titles and your place in them are kept: reconnect the drive, \
             restore access, or relink the location. Nothing was deleted.",
        )
    } else if configured || scanned {
        String::from(
            "Screenroom looked where you pointed it and found nothing it can play. \
             Nothing was moved, renamed or deleted.",
        )
    } else {
        String::from(
            "No folders are included yet, so there is nothing to read. Choose at least \
             one folder in a location, then scan it. Nothing on disk is changed.",
        )
    };
    if scanning {
        copy.push_str(" A scan is running now, and this list updates as files arrive.");
    } else if unreachable {
        copy.push_str(" Use Review folders to relink it.");
    } else if configured || scanned {
        copy.push_str(" Review the included folders, add another location, or scan again.");
    }
    (headline.to_string(), copy)
}

fn empty_card(card: &SourceCard, measure: f32) -> Element<'static, Message> {
    let tokens = design::tokens();
    let line_room = (measure - 210.0).max(140.0);
    let title = fit(&card.label, line_room, tokens.type_scale.body_lg);
    let path = fit_path(&card.root_path, line_room, tokens.type_scale.caption);

    let facts = row![
        text(indexed_phrase(card.indexed))
            .size(tokens.type_scale.caption)
            .color(theme::muted()),
        dot(),
        text(folders_phrase(&card.folders, card.folders_configured))
            .size(tokens.type_scale.caption)
            .color(theme::muted()),
        dot(),
        text(scan_phrase(card))
            .size(tokens.type_scale.caption)
            .color(theme::muted()),
    ]
    .spacing(theme::gap(Gap::Card))
    .align_y(Alignment::Center)
    .wrap();

    let header = row![
        column![
            text(title)
                .size(tokens.type_scale.body_lg)
                .font(theme::semibold())
                .color(theme::ink()),
            text(path)
                .size(tokens.type_scale.caption)
                .font(theme::regular())
                .color(theme::muted())
                .wrapping(Wrapping::None),
        ]
        .spacing(2.0)
        .width(Length::Fill),
        source_state_tag(card),
    ]
    .spacing(theme::gap(Gap::Group))
    .align_y(Alignment::Center)
    .width(Length::Fill);

    let footer: Element<'static, Message> = if measure < 760.0 {
        column![
            facts,
            micro_action(
                "Review folders",
                Some(Icon::Folder),
                Some(Message::SourceChosen(card.id.clone())),
                Weight::Secondary
            ),
        ]
        .spacing(theme::gap(Gap::Group))
        .width(Length::Fill)
        .into()
    } else {
        row![
            facts,
            Space::new().width(Length::Fill),
            micro_action(
                "Review folders",
                Some(Icon::Folder),
                Some(Message::SourceChosen(card.id.clone())),
                Weight::Secondary
            ),
        ]
        .spacing(theme::gap(Gap::Group))
        .align_y(Alignment::Center)
        .width(Length::Fill)
        .into()
    };

    container(
        column![header, hairline(), footer]
            .spacing(theme::gap(Gap::Group))
            .width(Length::Fill),
    )
    .style(|_theme| panel_style())
    .padding(tokens.spacing.content_pad)
    .width(Length::Fill)
    .into()
}

pub fn manager(view: Manager) -> Element<'static, Message> {
    let Manager {
        cards,
        selected,
        browser,
        width,
    } = view;
    let tokens = design::tokens();
    let page = span(width);
    let inner = inner_width(page);
    let stacked = !manager_split(page);
    let rail_w = if stacked { inner } else { rail_width(page) };
    let panel_w = panel_width(page);

    let active_card = browser.as_ref().and_then(|browser| {
        cards
            .iter()
            .find(|card| card.id == browser.source_id)
            .cloned()
    });

    let rail = source_rail(&cards, selected.as_deref(), rail_w, stacked);
    let panel = match browser {
        Some(browser) => folder_panel(browser, active_card, panel_w),
        None => empty_manager_panel(&cards, panel_w),
    };

    let workspace: Element<'static, Message> = if stacked {
        column![rail, hairline(), panel]
            .spacing(theme::gap(Gap::Rail))
            .width(Length::Fill)
            .into()
    } else {
        row![rail, panel]
            .spacing(RAIL_GAP)
            .align_y(Alignment::Start)
            .width(Length::Fill)
            .into()
    };

    let header = row![
        column![
            text("SOURCES")
                .size(tokens.type_scale.micro)
                .font(theme::semibold())
                .color(theme::muted()),
            text("Media sources")
                .size(tokens.type_scale.title_sm)
                .font(theme::semibold())
                .color(theme::ink()),
        ]
        .spacing(theme::gap(Gap::Tight)),
        Space::new().width(Length::Fill),
        text(fit(
            "Reads in place  ·  Nothing uploaded  ·  Nothing moved",
            inner * 0.5,
            tokens.type_scale.micro
        ))
        .size(tokens.type_scale.micro)
        .font(theme::regular())
        .color(theme::muted()),
    ]
    .spacing(theme::gap(Gap::Group))
    .align_y(Alignment::End);

    let body = column![header, hairline(), workspace]
        .spacing(theme::gap(Gap::Section))
        .width(Length::Fill);

    let content = container(body).width(Length::Fill).padding(Padding {
        top: theme::gap(Gap::Section),
        bottom: theme::gap(Gap::Section),
        left: page_pad(page),
        right: page_pad(page),
    });

    scrollable(content)
        .height(Length::Fill)
        .width(Length::Fill)
        .style(theme::scrollbar)
        .into()
}

fn source_rail(
    cards: &[SourceCard],
    selected: Option<&str>,
    width: f32,
    stacked: bool,
) -> Element<'static, Message> {
    let tokens = design::tokens();
    let count = match cards.len() {
        1 => "1 location".to_string(),
        count => format!("{count} locations"),
    };

    let mut list = column![
        row![
            text("LOCATIONS")
                .size(tokens.type_scale.micro)
                .font(theme::semibold())
                .color(theme::muted()),
            Space::new().width(Length::Fill),
            text(count)
                .size(tokens.type_scale.micro)
                .font(theme::regular())
                .color(theme::muted()),
        ]
        .align_y(Alignment::Center)
        .width(Length::Fill),
    ]
    .spacing(theme::gap(Gap::Card))
    .width(Length::Fill);

    if cards.is_empty() {
        list = list.push(
            text("No locations connected yet.")
                .size(tokens.type_scale.caption)
                .font(theme::regular())
                .color(theme::muted()),
        );
    } else {
        for card in cards {
            let active = selected == Some(card.id.as_str());
            list = list.push(rail_card(card, active, width));
        }
    }

    let add_location: Element<'static, Message> = container(action(
        "Add a location",
        Some(Icon::Plus),
        Some(Message::ChooseFolder),
        Weight::Secondary,
    ))
    .width(Length::Fill)
    .into();
    list = list.push(add_location);

    container(list)
        .width(if stacked {
            Length::Fill
        } else {
            Length::Fixed(width)
        })
        .into()
}

fn rail_style(active: bool) -> impl Fn(&Theme, button::Status) -> button::Style + 'static {
    move |_theme, status| {
        let palette = &design::tokens().palette;
        let (background, border) = match status {
            button::Status::Hovered | button::Status::Pressed => {
                (palette.surface2, Color::TRANSPARENT)
            }
            _ if active => (
                palette.surface2,
                motion::with_alpha(palette.foreground, 0.18),
            ),
            _ => (Color::TRANSPARENT, Color::TRANSPARENT),
        };
        button::Style {
            background: Some(Background::Color(background)),
            text_color: palette.foreground,
            border: Border {
                color: border,
                width: if active { HAIRLINE } else { 0.0 },
                radius: CornerRadius::from(theme::radius(Radius::Surface)),
            },
            shadow: Shadow::default(),
            snap: false,
        }
    }
}

fn marker(active: bool) -> Element<'static, Message> {
    container(
        Space::new()
            .width(Length::Fixed(8.0))
            .height(Length::Fixed(8.0)),
    )
    .style(move |_theme| container::Style {
        background: Some(Background::Color(if active {
            theme::signal()
        } else {
            motion::with_alpha(theme::ink(), 0.18)
        })),
        border: Border {
            radius: CornerRadius::from(theme::radius(Radius::Small)),
            ..Border::default()
        },
        ..container::Style::default()
    })
    .into()
}

fn rail_card(card: &SourceCard, active: bool, width: f32) -> Element<'static, Message> {
    let tokens = design::tokens();
    let room = (width - 110.0).max(90.0);
    let label = fit(&card.label, room, tokens.type_scale.label);
    let path = fit_path(&card.root_path, room, tokens.type_scale.micro);
    let state_colour = if card.state == SourceState::Available {
        theme::muted()
    } else {
        theme::ink()
    };

    let content = column![
        row![
            marker(active),
            text(label)
                .size(tokens.type_scale.label)
                .font(theme::medium())
                .color(theme::ink())
                .wrapping(Wrapping::None)
                .width(Length::Fill),
        ]
        .spacing(theme::gap(Gap::Card))
        .align_y(Alignment::Center)
        .width(Length::Fill),
        text(if card.scanning {
            "Scanning now".to_string()
        } else {
            card.state_label().to_string()
        })
        .size(tokens.type_scale.caption)
        .font(theme::regular())
        .color(state_colour),
        text(path)
            .size(tokens.type_scale.micro)
            .font(theme::regular())
            .color(theme::muted())
            .wrapping(Wrapping::None),
        text(format!(
            "{}  ·  {}",
            indexed_short(card.indexed),
            folders_short(&card.folders)
        ))
        .size(tokens.type_scale.micro)
        .font(theme::regular())
        .color(theme::muted()),
    ]
    .spacing(theme::gap(Gap::Tight))
    .width(Length::Fill);

    button(
        container(content)
            .padding(Padding {
                top: theme::gap(Gap::Group),
                bottom: theme::gap(Gap::Group),
                left: theme::gap(Gap::Group),
                right: theme::gap(Gap::Group),
            })
            .width(Length::Fill),
    )
    .on_press(Message::SourceChosen(card.id.clone()))
    .style(rail_style(active))
    .padding(0)
    .width(Length::Fill)
    .into()
}

fn empty_manager_panel(cards: &[SourceCard], width: f32) -> Element<'static, Message> {
    let tokens = design::tokens();
    let (title, detail) = if cards.is_empty() {
        (
            "No locations yet",
            "Add a folder, a drive or any connected storage to begin.",
        )
    } else {
        (
            "Choose a location",
            "Pick a location on the left to review and include its folders.",
        )
    };
    container(
        column![
            text(title)
                .size(tokens.type_scale.title_sm)
                .font(theme::semibold())
                .color(theme::ink()),
            text(detail)
                .size(tokens.type_scale.body)
                .font(theme::regular())
                .color(theme::muted())
                .width(Length::Fixed(width.min(BODY_MEASURE))),
            action(
                "Add a location",
                Some(Icon::Plus),
                Some(Message::ChooseFolder),
                Weight::Primary
            ),
        ]
        .spacing(theme::gap(Gap::Rail))
        .width(Length::Fill),
    )
    .width(Length::Fill)
    .padding(Padding {
        top: theme::gap(Gap::Section),
        bottom: theme::gap(Gap::Section),
        left: 0.0,
        right: 0.0,
    })
    .into()
}

fn folder_panel(
    browser: FolderBrowser,
    card: Option<SourceCard>,
    panel: f32,
) -> Element<'static, Message> {
    let tokens = design::tokens();
    let available = browser.state == SourceState::Available;
    let narrow = panel < 620.0;
    let title_room = if narrow {
        (panel - 150.0).max(120.0)
    } else {
        (panel * 0.6).max(120.0)
    };

    let title = fit(
        &browser.source_label,
        title_room,
        tokens.type_scale.title_sm,
    );
    let path = fit_path(&browser.source_root, panel * 0.8, tokens.type_scale.caption);

    let facts = match &card {
        Some(card) => format!(
            "{}  ·  {}  ·  {}",
            indexed_phrase(card.indexed),
            folders_phrase(&browser.selected, browser.folders_configured),
            scan_phrase(card)
        ),
        None => format!(
            "{}  ·  {}",
            folders_phrase(&browser.selected, browser.folders_configured),
            browser.state_label()
        ),
    };

    let state_tag = tag(
        match &card {
            Some(card) => card.state_label(),
            None => browser.state_label(),
        },
        None,
        if available { Tone::Muted } else { Tone::Danger },
    );

    let mut identity = column![
        text("LOCATION")
            .size(tokens.type_scale.micro)
            .font(theme::semibold())
            .color(theme::muted()),
        text(title)
            .size(tokens.type_scale.title_sm)
            .font(theme::semibold())
            .color(theme::ink()),
    ]
    .spacing(theme::gap(Gap::Tight))
    .width(Length::Fill);

    if !narrow {
        identity = identity.push(
            text(path.clone())
                .size(tokens.type_scale.caption)
                .font(theme::regular())
                .color(theme::muted())
                .wrapping(Wrapping::None),
        );
        identity = identity.push(
            text(facts.clone())
                .size(tokens.type_scale.caption)
                .font(theme::regular())
                .color(theme::muted()),
        );
    }

    let header = row![identity, state_tag]
        .spacing(theme::gap(Gap::Group))
        .align_y(Alignment::Start)
        .width(Length::Fill);

    let mut blocks: Vec<Element<'static, Message>> = vec![header.into()];
    if narrow {
        blocks.push(
            text(path)
                .size(tokens.type_scale.caption)
                .font(theme::regular())
                .color(theme::muted())
                .wrapping(Wrapping::None)
                .into(),
        );
        blocks.push(
            text(facts)
                .size(tokens.type_scale.caption)
                .font(theme::regular())
                .color(theme::muted())
                .into(),
        );
    }
    blocks.push(hairline());

    if !available {
        blocks.push(relink_block(&browser, panel));
    }

    if let Some(unreadable) = &browser.unreadable {
        blocks.push(unreadable_note(unreadable, &browser));
    }

    blocks.push(breadcrumbs(&browser, panel));
    blocks.push(everything_toggle(&browser, panel));
    blocks.push(folder_section(&browser, panel));
    blocks.push(selection_summary(&browser, panel));
    blocks.push(panel_actions(&browser, available, narrow));
    blocks.push(
        text("Keyboard: S scan · A select all · C clear · R rescan · N add location · Esc back")
            .size(tokens.type_scale.micro)
            .font(theme::regular())
            .color(theme::muted())
            .into(),
    );

    let mut notes: Vec<Element<'static, Message>> = Vec::new();
    if !available {
        notes.push(
            text("Reconnect this location before scanning.")
                .size(tokens.type_scale.micro)
                .font(theme::regular())
                .color(theme::danger())
                .into(),
        );
    } else if browser.selected.is_empty() {
        notes.push(
            text(if browser.folders_configured {
                "Include at least one folder to make a scan."
            } else {
                "This whole location is read until you save a folder selection."
            })
            .size(tokens.type_scale.micro)
            .font(theme::regular())
            .color(theme::muted())
            .into(),
        );
    }
    notes.push(
        text("Remove asks for a second press. Files on disk are never deleted.")
            .size(tokens.type_scale.micro)
            .font(theme::regular())
            .color(theme::muted())
            .into(),
    );
    blocks.push(column(notes).spacing(theme::gap(Gap::Tight)).into());

    column(blocks)
        .spacing(theme::gap(Gap::Group))
        .width(Length::Fill)
        .into()
}

impl FolderBrowser {
    fn state_label(&self) -> &'static str {
        match self.state {
            SourceState::Available => "Available",
            SourceState::Disconnected => "Disconnected",
            SourceState::PermissionLost => "Access lost",
            SourceState::Missing => "Folder moved or missing",
            SourceState::Removed => "Removed",
        }
    }
}

fn breadcrumbs(browser: &FolderBrowser, panel: f32) -> Element<'static, Message> {
    let tokens = design::tokens();
    let trail = if browser.breadcrumb.is_empty() {
        breadcrumb_for(&browser.current)
    } else {
        browser.breadcrumb.clone()
    };
    let room = (panel * 0.28).max(90.0);

    let mut crumbs = row![]
        .spacing(theme::gap(Gap::Tight))
        .align_y(Alignment::Center);

    if !browser.current.is_empty() {
        crumbs = crumbs.push(micro_action(
            "Up",
            Some(Icon::Back),
            Some(Message::FolderBrowseUp),
            Weight::Quiet,
        ));
        let slash: Element<'static, Message> = text("/")
            .size(tokens.type_scale.caption)
            .color(theme::muted())
            .into();
        crumbs = crumbs.push(slash);
    }

    let last = trail.len().saturating_sub(1);
    for (index, (label, path)) in trail.iter().enumerate() {
        let shown = fit(label, room, tokens.type_scale.caption);
        if index == last {
            let here: Element<'static, Message> = text(shown)
                .size(tokens.type_scale.caption)
                .font(theme::medium())
                .color(theme::ink())
                .into();
            crumbs = crumbs.push(here);
        } else {
            let crumb: Element<'static, Message> = button(
                text(shown)
                    .size(tokens.type_scale.caption)
                    .font(theme::regular())
                    .color(theme::muted()),
            )
            .on_press(Message::FolderBrowseInto(path.clone()))
            .style(design::quiet_style(false))
            .padding(Padding {
                top: 3.0,
                bottom: 3.0,
                left: theme::gap(Gap::Card),
                right: theme::gap(Gap::Card),
            })
            .into();
            crumbs = crumbs.push(crumb);
            let separator: Element<'static, Message> = text("›")
                .size(tokens.type_scale.caption)
                .color(theme::muted())
                .into();
            crumbs = crumbs.push(separator);
        }
    }

    crumbs.wrap().into()
}

fn toggle_style(
    active: bool,
    focused: bool,
) -> impl Fn(&Theme, button::Status) -> button::Style + 'static {
    move |_theme, status| {
        let palette = &design::tokens().palette;
        let corner = CornerRadius::from(theme::radius(Radius::Surface));
        if focused {
            return button::Style {
                background: Some(Background::Color(palette.focus_surface)),
                text_color: palette.focus_foreground,
                border: Border {
                    color: Color::TRANSPARENT,
                    width: 0.0,
                    radius: corner,
                },
                shadow: Shadow::default(),
                snap: false,
            };
        }
        let (background, border) = match status {
            button::Status::Hovered => (
                palette.surface2,
                motion::with_alpha(palette.foreground, 0.2),
            ),
            button::Status::Pressed => (
                palette.surface3,
                motion::with_alpha(palette.foreground, 0.24),
            ),
            _ if active => (
                motion::with_alpha(palette.signal, 0.08),
                motion::with_alpha(palette.signal, 0.3),
            ),
            _ => (Color::TRANSPARENT, palette.hair),
        };
        button::Style {
            background: Some(Background::Color(background)),
            text_color: palette.foreground,
            border: Border {
                color: border,
                width: HAIRLINE,
                radius: corner,
            },
            shadow: Shadow::default(),
            snap: false,
        }
    }
}

fn everything_toggle(browser: &FolderBrowser, panel: f32) -> Element<'static, Message> {
    let tokens = design::tokens();
    let focused = browser.focus == Some(0);
    let state = inclusion(&browser.selected, &browser.current);
    let active = state != Inclusion::Excluded;
    let room = (panel - 220.0).max(120.0);
    let title = fit("Everything in this folder", room, tokens.type_scale.label);
    let subtitle = format!(
        "{}  ·  {}",
        media_phrase(browser.media_here),
        toggle_hint(state)
    );

    button(
        container(
            row![
                icons::icon(
                    if state == Inclusion::Excluded {
                        Icon::Plus
                    } else {
                        Icon::Check
                    },
                    14.0,
                    if active { theme::ink() } else { theme::muted() },
                ),
                column![
                    text(title)
                        .size(tokens.type_scale.label)
                        .font(theme::medium())
                        .color(theme::ink())
                        .wrapping(Wrapping::None),
                    text(fit(
                        &subtitle,
                        (panel - 260.0).max(120.0),
                        tokens.type_scale.caption
                    ))
                    .size(tokens.type_scale.caption)
                    .font(theme::regular())
                    .color(theme::muted())
                    .wrapping(Wrapping::None),
                ]
                .spacing(theme::gap(Gap::Tight))
                .width(Length::Fill),
                inclusion_tag(state),
            ]
            .spacing(theme::gap(Gap::Card))
            .align_y(Alignment::Center)
            .width(Length::Fill),
        )
        .padding(Padding {
            top: theme::gap(Gap::Group),
            bottom: theme::gap(Gap::Group),
            left: theme::gap(Gap::Group),
            right: theme::gap(Gap::Group),
        })
        .width(Length::Fill),
    )
    .on_press(Message::FolderToggle(browser.current.clone()))
    .style(toggle_style(active, focused))
    .padding(0)
    .width(Length::Fill)
    .into()
}

fn folder_row_style(state: Inclusion, focused: bool) -> container::Style {
    if focused {
        return container::Style {
            background: Some(Background::Color(design::tokens().palette.focus_surface)),
            text_color: Some(design::tokens().palette.focus_foreground),
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: CornerRadius::from(theme::radius(Radius::Control)),
            },
            shadow: Shadow::default(),
            snap: true,
        };
    }
    let background = match state {
        Inclusion::Selected => motion::with_alpha(theme::signal(), 0.06),
        Inclusion::Included => motion::with_alpha(theme::ink(), 0.03),
        Inclusion::Excluded => Color::TRANSPARENT,
    };
    container::Style {
        background: Some(Background::Color(background)),
        text_color: Some(theme::ink()),
        border: Border {
            color: theme::hair(),
            width: HAIRLINE,
            radius: CornerRadius::from(theme::radius(Radius::Control)),
        },
        shadow: Shadow::default(),
        snap: true,
    }
}

fn folder_section(browser: &FolderBrowser, panel: f32) -> Element<'static, Message> {
    let tokens = design::tokens();
    let narrow = panel < 620.0;

    let mut list = column![].spacing(theme::gap(Gap::Card)).width(Length::Fill);
    if browser.entries.is_empty() {
        list = list.push(
            text("No folders here. Files directly inside are counted above.")
                .size(tokens.type_scale.caption)
                .font(theme::regular())
                .color(theme::muted()),
        );
    } else {
        for entry in browser.entries.iter().take(VISIBLE_FOLDERS) {
            let index = browser
                .entries
                .iter()
                .position(|candidate| candidate.relative_path == entry.relative_path)
                .unwrap_or(0);
            let focused = browser.focus == Some(index + 1);
            list = list.push(folder_row(entry, &browser.selected, panel, narrow, focused));
        }
    }

    let shown = browser.entries.len().min(VISIBLE_FOLDERS);
    let note = if browser.entries.len() > VISIBLE_FOLDERS {
        Some(format!(
            "Showing the first {shown} of {} folders in this location.",
            browser.entries.len()
        ))
    } else {
        None
    };

    let mut section = column![
        row![
            text("FOLDERS HERE")
                .size(tokens.type_scale.micro)
                .font(theme::semibold())
                .color(theme::muted()),
            Space::new().width(Length::Fill),
            text(format!(
                "{} of {} shown",
                browser.entries.len().min(VISIBLE_FOLDERS),
                browser.entries.len()
            ))
            .size(tokens.type_scale.micro)
            .font(theme::regular())
            .color(theme::muted()),
        ]
        .align_y(Alignment::Center)
        .width(Length::Fill),
        text(if browser.folders_configured {
            "Included folders are scanned. Including a folder includes everything inside it."
        } else {
            "No folder selection is saved yet, so a scan reads this whole location. Include \
             folders to narrow it."
        })
        .size(tokens.type_scale.caption)
        .font(theme::regular())
        .color(theme::muted()),
        list,
    ]
    .spacing(theme::gap(Gap::Group))
    .width(Length::Fill);

    if let Some(note) = note {
        section = section.push(
            text(note)
                .size(tokens.type_scale.micro)
                .font(theme::regular())
                .color(theme::muted()),
        );
    }

    section.into()
}

fn folder_row(
    entry: &FolderEntry,
    selected: &[String],
    panel: f32,
    narrow: bool,
    focused: bool,
) -> Element<'static, Message> {
    let tokens = design::tokens();
    let state = inclusion(selected, &entry.relative_path);
    let room = if narrow {
        (panel - 170.0).max(90.0)
    } else {
        (panel - 430.0).max(120.0)
    };
    let name = fit(&entry.name, room, tokens.type_scale.label);
    let facts = media_phrase(entry.media);
    let issue = issue_label(entry.issue);

    let mut identity = row![
        icons::icon(
            Icon::Folder,
            14.0,
            if state == Inclusion::Excluded {
                theme::muted()
            } else {
                theme::ink()
            },
        ),
        text(name)
            .size(tokens.type_scale.label)
            .font(theme::medium())
            .color(theme::ink())
            .wrapping(Wrapping::None)
            .width(Length::Fill),
    ]
    .spacing(theme::gap(Gap::Card))
    .align_y(Alignment::Center);

    if let Some(issue) = issue {
        identity = identity.push(tag(issue, None, Tone::Danger));
    }

    let inclusion_control = micro_action(
        inclusion_action(state),
        if state == Inclusion::Excluded {
            Some(Icon::Plus)
        } else {
            Some(Icon::Check)
        },
        Some(Message::FolderToggle(entry.relative_path.clone())),
        if state == Inclusion::Excluded {
            Weight::Secondary
        } else {
            Weight::Quiet
        },
    );

    let open = micro_action(
        "Open",
        None,
        Some(Message::FolderBrowseInto(entry.relative_path.clone())),
        Weight::Quiet,
    );

    let line = column![
        identity,
        text(facts)
            .size(tokens.type_scale.caption)
            .font(theme::regular())
            .color(theme::muted()),
    ]
    .spacing(theme::gap(Gap::Tight))
    .width(Length::Fill);

    let content: Element<'static, Message> = if narrow {
        column![
            line,
            row![
                inclusion_tag(state),
                Space::new().width(Length::Fill),
                inclusion_control,
                open
            ]
            .spacing(theme::gap(Gap::Card))
            .align_y(Alignment::Center)
            .width(Length::Fill),
        ]
        .spacing(theme::gap(Gap::Group))
        .width(Length::Fill)
        .into()
    } else {
        row![line, inclusion_tag(state), inclusion_control, open,]
            .spacing(theme::gap(Gap::Group))
            .align_y(Alignment::Center)
            .width(Length::Fill)
            .into()
    };

    container(content)
        .style(move |_theme| folder_row_style(state, focused))
        .padding(Padding {
            top: theme::gap(Gap::Group),
            bottom: theme::gap(Gap::Group),
            left: theme::gap(Gap::Group),
            right: theme::gap(Gap::Group),
        })
        .width(Length::Fill)
        .into()
}

fn unreadable_note(message: &str, browser: &FolderBrowser) -> Element<'static, Message> {
    let tokens = design::tokens();
    let mut body = column![
        text("This folder cannot be read")
            .size(tokens.type_scale.label)
            .font(theme::semibold())
            .color(theme::ink()),
        text(message.to_string())
            .size(tokens.type_scale.caption)
            .font(theme::regular())
            .color(theme::ink()),
    ]
    .spacing(theme::gap(Gap::Card));

    if !browser.current.is_empty() {
        body = body.push(micro_action(
            "Go up a folder",
            Some(Icon::Back),
            Some(Message::FolderBrowseUp),
            Weight::Secondary,
        ));
    }

    container(body)
        .style(|_theme| danger_panel_style())
        .padding(tokens.spacing.content_pad)
        .width(Length::Fill)
        .into()
}

fn relink_block(browser: &FolderBrowser, panel: f32) -> Element<'static, Message> {
    let tokens = design::tokens();
    let narrow = panel < 620.0;
    let note = relink_note(browser.state);
    let enabled = !browser.relink_path.trim().is_empty();

    let input = text_input("New path for this location", &browser.relink_path)
        .on_input(Message::RelinkPathChanged)
        .on_submit(Message::RelinkSource)
        .style(design::input_style())
        .padding(tokens.spacing.field_y)
        .width(Length::Fill);

    let choose = action(
        "Choose folder…",
        Some(Icon::Folder),
        Some(Message::ChooseRelinkFolder),
        Weight::Secondary,
    );
    let relink = action(
        "Relink",
        None,
        enabled.then_some(Message::RelinkSource),
        Weight::Primary,
    );

    let controls: Element<'static, Message> = if narrow {
        column![input, row![choose, relink].spacing(theme::gap(Gap::Card))]
            .spacing(theme::gap(Gap::Card))
            .into()
    } else {
        row![input, choose, relink]
            .spacing(theme::gap(Gap::Card))
            .align_y(Alignment::Center)
            .into()
    };

    container(
        column![
            text("Reconnect this location")
                .size(tokens.type_scale.label)
                .font(theme::semibold())
                .color(theme::ink()),
            text(note.to_string())
                .size(tokens.type_scale.caption)
                .font(theme::regular())
                .color(theme::muted()),
            controls,
            text("Typing the path directly works too. Even when the folder has moved, its indexed titles stay.")
                .size(tokens.type_scale.micro)
                .font(theme::regular())
                .color(theme::muted()),
        ]
        .spacing(theme::gap(Gap::Group))
        .width(Length::Fill),
    )
    .style(|_theme| raised_style())
    .padding(tokens.spacing.content_pad)
    .width(Length::Fill)
    .into()
}

fn selection_summary(browser: &FolderBrowser, panel: f32) -> Element<'static, Message> {
    let tokens = design::tokens();
    let intent = scan_intent(&browser.selected, browser.folders_configured);
    let mut block = column![
        text("WHAT A SCAN WILL READ")
            .size(tokens.type_scale.micro)
            .font(theme::semibold())
            .color(theme::muted()),
        text(intent)
            .size(tokens.type_scale.label)
            .font(theme::medium())
            .color(theme::ink()),
    ]
    .spacing(theme::gap(Gap::Card))
    .width(Length::Fill);

    if let Some(note) = selection_note(&browser.selected, browser.folders_configured) {
        block = block.push(
            text(note)
                .size(tokens.type_scale.caption)
                .font(theme::regular())
                .color(theme::muted()),
        );
    }

    if !browser.selected.is_empty() {
        let mut chips = row![]
            .spacing(theme::gap(Gap::Card))
            .align_y(Alignment::Center);
        for entry in browser.selected.iter().take(4) {
            let label = format!("{} · Remove", folder_label(entry));
            let shown = fit(&label, panel * 0.45, tokens.type_scale.caption);
            chips = chips.push(micro_action(
                shown,
                None,
                Some(Message::FolderToggle(entry.clone())),
                Weight::Quiet,
            ));
        }
        if browser.selected.len() > 4 {
            let rest: Element<'static, Message> =
                text(format!("+ {} more selected", browser.selected.len() - 4))
                    .size(tokens.type_scale.caption)
                    .font(theme::regular())
                    .color(theme::muted())
                    .into();
            chips = chips.push(rest);
        }
        block = block.push(chips.wrap());
    }

    container(column![hairline(), block].spacing(theme::gap(Gap::Group)))
        .width(Length::Fill)
        .into()
}

fn panel_actions(
    browser: &FolderBrowser,
    available: bool,
    narrow: bool,
) -> Element<'static, Message> {
    let scan_enabled = available && !browser.selected.is_empty();
    let scan = action(
        "Scan selected folders",
        None,
        scan_enabled.then_some(Message::SaveSourceFolders),
        Weight::Primary,
    );
    let select_all = action(
        "Select all shown",
        None,
        (!browser.entries.is_empty()).then_some(Message::FolderSelectAll),
        Weight::Secondary,
    );
    let clear = action(
        "Clear selection",
        None,
        (!browser.selected.is_empty()).then_some(Message::FolderClearSelection),
        Weight::Quiet,
    );
    let rescan = action(
        "Rescan",
        None,
        available.then_some(Message::RescanSource(browser.source_id.clone())),
        Weight::Quiet,
    );
    let remove = action(
        "Remove location",
        None,
        Some(Message::RemoveSource(browser.source_id.clone())),
        Weight::Hazard,
    );

    if narrow {
        column![scan, select_all, clear, rescan, remove,]
            .spacing(theme::gap(Gap::Card))
            .width(Length::Fill)
            .into()
    } else {
        row![scan, select_all, clear, rescan, remove]
            .spacing(theme::gap(Gap::Group))
            .align_y(Alignment::Center)
            .wrap()
            .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIDTHS: [f32; 6] = [600.0, 900.0, 1100.0, 1440.0, 1760.0, 3440.0];

    fn card(id: &str, label: &str, indexed: usize, folders: Vec<&str>) -> SourceCard {
        let folders: Vec<String> = folders.into_iter().map(str::to_string).collect();
        SourceCard {
            id: id.to_string(),
            label: label.to_string(),
            root_path: format!("/Volumes/Library/{label}"),
            state: SourceState::Available,
            folders_configured: !folders.is_empty(),
            folders,
            indexed,
            total: indexed,
            scanning: false,
            last_scan: Some("4 minutes ago".to_string()),
        }
    }

    fn entry(relative: &str, media: usize) -> FolderEntry {
        FolderEntry {
            relative_path: relative.to_string(),
            name: folder_label(relative),
            media,
            subfolders: 0,
            issue: FolderIssue::Available,
        }
    }

    fn browser() -> FolderBrowser {
        FolderBrowser {
            source_id: "films".to_string(),
            source_label: "Films".to_string(),
            source_root: "/Volumes/Library/Films".to_string(),
            state: SourceState::Available,
            current: "Films".to_string(),
            breadcrumb: breadcrumb_for("Films"),
            entries: vec![entry("Films/2024", 4), entry("Films/Extras", 0)],
            media_here: 2,
            selected: vec!["Films".to_string()],
            folders_configured: true,
            relink_path: String::new(),
            unreadable: None,
            focus: None,
        }
    }

    #[test]
    fn a_parent_selection_includes_its_descendants() {
        let selected = vec!["Films".to_string()];
        assert_eq!(inclusion(&selected, "Films"), Inclusion::Selected);
        assert_eq!(inclusion(&selected, "Films/2024"), Inclusion::Included);
        assert_eq!(inclusion(&selected, "Series"), Inclusion::Excluded);
        assert_eq!(inclusion(&[String::new()], "Anything"), Inclusion::Included);
    }

    #[test]
    fn a_root_selection_absorbs_every_other_selection() {
        assert_eq!(
            normalize(&["Films".to_string(), String::new(), "Series".to_string()]),
            vec![String::new()]
        );
        assert_eq!(
            normalize(&["Films".to_string(), "Series".to_string(), String::new()]),
            vec![String::new()]
        );
        assert_eq!(
            normalize(&["/".to_string(), "Films".to_string()]),
            vec![String::new()]
        );
    }

    #[test]
    fn sibling_selections_survive_normalisation() {
        assert_eq!(
            normalize(&[
                "Films".to_string(),
                "Series".to_string(),
                "Films".to_string()
            ]),
            vec!["Films".to_string(), "Series".to_string()]
        );
    }

    #[test]
    fn a_breadcrumb_always_starts_at_the_location() {
        assert_eq!(
            breadcrumb_for(""),
            vec![("This location".to_string(), String::new())]
        );
        assert_eq!(
            breadcrumb_for("Films/2024"),
            vec![
                ("This location".to_string(), String::new()),
                ("Films".to_string(), "Films".to_string()),
                ("2024".to_string(), "Films/2024".to_string()),
            ]
        );
    }

    #[test]
    fn a_parent_and_label_follow_the_relative_path() {
        assert_eq!(parent_of(""), "");
        assert_eq!(parent_of("Films"), "");
        assert_eq!(parent_of("Films/2024"), "Films");
        assert_eq!(parent_of("Films/2024/Extras"), "Films/2024");
        assert_eq!(folder_label(""), "Everything in this folder");
        assert_eq!(folder_label("Films"), "Films");
        assert_eq!(folder_label("Films/2024"), "2024");
    }

    #[test]
    fn the_copy_never_claims_media_when_nothing_is_indexed() {
        let empty = card("films", "Films", 0, vec!["Films"]);
        assert_eq!(empty.state_label(), "No media indexed");
        let phrase = indexed_phrase(0);
        assert!(phrase.to_lowercase().contains("no media"), "{phrase}");
        assert!(!phrase.contains(char::is_numeric), "{phrase}");
        assert!(!phrase.to_lowercase().contains("title"), "{phrase}");
        assert_eq!(indexed_short(0), "Nothing indexed");
    }

    #[test]
    fn the_copy_only_counts_folders_that_are_actually_included() {
        let none: Vec<String> = Vec::new();
        assert_eq!(inclusion_label(Inclusion::Excluded), "Excluded");
        assert_eq!(inclusion_action(Inclusion::Excluded), "Include");
        assert_eq!(folders_phrase(&none, true), "No folders selected");
        assert_eq!(
            folders_phrase(&none, false),
            "No folder selection saved yet"
        );

        let one = vec!["Films".to_string()];
        let two = vec!["Films".to_string(), "Series".to_string()];
        assert_eq!(folders_phrase(&one, true), "1 folder included");
        assert_eq!(folders_phrase(&two, true), "2 folders included");
        assert_eq!(folders_short(&one), "1 folder");

        assert!(scan_intent(&none, true).contains("Nothing selected yet"));
        assert!(scan_intent(&none, true).contains("will not add any media"));
        assert_eq!(
            scan_intent(&one, true),
            "A scan will read 1 selected folder."
        );
        assert_eq!(
            scan_intent(&two, true),
            "A scan will read 2 selected folders."
        );
        assert!(!scan_intent(&one, true).contains("everything"));
    }

    #[test]
    fn a_saved_location_without_folders_says_so_instead_of_pretending() {
        let none: Vec<String> = Vec::new();
        assert!(selection_note(&none, true).is_some());
        assert!(selection_note(&none, false).is_some());
        assert!(selection_note(&["Films".to_string()], true).is_none());
    }

    #[test]
    fn an_unreadable_folder_gets_a_recovery_path_named() {
        assert!(issue_label(FolderIssue::Available).is_none());
        assert_eq!(issue_label(FolderIssue::Missing), Some("Missing"));
        assert_eq!(issue_label(FolderIssue::PermissionLost), Some("No access"));
        assert!(!relink_note(SourceState::Missing).is_empty());
        assert!(!relink_note(SourceState::PermissionLost).is_empty());
        assert!(!relink_note(SourceState::Disconnected).is_empty());
    }

    #[test]
    fn a_folder_with_no_indexed_media_never_claims_a_number() {
        assert_eq!(media_phrase(0), "No indexed media inside");
        assert_eq!(media_phrase(1), "1 indexed item inside");
        assert_eq!(media_phrase(9), "9 indexed items inside");
        assert!(!media_phrase(0).contains(char::is_numeric));
    }

    #[test]
    fn truncation_helpers_stay_on_character_boundaries_and_inside_the_budget() {
        let ascii = "Films from the 1970s restored in 4K with commentary and subtitles";
        for budget in [0_usize, 1, 2, 5, 12, 40, 90] {
            let cut = end_ellipsis(ascii, budget);
            assert!(cut.chars().count() <= budget, "{cut} exceeds {budget}");
            assert!(!cut.contains('\u{FFFD}'));
        }
        let cjk = "映画のフォルダがとても長い場合のテスト用ラベルです";
        for budget in [1_usize, 3, 8, 20] {
            let cut = middle_ellipsis(cjk, budget);
            assert!(cut.chars().count() <= budget);
            assert!(!cut.contains('\u{FFFD}'));
        }
        assert_eq!(end_ellipsis("Short", 40), "Short");
        assert_eq!(middle_ellipsis("Short", 40), "Short");
        assert_eq!(end_ellipsis("Short", 0), "");
        assert_eq!(middle_ellipsis("Short", 0), "");
    }

    #[test]
    fn a_useless_width_never_produces_a_panic_or_a_negative_room() {
        assert_eq!(char_budget(f32::NAN, 14.0), 0);
        assert_eq!(char_budget(120.0, f32::NAN), 0);
        assert_eq!(char_budget(-40.0, 14.0), 0);
        assert!(char_budget(320.0, 14.0) > 0);
        assert_eq!(
            welcome_measure(f32::NAN),
            WELCOME_MEASURE.min(FALLBACK_WIDTH - 2.0 * theme::PAD)
        );
        assert_eq!(
            inner_width(f32::NAN),
            FALLBACK_WIDTH - 2.0 * page_pad(FALLBACK_WIDTH)
        );
        assert!(display_size(f32::NAN).is_finite());
        assert!(panel_width(f32::NAN) > 0.0);
        assert!(rail_width(f32::NAN) > 0.0);
    }

    #[test]
    fn the_display_type_grows_with_the_window_and_stays_readable() {
        let mut previous = 0.0;
        for width in WIDTHS {
            let size = display_size(width);
            assert!(size.is_finite() && size > 32.0, "{width}: {size}");
            assert!(size >= previous, "{width} shrank the headline");
            previous = size;
        }
        assert!(!manager_split(600.0));
        assert!(manager_split(900.0));
        assert!(manager_split(f32::NAN));
    }

    #[test]
    fn every_view_composes_at_every_width_with_empty_data() {
        for width in WIDTHS.into_iter().chain([f32::NAN, 0.0, -50.0]) {
            let _ = welcome(Welcome {
                width,
                path: "",
                busy: false,
                error: None,
            });
            let _ = empty_library(EmptyLibrary {
                cards: Vec::new(),
                width,
                scanning: false,
            });
            let _ = manager(Manager {
                cards: Vec::new(),
                selected: None,
                browser: None,
                width,
            });
        }
    }

    #[test]
    fn every_view_composes_at_every_width_with_populated_data() {
        for width in WIDTHS.into_iter().chain([f32::NAN]) {
            let _ = welcome(Welcome {
                width,
                path: "/Volumes/Media",
                busy: true,
                error: Some("That folder is not absolute."),
            });
            let _ = empty_library(EmptyLibrary {
                cards: vec![
                    card("films", "Films", 0, vec!["Films/2024"]),
                    card("series", "Series", 0, Vec::new()),
                ],
                width,
                scanning: true,
            });
            let _ = manager(Manager {
                cards: vec![card("films", "Films", 12, vec!["Films"])],
                selected: Some("films".to_string()),
                browser: Some(browser()),
                width,
            });
            let _ = manager(Manager {
                cards: vec![card("films", "Films", 0, Vec::new())],
                selected: Some("films".to_string()),
                browser: None,
                width,
            });
        }
    }

    #[test]
    fn every_view_composes_with_long_labels_paths_and_folder_names() {
        let long_label = "A".repeat(240);
        let long_path = format!("/Volumes/{}/{}", "B".repeat(120), "C".repeat(120));
        let long_name = "映画".repeat(80);
        for width in WIDTHS.into_iter().chain([f32::NAN]) {
            let _ = welcome(Welcome {
                width,
                path: &long_path,
                busy: false,
                error: Some(&long_path),
            });
            let mut source = card("long", &long_label, 3, vec!["Films"]);
            source.root_path = long_path.clone();
            let _ = empty_library(EmptyLibrary {
                cards: vec![source.clone()],
                width,
                scanning: false,
            });

            let mut page = browser();
            page.source_label = long_label.clone();
            page.source_root = long_path.clone();
            page.current = "Films/2024".to_string();
            page.entries = vec![
                entry("Films/2024/aaaa", 2),
                FolderEntry {
                    relative_path: "Films/2024/long".to_string(),
                    name: long_name.clone(),
                    media: 0,
                    subfolders: 0,
                    issue: FolderIssue::Missing,
                },
            ];
            page.unreadable = Some("This folder cannot be read. Grant access again.".to_string());
            let _ = manager(Manager {
                cards: vec![source],
                selected: Some("long".to_string()),
                browser: Some(page),
                width,
            });
        }
    }

    #[test]
    fn an_unavailable_location_is_composed_with_relink_controls() {
        for width in WIDTHS {
            let mut page = browser();
            page.state = SourceState::Missing;
            page.unreadable =
                Some("This folder is no longer there. Relink the location.".to_string());
            let _ = manager(Manager {
                cards: vec![card("films", "Films", 0, Vec::new())],
                selected: Some("films".to_string()),
                browser: Some(page),
                width,
            });

            let mut lost = browser();
            lost.state = SourceState::PermissionLost;
            lost.relink_path = "/Volumes/Moved".to_string();
            let _ = manager(Manager {
                cards: vec![card("films", "Films", 0, Vec::new())],
                selected: Some("films".to_string()),
                browser: Some(lost),
                width,
            });
        }
    }

    #[test]
    fn an_unreachable_location_is_not_reported_as_an_empty_search() {
        let mut source = card("drive", "External SSD", 40, vec!["Films"]);
        source.state = SourceState::Disconnected;
        source.last_scan = Some("2 hours ago".to_string());
        let (headline, copy) = empty_state_copy(&[source], false);
        assert!(headline.contains("offline"), "{headline}");
        assert!(copy.contains("cannot read"), "{copy}");
        assert!(copy.contains("kept"), "{copy}");
        assert!(
            !copy.contains("looked where you pointed it"),
            "an absent drive is not an empty search: {copy}"
        );
    }

    #[test]
    fn an_unscanned_location_does_not_claim_a_finished_search() {
        let mut source = card("new", "New location", 0, Vec::new());
        source.folders_configured = true;
        source.last_scan = None;
        let (_, copy) = empty_state_copy(&[source], false);
        assert!(copy.contains("No folders are included yet"), "{copy}");
        assert!(!copy.contains("looked where"), "{copy}");
    }

    #[test]
    fn a_source_with_no_selection_says_nothing_is_selected() {
        let mut page = browser();
        page.selected = Vec::new();
        page.folders_configured = true;
        assert!(
            scan_intent(&page.selected, page.folders_configured).contains("Nothing selected yet")
        );
        assert!(selection_note(&page.selected, page.folders_configured).is_some());
        assert!(
            scan_intent(&page.selected, false).contains("whole location"),
            "a legacy source is read in full until a selection is saved"
        );
        let _ = manager(Manager {
            cards: vec![card("films", "Films", 0, Vec::new())],
            selected: Some("films".to_string()),
            browser: Some(page),
            width: 1440.0,
        });
    }

    #[test]
    fn a_long_selection_is_summarised_without_growing_without_bound() {
        let mut page = browser();
        page.selected = (0..30).map(|index| format!("Folder {index}")).collect();
        assert!(
            scan_intent(&page.selected, page.folders_configured).contains("30 selected folders")
        );
        let mut source = card("films", "Films", 90, Vec::new());
        source.folders = page.selected.clone();
        source.folders_configured = true;
        let _ = manager(Manager {
            cards: vec![source],
            selected: Some("films".to_string()),
            browser: Some(page),
            width: 900.0,
        });
    }

    #[test]
    fn every_composed_state_carries_a_real_action() {
        let _ = manager(Manager {
            cards: vec![card("films", "Films", 12, vec!["Films"])],
            selected: Some("films".to_string()),
            browser: Some(browser()),
            width: 1440.0,
        });

        assert_eq!(inclusion_action(Inclusion::Selected), "Exclude");
        assert_eq!(inclusion_action(Inclusion::Included), "Included above");
        assert_eq!(
            toggle_hint(Inclusion::Excluded),
            "Press to include this folder"
        );
        assert_eq!(
            selection_note(&["Films".to_string()], true),
            None,
            "a configured selection never needs a warning"
        );
    }

    #[test]
    fn the_number_of_locations_in_the_rail_reads_as_a_count() {
        assert_eq!(folders_short(&[]), "No folders chosen");
        assert_eq!(folders_short(&["".to_string()]), "Everything included");
        assert_eq!(
            folders_short(&["Films".to_string(), "Series".to_string()]),
            "2 folders"
        );
        assert_eq!(indexed_short(12), "12 titles");
        assert_eq!(indexed_phrase(12), "12 titles indexed");
        assert_eq!(
            scan_phrase(&card("films", "Films", 1, vec!["Films"])),
            "Scanned 4 minutes ago"
        );
    }
}
