use crate::app::Message;
use iced::widget::canvas::{self, Canvas, Program};
use iced::{Color, Element, Length, Point, Rectangle, Renderer, Theme, mouse};
use std::sync::LazyLock;

pub type Segment = (Vec<Point>, bool);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Back,

    Play,

    Pause,

    SkipBack,

    SkipForward,

    Volume,

    Subtitles,

    Fullscreen,

    Search,

    Plus,

    Check,

    Home,

    Library,

    Settings,
}

impl Icon {
    pub const ALL: &'static [Icon] = &[
        Icon::Back,
        Icon::Play,
        Icon::Pause,
        Icon::SkipBack,
        Icon::SkipForward,
        Icon::Volume,
        Icon::Subtitles,
        Icon::Fullscreen,
        Icon::Search,
        Icon::Plus,
        Icon::Check,
        Icon::Home,
        Icon::Library,
        Icon::Settings,
    ];

    fn index(self) -> usize {
        match self {
            Icon::Back => 0,
            Icon::Play => 1,
            Icon::Pause => 2,
            Icon::SkipBack => 3,
            Icon::SkipForward => 4,
            Icon::Volume => 5,
            Icon::Subtitles => 6,
            Icon::Fullscreen => 7,
            Icon::Search => 8,
            Icon::Plus => 9,
            Icon::Check => 10,
            Icon::Home => 11,
            Icon::Library => 12,
            Icon::Settings => 13,
        }
    }
}

fn line(points: &[(f32, f32)]) -> Segment {
    (
        points.iter().map(|(x, y)| Point::new(*x, *y)).collect(),
        false,
    )
}

fn shape(points: &[(f32, f32)]) -> Segment {
    (
        points.iter().map(|(x, y)| Point::new(*x, *y)).collect(),
        true,
    )
}

fn build() -> [Vec<Segment>; 14] {
    let search_ring = [
        (15.5, 10.5),
        (14.83, 13.0),
        (13.0, 14.83),
        (10.5, 15.5),
        (8.0, 14.83),
        (6.17, 13.0),
        (5.5, 10.5),
        (6.17, 8.0),
        (8.0, 6.17),
        (10.5, 5.5),
        (13.0, 6.17),
        (14.83, 8.0),
    ];

    [
        vec![line(&[(14.0, 5.0), (7.0, 12.0), (14.0, 19.0)])],
        vec![shape(&[(8.0, 5.0), (19.0, 12.0), (8.0, 19.0)])],
        vec![
            line(&[(9.0, 5.0), (9.0, 19.0)]),
            line(&[(15.0, 5.0), (15.0, 19.0)]),
        ],
        vec![
            shape(&[(16.0, 6.0), (8.0, 12.0), (16.0, 18.0)]),
            line(&[(7.0, 6.0), (7.0, 18.0)]),
        ],
        vec![
            shape(&[(8.0, 6.0), (16.0, 12.0), (8.0, 18.0)]),
            line(&[(17.0, 6.0), (17.0, 18.0)]),
        ],
        vec![
            shape(&[
                (4.0, 9.0),
                (8.0, 9.0),
                (12.0, 5.0),
                (12.0, 19.0),
                (8.0, 15.0),
                (4.0, 15.0),
            ]),
            line(&[(15.0, 9.0), (17.0, 12.0), (15.0, 15.0)]),
            line(&[(18.0, 7.0), (21.0, 12.0), (18.0, 17.0)]),
        ],
        vec![
            shape(&[(4.0, 6.0), (20.0, 6.0), (20.0, 18.0), (4.0, 18.0)]),
            line(&[(7.0, 13.0), (11.0, 13.0)]),
            line(&[(13.0, 13.0), (17.0, 13.0)]),
        ],
        vec![
            line(&[(4.0, 9.0), (4.0, 4.0), (9.0, 4.0)]),
            line(&[(15.0, 4.0), (20.0, 4.0), (20.0, 9.0)]),
            line(&[(20.0, 15.0), (20.0, 20.0), (15.0, 20.0)]),
            line(&[(9.0, 20.0), (4.0, 20.0), (4.0, 15.0)]),
        ],
        vec![shape(&search_ring), line(&[(14.2, 14.2), (19.0, 19.0)])],
        vec![
            line(&[(12.0, 5.0), (12.0, 19.0)]),
            line(&[(5.0, 12.0), (19.0, 12.0)]),
        ],
        vec![line(&[(5.0, 12.0), (10.0, 17.0), (19.0, 7.0)])],
        vec![
            shape(&[
                (4.0, 11.0),
                (12.0, 4.0),
                (20.0, 11.0),
                (20.0, 20.0),
                (4.0, 20.0),
            ]),
            line(&[(9.0, 20.0), (9.0, 14.5), (15.0, 14.5), (15.0, 20.0)]),
        ],
        vec![
            shape(&[(4.0, 4.0), (10.5, 4.0), (10.5, 10.5), (4.0, 10.5)]),
            shape(&[(13.5, 4.0), (20.0, 4.0), (20.0, 10.5), (13.5, 10.5)]),
            shape(&[(4.0, 13.5), (10.5, 13.5), (10.5, 20.0), (4.0, 20.0)]),
            shape(&[(13.5, 13.5), (20.0, 13.5), (20.0, 20.0), (13.5, 20.0)]),
        ],
        vec![
            line(&[(4.0, 7.0), (20.0, 7.0)]),
            line(&[(9.0, 4.5), (9.0, 9.5)]),
            line(&[(4.0, 12.0), (20.0, 12.0)]),
            line(&[(15.0, 9.5), (15.0, 14.5)]),
            line(&[(4.0, 17.0), (20.0, 17.0)]),
            line(&[(11.0, 14.5), (11.0, 19.5)]),
        ],
    ]
}

