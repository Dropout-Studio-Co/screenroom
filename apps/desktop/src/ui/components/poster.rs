use crate::app::Message;
use crate::design;
use crate::ui::motion::{self, lift_scale};
use crate::ui::text_fit;
use crate::ui::theme::{self, Breakpoint, Gap, Radius};
use iced::widget::text::Wrapping;
use iced::widget::{Float, Space, button, column, container, image, row, stack, text};
use iced::{Alignment, Background, Border, ContentFit, Element, Font, Length, Shadow, Theme};

pub const CARD_WIDTH: f32 = 240.0;

pub const CARD_GAP: f32 = 16.0;

const MIN_TEXT_INSET: f32 = 1.0;

const ART_RATIO: f32 = 9.0 / 16.0;

const PROGRESS_HEIGHT: f32 = 2.0;

const WAITING_LABEL: &str = "Loading poster…";

pub const NO_POSTER_LABEL: &str = "No poster";

pub fn bar_width(progress: f32) -> f32 {
    if !progress.is_finite() {
        return 0.0;
    }
    CARD_WIDTH * progress.clamp(0.0, 1.0)
}

pub fn truncate(text: &str, max_chars: usize) -> &str {
    if max_chars == 0 {
        return "";
    }
    match text.char_indices().nth(max_chars) {
        Some((index, _)) => &text[..index],
        None => text,
    }
}

pub fn card_columns(width: f32) -> usize {
    Breakpoint::at(width).columns()
}

pub fn text_inset(width: f32) -> f32 {
    if !width.is_finite() || width <= 0.0 {
        return MIN_TEXT_INSET;
    }

    (width * motion::MOTION.lift / 2.0)
        .ceil()
        .max(MIN_TEXT_INSET)
}

pub fn text_room(width: f32) -> f32 {
    let room = width - 2.0 * text_inset(width);

    if room.is_finite() && room > 1.0 {
        room
    } else {
        1.0
    }
}

pub fn fitted_label(label: &str, room: f32, size: f32, font: Font) -> String {
    text_fit::fit_measured(label, room, size, font)
}

pub fn face_label(waiting: bool, failed: bool) -> Option<&'static str> {
    if failed {
        Some(NO_POSTER_LABEL)
    } else if waiting {
        Some(WAITING_LABEL)
    } else {
        None
    }
}

pub struct Poster<'a> {
    title: &'a str,
    caption: String,
    art: Option<image::Handle>,
    waiting: bool,
    failed: bool,
    progress: Option<f32>,
    focused: bool,
    lift: f32,
    on_press: Option<Message>,
    width: f32,
}

impl<'a> Poster<'a> {
    pub fn new(title: &'a str, caption: impl Into<String>) -> Self {
        let caption: String = caption.into();
        Self {
            title,
            caption,
            art: None,
            waiting: true,
            failed: false,
            progress: None,
            focused: false,
            lift: 0.0,
            on_press: None,
            width: CARD_WIDTH,
        }
    }

    pub fn art(mut self, art: Option<image::Handle>) -> Self {
        self.art = art;
        self
    }

    pub fn waiting(mut self, waiting: bool) -> Self {
        self.waiting = waiting;
        self
    }

    pub fn failed(mut self, failed: bool) -> Self {
        self.failed = failed;
        self
    }

    pub fn progress(mut self, progress: Option<f32>) -> Self {
        self.progress = progress;
        self
    }

    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    pub fn lift(mut self, lift: f32) -> Self {
        if lift.is_finite() {
            self.lift = lift.clamp(0.0, 1.0);
        }
        self
    }

    pub fn on_press(mut self, message: Message) -> Self {
        self.on_press = Some(message);
        self
    }

    pub fn width(mut self, width: f32) -> Self {
        if width.is_finite() && width > 0.0 {
            self.width = width;
        }
        self
    }

