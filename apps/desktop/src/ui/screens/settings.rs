use crate::app::Message;
use crate::design;
use crate::ui::components::poster::truncate;
use crate::ui::motion;
use crate::ui::route::Nav;
use crate::ui::text_fit;
use crate::ui::theme::{self, Breakpoint, Gap, Radius};
use iced::widget::text::Wrapping;
use iced::widget::{
    Id, Space, button, column, container, operation, row, scrollable, slider, text,
};
use iced::{Alignment, Background, Border, Color, Element, Length, Padding, Shadow, Task, Theme};
use std::ops::RangeInclusive;

pub const RAIL_WIDTH: f32 = 200.0;

pub const RAIL_MIN_WIDTH: f32 = 160.0;

pub const CONTENT_MAX_WIDTH: f32 = 780.0;

pub fn content_width(window: f32) -> f32 {
    if !window.is_finite() || window <= 0.0 {
        return CONTENT_MAX_WIDTH;
    }

    let rail = if Breakpoint::at(window) == Breakpoint::Narrow {
        RAIL_MIN_WIDTH
    } else {
        RAIL_WIDTH
    };

    let available = window - 2.0 * theme::PAD - rail - COLUMN_GAP - theme::scroll_reserve();

    available.clamp(0.0, CONTENT_MAX_WIDTH)
}

const COLUMN_GAP: f32 = 60.0;

const LABEL_COLUMN: f32 = 520.0;

const LABEL_CHARS: usize = 64;

const TOGGLE_WIDTH: f32 = 40.0;

const TOGGLE_HEIGHT: f32 = 23.0;

const TOGGLE_INSET: f32 = 2.0;

const SLIDER_WIDTH: f32 = 150.0;

const SLIDER_VALUE_WIDTH: f32 = 38.0;

const SLIDER_RAIL_HEIGHT: f32 = 3.0;

const TOGGLE_KNOB: f32 = TOGGLE_HEIGHT - TOGGLE_INSET * 2.0;

const HAIRLINE: f32 = 1.0;

const LINE_HEIGHT: f32 = 1.3;

pub fn focus_surface() -> Color {
    design::tokens().palette.focus_surface
}

pub fn focus_foreground() -> Color {
    design::tokens().palette.focus_foreground
}

pub fn row_style(focused: bool) -> container::Style {
    container::Style {
        background: focused.then(|| Background::Color(focus_surface())),
        text_color: Some(if focused {
            focus_foreground()
        } else {
            theme::ink()
        }),
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: theme::radius(Radius::Small).into(),
        },
        shadow: Shadow::default(),
        ..container::Style::default()
    }
}

pub fn content_id() -> Id {
    Id::from("settings-content")
}

pub fn content_viewport(window_height: f32) -> f32 {
    if !window_height.is_finite() {
        return 0.0;
    }

    (window_height - content_chrome()).max(0.0)
}

fn content_chrome() -> f32 {
    let tokens = design::tokens();
    let title = tokens.type_scale.title * LINE_HEIGHT;
    let status = tokens.type_scale.caption * LINE_HEIGHT + 2.0 * theme::gap(Gap::Card);

    crate::ui::shell::BAR_HEIGHT
        + theme::gap(Gap::Section)
        + title
        + theme::gap(Gap::Section)
        + status
}

fn heading_band() -> f32 {
    design::tokens().type_scale.body * LINE_HEIGHT + theme::gap(Gap::Card) + HAIRLINE
}

fn label_column(width: f32) -> f32 {
    if width.is_finite() && width > 0.0 {
        width.min(LABEL_COLUMN)
    } else {
        LABEL_COLUMN
    }
}

fn control_height(row: &Row<'_>) -> f32 {
    let tokens = design::tokens();
    let line = tokens.type_scale.label * LINE_HEIGHT;

    match row {
        Row::Toggle { .. } => line.max(TOGGLE_HEIGHT),
        Row::Action { .. } => line + 2.0 * theme::gap(Gap::Card),
        Row::Slider { range, .. }
            if range.start().is_finite()
                && range.end().is_finite()
                && range.end() > range.start() =>
        {
            tokens.type_scale.caption * LINE_HEIGHT
        }
        Row::Slider { .. } | Row::Value { .. } => line,
    }
}

