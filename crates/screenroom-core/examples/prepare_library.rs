use std::path::{Path, PathBuf};
use std::time::Duration;

use screenroom_core::{Library, LibraryQuery, LibraryScanner, ScanToken, Sort};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let (database, root, label) = match arguments.as_slice() {
        [database, root] => (database.clone(), root.clone(), None),
        [database, root, label] => (database.clone(), root.clone(), Some(label.clone())),
        _ => {
            eprintln!(
                "usage: prepare_library <library.db> <media-root> [source-label]\n\
                 \n\
                 Grants <media-root> to the library at <library.db> and scans it."
            );
            std::process::exit(2);
        }
    };

    let root = PathBuf::from(&root);
    if !root.is_dir() {
        return Err(format!("{} is not a readable folder", root.display()).into());
    }

    let library = Library::open(&database)?;
    let source = ensure_source(&library, &root, label.as_deref())?;
    println!("database: {}", database);
    println!("source:   {} ({})", source.label, source.root_path);

    let scanner = LibraryScanner::new(&library).with_limits(250_000, 64, Duration::ZERO);
    let token = ScanToken::new();
    let summary = scanner.scan(&source.id, &token, |_| {})?;

    println!(
        "scan:     visited={} imported={} complete={} cancelled={} issues={}",
        summary.visited,
        summary.imported,
        summary.complete,
        summary.cancelled,
        summary.issues.len()
    );
    for issue in summary.issues.iter().take(10) {
        println!("  issue: {} {}", issue.relative_path, issue.code);
    }

    let profile_id = library
        .profiles()?
        .into_iter()
        .next()
        .map(|profile| profile.id)
        .unwrap_or_default();
    let items = library.items(&LibraryQuery {
        profile_id: Some(profile_id),
        sort: Sort::Title,
        ..LibraryQuery::page(500)
    })?;
    println!("library:  {} item(s)", items.len());
    for item in &items {
        let episode = item.episode_label();
        let episode = if episode.is_empty() {
            String::new()
        } else {
            format!(" · {episode}")
        };
        println!(
            "  [{}] {}{} — {} ms resolve, available={}",
            item.kind.as_str(),
            item.title,
            episode,
            item.duration_ms,
            item.available
        );
    }
    if items.iter().any(|item| item.duration_ms == 0) {
        println!(
            "note: at least one file has no stored duration; \
             the player reports the real duration once it opens the file."
        );
    }
    Ok(())
}

fn ensure_source(
    library: &Library,
    root: &Path,
    label: Option<&str>,
) -> Result<screenroom_core::Source, Box<dyn std::error::Error>> {
    let canonical = std::fs::canonicalize(root)?;
    let canonical = canonical.to_string_lossy().to_string();
    for source in library.sources()? {
        if source.root_path == canonical {
            return Ok(source);
        }
    }
    Ok(library.add_source(root, label)?)
}
