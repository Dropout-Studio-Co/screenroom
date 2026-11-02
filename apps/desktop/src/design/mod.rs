use iced::border::Radius;
use iced::widget::{button, container, text_input};
use iced::{Background, Border, Color, Shadow, Theme};

const ROOT_PX: f32 = 16.0;

pub const fn color(r: f32, g: f32, b: f32, a: f32) -> Color {
    Color { r, g, b, a }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    pub background: Color,

    pub surface1: Color,

    pub surface2: Color,

    pub surface3: Color,

    pub foreground: Color,

    pub ink_muted: Color,

    pub signal: Color,

    pub signal_foreground: Color,

    pub focus_surface: Color,

    pub focus_foreground: Color,

    pub destructive: Color,

    pub destructive_foreground: Color,

    pub hair: Color,

    pub disabled_foreground: Color,
}

#[derive(Debug, Clone, Copy)]
pub struct TypeScale {
    pub micro: f32,

    pub caption: f32,

    pub label: f32,

    pub body: f32,

    pub body_lg: f32,

    pub lead: f32,

    pub subtitle: f32,

    pub title_sm: f32,

    pub title: f32,

    pub title_lg: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct RadiusScale {
    pub small: f32,

    pub control: f32,

    pub surface: f32,

    pub overlay: f32,

    pub full: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct ControlScale {
    pub sm: f32,

    pub md: f32,

    pub lg: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct Spacing {
    pub shell_pad: f32,

    pub content_pad: f32,

    pub section: f32,

    pub field_x: f32,

    pub field_y: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct Tokens {
    pub palette: Palette,

    pub type_scale: TypeScale,

    pub radius: RadiusScale,

    pub control: ControlScale,

    pub spacing: Spacing,

    pub provenance: &'static str,
}

pub const NEUTRAL: Tokens = Tokens {
    palette: Palette {
        background: color(0.07, 0.07, 0.08, 1.0),
        surface1: color(0.11, 0.11, 0.13, 1.0),
        surface2: color(0.15, 0.15, 0.17, 1.0),
        surface3: color(0.19, 0.19, 0.22, 1.0),
        foreground: color(0.96, 0.96, 0.97, 1.0),
        ink_muted: color(0.66, 0.66, 0.70, 1.0),
        signal: color(0.99, 0.99, 1.00, 1.0),
        signal_foreground: color(0.08, 0.08, 0.09, 1.0),
        focus_surface: color(0.98, 0.98, 1.00, 1.0),
        focus_foreground: color(0.06, 0.06, 0.07, 1.0),
        destructive: color(0.85, 0.35, 0.35, 1.0),
        destructive_foreground: color(0.06, 0.06, 0.07, 1.0),
        hair: color(1.0, 1.0, 1.0, 0.10),
        disabled_foreground: color(0.45, 0.45, 0.49, 1.0),
    },
    type_scale: TypeScale {
        micro: 0.625 * ROOT_PX,
        caption: 0.75 * ROOT_PX,
        label: 0.875 * ROOT_PX,
        body: ROOT_PX,
        body_lg: 1.0625 * ROOT_PX,
        lead: 1.125 * ROOT_PX,
        subtitle: 1.5 * ROOT_PX,
        title_sm: 1.875 * ROOT_PX,
        title: 2.25 * ROOT_PX,
        title_lg: 3.0 * ROOT_PX,
    },
    radius: RadiusScale {
        small: 0.5 * ROOT_PX,
        control: 0.75 * ROOT_PX,
        surface: 0.875 * ROOT_PX,
        overlay: ROOT_PX,
        full: 9999.0,
    },
    control: ControlScale {
        sm: 2.25 * ROOT_PX,
        md: 2.5 * ROOT_PX,
        lg: 2.75 * ROOT_PX,
    },
    spacing: Spacing {
        shell_pad: 1.125 * ROOT_PX,
        content_pad: ROOT_PX,
        section: 6.0 * ROOT_PX,
        field_x: 0.9375 * ROOT_PX,
        field_y: 0.8125 * ROOT_PX,
    },
    provenance: "public fallback (no design-system pin in this build)",
};

#[cfg(screenroom_ds_pinned)]
#[path = "ds_pinned.rs"]
#[rustfmt::skip]
mod pinned;

pub fn tokens() -> &'static Tokens {
    #[cfg(screenroom_ds_pinned)]
    {
        &pinned::PINNED
    }
    #[cfg(not(screenroom_ds_pinned))]
    {
        &NEUTRAL
    }
}

pub fn provenance() -> &'static str {
    tokens().provenance
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Page,

    Panel,

    Raised,

    Overlay,
}

impl Level {
    fn fill(self) -> Color {
        let palette = &tokens().palette;
        match self {
            Self::Page => palette.background,
            Self::Panel => palette.surface1,
            Self::Raised => palette.surface2,
            Self::Overlay => palette.surface3,
        }
    }
}

pub fn container_style(level: Level, focused: bool) -> container::Style {
    let palette = &tokens().palette;
    container::Style {
        background: Some(Background::Color(if focused {
            palette.focus_surface
        } else {
            level.fill()
        })),
        text_color: Some(if focused {
            palette.focus_foreground
        } else {
            palette.foreground
        }),
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: tokens().radius.surface.into(),
        },
        shadow: Shadow::default(),

        snap: true,
    }
}

pub fn page_style() -> container::Style {
    container_style(Level::Page, false)
}

pub fn tile_style(focused: bool) -> impl Fn(&Theme, button::Status) -> button::Style + 'static {
    move |_theme, status| {
        let palette = &tokens().palette;
        let (background, text_color) = match status {
            button::Status::Disabled => (palette.surface1, palette.disabled_foreground),
            _ if focused => (palette.focus_surface, palette.focus_foreground),
            button::Status::Hovered => (palette.surface2, palette.foreground),
            button::Status::Pressed => (palette.surface3, palette.foreground),
            button::Status::Active => (palette.surface1, palette.foreground),
        };
        button::Style {
            background: Some(Background::Color(background)),
            text_color,
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: tokens().radius.surface.into(),
            },
            shadow: Shadow::default(),
            ..button::Style::default()
        }
    }
}

