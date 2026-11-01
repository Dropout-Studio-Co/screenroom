use crate::app::{Message, Screen};
use crate::design;
use crate::ui::icons::{self, Icon};
use crate::ui::theme::{self, Breakpoint, Gap, Radius};
use iced::widget::canvas::{self, Canvas, Program};
use iced::widget::{Space, button, column, container, row, stack, text};
use iced::{
    Alignment, Background, Border, Color, Element, Length, Padding, Point, Rectangle, Renderer,
    Shadow, Theme, mouse,
};

pub const BAR_HEIGHT: f32 = 76.0;

pub const BAR_FADE_HEIGHT: f32 = 36.0;

const NAV_GLYPH_SIZE: f32 = 16.0;

pub const WORDMARK: &str = "SCREENROOM";

pub const WORDMARK_MARK: &str = "SR";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavLabels {
    Full,

    Compact,
}

pub struct Destination {
    pub screen: Screen,

    pub label: &'static str,

    pub icon: Icon,

    pub message: Message,
}

pub const DESTINATIONS: [Destination; 3] = [
    Destination {
        screen: Screen::Home,
        label: "Home",
        icon: Icon::Home,
        message: Message::GoHome,
    },
    Destination {
        screen: Screen::Browse,
        label: "Library",
        icon: Icon::Library,
        message: Message::GoBrowse,
    },
    Destination {
        screen: Screen::Settings,
        label: "Settings",
        icon: Icon::Settings,
        message: Message::GoSettings,
    },
];

pub fn nav_labels(breakpoint: Breakpoint) -> NavLabels {
    match breakpoint {
        Breakpoint::Narrow | Breakpoint::Compact => NavLabels::Compact,
        Breakpoint::Standard | Breakpoint::Wide | Breakpoint::Ultra => NavLabels::Full,
    }
}

pub fn wordmark(breakpoint: Breakpoint) -> &'static str {
    match breakpoint {
        Breakpoint::Narrow | Breakpoint::Compact => WORDMARK_MARK,
        Breakpoint::Standard | Breakpoint::Wide | Breakpoint::Ultra => WORDMARK,
    }
}

pub fn destination_label(destination: &Destination, _labels: NavLabels) -> &'static str {
    destination.label
}

fn nav_item<'a>(
    destination: &Destination,
    active: Option<Screen>,
    labels: NavLabels,
) -> Element<'a, Message> {
    let tokens = design::tokens();
    let current = active == Some(destination.screen);
    let ink = if current {
        theme::ink()
    } else {
        theme::muted()
    };

    let content: Element<'a, Message> = match labels {
        NavLabels::Full => text(destination_label(destination, labels))
            .size(tokens.type_scale.label)
            .font(if current {
                theme::medium()
            } else {
                theme::regular()
            })
            .color(ink)
            .into(),
        NavLabels::Compact => icons::icon(destination.icon, NAV_GLYPH_SIZE, ink),
    };

    button(content)
        .on_press(destination.message.clone())
        .padding(Padding {
            top: 6.0,
            bottom: 6.0,
            left: 6.0,
            right: 6.0,
        })
        .style(quiet_button)
        .into()
}

pub fn nav_bar<'a>(active: Option<Screen>, width: f32) -> Element<'a, Message> {
    let tokens = design::tokens();
    let breakpoint = Breakpoint::at(width);
    let labels = nav_labels(breakpoint);

    let mut links = row![]
        .spacing(theme::gap(Gap::Section))
        .align_y(Alignment::Center);

    for destination in &DESTINATIONS {
        links = links.push(nav_item(destination, active, labels));
    }

    let search = button(icons::icon(Icon::Search, 16.0, theme::muted()))
        .on_press(Message::GoBrowse)
        .padding(6.0)
        .style(quiet_button);

    let refresh = button(
        text("Refresh")
            .size(tokens.type_scale.caption)
            .font(theme::regular())
            .color(theme::muted()),
    )
    .on_press(Message::Refresh)
    .padding(6.0)
    .style(quiet_button);

    row![
        text(wordmark(breakpoint))
            .size(tokens.type_scale.caption)
            .font(theme::semibold())
            .color(theme::ink()),
        links,
        Space::new().width(Length::Fill),
        refresh,
        search,
        profile(),
    ]
    .spacing(theme::gap(Gap::Rail))
    .align_y(Alignment::Center)
    .width(Length::Fill)
    .into()
}