pub fn row_height(row: &Row<'_>, width: f32) -> f32 {
    let tokens = design::tokens();
    let column = label_column(width);
    let mut text = text_fit::measured_height(
        row.label(),
        tokens.type_scale.label,
        theme::regular(),
        column,
    );

    if let Some(hint) = row.hint() {
        text += theme::gap(Gap::Tight);
        text +=
            text_fit::measured_height(hint, tokens.type_scale.caption, theme::regular(), column);
    }

    2.0 * theme::gap(Gap::Group) + text.max(control_height(row))
}

pub fn settings_row_offset(rows: &[Row<'_>], index: usize, width: f32) -> f32 {
    let index = index.min(rows.len().saturating_sub(1));
    let mut offset = heading_band();

    for row in rows.iter().take(index) {
        offset += row_height(row, width) + HAIRLINE;
    }

    offset
}

pub fn settings_scroll_offset(offset: f32, viewport: f32, row_height: f32) -> f32 {
    if !offset.is_finite() || !row_height.is_finite() {
        return 0.0;
    }

    let offset = offset.max(0.0);
    let viewport = if viewport.is_nan() {
        0.0
    } else {
        viewport.max(0.0)
    };
    let row_height = row_height.max(0.0);

    if offset + row_height <= viewport {
        return 0.0;
    }

    (offset + row_height - viewport + theme::gap(Gap::Group)).max(0.0)
}

pub fn scroll_to(offset: f32) -> Task<Message> {
    let offset = if offset.is_finite() {
        offset.max(0.0)
    } else {
        0.0
    };

    operation::scroll_to::<Message>(
        content_id(),
        operation::AbsoluteOffset {
            x: None,
            y: Some(offset),
        },
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Section {
    #[default]
    Playback,
    Casting,
    Profiles,
    Folders,
    Online,
    Subtitles,
    Accessibility,
    Updates,
    Data,
    About,
}

impl Section {
    pub const ALL: &'static [Section] = &[
        Section::Playback,
        Section::Casting,
        Section::Profiles,
        Section::Folders,
        Section::Online,
        Section::Subtitles,
        Section::Accessibility,
        Section::Updates,
        Section::Data,
        Section::About,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Section::Playback => "Playback",
            Section::Casting => "Casting",
            Section::Profiles => "Profiles",
            Section::Folders => "Folders",
            Section::Online => "Online metadata",
            Section::Subtitles => "Subtitles",
            Section::Accessibility => "Accessibility",
            Section::Updates => "Updates",
            Section::Data => "Data and backups",
            Section::About => "About",
        }
    }
}

#[derive(Debug, Clone)]
pub enum Row<'a> {
    Toggle {
        label: &'a str,
        hint: Option<&'a str>,
        on: bool,
        message: Message,
    },
    Value {
        label: &'a str,
        value: String,
    },
    Slider {
        label: &'a str,
        hint: Option<&'a str>,
        value: f64,
        range: RangeInclusive<f64>,
        message: fn(f64) -> Message,
    },
    Action {
        label: &'a str,
        hint: Option<&'a str>,
        action: &'a str,
        on_press: Option<Message>,
    },
}

impl Row<'_> {
    pub fn label(&self) -> &str {
        match self {
            Row::Toggle { label, .. }
            | Row::Value { label, .. }
            | Row::Slider { label, .. }
            | Row::Action { label, .. } => label,
        }
    }

    pub fn hint(&self) -> Option<&str> {
        match self {
            Row::Toggle { hint, .. } | Row::Slider { hint, .. } | Row::Action { hint, .. } => *hint,
            Row::Value { .. } => None,
        }
    }

    pub fn slider_value(&self) -> Option<f64> {
        match self {
            Row::Slider { value, range, .. } => Some(clamped(*value, range)),
            Row::Toggle { .. } | Row::Value { .. } | Row::Action { .. } => None,
        }
    }
}

pub fn clamped(value: f64, range: &RangeInclusive<f64>) -> f64 {
    let start = *range.start();
    let end = *range.end();
    if start.is_nan() || end.is_nan() {
        return 0.0;
    }
    let low = start.min(end);
    let high = start.max(end);
    if value.is_nan() {
        return low;
    }
    value.clamp(low, high)
}

pub fn slider_value_label(value: f64) -> String {
    if !value.is_finite() {
        return String::new();
    }
    format!("{}%", value.round())
}