    pub fn into_element(self) -> Element<'a, Message> {
        let tokens = design::tokens();
        let art_height = (self.width * ART_RATIO).round();
        let focused = self.focused;
        let lift = self.lift;

        let face: Element<'a, Message> = match self.art {
            Some(handle) => image(handle)
                .width(Length::Fill)
                .height(Length::Fill)
                .content_fit(ContentFit::Cover)
                .into(),
            None => match face_label(self.waiting, self.failed) {
                Some(label) => placeholder(label),
                None => container(Space::new().width(Length::Fill).height(Length::Fill)).into(),
            },
        };

        let framed: Element<'a, Message> = container(face)
            .width(Length::Fixed(self.width))
            .height(Length::Fixed(art_height))
            .clip(true)
            .style(move |_theme| art_frame(focused, lift))
            .into();

        let mut layers = stack![framed];
        if let Some(progress) = self
            .progress
            .filter(|value| value.is_finite() && *value > 0.0)
        {
            layers = stack![layers, progress_bar(self.width, progress)];
        }

        let room = text_room(self.width);
        let label_size = tokens.type_scale.label;
        let caption_size = tokens.type_scale.caption;

        let title = text(fitted_label(self.title, room, label_size, theme::medium()))
            .size(label_size)
            .font(theme::medium())
            .color(if focused {
                theme::ink()
            } else {
                theme::muted()
            })
            .wrapping(Wrapping::None)
            .width(Length::Fill);

        let caption = text(fitted_label(
            &self.caption,
            room,
            caption_size,
            theme::regular(),
        ))
        .size(caption_size)
        .font(theme::regular())
        .color(theme::muted())
        .wrapping(Wrapping::None)
        .width(Length::Fill);

        let lines = container(
            column![title, caption]
                .spacing(theme::gap(Gap::Tight))
                .width(Length::Fill),
        )
        .padding([0.0, text_inset(self.width)])
        .width(Length::Fill);

        let card = column![layers, lines]
            .spacing(theme::gap(Gap::Tight))
            .width(Length::Fixed(self.width));

        let card: Element<'a, Message> = match self.on_press {
            Some(message) => button(card)
                .on_press(message)
                .padding(0)
                .style(card_button)
                .into(),
            None => card.into(),
        };

        if lift <= 0.002 {
            return card;
        }

        Float::new(card)
            .scale(lift_scale(lift))
            .style(|_theme| iced::widget::float::Style::default())
            .into()
    }
}

#[cfg(test)]
impl Poster<'_> {
    pub fn caption_text(&self) -> &str {
        &self.caption
    }
}

fn placeholder(label: &'static str) -> Element<'static, Message> {
    container(
        text(label.to_string())
            .size(design::tokens().type_scale.caption)
            .font(theme::regular())
            .color(theme::muted())
            .align_x(Alignment::Center),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .align_x(Alignment::Center)
    .align_y(Alignment::Center)
    .padding(theme::gap(Gap::Tight))
    .into()
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

fn art_frame(focused: bool, lift: f32) -> container::Style {
    let ring = motion::mix(theme::hair(), theme::ink(), lift.clamp(0.0, 1.0));
    container::Style {
        background: Some(Background::Color(theme::surface(2))),
        border: Border {
            color: if focused { ring } else { theme::hair() },
            width: 1.0 + lift.clamp(0.0, 1.0),
            radius: theme::radius(Radius::Small).into(),
        },
        text_color: Some(theme::ink()),
        shadow: Shadow::default(),
        snap: true,
    }
}

fn card_button(_theme: &Theme, status: button::Status) -> button::Style {
    let pressed = matches!(status, button::Status::Pressed);
    button::Style {
        background: pressed.then(|| Background::Color(motion::with_alpha(theme::ink(), 0.07))),
        text_color: theme::ink(),
        border: Border {
            radius: theme::radius(Radius::Surface).into(),
            ..Border::default()
        },
        shadow: Shadow::default(),
        snap: false,
    }
}
