use std::sync::Arc;
use std::time::{Duration, Instant};

use screenroom_core::Library;
use screenroom_desktop::artwork::{ArtworkEvent, Extractor};

const FILL_INTERVAL: Duration = Duration::from_millis(5);

const BUDGET: Duration = Duration::from_secs(600);

fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let Some(path) = arguments.first() else {
        eprintln!("usage: artwork_bench <library.db> [COUNT]");
        std::process::exit(2);
    };
    let count: usize = arguments
        .get(1)
        .and_then(|value| value.parse().ok())
        .unwrap_or(30);

    let library = Arc::new(Library::open(path).expect("open library"));
    let mut pending = library
        .artwork_candidates(count as i64)
        .expect("candidates");
    let planned = pending.len().min(count);
    pending.truncate(planned);
    println!("planned: {planned} poster(s) from {path}");

    let extractor = Extractor::spawn(Arc::clone(&library));
    let started = Instant::now();
    let (mut stored, mut failed, mut first_at_ms) = (0_usize, 0_usize, None);

    loop {
        while let Some(candidate) = pending.first().cloned() {
            if !extractor.request(candidate) {
                break;
            }
            pending.remove(0);
        }

        for event in extractor.poll() {
            match event {
                ArtworkEvent::Stored { .. } => stored += 1,
                ArtworkEvent::Failed { work_id, reason } => {
                    failed += 1;
                    eprintln!("failed {work_id}: {reason}");
                }
            }
            let elapsed = started.elapsed();
            if first_at_ms.is_none() {
                first_at_ms = Some(elapsed.as_millis());
                println!("first poster at {:.2?}", elapsed);
            }
            println!(
                "  #{:<3} {:.2?} (first at {:.2?})",
                stored + failed,
                elapsed,
                Duration::from_millis(first_at_ms.unwrap_or_default() as u64)
            );
        }

        if stored + failed >= planned {
            break;
        }
        if started.elapsed() >= BUDGET {
            eprintln!("budget exhausted after {stored} stored, {failed} failed");
            std::process::exit(1);
        }
        std::thread::sleep(FILL_INTERVAL);
    }

    println!(
        "done: {stored} stored, {failed} failed in {:.2?} ({} per minute)",
        started.elapsed(),
        (stored as f64 / started.elapsed().as_secs_f64() * 60.0).round()
    );
}