pub fn status_bar<'a>(
    status: Option<&'a str>,
    error: Option<&'a str>,
) -> Option<Element<'a, Message>> {
    let tokens = design::tokens();
    let (line, color) = match (error, status) {
        (Some(error), _) => (error.to_string(), theme::danger()),
        (None, Some(status)) => (status.to_string(), theme::ink()),
        (None, None) => return None,
    };

    Some(
        container(
            text(line)
                .size(tokens.type_scale.caption)
                .font(theme::regular())
                .color(color),
        )
        .width(Length::Fill)
        .padding(Padding {
            top: theme::gap(Gap::Card),
            bottom: theme::gap(Gap::Card),
            left: theme::PAD,
            right: theme::PAD,
        })
        .style(|_theme| container::Style {
            background: Some(Background::Color(theme::surface(1))),
            ..container::Style::default()
        })
        .into(),
    )
}

pub fn owns_full_bleed_hero(active: Option<Screen>) -> bool {
    matches!(active, Some(Screen::Home) | Some(Screen::Detail))
}

pub fn frame<'a>(
    body: Element<'a, Message>,
    active: Option<Screen>,
    status: Option<&'a str>,
    error: Option<&'a str>,
    width: f32,
) -> Element<'a, Message> {
    if active == Some(Screen::Player) {
        return body;
    }

    let hero = owns_full_bleed_hero(active);
    let body = container(body).width(Length::Fill).height(Length::Fill);
    let body = if hero {
        body
    } else {
        body.padding(Padding {
            top: 0.0,
            bottom: 0.0,
            left: theme::PAD,
            right: theme::PAD,
        })
    };

    let bar = container(nav_bar(active, width))
        .width(Length::Fill)
        .height(Length::Fixed(BAR_HEIGHT))
        .padding(Padding {
            top: 0.0,
            bottom: 0.0,
            left: theme::PAD,
            right: theme::PAD,
        })
        .align_y(Alignment::Center)
        .style(move |_theme| bar_style(hero));

    let mut shell: Element<'a, Message> = if hero {
        let scrim = column![bar, bar_fade()]
            .width(Length::Fill)
            .height(Length::Fixed(BAR_HEIGHT + BAR_FADE_HEIGHT));

        stack![body, scrim].into()
    } else {
        column![bar, body]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    };

    if let Some(status) = status_bar(status, error) {
        shell = column![shell, status]
            .width(Length::Fill)
            .height(Length::Fill)
            .into();
    }

    container(shell)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(|_theme| container::Style {
            background: Some(Background::Color(theme::ground())),
            ..container::Style::default()
        })
        .into()
}

const SCRIM_INK: Color = design::color(0.039, 0.039, 0.039, 1.0);

fn scrim_ink(alpha: f32) -> Color {
    Color {
        a: alpha,
        ..SCRIM_INK
    }
}

fn scrim_gradient() -> iced::Gradient {
    iced::Gradient::Linear(
        iced::gradient::Linear::new(iced::Radians(std::f32::consts::FRAC_PI_2))
            .add_stop(0.0, scrim_ink(0.97))
            .add_stop(0.45, scrim_ink(0.9))
            .add_stop(0.78, scrim_ink(0.62))
            .add_stop(1.0, scrim_ink(0.28)),
    )
}

fn fade_gradient() -> iced::Gradient {
    iced::Gradient::Linear(
        iced::gradient::Linear::new(iced::Radians(std::f32::consts::FRAC_PI_2))
            .add_stop(0.0, scrim_ink(0.28))
            .add_stop(0.32, scrim_ink(0.16))
            .add_stop(0.7, scrim_ink(0.05))
            .add_stop(1.0, scrim_ink(0.0)),
    )
}

