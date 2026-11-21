use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use screenroom_core::{Ingest, Library, LibraryQuery, Sort, parse_filename};
use tempfile::TempDir;

const TITLE_ORDER: &str =
    "title COLLATE NOCASE, COALESCE(season_number, 0), COALESCE(episode_number, 0), id";

const CATALOG_SELECT: &str = "SELECT f.id, f.source_id, f.edition_id, f.relative_path, f.filename,
    f.available, f.added_ms, w.id work_id, w.title, w.kind, w.year, w.overview, w.genres,
    w.manual_match, w.metadata_origin, e.duration_ms, f.season_number, f.episode_number,
    (SELECT CASE WHEN ep.metadata_origin='stale' THEN '' ELSE ep.title END
       FROM episodes ep JOIN file_episodes fe ON fe.episode_id=ep.id
       WHERE fe.file_id=f.id ORDER BY fe.sequence LIMIT 1) episode_title,
    (SELECT CASE WHEN ep.metadata_origin='stale' THEN '' ELSE ep.overview END
       FROM episodes ep JOIN file_episodes fe ON fe.episode_id=ep.id
       WHERE fe.file_id=f.id ORDER BY fe.sequence LIMIT 1) episode_overview,
    f.episode_end, COALESCE(pr.position_ms, 0) position_ms,
    COALESCE(pr.watched, 0) watched, COALESCE(pr.hidden, 0) hidden,
    COALESCE(pr.updated_ms, 0) progress_updated_ms,
    COALESCE(wl.favorite, 0) favorite, COALESCE(wl.watchlisted, 0) watchlisted
    FROM files f JOIN editions e ON e.id = f.edition_id JOIN works w ON w.id = e.work_id
    JOIN sources s ON s.id = f.source_id
    LEFT JOIN progress pr ON pr.edition_id = e.id AND pr.profile_id = ?
    LEFT JOIN watchlist wl ON wl.work_id = w.id AND wl.profile_id = ?";

fn main() {
    let mut count = 10_000usize;
    let mut runs = 3usize;
    let mut database: Option<PathBuf> = None;
    let mut explain: Option<PathBuf> = None;

    for argument in std::env::args().skip(1) {
        if let Some(value) = argument.strip_prefix("--runs=") {
            runs = value.parse().expect("runs");
        } else if let Some(value) = argument.strip_prefix("--database=") {
            database = Some(PathBuf::from(value));
        } else if let Some(value) = argument.strip_prefix("--explain=") {
            explain = Some(PathBuf::from(value));
        } else if let Ok(value) = argument.parse() {
            count = value;
        }
    }

    let scratch = TempDir::new().expect("temp dir");
    let library = match database.as_deref() {
        Some(path) => Library::open(path).expect("library"),
        None => Library::open_in_memory().expect("library"),
    };
    let source = library.add_source(scratch.path(), None).expect("source");

    let started = Instant::now();
    for index in 0..count {
        let name = format!("Title {index:05} ({}).mkv", 1950 + index % 70);
        let parsed = parse_filename(&name, None);
        library
            .ingest_file(Ingest {
                source_id: &source.id,
                relative_path: &name,
                fingerprint: &format!("fingerprint-{index}"),
                size: 1_000,
                modified_ms: 1,
                scan_id: "bench",
                parsed: Some(&parsed),
                expected_root: Some(&source.root_path),
            })
            .expect("ingest");
    }
    let ingest = started.elapsed();
    println!(
        "ingest {count} entries: {ingest:?} ({:.0} entries/s)",
        count as f64 / ingest.as_secs_f64()
    );
    println!();

    let viewer = library.profiles().expect("profiles").remove(0).id;

    let measurements: Vec<(String, LibraryQuery)> = vec![
        (
            "first page".to_string(),
            LibraryQuery {
                sort: Sort::Title,
                ..LibraryQuery::page(24)
            },
        ),
        (
            "last page".to_string(),
            LibraryQuery {
                offset: count as i64 - 24,
                sort: Sort::Title,
                ..LibraryQuery::page(24)
            },
        ),
        (
            "Added ordering".to_string(),
            LibraryQuery {
                sort: Sort::Added,
                ..LibraryQuery::page(24)
            },
        ),
        (
            "Year ordering".to_string(),
            LibraryQuery {
                sort: Sort::Year,
                ..LibraryQuery::page(24)
            },
        ),
        (
            "Progress ordering".to_string(),
            LibraryQuery {
                sort: Sort::Progress,
                ..LibraryQuery::page(24)
            },
        ),
        (
            "literal search".to_string(),
            LibraryQuery {
                text: format!("{:05}", count.saturating_sub(1)),
                ..LibraryQuery::page(24)
            },
        ),
        (
            "search with no match".to_string(),
            LibraryQuery {
                text: "no-such-title".to_string(),
                ..LibraryQuery::page(24)
            },
        ),
        ("count everything".to_string(), LibraryQuery::page(24)),
        (
            "grouped first page".to_string(),
            LibraryQuery {
                profile_id: Some(viewer.clone()),
                group_by_work: true,
                watched: Some(false),
                offset: 0,
                sort: Sort::Title,
                ..LibraryQuery::page(24)
            },
        ),
        (
            "grouped last page".to_string(),
            LibraryQuery {
                profile_id: Some(viewer.clone()),
                group_by_work: true,
                watched: Some(false),
                offset: (count as i64 - 24).max(0),
                sort: Sort::Title,
                ..LibraryQuery::page(24)
            },
        ),
    ];

    let mut slowest = Duration::ZERO;
    let mut slowest_label = String::new();
    for (label, query) in &measurements {
        let (rows, total, page, count_time, page_samples) = measure(&library, query, runs.max(1));
        println!(
            "{label:<22} {rows:>4} rows {page:>12?}   count {total:>6} {count_time:>12?}   [{}]",
            samples_ms(&page_samples)
        );
        if page > slowest {
            slowest = page;
            slowest_label = label.clone();
        }
    }

    let profile = library.profiles().expect("profiles").remove(0);
    let started = Instant::now();
    let genres = library.known_genres().expect("genres");
    let genres_time = started.elapsed();
    println!(
        "{:<22} {:>4} genres {genres_time:>12?}",
        "known genres",
        genres.len()
    );

    let started = Instant::now();
    let watching = library
        .continue_watching(&profile.id, 20)
        .expect("continue watching");
    let watching_time = started.elapsed();
    println!(
        "{:<22} {:>4} rows {watching_time:>12?}",
        "continue watching",
        watching.len()
    );

    println!();
    println!("slowest page: {slowest_label} at {slowest:?}");

    if let Some(directory) = explain.as_deref() {
        capture_plans(&library, &measurements, directory, count, &viewer);
    }
}

