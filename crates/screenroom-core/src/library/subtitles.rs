use std::path::Path;

use rusqlite::params;
use serde::{Deserialize, Serialize};

use super::{Library, new_id, now_ms};
use crate::error::{CoreError, Result};

pub const MAX_SUBTITLE_BYTES: u64 = 4 * 1024 * 1024;

pub const SUBTITLE_EXTENSIONS: &[&str] =
    &["srt", "smi", "vtt", "ass", "ssa", "sub", "ttml", "dfxp"];

const IMPORTED: &str = "imported by you from a file on this machine";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredSubtitle {
    pub id: String,

    pub file_id: String,

    pub label: String,

    pub language: String,

    pub format: String,

    pub provenance: String,

    pub bytes: i64,

    pub created_ms: i64,
}

pub fn language_label(body: &str, filename: &str) -> String {
    let code = suggestion(body, filename);
    describe(&code).to_string()
}
fn suggestion(body: &str, filename: &str) -> String {
    let name = filename.rsplit('/').next().unwrap_or(filename);
    let stem = name.rsplit_once('.').map(|(stem, _)| stem).unwrap_or(name);
    let tokens: Vec<String> = stem
        .split(['.', '_', '-', ' ', '[', ']', '(', ')'])
        .filter(|token| !token.is_empty())
        .map(|token| token.to_ascii_lowercase())
        .collect();
    for candidate in tokens.iter().rev().take(4) {
        if (2..=3).contains(&candidate.len())
            && candidate
                .chars()
                .all(|character| character.is_ascii_alphabetic())
            && describe(candidate) != "Unknown"
        {
            return candidate.clone();
        }
    }
    let lower = body.to_ascii_lowercase();
    for (code, _) in KNOWN {
        if lower.contains(&format!("lang=\"{code}\""))
            || lower.contains(&format!("language: {code}"))
        {
            return (*code).to_string();
        }
    }
    "und".to_string()
}

const KNOWN: &[(&str, &str)] = &[
    ("en", "English"),
    ("bn", "Bengali"),
    ("hi", "Hindi"),
    ("ur", "Urdu"),
    ("ar", "Arabic"),
    ("es", "Spanish"),
    ("fr", "French"),
    ("de", "German"),
    ("it", "Italian"),
    ("pt", "Portuguese"),
    ("ru", "Russian"),
    ("ja", "Japanese"),
    ("ko", "Korean"),
    ("zh", "Chinese"),
    ("ta", "Tamil"),
    ("te", "Telugu"),
    ("ml", "Malayalam"),
    ("id", "Indonesian"),
    ("tr", "Turkish"),
    ("nl", "Dutch"),
    ("pl", "Polish"),
    ("fa", "Persian"),
    ("und", "Unknown"),
];

fn describe(code: &str) -> &'static str {
    KNOWN
        .iter()
        .find(|(known, _)| *known == code)
        .map(|(_, label)| *label)
        .unwrap_or("Unknown")
}

fn format_of(path: &Path) -> Option<String> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    SUBTITLE_EXTENSIONS
        .contains(&extension.as_str())
        .then_some(extension)
}

fn looks_textual(bytes: &[u8]) -> bool {
    let sample = &bytes[..bytes.len().min(8_192)];
    !sample.contains(&0) && sample.iter().any(|byte| !byte.is_ascii_whitespace())
}