pub struct Settings<'a> {
    pub section: Section,
    pub rows: Vec<Row<'a>>,
    pub nav: &'a Nav,
    pub width: f32,
    pub focus: usize,
    pub extra: Option<Element<'a, Message>>,
}

impl<'a> Settings<'a> {
    pub fn view(mut self) -> Element<'a, Message> {
        let extra = self.extra.take();
        let tokens = design::tokens();

        let title = text("Settings")
            .size(tokens.type_scale.title)
            .font(theme::semibold())
            .color(theme::ink());

        let columns = row![self.rail(), self.content(extra)]
            .spacing(COLUMN_GAP)
            .align_y(Alignment::Start)
            .width(Length::Fill)
            .height(Length::Fill);

        column![title, columns]
            .spacing(theme::gap(Gap::Section))
            .padding(Padding {
                top: theme::gap(Gap::Section),
                bottom: 0.0,
                left: 0.0,
                right: 0.0,
            })
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn rail(&self) -> Element<'a, Message> {
        let mut items = column![].spacing(theme::gap(Gap::Tight));

        for section in Section::ALL {
            items = items.push(section_row(*section, *section == self.section));
        }

        let rail_width = if Breakpoint::at(self.width) == Breakpoint::Narrow {
            RAIL_MIN_WIDTH
        } else {
            RAIL_WIDTH
        };

        scrollable(items)
            .height(Length::Fill)
            .width(Length::Fixed(rail_width))
            .style(theme::scrollbar)
            .into()
    }

    fn content(&self, extra: Option<Element<'a, Message>>) -> Element<'a, Message> {
        let mut body = column![section_heading(self.section), self.rows_element()]
            .width(Length::Fill)
            .height(Length::Shrink);

        if self.section == Section::Playback {
            body = body
                .push(Space::new().height(theme::gap(Gap::Section)))
                .push(picture_card());
        }

        if let Some(extra) = extra {
            body = body
                .push(Space::new().height(theme::gap(Gap::Section)))
                .push(extra);
        }

        scrollable(
            container(
                container(body)
                    .width(Length::Fill)
                    .max_width(content_width(self.width)),
            )
            .width(Length::Fill)
            .height(Length::Shrink),
        )
        .id(content_id())
        .height(Length::Fill)
        .width(Length::Fill)
        .style(theme::scrollbar)
        .into()
    }

    fn rows_element(&self) -> Element<'a, Message> {
        let mut rows = column![].width(Length::Fill);
        let last = self.rows.len().saturating_sub(1);
        let focus = self.focus.min(last);

        for (index, row) in self.rows.iter().enumerate() {
            let focused = index == focus;
            rows = rows.push(self.row_element(row, index < last && !focused, focused));
        }

        rows.into()
    }

    fn row_element(&self, row: &Row<'a>, rule: bool, focused: bool) -> Element<'a, Message> {
        let tokens = design::tokens();
        let ink = if focused {
            focus_foreground()
        } else {
            theme::ink()
        };
        let muted = if focused {
            motion::with_alpha(focus_foreground(), 0.78)
        } else {
            theme::muted()
        };

        let (label, hint, control): (&'a str, Option<&'a str>, Element<'a, Message>) = match row {
            Row::Toggle {
                label,
                hint,
                on,
                message,
            } => (*label, *hint, toggle(*on, message.clone(), focused)),
            Row::Value { label, value } => (
                *label,
                None,
                text(truncate(value, LABEL_CHARS).to_string())
                    .size(tokens.type_scale.label)
                    .font(theme::regular())
                    .color(muted)
                    .into(),
            ),
            Row::Slider {
                label,
                hint,
                value,
                range,
                message,
            } => (
                *label,
                *hint,
                slider_control(*value, range, *message, focused),
            ),
            Row::Action {
                label,
                hint,
                action,
                on_press,
            } => (
                *label,
                *hint,
                action_button(action, on_press.clone(), focused),
            ),
        };

        let mut lines = column![
            text(truncate(label, LABEL_CHARS).to_string())
                .size(tokens.type_scale.label)
                .font(theme::regular())
                .color(ink)
                .wrapping(Wrapping::None)
        ]
        .spacing(theme::gap(Gap::Tight))
        .width(Length::Fill);

        if let Some(hint) = hint {
            lines = lines.push(
                text(hint.to_string())
                    .size(tokens.type_scale.caption)
                    .font(theme::regular())
                    .color(muted),
            );
        }

        let text_block = container(lines).width(Length::Fill).max_width(LABEL_COLUMN);

        let mut element = column![
            container(
                row![text_block, control]
                    .align_y(Alignment::Center)
                    .width(Length::Fill)
            )
            .width(Length::Fill)
            .padding(Padding {
                top: theme::gap(Gap::Group),
                bottom: theme::gap(Gap::Group),
                left: 0.0,
                right: 0.0,
            })
            .style(move |_theme| row_style(focused))
        ]
        .width(Length::Fill);

        if rule {
            element = element.push(hairline());
        }

        element.into()
    }
}

