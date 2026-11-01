use crate::design;
use iced::font::{Family, Weight};
use iced::widget::{container, scrollable};
use iced::{Background, Border, Color, Font, Shadow, Theme};
use std::borrow::Cow;

pub fn content_inset(width: f32) -> f32 {
    if !width.is_finite() || width <= 0.0 {
        return 0.0;
    }
    match Breakpoint::at(width).content_cap() {
        Some(cap) => ((width - cap) / 2.0).max(0.0),
        None => 0.0,
    }
}

pub fn scrollbar(_theme: &Theme, _status: scrollable::Status) -> scrollable::Style {
    let scroller = scrollable::Scroller {
        background: Background::Color(Color::from_rgba(1.0, 1.0, 1.0, 0.07)),
        border: Border::default(),
    };
    scrollable::Style {
        container: container::Style::default(),
        vertical_rail: scrollable::Rail {
            background: None,
            border: Border::default(),
            scroller,
        },
        horizontal_rail: scrollable::Rail {
            background: None,
            border: Border::default(),
            scroller,
        },
        gap: None,
        auto_scroll: scrollable::AutoScroll {
            background: Background::Color(Color::from_rgba(0.08, 0.08, 0.09, 0.96)),
            border: Border::default(),
            shadow: Shadow::default(),
            icon: Color::from_rgba(0.96, 0.96, 0.97, 1.0),
        },
    }
}

pub const PAD: f32 = 40.0;

pub const FONT: &str = "Inter";