fn measure(
    library: &Library,
    query: &LibraryQuery,
    runs: usize,
) -> (usize, i64, Duration, Duration, Vec<Duration>) {
    let mut page_samples = Vec::with_capacity(runs);
    let mut count_samples = Vec::with_capacity(runs);
    let mut rows = 0;
    let mut total = 0;
    for _ in 0..runs {
        let started = Instant::now();
        rows = library.items(query).expect("page").len();
        page_samples.push(started.elapsed());
        let started = Instant::now();
        total = library.count(query).expect("count");
        count_samples.push(started.elapsed());
    }
    (
        rows,
        total,
        median(&page_samples),
        median(&count_samples),
        page_samples,
    )
}

fn median(samples: &[Duration]) -> Duration {
    let mut sorted = samples.to_vec();
    sorted.sort();
    sorted[sorted.len() / 2]
}

fn samples_ms(samples: &[Duration]) -> String {
    samples
        .iter()
        .map(|sample| format!("{:.2}", sample.as_secs_f64() * 1_000.0))
        .collect::<Vec<_>>()
        .join(" ")
}

fn quoted(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn plan_queries(viewer: &str, count: usize) -> Vec<(&'static str, String)> {
    let grouped_where = "s.state != 'removed' AND COALESCE(pr.watched, 0) = 0";
    let plain_where = "s.state != 'removed'";
    let viewer = quoted(viewer);
    let plain = CATALOG_SELECT.replacen('?', &viewer, 2);
    let ranked = plain.replacen(
        "SELECT ",
        "SELECT ROW_NUMBER() OVER (PARTITION BY w.id ORDER BY \
         CASE WHEN f.available=1 AND s.state='available' THEN 0 ELSE 1 END, \
         COALESCE(f.season_number,0), COALESCE(f.episode_number,0), f.id) work_rank, ",
        1,
    );
    vec![
        (
            "grouped-first",
            format!(
                "SELECT * FROM ({ranked} WHERE {grouped_where}) WHERE work_rank = 1 \
                 ORDER BY {TITLE_ORDER} LIMIT 24 OFFSET 0"
            ),
        ),
        (
            "grouped-last",
            format!(
                "SELECT * FROM ({ranked} WHERE {grouped_where}) WHERE work_rank = 1 \
                 ORDER BY {TITLE_ORDER} LIMIT 24 OFFSET {}",
                count.saturating_sub(24)
            ),
        ),
        (
            "ungrouped-first",
            format!(
                "SELECT * FROM ({plain} WHERE {plain_where}) ORDER BY {TITLE_ORDER} LIMIT 24 OFFSET 0"
            ),
        ),
        (
            "count-ungrouped",
            format!("SELECT COUNT(*) FROM ({plain} WHERE {plain_where})"),
        ),
        (
            "count-grouped",
            format!("SELECT COUNT(DISTINCT work_id) FROM ({plain} WHERE {grouped_where})"),
        ),
    ]
}

fn capture_plans(
    library: &Library,
    measurements: &[(String, LibraryQuery)],
    directory: &Path,
    count: usize,
    viewer: &str,
) {
    fs::create_dir_all(directory).expect("plan directory");
    let queries = plan_queries(viewer, count);
    for (slug, sql) in &queries {
        let flat = sql.split_whitespace().collect::<Vec<_>>().join(" ");
        fs::write(directory.join(format!("{slug}.sql")), flat).expect("plan sql");
    }
    println!();
    println!("plan queries written to {}", directory.display());

    for (slug, label) in [
        ("grouped-first", "grouped first page"),
        ("grouped-last", "grouped last page"),
        ("ungrouped-first", "first page"),
    ] {
        let Some((_, query)) = measurements.iter().find(|(name, _)| name == label) else {
            continue;
        };
        let ids = library
            .items(query)
            .expect("page")
            .into_iter()
            .map(|item| item.id)
            .collect::<Vec<_>>()
            .join(",");
        println!("IDS {slug} {ids}");
    }
}
