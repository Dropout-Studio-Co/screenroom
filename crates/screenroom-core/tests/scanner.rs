use std::fs::{self, File};
use std::path::Path;
use std::time::{Duration, SystemTime};

use screenroom_core::{Library, LibraryQuery, LibraryScanner, ScanToken, SourceState};
use tempfile::TempDir;

fn write_settled(path: &Path, contents: &[u8]) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, contents).unwrap();
    let file = File::options().write(true).open(path).unwrap();
    file.set_modified(SystemTime::now() - Duration::from_secs(3600))
        .unwrap();
}

fn scanner(library: &Library) -> LibraryScanner<'_> {
    LibraryScanner::new(library).with_limits(10_000, 32, Duration::from_secs(15))
}

#[test]
fn scans_media_and_reports_skipped_entries() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    write_settled(&root.join("Movies/Film.2020.1080p.mkv"), b"movie");
    write_settled(&root.join("Show/Season 1/Show.S01E01.mkv"), b"episode one");
    write_settled(&root.join("Show/Season 1/Show.S01E02.mkv"), b"episode two");
    write_settled(&root.join("notes.txt"), b"not media");
    write_settled(&root.join(".hidden/Secret.2020.mkv"), b"hidden");

    let library = Library::open_in_memory().unwrap();
    let source = library.add_source(root, None).unwrap();
    let token = ScanToken::new();
    let summary = scanner(&library).scan(&source.id, &token, |_| {}).unwrap();

    assert_eq!(summary.imported, 3);
    assert!(!summary.cancelled);
    assert!(summary.complete, "issues: {:?}", summary.issues);
    assert_eq!(library.count(&LibraryQuery::default()).unwrap(), 3);

    let show = LibraryQuery {
        kind: Some("series".into()),
        ..LibraryQuery::page(10)
    };
    let episodes = library.items(&show).unwrap();
    assert_eq!(episodes.len(), 2);
    assert_eq!(episodes[0].title, "Show");
    assert_eq!(episodes[0].season, Some(1));
    assert!(episodes.iter().all(|item| item.available));
}

#[test]
fn scan_ingest_accepts_the_stored_root_spelling() {
    let dir = TempDir::new().unwrap();

    let base = fs::canonicalize(dir.path()).unwrap();
    let root = base.join("media");
    fs::create_dir_all(&root).unwrap();
    write_settled(&root.join("Film.2020.mkv"), b"film");
    let db = base.join("library.db");
    let library = Library::open(&db).unwrap();
    let source = library.add_source(&root, None).unwrap();
    drop(library);

    let alternate = format!("{}//media", root.parent().unwrap().display());
    let connection = rusqlite::Connection::open(&db).unwrap();
    connection
        .execute(
            "UPDATE sources SET root_path = ?1 WHERE id = ?2",
            rusqlite::params![alternate, source.id],
        )
        .unwrap();
    drop(connection);

    let library = Library::open(&db).unwrap();
    let summary = scanner(&library)
        .scan(&source.id, &ScanToken::new(), |_| {})
        .unwrap();
    assert_eq!(summary.imported, 1, "issues: {:?}", summary.issues);
    assert!(summary.complete, "issues: {:?}", summary.issues);
}

#[cfg(unix)]
#[test]
fn non_utf8_parent_directory_is_skipped_as_invalid_name() {
    use std::os::unix::ffi::OsStrExt;

    let dir = TempDir::new().unwrap();
    let mut folder = b"bad".to_vec();
    folder.push(0xff);
    let path = dir.path().join(std::ffi::OsStr::from_bytes(&folder));
    if let Err(error) = fs::create_dir(&path) {
        eprintln!("SKIP: filesystem rejected a non-UTF-8 path component: {error}");
        return;
    }
    write_settled(&path.join("Film.2020.mkv"), b"film");
    let library = Library::open_in_memory().unwrap();
    let source = library.add_source(dir.path(), None).unwrap();
    let summary = scanner(&library)
        .scan(&source.id, &ScanToken::new(), |_| {})
        .unwrap();
    assert!(
        summary
            .issues
            .iter()
            .any(|issue| issue.code == "invalid_name")
    );
    assert_eq!(summary.imported, 0);
}

