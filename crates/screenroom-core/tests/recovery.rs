use std::fs;
use std::path::Path;

use rusqlite::{Connection, params};
use screenroom_core::{
    BookmarkDraft, EnrichmentState, EnrichmentTarget, Ingest, Library, LibraryQuery, parse_filename,
};
use tempfile::TempDir;

fn raw(directory: &TempDir) -> Connection {
    Connection::open(directory.path().join("library.db")).expect("reading connection")
}

fn raw_count(directory: &TempDir, sql: &str) -> i64 {
    raw(directory)
        .query_row(sql, [], |row| row.get(0))
        .expect(sql)
}

fn raw_text(directory: &TempDir, sql: &str) -> String {
    raw(directory)
        .query_row(sql, [], |row| row.get(0))
        .expect(sql)
}

fn ingest_extra(library: &Library, media: &Path, name: &str, fingerprint: &str) -> String {
    let source = library.sources().expect("sources").remove(0);
    fs::write(media.join(name), b"not really a video").unwrap();
    let parsed = parse_filename(name, None);
    library
        .ingest_file(Ingest {
            source_id: &source.id,
            relative_path: name,
            fingerprint,
            size: 18,
            modified_ms: 1,
            scan_id: "test",
            parsed: Some(&parsed),
            expected_root: Some(&source.root_path),
        })
        .expect("ingest")
}

fn library_with_a_file(directory: &TempDir) -> (Library, String) {
    let media = directory.path().join("media");
    fs::create_dir_all(&media).unwrap();
    let library = Library::open(directory.path().join("library.db")).expect("library");
    let source = library.add_source(&media, None).expect("source");
    let name = "Heat (1995).mkv";
    fs::write(media.join(name), b"not really a video").unwrap();
    let parsed = parse_filename(name, None);
    let file_id = library
        .ingest_file(Ingest {
            source_id: &source.id,
            relative_path: name,
            fingerprint: "fingerprint",
            size: 18,
            modified_ms: 1,
            scan_id: "test",
            parsed: Some(&parsed),
            expected_root: Some(&source.root_path),
        })
        .expect("ingest");
    (library, file_id)
}

#[test]
fn a_playback_session_open_when_the_process_died_is_closed_on_reopen() {
    let directory = TempDir::new().expect("temp dir");
    let (library, file_id) = library_with_a_file(&directory);
    let profile = library.profiles().unwrap().remove(0);

    let session = library
        .start_session(&profile.id, &file_id)
        .expect("session");
    library
        .checkpoint(screenroom_core::Checkpoint {
            session: &session,
            position_ms: 12_000,
            duration_ms: 60_000,
            sequence: 1,
            seek_generation: 0,
            ended: false,
        })
        .expect("checkpoint");
    const ACTIVE: &str = "SELECT COUNT(*) FROM sessions WHERE status = 'active'";
    assert_eq!(
        raw_count(&directory, ACTIVE),
        1,
        "the session is active before the reopen"
    );

    let reopened = Library::open(directory.path().join("library.db")).expect("reopen");
    assert_eq!(
        raw_count(&directory, ACTIVE),
        0,
        "a crashed process cannot stay authoritative for a session"
    );

    let watching = reopened
        .continue_watching(&profile.id, 5)
        .expect("continue");
    assert_eq!(
        watching.len(),
        1,
        "the interrupted viewing is still offered"
    );
    assert_eq!(
        watching[0].position_ms, 12_000,
        "at the position it reached"
    );
}

