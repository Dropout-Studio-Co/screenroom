use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, channel, sync_channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use screenroom_core::{ArtworkCandidate, Library};

const POSTER_WIDTH: u32 = 1280;

const MAX_POSTER_BYTES: usize = 2 * 1024 * 1024;

const EXTRACT_TIMEOUT: Duration = Duration::from_secs(20);

const QUEUE_DEPTH: usize = 16;

const POSTER_WORKERS: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Poster {
    pub bytes: Vec<u8>,

    pub width: i64,

    pub height: i64,

    pub sampled_at_ms: i64,

    pub cover: bool,
}

pub fn sample_position_ms(duration_ms: i64) -> i64 {
    if duration_ms <= 0 {
        return 120_000;
    }
    (duration_ms / 10).clamp(1_000, 900_000)
}

pub fn tool(name: &str) -> PathBuf {
    tool_in(name, &std::env::var_os("PATH").unwrap_or_default())
}

fn tool_in(name: &str, path: &std::ffi::OsStr) -> PathBuf {
    std::env::split_paths(path)
        .chain([
            PathBuf::from("/opt/homebrew/bin"),
            PathBuf::from("/usr/local/bin"),
        ])
        .filter(|directory| directory.is_absolute())
        .map(|directory| directory.join(name))
        .find(|candidate| candidate.is_file())
        .unwrap_or_else(|| PathBuf::from(name))
}

pub fn command_for(path: &Path, at_ms: i64) -> Command {
    let mut command = Command::new(tool("ffmpeg"));
    command
        .arg("-nostdin")
        .arg("-v")
        .arg("error")
        .arg("-ss")
        .arg(format!("{:.3}", at_ms.max(0) as f64 / 1_000.0))
        .arg("-i")
        .arg(path)
        .arg("-an")
        .arg("-sn")
        .arg("-frames:v")
        .arg("1")
        .arg("-vf")
        .arg(format!("scale={POSTER_WIDTH}:-2"))
        .arg("-f")
        .arg("image2")
        .arg("-vcodec")
        .arg("mjpeg")
        .arg("pipe:1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    command
}

pub fn cover_command_for(path: &Path, stream: u32) -> Command {
    let mut command = Command::new(tool("ffmpeg"));
    command
        .arg("-nostdin")
        .arg("-v")
        .arg("error")
        .arg("-i")
        .arg(path)
        .arg("-map")
        .arg(format!("0:{stream}"))
        .arg("-frames:v")
        .arg("1")
        .arg("-vf")
        .arg(format!("scale={POSTER_WIDTH}:-2"))
        .arg("-f")
        .arg("image2")
        .arg("-vcodec")
        .arg("mjpeg")
        .arg("pipe:1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    command
}

pub fn extract_cover(path: &Path, stream: u32) -> Result<Poster, String> {
    let bytes = capture(
        cover_command_for(path, stream),
        MAX_POSTER_BYTES,
        "Reading the cover",
    )?;
    let (width, height) =
        jpeg_dimensions(&bytes).ok_or_else(|| "The cover was not a readable image.".to_string())?;
    Ok(Poster {
        bytes,
        width,
        height,
        sampled_at_ms: 0,
        cover: true,
    })
}

pub fn best_poster(library: &Library, candidate: &ArtworkCandidate) -> Result<Poster, String> {
    let mut duration_ms = candidate.duration_ms;
    let mut cover = None;
    if let Ok(facts) = crate::probe::probe(&candidate.path) {
        if facts.duration_ms > 0 {
            let _ = library.record_duration(&candidate.file_id, facts.duration_ms);
            if duration_ms <= 0 {
                duration_ms = facts.duration_ms;
            }
        }
        cover = facts.cover_stream;
    }
    if let Some(stream) = cover
        && let Ok(poster) = extract_cover(&candidate.path, stream)
    {
        return Ok(poster);
    }
    extract_poster(&candidate.path, sample_position_ms(duration_ms))
}

pub fn extract_poster(path: &Path, at_ms: i64) -> Result<Poster, String> {
    let mut last_reason = String::new();
    for attempt in [at_ms, 0] {
        match run(path, attempt) {
            Ok(bytes) if !bytes.is_empty() => {
                let (width, height) = jpeg_dimensions(&bytes)
                    .ok_or_else(|| "The extracted frame was not a readable JPEG.".to_string())?;
                return Ok(Poster {
                    bytes,
                    width,
                    height,
                    sampled_at_ms: attempt,
                    cover: false,
                });
            }
            Ok(_) => last_reason = format!("No frame could be read at {attempt} ms."),
            Err(reason) => last_reason = reason,
        }
    }
    Err(last_reason)
}

fn run(path: &Path, at_ms: i64) -> Result<Vec<u8>, String> {
    capture(
        command_for(path, at_ms),
        MAX_POSTER_BYTES,
        "Reading a frame",
    )
}

pub(crate) fn capture(mut command: Command, cap: usize, what: &str) -> Result<Vec<u8>, String> {
    let program = command.get_program().to_string_lossy().to_string();
    let mut child = command
        .spawn()
        .map_err(|error| format!("{program} could not be started: {error}"))?;

    let stdout = child.stdout.take();
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(mut stream) = stdout {
            let _ = stream.by_ref().take(cap as u64 + 1).read_to_end(&mut bytes);
        }
        bytes
    });

    let deadline = Instant::now() + EXTRACT_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = reader.join();
                return Err(format!("{program} could not be waited on: {error}"));
            }
        }
    };

    let bytes = reader
        .join()
        .map_err(|_| "the output reader stopped unexpectedly".to_string())?;
    match status {
        None => Err(format!("{what} took too long and was stopped.")),
        Some(status) if !status.success() && bytes.is_empty() => {
            Err(format!("{program} could not read this file."))
        }
        Some(_) if bytes.len() > cap => Err("The output was too large to keep.".to_string()),
        Some(_) => Ok(bytes),
    }
}

