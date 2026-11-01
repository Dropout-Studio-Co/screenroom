use crate::app::Message;
use crate::design;
use crate::ui::components::poster::{CARD_GAP, CARD_WIDTH};
use crate::ui::theme::{self, Gap};
use iced::widget::{Id, Space, column, operation, row, scrollable, text};
use iced::{Alignment, Element, Length, Task};

pub fn rail_id(label: &str) -> Id {
    Id::from(format!("rail:{label}"))
}

pub fn focus_offset(focus: usize, card_width: f32) -> f32 {
    (card_width + CARD_GAP) * focus as f32
}

pub fn scroll_task(label: &str, focus: usize, card_width: f32) -> Task<Message> {
    operation::scroll_to::<Message>(
        rail_id(label),
        operation::AbsoluteOffset {
            x: Some(focus_offset(focus, card_width)),
            y: None,
        },
    )
}

pub fn rail<'a>(
    label: &'a str,
    count: usize,
    cards: Vec<(Element<'a, Message>, usize)>,
    focus: usize,
) -> Element<'a, Message> {
    let tokens = design::tokens();
    let at = focus.min(count.saturating_sub(1));

    let header = row![
        text(label.to_string())
            .size(tokens.type_scale.label)
            .font(theme::semibold())
            .color(theme::ink()),
        text(count.to_string())
            .size(tokens.type_scale.micro)
            .color(theme::muted()),
        Space::new().width(Length::Fill),
    ]
    .spacing(theme::gap(Gap::Tight))
    .align_y(Alignment::Center)
    .width(Length::Fill);

    let mut strip = row![].spacing(CARD_GAP).align_y(Alignment::Start);
    for (card, _position) in cards {
        strip = strip.push(card);
    }

    let strip = scrollable(strip)
        .id(rail_id(label))
        .direction(scrollable::Direction::Horizontal(
            scrollable::Scrollbar::hidden(),
        ))
        .anchor_x(if at == 0 {
            scrollable::Anchor::Start
        } else {
            scrollable::Anchor::End
        })
        .width(Length::Fill)
        .height(Length::Fixed(CARD_WIDTH * 9.0 / 16.0 + 52.0));

    column![header, strip]
        .spacing(theme::gap(Gap::Group))
        .width(Length::Fill)
        .into()
}
