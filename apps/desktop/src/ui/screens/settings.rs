use crate::app::Message;
use crate::design;
use crate::ui::components::poster::truncate;
use crate::ui::route::Nav;
use crate::ui::theme::{self, Breakpoint, Gap, Radius};
use iced::widget::text::Wrapping;
use iced::widget::{Space, button, column, container, row, scrollable, slider, text};
use iced::{Alignment, Background, Border, Element, Length, Padding, Shadow, Theme};
use std::ops::RangeInclusive;

pub const RAIL_WIDTH: f32 = 200.0;

pub const RAIL_MIN_WIDTH: f32 = 160.0;

pub const CONTENT_MAX_WIDTH: f32 = 780.0;

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
            container(body)
                .width(Length::Fill)
                .max_width(CONTENT_MAX_WIDTH),
        )
        .height(Length::Fill)
        .width(Length::Fill)
        .style(theme::scrollbar)
        .into()
    }

    fn rows_element(&self) -> Element<'a, Message> {
        let mut rows = column![].width(Length::Fill);
        let last = self.rows.len().saturating_sub(1);

        for (index, row) in self.rows.iter().enumerate() {
            rows = rows.push(self.row_element(row, index < last));
        }

        rows.into()
    }

    fn row_element(&self, row: &Row<'a>, rule: bool) -> Element<'a, Message> {
        let tokens = design::tokens();

        let (label, hint, control): (&'a str, Option<&'a str>, Element<'a, Message>) = match row {
            Row::Toggle {
                label,
                hint,
                on,
                message,
            } => (*label, *hint, toggle(*on, message.clone())),
            Row::Value { label, value } => (
                *label,
                None,
                text(truncate(value, LABEL_CHARS).to_string())
                    .size(tokens.type_scale.label)
                    .font(theme::regular())
                    .color(theme::muted())
                    .into(),
            ),
            Row::Slider {
                label,
                hint,
                value,
                range,
                message,
            } => (*label, *hint, slider_control(*value, range, *message)),
            Row::Action {
                label,
                hint,
                action,
                on_press,
            } => (*label, *hint, action_button(action, on_press.clone())),
        };

        let mut lines = column![
            text(truncate(label, LABEL_CHARS).to_string())
                .size(tokens.type_scale.label)
                .font(theme::regular())
                .color(theme::ink())
                .wrapping(Wrapping::None)
        ]
        .spacing(theme::gap(Gap::Tight))
        .width(Length::Fill);

        if let Some(hint) = hint {
            lines = lines.push(
                text(hint.to_string())
                    .size(tokens.type_scale.caption)
                    .font(theme::regular())
                    .color(theme::muted()),
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

fn action_button<'a>(action: &'a str, on_press: Option<Message>) -> Element<'a, Message> {
    let tokens = design::tokens();

    button(
        text(action.to_string())
            .size(tokens.type_scale.label)
            .font(theme::medium())
            .color(theme::ink()),
    )
    .on_press_maybe(on_press)
    .padding(Padding {
        top: theme::gap(Gap::Card),
        bottom: theme::gap(Gap::Card),
        left: theme::gap(Gap::Group),
        right: theme::gap(Gap::Group),
    })
    .style(|_theme, status| action_style(status))
    .into()
}

fn action_style(status: button::Status) -> button::Style {
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

fn toggle<'a>(on: bool, message: Message) -> Element<'a, Message> {
    let knob = container(
        Space::new()
            .width(Length::Fixed(TOGGLE_KNOB))
            .height(Length::Fixed(TOGGLE_KNOB)),
    )
    .style(move |_theme| container::Style {
        background: Some(Background::Color(if on {
            theme::ground()
        } else {
            theme::muted()
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
                background: Some(Background::Color(if on {
                    theme::ink()
                } else {
                    theme::surface(3)
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
) -> Element<'a, Message> {
    let tokens = design::tokens();
    let value = clamped(value, range);
    let show = text(slider_value_label(value))
        .size(tokens.type_scale.caption)
        .font(theme::regular())
        .color(theme::muted())
        .width(Length::Fixed(SLIDER_VALUE_WIDTH))
        .align_x(Alignment::End);

    if !range.start().is_finite() || !range.end().is_finite() || range.end() <= range.start() {
        return show.into();
    }

    row![
        slider(range.clone(), value, message)
            .width(Length::Fixed(SLIDER_WIDTH))
            .style(slider_style),
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

fn slider_style(_theme: &Theme, _status: slider::Status) -> slider::Style {
    slider::Style {
        rail: slider::Rail {
            backgrounds: (
                Background::Color(theme::signal()),
                Background::Color(theme::surface(3)),
            ),
            width: SLIDER_RAIL_HEIGHT,
            border: Border {
                radius: (SLIDER_RAIL_HEIGHT / 2.0).into(),
                ..Border::default()
            },
        },
        handle: slider::Handle {
            shape: slider::HandleShape::Circle { radius: 0.0 },
            background: Background::Color(theme::signal()),
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
        assert_eq!(sections.len(), 9);
        let labels: Vec<&str> = sections.iter().map(|s| s.label()).collect();
        assert!(labels.contains(&"Casting") && labels.contains(&"Profiles"));
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
            extra: None,
        };
        let _ = settings.view();
    }
}
