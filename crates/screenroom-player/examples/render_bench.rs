use std::collections::HashSet;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

use screenroom_player::Player;
use tempfile::TempDir;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = std::env::args().skip(1);
    let size = arguments.next().unwrap_or_else(|| "1920x1080".to_string());
    let seconds: f64 = arguments
        .next()
        .and_then(|value| value.parse().ok())
        .unwrap_or(12.0);
    let (width, height) = parse_size(&size)?;

    let directory = TempDir::new()?;
    let path = fixture(&directory, width, height, seconds)?;
    println!(
        "fixture:  {size}, {seconds}s, {}",
        human_bytes(file_size(&path)?)
    );

    let capability = screenroom_player::capability();
    if !capability.available {
        println!("libmpv is unavailable: {}", capability.reason);
        println!("no measurement was taken.");
        return Ok(());
    }
    println!(
        "libmpv:   {} via {} ({:?} bridge)",
        capability.api_version.as_deref().unwrap_or("-"),
        capability.library_path.as_deref().unwrap_or("-"),
        screenroom_player::preferred_bridge()
    );

    let player = Player::start()?;

    let started = Instant::now();
    player.load(&path);
    let mut first_frame = None;
    let mut first_after_load = None;
    let deadline = started + Duration::from_secs(30);
    while Instant::now() < deadline {
        let snapshot = player.snapshot();
        if let Some(frame) = snapshot.frame.as_ref()
            && snapshot.loaded
            && !frame.is_blank()
        {
            first_after_load = Some(started.elapsed());
            first_frame = Some(frame.generation);
            break;
        }
        if snapshot.failure.is_some() {
            println!("load failed: {:?}", snapshot.failure);
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    let Some(first_after_load) = first_after_load else {
        println!("no frame arrived within 30s; no measurement was taken.");
        return Ok(());
    };
    println!();
    println!("time to first frame: {first_after_load:?}");

    let sample_for = Duration::from_secs_f64((seconds - 1.0).max(3.0));
    let sampling_started = Instant::now();
    let mut seen: HashSet<u64> = HashSet::new();
    if let Some(generation) = first_frame {
        seen.insert(generation);
    }
    let mut last_generation = first_frame;
    let mut last_change = Instant::now();
    let mut worst_gap = Duration::ZERO;

    while sampling_started.elapsed() < sample_for {
        let snapshot = player.snapshot();
        if let Some(frame) = snapshot.frame.as_ref()
            && seen.insert(frame.generation)
        {
            if last_generation.is_some() {
                worst_gap = worst_gap.max(last_change.elapsed());
            }
            last_change = Instant::now();
            last_generation = Some(frame.generation);
        }
        if snapshot.ended {
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    let elapsed = sampling_started.elapsed();
    let delivered = seen.len().saturating_sub(1);
    let fps = delivered as f64 / elapsed.as_secs_f64();
    let after = player.snapshot();
    println!("delivered:           {delivered} frames in {elapsed:?} ({fps:.1} fps)");
    println!(
        "frame size:          {}x{}",
        after.frame.as_ref().map_or(0, |f| f.width),
        after.frame.as_ref().map_or(0, |f| f.height)
    );
    println!("worst frame gap:     {worst_gap:?}");
    println!(
        "playhead after:      {:.2}s of {:.2}s",
        after.position, after.duration
    );
    if let Some(mistimed) = after.mistimed_frames {
        println!("mistimed frames:     {mistimed}");
    }
    if let Some(avsync) = after.avsync {
        println!("last avsync:         {avsync:+.4}s");
    }

    println!();
    println!("command latency (seek, round trip to the playhead arriving):");
    for target in [2.0_f64, 5.0, 1.0] {
        let before = player.snapshot().position;
        let issued = Instant::now();
        player.seek(target);
        let deadline = issued + Duration::from_secs(10);
        let mut observed = None;
        while Instant::now() < deadline {
            let position = player.snapshot().position;

            if (position - target).abs() < 0.5 && (before - position).abs() > 0.5 {
                observed = Some(issued.elapsed());
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        match observed {
            Some(latency) => println!("  seek {before:>5.1}s → {target:>4.1}s   {latency:?}"),
            None => println!(
                "  seek {before:>5.1}s → {target:>4.1}s   playhead never arrived at the target"
            ),
        }
    }

    println!();
    println!(
        "Reminder: these numbers are the offscreen ceiling. On-screen presentation \
         through iced and wgpu adds cost this example cannot see."
    );
    Ok(())
}

fn parse_size(value: &str) -> Result<(u32, u32), Box<dyn std::error::Error>> {
    let (width, height) = value
        .split_once(['x', 'X'])
        .ok_or_else(|| format!("expected WIDTHxHEIGHT, got {value}"))?;
    let width: u32 = width.parse()?;
    let height: u32 = height.parse()?;
    if width == 0 || height == 0 {
        return Err("dimensions must be positive".into());
    }
    Ok((width, height))
}

fn fixture(
    directory: &TempDir,
    width: u32,
    height: u32,
    seconds: f64,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let path = directory.path().join("bench.mp4");
    let status = Command::new("ffmpeg")
        .args([
            "-nostdin",
            "-y",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            &format!("testsrc2=size={width}x{height}:rate=24:duration={seconds:.3}"),
            "-f",
            "lavfi",
            "-i",
            &format!("sine=frequency=440:sample_rate=48000:duration={seconds:.3}"),
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-shortest",
        ])
        .arg(&path)
        .status()?;
    if !status.success() {
        return Err("ffmpeg could not build the fixture".into());
    }
    Ok(path)
}

fn file_size(path: &PathBuf) -> Result<u64, Box<dyn std::error::Error>> {
    Ok(std::fs::metadata(path)?.len())
}

fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KiB", "MiB", "GiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}
