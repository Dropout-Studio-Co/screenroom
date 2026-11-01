use rusqlite::params;
use serde::{Deserialize, Serialize};

use super::{Library, new_id, now_ms};
use crate::error::{CoreError, Result};

pub const MAX_LABEL: usize = 80;

pub const MAX_ADDRESS: usize = 2_048;

pub const MAX_ACCESS: usize = 120;

const UNVERIFIED: &str = "added by you here; nothing about it has been verified from this machine";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bookmark {
    pub id: String,

    pub label: String,

    pub address: String,

    pub protocol: String,

    pub access: String,

    pub provenance: String,

    pub added_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BookmarkDraft {
    pub label: String,

    pub address: String,

    pub access: String,
}

pub fn protocol_of(address: &str) -> Result<String> {
    let lower = address.trim().to_ascii_lowercase();
    let scheme = lower
        .split_once("://")
        .map(|(scheme, _)| scheme)
        .filter(|scheme| !scheme.is_empty())
        .ok_or_else(|| {
            CoreError::library(
                "bookmark_address",
                "A saved source needs a full address, starting with https://.",
            )
        })?;
    match scheme {
        "https" | "http" | "ftp" => Ok(scheme.to_ascii_uppercase()),
        _ => Err(CoreError::library(
            "bookmark_protocol",
            "Only https, http and ftp addresses can be saved; nothing is fetched from them.",
        )),
    }
}

fn text(value: &str, field: &str, max: usize) -> Result<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.chars().count() > max || trimmed.chars().any(char::is_control)
    {
        return Err(CoreError::library("bookmark_invalid", field));
    }
    Ok(trimmed.to_string())
}