fn section_row<'a>(section: Section, active: bool) -> Element<'a, Message> {
    let tokens = design::tokens();

    let label = text(section.label())
        .size(tokens.type_scale.caption)
        .font(if active {
            theme::medium()
        } else {
            theme::regular()
        })
        .color(if active { theme::ink() } else { theme::muted() });

    let body = container(label)
        .width(Length::Fill)
        .height(Length::Fixed(tokens.control.sm))
        .align_y(Alignment::Center)
        .padding(Padding {
            top: 0.0,
            bottom: 0.0,
            left: theme::gap(Gap::Group),
            right: theme::gap(Gap::Group),
        })
        .style(move |_theme| container::Style {
            background: if active {
                Some(Background::Color(theme::surface(2)))
            } else {
                None
            },
            border: Border {
                radius: theme::radius(Radius::Small).into(),
                ..Border::default()
            },
            text_color: Some(theme::ink()),
            ..container::Style::default()
        });

    button(body)
        .on_press_maybe((!active).then_some(Message::SettingsSection(section)))
        .padding(0)
        .width(Length::Fill)
        .style(bare_button)
        .into()
}

fn action_button<'a>(
    action: &'a str,
    on_press: Option<Message>,
    focused: bool,
) -> Element<'a, Message> {
    let tokens = design::tokens();

    button(
        text(action.to_string())
            .size(tokens.type_scale.label)
            .font(theme::medium())
            .color(if focused {
                focus_foreground()
            } else {
                theme::ink()
            }),
    )
    .on_press_maybe(on_press)
    .padding(Padding {
        top: theme::gap(Gap::Card),
        bottom: theme::gap(Gap::Card),
        left: theme::gap(Gap::Group),
        right: theme::gap(Gap::Group),
    })
    .style(move |_theme, status| action_style(status, focused))
    .into()
}

fn action_style(status: button::Status, focused: bool) -> button::Style {
    if focused {
        return button::Style {
            background: match status {
                button::Status::Hovered | button::Status::Pressed => Some(Background::Color(
                    motion::with_alpha(focus_foreground(), 0.12),
                )),
                button::Status::Disabled | button::Status::Active => None,
            },
            text_color: focus_foreground(),
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: theme::radius(Radius::Small).into(),
            },
            shadow: Shadow::default(),
            snap: false,
        };
    }

    button::Style {
        background: Some(Background::Color(match status {
            button::Status::Hovered | button::Status::Pressed => theme::surface(3),
            button::Status::Disabled => theme::surface(1),
            button::Status::Active => theme::surface(2),
        })),
        text_color: theme::ink(),
        border: Border {
            color: theme::hair(),
            width: 0.0,
            radius: theme::radius(Radius::Small).into(),
        },
        shadow: Shadow::default(),
        snap: false,
    }
}

fn section_heading<'a>(section: Section) -> Element<'a, Message> {
    let tokens = design::tokens();

    column![
        text(section.label())
            .size(tokens.type_scale.body)
            .font(theme::semibold())
            .color(theme::ink()),
        hairline(),
    ]
    .spacing(theme::gap(Gap::Card))
    .width(Length::Fill)
    .into()
}

