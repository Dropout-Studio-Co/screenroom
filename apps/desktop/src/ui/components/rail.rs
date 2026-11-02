use crate::app::Message;
use crate::design;
use crate::ui::components::poster::CARD_GAP;
use crate::ui::motion::{self, MOTION};
use crate::ui::theme::{self, Gap};
use iced::widget::{Id, Space, column, operation, row, scrollable, text};
use iced::{Alignment, Element, Length, Task};

pub const LIFT_ROOM: f32 = 8.0;

pub fn rail_id(label: &str) -> Id {
    Id::from(format!("rail:{label}"))
}

pub fn focus_offset(focus: usize, card_width: f32) -> f32 {
    (card_width + CARD_GAP) * focus as f32
}

pub fn cards_per_view(viewport: f32, card_width: f32) -> usize {
    if !viewport.is_finite() || viewport <= 0.0 || !card_width.is_finite() || card_width <= 0.0 {
        return 1;
    }
    let stride = card_width + CARD_GAP;
    ((viewport + CARD_GAP) / stride).floor().max(1.0) as usize
}

pub fn window_start(column: usize, count: usize, per_view: usize) -> usize {
    let per_view = per_view.max(1);
    if count <= per_view {
        return 0;
    }
    column
        .saturating_sub(per_view / 3)
        .min(count.saturating_sub(per_view))
}

pub fn window(column: usize, focused: bool, count: usize, per_view: usize) -> (usize, usize) {
    let per_view = per_view.max(1);
    if !focused {
        return (0, count.min(per_view));
    }
    let start = window_start(column, count, per_view);
    (start, count.saturating_sub(start).min(per_view))
}

pub fn limit(column: usize, focused: bool, count: usize, card_width: f32, viewport: f32) -> f32 {
    if !viewport.is_finite() || viewport <= 0.0 {
        return 0.0;
    }
    let (start, shown) = window(column, focused, count, cards_per_view(viewport, card_width));
    if shown == 0 {
        return 0.0;
    }
    let stride = card_width + CARD_GAP;
    let lead = if start > 0 {
        stride * start as f32
    } else {
        0.0
    };
    let content = lead + 2.0 * LIFT_ROOM + stride * shown as f32 - CARD_GAP;
    (content - viewport).max(0.0)
}

pub fn extent(
    column: usize,
    focused: bool,
    count: usize,
    card_width: f32,
    viewport: f32,
) -> motion::RailExtent {
    motion::RailExtent::new(
        limit(column, focused, count, card_width, viewport),
        viewport,
        MOTION.rubber_room,
    )
}

pub fn scroll_task(label: &str, offset: f32) -> Task<Message> {
    operation::scroll_to::<Message>(
        rail_id(label),
        operation::AbsoluteOffset {
            x: Some(offset),
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

    let mut strip = row![Space::new().width(Length::Fixed(LIFT_ROOM))]
        .spacing(CARD_GAP)
        .align_y(Alignment::Start);
    for (card, _position) in cards {
        strip = strip.push(card);
    }
    strip = strip.push(Space::new().width(Length::Fixed(LIFT_ROOM + MOTION.rubber_room)));

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
        .on_scroll({
            let label = label.to_string();
            move |viewport| Message::RailScrolled {
                label: label.clone(),
                offset: viewport.absolute_offset().x,
            }
        })
        .width(Length::Fill)
        .height(Length::Fixed(
            card_width * 9.0 / 16.0 + 52.0 + 2.0 * LIFT_ROOM,
        ));

    column![header, strip]
        .spacing(theme::gap(Gap::Group))
        .width(Length::Fill)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_follows_the_focused_column_without_running_off_the_end() {
        assert_eq!(window(0, true, 60, 5), (0, 5));
        assert_eq!(window(3, false, 60, 5), (0, 5));
        assert_eq!(window(12, true, 60, 5).0, 11);
        assert_eq!(window(59, true, 60, 5).0, 55);
        assert_eq!(window(5, true, 3, 5), (0, 3));
        assert_eq!(window(0, true, 0, 5), (0, 0));
    }

    #[test]
    fn a_rail_that_fits_the_window_has_no_scroll_and_one_that_does_not_has_room() {
        let wide = limit(0, true, 2, 240.0, 1200.0);
        assert_eq!(wide, 0.0, "two cards cannot scroll a wide rail");

        let long = limit(20, true, 60, 240.0, 1200.0);
        assert!(long > 0.0);
        assert!(long > limit(0, true, 60, 240.0, 1200.0));
    }

    #[test]
    fn the_extent_reserves_the_room_the_lift_needs() {
        let extent = extent(0, true, 2, 240.0, 1200.0);
        assert!(extent.reach() > 0.0, "the lift room is always available");
        assert_eq!(extent.room, MOTION.rubber_room);
    }
}