pub fn action_style(focused: bool) -> impl Fn(&Theme, button::Status) -> button::Style + 'static {
    move |_theme, status| {
        let palette = &tokens().palette;
        let (background, text_color) = match status {
            button::Status::Disabled => (palette.surface1, palette.disabled_foreground),
            _ if focused => (palette.focus_surface, palette.focus_foreground),
            button::Status::Hovered | button::Status::Pressed => {
                (palette.surface3, palette.foreground)
            }
            button::Status::Active => (palette.signal, palette.signal_foreground),
        };
        button::Style {
            background: Some(Background::Color(background)),
            text_color,
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: tokens().radius.control.into(),
            },
            shadow: Shadow::default(),
            ..button::Style::default()
        }
    }
}

pub fn quiet_style(focused: bool) -> impl Fn(&Theme, button::Status) -> button::Style + 'static {
    move |_theme, status| {
        let palette = &tokens().palette;
        let (background, text_color) = match status {
            button::Status::Disabled => (Color::TRANSPARENT, palette.disabled_foreground),
            _ if focused => (palette.focus_surface, palette.focus_foreground),
            button::Status::Hovered | button::Status::Pressed => {
                (palette.surface2, palette.foreground)
            }
            button::Status::Active => (palette.surface1, palette.foreground),
        };
        button::Style {
            background: Some(Background::Color(background)),
            text_color,
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: tokens().radius.control.into(),
            },
            shadow: Shadow::default(),
            ..button::Style::default()
        }
    }
}

pub fn danger_style(focused: bool) -> impl Fn(&Theme, button::Status) -> button::Style + 'static {
    move |_theme, status| {
        let palette = &tokens().palette;
        let (background, text_color) = match status {
            button::Status::Disabled => (palette.surface1, palette.disabled_foreground),
            _ if focused => (palette.focus_surface, palette.focus_foreground),
            _ => (palette.destructive, palette.destructive_foreground),
        };
        button::Style {
            background: Some(Background::Color(background)),
            text_color,
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: tokens().radius.control.into(),
            },
            shadow: Shadow::default(),
            ..button::Style::default()
        }
    }
}