fn bar_fade<'a>() -> Element<'a, Message> {
    container(Space::new())
        .width(Length::Fill)
        .height(Length::Fixed(BAR_FADE_HEIGHT))
        .style(|_theme| container::Style {
            background: Some(Background::Gradient(fade_gradient())),
            ..container::Style::default()
        })
        .into()
}

fn bar_style(hero: bool) -> container::Style {
    let background = if hero {
        Some(Background::Gradient(scrim_gradient()))
    } else {
        Some(Background::Color(theme::ground()))
    };
    container::Style {
        background,
        text_color: Some(theme::ink()),
        ..container::Style::default()
    }
}

fn quiet_button(_theme: &Theme, status: button::Status) -> button::Style {
    let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);

    button::Style {
        background: hovered.then(|| Background::Color(theme::surface(1))),
        text_color: theme::ink(),
        border: Border {
            radius: theme::radius(Radius::Small).into(),
            ..Border::default()
        },
        shadow: Shadow::default(),
        snap: false,
    }
}

fn profile<'a>() -> Element<'a, Message> {
    let avatar = Canvas::new(Avatar)
        .width(Length::Fixed(18.0))
        .height(Length::Fixed(18.0));

    container(avatar)
        .width(Length::Fixed(30.0))
        .height(Length::Fixed(30.0))
        .center_x(Length::Fixed(30.0))
        .center_y(Length::Fixed(30.0))
        .style(|_theme| container::Style {
            background: Some(Background::Color(theme::surface(3))),
            border: Border {
                color: theme::hair(),
                width: 1.0,
                radius: 15.0.into(),
            },
            text_color: Some(theme::ink()),
            shadow: Shadow::default(),
            snap: true,
        })
        .into()
}

struct Avatar;

impl Program<Message> for Avatar {
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
        let center = frame.center();
        let ink = theme::ink();
        frame.fill(
            &canvas::Path::circle(Point::new(center.x, center.y - 2.5), 3.0),
            ink,
        );
        frame.fill(
            &canvas::Path::new(|builder| {
                builder.move_to(Point::new(center.x - 5.5, center.y + 7.5));
                builder.line_to(Point::new(center.x - 5.5, center.y + 5.0));
                builder.line_to(Point::new(center.x, center.y + 1.5));
                builder.line_to(Point::new(center.x + 5.5, center.y + 5.0));
                builder.line_to(Point::new(center.x + 5.5, center.y + 7.5));
                builder.close();
            }),
            ink,
        );
        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use crate::app::Screen;
    use crate::ui::icons::Icon;
    use crate::ui::shell;
    use crate::ui::shell::NavLabels;
    use crate::ui::theme::Breakpoint;

    #[test]
    #[allow(clippy::useless_conversion)]
    fn the_status_bar_is_absent_until_there_is_something_to_say() {
        assert!(shell::status_bar(None, None).is_none());
        assert!(shell::status_bar(Some("Backed up 1.5 MB").into(), None).is_some());
        assert!(shell::status_bar(None, Some("backup_failed: disk full").into()).is_some());
    }

    #[test]
    fn the_navigation_drops_its_labels_only_once_the_window_is_narrower_than_standard() {
        for breakpoint in [Breakpoint::Standard, Breakpoint::Wide, Breakpoint::Ultra] {
            assert_eq!(
                shell::nav_labels(breakpoint),
                NavLabels::Full,
                "{breakpoint:?} has room for words"
            );
        }

        for breakpoint in [Breakpoint::Compact, Breakpoint::Narrow] {
            assert_eq!(
                shell::nav_labels(breakpoint),
                NavLabels::Compact,
                "{breakpoint:?} must not keep full labels"
            );
        }

        assert_eq!(shell::nav_labels(Breakpoint::at(1100.0)), NavLabels::Full);
        assert_eq!(
            shell::nav_labels(Breakpoint::at(1099.0)),
            NavLabels::Compact
        );
        assert_eq!(shell::nav_labels(Breakpoint::at(3440.0)), NavLabels::Full);
        assert_eq!(shell::nav_labels(Breakpoint::at(600.0)), NavLabels::Compact);
    }