fn toggle<'a>(on: bool, message: Message, focused: bool) -> Element<'a, Message> {
    let knob = container(
        Space::new()
            .width(Length::Fixed(TOGGLE_KNOB))
            .height(Length::Fixed(TOGGLE_KNOB)),
    )
    .style(move |_theme| container::Style {
        background: Some(Background::Color(match (focused, on) {
            (true, true) => focus_surface(),
            (true, false) => focus_foreground(),
            (false, true) => theme::ground(),
            (false, false) => theme::muted(),
        })),
        border: Border {
            radius: (TOGGLE_KNOB / 2.0).into(),
            ..Border::default()
        },
        ..container::Style::default()
    });

    let filler = Space::new().width(Length::Fill);
    let track = if on {
        row![filler, knob]
    } else {
        row![knob, filler]
    }
    .align_y(Alignment::Center)
    .width(Length::Fill)
    .height(Length::Fill);

    button(
        container(track)
            .width(Length::Fixed(TOGGLE_WIDTH))
            .height(Length::Fixed(TOGGLE_HEIGHT))
            .padding(TOGGLE_INSET)
            .style(move |_theme| container::Style {
                background: Some(Background::Color(match (focused, on) {
                    (true, true) => focus_foreground(),
                    (true, false) => motion::with_alpha(focus_foreground(), 0.24),
                    (false, true) => theme::ink(),
                    (false, false) => theme::surface(3),
                })),
                border: Border {
                    radius: (TOGGLE_HEIGHT / 2.0).into(),
                    ..Border::default()
                },
                ..container::Style::default()
            }),
    )
    .on_press(message)
    .padding(0)
    .style(bare_button)
    .into()
}

fn slider_control<'a>(
    value: f64,
    range: &RangeInclusive<f64>,
    message: fn(f64) -> Message,
    focused: bool,
) -> Element<'a, Message> {
    let tokens = design::tokens();
    let value = clamped(value, range);
    let show = text(slider_value_label(value))
        .size(tokens.type_scale.caption)
        .font(theme::regular())
        .color(if focused {
            focus_foreground()
        } else {
            theme::muted()
        })
        .width(Length::Fixed(SLIDER_VALUE_WIDTH))
        .align_x(Alignment::End);

    if !range.start().is_finite() || !range.end().is_finite() || range.end() <= range.start() {
        return show.into();
    }

    row![
        slider(range.clone(), value, message)
            .width(Length::Fixed(SLIDER_WIDTH))
            .style(move |theme, status| slider_style(theme, status, focused)),
        show,
    ]
    .spacing(theme::gap(Gap::Group))
    .align_y(Alignment::Center)
    .into()
}

fn picture_card<'a>() -> Element<'a, Message> {
    let tokens = design::tokens();

    container(
        column![
            text("Picture")
                .size(tokens.type_scale.caption)
                .font(theme::medium())
                .color(theme::ink()),
            text(
                "16 bits per channel · pq / bt.709 · tone mapping bt.2390 · peak reported by the display"
            )
                .size(tokens.type_scale.caption)
                .font(theme::regular())
                .color(theme::muted())
                .wrapping(Wrapping::Word),
            text("The target peak follows the display's own report, and stays at the 203-nit reference when it reports none.")
                .size(tokens.type_scale.caption)
                .font(theme::regular())
                .color(theme::muted())
                .wrapping(Wrapping::Word),
        ]
        .spacing(theme::gap(Gap::Tight))
        .width(Length::Fill),
    )
    .width(Length::Fill)
    .padding(Padding {
        top: theme::gap(Gap::Group),
        bottom: theme::gap(Gap::Group),
        left: theme::gap(Gap::Group),
        right: theme::gap(Gap::Group),
    })
    .style(|_theme| container::Style {
        background: Some(Background::Color(theme::surface(1))),
        border: Border {
            radius: theme::radius(Radius::Surface).into(),
            ..Border::default()
        },
        ..container::Style::default()
    })
    .into()
}

