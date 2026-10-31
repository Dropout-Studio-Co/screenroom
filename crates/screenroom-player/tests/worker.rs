use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use screenroom_player::{
    HwdecMode, NO_TRACK, Player, PlayerSnapshot, PlayerState, SUB_POSITION_RANGE, SUB_SCALE_RANGE,
    TrackKind,
};
use tempfile::TempDir;

fn clip(directory: &TempDir) -> PathBuf {
    let path = directory.path().join("clip.mp4");
    let status = Command::new("ffmpeg")
        .args([
            "-nostdin",
            "-y",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=320x240:rate=25:duration=2",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=2",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-shortest",
        ])
        .arg(&path)
        .status()
        .expect("ffmpeg should be installed");
    assert!(status.success(), "ffmpeg could not build the test clip");
    path
}

fn software_only_clip(directory: &TempDir) -> Option<PathBuf> {
    let path = directory.path().join("software-only.mpg");
    let status = Command::new("ffmpeg")
        .args([
            "-nostdin",
            "-y",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=320x240:rate=25:duration=3",
            "-c:v",
            "mpeg2video",
            "-q:v",
            "4",
        ])
        .arg(&path)
        .status()
        .ok()?;
    status.success().then_some(path)
}

fn player() -> Option<Player> {
    match Player::start() {
        Ok(player) => Some(player),
        Err(error) => {
            eprintln!("skipped: {error}");
            None
        }
    }
}