pub fn jpeg_dimensions(bytes: &[u8]) -> Option<(i64, i64)> {
    if !bytes.starts_with(&[0xFF, 0xD8]) {
        return None;
    }
    let mut index = 2;

    while index + 8 < bytes.len() {
        if bytes[index] != 0xFF {
            index += 1;
            continue;
        }
        let marker = bytes[index + 1];

        if (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
            let height = u16::from_be_bytes([bytes[index + 5], bytes[index + 6]]) as i64;
            let width = u16::from_be_bytes([bytes[index + 7], bytes[index + 8]]) as i64;
            return (width > 0 && height > 0).then_some((width, height));
        }
        let length = u16::from_be_bytes([bytes[index + 2], bytes[index + 3]]) as usize;
        if length < 2 {
            return None;
        }
        index += 2 + length;
    }
    None
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArtworkEvent {
    Stored { work_id: String },

    Failed { work_id: String, reason: String },
}

struct Job {
    candidate: ArtworkCandidate,
}

type Handler = Arc<dyn Fn(&ArtworkCandidate) -> ArtworkEvent + Send + Sync>;

pub struct Extractor {
    requests: SyncSender<Job>,
    events: Receiver<ArtworkEvent>,
}

impl Extractor {
    pub fn spawn(library: Arc<Library>) -> Self {
        Self::pooled(Arc::new(move |candidate: &ArtworkCandidate| {
            draw_and_store(&library, candidate)
        }))
    }

    fn pooled(handler: Handler) -> Self {
        let (requests, queue) = sync_channel::<Job>(QUEUE_DEPTH);
        let (sender, events) = channel::<ArtworkEvent>();
        let queue = Arc::new(Mutex::new(queue));
        for _ in 0..POSTER_WORKERS {
            let queue = Arc::clone(&queue);
            let handler = Arc::clone(&handler);
            let sender = sender.clone();
            std::thread::spawn(move || {
                loop {
                    let job = {
                        let queue = queue
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner());
                        queue.recv()
                    };
                    let Ok(job) = job else { return };
                    if sender.send(handler(&job.candidate)).is_err() {
                        return;
                    }
                }
            });
        }
        Self { requests, events }
    }

    pub fn request(&self, candidate: ArtworkCandidate) -> bool {
        match self.requests.try_send(Job { candidate }) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => false,
        }
    }

    pub fn poll(&self) -> Vec<ArtworkEvent> {
        self.events.try_iter().collect()
    }
}

