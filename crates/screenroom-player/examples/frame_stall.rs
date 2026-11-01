use std::path::PathBuf;
use std::time::{Duration, Instant};

use screenroom_player::{HwdecMode, Player};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = std::env::args().skip(1);
    let mode = match arguments.next().as_deref() {
        Some("no") => HwdecMode::No,
        Some("videotoolbox-copy") => HwdecMode::VideoToolboxCopy,
        Some("auto-copy") => HwdecMode::AutoCopy,
        other => {
            return Err(format!(
                "usage: frame_stall <no|videotoolbox-copy|auto-copy> <media-path> [seconds] [threshold-ms]; got {other:?}"
            )
            .into());
        }
    };
    let media = PathBuf::from(
        arguments
            .next()
            .ok_or("usage: frame_stall <mode> <media-path> [seconds] [threshold-ms]")?,
    );
    let seconds: f64 = arguments
        .next()
        .and_then(|value| value.parse().ok())
        .unwrap_or(20.0);
    let threshold_ms: f64 = arguments
        .next()
        .and_then(|value| value.parse().ok())
        .unwrap_or(40.0);
    if !media.is_absolute() || !media.is_file() {
        return Err("media path must be an existing absolute file".into());
    }
    if seconds < 5.0 {
        return Err("sample at least 5 seconds".into());
    }

    let capability = screenroom_player::capability();
    if !capability.available {
        return Err(format!("libmpv unavailable: {}", capability.reason).into());
    }
    let player = Player::start_with_hwdec(mode)?;
    player.set_muted(true);

    let began = Instant::now();
    player.load(&media);
    let deadline = began + Duration::from_secs(90);
    let first = loop {
        let snapshot = player.snapshot();
        if let Some(failure) = snapshot.failure.clone() {
            return Err(format!("playback failed: {failure:?}").into());
        }
        if snapshot.loaded
            && let Some(frame) = snapshot.frame.clone()
            && !frame.is_blank()
        {
            break (frame, began.elapsed());
        }
        if Instant::now() >= deadline {
            return Err("no frame arrived within 90 seconds".into());
        }
        std::thread::sleep(Duration::from_millis(1));
    };

    println!(
        "media:    {} ({})",
        media.display(),
        if mode_is_hardware(mode) {
            "hardware"
        } else {
            "software"
        }
    );
    println!(
        "first    frame at {:.0} ms, {}x{}",
        first.1.as_secs_f64() * 1_000.0,
        first.0.width,
        first.0.height
    );

    let sampling_started = Instant::now();
    let mut last_generation = first.0.generation;
    let mut last_change = Instant::now();
    let mut gaps = Vec::<(f64, f64, f64, Option<f64>, Option<i64>, u32, u32)>::new();
    let mut frames = 0_u64;
    let mut previous_dimensions = (first.0.width, first.0.height);

    while sampling_started.elapsed() < Duration::from_secs_f64(seconds) {
        let snapshot = player.snapshot();
        if let Some(frame) = snapshot.frame.as_ref()
            && frame.generation != last_generation
        {
            let gap = last_change.elapsed().as_secs_f64() * 1_000.0;
            let dimensions = (frame.width, frame.height);
            if gap >= threshold_ms || dimensions != previous_dimensions {
                gaps.push((
                    sampling_started.elapsed().as_secs_f64() * 1_000.0,
                    gap,
                    snapshot.position,
                    snapshot.avsync,
                    snapshot.mistimed_frames,
                    dimensions.0,
                    dimensions.1,
                ));
            }
            previous_dimensions = dimensions;
            last_generation = frame.generation;
            last_change = Instant::now();
            frames = frames.saturating_add(1);
        }
        if snapshot.failure.is_some() {
            return Err(format!("playback failed: {:?}", snapshot.failure).into());
        }
        if snapshot.ended {
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }

    let elapsed = sampling_started.elapsed().as_secs_f64();
    let fps = frames as f64 / elapsed;
    println!("delivered: {frames} frames in {elapsed:.2} s ({fps:.2} fps)");
    println!();
    println!("offset_ms,gap_ms,playhead_s,avsync_s,mistimed,width,height");
    for (at, gap, position, avsync, mistimed, width, height) in &gaps {
        println!(
            "{at:.0},{gap:.1},{position:.3},{},{},{width},{height}",
            avsync
                .map(|value| format!("{value:+.4}"))
                .unwrap_or_default(),
            mistimed.map(|value| value.to_string()).unwrap_or_default()
        );
    }
    if gaps.is_empty() {
        println!("no gap reached {threshold_ms:.0} ms");
    }
    player.stop();
    Ok(())
}

fn mode_is_hardware(mode: HwdecMode) -> bool {
    !matches!(mode, HwdecMode::No)
}