#[test]
fn apple_double_companions_are_not_media() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();

    write_settled(&root.join("Film.2020.mkv"), b"real film");
    write_settled(&root.join("._Film.2020.mkv"), b"apple double");
    write_settled(&root.join("Show/Season 1/Show.S01E01.mkv"), b"real episode");
    write_settled(
        &root.join("Show/Season 1/._Show.S01E01.mkv"),
        b"apple double",
    );

    let library = Library::open_in_memory().unwrap();
    let source = library.add_source(root, None).unwrap();
    let summary = scanner(&library)
        .scan(&source.id, &ScanToken::new(), |_| {})
        .unwrap();

    assert_eq!(summary.imported, 2, "only the two real files");
    assert!(summary.complete, "issues: {:?}", summary.issues);
    let items = library.items(&LibraryQuery::page(10)).unwrap();
    assert_eq!(items.len(), 2);
    assert!(
        items.iter().all(|item| !item.title.starts_with("._")),
        "none of {:?} is an AppleDouble companion",
        items.iter().map(|item| &item.title).collect::<Vec<_>>()
    );
}

#[test]
fn two_shows_with_one_title_and_different_years_stay_separate() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();

    write_settled(
        &root.join("The Office (2001)/Season 01/The Office - s01e01.mkv"),
        b"british",
    );
    write_settled(
        &root.join("The Office (2005)/Season 01/The Office - s01e01.mkv"),
        b"american",
    );

    let library = Library::open_in_memory().unwrap();
    let source = library.add_source(root, None).unwrap();
    scanner(&library)
        .scan(&source.id, &ScanToken::new(), |_| {})
        .unwrap();

    let episodes = library
        .items(&LibraryQuery {
            kind: Some("series".into()),
            ..LibraryQuery::page(10)
        })
        .unwrap();
    assert_eq!(episodes.len(), 2, "two episodes, one per show");
    assert_ne!(episodes[0].work_id, episodes[1].work_id, "two works");
    let mut years: Vec<_> = episodes.iter().filter_map(|item| item.year).collect();
    years.sort();
    assert_eq!(years, vec![2001, 2005]);
}

#[test]
fn young_files_and_download_sidecars_are_not_imported() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();

    fs::write(root.join("Fresh.2020.mkv"), b"growing").unwrap();

    write_settled(&root.join("Queued.2020.mkv"), b"queued");
    fs::write(root.join("Queued.2020.mkv.part"), b"partial").unwrap();

    let library = Library::open_in_memory().unwrap();
    let source = library.add_source(root, None).unwrap();
    let summary = scanner(&library)
        .scan(&source.id, &ScanToken::new(), |_| {})
        .unwrap();

    assert_eq!(summary.imported, 0);
    assert!(!summary.complete);
    let codes: Vec<_> = summary.issues.iter().map(|issue| issue.code).collect();
    assert!(codes.contains(&"still_downloading"), "codes: {codes:?}");
    assert_eq!(library.count(&LibraryQuery::default()).unwrap(), 0);
}

#[test]
fn cancellation_stops_the_walk_and_keeps_availability() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    for index in 0..5 {
        write_settled(&root.join(format!("Film.{index}.2020.mkv")), b"data");
    }
    let library = Library::open_in_memory().unwrap();
    let source = library.add_source(root, None).unwrap();
    scanner(&library)
        .scan(&source.id, &ScanToken::new(), |_| {})
        .unwrap();
    assert_eq!(library.count(&LibraryQuery::default()).unwrap(), 5);

    fs::remove_file(root.join("Film.0.2020.mkv")).unwrap();
    let token = ScanToken::new();
    token.cancel();
    let summary = scanner(&library).scan(&source.id, &token, |_| {}).unwrap();
    assert!(summary.cancelled);

    assert_eq!(library.count(&LibraryQuery::default()).unwrap(), 5);
}