#[test]
fn a_job_that_was_running_when_the_process_died_is_queued_again() {
    let directory = TempDir::new().expect("temp dir");
    let (_library, _file_id) = library_with_a_file(&directory);
    let database = directory.path().join("library.db");
    {
        let connection = Connection::open(&database).unwrap();
        connection
            .execute(
                "INSERT INTO jobs(id, kind, target, status, priority, attempts, next_retry_ms)
                 VALUES('job-1', 'probe', 'target-1', 'running', 0, 1, 0)",
                [],
            )
            .unwrap();
    }

    let _reopened = Library::open(&database).expect("reopen");
    let status: String = Connection::open(&database)
        .unwrap()
        .query_row("SELECT status FROM jobs WHERE id = 'job-1'", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(
        status, "queued",
        "interrupted work is retried, not left claiming to run"
    );
}

#[test]
fn an_unsupported_schema_is_refused_without_resetting_the_data() {
    let directory = TempDir::new().expect("temp dir");
    let database = directory.path().join("library.db");
    {
        let connection = Connection::open(&database).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE marker(text TEXT);
                 INSERT INTO marker(text) VALUES('the visitor');
                 PRAGMA user_version = 1;",
            )
            .unwrap();
    }

    let error = Library::open(&database).unwrap_err();
    assert_eq!(error.code(), "migration_unavailable");
    let survived: String = Connection::open(&database)
        .unwrap()
        .query_row("SELECT text FROM marker", [], |row| row.get(0))
        .unwrap();
    assert_eq!(
        survived, "the visitor",
        "a refused migration must leave the file exactly as it was"
    );
}

#[test]
fn a_future_schema_is_refused_and_named() {
    let directory = TempDir::new().expect("temp dir");
    let database = directory.path().join("library.db");
    {
        let connection = Connection::open(&database).unwrap();
        connection
            .execute_batch("PRAGMA user_version = 99;")
            .unwrap();
    }

    let error = Library::open(&database).unwrap_err();
    assert_eq!(error.code(), "future_database");
    assert!(
        error.to_string().contains("99"),
        "the refusal names the version it found: {error}"
    );
}

#[test]
fn a_damaged_database_is_reported_rather_than_replaced() {
    let directory = TempDir::new().expect("temp dir");
    let database = directory.path().join("library.db");

    fs::write(&database, vec![0x7f; 64 * 1024]).unwrap();
    let before = fs::read(&database).unwrap();

    let error = Library::open(&database).unwrap_err();
    assert!(
        matches!(error.code(), "corrupt_database" | "database_error"),
        "unexpected code {}",
        error.code()
    );
    assert_eq!(
        fs::read(&database).unwrap(),
        before,
        "a damaged library is never overwritten by an empty one"
    );
}

#[test]
fn a_database_with_no_schema_is_initialised_rather_than_refused() {
    let directory = TempDir::new().expect("temp dir");
    let database = directory.path().join("library.db");

    fs::write(&database, b"").unwrap();

    let library = Library::open(&database).expect("a fresh library");
    assert_eq!(library.profiles().unwrap().len(), 1);
    assert_eq!(library.items(&LibraryQuery::page(10)).unwrap().len(), 0);
    assert!(database.metadata().unwrap().len() > 0);
}

#[test]
fn progress_survives_the_whole_recovery_sequence() {
    let directory = TempDir::new().expect("temp dir");
    let (library, file_id) = library_with_a_file(&directory);
    let profile = library.profiles().unwrap().remove(0);
    library
        .set_favorite(
            &profile.id,
            &library.item(&file_id, None).unwrap().unwrap().work_id,
            true,
        )
        .expect("favorite");
    let session = library
        .start_session(&profile.id, &file_id)
        .expect("session");
    library
        .checkpoint(screenroom_core::Checkpoint {
            session: &session,
            position_ms: 30_000,
            duration_ms: 60_000,
            sequence: 1,
            seek_generation: 0,
            ended: false,
        })
        .expect("checkpoint");

    drop(session);
    drop(library);

    for _ in 0..3 {
        let reopened = Library::open(directory.path().join("library.db")).expect("reopen");
        let watching = reopened
            .continue_watching(&profile.id, 5)
            .expect("continue");
        assert_eq!(watching.len(), 1);
        assert_eq!(watching[0].position_ms, 30_000, "the position is stable");
        assert!(watching[0].favorite, "and so is the list entry");
    }
}

