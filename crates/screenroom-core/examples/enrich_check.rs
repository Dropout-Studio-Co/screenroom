use std::time::{SystemTime, UNIX_EPOCH};

use screenroom_core::{CatalogueMetadataProvider, Library, ProviderConfig, TmdbProvider};

fn epoch_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or_default()
}

fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let Some(path) = arguments.first() else {
        eprintln!("usage: enrich_check <library.db> [ROUNDS]");
        std::process::exit(2);
    };
    let rounds: usize = arguments
        .get(1)
        .and_then(|value| value.parse().ok())
        .unwrap_or(8);

    let library = match Library::open(path) {
        Ok(library) => library,
        Err(error) => {
            eprintln!("cannot open {}: {}", path, error.code());
            std::process::exit(1);
        }
    };

    let provider = TmdbProvider::new(ProviderConfig::from_environment());
    let capability = provider.capability();
    println!(
        "provider: configured={} language={} detail={}",
        capability.is_configured(),
        capability.language,
        capability.detail
    );
    if !capability.is_configured() {
        std::process::exit(3);
    }

    match library.enqueue_whole_library("en-US") {
        Ok(queued) => println!("queued: {queued}"),
        Err(error) => {
            eprintln!("cannot queue the library: {}", error.code());
            std::process::exit(1);
        }
    }

    let mut succeeded = 0_u64;
    let mut failed = 0_u64;
    let mut remaining = 0_i64;
    for round in 0..rounds {
        match library.drain_enrichment_queue(&provider, 40, epoch_ms()) {
            Ok(report) => {
                succeeded += report.succeeded;
                failed += report.failed;
                remaining = report.remaining;
                println!(
                    "round {}: attempted={} succeeded={} failed={} remaining={}",
                    round + 1,
                    report.attempted,
                    report.succeeded,
                    report.failed,
                    report.remaining
                );
                if report.attempted == 0 {
                    break;
                }
            }
            Err(error) => {
                eprintln!("round {} failed: {}", round + 1, error.code());
                std::process::exit(1);
            }
        }
    }

    println!("done: succeeded={succeeded} failed={failed} remaining={remaining}");
}