#[test]
fn complete_rescan_withdraws_availability_without_losing_progress() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    write_settled(&root.join("Keep.2020.mkv"), b"keep");
    write_settled(&root.join("Gone.2020.mkv"), b"gone");

    let library = Library::open_in_memory().unwrap();
    let source = library.add_source(root, None).unwrap();
    scanner(&library)
        .scan(&source.id, &ScanToken::new(), |_| {})
        .unwrap();
    assert_eq!(library.count(&LibraryQuery::default()).unwrap(), 2);

    let profile = library.profiles().unwrap().remove(0);
    let gone = library
        .items(&LibraryQuery::page(10))
        .unwrap()
        .into_iter()
        .find(|item| item.filename == "Gone.2020.mkv")
        .unwrap();
    library
        .set_favorite(&profile.id, &gone.work_id, true)
        .unwrap();

    fs::remove_file(root.join("Gone.2020.mkv")).unwrap();
    let summary = scanner(&library)
        .scan(&source.id, &ScanToken::new(), |_| {})
        .unwrap();
    assert!(summary.complete);

    let offline = library.item(&gone.id, Some(&profile.id)).unwrap().unwrap();
    assert!(!offline.available);
    assert!(offline.favorite);

    assert_eq!(library.count(&LibraryQuery::default()).unwrap(), 2);
    let only_available = LibraryQuery {
        available_only: true,
        ..LibraryQuery::page(10)
    };
    assert_eq!(library.count(&only_available).unwrap(), 1);
}

#[test]
fn partial_scan_withdraws_only_locators_that_are_confirmed_missing() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    write_settled(&root.join("Gone.2020.mkv"), b"gone");
    write_settled(&root.join("Keep.2020.mkv"), b"keep");
    let library = Library::open_in_memory().unwrap();
    let source = library.add_source(root, None).unwrap();
    scanner(&library)
        .scan(&source.id, &ScanToken::new(), |_| {})
        .unwrap();
    let last_scan = library.source(&source.id).unwrap().last_scan_ms;
    std::thread::sleep(Duration::from_millis(2));

    fs::remove_file(root.join("Gone.2020.mkv")).unwrap();
    write_settled(&root.join("Downloading.2020.mkv"), b"partial");
    fs::write(root.join("Downloading.2020.mkv.part"), b"downloading").unwrap();
    let summary = scanner(&library)
        .scan(&source.id, &ScanToken::new(), |_| {})
        .unwrap();

    assert!(!summary.complete);
    let items = library.items(&LibraryQuery::page(10)).unwrap();
    assert!(
        !items
            .iter()
            .find(|item| item.filename.starts_with("Gone"))
            .unwrap()
            .available
    );
    assert!(
        items
            .iter()
            .find(|item| item.filename.starts_with("Keep"))
            .unwrap()
            .available
    );
    assert_eq!(library.source(&source.id).unwrap().last_scan_ms, last_scan);
}

#[test]
fn a_returned_folder_is_available_again_even_when_its_rescan_is_partial() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().join("drive");
    fs::create_dir_all(&root).unwrap();
    write_settled(&root.join("Film.2020.mkv"), b"film");
    let library = Library::open_in_memory().unwrap();
    let source = library.add_source(&root, None).unwrap();
    scanner(&library)
        .scan(&source.id, &ScanToken::new(), |_| {})
        .unwrap();

    let held = dir.path().join("held");
    fs::rename(&root, &held).unwrap();
    scanner(&library)
        .scan(&source.id, &ScanToken::new(), |_| {})
        .unwrap();
    assert_ne!(
        library.source(&source.id).unwrap().state,
        SourceState::Available
    );

    fs::rename(&held, &root).unwrap();
    write_settled(&root.join("Downloading.2020.mkv"), b"partial");
    fs::write(root.join("Downloading.2020.mkv.part"), b"downloading").unwrap();
    let summary = scanner(&library)
        .scan(&source.id, &ScanToken::new(), |_| {})
        .unwrap();
    assert!(!summary.complete);
    assert_eq!(
        library.source(&source.id).unwrap().state,
        SourceState::Available,
        "a recurring scan issue must not keep a returned drive out of autoplay"
    );
}