fn draw_and_store(library: &Library, candidate: &ArtworkCandidate) -> ArtworkEvent {
    let work_id = candidate.work_id.clone();
    match best_poster(library, candidate) {
        Ok(poster) => match library.store_artwork(
            &candidate.work_id,
            Some(&candidate.file_id),
            &poster.bytes,
            poster.width,
            poster.height,
            poster.sampled_at_ms,
        ) {
            Ok(()) => ArtworkEvent::Stored { work_id },
            Err(error) => ArtworkEvent::Failed {
                work_id,
                reason: format!("{}: {error}", error.code()),
            },
        },
        Err(reason) => ArtworkEvent::Failed { work_id, reason },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sample_position_is_inside_the_file_and_never_at_its_start() {
        assert_eq!(sample_position_ms(0), 120_000);

        assert_eq!(sample_position_ms(1_000), 1_000);
        assert_eq!(sample_position_ms(600_000), 60_000);
        assert_eq!(sample_position_ms(10_800_000), 900_000);
    }

    #[test]
    fn the_command_passes_the_path_as_an_argument_and_writes_to_stdout() {
        let command = command_for(Path::new("/media/a film; rm -rf /.mkv"), 65_000);
        let args: Vec<String> = command
            .get_args()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect();
        assert_eq!(args[0], "-nostdin");
        assert!(args.contains(&"/media/a film; rm -rf /.mkv".to_string()));
        assert!(args.contains(&"65.000".to_string()));
        assert!(args.contains(&"1".to_string()));
        assert_eq!(args.last().map(String::as_str), Some("pipe:1"));

        assert!(command.get_program().to_string_lossy().ends_with("ffmpeg"));
    }

    #[test]
    fn a_tool_is_found_outside_a_minimal_launch_path() {
        let launchd = std::ffi::OsStr::new("/usr/bin:/bin:/usr/sbin:/sbin");
        let homebrew = ["/opt/homebrew/bin/ffmpeg", "/usr/local/bin/ffmpeg"]
            .into_iter()
            .map(PathBuf::from)
            .find(|candidate| candidate.is_file());
        let found = tool_in("ffmpeg", launchd);
        match homebrew {
            Some(expected) => assert_eq!(found, expected),
            None => assert_eq!(found, PathBuf::from("ffmpeg")),
        }

        let directory = tempfile::TempDir::new().expect("temp dir");
        let planted = directory.path().join("ffmpeg");
        std::fs::write(&planted, b"").expect("planted tool");
        assert_eq!(tool_in("ffmpeg", directory.path().as_os_str()), planted);
    }

    #[test]
    fn a_poster_is_extracted_wide_enough_for_the_hero() {
        const {
            assert!(
                POSTER_WIDTH >= 1280,
                "a 530px hero at 1440 must not upscale"
            )
        };
        const {
            assert!(
                MAX_POSTER_BYTES <= 2 * 1024 * 1024,
                "one poster must stay bounded"
            )
        };
        let args: Vec<String> = command_for(Path::new("/media/a.mkv"), 0)
            .get_args()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect();
        assert!(args.contains(&"scale=1280:-2".to_string()));
    }

    #[test]
    fn dimensions_are_read_from_a_start_of_frame_header() {
        let mut bytes = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x04, 0x00, 0x00];
        bytes.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x11, 0x08, 0x02, 0x58, 0x01, 0x90]);
        assert_eq!(jpeg_dimensions(&bytes), Some((400, 600)));
        assert_eq!(jpeg_dimensions(b"not a jpeg"), None);
    }

    fn candidate(index: usize) -> ArtworkCandidate {
        ArtworkCandidate {
            work_id: format!("work-{index}"),
            file_id: format!("file-{index}"),
            title: format!("Title {index}"),
            duration_ms: 600_000,
            path: PathBuf::from(format!("/media/{index}.mkv")),
        }
    }

    fn drain_until(extractor: &Extractor, wanted: usize) -> Vec<ArtworkEvent> {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut events = Vec::new();
        while events.len() < wanted && Instant::now() < deadline {
            events.extend(extractor.poll());
            std::thread::sleep(Duration::from_millis(2));
        }
        events
    }

    #[test]
    fn the_pool_draws_several_posters_at_once_and_stays_small() {
        const JOBS: usize = 6;
        const JOB_COST: Duration = Duration::from_millis(120);

        let live = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let high_water = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let handler: Handler = {
            let live = Arc::clone(&live);
            let high_water = Arc::clone(&high_water);
            Arc::new(move |candidate: &ArtworkCandidate| {
                let now = live.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                high_water.fetch_max(now, std::sync::atomic::Ordering::SeqCst);
                std::thread::sleep(JOB_COST);
                live.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
                ArtworkEvent::Stored {
                    work_id: candidate.work_id.clone(),
                }
            })
        };

        let extractor = Extractor::pooled(handler);
        let started = Instant::now();
        let queued = (0..JOBS)
            .filter(|index| extractor.request(candidate(*index)))
            .count();
        assert_eq!(queued, JOBS, "every poster must reach the pool");

        let events = drain_until(&extractor, JOBS);
        let elapsed = started.elapsed();
        assert_eq!(events.len(), JOBS, "the pool must report every poster");

        let peak = high_water.load(std::sync::atomic::Ordering::SeqCst);
        assert!(
            peak >= 2,
            "a pool of {POSTER_WORKERS} drew at most {peak} poster(s) at once"
        );
        assert!(
            peak <= POSTER_WORKERS,
            "the pool ran {peak} workers, above its bound of {POSTER_WORKERS}"
        );
        assert!(
            (2..=4).contains(&POSTER_WORKERS),
            "the owner's laptop must not be flooded by {POSTER_WORKERS} decoders"
        );

        let serial = JOB_COST * JOBS as u32;
        assert!(
            elapsed < serial,
            "{JOBS} posters took {elapsed:?}, no better than one at a time ({serial:?})"
        );
    }

    #[test]
    fn a_pooled_poster_reports_its_own_work_even_when_jobs_overlap() {
        let handler: Handler = Arc::new(|candidate: &ArtworkCandidate| ArtworkEvent::Failed {
            work_id: candidate.work_id.clone(),
            reason: "no frame".to_string(),
        });
        let extractor = Extractor::pooled(handler);
        for index in 0..5 {
            assert!(extractor.request(candidate(index)));
        }
        let events = drain_until(&extractor, 5);
        let mut seen: Vec<String> = events
            .into_iter()
            .map(|event| match event {
                ArtworkEvent::Stored { work_id } | ArtworkEvent::Failed { work_id, .. } => work_id,
            })
            .collect();
        seen.sort();
        assert_eq!(
            seen,
            (0..5)
                .map(|index| format!("work-{index}"))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_full_queue_refuses_rather_than_growing_without_bound() {
        let handler: Handler = Arc::new(|_candidate: &ArtworkCandidate| {
            std::thread::sleep(Duration::from_secs(5));
            ArtworkEvent::Failed {
                work_id: "held".to_string(),
                reason: "held".to_string(),
            }
        });
        let extractor = Extractor::pooled(handler);
        let accepted = (0..QUEUE_DEPTH + POSTER_WORKERS + 8)
            .filter(|index| extractor.request(candidate(*index)))
            .count();
        assert!(
            accepted <= QUEUE_DEPTH + POSTER_WORKERS,
            "{accepted} requests were accepted, above the bound of {}",
            QUEUE_DEPTH + POSTER_WORKERS
        );
        assert!(accepted >= QUEUE_DEPTH, "the queue refused work too early");
    }
}
