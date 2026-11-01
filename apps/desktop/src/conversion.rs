use std::io::{BufRead, BufReader, Read};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use screenroom_core::{ConversionPlan, ConversionProfile};

const POLL_INTERVAL: Duration = Duration::from_millis(100);

const MAX_ERROR_BYTES: u64 = 8 * 1024;

const MIN_BUDGET: Duration = Duration::from_secs(900);

const MAX_REASON: usize = 300;

pub fn command_for(plan: &ConversionPlan) -> Command {
    let mut command = Command::new(crate::artwork::tool("ffmpeg"));
    command
        .arg("-nostdin")
        .arg("-v")
        .arg("error")
        .arg("-n")
        .arg("-progress")
        .arg("pipe:1")
        .arg("-nostats")
        .arg("-i")
        .arg(&plan.source_path)
        .arg("-map")
        .arg("0:v:0")
        .arg("-map")
        .arg("0:a?")
        .arg("-map")
        .arg("0:s?")
        .args(profile_args(plan.profile))
        .arg("-movflags")
        .arg("+faststart")
        .arg(&plan.output_path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

pub fn profile_args(profile: ConversionProfile) -> Vec<&'static str> {
    match profile {
        ConversionProfile::Remux => vec!["-c", "copy", "-c:s", "mov_text"],
        ConversionProfile::AudioFix => vec![
            "-c:v", "copy", "-c:a", "aac", "-b:a", "192k", "-c:s", "mov_text",
        ],
        ConversionProfile::Standard => vec![
            "-c:v",
            "libx264",
            "-preset",
            "veryfast",
            "-crf",
            "20",
            "-vf",
            "scale=min(1920\\,iw):-2",
            "-c:a",
            "aac",
            "-b:a",
            "192k",
            "-c:s",
            "mov_text",
        ],
    }
}

pub fn budget(duration_ms: i64) -> Duration {
    if duration_ms <= 0 {
        return MIN_BUDGET;
    }
    let scaled = Duration::from_millis((duration_ms.max(0) as u64).saturating_mul(4));
    scaled.max(MIN_BUDGET)
}

pub fn parse_progress(line: &str, duration_ms: i64) -> Option<u8> {
    let (key, value) = line.trim().split_once('=')?;
    let value = value.trim();
    match key {
        "progress" => (value == "end").then_some(99),
        "out_time_us" | "out_time_ms" => {
            if duration_ms <= 0 {
                return None;
            }
            let micros = value.parse::<i64>().ok()?;
            if micros <= 0 {
                return Some(0);
            }
            let total = duration_ms.saturating_mul(1_000);
            Some(((micros.saturating_mul(100) / total).clamp(0, 99)) as u8)
        }
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConversionOutcome {
    Succeeded {
        bytes: u64,
        duration_ms: i64,
        has_video: bool,
        has_audio: bool,
        seconds: u64,
    },

    Failed(String),

    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConversionEvent {
    Progress(u8),

    Finished(ConversionOutcome),
}

pub struct Runner {
    cancel: Arc<AtomicBool>,

    events: Receiver<ConversionEvent>,
}

impl Runner {
    pub fn spawn(plan: ConversionPlan) -> Result<Self, String> {
        let mut command = command_for(&plan);
        let mut child = command
            .spawn()
            .map_err(|error| format!("ffmpeg could not be started: {error}"))?;

        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let (sender, events) = channel::<ConversionEvent>();
        let cancel = Arc::new(AtomicBool::new(false));

        let progress_sender = sender.clone();
        let duration_ms = plan.duration_ms;
        std::thread::spawn(move || {
            let Some(stdout) = stdout else { return };
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if let Some(percent) = parse_progress(&line, duration_ms)
                    && progress_sender
                        .send(ConversionEvent::Progress(percent))
                        .is_err()
                {
                    return;
                }
            }
        });

        let (error_sender, errors) = channel::<String>();
        std::thread::spawn(move || {
            let mut collected = Vec::new();
            if let Some(stderr) = stderr {
                let mut reader = stderr.take(MAX_ERROR_BYTES);
                let _ = reader.read_to_end(&mut collected);
            }
            let text = String::from_utf8_lossy(&collected).to_string();
            let last = text
                .lines()
                .rev()
                .map(str::trim)
                .find(|line| !line.is_empty())
                .unwrap_or_default()
                .chars()
                .take(MAX_REASON)
                .collect::<String>();
            let _ = error_sender.send(last);
        });

        let worker_cancel = Arc::clone(&cancel);
        std::thread::spawn(move || {
            let supervised = supervise(
                &mut child,
                &worker_cancel,
                budget(duration_ms),
                &plan.output_path,
            );
            let outcome = match supervised {
                Supervised::Succeeded(seconds) => match inspect(&plan.output_path, seconds) {
                    Ok(outcome) => outcome,
                    Err(reason) => ConversionOutcome::Failed(reason),
                },
                Supervised::Finished => ConversionOutcome::Failed(failure_reason(
                    &errors,
                    "ffmpeg stopped without writing a result.",
                )),
                Supervised::Cancelled => ConversionOutcome::Cancelled,
                Supervised::Overran => ConversionOutcome::Failed(
                    "The conversion ran past its time budget and was stopped.".to_string(),
                ),
            };
            let _ = sender.send(ConversionEvent::Finished(outcome));
        });

        Ok(Self { cancel, events })
    }

    pub fn poll(&self) -> Vec<ConversionEvent> {
        self.events.try_iter().collect()
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }
}

impl Drop for Runner {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::SeqCst);
    }
}

enum Supervised {
    Succeeded(u64),

    Finished,

    Cancelled,

    Overran,
}

fn failure_reason(errors: &Receiver<String>, fallback: &str) -> String {
    match errors.recv_timeout(Duration::from_millis(500)) {
        Ok(reason) if !reason.trim().is_empty() => reason,
        _ => fallback.to_string(),
    }
}

fn supervise(
    child: &mut Child,
    cancel: &AtomicBool,
    budget: Duration,
    _output: &std::path::Path,
) -> Supervised {
    let started = Instant::now();
    loop {
        if cancel.load(Ordering::SeqCst) {
            let _ = child.kill();
            let _ = child.wait();
            return Supervised::Cancelled;
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                if status.success() {
                    return Supervised::Succeeded(started.elapsed().as_secs());
                }
                return Supervised::Finished;
            }
            Ok(None) => {
                if started.elapsed() >= budget {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Supervised::Overran;
                }
                std::thread::sleep(POLL_INTERVAL);
            }
            Err(_) => return Supervised::Finished,
        }
    }
}