impl Library {
    pub fn subtitles_for(&self, file_id: &str) -> Result<Vec<StoredSubtitle>> {
        let connection = self.connection();
        let mut statement = connection.prepare(
            "SELECT id, file_id, label, language, format, provenance, LENGTH(bytes), created_ms
             FROM subtitles WHERE file_id = ?1 ORDER BY language, created_ms",
        )?;
        let rows = statement.query_map(params![file_id], |row| {
            Ok(StoredSubtitle {
                id: row.get(0)?,
                file_id: row.get(1)?,
                label: row.get(2)?,
                language: row.get(3)?,
                format: row.get(4)?,
                provenance: row.get(5)?,
                bytes: row.get(6)?,
                created_ms: row.get(7)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn import_subtitle(&self, file_id: &str, source: &Path) -> Result<StoredSubtitle> {
        let format = format_of(source).ok_or_else(|| {
            CoreError::library(
                "subtitle_format",
                "A subtitle must be an srt, smi, vtt, ass, ssa, sub, ttml or dfxp file.",
            )
        })?;
        let metadata = std::fs::metadata(source).map_err(|_| {
            CoreError::library("subtitle_unreadable", "That subtitle file cannot be read.")
        })?;
        if !metadata.is_file() {
            return Err(CoreError::library(
                "subtitle_unreadable",
                "That subtitle file cannot be read.",
            ));
        }
        if metadata.len() == 0 || metadata.len() > MAX_SUBTITLE_BYTES {
            return Err(CoreError::library(
                "subtitle_size",
                "That subtitle is empty or larger than 4 MiB.",
            ));
        }
        let bytes = std::fs::read(source).map_err(|_| {
            CoreError::library("subtitle_unreadable", "That subtitle file cannot be read.")
        })?;
        if !looks_textual(&bytes) {
            return Err(CoreError::library(
                "subtitle_binary",
                "That file is not a text subtitle.",
            ));
        }
        let name = source
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("subtitle");
        let body = String::from_utf8_lossy(&bytes);
        let language = suggestion(&body, name);
        let label = format!("{} · {}", describe(&language), name);
        let exists: Option<String> = self
            .connection()
            .query_row(
                "SELECT id FROM files WHERE id = ?1",
                params![file_id],
                |row| row.get(0),
            )
            .ok();
        if exists.is_none() {
            return Err(CoreError::library(
                "subtitle_file",
                "That file is no longer in the library.",
            ));
        }
        let stored = StoredSubtitle {
            id: new_id(),
            file_id: file_id.to_string(),
            label,
            language: describe(&language).to_string(),
            format,
            provenance: IMPORTED.to_string(),
            bytes: bytes.len() as i64,
            created_ms: now_ms(),
        };
        self.connection().execute(
            "INSERT INTO subtitles(id, file_id, label, language, format, provenance, bytes, created_ms)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                stored.id,
                stored.file_id,
                stored.label,
                stored.language,
                stored.format,
                stored.provenance,
                bytes,
                stored.created_ms
            ],
        )?;
        Ok(stored)
    }

    pub fn subtitle_bytes(&self, id: &str) -> Result<Option<Vec<u8>>> {
        let connection = self.connection();
        let mut statement =
            connection.prepare("SELECT bytes FROM subtitles WHERE id = ?1 LIMIT 1")?;
        let mut rows = statement.query(params![id])?;
        match rows.next()? {
            Some(row) => Ok(Some(row.get(0)?)),
            None => Ok(None),
        }
    }

    pub fn subtitle_path_for(&self, id: &str) -> Result<Option<std::path::PathBuf>> {
        let (format, bytes) = {
            let connection = self.connection();
            let mut statement =
                connection.prepare("SELECT format, bytes FROM subtitles WHERE id = ?1 LIMIT 1")?;
            let mut rows = statement.query(params![id])?;
            let Some(row) = rows.next()? else {
                return Ok(None);
            };
            (row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?)
        };
        let directory = std::env::temp_dir().join("screenroom-subtitles");
        std::fs::create_dir_all(&directory)?;
        let path = directory.join(format!(
            "screenroom-{}.{}",
            id.replace(|character: char| !character.is_ascii_alphanumeric(), ""),
            format
        ));
        std::fs::write(&path, bytes)?;
        Ok(Some(path))
    }

    pub fn remove_subtitle(&self, id: &str) -> Result<bool> {
        let removed = self
            .connection()
            .execute("DELETE FROM subtitles WHERE id = ?1", params![id])?;
        Ok(removed > 0)
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::MAX_SUBTITLE_BYTES;
    use crate::library::{Ingest, Library};
    use crate::{SCHEMA_VERSION, parse_filename};

    fn library_with_a_film() -> (tempfile::TempDir, Library, String) {
        let directory = tempfile::tempdir().unwrap();
        let library = Library::open_in_memory().unwrap();
        let source = library.add_source(directory.path(), None).unwrap();
        let name = "Film.2020.mkv";
        std::fs::write(directory.path().join(name), b"video").unwrap();
        let parsed = parse_filename(name, None);
        library
            .ingest_file(Ingest {
                source_id: &source.id,
                relative_path: name,
                fingerprint: name,
                size: 5,
                modified_ms: 1,
                scan_id: "scan-1",
                parsed: Some(&parsed),
                expected_root: Some(&source.root_path),
            })
            .unwrap();
        let file_id = library
            .items(&crate::library::LibraryQuery::page(10))
            .unwrap()[0]
            .id
            .clone();
        (directory, library, file_id)
    }

    fn write(directory: &Path, name: &str, body: &str) -> PathBuf {
        let path = directory.join(name);
        std::fs::write(&path, body).unwrap();
        path
    }

    #[test]
    fn an_imported_subtitle_is_kept_with_its_language_format_and_provenance() {
        let (directory, library, file_id) = library_with_a_film();
        let path = write(
            directory.path(),
            "Film.2020.en.srt",
            "1\n00:00:01,000 --> 00:00:02,000\nHello\n",
        );
        let stored = library.import_subtitle(&file_id, &path).unwrap();
        assert_eq!(stored.language, "English");
        assert_eq!(stored.format, "srt");
        assert!(stored.label.starts_with("English"));
        assert!(stored.provenance.contains("imported by you"));
        assert_eq!(stored.bytes, std::fs::metadata(&path).unwrap().len() as i64);

        let listed = library.subtitles_for(&file_id).unwrap();
        assert_eq!(listed, vec![stored.clone()]);

        let bytes = library.subtitle_bytes(&stored.id).unwrap().expect("bytes");
        assert_eq!(bytes, std::fs::read(&path).unwrap());

        let materialised = library
            .subtitle_path_for(&stored.id)
            .unwrap()
            .expect("a path for the player");
        assert_eq!(std::fs::read(&materialised).unwrap(), bytes);
        assert!(materialised.extension().unwrap() == "srt");
    }

    #[test]
    fn bengali_and_unknown_languages_are_named_not_guessed_wrongly() {
        let (directory, library, file_id) = library_with_a_film();
        let bengali = write(
            directory.path(),
            "Film.2020[bn].smi",
            "<SAMI><BODY><SYNC Start=1000><P>নমস্কার</P></BODY></SAMI>",
        );
        let stored = library.import_subtitle(&file_id, &bengali).unwrap();
        assert_eq!(stored.language, "Bengali");

        let plain = write(directory.path(), "subtitle.vtt", "WEBVTT\n");
        let stored = library.import_subtitle(&file_id, &plain).unwrap();
        assert_eq!(stored.language, "Unknown");
    }

    #[test]
    fn binaries_wrong_extensions_empty_and_oversized_files_are_refused() {
        let (directory, library, file_id) = library_with_a_film();
        let binary = directory.path().join("Film.2020.srt");
        std::fs::write(&binary, b"\0\0\0\0").unwrap();
        assert_eq!(
            library
                .import_subtitle(&file_id, &binary)
                .unwrap_err()
                .code(),
            "subtitle_binary"
        );

        let wrong = write(directory.path(), "Film.2020.txt", "Hello");
        assert_eq!(
            library
                .import_subtitle(&file_id, &wrong)
                .unwrap_err()
                .code(),
            "subtitle_format"
        );

        let empty = directory.path().join("empty.srt");
        std::fs::write(&empty, b"").unwrap();
        assert_eq!(
            library
                .import_subtitle(&file_id, &empty)
                .unwrap_err()
                .code(),
            "subtitle_size"
        );

        let oversized = directory.path().join("big.srt");
        std::fs::write(&oversized, vec![b'a'; MAX_SUBTITLE_BYTES as usize + 1]).unwrap();
        assert_eq!(
            library
                .import_subtitle(&file_id, &oversized)
                .unwrap_err()
                .code(),
            "subtitle_size"
        );

        assert!(library.subtitles_for(&file_id).unwrap().is_empty());
    }

    #[test]
    fn removal_takes_the_copy_and_leaves_the_original_file_alone() {
        let (directory, library, file_id) = library_with_a_film();
        let path = write(
            directory.path(),
            "Film.2020.srt",
            "1\n00:00:01,000 --> 00:00:02,000\nHi\n",
        );
        let stored = library.import_subtitle(&file_id, &path).unwrap();
        assert!(library.remove_subtitle(&stored.id).unwrap());
        assert!(!library.remove_subtitle(&stored.id).unwrap());
        assert!(library.subtitles_for(&file_id).unwrap().is_empty());
        assert!(path.is_file(), "the file the owner picked must survive");
    }

    #[test]
    fn a_schema_seven_library_gains_the_subtitle_table_and_keeps_its_rows() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("library.db");
        {
            let library = Library::open(&path).unwrap();
            library
                .connection()
                .execute("DROP TABLE subtitles", [])
                .unwrap();
            library
                .connection()
                .execute("INSERT INTO works(id, identity_key, title, kind) VALUES('w','k','Kept','movie')", [])
                .unwrap();
            library
                .connection()
                .pragma_update(None, "user_version", 7)
                .unwrap();
        }
        let library = Library::open(&path).unwrap();
        let version: i64 = library
            .connection()
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
        let title: String = library
            .connection()
            .query_row("SELECT title FROM works WHERE id = 'w'", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(title, "Kept");
    }
}
