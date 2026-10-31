use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

use screenroom_player::Player;
use tempfile::TempDir;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let directory = TempDir::new()?;
    let path = match std::env::args().nth(1) {
        Some(path) => PathBuf::from(path),
        None => fixture(&directory)?,
    };
    println!("clip: {}", path.display());

    let player = Player::start()?;
    let capability = screenroom_player::capability();
    println!(
        "libmpv {} via {} ({:?} bridge)",
        capability.api_version.as_deref().unwrap_or("-"),
        capability.library_path.as_deref().unwrap_or("-"),
        screenroom_player::preferred_bridge()
    );
    player.load(&path);

    let deadline = Instant::now() + Duration::from_secs(20);
    while player.snapshot().position <= 0.3 {
        if Instant::now() >= deadline {
            return Err("playback never started".into());
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    let began = Instant::now();
    let mut previous: Option<(f64, f64)> = None;
    println!(
        "{:>7}  {:>10}  {:>10}  {:>10}  {:>12}",
        "wall", "video", "audio", "delta_ms", "audio_rate"
    );
    for _ in 0..12 {
        let snapshot = player.snapshot();
        let wall = began.elapsed().as_secs_f64();
        let video = snapshot.position;
        let Some(audio) = snapshot.audio_pts else {
            println!("{wall:>7.2}  no audio clock (file has no audio track)");
            break;
        };
        let delta_ms = (audio - video) * 1000.0;
        let rate = previous
            .filter(|(last_wall, _)| wall - last_wall > 0.2)
            .map(|(last_wall, last_audio)| (audio - last_audio) / (wall - last_wall));
        println!(
            "{wall:>7.2}  {video:>10.3}  {audio:>10.3}  {delta_ms:>+10.1}  {}",
            match rate {
                Some(rate) => format!("{rate:>11.3}x"),
                None => "          —".to_string(),
            }
        );
        previous = Some((wall, audio));
        std::thread::sleep(Duration::from_secs(1));
    }

    let tracks = player.snapshot().tracks;
    println!("\ntracks: {}", tracks.len());
    for track in &tracks {
        println!(
            "  [{}] id={} selected={} external={} — {}",
            track.kind.as_str(),
            track.id,
            track.selected,
            track.external,
            track.label()
        );
    }
    Ok(())
}

fn fixture(directory: &TempDir) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let path = directory.path().join("audio-clock.mp4");
    let status = Command::new("ffmpeg")
        .args([
            "-nostdin",
            "-y",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=640x360:rate=30:duration=20",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000:duration=20",
            "-ac",
            "2",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-preset",
            "veryfast",
            "-crf",
            "30",
            "-c:a",
            "aac",
            "-b:a",
            "128k",
        ])
        .arg(&path)
        .status()?;
    if !status.success() {
        return Err("ffmpeg could not build the fixture".into());
    }
    Ok(path)
}
