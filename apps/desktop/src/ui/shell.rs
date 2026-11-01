use crate::app::{Message, Screen};
use crate::design;
use crate::ui::icons::{self, Icon};
use crate::ui::theme::{self, Gap};
use iced::widget::canvas::{self, Canvas, Program};
use iced::widget::{Space, button, column, container, row, stack, text};
use iced::{
    Alignment, Background, Border, Element, Length, Padding, Point, Rectangle, Renderer, Shadow,
    Theme, mouse,
};

pub const BAR_HEIGHT: f32 = 76.0;

pub fn nav_bar<'a>(active: Option<Screen>) -> Element<'a, Message> {
    let tokens = design::tokens();

    let item = |label: &'static str, screen: Screen, message: Message| -> Element<'a, Message> {
        let current = active == Some(screen);
        button(
            text(label)
                .size(tokens.type_scale.label)
                .font(if current {
                    theme::medium()
                } else {
                    theme::regular()
                })
                .color(if current {
                    theme::ink()
                } else {
                    theme::muted()
                }),
        )
        .on_press(message)
        .padding(Padding {
            top: 6.0,
            bottom: 6.0,
            left: 0.0,
            right: 0.0,
        })
        .style(quiet_button)
        .into()
    };

    let links = row![
        item("Home", Screen::Home, Message::GoHome),
        item("Library", Screen::Browse, Message::GoBrowse),
        item("Settings", Screen::Settings, Message::GoSettings),
    ]
    .spacing(theme::gap(Gap::Section))
    .align_y(Alignment::Center);

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
        text("SCREENROOM")
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

pub fn frame<'a>(
    body: Element<'a, Message>,
    active: Option<Screen>,
    status: Option<&'a str>,
    error: Option<&'a str>,
) -> Element<'a, Message> {
    if active == Some(Screen::Player) {
        return body;
    }

    let hero = matches!(active, Some(Screen::Home) | Some(Screen::Detail));
    let body = container(body)
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(Padding {
            top: 0.0,
            bottom: 0.0,
            left: theme::PAD,
            right: theme::PAD,
        });

    let bar = container(nav_bar(active))
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
        stack![body, bar].into()
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

fn bar_style(hero: bool) -> container::Style {
    container::Style {
        background: if hero {
            None
        } else {
            Some(Background::Color(theme::ground()))
        },
        text_color: Some(theme::ink()),
        ..container::Style::default()
    }
}

fn quiet_button(_theme: &Theme, _status: button::Status) -> button::Style {
    button::Style {
        background: None,
        text_color: theme::ink(),
        border: Border::default(),
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
    use crate::ui::shell;

    #[test]
    #[allow(clippy::useless_conversion)]
    fn the_status_bar_is_absent_until_there_is_something_to_say() {
        assert!(shell::status_bar(None, None).is_none());
        assert!(shell::status_bar(Some("Backed up 1.5 MB").into(), None).is_some());
        assert!(shell::status_bar(None, Some("backup_failed: disk full").into()).is_some());
    }
}