    #[test]
    fn the_wordmark_shortens_below_the_standard_breakpoint_and_stays_legible() {
        for breakpoint in [Breakpoint::Standard, Breakpoint::Wide, Breakpoint::Ultra] {
            assert_eq!(shell::wordmark(breakpoint), shell::WORDMARK);
        }

        for breakpoint in [Breakpoint::Compact, Breakpoint::Narrow] {
            let mark = shell::wordmark(breakpoint);
            assert_eq!(mark, shell::WORDMARK_MARK);
            assert!(!mark.is_empty(), "{breakpoint:?} must still name the app");
            assert!(
                mark.chars().count() < shell::WORDMARK.chars().count(),
                "{breakpoint:?} must be a shorter form"
            );
        }

        assert_eq!(
            shell::wordmark(Breakpoint::at(1099.0)),
            shell::WORDMARK_MARK
        );
        assert_eq!(shell::wordmark(Breakpoint::at(1100.0)), shell::WORDMARK);
    }

    #[test]
    fn every_destination_keeps_its_name_and_its_own_glyph_when_the_bar_collapses() {
        assert_eq!(
            shell::DESTINATIONS.map(|destination| destination.screen),
            [Screen::Home, Screen::Browse, Screen::Settings]
        );

        for (index, destination) in shell::DESTINATIONS.iter().enumerate() {
            let full = shell::destination_label(destination, NavLabels::Full);
            let compact = shell::destination_label(destination, NavLabels::Compact);

            assert_eq!(full, destination.label);
            assert_eq!(
                compact, full,
                "{full} must keep its name when the bar collapses"
            );
            assert!(
                !shell::DESTINATIONS[..index]
                    .iter()
                    .any(|earlier| earlier.icon == destination.icon),
                "{:?} is drawn for more than one destination",
                destination.icon
            );
        }

        assert_eq!(
            shell::DESTINATIONS.map(|destination| destination.label),
            ["Home", "Library", "Settings"]
        );
        assert_eq!(
            shell::DESTINATIONS.map(|destination| destination.icon),
            [Icon::Home, Icon::Library, Icon::Settings]
        );
    }

    fn alpha_stops(gradient: iced::Gradient) -> Vec<(f32, f32)> {
        match gradient {
            iced::Gradient::Linear(linear) => linear
                .stops
                .iter()
                .flatten()
                .map(|stop| (stop.offset, stop.color.a))
                .collect(),
        }
    }

    #[test]
    fn the_hero_scrim_reaches_transparency_below_the_bar_without_a_visible_edge() {
        const {
            assert!(shell::BAR_FADE_HEIGHT > 0.0);
            assert!(shell::BAR_FADE_HEIGHT < shell::BAR_HEIGHT);
        }

        let bar = alpha_stops(shell::scrim_gradient());
        let fade = alpha_stops(shell::fade_gradient());

        let bar_bottom = bar.last().copied().expect("the bar keeps a scrim");
        let fade_top = fade
            .first()
            .copied()
            .expect("the fade starts under the bar");
        let fade_bottom = fade.last().copied().expect("the fade ends below the bar");

        assert_eq!(bar_bottom.0, 1.0);
        assert!(bar_bottom.1 > 0.0, "the bar must keep its near-solid scrim");
        assert_eq!(fade_top.0, 0.0);
        assert_eq!(
            fade_top.1, bar_bottom.1,
            "the bar and the fade must meet without a step in alpha"
        );
        assert_eq!(fade_bottom.0, 1.0);
        assert_eq!(fade_bottom.1, 0.0, "the fade must reach transparency");
        assert!(
            fade.windows(2).all(|pair| pair[1].1 <= pair[0].1),
            "the fade may only darken the artwork less as it descends"
        );
    }

    #[test]
    fn the_bar_composes_at_every_breakpoint_including_unknown_widths() {
        for width in [0.0, 600.0, 900.0, 1099.0, 1440.0, 3440.0, f32::NAN] {
            let _ = shell::nav_bar(Some(Screen::Home), width);
            let _ = shell::nav_bar(None, width);
        }
    }
}
