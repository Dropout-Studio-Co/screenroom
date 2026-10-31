use crate::library::PathSemantics;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathStyle {
    Windows,

    Posix,
}

impl PathStyle {
    pub fn host() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else {
            Self::Posix
        }
    }
}

const VERBATIM: &str = "//?/";

const VERBATIM_UNC: &str = "//?/UNC/";

pub fn location_key(raw: &str, semantics: PathSemantics, style: PathStyle) -> String {
    let mut key = raw.to_string();

    if style == PathStyle::Windows {
        key = key.replace('\\', "/");

        if key.starts_with("//") {
            let rest = key.trim_start_matches('/');
            key = format!("//{rest}");
        }
    }

    key = strip_verbatim(&key);
    key = collapse_separators(&key);
    key = trim_trailing_separator(&key);

    match semantics {
        PathSemantics::CaseInsensitive => key.to_lowercase(),
        PathSemantics::CaseSensitive => key,
    }
}

fn strip_verbatim(key: &str) -> String {
    if let Some(rest) = key.strip_prefix(VERBATIM_UNC) {
        return format!("//{rest}");
    }

    if key.len() >= VERBATIM_UNC.len()
        && key[..VERBATIM_UNC.len()].eq_ignore_ascii_case(VERBATIM_UNC)
    {
        return format!("//{}", &key[VERBATIM_UNC.len()..]);
    }
    match key.strip_prefix(VERBATIM) {
        Some(rest) => rest.to_string(),
        None => key.to_string(),
    }
}

fn collapse_separators(key: &str) -> String {
    let unc = key.starts_with("//");
    let body = if unc {
        key.trim_start_matches('/')
    } else {
        key
    };
    let mut collapsed = String::with_capacity(key.len());
    let mut last_was_separator = false;
    for character in body.chars() {
        let separator = character == '/';
        if !(separator && last_was_separator) {
            collapsed.push(character);
        }
        last_was_separator = separator;
    }

    if unc && !body.is_empty() {
        format!("//{collapsed}")
    } else {
        collapsed
    }
}

fn trim_trailing_separator(key: &str) -> String {
    let trimmed = key.trim_end_matches('/');
    if trimmed.is_empty() {
        return "/".to_string();
    }
    if key.len() > trimmed.len() && ends_with_drive_prefix(trimmed) {
        return format!("{trimmed}/");
    }
    trimmed.to_string()
}

fn ends_with_drive_prefix(key: &str) -> bool {
    let bytes = key.as_bytes();
    bytes.len() == 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

#[cfg(test)]
mod tests {
    use super::*;

    fn win(raw: &str) -> String {
        location_key(raw, PathSemantics::CaseInsensitive, PathStyle::Windows)
    }

    fn posix(raw: &str) -> String {
        location_key(raw, PathSemantics::CaseSensitive, PathStyle::Posix)
    }

    #[test]
    fn a_verbatim_path_matches_the_spelling_it_was_canonicalized_from() {
        assert_eq!(win(r"\\?\D:\Media"), win(r"D:\Media"));
        assert_eq!(win(r"\\?\D:\Media\Shows"), win(r"D:\Media\Shows"));

        assert_eq!(win("//?/D:/Media"), win(r"D:\Media"));
        assert_eq!(win(r"\\?\d:\media"), win(r"D:\Media"));
    }

    #[test]
    fn a_verbatim_unc_path_matches_its_share() {
        assert_eq!(win(r"\\?\UNC\server\share"), win(r"\\server\share"));
        assert_eq!(
            win(r"\\?\UNC\server\share\Shows"),
            win(r"\\server\share\Shows")
        );

        assert_eq!(win(r"\\?\unc\server\share"), win(r"\\SERVER\share"));
    }

    #[test]
    fn the_device_namespace_is_not_treated_as_a_folder() {
        assert_ne!(win(r"\\.\C:"), win(r"C:\"));

        assert_eq!(win(r"\\.\C:"), "//./c:");
    }

    #[test]
    fn both_windows_separators_name_one_folder() {
        assert_eq!(win(r"D:\Media\Shows"), win("D:/Media/Shows"));
        assert_eq!(win(r"D:\Media\Shows"), win(r"D:\Media/Shows"));

        assert_eq!(win(r"D:\Media\\Shows"), win(r"D:\Media\Shows"));
    }

    #[test]
    fn a_trailing_separator_does_not_make_a_different_folder() {
        assert_eq!(win(r"D:\Media\"), win(r"D:\Media"));
        assert_eq!(win("D:/Media//"), win(r"D:\Media"));
        assert_eq!(posix("/Media/Shows/"), posix("/Media/Shows"));
    }

    #[test]
    fn a_drive_root_is_not_the_drive_prefix() {
        assert_ne!(win(r"C:\"), win("C:"));
        assert_eq!(win(r"C:\"), "c:/");
    }

    #[test]
    fn a_posix_backslash_is_part_of_the_name() {
        assert_ne!(posix(r"/Media/Shows\Archive"), posix("/Media/Shows"));
        assert_eq!(posix(r"/Media/Shows\Archive"), r"/Media/Shows\Archive");
    }

    #[test]
    fn case_folding_follows_the_volume_not_the_style() {
        let windows_sensitive = location_key(
            r"D:\Media",
            PathSemantics::CaseSensitive,
            PathStyle::Windows,
        );
        assert_ne!(
            windows_sensitive,
            location_key(
                r"d:\media",
                PathSemantics::CaseSensitive,
                PathStyle::Windows
            )
        );
        assert_eq!(
            location_key("/Media", PathSemantics::CaseInsensitive, PathStyle::Posix),
            location_key("/media", PathSemantics::CaseInsensitive, PathStyle::Posix)
        );
    }

    #[test]
    fn a_root_stays_a_root_rather_than_becoming_an_empty_key() {
        assert_eq!(posix("/"), "/");
        assert_eq!(posix("///"), "/");
        assert_eq!(win("//server/share/"), "//server/share");
        assert_eq!(win(r"C:\"), "c:/");
    }

    #[test]
    fn a_folder_that_differs_is_never_folded_together() {
        assert_ne!(win(r"D:\Media"), win(r"D:\Media2"));
        assert_ne!(win(r"D:\Media\Shows"), win(r"D:\Media\Movies"));
        assert_ne!(win(r"D:\Media"), win(r"E:\Media"));
        assert_ne!(posix("/Media"), posix("/Media2"));

        assert_ne!(win(r"D:\Media"), win(r"D:\Media\Shows"));
    }
}