#[test]
fn an_unplugged_drive_is_not_an_empty_library() {
    let dir = TempDir::new().unwrap();
    let volume = dir.path().join("volume");
    let root = volume.join("drive");
    fs::create_dir_all(&root).unwrap();
    write_settled(&root.join("Film.2020.mkv"), b"film");
    write_settled(&root.join("Show.S01E01.mkv"), b"episode");

    let library = Library::open_in_memory().unwrap();
    let source = library.add_source(&root, None).unwrap();
    scanner(&library)
        .scan(&source.id, &ScanToken::new(), |_| {})
        .unwrap();
    assert_eq!(library.count(&LibraryQuery::default()).unwrap(), 2);

    let profile = library.profiles().unwrap().remove(0);
    let file = library
        .items(&LibraryQuery::page(10))
        .unwrap()
        .into_iter()
        .find(|item| item.filename == "Film.2020.mkv")
        .unwrap();
    library
        .set_favorite(&profile.id, &file.work_id, true)
        .unwrap();
    let file_id = file.id.clone();

    fs::remove_dir_all(&root).unwrap();
    let summary = scanner(&library)
        .scan(&source.id, &ScanToken::new(), |_| {})
        .unwrap();

    assert!(
        !summary.complete,
        "a scan that could not read its root must not claim to have completed"
    );
    assert_eq!(summary.imported, 0);
    assert_eq!(summary.visited, 0);
    assert_eq!(
        summary.issues.first().map(|issue| issue.code),
        Some("source_missing"),
        "a deleted folder is not an unmounted drive"
    );
    assert_eq!(
        library.source(&source.id).unwrap().state,
        SourceState::Missing
    );

    assert_eq!(library.count(&LibraryQuery::default()).unwrap(), 2);
    let offline = library.item(&file_id, Some(&profile.id)).unwrap().unwrap();
    assert!(!offline.available);
    assert!(offline.favorite, "a list entry was lost with the drive");
    let favorites = LibraryQuery {
        profile_id: Some(profile.id.clone()),
        favorites_only: true,
        ..LibraryQuery::page(10)
    };
    assert_eq!(library.count(&favorites).unwrap(), 1);

    fs::create_dir_all(&root).unwrap();
    write_settled(&root.join("Film.2020.mkv"), b"film");
    write_settled(&root.join("Show.S01E01.mkv"), b"episode");
    let again = scanner(&library)
        .scan(&source.id, &ScanToken::new(), |_| {})
        .unwrap();
    assert!(again.complete);
    assert!(
        library
            .item(&file_id, Some(&profile.id))
            .unwrap()
            .unwrap()
            .available,
        "reconnecting the drive did not restore availability"
    );
    assert_eq!(library.count(&LibraryQuery::default()).unwrap(), 2);

    fs::remove_dir_all(&volume).unwrap();
    let unplugged = scanner(&library)
        .scan(&source.id, &ScanToken::new(), |_| {})
        .unwrap();
    assert!(!unplugged.complete);
    assert_eq!(
        unplugged.issues.first().map(|issue| issue.code),
        Some("source_disconnected")
    );
    assert_eq!(
        library.source(&source.id).unwrap().state,
        SourceState::Disconnected
    );
    assert_eq!(
        library.count(&LibraryQuery::default()).unwrap(),
        2,
        "an unmounted volume withdrew rows"
    );
    assert!(
        library
            .item(&file_id, Some(&profile.id))
            .unwrap()
            .unwrap()
            .favorite,
        "an unmounted volume lost a list entry"
    );
}

#[test]
fn a_root_behind_a_withdrawn_grant_is_not_a_missing_drive() {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let dir = TempDir::new().unwrap();
        let locked = dir.path().join("locked");
        let root = locked.join("media");
        fs::create_dir_all(&root).unwrap();
        write_settled(&root.join("Film.2020.mkv"), b"film");

        let library = Library::open_in_memory().unwrap();
        let source = library.add_source(&root, None).unwrap();
        scanner(&library)
            .scan(&source.id, &ScanToken::new(), |_| {})
            .unwrap();

        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
        let denied = fs::read_dir(&locked).is_err();
        let outcome = denied.then(|| {
            scanner(&library)
                .scan(&source.id, &ScanToken::new(), |_| {})
                .unwrap()
        });

        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();

        if let Some(summary) = outcome {
            assert_eq!(
                summary.issues.first().map(|issue| issue.code),
                Some("permission_lost")
            );
            assert_eq!(
                library.source(&source.id).unwrap().state,
                SourceState::PermissionLost
            );
            assert_eq!(
                library.count(&LibraryQuery::default()).unwrap(),
                1,
                "a withdrawn grant withdrew rows"
            );
        }
    }
}