fn inspect(output: &std::path::Path, seconds: u64) -> Result<ConversionOutcome, String> {
    let bytes = std::fs::metadata(output)
        .map_err(|error| format!("The converted file could not be read back: {error}"))?;
    if bytes.len() == 0 {
        return Err("ffmpeg wrote an empty file, so it was not kept.".to_string());
    }
    let facts = crate::probe::probe(output)
        .map_err(|reason| format!("The converted file could not be checked: {reason}"))?;
    Ok(ConversionOutcome::Succeeded {
        bytes: bytes.len(),
        duration_ms: facts.duration_ms,
        has_video: facts.has_video,
        has_audio: facts.has_audio,
        seconds,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use screenroom_core::ConversionProfile;
    use std::path::PathBuf;

    fn plan(profile: ConversionProfile) -> ConversionPlan {
        ConversionPlan {
            file_id: "file-1".to_string(),
            title: "A Film".to_string(),
            profile,
            source_path: PathBuf::from("/media/A Film; rm -rf /.mkv"),
            output_path: PathBuf::from("/Users/me/Movies/Screenroom Converted/A Film (remux).mp4"),
            source_size: 10,
            source_modified_ms: 1,
            duration_ms: 6_000_000,
        }
    }

    #[test]
    fn the_command_never_overwrites_and_writes_progress_on_stdout() {
        let command = command_for(&plan(ConversionProfile::Remux));
        let args: Vec<String> = command
            .get_args()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect();
        assert_eq!(args[0], "-nostdin");
        assert!(args.contains(&"-n".to_string()));
        assert!(args.windows(2).any(|pair| pair == ["-progress", "pipe:1"]));
        assert!(args.contains(&"/media/A Film; rm -rf /.mkv".to_string()));
        assert_eq!(
            args.last().map(String::as_str),
            Some("/Users/me/Movies/Screenroom Converted/A Film (remux).mp4")
        );
        assert!(args.contains(&"0:v:0".to_string()));
        assert!(command.get_program().to_string_lossy().ends_with("ffmpeg"));
    }

    #[test]
    fn a_remux_copies_both_streams_and_the_standard_profile_re_encodes() {
        let remux = profile_args(ConversionProfile::Remux);
        assert!(remux.windows(2).any(|pair| pair == ["-c", "copy"]));
        assert!(!remux.contains(&"libx264"));

        let audio = profile_args(ConversionProfile::AudioFix);
        assert!(audio.windows(2).any(|pair| pair == ["-c:v", "copy"]));
        assert!(audio.windows(2).any(|pair| pair == ["-c:a", "aac"]));

        let standard = profile_args(ConversionProfile::Standard);
        assert!(standard.contains(&"libx264"));
        assert!(standard.iter().any(|arg| arg.starts_with("scale=min(1920")));
    }

    #[test]
    fn progress_is_read_from_the_microsecond_clock_and_stays_under_a_hundred() {
        assert_eq!(parse_progress("out_time_us=0", 6_000_000), Some(0));
        assert_eq!(
            parse_progress("out_time_us=3000000000", 6_000_000),
            Some(50)
        );
        assert_eq!(
            parse_progress("out_time_ms=6000000000", 6_000_000),
            Some(99)
        );
        assert_eq!(parse_progress("progress=end", 6_000_000), Some(99));
        assert_eq!(parse_progress("progress=continue", 6_000_000), None);
        assert_eq!(parse_progress("frame=120", 6_000_000), None);
        assert_eq!(parse_progress("out_time_us=100", 0), None);
        assert_eq!(
            parse_progress("out_time_us=100", 6_000_000),
            Some(0),
            "a tiny position never rounds up past what is done"
        );
        assert_eq!(parse_progress("nonsense", 6_000_000), None);
    }

    #[test]
    fn the_time_budget_never_drops_below_the_floor() {
        assert_eq!(budget(0), MIN_BUDGET);
        assert_eq!(budget(-5), MIN_BUDGET);
        assert_eq!(budget(1_000), MIN_BUDGET);
        assert!(budget(3_600_000) > MIN_BUDGET);
    }
}
