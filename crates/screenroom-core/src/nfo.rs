use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use regex::Regex;

pub const MAX_SIDECAR_BYTES: u64 = 256 * 1024;

const MAX_TEXT: usize = 2_000;

const MAX_GENRES: usize = 20;

const MAX_GENRE: usize = 40;

const MAX_YEAR: i64 = 2_100;

const MIN_YEAR: i64 = 1_880;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NfoKind {
    Movie,
    Series,
    Episode,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NfoDetails {
    pub kind: Option<NfoKind>,
    pub title: String,
    pub year: Option<i64>,
    pub overview: String,
    pub genres: Vec<String>,
}

impl NfoDetails {
    pub fn is_empty(&self) -> bool {
        self.title.is_empty()
            && self.year.is_none()
            && self.overview.is_empty()
            && self.genres.is_empty()
    }
}

fn entity_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN
        .get_or_init(|| Regex::new(r"&#(?:x[0-9a-fA-F]{1,6}|[0-9]{1,7});").expect("entity pattern"))
}

fn decode(value: &str) -> String {
    let value = entity_pattern()
        .replace_all(value, |found: &regex::Captures<'_>| {
            let body = found[0].trim_start_matches("&#").trim_end_matches(';');
            let code = match body.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok(),
                None => body.parse::<u32>().ok(),
            };
            code.and_then(char::from_u32)
                .map(|character| character.to_string())
                .unwrap_or_default()
        })
        .into_owned();
    value
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

fn strip_tags(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut depth = 0_usize;
    for character in value.chars() {
        match character {
            '<' => depth += 1,
            '>' => depth = depth.saturating_sub(1),
            other if depth == 0 => out.push(other),
            _ => {}
        }
    }
    out
}

fn clean(value: &str, limit: usize) -> String {
    let collapsed = decode(&strip_tags(value))
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    collapsed
        .chars()
        .filter(|character| !character.is_control())
        .take(limit)
        .collect::<String>()
        .trim()
        .to_string()
}

fn open_tag(lower: &str, tag: &str) -> Option<usize> {
    let needle = format!("<{tag}");
    let mut from = 0_usize;
    while let Some(found) = lower[from..].find(&needle) {
        let index = from + found;
        let after = lower[index + needle.len()..].chars().next();
        if matches!(
            after,
            Some('>') | Some('/') | Some(' ') | Some('\t') | Some('\n') | Some('\r')
        ) {
            return Some(index);
        }
        from = index + needle.len();
    }
    None
}

fn values(body: &str, lower: &str, tag: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut from = 0_usize;
    while found.len() < MAX_GENRES {
        let Some(start) = open_tag(&lower[from..], tag) else {
            break;
        };
        let start = from + start;
        let Some(open_end) = body[start..].find('>') else {
            break;
        };
        let text_start = start + open_end + 1;
        let closing = format!("</{tag}>");
        let Some(end) = lower[text_start..].find(&closing) else {
            break;
        };
        found.push(clean(&body[text_start..text_start + end], MAX_TEXT));
        from = text_start + end + closing.len();
    }
    found
}

fn first_value(body: &str, lower: &str, tags: &[&str]) -> Option<String> {
    tags.iter()
        .flat_map(|tag| values(body, lower, tag))
        .map(|value| value.trim().to_string())
        .find(|value| !value.is_empty())
}

fn year_from(raw: Option<String>) -> Option<i64> {
    let raw = raw?;
    let digits = raw
        .chars()
        .take_while(|character| character.is_ascii_digit())
        .collect::<String>();
    let year = digits.parse::<i64>().ok()?;
    (MIN_YEAR..=MAX_YEAR).contains(&year).then_some(year)
}

fn genres_from(raw: Vec<String>) -> Vec<String> {
    let mut genres: Vec<String> = Vec::new();
    for entry in raw {
        for part in entry.split(['/', '|']) {
            let label = part.trim().chars().take(MAX_GENRE).collect::<String>();
            let label = label.trim().to_string();
            if label.is_empty() {
                continue;
            }
            if genres
                .iter()
                .any(|known| known.eq_ignore_ascii_case(&label))
            {
                continue;
            }
            genres.push(label);
            if genres.len() >= MAX_GENRES {
                return genres;
            }
        }
    }
    genres
}

pub fn parse(bytes: &[u8]) -> Option<NfoDetails> {
    let limit = bytes.len().min(MAX_SIDECAR_BYTES as usize);
    if limit == 0 {
        return None;
    }
    let text = String::from_utf8_lossy(&bytes[..limit]);
    let body = text.trim_start_matches('\u{feff}');
    let lower = body.to_ascii_lowercase();
    let (kind, start) = [
        (NfoKind::Movie, "movie"),
        (NfoKind::Series, "tvshow"),
        (NfoKind::Episode, "episodedetails"),
    ]
    .into_iter()
    .filter_map(|(kind, tag)| open_tag(&lower, tag).map(|start| (kind, start)))
    .min_by_key(|(_, start)| *start)?;
    let section = &body[start..];
    let section_lower = &lower[start..];

    let details = NfoDetails {
        kind: Some(kind),
        title: first_value(section, section_lower, &["title"]).unwrap_or_default(),
        year: year_from(first_value(
            section,
            section_lower,
            &["year", "premiered", "releasedate"],
        )),
        overview: first_value(section, section_lower, &["plot", "outline", "overview"])
            .unwrap_or_default(),
        genres: genres_from(values(section, section_lower, "genre")),
    };
    (!details.is_empty()).then_some(details)
}