#[test]
fn a_root_replaced_by_a_symlink_is_refused_rather_than_followed() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().join("root");
    fs::create_dir_all(&root).unwrap();
    write_settled(&root.join("Inside.2020.mkv"), b"inside");

    let library = Library::open_in_memory().unwrap();
    let source = library.add_source(&root, None).unwrap();
    scanner(&library)
        .scan(&source.id, &ScanToken::new(), |_| {})
        .unwrap();

    let elsewhere = dir.path().join("elsewhere");
    fs::create_dir_all(&elsewhere).unwrap();
    write_settled(&elsewhere.join("Outside.2020.mkv"), b"outside");
    fs::remove_dir_all(&root).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&elsewhere, &root).unwrap();

    #[cfg(unix)]
    {
        let error = scanner(&library)
            .scan(&source.id, &ScanToken::new(), |_| {})
            .unwrap_err();
        assert_eq!(error.code(), "unsafe_path");

        assert_eq!(library.count(&LibraryQuery::default()).unwrap(), 1);
        assert!(
            !library
                .items(&LibraryQuery::page(10))
                .unwrap()
                .iter()
                .any(|item| item.filename == "Outside.2020.mkv"),
            "media outside the authorized root was read"
        );
    }
}

#[test]
fn symlinks_are_never_followed() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().join("root");
    fs::create_dir_all(&root).unwrap();
    let outside = dir.path().join("outside");
    fs::create_dir_all(&outside).unwrap();
    write_settled(&outside.join("Elsewhere.2020.mkv"), b"elsewhere");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, root.join("link")).unwrap();

    let library = Library::open_in_memory().unwrap();
    let source = library.add_source(&root, None).unwrap();
    let summary = scanner(&library)
        .scan(&source.id, &ScanToken::new(), |_| {})
        .unwrap();

    assert_eq!(summary.imported, 0);
    assert_eq!(library.count(&LibraryQuery::default()).unwrap(), 0);
}

#[test]
fn a_source_cannot_be_scanned_twice_at_once() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    write_settled(&root.join("Film.2020.mkv"), b"data");
    let library = Library::open_in_memory().unwrap();
    let source = library.add_source(root, None).unwrap();

    let mut nested_error = None;
    scanner(&library)
        .scan(&source.id, &ScanToken::new(), |update| {
            if !update.done {
                nested_error = Some(
                    LibraryScanner::new(&library)
                        .scan(&source.id, &ScanToken::new(), |_| {})
                        .unwrap_err(),
                );
            }
        })
        .unwrap();
    assert_eq!(
        nested_error.map(|error| error.code().to_string()),
        Some("scan_running".into())
    );

    assert!(
        scanner(&library)
            .scan(&source.id, &ScanToken::new(), |_| {})
            .is_ok()
    );
}

#[test]
fn progress_streams_while_scanning() {
    let dir = TempDir::new().unwrap();
    let root = dir.path();
    for index in 0..3 {
        write_settled(&root.join(format!("Film.{index}.2020.mkv")), b"data");
    }
    let library = Library::open_in_memory().unwrap();
    let source = library.add_source(root, None).unwrap();
    let mut updates = 0;
    let summary = scanner(&library)
        .scan(&source.id, &ScanToken::new(), |update| {
            if !update.done {
                updates += 1;
            }
        })
        .unwrap();
    assert_eq!(summary.imported, 3);
    assert_eq!(updates, 3);
}

