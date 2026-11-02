use crate::app::Message;
use crate::design;
use crate::ui::components::poster::CARD_GAP;
use crate::ui::theme::{self, Gap};
use iced::widget::{Id, Space, column, operation, row, scrollable, text};
use iced::{Alignment, Element, Length, Task};

pub fn rail_id(label: &str) -> Id {
    Id::from(format!("rail:{label}"))
}

pub fn focus_offset(focus: usize, card_width: f32) -> f32 {
    (card_width + CARD_GAP) * focus as f32
}

pub fn cards_per_view(available: f32, card_width: f32) -> usize {
    if !available.is_finite() || !card_width.is_finite() || card_width <= 0.0 {
        return 1;
    }

    let pitch = card_width + CARD_GAP;
    let count = ((available + CARD_GAP) / pitch).floor();

    if count.is_finite() && count >= 1.0 {
        count as usize
    } else {
        1
    }
}

pub fn window_start(focused: usize, len: usize, per_view: usize) -> usize {
    if per_view == 0 || len <= per_view {
        return 0;
    }

    let lead = per_view / 3;
    focused.saturating_sub(lead).min(len - per_view)
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
    card_width: f32,
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
        .height(Length::Fixed(card_width * 9.0 / 16.0 + 52.0));

    column![header, strip]
        .spacing(theme::gap(Gap::Group))
        .width(Length::Fill)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rail_only_shows_cards_that_fit_whole() {
        for (available, card) in [
            (1680.0_f32, 268.0_f32),
            (1360.0, 240.0),
            (1200.0, 224.0),
            (820.0, 208.0),
            (420.0, 176.0),
        ] {
            let per_view = cards_per_view(available, card);
            let span = per_view as f32 * card + (per_view.saturating_sub(1)) as f32 * CARD_GAP;

            assert!(per_view >= 1, "{available}px must show at least one card");
            assert!(
                span <= available + 0.01,
                "{per_view} cards of {card} need {span}px, past the {available}px rail"
            );
            let next = (per_view + 1) as f32 * card + per_view as f32 * CARD_GAP;
            assert!(
                next > available + 0.01,
                "the rail at {available}px could have fitted {per_view} cards, not fewer"
            );
        }
    }

    #[test]
    fn the_window_keeps_the_focused_card_on_screen() {
        let len = 24;
        for per_view in [1_usize, 3, 5, 7] {
            for focused in 0..len {
                let start = window_start(focused, len, per_view);
                assert!(
                    start + per_view <= len,
                    "the window must not run past the end of the rail"
                );
                assert!(
                    focused >= start && focused < start + per_view,
                    "{focused} is outside the {start}..{} window",
                    start + per_view
                );
            }
        }
        assert_eq!(
            window_start(5, 3, 7),
            0,
            "a short rail starts at its first card"
        );
        assert_eq!(window_start(0, 24, 5), 0);
        assert_eq!(window_start(23, 24, 5), 19);
        assert_eq!(cards_per_view(f32::NAN, 240.0), 1);
        assert_eq!(cards_per_view(1000.0, 0.0), 1);
    }
}