#[test]
fn a_backup_keeps_subtitles_and_saved_sources_across_a_restore() {
    let directory = TempDir::new().expect("temp dir");
    let (library, file_id) = library_with_a_file(&directory);
    let media = directory.path().join("media");
    let subtitle_body = b"1\n00:00:01,000 --> 00:00:02,000\nHello\n";
    let subtitle_source = directory.path().join("heat.en.srt");
    fs::write(&subtitle_source, subtitle_body).unwrap();
    let subtitle = library
        .import_subtitle(&file_id, &subtitle_source)
        .expect("subtitle");
    let bookmark = library
        .add_bookmark(&BookmarkDraft {
            label: "Streaming shelf".to_string(),
            address: "https://example.org/shelf".to_string(),
            access: String::new(),
        })
        .expect("bookmark");

    let backup = directory.path().join("copy.db");
    library.backup_to(&backup).expect("backup");

    let added = ingest_extra(&library, &media, "Alien (1979).mkv", "alien");
    let added_subtitle = directory.path().join("alien.en.srt");
    fs::write(
        &added_subtitle,
        b"1\n00:00:03,000 --> 00:00:04,000\nLater\n",
    )
    .unwrap();
    library
        .import_subtitle(&added, &added_subtitle)
        .expect("later subtitle");
    assert_eq!(library.items(&LibraryQuery::page(50)).unwrap().len(), 2);

    library.restore_from(&backup).expect(
        "a restore must not fail because the live library holds subtitles the backup does not",
    );

    assert_eq!(
        library.items(&LibraryQuery::page(50)).unwrap().len(),
        1,
        "the file added after the backup is gone"
    );
    assert_eq!(
        library
            .subtitle_bytes(&subtitle.id)
            .unwrap()
            .expect("the imported subtitle came back"),
        subtitle_body.to_vec()
    );
    assert_eq!(
        library.bookmarks().unwrap(),
        vec![bookmark],
        "a saved source survives the round trip"
    );
    assert_eq!(library.subtitles_for(&file_id).unwrap().len(), 1);
    assert!(
        library.subtitles_for(&added).unwrap().is_empty(),
        "the subtitle added after the backup is gone"
    );
}

#[test]
fn a_restore_makes_the_work_a_backup_interrupted_due_again() {
    let directory = TempDir::new().expect("temp dir");
    let (library, file_id) = library_with_a_file(&directory);
    let work_id = library
        .item(&file_id, None)
        .expect("item")
        .expect("present")
        .work_id;
    let task = library
        .enqueue_enrichment(&work_id, EnrichmentTarget::Work, "en-US", 0)
        .expect("task");
    assert!(
        library.claim_enrichment_task(&task, 0).expect("claim"),
        "the task is running when the backup is taken"
    );

    {
        let connection = Connection::open(directory.path().join("library.db")).unwrap();
        connection
            .execute(
                "INSERT INTO jobs(id, kind, target, status, priority, attempts, next_retry_ms)
                 VALUES('job-during-backup', 'probe', 'target-1', 'running', 0, 1, 0)",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO conversion_jobs(id, file_id, title, profile, status, progress,
                   message, source_path, output_path, source_size, source_modified_ms,
                   output_bytes, created_ms, updated_ms)
                 VALUES('conversion-during-backup', ?1, 'Heat', 'remux', 'verifying', 50, '',
                   '/in.mkv', '/out.mp4', 1, 1, 0, 1, 1)",
                params![file_id],
            )
            .unwrap();
    }

    let backup = directory.path().join("copy.db");
    library.backup_to(&backup).expect("backup");
    library.restore_from(&backup).expect("restore");

    let restored = library
        .enrichment_task(&task)
        .expect("task lookup")
        .expect("the task came back");
    assert_eq!(
        restored.state,
        EnrichmentState::Queued,
        "a task the backup interrupted is not left claiming to run"
    );
    let due = library
        .due_enrichment_tasks(i64::MAX, 10)
        .expect("due tasks");
    assert!(
        due.iter().any(|candidate| candidate.id == task),
        "the interrupted task is offered to the worker again"
    );
    assert_eq!(
        raw_text(
            &directory,
            "SELECT status FROM jobs WHERE id = 'job-during-backup'"
        ),
        "queued",
        "a job the backup interrupted is retried"
    );
    assert_eq!(
        raw_text(
            &directory,
            "SELECT status FROM conversion_jobs WHERE id = 'conversion-during-backup'"
        ),
        "failed",
        "a conversion nothing is working on stops claiming progress"
    );
}
