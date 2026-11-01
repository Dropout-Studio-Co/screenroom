use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Picked {
    Path(PathBuf),

    Cancelled,

    Unavailable(&'static str),
}

pub fn choose_folder(start: Option<&Path>) -> Picked {
    #[cfg(target_os = "macos")]
    {
        let mut dialog = rfd::FileDialog::new().set_title("Add a folder of your media");
        if let Some(start) = start.filter(|path| path.is_dir()) {
            dialog = dialog.set_directory(start);
        }
        match dialog.pick_folder() {
            Some(folder) => Picked::Path(folder),
            None => Picked::Cancelled,
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = start;
        Picked::Unavailable(
            "This build has no native folder picker on this platform; type the folder's path.",
        )
    }
}

pub fn choose_backup_path(start: Option<&Path>, file_name: &str) -> Picked {
    #[cfg(target_os = "macos")]
    {
        let mut dialog = rfd::FileDialog::new()
            .set_title("Save a Screenroom backup")
            .set_file_name(file_name)
            .add_filter("Screenroom backup", &["db"]);
        if let Some(start) = start.filter(|path| path.is_dir()) {
            dialog = dialog.set_directory(start);
        }
        match dialog.save_file() {
            Some(path) => Picked::Path(path),
            None => Picked::Cancelled,
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (start, file_name);
        Picked::Unavailable("This build has no native save panel; type the backup path.")
    }
}

pub fn choose_subtitle(start: Option<&Path>) -> Picked {
    #[cfg(target_os = "macos")]
    {
        let mut dialog = rfd::FileDialog::new()
            .set_title("Choose a subtitle file")
            .add_filter(
                "Subtitle",
                &["srt", "smi", "vtt", "ass", "ssa", "sub", "ttml", "dfxp"],
            );
        if let Some(start) = start.filter(|path| path.is_dir()) {
            dialog = dialog.set_directory(start);
        }
        match dialog.pick_file() {
            Some(path) => Picked::Path(path),
            None => Picked::Cancelled,
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = start;
        Picked::Unavailable("This build has no native file panel; import is unavailable here.")
    }
}

pub fn choose_restore_file(start: Option<&Path>) -> Picked {
    #[cfg(target_os = "macos")]
    {
        let mut dialog = rfd::FileDialog::new()
            .set_title("Choose a Screenroom backup")
            .add_filter("Screenroom backup", &["db"]);
        if let Some(start) = start.filter(|path| path.is_dir()) {
            dialog = dialog.set_directory(start);
        }
        match dialog.pick_file() {
            Some(path) => Picked::Path(path),
            None => Picked::Cancelled,
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = start;
        Picked::Unavailable("This build has no native file panel; type the backup path.")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unavailable_picker_says_so_rather_than_looking_cancelled() {
        let picked = Picked::Unavailable("no picker");
        assert_ne!(picked, Picked::Cancelled);
    }
}
