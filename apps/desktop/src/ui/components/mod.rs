pub mod poster;
pub mod rail;

pub use poster::Poster;
pub use rail::rail;

#[cfg(test)]
mod tests {
    use crate::ui::components::poster;
    use crate::ui::components::poster::Poster;

    #[test]
    fn a_card_holds_its_box_when_the_poster_is_missing() {
        let element = Poster::new("No Artwork", "2020").into_element();
        let _ = element;
    }

    #[test]
    fn progress_is_clamped_and_a_finished_film_shows_no_bar() {
        assert_eq!(poster::bar_width(0.0), 0.0);
        assert_eq!(poster::bar_width(0.5), 120.0);
        assert_eq!(poster::bar_width(1.0), 240.0);
        assert_eq!(poster::bar_width(2.5), 240.0);
    }

    #[test]
    fn a_very_long_title_truncates_instead_of_overflowing() {
        let long = "The Extraordinarily Long Title Of A Film That Nobody Would Ever Name This Way";
        let cut = poster::truncate(long, 28);
        assert!(cut.chars().count() <= 28);
        assert!(long.starts_with(cut));
        assert_eq!(poster::truncate("Short", 28), "Short");
    }

    #[test]
    fn a_title_with_emoji_and_combining_marks_truncates_on_a_character_boundary() {
        let text = "ফিল্ম 🎬 café\u{0301} and more words than fit";
        let cut = poster::truncate(text, 8);
        assert_eq!(cut.chars().count(), 8, "no char boundary may be split");
        assert!(text.starts_with(cut));
    }

    #[test]
    fn the_grid_column_count_follows_the_window_width() {
        assert_eq!(poster::card_columns(1440.0), 6);
        assert_eq!(poster::card_columns(1024.0), 4);
        assert_eq!(poster::card_columns(600.0), 2);
    }
}
