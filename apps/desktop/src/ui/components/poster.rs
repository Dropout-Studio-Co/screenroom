use crate::app::Message;
use crate::design;
use crate::ui::theme::{self, Breakpoint, Gap, Radius};
use iced::widget::{Space, button, column, container, image, row, stack, text};
use iced::{Alignment, Background, Border, ContentFit, Element, Length, Shadow, Theme};

pub const CARD_WIDTH: f32 = 240.0;

pub const CARD_GAP: f32 = 16.0;

const ART_RATIO: f32 = 9.0 / 16.0;

const TITLE_CHARS: usize = 28;

const CAPTION_CHARS: usize = 34;

const PROGRESS_HEIGHT: f32 = 2.0;

const WAITING_LABEL: &str = "Loading poster…";

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

pub struct Poster<'a> {
    title: &'a str,
    caption: String,
    art: Option<image::Handle>,
    waiting: bool,
    progress: Option<f32>,
    focused: bool,
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
            progress: None,
            focused: false,
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

    pub fn progress(mut self, progress: Option<f32>) -> Self {
        self.progress = progress;
        self
    }

    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
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

        let face: Element<'a, Message> = match self.art {
            Some(handle) => image(handle)
                .width(Length::Fill)
                .height(Length::Fill)
                .content_fit(ContentFit::Cover)
                .into(),
            None if self.waiting => waiting_face(),
            None => container(Space::new().width(Length::Fill).height(Length::Fill)).into(),
        };

        let framed: Element<'a, Message> = container(face)
            .width(Length::Fixed(self.width))
            .height(Length::Fixed(art_height))
            .clip(true)
            .style(move |_theme| art_frame(focused))
            .into();

        let mut layers = stack![framed];
        if let Some(progress) = self
            .progress
            .filter(|value| value.is_finite() && *value > 0.0)
        {
            layers = stack![layers, progress_bar(self.width, progress)];
        }

        let title = text(truncate(self.title, TITLE_CHARS).to_string())
            .size(tokens.type_scale.label)
            .font(theme::medium())
            .color(if focused {
                theme::ink()
            } else {
                theme::muted()
            })
            .width(Length::Fill);

        let caption = text(truncate(&self.caption, CAPTION_CHARS).to_string())
            .size(tokens.type_scale.caption)
            .font(theme::regular())
            .color(theme::muted())
            .width(Length::Fill);

        let card = column![layers, title, caption]
            .spacing(theme::gap(Gap::Tight))
            .width(Length::Fixed(self.width));

        match self.on_press {
            Some(message) => button(card)
                .on_press(message)
                .padding(0)
                .style(card_button)
                .into(),
            None => card.into(),
        }
    }
}

#[cfg(test)]
impl Poster<'_> {
    pub fn caption_text(&self) -> &str {
        &self.caption
    }
}

fn waiting_face() -> Element<'static, Message> {
    container(
        text(WAITING_LABEL.to_string())
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

fn card_button(_theme: &Theme, _status: button::Status) -> button::Style {
    button::Style {
        background: None,
        text_color: theme::ink(),
        border: Border::default(),
        shadow: Shadow::default(),
        snap: false,
    }
}