#[test]
fn a_stale_empty_mount_point_does_not_empty_the_library() {
    let dir = TempDir::new().unwrap();
    let base = fs::canonicalize(dir.path()).unwrap();
    let root = base.join("drive");
    fs::create_dir_all(&root).unwrap();
    write_settled(&root.join("Film.2020.mkv"), b"film");
    write_settled(&root.join("Show.S01E01.mkv"), b"episode");
    let database = base.join("library.db");

    let source_id = {
        let library = Library::open(&database).unwrap();
        let source = library.add_source(&root, None).unwrap();
        scanner(&library)
            .scan(&source.id, &ScanToken::new(), |_| {})
            .unwrap();
        assert_eq!(library.count(&LibraryQuery::default()).unwrap(), 2);
        source.id
    };

    fs::remove_file(root.join("Show.S01E01.mkv")).unwrap();
    let present = Library::open(&database).unwrap();
    let withdrawal = scanner(&present)
        .scan(&source_id, &ScanToken::new(), |_| {})
        .unwrap();
    assert!(withdrawal.complete, "issues: {:?}", withdrawal.issues);
    let gone = present
        .items(&LibraryQuery::page(10))
        .unwrap()
        .into_iter()
        .find(|item| item.filename == "Show.S01E01.mkv")
        .unwrap();
    assert!(
        !gone.available,
        "a confirmed deletion on the registered volume was not withdrawn"
    );
    drop(present);

    {
        let connection = rusqlite::Connection::open(&database).unwrap();
        connection
            .execute(
                "UPDATE sources SET volume_identity = ?1 WHERE id = ?2",
                rusqlite::params![
                    r#"{"device_id":18446744073709551614,"volume_id":"the-drive-that-left"}"#,
                    source_id
                ],
            )
            .unwrap();
    }
    fs::remove_file(root.join("Film.2020.mkv")).unwrap();

    let library = Library::open(&database).unwrap();
    let stored = library
        .source_volume_identity(&source_id)
        .unwrap()
        .expect("the test writes a decodable identity");
    assert_eq!(stored.device_id, u64::MAX - 1);
    assert_eq!(stored.volume_id.as_deref(), Some("the-drive-that-left"));

    let stale = scanner(&library)
        .scan(&source_id, &ScanToken::new(), |_| {})
        .unwrap();

    assert!(
        stale
            .issues
            .iter()
            .any(|issue| issue.code == "volume_mismatch"),
        "a walk of a different volume claimed to be the source: {:?}",
        stale.issues
    );
    assert!(!stale.complete);
    let items = library.items(&LibraryQuery::page(10)).unwrap();
    assert_eq!(items.len(), 2, "rows were removed");
    let film = items
        .iter()
        .find(|item| item.filename == "Film.2020.mkv")
        .unwrap();
    assert!(
        film.available,
        "an empty walk of a stale mount point withdrew the library"
    );
    assert_eq!(
        items.iter().filter(|item| item.available).count(),
        1,
        "the withdrawal applied to the stale mount point instead of the deletion"
    );
    assert_eq!(
        library.source(&source_id).unwrap().state,
        SourceState::Disconnected
    );
}

#[test]
fn scan_paths_imports_only_the_named_files() {
    let dir = TempDir::new().unwrap();
    let root = fs::canonicalize(dir.path()).unwrap();
    write_settled(&root.join("Known.2020.mkv"), b"known");
    let library = Library::open_in_memory().unwrap();
    let source = library.add_source(&root, None).unwrap();
    let mut scanner = scanner(&library);
    scanner.scan(&source.id, &ScanToken::new(), |_| {}).unwrap();
    assert_eq!(library.count(&LibraryQuery::default()).unwrap(), 1);

    write_settled(&root.join("Show/Season 1/Show.S01E01.mkv"), b"episode");
    let arriving = root.join("Show/Season 1/Show.S01E01.mkv");
    let last_full_scan = library.source(&source.id).unwrap().last_scan_ms;
    let summary = scanner
        .scan_paths(
            &root,
            std::slice::from_ref(&arriving),
            &ScanToken::new(),
            |_| {},
        )
        .unwrap();

    assert_eq!(summary.visited, 1);
    assert_eq!(summary.imported, 1);
    assert!(
        !summary.complete,
        "a targeted pass never claims completeness"
    );
    assert_eq!(library.count(&LibraryQuery::default()).unwrap(), 2);
    assert_eq!(
        library.source(&source.id).unwrap().last_scan_ms,
        last_full_scan,
        "a targeted pass is not a full scan"
    );
}

