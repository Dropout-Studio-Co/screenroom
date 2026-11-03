use std::path::{Path, PathBuf};

use screenroom_core::library::SUBTITLE_EXTENSIONS;

pub fn subtitle_beside(video: &Path) -> Option<PathBuf> {
    let folder = video.parent()?;
    let stem = video.file_stem()?.to_str()?;
    if stem.is_empty() {
        return None;
    }
    let prefix = format!("{stem}.");
    let mut exact: Option<(String, PathBuf)> = None;
    let mut tagged: Option<(String, PathBuf)> = None;
    for entry in std::fs::read_dir(folder).ok()?.flatten() {
        if !entry
            .file_type()
            .map(|kind| kind.is_file())
            .unwrap_or(false)
        {
            continue;
        }
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let Some((base, extension)) = name.rsplit_once('.') else {
            continue;
        };
        if !SUBTITLE_EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str()) {
            continue;
        }
        let chosen = if base == stem {
            &mut exact
        } else if base
            .strip_prefix(&prefix)
            .is_some_and(|tag| !tag.is_empty())
        {
            &mut tagged
        } else {
            continue;
        };
        let earlier = chosen
            .as_ref()
            .is_none_or(|(current, _)| name < current.as_str());
        if earlier {
            *chosen = Some((name.to_string(), entry.path()));
        }
    }
    exact.or(tagged).map(|(_, path)| path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn film(folder: &Path) -> PathBuf {
        let path = folder.join("Film.2020.mkv");
        std::fs::write(&path, b"video").expect("video");
        path
    }

    fn cue(folder: &Path, name: &str) -> PathBuf {
        let path = folder.join(name);
        std::fs::write(&path, "1\n00:00:01,000 --> 00:00:02,000\nHello\n").expect("cue");
        path
    }

    #[test]
    fn the_file_with_the_films_own_name_wins_over_a_tagged_one() {
        let directory = tempfile::tempdir().expect("temp dir");
        let video = film(directory.path());
        cue(directory.path(), "Film.2020.en.srt");
        let plain = cue(directory.path(), "Film.2020.srt");

        assert_eq!(subtitle_beside(&video), Some(plain));
    }

    #[test]
    fn one_tagged_file_is_chosen_alphabetically_and_stably() {
        let directory = tempfile::tempdir().expect("temp dir");
        let video = film(directory.path());
        cue(directory.path(), "Film.2020.en.srt");
        let bengali = cue(directory.path(), "Film.2020.bn.srt");
        cue(directory.path(), "Film.2020.en.forced.srt");

        for _ in 0..3 {
            assert_eq!(subtitle_beside(&video), Some(bengali.clone()));
        }
    }

    #[test]
    fn nothing_else_in_the_folder_counts_as_the_films_subtitle() {
        let directory = tempfile::tempdir().expect("temp dir");
        let video = film(directory.path());
        cue(directory.path(), "Film.2020.en.txt");
        cue(directory.path(), "Another Film.2020.en.srt");
        cue(directory.path(), "Film.2020X.en.srt");
        cue(directory.path(), "Film.2020.en");
        std::fs::create_dir(directory.path().join("Film.2020.de.srt")).expect("folder");

        assert_eq!(subtitle_beside(&video), None);
    }

    #[test]
    fn a_subtitle_in_another_folder_is_not_beside_the_film() {
        let directory = tempfile::tempdir().expect("temp dir");
        let folder = directory.path().join("films");
        std::fs::create_dir(&folder).expect("films");
        let video = film(&folder);
        cue(directory.path(), "Film.2020.srt");

        assert_eq!(subtitle_beside(&video), None);
    }

    #[test]
    fn a_path_with_no_folder_or_stem_finds_nothing() {
        assert_eq!(subtitle_beside(Path::new("/")), None);
        assert_eq!(subtitle_beside(Path::new("")), None);
        assert_eq!(subtitle_beside(Path::new("Film.2020.mkv")), None);
    }
}
