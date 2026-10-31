use std::path::PathBuf;
use std::process::Command;

use screenroom_core::{Library, LibraryQuery};
use tempfile::TempDir;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = std::env::args().skip(1);
    let path: PathBuf = arguments
        .next()
        .expect("usage: artwork_check <library.db> [fresh]")
        .into();
    let fresh = arguments.next().as_deref() == Some("fresh");
    let library = Library::open(&path)?;
    let directory = TempDir::new()?;
    let mut failures = 0_usize;

    let works = library.count(&LibraryQuery {
        group_by_work: true,
        ..LibraryQuery::default()
    })?;
    let stored = library.count(&LibraryQuery {
        group_by_work: true,
        ..LibraryQuery::default()
    })? - library.missing_artwork_count()?;
    println!(
        "works: {works}, with a poster: {stored}, without: {}",
        library.missing_artwork_count()?
    );

    let mut checked = 0_usize;
    let mut decoded = 0_usize;
    let mut total_bytes = 0_u64;
    let mut worst: Option<(String, String)> = None;
    let mut offset = 0_i64;
    loop {
        let page = library.items(&LibraryQuery {
            offset,
            limit: 50,
            group_by_work: true,
            ..LibraryQuery::default()
        })?;
        if page.is_empty() {
            break;
        }
        offset += page.len() as i64;
        let work_ids: Vec<String> = page.iter().map(|item| item.work_id.clone()).collect();
        let titles: std::collections::HashMap<String, String> = page
            .iter()
            .map(|item| (item.work_id.clone(), item.title.clone()))
            .collect();
        for artwork in library.artworks_for(&work_ids)? {
            checked += 1;
            match inspect(&artwork, &directory, &mut total_bytes) {
                Ok(()) => decoded += 1,
                Err(reason) => {
                    let title = titles
                        .get(&artwork.work_id)
                        .cloned()
                        .unwrap_or_else(|| artwork.work_id.clone());
                    if worst.is_none() {
                        worst = Some((title, reason));
                    }
                }
            }
        }
    }
    if checked != decoded {
        failures += 1;
    }
    println!(
        "{checked} stored poster(s) read back: {decoded} decoded as JPEG by ffprobe, {} rejected, {total_bytes} bytes",
        checked - decoded
    );
    if let Some((title, reason)) = worst {
        println!("  first failure on \"{title}\": {reason}");
    }

    let mut candidates = library.artwork_candidates(200)?;
    let mut missing_path = 0_usize;
    let mut zero_duration = 0_usize;
    for candidate in &candidates {
        let metadata = std::fs::metadata(&candidate.path);
        if !metadata.map(|meta| meta.is_file()).unwrap_or(false) {
            missing_path += 1;
        }
        if candidate.duration_ms <= 0 {
            zero_duration += 1;
        }
    }

    let remaining = library.missing_artwork_count()?;
    if missing_path > 0 {
        failures += 1;
    }
    println!(
        "{} candidate(s) of {remaining} work(s) still missing artwork: {missing_path} point at a missing file, {zero_duration} have no stored duration",
        candidates.len()
    );

    if fresh {
        let target = &mut candidates;
        target.truncate(6);
        let mut extracted = 0_usize;
        let mut refused = 0_usize;
        for candidate in target.iter() {
            match extract(candidate, &directory) {
                Ok((bytes, width, height)) => {
                    library.store_artwork(
                        &candidate.work_id,
                        Some(&candidate.file_id),
                        &bytes,
                        width,
                        height,
                        60_000,
                    )?;

                    let readback = library.artwork(&candidate.work_id)?;
                    let check = match readback {
                        Some(artwork) => inspect(&artwork, &directory, &mut 0),
                        None => Err("the row did not come back".to_string()),
                    };
                    match check {
                        Ok(()) => extracted += 1,
                        Err(reason) => {
                            failures += 1;
                            println!(
                                "  \"{}\" stored but did not decode: {reason}",
                                candidate.title
                            );
                        }
                    }
                }
                Err(Sampling::UnreadableMedia(reason)) => {
                    refused += 1;
                    println!(
                        "  \"{}\" has no usable picture: {reason} (the app records it and stops asking)",
                        candidate.title
                    );
                }
                Err(Sampling::Broken(reason)) => {
                    failures += 1;
                    println!("  \"{}\" could not be sampled: {reason}", candidate.title);
                }
            }
        }
        println!(
            "{extracted} poster(s) extracted, stored and decoded again; {refused} file(s) unreadable"
        );
    }

    if failures == 0 {
        println!("\nall artwork checks passed");
        Ok(())
    } else {
        println!("\n{failures} artwork check(s) failed");
        std::process::exit(1);
    }
}