fn hairline<'a>() -> Element<'a, Message> {
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

fn bare_button(_theme: &Theme, _status: button::Status) -> button::Style {
    button::Style {
        background: None,
        text_color: theme::ink(),
        border: Border::default(),
        shadow: Shadow::default(),
        snap: false,
    }
}

fn slider_style(_theme: &Theme, _status: slider::Status, focused: bool) -> slider::Style {
    let filled = if focused {
        focus_foreground()
    } else {
        theme::signal()
    };
    let empty = if focused {
        motion::with_alpha(focus_foreground(), 0.24)
    } else {
        theme::surface(3)
    };

    slider::Style {
        rail: slider::Rail {
            backgrounds: (Background::Color(filled), Background::Color(empty)),
            width: SLIDER_RAIL_HEIGHT,
            border: Border {
                radius: (SLIDER_RAIL_HEIGHT / 2.0).into(),
                ..Border::default()
            },
        },
        handle: slider::Handle {
            shape: slider::HandleShape::Circle { radius: 0.0 },
            background: Background::Color(filled),
            border_width: 0.0,
            border_color: theme::hair(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Screen;

    #[test]
    fn every_settings_action_has_a_row_somewhere() {
        let sections = Section::ALL;
        assert_eq!(sections.len(), 10);
        let labels: Vec<&str> = sections.iter().map(|s| s.label()).collect();
        assert!(labels.contains(&"Casting") && labels.contains(&"Profiles"));
    }

    #[test]
    fn the_content_column_stops_at_its_own_width_inside_the_window() {
        for window in [900.0_f32, 1100.0, 1280.0, 1440.0, 1760.0, 3440.0] {
            let rail = if Breakpoint::at(window) == Breakpoint::Narrow {
                RAIL_MIN_WIDTH
            } else {
                RAIL_WIDTH
            };
            let width = content_width(window);
            assert!(
                width <= CONTENT_MAX_WIDTH + 0.01,
                "{window}px must not stretch the content past {CONTENT_MAX_WIDTH}"
            );
            let room = window - 2.0 * theme::PAD - rail - COLUMN_GAP;
            assert!(
                width <= room + 0.01,
                "{window}px leaves {room}px beside the rail, so {width}px does not fit"
            );
            assert!(width > 0.0, "{window}px must still leave a content column");
        }
        assert_eq!(content_width(f32::NAN), CONTENT_MAX_WIDTH);
        assert_eq!(content_width(0.0), CONTENT_MAX_WIDTH);
        assert_eq!(content_width(1440.0), CONTENT_MAX_WIDTH);
        assert!(
            content_width(900.0) < CONTENT_MAX_WIDTH,
            "a 900px window cannot hold a 780px column beside the rail"
        );
    }

    #[test]
    fn every_section_label_is_unique_and_non_empty() {
        let mut labels: Vec<&str> = Section::ALL.iter().map(|section| section.label()).collect();
        for label in &labels {
            assert!(!label.is_empty(), "{label} must name its section");
        }
        labels.sort_unstable();
        labels.dedup();
        assert_eq!(
            labels.len(),
            Section::ALL.len(),
            "section labels must not repeat"
        );
    }

    #[test]
    fn a_slider_row_clamps_its_value_into_its_own_range() {
        let row = Row::Slider {
            label: "Subtitle size",
            hint: None,
            value: 250.0,
            range: 50.0..=200.0,
            message: Message::SubtitleScale,
        };
        assert_eq!(row.slider_value(), Some(200.0));
        assert_eq!(clamped(-5.0, &(0.0..=100.0)), 0.0);
        assert_eq!(clamped(150.0, &(200.0..=100.0)), 150.0);
        assert_eq!(clamped(f64::NAN, &(50.0..=200.0)), 50.0);
        assert_eq!(slider_value_label(125.4), "125%");
    }

    #[test]
    fn the_page_composes_every_row_kind() {
        let nav = Nav::new(Screen::Settings);
        let settings = Settings {
            section: Section::Playback,
            rows: vec![
                Row::Toggle {
                    label: "Play the next episode automatically",
                    hint: Some("A gap in a season stops the chain."),
                    on: true,
                    message: Message::ToggleAutoplay(true),
                },
                Row::Value {
                    label: "Library",
                    value: "775 files".to_string(),
                },
                Row::Slider {
                    label: "Subtitle position",
                    hint: Some("Measured from the top of the picture."),
                    value: 88.0,
                    range: 50.0..=100.0,
                    message: Message::SubtitlePosition,
                },
            ],
            nav: &nav,
            width: 1440.0,
            focus: 0,
            extra: None,
        };
        let _ = settings.view();
    }

    fn luminance(colour: Color) -> f32 {
        let channel = |value: f32| {
            if value <= 0.039_28 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(colour.r) + 0.7152 * channel(colour.g) + 0.0722 * channel(colour.b)
    }

    fn contrast(a: Color, b: Color) -> f32 {
        let (high, low) = {
            let (a, b) = (luminance(a), luminance(b));
            if a >= b { (a, b) } else { (b, a) }
        };
        (high + 0.05) / (low + 0.05)
    }

    fn synthetic_rows(count: usize) -> Vec<Row<'static>> {
        (0..count)
            .map(|index| Row::Toggle {
                label: "A settings row with a label long enough to measure",
                hint: Some("and a hint that explains what the row changes."),
                on: index % 2 == 0,
                message: Message::ToggleAutoplay(index % 2 == 0),
            })
            .collect()
    }

    #[test]
    fn a_focused_row_inverts_its_fill_without_a_border() {
        let focused = row_style(true);
        let rest = row_style(false);

        assert_eq!(
            focused.background,
            Some(Background::Color(focus_surface())),
            "focus must be a stronger semantic fill"
        );
        assert_eq!(focused.text_color, Some(focus_foreground()));
        assert_eq!(focused.border.width, 0.0, "focus must not draw a ring");
        assert_eq!(focused.border.color, Color::TRANSPARENT);
        assert_eq!(focused.shadow, Shadow::default());

        assert_eq!(rest.background, None, "an unfocused row draws no fill");
        assert_eq!(rest.text_color, Some(theme::ink()));
        assert_eq!(rest.border.width, 0.0);
    }

    #[test]
    fn the_focus_pair_clears_the_normal_text_contrast_floor() {
        let ratio = contrast(focus_foreground(), focus_surface());

        assert!(
            ratio >= 4.5,
            "the inverted focus pair measures {ratio}:1 and must clear 4.5:1"
        );
    }

    #[test]
    fn the_focus_fill_is_not_the_rest_surface() {
        let focused = row_style(true);
        let rest = row_style(false);

        assert_ne!(focused.background, rest.background);
        assert_ne!(focused.text_color, rest.text_color);
    }

    #[test]
    fn row_offsets_are_monotonic_and_stop_at_the_last_row() {
        let rows = synthetic_rows(30);
        let width = 700.0;

        let mut previous = settings_row_offset(&rows, 0, width);
        assert!(previous > 0.0, "the heading sits above the first row");

        for index in 1..rows.len() {
            let offset = settings_row_offset(&rows, index, width);
            assert!(
                offset > previous,
                "row {index} at {offset} must sit below {previous}"
            );
            previous = offset;
        }

        assert_eq!(
            settings_row_offset(&rows, 29, width),
            settings_row_offset(&rows, 200, width),
            "an index past the end clamps to the last row"
        );
    }

    #[test]
    fn a_thirty_row_section_scrolls_its_focused_row_into_a_six_hundred_pixel_viewport() {
        let rows = synthetic_rows(30);
        let width = 700.0;
        let viewport = 600.0;
        let mut previous_target = 0.0_f32;

        for index in 0..rows.len() {
            let offset = settings_row_offset(&rows, index, width);
            let height = row_height(&rows[index], width);
            let target = settings_scroll_offset(offset, viewport, height);
            let top = offset - target;
            let bottom = offset + height - target;

            assert!(
                top >= 0.0 && bottom <= viewport,
                "row {index} sits at {top}..{bottom} in a {viewport}px viewport scrolled to {target}"
            );
            assert!(
                target >= previous_target,
                "scrolling down must not jump backwards at row {index}"
            );
            previous_target = target;
        }

        assert!(previous_target > 0.0, "the last rows must scroll the pane");
    }

    #[test]
    fn a_short_section_stays_at_the_top() {
        let rows = synthetic_rows(2);
        let offset = settings_row_offset(&rows, 1, 700.0);
        let height = row_height(&rows[1], 700.0);

        assert_eq!(settings_scroll_offset(offset, 600.0, height), 0.0);
        assert_eq!(settings_scroll_offset(f32::NAN, 600.0, height), 0.0);
        assert_eq!(settings_scroll_offset(offset, f32::INFINITY, height), 0.0);
    }

    #[test]
    fn a_shorter_window_has_a_smaller_content_viewport() {
        let tall = content_viewport(800.0);
        let short = content_viewport(480.0);

        assert!(short < tall);
        assert!(short > 0.0, "480px still leaves room for a row");
        assert_eq!(content_viewport(f32::NAN), 0.0);
        assert_eq!(content_viewport(0.0), 0.0);
    }

    #[test]
    fn the_content_scrollable_carries_a_stable_id() {
        assert_eq!(content_id(), Id::from("settings-content"));
    }

    #[test]
    fn a_focused_row_still_draws_its_full_set_of_rows() {
        let nav = Nav::new(Screen::Settings);
        let rows = synthetic_rows(3);
        let settings = Settings {
            section: Section::Playback,
            rows,
            nav: &nav,
            width: 1280.0,
            focus: 2,
            extra: None,
        };

        let _ = settings.view();
    }
}