static PATHS: LazyLock<[Vec<Segment>; 14]> = LazyLock::new(build);

pub fn path(icon: Icon) -> &'static [Segment] {
    &PATHS[icon.index()]
}

pub fn icon<'a>(icon: Icon, size: f32, color: Color) -> Element<'a, Message> {
    let glyph = Glyph {
        segments: path(icon),
        color,
    };

    iced::widget::container(
        Canvas::new(glyph)
            .width(Length::Fixed(size))
            .height(Length::Fixed(size)),
    )
    .width(Length::Fixed(size))
    .height(Length::Fixed(size))
    .into()
}

struct Glyph {
    segments: &'static [Segment],
    color: Color,
}

impl Glyph {
    fn geometry(&self, renderer: &Renderer, bounds: Rectangle) -> canvas::Geometry {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let scale = bounds.width.min(bounds.height) / 24.0;
        if !scale.is_finite() || scale <= 0.0 {
            return frame.into_geometry();
        }
        let stroke = canvas::Stroke::default()
            .with_color(self.color)
            .with_width((1.75 * scale).max(1.0));

        for (points, closed) in self.segments {
            let Some(first) = points.first() else {
                continue;
            };
            let path = canvas::Path::new(|builder| {
                builder.move_to(Point::new(first.x * scale, first.y * scale));
                for point in &points[1..] {
                    builder.line_to(Point::new(point.x * scale, point.y * scale));
                }
                if *closed {
                    builder.close();
                }
            });
            frame.stroke(&path, stroke);
        }

        frame.into_geometry()
    }
}

impl Program<Message> for Glyph {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        vec![self.geometry(renderer, bounds)]
    }
}

#[cfg(test)]
mod tests {
    use crate::ui::icons;
    use crate::ui::icons::Icon;

    #[test]
    fn every_icon_has_a_drawable_path_inside_its_box() {
        for icon in Icon::ALL {
            let segments = icons::path(*icon);
            assert!(!segments.is_empty(), "{icon:?} has no path");
            for (points, _) in segments {
                assert!(points.len() >= 2, "{icon:?} has a segment with one point");
                for p in points {
                    assert!(
                        (0.0..=24.0).contains(&p.x) && (0.0..=24.0).contains(&p.y),
                        "{icon:?} escapes its 24x24 box at {p:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn the_icon_set_covers_every_glyph_the_ui_draws() {
        assert_eq!(Icon::ALL.len(), 14);
    }

    #[test]
    fn the_navigation_glyphs_are_distinct_and_fit_the_set_weight() {
        let navigation = [Icon::Home, Icon::Library, Icon::Settings];

        for (index, icon) in navigation.iter().enumerate() {
            assert_ne!(
                *icon,
                Icon::Search,
                "{icon:?} must not reuse the search glyph"
            );
            assert!(
                !navigation[..index].contains(icon),
                "{icon:?} is drawn for two destinations"
            );
        }
    }
}