fn wait_for(
    player: &Player,
    timeout: Duration,
    mut done: impl FnMut(&PlayerSnapshot) -> bool,
) -> PlayerSnapshot {
    let deadline = Instant::now() + timeout;
    loop {
        let snapshot = player.snapshot();
        if done(&snapshot) {
            return snapshot;
        }
        if Instant::now() >= deadline {
            panic!("timed out waiting for the player; last snapshot: {snapshot:?}");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn plays_a_real_file_and_presents_frames() {
    let directory = TempDir::new().expect("temp dir");
    let path = clip(&directory);
    let Some(player) = player() else { return };

    player.load(&path);

    let loaded = wait_for(&player, Duration::from_secs(20), |snapshot| {
        snapshot.loaded && snapshot.frame.is_some()
    });
    assert!(
        matches!(loaded.state, PlayerState::Playing | PlayerState::Paused),
        "unexpected state {:?}",
        loaded.state
    );
    assert!(
        (loaded.duration - 2.0).abs() < 0.3,
        "duration was {}",
        loaded.duration
    );

    let frame = loaded.frame.expect("a frame");
    assert!(frame.is_well_formed(), "frame buffer is malformed");
    assert_eq!((frame.width, frame.height), (320, 240));
    assert!(!frame.is_blank(), "the presented frame was empty");

    let coloured = (0..frame.height)
        .step_by(17)
        .flat_map(|y| (0..frame.width).step_by(17).map(move |x| (x, y)))
        .filter_map(|(x, y)| frame.pixel(x, y))
        .any(|[r, g, b, _]| r > 8 || g > 8 || b > 8);
    assert!(coloured, "every sampled pixel was black");

    let advanced = wait_for(&player, Duration::from_secs(10), |snapshot| {
        snapshot.position > 0.3
    });
    assert!(advanced.position > 0.3);
}

#[test]
fn default_decode_policy_still_delivers_generated_frames() {
    let directory = TempDir::new().expect("temp dir");
    let path = clip(&directory);
    let Some(player) = player() else { return };

    player.load(&path);
    let loaded = wait_for(&player, Duration::from_secs(20), |snapshot| {
        snapshot.loaded && snapshot.frame.is_some()
    });
    let frame = loaded.frame.expect("default policy must deliver a frame");
    assert!(frame.is_well_formed());
    assert!(!frame.is_blank(), "default policy returned a blank frame");
    assert_eq!((frame.width, frame.height), (320, 240));
    assert!(
        loaded
            .hardware_decoder
            .as_deref()
            .is_some_and(|decoder| { decoder == "no" || decoder.ends_with("-copy") }),
        "libmpv did not report hwdec-current: {:?}",
        loaded.hardware_decoder
    );
    player.stop();
    let stopped = wait_for(&player, Duration::from_secs(10), |snapshot| {
        !snapshot.loaded && snapshot.hardware_decoder.is_none()
    });
    assert!(stopped.hardware_decoder.is_none());
}

#[test]
fn auto_copy_reports_software_for_a_codec_outside_the_hwdec_whitelist() {
    if !screenroom_player::capability().available {
        eprintln!("SKIPPED: libmpv unavailable");
        return;
    }
    let directory = TempDir::new().expect("temp dir");
    let Some(path) = software_only_clip(&directory) else {
        eprintln!("SKIPPED: FFmpeg has no usable MPEG-2 encoder");
        return;
    };
    let player = match Player::start_with_hwdec(HwdecMode::AutoCopy) {
        Ok(player) => player,
        Err(error) => {
            eprintln!("SKIPPED: {error}");
            return;
        }
    };
    player.set_muted(true);
    player.load(path);
    let snapshot = wait_for(&player, Duration::from_secs(20), |snapshot| {
        snapshot.loaded && snapshot.frame.is_some() && snapshot.hardware_decoder.is_some()
    });
    assert_eq!(
        snapshot.hardware_decoder.as_deref(),
        Some("no"),
        "auto-copy should report software decoding when this codec has no allowed copy-back decoder"
    );
    assert!(
        snapshot.frame.is_some_and(|frame| !frame.is_blank()),
        "software fallback must still deliver a decoded frame"
    );
}

#[test]
fn pause_holds_the_playhead_and_resume_continues() {
    let directory = TempDir::new().expect("temp dir");
    let path = clip(&directory);
    let Some(player) = player() else { return };

    player.load(&path);
    wait_for(&player, Duration::from_secs(20), |snapshot| {
        snapshot.position > 0.3
    });

    player.set_paused(true);
    let paused = wait_for(&player, Duration::from_secs(5), |snapshot| {
        snapshot.state == PlayerState::Paused
    });
    std::thread::sleep(Duration::from_millis(300));
    let later = player.snapshot();
    assert!(
        (later.position - paused.position).abs() < 0.2,
        "position moved from {} to {} while paused",
        paused.position,
        later.position
    );

    player.set_paused(false);
    wait_for(&player, Duration::from_secs(5), |snapshot| {
        snapshot.state == PlayerState::Playing
    });
}

#[test]
fn seek_moves_the_playhead() {
    let directory = TempDir::new().expect("temp dir");
    let path = clip(&directory);
    let Some(player) = player() else { return };

    player.load(&path);
    wait_for(&player, Duration::from_secs(20), |snapshot| snapshot.loaded);

    let seek_serial = player.seek(1.4);
    let sought = wait_for(&player, Duration::from_secs(10), |snapshot| {
        snapshot.position_ready && snapshot.seek_serial >= seek_serial && snapshot.position >= 1.2
    });
    assert!(sought.position >= 1.2, "position was {}", sought.position);
}

#[test]
fn rapid_seeks_confirm_only_after_the_latest_target_lands() {
    let directory = TempDir::new().expect("temp dir");
    let path = clip(&directory);
    let Some(player) = player() else { return };

    player.load(&path);
    wait_for(&player, Duration::from_secs(20), |snapshot| {
        snapshot.position_ready
    });
    let _superseded = player.seek(0.7);
    let latest = player.seek(1.4);
    let sought = wait_for(&player, Duration::from_secs(10), |snapshot| {
        snapshot.seek_serial >= latest && snapshot.position >= 1.2
    });
    assert_eq!(sought.seek_serial, latest);
    assert!(sought.position >= 1.2, "position was {}", sought.position);
}

#[test]
fn a_seek_sent_before_the_file_loads_runs_once_it_has_and_is_confirmed() {
    let directory = TempDir::new().expect("temp dir");
    let path = clip(&directory);
    let Some(player) = player() else { return };

    let load_id = player.load_at(&path, None);
    let early = player.seek(1.4);
    let sought = wait_for(&player, Duration::from_secs(20), |snapshot| {
        snapshot.load_id == load_id && snapshot.position_ready && snapshot.seek_serial >= early
    });
    assert!(sought.position >= 1.2, "position was {}", sought.position);
}

#[test]
fn a_seek_into_the_end_is_settled_when_the_file_ends() {
    let directory = TempDir::new().expect("temp dir");
    let path = clip(&directory);
    let Some(player) = player() else { return };

    let load_id = player.load_at(&path, None);
    let loaded = wait_for(&player, Duration::from_secs(20), |snapshot| {
        snapshot.load_id == load_id && snapshot.position_ready && snapshot.duration > 0.0
    });
    let last = player.seek(loaded.duration);
    let ended = wait_for(&player, Duration::from_secs(20), |snapshot| {
        snapshot.state == PlayerState::Ended
    });
    assert!(
        ended.seek_serial >= last,
        "the seek into the end was never settled: {ended:?}"
    );
}

#[test]
fn an_impossible_seek_is_settled_rather_than_left_pending() {
    let directory = TempDir::new().expect("temp dir");
    let path = clip(&directory);
    let Some(player) = player() else { return };

    player.load(&path);
    wait_for(&player, Duration::from_secs(20), |snapshot| {
        snapshot.position_ready
    });
    let serial = player.seek(f64::NAN);
    wait_for(&player, Duration::from_secs(5), |snapshot| {
        snapshot.seek_serial >= serial
    });
}

#[test]
fn resume_position_is_honoured_when_loading() {
    let directory = TempDir::new().expect("temp dir");
    let path = clip(&directory);
    let Some(player) = player() else { return };

    let load_id = player.load_at(&path, Some(1.2));

    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    let resumed = loop {
        let snapshot = player.snapshot();
        if snapshot.load_id == load_id && snapshot.loaded && snapshot.position_ready {
            assert!(
                snapshot.position > 0.9,
                "a settled snapshot reported {} s before the resume point",
                snapshot.position
            );
            break snapshot;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the file never settled"
        );
        std::thread::sleep(Duration::from_millis(1));
    };
    assert!(
        resumed.position > 0.9,
        "resume did not take effect: {}",
        resumed.position
    );
}

#[test]
fn a_new_load_leaves_ended_state_and_plays_again() {
    let directory = TempDir::new().expect("temp dir");
    let path = clip(&directory);
    let Some(player) = player() else { return };

    let first_load = player.load_at(&path, None);
    wait_for(&player, Duration::from_secs(20), |snapshot| {
        snapshot.load_id == first_load && snapshot.state == PlayerState::Ended
    });
    let second_load = player.load_at(&path, None);
    let restarted = wait_for(&player, Duration::from_secs(20), |snapshot| {
        snapshot.load_id == second_load
            && snapshot.loaded
            && snapshot.position_ready
            && snapshot.state == PlayerState::Playing
    });
    assert_ne!(first_load, second_load);
    assert_eq!(restarted.load_id, second_load);
}

#[test]
fn a_new_load_recovers_from_a_failed_state() {
    let directory = TempDir::new().expect("temp dir");
    let path = clip(&directory);
    let Some(player) = player() else { return };

    let missing = directory.path().join("missing.mp4");
    let failed_load = player.load_at(&missing, None);
    let failed = wait_for(&player, Duration::from_secs(5), |snapshot| {
        snapshot.load_id == failed_load && snapshot.state == PlayerState::Failed
    });
    assert!(failed.failure.is_some());

    let recovered_load = player.load_at(&path, None);
    let recovered = wait_for(&player, Duration::from_secs(20), |snapshot| {
        snapshot.load_id == recovered_load
            && snapshot.loaded
            && snapshot.position_ready
            && snapshot.state == PlayerState::Playing
    });
    assert_eq!(recovered.load_id, recovered_load);
}

#[test]
fn anamorphic_video_renders_at_its_display_aspect_without_letterbox_rows() {
    let directory = TempDir::new().expect("temp dir");
    let path = directory.path().join("anamorphic.mp4");
    let status = Command::new("ffmpeg")
        .args([
            "-nostdin",
            "-y",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=1440x1080:rate=24:duration=3",
            "-vf",
            "setsar=4/3",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
        ])
        .arg(&path)
        .status()
        .expect("ffmpeg should be installed");
    assert!(
        status.success(),
        "ffmpeg could not build the anamorphic clip"
    );
    let Some(player) = player() else { return };

    let load_id = player.load_at(&path, None);
    let rendered = wait_for(&player, Duration::from_secs(20), |snapshot| {
        snapshot.load_id == load_id && snapshot.position_ready && snapshot.frame.is_some()
    });
    let frame = rendered.frame.expect("a rendered frame");
    assert_eq!((frame.width, frame.height), (1920, 1080));
    assert!((frame.width as f64 / frame.height as f64 - 16.0 / 9.0).abs() < 0.002);
    let edge_band_has_picture = |rows: std::ops::Range<u32>| {
        rows.into_iter().any(|y| {
            (0..frame.width).step_by(16).any(|x| {
                frame
                    .pixel(x, y)
                    .is_some_and(|[r, g, b, _]| r > 8 || g > 8 || b > 8)
            })
        })
    };
    assert!(
        edge_band_has_picture(0..54) && edge_band_has_picture(frame.height - 54..frame.height),
        "render has internal black bars"
    );
}

fn timing_clip(directory: &TempDir) -> PathBuf {
    let path = directory.path().join("timing.mp4");
    let status = Command::new("ffmpeg")
        .args([
            "-nostdin",
            "-y",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=1920x1080:rate=30:duration=8",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000:duration=8",
            "-ac",
            "2",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-preset",
            "veryfast",
            "-crf",
            "28",
            "-c:a",
            "aac",
            "-b:a",
            "128k",
            "-shortest",
        ])
        .arg(&path)
        .status()
        .expect("ffmpeg should be installed");
    assert!(status.success(), "ffmpeg could not build the timing clip");
    path
}

#[test]
fn the_software_bridge_sustains_a_watchable_frame_rate() {
    let directory = TempDir::new().expect("temp dir");
    let path = timing_clip(&directory);
    let Some(player) = player() else { return };
    player.load(&path);
    let started = wait_for(&player, Duration::from_secs(30), |snapshot| {
        snapshot.frame.is_some() && snapshot.position > 0.3
    });

    let first = started
        .frame
        .as_ref()
        .map(|frame| frame.generation)
        .unwrap_or(0);
    let began = Instant::now();
    std::thread::sleep(Duration::from_secs(4));
    let ended = player.snapshot();
    let elapsed = began.elapsed().as_secs_f64();

    let presented = ended
        .frame
        .as_ref()
        .map(|frame| frame.generation)
        .unwrap_or(first)
        .saturating_sub(first);
    let rate = presented as f64 / elapsed.max(0.001);

    let advance = (ended.position - started.position) / elapsed.max(0.001);
    println!(
        "software bridge: {presented} frames in {elapsed:.2} s = {rate:.1} fps at 1920x1080 \
         (playhead advanced {advance:.2}x real time)"
    );
    assert!(
        rate >= 20.0,
        "the bridge published {rate:.1} fps, too slow to watch"
    );
}

fn silent_clip(directory: &TempDir) -> PathBuf {
    let path = directory.path().join("silent.mp4");
    let status = Command::new("ffmpeg")
        .args([
            "-nostdin",
            "-y",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=320x240:rate=25:duration=2",
            "-f",
            "lavfi",
            "-i",
            "anullsrc=channel_layout=stereo:sample_rate=48000",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-t",
            "2",
        ])
        .arg(&path)
        .status()
        .expect("ffmpeg should be installed");
    assert!(status.success(), "ffmpeg could not build the silent clip");
    path
}

fn multi_track_clip(directory: &TempDir) -> PathBuf {
    let english = directory.path().join("eng.srt");
    let french = directory.path().join("fra.srt");
    std::fs::write(&english, "1\n00:00:00,500 --> 00:00:02,500\nHello.\n").expect("subtitle file");
    std::fs::write(&french, "1\n00:00:00,500 --> 00:00:02,500\nBonjour.\n").expect("subtitle file");

    let path = directory.path().join("multitrack.mkv");
    let status = Command::new("ffmpeg")
        .args([
            "-nostdin",
            "-y",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=320x240:rate=25:duration=4",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=4",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=880:duration=4",
            "-i",
        ])
        .arg(&english)
        .arg("-i")
        .arg(&french)
        .args([
            "-map",
            "0:v",
            "-map",
            "1:a",
            "-map",
            "2:a",
            "-map",
            "3:s",
            "-map",
            "4:s",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-c:s",
            "srt",
            "-metadata:s:a:0",
            "language=eng",
            "-metadata:s:a:1",
            "language=jpn",
            "-metadata:s:s:0",
            "language=eng",
            "-metadata:s:s:1",
            "language=fra",
        ])
        .arg(&path)
        .status()
        .expect("ffmpeg should be installed");
    assert!(
        status.success(),
        "ffmpeg could not build the multitrack clip"
    );
    path
}

fn assert_source_audio_is_audible(path: &Path) {
    let output = Command::new("ffmpeg")
        .args(["-nostdin", "-hide_banner", "-i"])
        .arg(path)
        .args(["-af", "volumedetect", "-f", "null", "-"])
        .output()
        .expect("ffmpeg should be installed");
    let report = String::from_utf8_lossy(&output.stderr).to_string();
    let mean = report
        .lines()
        .find(|line| line.contains("mean_volume"))
        .unwrap_or("")
        .trim()
        .to_string();
    println!("source audio: {mean}");
    assert!(
        mean.contains("mean_volume") && !mean.contains("-inf"),
        "the fixture has no audible audio: {mean}"
    );
}

fn audio_clip(directory: &TempDir) -> PathBuf {
    let path = directory.path().join("audio.mp4");
    let status = Command::new("ffmpeg")
        .args([
            "-nostdin",
            "-y",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=640x360:rate=30:duration=16",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000:duration=16",
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
        .status()
        .expect("ffmpeg should be installed");
    assert!(status.success(), "ffmpeg could not build the audio clip");
    path
}

#[test]
fn audio_is_consumed_in_real_time_without_drifting() {
    let directory = TempDir::new().expect("temp dir");
    let path = audio_clip(&directory);
    assert_source_audio_is_audible(&path);
    let Some(player) = player() else { return };

    player.load(&path);
    let started = wait_for(&player, Duration::from_secs(30), |snapshot| {
        snapshot.position > 0.5 && snapshot.audio_pts.is_some()
    });
    assert!(
        started.audio_pts.expect("the audio clock") > 0.0,
        "the audio clock never advanced past zero, so nothing was played"
    );

    let began = Instant::now();
    let mut series: Vec<(f64, f64, f64)> = Vec::new();
    let mut last_audio = None;
    while began.elapsed() < Duration::from_secs(10) {
        let snapshot = player.snapshot();
        let Some(audio) = snapshot.audio_pts else {
            std::thread::sleep(Duration::from_millis(5));
            continue;
        };
        if last_audio != Some(audio) {
            series.push((began.elapsed().as_secs_f64(), snapshot.position, audio));
            last_audio = Some(audio);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        series.len() >= 12,
        "only {} distinct audio clock changes were collected",
        series.len()
    );

    let first = series.first().expect("a first sample");
    let last = series.last().expect("a last sample");
    let audio_advanced = last.2 - first.2;
    let video_advanced = last.1 - first.1;
    let wall_advanced = last.0 - first.0;
    let mean_wall = series.iter().map(|point| point.0).sum::<f64>() / series.len() as f64;
    let mean_audio = series.iter().map(|point| point.2).sum::<f64>() / series.len() as f64;
    let covariance = series
        .iter()
        .map(|point| (point.0 - mean_wall) * (point.2 - mean_audio))
        .sum::<f64>();
    let wall_variance = series
        .iter()
        .map(|point| (point.0 - mean_wall).powi(2))
        .sum::<f64>();
    let rate = covariance / wall_variance.max(0.001);
    let intercept = mean_audio - rate * mean_wall;
    let max_residual = series
        .iter()
        .map(|point| (point.2 - (intercept + rate * point.0)).abs())
        .fold(0.0_f64, f64::max);
    let max_gap = series
        .windows(2)
        .map(|pair| pair[1].0 - pair[0].0)
        .fold(0.0_f64, f64::max);
    let early_gap_ms = (first.2 - first.1) * 1000.0;
    let late_gap_ms = (last.2 - last.1) * 1000.0;
    let drift_ms = late_gap_ms - early_gap_ms;

    println!(
        "audio: {} distinct clock changes; OLS {rate:.4}x, max residual {:.1} ms, \
         max observation gap {max_gap:.3}s; clock advanced {audio_advanced:.2}s against \
         {video_advanced:.2}s of video; gap {early_gap_ms:+.1} to {late_gap_ms:+.1} ms \
         (drift {drift_ms:+.1} ms)",
        series.len(),
        max_residual * 1000.0
    );
    println!(
        "  against {wall_advanced:.2}s of wall clock: audio {:.4}x, video {:.4}x",
        audio_advanced / wall_advanced.max(0.001),
        video_advanced / wall_advanced.max(0.001)
    );
    let final_snapshot = player.snapshot();
    let selected_audio = final_snapshot
        .tracks
        .iter()
        .filter(|track| track.kind == TrackKind::Audio && track.selected)
        .count();
    println!("audio tracks selected: {selected_audio}");
    assert_eq!(
        selected_audio, 1,
        "exactly one audio track should be in use"
    );

    let avsync_ms = final_snapshot.avsync.map(|value| value * 1000.0);
    let total_ms = final_snapshot
        .total_avsync_change
        .map(|value| value * 1000.0);
    println!(
        "libmpv avsync: last {avsync_ms:?} ms, cumulative adjustment {total_ms:?} ms, mistimed \
         frames {:?}",
        final_snapshot.mistimed_frames
    );
    assert!(
        avsync_ms.is_some(),
        "libmpv did not report an avsync figure, so sync is unmeasured"
    );
    assert!(
        avsync_ms.unwrap_or(0.0).abs() < 100.0,
        "libmpv reports {avsync_ms:?} ms of A/V sync error"
    );
    assert!(
        total_ms.unwrap_or(0.0).abs() < 250.0,
        "libmpv adjusted sync by {total_ms:?} ms over one short file"
    );

    assert!(
        (rate - 1.0).abs() < 0.05,
        "the audio output consumed samples at {rate:.3}x wall-clock rate, which is not playback"
    );
    assert!(
        max_gap < 0.5,
        "audio clock observation gap was {max_gap:.3}s"
    );
    assert!(
        max_residual < 0.150,
        "audio clock residual was {max_residual:.3}s"
    );

    println!(
        "audio output buffer sits {late_gap_ms:+.1} ms from the video playhead (moved \
         {drift_ms:+.1} ms across the window; not an A/V sync figure)"
    );

    let silent = silent_clip(&directory);
    player.load(&silent);
    let silent_state = wait_for(&player, Duration::from_secs(20), |snapshot| {
        snapshot.loaded && snapshot.position > 0.5
    });
    assert_eq!(
        silent_state
            .tracks
            .iter()
            .filter(|track| track.kind == TrackKind::Audio && track.selected)
            .count(),
        1,
        "the silent fixture should still have one selected audio track"
    );
}

#[test]
fn tracks_are_enumerated_and_can_be_switched() {
    let directory = TempDir::new().expect("temp dir");
    let path = multi_track_clip(&directory);
    let Some(player) = player() else { return };

    player.load(&path);
    let loaded = wait_for(&player, Duration::from_secs(30), |snapshot| {
        snapshot.loaded
            && snapshot
                .tracks
                .iter()
                .any(|track| track.kind == TrackKind::Audio)
    });

    let audio: Vec<_> = loaded
        .tracks
        .iter()
        .filter(|track| track.kind == TrackKind::Audio)
        .collect();
    let subtitles: Vec<_> = loaded
        .tracks
        .iter()
        .filter(|track| track.kind == TrackKind::Subtitle)
        .collect();
    assert_eq!(audio.len(), 2, "expected two audio tracks: {audio:#?}");
    assert_eq!(subtitles.len(), 2, "expected two subtitle tracks");

    let mut languages: Vec<String> = audio
        .iter()
        .filter_map(|track| track.language.clone())
        .collect();
    languages.sort();
    assert_eq!(languages, vec!["eng".to_string(), "jpn".to_string()]);
    assert!(
        audio
            .iter()
            .any(|track| track.codec.as_deref() == Some("aac")),
        "audio codecs were {:?}",
        audio.iter().map(|track| &track.codec).collect::<Vec<_>>()
    );
    assert!(
        audio.iter().filter(|track| track.selected).count() == 1,
        "exactly one audio track should be selected at load"
    );

    let wanted = audio
        .iter()
        .find(|track| track.language.as_deref() == Some("jpn"))
        .expect("a Japanese track")
        .id;
    player.select_track(TrackKind::Audio, wanted);
    let switched = wait_for(&player, Duration::from_secs(10), |snapshot| {
        snapshot
            .tracks
            .iter()
            .any(|track| track.kind == TrackKind::Audio && track.selected && track.id == wanted)
    });
    assert_eq!(
        switched
            .tracks
            .iter()
            .filter(|track| track.kind == TrackKind::Audio && track.selected)
            .count(),
        1,
        "switching must leave exactly one audio track selected"
    );

    let subtitle_id = subtitles[0].id;
    player.select_track(TrackKind::Subtitle, subtitle_id);
    wait_for(&player, Duration::from_secs(10), |snapshot| {
        snapshot
            .tracks
            .iter()
            .any(|track| track.kind == TrackKind::Subtitle && track.selected)
    });

    player.select_track(TrackKind::Subtitle, NO_TRACK);
    let off = wait_for(&player, Duration::from_secs(10), |snapshot| {
        !snapshot
            .tracks
            .iter()
            .any(|track| track.kind == TrackKind::Subtitle && track.selected)
    });
    assert!(
        !off.tracks
            .iter()
            .any(|track| track.kind == TrackKind::Subtitle && track.selected),
        "turning subtitles off did not take effect"
    );
}

#[test]
fn subtitle_style_reaches_libmpv_and_survives_a_file_change() {
    let directory = TempDir::new().expect("temp dir");
    let path = multi_track_clip(&directory);
    let Some(player) = player() else { return };

    player.load(&path);
    wait_for(&player, Duration::from_secs(30), |snapshot| snapshot.loaded);

    player.set_subtitle_style(1.75, 88.0);
    let styled = wait_for(&player, Duration::from_secs(10), |snapshot| {
        snapshot.subtitle_scale.is_some() && snapshot.subtitle_position.is_some()
    });

    let scale = styled.subtitle_scale.expect("a scale");
    assert!(
        (scale - 1.75).abs() < 0.01,
        "libmpv holds sub-scale {scale}, not the requested 1.75"
    );
    let position = styled.subtitle_position.expect("a position");
    assert!(
        (position - 88.0).abs() < 0.01,
        "libmpv holds sub-pos {position}, not the requested 88"
    );

    player.load(&path);
    let reloaded = wait_for(&player, Duration::from_secs(30), |snapshot| {
        snapshot.loaded
            && snapshot
                .tracks
                .iter()
                .any(|t| t.kind == TrackKind::Subtitle)
    });
    assert_eq!(reloaded.subtitle_scale, styled.subtitle_scale);
    assert_eq!(reloaded.subtitle_position, styled.subtitle_position);
}

#[test]
fn an_impossible_subtitle_style_is_clamped_rather_than_passed_on() {
    let directory = TempDir::new().expect("temp dir");
    let path = multi_track_clip(&directory);
    let Some(player) = player() else { return };

    player.load(&path);
    wait_for(&player, Duration::from_secs(30), |snapshot| snapshot.loaded);

    player.set_subtitle_style(-5.0, 9_000.0);
    let clamped = wait_for(&player, Duration::from_secs(10), |snapshot| {
        snapshot.subtitle_scale.is_some()
    });

    let scale = clamped.subtitle_scale.expect("a clamped scale");
    assert!(
        (scale - SUB_SCALE_RANGE.0).abs() < 1e-6,
        "clamped scale was {scale}, expected {}",
        SUB_SCALE_RANGE.0
    );
    let position = clamped.subtitle_position.expect("a clamped position");
    assert!(
        (position - SUB_POSITION_RANGE.1).abs() < 1e-6,
        "clamped position was {position}, expected {}",
        SUB_POSITION_RANGE.1
    );

    player.set_subtitle_style(f64::NAN, f64::INFINITY);
    let after = wait_for(&player, Duration::from_secs(3), |snapshot| {
        snapshot.subtitle_scale == clamped.subtitle_scale
            && snapshot.subtitle_position == clamped.subtitle_position
    });
    assert_eq!(
        after.failure, None,
        "a skipped value must not fail the player"
    );
    assert_eq!(
        after.subtitle_scale, clamped.subtitle_scale,
        "a non-finite scale must leave the last good value in place"
    );
}

#[test]
fn a_missing_file_is_reported_honestly() {
    let Some(player) = player() else { return };
    player.load("/definitely/not/here.mkv");

    let failed = wait_for(&player, Duration::from_secs(5), |snapshot| {
        snapshot.state == PlayerState::Failed
    });
    let failure = failed.failure.expect("a failure reason");
    assert_eq!(failure.code, "file_missing");
    assert!(failure.message.contains("no longer on disk"));
}

#[test]
fn a_malformed_media_file_is_reported_rather_than_shown_as_empty() {
    let directory = TempDir::new().expect("temp dir");

    let path = directory.path().join("broken.mkv");
    std::fs::write(&path, b"\x1aE\xdf\xa3this is not actually matroska").expect("write");
    let Some(player) = player() else { return };
    player.load(&path);

    let settled = wait_for(&player, Duration::from_secs(10), |snapshot| {
        snapshot.state == PlayerState::Failed || snapshot.failure.is_some()
    });
    let failure = settled.failure.expect("a reason the file cannot play");
    assert!(
        !failure.code.is_empty() && !failure.message.is_empty(),
        "the failure must name a code and say something: {failure:?}"
    );
    assert!(
        settled.frame.is_none(),
        "a file that cannot be decoded must not present a frame"
    );
}

#[test]
fn a_long_session_keeps_one_frame_and_advances_monotonically() {
    let directory = TempDir::new().expect("temp dir");

    let path = thirty_second_clip(&directory);
    let Some(player) = player() else { return };
    player.load(&path);
    wait_for(&player, Duration::from_secs(30), |snapshot| {
        snapshot.position > 1.0
    });

    let mut last = 0.0_f64;
    let mut samples = 0;
    let deadline = Instant::now() + Duration::from_secs(12);
    while Instant::now() < deadline {
        let snapshot = player.snapshot();
        assert!(
            snapshot.position >= last - 0.5,
            "the playhead went backwards without a seek: {last} then {}",
            snapshot.position
        );
        assert!(
            snapshot.frame.as_ref().is_none_or(|frame| frame.width > 0),
            "a retained frame must be a real frame"
        );
        last = snapshot.position;
        samples += 1;
        std::thread::sleep(Duration::from_millis(250));
    }
    assert!(samples > 20, "the session ran for {samples} samples");
    assert!(last > 1.0, "the playhead advanced: {last}");
}

fn thirty_second_clip(directory: &TempDir) -> PathBuf {
    let path = directory.path().join("long.mp4");
    let status = Command::new("ffmpeg")
        .args([
            "-nostdin",
            "-y",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=320x240:rate=25:duration=30",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=30",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-t",
            "30",
        ])
        .arg(&path)
        .status()
        .expect("ffmpeg should be installed");
    assert!(status.success(), "ffmpeg could not build the long clip");
    path
}

#[test]
fn volume_and_mute_are_read_back_from_the_player() {
    let Some(player) = player() else {
        return;
    };

    player.set_volume(42.0);
    let snapshot = wait_for(&player, Duration::from_secs(5), |snapshot| {
        snapshot
            .volume
            .is_some_and(|volume| (volume - 42.0).abs() < 0.5)
    });
    assert!(!snapshot.muted.unwrap_or(false), "not muted yet");

    player.set_muted(true);
    let snapshot = wait_for(&player, Duration::from_secs(5), |snapshot| {
        snapshot.muted == Some(true)
    });
    assert!(snapshot.muted.expect("reported"), "mute was accepted");

    player.set_muted(false);
    wait_for(&player, Duration::from_secs(5), |snapshot| {
        snapshot.muted == Some(false)
    });

    player.set_volume(500.0);
    let snapshot = wait_for(&player, Duration::from_secs(5), |snapshot| {
        snapshot.volume.is_some_and(|volume| volume <= 100.0)
    });
    assert!(snapshot.volume.expect("reported") <= 100.0);
}