pub fn input_style() -> impl Fn(&Theme, text_input::Status) -> text_input::Style + 'static {
    move |_theme, status| {
        let palette = &tokens().palette;
        let focused = matches!(status, text_input::Status::Focused { .. });
        text_input::Style {
            background: Background::Color(if focused {
                palette.surface3
            } else {
                palette.surface1
            }),
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: tokens().radius.control.into(),
            },
            icon: palette.ink_muted,
            placeholder: palette.ink_muted,
            value: palette.foreground,
            selection: palette.surface3,
        }
    }
}

pub fn hairline() -> Color {
    tokens().palette.hair
}

pub fn radius_surface() -> Radius {
    tokens().radius.surface.into()
}

pub fn theme() -> Theme {
    let palette = &tokens().palette;
    Theme::custom(
        "Screenroom".to_string(),
        iced::theme::Palette {
            background: palette.background,
            text: palette.foreground,
            primary: palette.signal,
            success: palette.focus_surface,
            danger: palette.destructive,

            warning: palette.signal,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_role_is_opaque_unless_it_is_a_hairline() {
        let palette = &tokens().palette;
        for (name, value) in [
            ("background", palette.background),
            ("surface1", palette.surface1),
            ("surface2", palette.surface2),
            ("surface3", palette.surface3),
            ("foreground", palette.foreground),
            ("ink_muted", palette.ink_muted),
            ("signal", palette.signal),
            ("focus_surface", palette.focus_surface),
            ("destructive", palette.destructive),
        ] {
            assert!(
                (value.a - 1.0).abs() < f32::EPSILON,
                "{name} must be fully opaque so surfaces composite predictably"
            );
        }
        assert!(
            palette.hair.a < 1.0,
            "the hairline role is intentionally soft"
        );
    }

    #[test]
    fn focus_is_never_the_rest_fill() {
        let palette = &tokens().palette;
        assert_ne!(palette.focus_surface, palette.surface1);
        assert_ne!(palette.focus_foreground, palette.foreground);
    }

    #[test]
    fn the_type_scale_is_ordered_and_legible() {
        let scale = &tokens().type_scale;
        let ordered = [
            scale.micro,
            scale.caption,
            scale.label,
            scale.body,
            scale.body_lg,
            scale.lead,
            scale.subtitle,
            scale.title_sm,
            scale.title,
            scale.title_lg,
        ];
        for pair in ordered.windows(2) {
            assert!(
                pair[0] < pair[1],
                "the type scale must increase monotonically"
            );
        }
        assert!(scale.caption >= 11.0, "caption text must stay readable");
    }

    #[test]
    fn control_heights_never_shrink_below_a_comfortable_target() {
        let control = &tokens().control;
        assert!(control.sm < control.md && control.md < control.lg);
        assert!(control.md >= 32.0);
    }

    #[test]
    fn provenance_is_always_reported() {
        assert!(!provenance().is_empty());
    }

    #[test]
    fn every_style_function_returns_a_style_for_every_status() {
        let statuses = [
            button::Status::Active,
            button::Status::Hovered,
            button::Status::Pressed,
            button::Status::Disabled,
        ];
        let theme = theme();
        for focused in [false, true] {
            for status in statuses {
                let tile = tile_style(focused)(&theme, status);
                let action = action_style(focused)(&theme, status);
                let quiet = quiet_style(focused)(&theme, status);
                let danger = danger_style(focused)(&theme, status);
                for style in [tile, action, quiet, danger] {
                    assert_eq!(style.border.width, 0.0, "controls must not draw borders");
                    assert!(style.background.is_some());
                }
            }
        }
    }

    #[test]
    fn a_focused_tile_uses_the_focus_fill_instead_of_the_rest_fill() {
        let theme = theme();
        let palette = &tokens().palette;
        let rest = tile_style(false)(&theme, button::Status::Active);
        let focused = tile_style(true)(&theme, button::Status::Active);
        assert_eq!(
            rest.background,
            Some(Background::Color(palette.surface1)),
            "an unfocused card sits on the first surface"
        );
        assert_eq!(
            focused.background,
            Some(Background::Color(palette.focus_surface)),
            "focus must be a stronger fill, not a ring"
        );
    }
}