#[test]
fn scan_paths_skips_a_file_still_being_written() {
    let dir = TempDir::new().unwrap();
    let root = fs::canonicalize(dir.path()).unwrap();
    let library = Library::open_in_memory().unwrap();
    let source = library.add_source(&root, None).unwrap();
    fs::write(root.join("Fresh.2020.mkv"), b"growing").unwrap();

    let summary = scanner(&library)
        .scan_paths(
            &root,
            &[root.join("Fresh.2020.mkv")],
            &ScanToken::new(),
            |_| {},
        )
        .unwrap();

    assert_eq!(summary.imported, 0);
    assert!(
        summary
            .issues
            .iter()
            .any(|issue| issue.code == "still_downloading")
    );
    assert_eq!(library.count(&LibraryQuery::default()).unwrap(), 0);
    let _ = source;
}

#[test]
fn scan_paths_refuses_a_path_outside_the_root() {
    let dir = TempDir::new().unwrap();
    let base = fs::canonicalize(dir.path()).unwrap();
    let root = base.join("media");
    fs::create_dir_all(&root).unwrap();
    let elsewhere = base.join("elsewhere");
    fs::create_dir_all(&elsewhere).unwrap();
    write_settled(&elsewhere.join("Outside.2020.mkv"), b"outside");
    let library = Library::open_in_memory().unwrap();
    let source = library.add_source(&root, None).unwrap();

    let summary = scanner(&library)
        .scan_paths(
            &root,
            &[
                elsewhere.join("Outside.2020.mkv"),
                std::path::PathBuf::from("../escape.mkv"),
            ],
            &ScanToken::new(),
            |_| {},
        )
        .unwrap();

    assert_eq!(summary.imported, 0);
    let codes: Vec<_> = summary.issues.iter().map(|issue| issue.code).collect();
    assert_eq!(
        codes.iter().filter(|code| **code == "outside_root").count(),
        2,
        "codes: {codes:?}"
    );
    assert_eq!(library.count(&LibraryQuery::default()).unwrap(), 0);
    let _ = source;
}

#[test]
fn scan_paths_refuses_a_file_reached_through_a_symlinked_directory() {
    let dir = TempDir::new().unwrap();
    let base = fs::canonicalize(dir.path()).unwrap();
    let root = base.join("media");
    fs::create_dir_all(&root).unwrap();
    let outside = base.join("outside");
    fs::create_dir_all(&outside).unwrap();
    write_settled(&outside.join("Linked.2020.mkv"), b"secret");

    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, root.join("shortcut")).unwrap();
    #[cfg(not(unix))]
    return;

    let library = Library::open_in_memory().unwrap();
    library.add_source(&root, None).unwrap();

    let summary = scanner(&library)
        .scan_paths(
            &root,
            &[root.join("shortcut").join("Linked.2020.mkv")],
            &ScanToken::new(),
            |_| {},
        )
        .unwrap();

    assert_eq!(
        summary.imported, 0,
        "a file reached through a symlinked folder must not be read"
    );
    let codes: Vec<_> = summary.issues.iter().map(|issue| issue.code).collect();
    assert!(
        codes.contains(&"unsafe_path"),
        "codes: {codes:?}"
    );
    assert_eq!(library.count(&LibraryQuery::default()).unwrap(), 0);
}

#[test]
fn scan_paths_honours_cancellation() {
    let dir = TempDir::new().unwrap();
    let root = fs::canonicalize(dir.path()).unwrap();
    write_settled(&root.join("Film.2020.mkv"), b"film");
    let library = Library::open_in_memory().unwrap();
    let source = library.add_source(&root, None).unwrap();
    let token = ScanToken::new();
    token.cancel();

    let summary = scanner(&library)
        .scan_paths(&root, &[root.join("Film.2020.mkv")], &token, |_| {})
        .unwrap();

    assert!(summary.cancelled);
    assert_eq!(summary.imported, 0);
    assert_eq!(library.count(&LibraryQuery::default()).unwrap(), 0);
    let _ = source;
}