pub fn sidecars(video: &Path) -> Vec<PathBuf> {
    let Some(directory) = video.parent() else {
        return Vec::new();
    };
    let mut candidates = Vec::with_capacity(3);
    if let Some(stem) = video.file_stem() {
        candidates.push(directory.join(format!("{}.nfo", stem.to_string_lossy())));
    }
    candidates.push(directory.join("movie.nfo"));
    candidates.push(directory.join("tvshow.nfo"));
    candidates
}

pub fn read(video: &Path) -> Option<(PathBuf, NfoDetails)> {
    for candidate in sidecars(video) {
        let Ok(metadata) = std::fs::metadata(&candidate) else {
            continue;
        };
        if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_SIDECAR_BYTES {
            continue;
        }
        let Ok(bytes) = std::fs::read(&candidate) else {
            continue;
        };
        if let Some(details) = parse(&bytes) {
            return Some((candidate, details));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_movie_sidecar_yields_title_year_overview_and_genres() {
        let details = parse(
            br#"<?xml version="1.0" encoding="UTF-8"?>
            <movie>
              <title>Pather Panchali</title>
              <year>1955</year>
              <genre>Drama</genre>
              <genre>World Cinema / Family</genre>
              <plot>  A boy and his
                 family in a Bengali village.  </plot>
            </movie>"#,
        )
        .expect("details");
        assert_eq!(details.kind, Some(NfoKind::Movie));
        assert_eq!(details.title, "Pather Panchali");
        assert_eq!(details.year, Some(1955));
        assert_eq!(
            details.overview,
            "A boy and his family in a Bengali village."
        );
        assert_eq!(details.genres, vec!["Drama", "World Cinema", "Family"]);
    }

    #[test]
    fn entities_and_nested_markup_are_decoded_not_shown() {
        let details = parse(
            b"<tvshow><title>Tom &amp; Jerry&#8217;s Show</title><plot><b>Bold</b> plot &lt;not a tag&gt;</plot></tvshow>",
        )
        .expect("details");
        assert_eq!(details.kind, Some(NfoKind::Series));
        assert_eq!(details.title, "Tom & Jerry\u{2019}s Show");
        assert_eq!(details.overview, "Bold plot <not a tag>");
    }

    #[test]
    fn an_episode_sidecar_keeps_its_own_title_and_kind() {
        let details = parse(
            b"<episodedetails><title>The Offer</title><season>2</season><episode>4</episode><plot>A trade.</plot></episodedetails>",
        )
        .expect("details");
        assert_eq!(details.kind, Some(NfoKind::Episode));
        assert_eq!(details.title, "The Offer");
    }

    #[test]
    fn unusable_sidecars_are_refused_rather_than_guessed() {
        assert!(parse(b"").is_none());
        assert!(parse(b"<html><body>not media</body></html>").is_none());
        assert!(parse(b"<movie><title>   </title><plot></plot></movie>").is_none());
        let narrow = parse(b"<movie><title>Old</title><year>12</year></movie>").expect("details");
        assert_eq!(narrow.year, None);
    }

    #[test]
    fn long_values_are_bounded_and_genres_deduplicated() {
        let title = "A".repeat(10_000);
        let body = format!("<movie><title>{title}</title></movie>");
        let details = parse(body.as_bytes()).expect("details");
        assert_eq!(details.title.chars().count(), MAX_TEXT);

        let details = parse(
            b"<movie><title>X</title><genre>Drama</genre><genre>drama</genre><genre> Comedy </genre></movie>",
        )
        .expect("details");
        assert_eq!(details.genres, vec!["Drama", "Comedy"]);
    }

    #[test]
    fn a_sidecar_beside_the_video_is_preferred_over_the_folder_wide_one() {
        let directory = tempfile::tempdir().unwrap();
        let video = directory.path().join("Film.2001.mkv");
        std::fs::write(&video, b"video").unwrap();
        std::fs::write(
            directory.path().join("Film.2001.nfo"),
            b"<movie><title>Beside</title></movie>",
        )
        .unwrap();
        std::fs::write(
            directory.path().join("movie.nfo"),
            b"<movie><title>Folder</title></movie>",
        )
        .unwrap();
        let (path, details) = read(&video).expect("sidecar");
        assert!(path.ends_with("Film.2001.nfo"));
        assert_eq!(details.title, "Beside");
    }

    #[test]
    fn an_oversized_or_missing_sidecar_is_skipped() {
        let directory = tempfile::tempdir().unwrap();
        let video = directory.path().join("Film.2001.mkv");
        std::fs::write(&video, b"video").unwrap();
        let oversized = vec![b'A'; MAX_SIDECAR_BYTES as usize + 1];
        std::fs::write(directory.path().join("Film.2001.nfo"), oversized).unwrap();
        assert!(read(&video).is_none());
    }
}
