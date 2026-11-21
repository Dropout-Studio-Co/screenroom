use iced::advanced::graphics::text::Paragraph;
use iced::advanced::graphics::text::font_system;
use iced::advanced::text::{Alignment, LineHeight, Paragraph as ParagraphTrait, Shaping, Text};
use iced::alignment::Vertical;
use iced::widget::text::Wrapping;
use iced::{Font, Pixels, Size};
use std::borrow::Cow;
use std::sync::OnceLock;

pub const ELLIPSIS: char = '…';

const UNBOUNDED: f32 = 100_000.0;

pub fn ensure_fonts() {
    static LOADED: OnceLock<()> = OnceLock::new();

    LOADED.get_or_init(|| {
        let mut system = font_system().write().expect("the font system is lockable");

        for face in crate::ui::theme::font_data() {
            system.load_font(Cow::Borrowed(face));
        }
    });
}

fn bounds_width(width: f32) -> f32 {
    if width.is_finite() && width > 0.0 {
        width
    } else {
        UNBOUNDED
    }
}

fn bounds_size(size: f32) -> f32 {
    if size.is_finite() && size > 0.0 {
        size
    } else {
        1.0
    }
}

fn laid_out(text: &str, size: f32, font: Font, wrapping: Wrapping, width: f32) -> Size {
    ensure_fonts();

    let paragraph = <Paragraph as ParagraphTrait>::with_text(Text {
        content: text,
        bounds: Size::new(bounds_width(width), UNBOUNDED),
        size: Pixels(bounds_size(size)),
        line_height: LineHeight::default(),
        font,
        align_x: Alignment::Left,
        align_y: Vertical::Top,
        shaping: Shaping::Auto,
        wrapping,
    });

    Size::new(paragraph.min_width(), paragraph.min_height())
}

pub fn measured_width(text: &str, size: f32, font: Font) -> f32 {
    laid_out(text, size, font, Wrapping::None, UNBOUNDED).width
}

pub fn measured_height(text: &str, size: f32, font: Font, width: f32) -> f32 {
    laid_out(text, size, font, Wrapping::Word, width).height
}

fn ellipsis_room(room: f32) -> Option<f32> {
    if room.is_nan() {
        return None;
    }

    if room == f32::INFINITY {
        return Some(room);
    }

    (room > 0.0).then_some(room)
}

pub fn fit_measured(text: &str, room: f32, size: f32, font: Font) -> String {
    if text.is_empty() {
        return String::new();
    }

    let Some(room) = ellipsis_room(room) else {
        return String::new();
    };

    if room.is_infinite() || measured_width(text, size, font) <= room {
        return text.to_string();
    }

    let tail = ELLIPSIS.to_string();

    if measured_width(&tail, size, font) > room {
        return String::new();
    }

    let boundaries: Vec<usize> = text
        .char_indices()
        .map(|(index, _)| index)
        .chain(std::iter::once(text.len()))
        .collect();

    let mut low = 0;
    let mut high = boundaries.len() - 1;

    while low < high {
        let middle = low + (high - low).div_ceil(2);
        let candidate = format!("{}{tail}", &text[..boundaries[middle]]);

        if measured_width(&candidate, size, font) <= room {
            low = middle;
        } else {
            high = middle - 1;
        }
    }

    let kept = text[..boundaries[low]].trim_end();

    format!("{kept}{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::theme;

    #[test]
    fn a_wide_glyph_measures_wider_than_a_narrow_one() {
        let regular = theme::regular();
        let wide = measured_width("WWW", 14.0, regular);
        let narrow = measured_width("iii", 14.0, regular);

        assert!(wide > narrow, "WWW measured {wide}, iii measured {narrow}");
    }

    #[test]
    fn a_character_the_font_must_substitute_still_measures() {
        let regular = theme::regular();
        let ascii = measured_width("tttttt", 14.0, regular);
        let cjk = measured_width("日本語字幕", 14.0, regular);

        assert!(cjk > ascii, "CJK measured {cjk}, ASCII measured {ascii}");
        assert!(cjk.is_finite() && cjk > 0.0);
    }

    #[test]
    fn larger_text_measures_wider_at_the_same_content() {
        let regular = theme::regular();

        assert!(
            measured_width("Parasite", 24.0, regular) > measured_width("Parasite", 12.0, regular)
        );
    }

    #[test]
    fn a_fitted_label_ends_with_an_ellipsis_and_fits_its_room() {
        let regular = theme::regular();
        let label = "Parasite.2019.1080p.AMZN.WEB-DL.Dual.Audio.HEVC.DDP.5.1";
        let room = 140.0;
        let fitted = fit_measured(label, room, 12.0, regular);

        assert!(fitted.ends_with(ELLIPSIS), "{fitted}");
        let width = measured_width(&fitted, 12.0, regular);
        assert!(width <= room, "{width} exceeds {room}");
        assert!(label.starts_with(fitted.trim_end_matches(ELLIPSIS).trim_end()));
    }

    #[test]
    fn a_label_that_already_fits_is_returned_untouched() {
        let regular = theme::regular();

        assert_eq!(fit_measured("Short", 400.0, 12.0, regular), "Short");
        assert_eq!(fit_measured("Short", f32::INFINITY, 12.0, regular), "Short");
    }

    #[test]
    fn a_room_with_no_space_yields_no_label() {
        let regular = theme::regular();

        assert_eq!(fit_measured("Parasite", 0.0, 12.0, regular), "");
        assert_eq!(fit_measured("Parasite", -20.0, 12.0, regular), "");
        assert_eq!(fit_measured("Parasite", f32::NAN, 12.0, regular), "");
        assert_eq!(fit_measured("", 120.0, 12.0, regular), "");
    }

    #[test]
    fn a_room_narrower_than_the_ellipsis_yields_no_label() {
        let regular = theme::regular();
        let ellipsis = measured_width(&ELLIPSIS.to_string(), 12.0, regular);

        assert!(ellipsis > 0.0);
        assert_eq!(fit_measured("Parasite", ellipsis - 0.5, 12.0, regular), "");
        assert_eq!(
            fit_measured("Parasite", ellipsis, 12.0, regular),
            ELLIPSIS.to_string()
        );
    }

    #[test]
    fn truncation_never_splits_a_multi_byte_character() {
        let regular = theme::regular();
        let label = "映画の字幕ファイル名がとても長い場合のテスト用ラベルです";

        for room in [10.0_f32, 15.0, 20.0, 40.0, 80.0, 160.0] {
            let fitted = fit_measured(label, room, 12.0, regular);
            assert!(label.starts_with(fitted.trim_end_matches(ELLIPSIS)));
            assert!(!fitted.contains('\u{FFFD}'));
            assert!(measured_width(&fitted, 12.0, regular) <= room);
        }
    }

    #[test]
    fn a_wrapped_height_grows_as_the_column_narrows() {
        let regular = theme::regular();
        let caption = "A subtitle track inside the film always comes first, and a subtitle file \
                       sitting beside the film comes next.";

        let wide = measured_height(caption, 12.0, regular, 400.0);
        let narrow = measured_height(caption, 12.0, regular, 160.0);

        assert!(narrow > wide, "narrow {narrow} must wrap past wide {wide}");
        assert!(measured_height("One line", 12.0, regular, 400.0) > 0.0);
    }
}
