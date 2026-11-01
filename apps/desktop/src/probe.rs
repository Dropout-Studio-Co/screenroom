use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

use screenroom_core::Library;

const MAX_PROBE_BYTES: usize = 64 * 1024;

const PASS_PAGE: i64 = 100;

const REPORT_EVERY: usize = 50;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Facts {
    pub duration_ms: i64,

    pub cover_stream: Option<u32>,

    pub has_video: bool,

    pub has_audio: bool,
}

pub fn command_for(path: &Path) -> Command {
    let mut command = Command::new(crate::artwork::tool("ffprobe"));
    command
        .arg("-v")
        .arg("error")
        .arg("-show_entries")
        .arg("format=duration:stream=index,codec_type:stream_disposition=attached_pic")
        .arg("-of")
        .arg("flat")
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    command
}

pub fn probe(path: &Path) -> Result<Facts, String> {
    let bytes = crate::artwork::capture(command_for(path), MAX_PROBE_BYTES, "Probing the file")?;
    Ok(parse(&String::from_utf8_lossy(&bytes)))
}

pub fn parse(output: &str) -> Facts {
    let mut facts = Facts::default();
    let mut video = Vec::new();
    let mut audio = Vec::new();
    let mut covers = Vec::new();
    for line in output.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim().trim_matches('"');
        if key == "format.duration" {
            if let Ok(seconds) = value.parse::<f64>()
                && seconds.is_finite()
                && seconds > 0.0
            {
                facts.duration_ms = (seconds * 1_000.0).round() as i64;
            }
            continue;
        }
        let Some(rest) = key.strip_prefix("streams.stream.") else {
            continue;
        };
        let Some((index, field)) = rest.split_once('.') else {
            continue;
        };
        let Ok(index) = index.parse::<u32>() else {
            continue;
        };
        match (field, value) {
            ("codec_type", "video") => video.push(index),
            ("codec_type", "audio") => audio.push(index),
            ("disposition.attached_pic", "1") => covers.push(index),
            _ => {}
        }
    }

    facts.cover_stream = covers.into_iter().find(|index| video.contains(index));
    facts.has_video = video
        .iter()
        .any(|index| !facts.cover_stream.is_some_and(|cover| cover == *index));
    facts.has_audio = !audio.is_empty();
    facts
}

pub struct DurationPass {
    pub reports: Receiver<usize>,
}

pub fn spawn_duration_pass(library: Arc<Library>, paused: Arc<AtomicBool>) -> DurationPass {
    let (sender, reports) = channel();
    std::thread::spawn(move || {
        let mut after: Option<String> = None;
        let mut stored = 0_usize;
        let mut unreported = 0_usize;
        loop {
            let page = match library.unprobed_files(after.as_deref(), PASS_PAGE) {
                Ok(page) if !page.is_empty() => page,
                _ => break,
            };
            for file in page {
                while paused.load(Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_millis(500));
                }
                if let Ok(facts) = probe(&file.path)
                    && library
                        .record_duration(&file.file_id, facts.duration_ms)
                        .unwrap_or(false)
                {
                    stored += 1;
                    unreported += 1;
                }
                after = Some(file.file_id);
                if unreported >= REPORT_EVERY {
                    unreported = 0;
                    if sender.send(stored).is_err() {
                        return;
                    }
                }
            }
        }
        let _ = sender.send(stored);
    });
    DurationPass { reports }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duration_and_cover_are_read_from_flat_output() {
        let output = "streams.stream.0.index=0\n\
            streams.stream.0.codec_type=\"video\"\n\
            streams.stream.0.disposition.attached_pic=0\n\
            streams.stream.1.codec_type=\"audio\"\n\
            streams.stream.1.disposition.attached_pic=0\n\
            streams.stream.8.index=8\n\
            streams.stream.8.codec_type=\"video\"\n\
            streams.stream.8.disposition.attached_pic=1\n\
            format.duration=\"6731.751000\"\n";
        assert_eq!(
            parse(output),
            Facts {
                duration_ms: 6_731_751,
                cover_stream: Some(8),
                has_video: true,
                has_audio: true,
            }
        );
    }

    #[test]
    fn missing_or_hostile_values_mean_unknown() {
        assert_eq!(parse(""), Facts::default());
        assert_eq!(parse("format.duration=\"N/A\"\n"), Facts::default());
        assert_eq!(
            parse("format.duration=\"-3\"\nformat.duration=inf\n"),
            Facts::default()
        );

        let audio_flag = "streams.stream.2.codec_type=\"audio\"\n\
            streams.stream.2.disposition.attached_pic=1\n";
        assert_eq!(parse(audio_flag).cover_stream, None);
    }

    #[test]
    fn the_probe_passes_the_path_as_one_argument() {
        let command = command_for(Path::new("/media/a film; rm -rf /.mkv"));
        let args: Vec<String> = command
            .get_args()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect();
        assert_eq!(
            args.last().map(String::as_str),
            Some("/media/a film; rm -rf /.mkv")
        );
        assert!(command.get_program().to_string_lossy().ends_with("ffprobe"));
    }
}