const FACES: [&str; 3] = ["Inter Regular", "Inter Medium", "Inter SemiBold"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Radius {
    Small,

    Control,

    Surface,

    Overlay,

    Full,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Breakpoint {
    Narrow,

    Compact,

    Standard,

    Wide,

    Ultra,
}

impl Breakpoint {
    pub const ALL: &'static [Breakpoint] = &[
        Breakpoint::Narrow,
        Breakpoint::Compact,
        Breakpoint::Standard,
        Breakpoint::Wide,
        Breakpoint::Ultra,
    ];

    pub fn at(width: f32) -> Breakpoint {
        if !width.is_finite() {
            return Breakpoint::Standard;
        }
        if width >= 1760.0 {
            Breakpoint::Ultra
        } else if width >= 1440.0 {
            Breakpoint::Wide
        } else if width >= 1100.0 {
            Breakpoint::Standard
        } else if width >= 900.0 {
            Breakpoint::Compact
        } else {
            Breakpoint::Narrow
        }
    }

    pub fn columns(self) -> usize {
        match self {
            Breakpoint::Narrow => 2,
            Breakpoint::Compact => 4,
            Breakpoint::Standard => 5,
            Breakpoint::Wide => 6,
            Breakpoint::Ultra => 7,
        }
    }

    pub fn hero(self) -> f32 {
        match self {
            Breakpoint::Narrow => 340.0,
            Breakpoint::Compact => 430.0,
            Breakpoint::Standard => 480.0,
            Breakpoint::Wide => 530.0,
            Breakpoint::Ultra => 600.0,
        }
    }

    pub fn card(self) -> f32 {
        match self {
            Breakpoint::Narrow => 176.0,
            Breakpoint::Compact => 208.0,
            Breakpoint::Standard => 224.0,
            Breakpoint::Wide => 240.0,
            Breakpoint::Ultra => 268.0,
        }
    }

    pub fn content_cap(self) -> Option<f32> {
        match self {
            Breakpoint::Ultra => Some(1760.0),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gap {
    Tight,

    Card,

    Group,

    Rail,

    Section,
}

pub fn ground() -> Color {
    design::tokens().palette.background
}

pub fn ink() -> Color {
    design::tokens().palette.foreground
}

pub fn muted() -> Color {
    design::tokens().palette.ink_muted
}

pub fn surface(level: u8) -> Color {
    let palette = &design::tokens().palette;
    match level {
        0 => palette.background,
        1 => palette.surface1,
        2 => palette.surface2,
        _ => palette.surface3,
    }
}

pub fn hair() -> Color {
    design::tokens().palette.hair
}

pub fn signal() -> Color {
    design::tokens().palette.signal
}

pub fn signal_foreground() -> Color {
    design::tokens().palette.signal_foreground
}

pub fn danger() -> Color {
    design::tokens().palette.destructive
}

pub fn radius(step: Radius) -> f32 {
    let radius = &design::tokens().radius;
    match step {
        Radius::Small => radius.small,
        Radius::Control => radius.control,
        Radius::Surface => radius.surface,
        Radius::Overlay => radius.overlay,
        Radius::Full => radius.full,
    }
}

pub fn gap(step: Gap) -> f32 {
    match step {
        Gap::Tight => 4.0,
        Gap::Card => 9.0,
        Gap::Group => 13.0,
        Gap::Rail => 26.0,
        Gap::Section => 34.0,
    }
}

pub fn fonts() -> Vec<Cow<'static, str>> {
    FACES.iter().map(|face| Cow::Borrowed(*face)).collect()
}

pub fn font_data() -> [&'static [u8]; 3] {
    [
        include_bytes!("../../assets/fonts/Inter-Regular.ttf"),
        include_bytes!("../../assets/fonts/Inter-Medium.ttf"),
        include_bytes!("../../assets/fonts/Inter-SemiBold.ttf"),
    ]
}

pub fn font(weight: Weight) -> Font {
    Font {
        family: Family::Name(FONT),
        weight,
        ..Font::DEFAULT
    }
}

pub fn regular() -> Font {
    font(Weight::Normal)
}

pub fn medium() -> Font {
    font(Weight::Medium)
}

pub fn semibold() -> Font {
    font(Weight::Semibold)
}

#[cfg(test)]
mod tests {
    use crate::ui::theme;
    use crate::ui::theme::{Breakpoint, Gap, Radius};

    fn luminance(colour: iced::Color) -> f32 {
        let channel = |value: f32| {
            if value <= 0.039_28 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(colour.r) + 0.7152 * channel(colour.g) + 0.0722 * channel(colour.b)
    }

    fn contrast(a: iced::Color, b: iced::Color) -> f32 {
        let (high, low) = {
            let (a, b) = (luminance(a), luminance(b));
            if a >= b { (a, b) } else { (b, a) }
        };
        (high + 0.05) / (low + 0.05)
    }

    #[test]
    fn the_text_colours_are_readable_on_the_ground_they_sit_on() {
        assert!(
            contrast(theme::ink(), theme::ground()) >= 7.0,
            "body text on the ground must clear the enhanced contrast floor"
        );
        assert!(
            contrast(theme::muted(), theme::ground()) >= 4.5,
            "muted captions must still clear the normal-text floor"
        );
        assert!(
            contrast(theme::ground(), theme::signal()) >= 7.0,
            "the primary button's label must clear the enhanced floor"
        );
        assert!(
            contrast(theme::ink(), theme::surface(2)) >= 4.5,
            "text on a raised surface must clear the normal-text floor"
        );
    }

    #[test]
    fn the_hero_starts_where_the_capped_content_starts() {
        for width in [900.0_f32, 1100.0, 1440.0, 1760.0, 2560.0, 3440.0] {
            let inset = theme::content_inset(width);
            let cap = Breakpoint::at(width).content_cap();
            match cap {
                None => assert_eq!(inset, 0.0, "{width}px is not capped"),
                Some(cap) => assert!(
                    (inset - (width - cap) / 2.0).abs() < 0.01,
                    "at {width}px the hero must start at the capped column's edge"
                ),
            }
        }
        assert_eq!(theme::content_inset(f32::NAN), 0.0);
        assert_eq!(theme::content_inset(-10.0), 0.0);
    }

    #[test]
    fn the_surfaces_step_up_from_the_ground() {
        assert!(luminance(theme::surface(1)) > luminance(theme::ground()));
        assert!(luminance(theme::surface(2)) > luminance(theme::surface(1)));
        assert!(luminance(theme::surface(3)) > luminance(theme::surface(2)));
    }

    #[test]
    fn the_gap_and_radius_scales_are_the_spec_values() {
        assert_eq!(theme::PAD, 40.0);
        assert_eq!(
            [
                theme::gap(Gap::Tight),
                theme::gap(Gap::Card),
                theme::gap(Gap::Group),
                theme::gap(Gap::Rail),
                theme::gap(Gap::Section)
            ],
            [4.0, 9.0, 13.0, 26.0, 34.0]
        );
        assert_eq!(
            [
                theme::radius(Radius::Small),
                theme::radius(Radius::Control),
                theme::radius(Radius::Surface),
                theme::radius(Radius::Overlay),
                theme::radius(Radius::Full)
            ],
            [8.0, 12.0, 14.0, 16.0, 9999.0]
        );
        assert_eq!(
            theme::signal_foreground(),
            crate::design::tokens().palette.signal_foreground
        );
    }

    #[test]
    fn every_width_maps_to_a_breakpoint_with_sane_geometry() {
        assert_eq!(Breakpoint::at(800.0), Breakpoint::Narrow);
        assert_eq!(Breakpoint::at(960.0), Breakpoint::Compact);
        assert_eq!(Breakpoint::at(1100.0), Breakpoint::Standard);
        assert_eq!(Breakpoint::at(1500.0), Breakpoint::Wide);
        assert_eq!(Breakpoint::at(3440.0), Breakpoint::Ultra);
        for breakpoint in Breakpoint::ALL {
            assert!(
                breakpoint.columns() >= 2 && breakpoint.columns() <= 7,
                "{breakpoint:?} must place between two and seven columns"
            );
            assert!(
                breakpoint.hero() > 0.0 && breakpoint.card() > 0.0,
                "{breakpoint:?} must carry a hero and a card size"
            );
        }
        assert_eq!(Breakpoint::at(f32::NAN), Breakpoint::Standard);
        assert_eq!(Breakpoint::at(1440.0), Breakpoint::Wide);
        assert_eq!(Breakpoint::at(1200.0), Breakpoint::Standard);
        assert_eq!(Breakpoint::at(900.0), Breakpoint::Compact);
    }

    #[test]
    fn an_ultrawide_caps_its_content_instead_of_stretching_cards() {
        assert_eq!(Breakpoint::Ultra.content_cap(), Some(1760.0));
        assert_eq!(Breakpoint::Wide.content_cap(), None);
    }

    #[test]
    fn the_bundled_font_is_inter_and_the_licence_ships_with_it() {
        assert_eq!(theme::FONT, "Inter");
        assert!(theme::fonts().iter().any(|f| f.contains("Inter")));
        let ofl = include_str!("../../assets/fonts/OFL.txt");
        assert!(ofl.contains("SIL Open Font License"));
    }
}
