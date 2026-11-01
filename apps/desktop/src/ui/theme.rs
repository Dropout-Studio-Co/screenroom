use crate::design;
use iced::Color;
use iced::Font;
use iced::font::{Family, Weight};
use std::borrow::Cow;

pub const PAD: f32 = 40.0;

pub const FONT: &str = "Inter";

const FACES: [&str; 3] = ["Inter Regular", "Inter Medium", "Inter SemiBold"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Radius {
    Small,

    Control,

    Surface,

    Overlay,
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
    use crate::ui::theme::{Gap, Radius};

    #[test]
    fn theme_values_come_from_the_pinned_tokens() {
        let tokens = crate::design::tokens();
        assert_eq!(theme::ground(), tokens.palette.background);
        assert_eq!(theme::ink(), tokens.palette.foreground);
        assert_eq!(theme::surface(1), tokens.palette.surface1);
        assert_eq!(theme::surface(2), tokens.palette.surface2);
        assert_eq!(theme::danger(), tokens.palette.destructive);
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
                theme::radius(Radius::Overlay)
            ],
            [8.0, 12.0, 14.0, 16.0]
        );
    }

    #[test]
    fn the_bundled_font_is_inter_and_the_licence_ships_with_it() {
        assert_eq!(theme::FONT, "Inter");
        assert!(theme::fonts().iter().any(|f| f.contains("Inter")));
        let ofl = include_str!("../../assets/fonts/OFL.txt");
        assert!(ofl.contains("SIL Open Font License"));
    }
}