fn inspect(
    artwork: &screenroom_core::Artwork,
    directory: &TempDir,
    total_bytes: &mut u64,
) -> Result<(), String> {
    if !artwork.bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Err("stored bytes are not a JPEG".into());
    }
    *total_bytes += artwork.bytes.len() as u64;
    let file = directory.path().join(format!("{}.jpg", artwork.work_id));
    std::fs::write(&file, &artwork.bytes).map_err(|error| error.to_string())?;
    let (width, height) = probe(&file)?;
    if width != artwork.width || height != artwork.height {
        return Err(format!(
            "row says {}x{} but the image is {width}x{height}",
            artwork.width, artwork.height
        ));
    }
    Ok(())
}

enum Sampling {
    UnreadableMedia(String),

    Broken(String),
}

fn extract(
    candidate: &screenroom_core::ArtworkCandidate,
    directory: &TempDir,
) -> Result<(Vec<u8>, i64, i64), Sampling> {
    let output = directory.path().join(format!("{}.jpg", candidate.work_id));

    let at = if candidate.duration_ms > 120_000 || candidate.duration_ms == 0 {
        "60"
    } else {
        "2"
    };
    let status = Command::new("ffmpeg")
        .args(["-nostdin", "-v", "error", "-y", "-ss", at, "-i"])
        .arg(&candidate.path)
        .args([
            "-frames:v",
            "1",
            "-an",
            "-sn",
            "-vf",
            "scale=400:-2",
            "-q:v",
            "4",
        ])
        .arg(&output)
        .status()
        .map_err(|error| Sampling::Broken(error.to_string()))?;
    if !status.success() {
        return Err(if readable(&candidate.path) {
            Sampling::Broken("ffmpeg refused a file ffprobe can read".into())
        } else {
            Sampling::UnreadableMedia("the file cannot be decoded at all".into())
        });
    }
    let bytes = std::fs::read(&output).map_err(|error| Sampling::Broken(error.to_string()))?;
    let (width, height) = probe(&output).map_err(Sampling::Broken)?;
    Ok((bytes, width, height))
}

fn readable(path: &std::path::Path) -> bool {
    Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "stream=codec_name",
            "-of",
            "csv=p=0",
        ])
        .arg(path)
        .output()
        .map(|output| output.status.success() && !output.stdout.is_empty())
        .unwrap_or(false)
}

fn probe(path: &std::path::Path) -> Result<(i64, i64), String> {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height,codec_name",
            "-of",
            "csv=p=0",
        ])
        .arg(path)
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err("ffprobe could not read the image".into());
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let first = text.lines().next().unwrap_or_default();
    let fields: Vec<&str> = first.trim().split(',').collect();
    if fields.len() < 3 {
        return Err(format!("unexpected ffprobe output: {first:?}"));
    }
    let codec = fields[0];
    let width: i64 = fields[1].parse().map_err(|_| "no width".to_string())?;
    let height: i64 = fields[2].parse().map_err(|_| "no height".to_string())?;
    if codec != "mjpeg" && codec != "jpeg2000" {
        return Err(format!("decoded as {codec}, not a JPEG"));
    }
    Ok((width, height))
}