impl Library {
    pub fn bookmarks(&self) -> Result<Vec<Bookmark>> {
        let connection = self.connection();
        let mut statement = connection.prepare(
            "SELECT id, label, address, protocol, access, provenance, added_ms
             FROM source_bookmarks ORDER BY added_ms, label COLLATE NOCASE",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(Bookmark {
                id: row.get(0)?,
                label: row.get(1)?,
                address: row.get(2)?,
                protocol: row.get(3)?,
                access: row.get(4)?,
                provenance: row.get(5)?,
                added_ms: row.get(6)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn add_bookmark(&self, draft: &BookmarkDraft) -> Result<Bookmark> {
        let label = text(
            &draft.label,
            "Give the source a name of up to 80 characters.",
            MAX_LABEL,
        )?;
        let address = text(
            &draft.address,
            "That address is empty or too long to be a source.",
            MAX_ADDRESS,
        )?;
        if address.chars().any(|character| character.is_whitespace()) {
            return Err(CoreError::library(
                "bookmark_address",
                "An address cannot contain spaces or line breaks.",
            ));
        }
        let protocol = protocol_of(&address)?;
        let access = if draft.access.trim().is_empty() {
            String::new()
        } else {
            text(
                &draft.access,
                "The access note is longer than 120 characters.",
                MAX_ACCESS,
            )?
        };
        let bookmark = Bookmark {
            id: new_id(),
            label,
            address,
            protocol,
            access,
            provenance: UNVERIFIED.to_string(),
            added_ms: now_ms(),
        };
        let inserted = self.connection().execute(
            "INSERT INTO source_bookmarks(id, label, address, protocol, access, provenance, added_ms)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(address) DO NOTHING",
            params![
                bookmark.id,
                bookmark.label,
                bookmark.address,
                bookmark.protocol,
                bookmark.access,
                bookmark.provenance,
                bookmark.added_ms
            ],
        )?;
        if inserted == 0 {
            return Err(CoreError::library(
                "bookmark_duplicate",
                "That address is already saved.",
            ));
        }
        Ok(bookmark)
    }

    pub fn remove_bookmark(&self, id: &str) -> Result<bool> {
        let removed = self
            .connection()
            .execute("DELETE FROM source_bookmarks WHERE id = ?1", params![id])?;
        Ok(removed > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft(label: &str, address: &str) -> BookmarkDraft {
        BookmarkDraft {
            label: label.to_string(),
            address: address.to_string(),
            access: String::new(),
        }
    }

    #[test]
    fn a_https_source_is_saved_with_its_protocol_and_unverified_provenance() {
        let library = Library::open_in_memory().unwrap();
        let saved = library
            .add_bookmark(&draft("Streaming site", "https://example.org/library"))
            .unwrap();
        assert_eq!(saved.protocol, "HTTPS");
        assert!(
            saved
                .provenance
                .contains("nothing about it has been verified")
        );

        let listed = library.bookmarks().unwrap();
        assert_eq!(listed, vec![saved]);
    }

    #[test]
    fn only_real_web_and_ftp_addresses_are_saved() {
        let library = Library::open_in_memory().unwrap();
        assert!(
            library
                .add_bookmark(&draft("FTP", "ftp://ftp.example.org/films/"))
                .is_ok()
        );
        for refused in [
            "example.org",
            "file:///etc/passwd",
            "magnet:?xt=urn:btih:00",
            "javascript:alert(1)",
            "https://example.org/a b",
        ] {
            let error = library
                .add_bookmark(&draft("Refused", refused))
                .expect_err(refused);
            assert!(
                error.code() == "bookmark_protocol" || error.code() == "bookmark_address",
                "{refused} gave {}",
                error.code()
            );
        }
        assert_eq!(library.bookmarks().unwrap().len(), 1);
    }

    #[test]
    fn the_same_address_cannot_be_saved_twice_and_everything_is_bounded() {
        let library = Library::open_in_memory().unwrap();
        library
            .add_bookmark(&draft("One", "https://example.org/a"))
            .unwrap();
        let duplicate = library
            .add_bookmark(&draft("Two", "https://example.org/a"))
            .unwrap_err();
        assert_eq!(duplicate.code(), "bookmark_duplicate");

        assert!(
            library
                .add_bookmark(&draft("", "https://example.org/b"))
                .is_err()
        );
        let long_label = "a".repeat(MAX_LABEL + 1);
        assert!(
            library
                .add_bookmark(&draft(&long_label, "https://example.org/c"))
                .is_err()
        );
        let mut spaced = draft("Spaced", "https://example.org/d");
        spaced.access = "x".repeat(MAX_ACCESS + 1);
        assert!(library.add_bookmark(&spaced).is_err());
    }

    #[test]
    fn removal_is_reported_and_only_touches_that_source() {
        let library = Library::open_in_memory().unwrap();
        let first = library
            .add_bookmark(&draft("One", "https://example.org/a"))
            .unwrap();
        let second = library
            .add_bookmark(&draft("Two", "https://example.org/b"))
            .unwrap();
        assert!(library.remove_bookmark(&first.id).unwrap());
        assert!(!library.remove_bookmark(&first.id).unwrap());
        assert!(!library.remove_bookmark("").unwrap());
        assert_eq!(library.bookmarks().unwrap(), vec![second]);
    }

    #[test]
    fn a_schema_six_library_gains_the_bookmark_table_and_keeps_its_rows() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("library.db");
        {
            let library = Library::open(&path).unwrap();
            library
                .connection()
                .execute("DROP TABLE source_bookmarks", [])
                .unwrap();
            library
                .connection()
                .execute("INSERT INTO works(id, identity_key, title, kind) VALUES('w','k','Kept','movie')", [])
                .unwrap();
            library
                .connection()
                .pragma_update(None, "user_version", 6)
                .unwrap();
        }
        let library = Library::open(&path).unwrap();
        let version: i64 = library
            .connection()
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, crate::SCHEMA_VERSION);
        let title: String = library
            .connection()
            .query_row("SELECT title FROM works WHERE id = 'w'", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(title, "Kept");
        assert!(library.bookmarks().unwrap().is_empty());
    }
}
