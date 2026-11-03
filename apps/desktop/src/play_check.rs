use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

pub const REPORT_ENV: &str = "SCREENROOM_PLAY_CHECK";
pub const MEDIA_ENV: &str = "SCREENROOM_PLAY_CHECK_MEDIA";
pub const SECONDS_ENV: &str = "SCREENROOM_PLAY_CHECK_SECONDS";
pub const WINDOW_ENV: &str = "SCREENROOM_WINDOW";
pub const FOREGROUND_ENV: &str = "SCREENROOM_ALLOW_FOREGROUND";
pub const DEFAULT_SECONDS: f64 = 15.0;
pub const MIN_SECONDS: f64 = 5.0;
pub const MAX_SECONDS: f64 = 300.0;

pub fn requested() -> bool {
    std::env::var_os(REPORT_ENV).is_some()
}

pub fn visible_requested() -> bool {
    std::env::var(WINDOW_ENV).is_ok_and(|value| value == "visible")
        && std::env::var(FOREGROUND_ENV).is_ok_and(|value| value == "1")
}

pub fn requests_hidden_window() -> bool {
    if std::env::var(WINDOW_ENV).is_ok_and(|value| value == "hidden") {
        return true;
    }
    (requested() || crate::quit::check_requested() || crate::quit::terminate_check_requested())
        && !visible_requested()
}

pub fn startup_error() -> Option<String> {
    if !requested() {
        return None;
    }
    match resolve_environment() {
        Ok(Some(_)) => None,
        Ok(None) => None,
        Err(reason) => Some(reason),
    }
}

fn resolve_environment() -> Result<Option<Config>, String> {
    Config::resolve(
        std::env::var_os(REPORT_ENV).map(PathBuf::from),
        std::env::var_os(MEDIA_ENV).map(PathBuf::from),
        std::env::var(SECONDS_ENV).ok(),
    )
}

static UPLOADS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub fn note_upload() {
    UPLOADS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

static IMPORTS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub fn note_import() {
    IMPORTS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

pub fn imports() -> u64 {
    IMPORTS.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn uploads() -> u64 {
    UPLOADS.load(std::sync::atomic::Ordering::Relaxed)
}

static DRAWS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub fn note_draw() {
    DRAWS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

pub fn draws() -> u64 {
    DRAWS.load(std::sync::atomic::Ordering::Relaxed)
}

#[derive(Debug, Clone)]
pub struct Config {
    report: PathBuf,
    media: PathBuf,
    seconds: f64,
}

impl Config {
    pub fn from_environment() -> Option<Self> {
        match resolve_environment() {
            Ok(Some(config)) => Some(config),
            Ok(None) => None,
            Err(reason) => {
                eprintln!("play check disabled: {reason}");
                None
            }
        }
    }

    fn resolve(
        report: Option<PathBuf>,
        media: Option<PathBuf>,
        seconds: Option<String>,
    ) -> Result<Option<Self>, String> {
        let Some(report) = report else {
            if media.is_some() {
                return Err(format!("{MEDIA_ENV} needs {REPORT_ENV} as well"));
            }
            return Ok(None);
        };
        let media = media.ok_or_else(|| format!("{REPORT_ENV} needs {MEDIA_ENV}"))?;
        let seconds = match seconds {
            Some(value) => value
                .parse::<f64>()
                .map_err(|_| format!("{SECONDS_ENV} is not a number: {value:?}"))?,
            None => DEFAULT_SECONDS,
        };
        if !seconds.is_finite() || seconds <= 0.0 {
            return Err(format!(
                "{SECONDS_ENV} must be a positive number of seconds, got {seconds}"
            ));
        }
        if !media.is_absolute() {
            return Err(format!(
                "{MEDIA_ENV} must be an absolute path: {}",
                media.display()
            ));
        }
        if !media.is_file() {
            return Err(format!(
                "{MEDIA_ENV} is not a readable file: {}",
                media.display()
            ));
        }
        let report = if report.is_absolute() {
            report
        } else {
            std::env::current_dir()
                .map_err(|error| format!("there is no working directory: {error}"))?
                .join(report)
        };
        if let Some(folder) = media.parent()
            && report.starts_with(folder)
        {
            return Err(format!(
                "{REPORT_ENV} must be outside the media folder {}",
                folder.display()
            ));
        }
        Ok(Some(Self {
            report,
            media,
            seconds: seconds.clamp(MIN_SECONDS, MAX_SECONDS),
        }))
    }

    pub fn report(&self) -> &Path {
        &self.report
    }

    pub fn media(&self) -> &Path {
        &self.media
    }

    pub fn seconds(&self) -> f64 {
        self.seconds
    }

    fn samples_path(&self) -> PathBuf {
        let mut name = self.report.as_os_str().to_owned();
        name.push(".samples.csv");
        PathBuf::from(name)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Sample {
    pub at: f64,
    pub cpu: f64,
    pub rss_kb: u64,
    pub decoded: u64,
    pub delivered: u64,
    pub uploaded: u64,
}

pub struct Run {
    config: Config,
    started: Option<Instant>,
    first_delivered: Option<Instant>,
    last_delivered: Option<u64>,
    last_change: Option<Instant>,
    gaps_ms: Vec<f64>,
    ticks: u64,
    samples: Vec<Sample>,
    next_sample: Option<Instant>,
    rendered: (u32, u32),
    visible: bool,
    drawn_frames: u64,

    reclaimed_while_reading: u64,
    drawn_seen: u64,
    sampler: Option<Sampler>,
    failure: Option<String>,
}

const MAX_GAPS: usize = 20_000;

pub struct Sampler {
    reading: std::sync::Arc<std::sync::Mutex<Option<(f64, u64)>>>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Sampler {
    pub fn start(interval: Duration) -> Self {
        let reading = std::sync::Arc::new(std::sync::Mutex::new(None));
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let pid = std::process::id();
        let shared = std::sync::Arc::clone(&reading);
        let flag = std::sync::Arc::clone(&stop);
        let thread = std::thread::spawn(move || {
            loop {
                if let Some(value) = sample_process(pid)
                    && let Ok(mut slot) = shared.lock()
                {
                    *slot = Some(value);
                }
                for _ in 0..(interval.as_millis() / 25).max(1) {
                    if flag.load(std::sync::atomic::Ordering::Relaxed) {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(25));
                }
            }
        });
        Self {
            reading,
            stop,
            thread: Some(thread),
        }
    }

    pub fn latest(&self) -> Option<(f64, u64)> {
        self.reading.lock().ok().and_then(|slot| *slot)
    }
}

impl Drop for Sampler {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Run {
    pub fn from_environment() -> Option<Self> {
        Config::from_environment().map(Self::new)
    }

    pub fn new(config: Config) -> Self {
        Self {
            config,
            started: None,
            first_delivered: None,
            last_delivered: None,
            last_change: None,
            gaps_ms: Vec::new(),
            ticks: 0,
            samples: Vec::new(),
            next_sample: None,
            rendered: (0, 0),
            visible: visible_requested(),
            drawn_frames: 0,
            reclaimed_while_reading: 0,
            drawn_seen: 0,
            sampler: None,
            failure: None,
        }
    }

    pub fn note_window(&mut self, visible: bool) {
        self.visible = visible;
    }

    pub fn note_reclaimed(&mut self, total: u64) {
        self.reclaimed_while_reading = total;
    }

    pub fn note_drawn(&mut self, total: u64, _now: Instant) {
        if self.started.is_none() {
            return;
        }
        if total <= self.drawn_seen {
            return;
        }
        let delta = total - self.drawn_seen;
        if self.drawn_seen == 0 {
            self.drawn_seen = total;
            return;
        }
        self.drawn_seen = total;
        self.drawn_frames += delta;
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn started(&self) -> bool {
        self.started.is_some()
    }

    pub fn begin(&mut self, now: Instant) {
        self.started = Some(now);
        self.last_change = Some(now);
        self.next_sample = Some(now);
        if self.sampler.is_none() {
            self.sampler = Some(Sampler::start(Duration::from_secs(1)));
        }
    }

    pub fn fail(&mut self, reason: String) {
        if self.failure.is_none() {
            self.failure = Some(reason);
        }
    }

    pub fn failure(&self) -> Option<&str> {
        self.failure.as_deref()
    }

    pub fn note_size(&mut self, size: (u32, u32)) {
        self.rendered = size;
    }

    pub fn note_tick(&mut self) {
        self.ticks += 1;
    }
    pub fn note(&mut self, now: Instant, delivered: Option<u64>, uploaded: u64) {
        let Some(delivered) = delivered else {
            return;
        };
        let _ = uploaded;
        if self.last_delivered == Some(delivered) {
            return;
        }
        if self.last_delivered.is_none() {
            self.first_delivered = Some(now);
            self.last_delivered = Some(delivered);
            self.last_change = Some(now);
            return;
        }
        if let Some(previous) = self.last_change
            && self.gaps_ms.len() < MAX_GAPS
        {
            self.gaps_ms
                .push(now.duration_since(previous).as_secs_f64() * 1_000.0);
        }
        self.last_delivered = Some(delivered);
        self.last_change = Some(now);
    }

    pub fn sample_due(&self, now: Instant) -> bool {
        self.next_sample.is_some_and(|at| now >= at)
    }

    pub fn sample(&mut self, now: Instant, counters: (u64, u64, u64)) {
        let (cpu, rss_kb) = self
            .sampler
            .as_ref()
            .and_then(Sampler::latest)
            .unwrap_or((0.0, 0));
        let at = self
            .started
            .map(|started| now.duration_since(started).as_secs_f64())
            .unwrap_or(0.0);
        self.samples.push(Sample {
            at,
            cpu,
            rss_kb,
            decoded: counters.0,
            delivered: counters.1,
            uploaded: counters.2,
        });
        self.next_sample = Some(now + Duration::from_secs(1));
    }

    pub fn elapsed(&self, now: Instant) -> f64 {
        self.started
            .map(|started| now.duration_since(started).as_secs_f64())
            .unwrap_or(0.0)
    }

    pub fn finished(&self, now: Instant) -> bool {
        self.failure.is_some() || self.elapsed(now) >= self.config.seconds
    }

    pub fn report(&self, now: Instant) -> Result<Vec<PathBuf>, String> {
        let summary = self.summary_path();
        let samples = self.config.samples_path();
        std::fs::write(&summary, self.summary_rows(now))
            .map_err(|error| format!("could not write {}: {error}", summary.display()))?;
        std::fs::write(&samples, sample_rows(&self.samples))
            .map_err(|error| format!("could not write {}: {error}", samples.display()))?;
        Ok(vec![summary, samples])
    }

    fn summary_path(&self) -> PathBuf {
        self.config.report().to_path_buf()
    }

    fn summary_rows(&self, now: Instant) -> String {
        let elapsed = self.elapsed(now).max(0.001);
        let startup_ms = self
            .first_delivered
            .map(|first| {
                self.started
                    .map(|started| first.duration_since(started).as_secs_f64() * 1_000.0)
                    .unwrap_or(0.0)
            })
            .unwrap_or(0.0);
        let measured = self
            .first_delivered
            .map(|first| now.duration_since(first).as_secs_f64())
            .unwrap_or(elapsed)
            .max(0.001);
        let delivered_until = self.last_change.unwrap_or(now);
        let window = self
            .first_delivered
            .map(|first| delivered_until.duration_since(first).as_secs_f64())
            .unwrap_or(elapsed)
            .max(0.001);
        let first = self.samples.first().copied();
        let last = self.samples.last().copied();
        let decoded = last.map(|sample| sample.decoded).unwrap_or(0);
        let delivered = last.map(|sample| sample.delivered).unwrap_or(0);
        let uploaded = last.map(|sample| sample.uploaded).unwrap_or(0);
        let delivered_delta = match (first, last) {
            (Some(first), Some(last)) => last.delivered.saturating_sub(first.delivered),
            _ => delivered,
        };
        let decoded_delta = match (first, last) {
            (Some(first), Some(last)) => last.decoded.saturating_sub(first.decoded),
            _ => decoded,
        };
        let uploaded_delta = match (first, last) {
            (Some(first), Some(last)) => last.uploaded.saturating_sub(first.uploaded),
            _ => uploaded,
        };
        let cpu: Vec<f64> = self.samples.iter().map(|sample| sample.cpu).collect();
        let rss: Vec<u64> = self.samples.iter().map(|sample| sample.rss_kb).collect();
        let gaps = &self.gaps_ms;
        let mut rows: Vec<(String, String)> = Vec::new();
        let mut push = |key: &str, value: String| rows.push((key.to_string(), value));
        push("media", self.config.media().display().to_string());
        push("sampled_seconds", format!("{elapsed:.3}"));
        push("measured_seconds", format!("{measured:.3}"));
        push("delivering_seconds", format!("{window:.3}"));
        push("startup_ms", format!("{startup_ms:.1}"));
        push(
            "ticks_per_second",
            format!("{:.1}", self.ticks as f64 / window),
        );
        push("requested_seconds", format!("{:.3}", self.config.seconds()));
        push("render_width", self.rendered.0.to_string());
        push("render_height", self.rendered.1.to_string());
        push(
            "decoded_fps",
            format!("{:.3}", decoded_delta as f64 / window),
        );
        push(
            "delivered_fps",
            format!("{:.3}", delivered_delta as f64 / window),
        );
        push(
            "uploaded_fps",
            format!("{:.3}", uploaded_delta as f64 / window),
        );
        push("delivered_total", delivered_delta.to_string());
        push("uploaded_total", uploaded_delta.to_string());
        push("imported_total", imports().to_string());
        push(
            "reclaimed_while_reading",
            self.reclaimed_while_reading.to_string(),
        );
        push("imported_fps", format!("{:.3}", imports() as f64 / window));
        push(
            "picture_surface",
            match crate::video_surface::target_format() {
                Some(format) => format!("{format:?}"),
                None => "none".to_string(),
            },
        );
        push(
            "worst_gap_ms",
            format!("{:.3}", gaps.iter().copied().fold(0.0_f64, f64::max)),
        );
        push("p95_gap_ms", format!("{:.3}", percentile(gaps, 0.95)));
        push("frame_gaps", gaps.len().to_string());
        push("cpu_mean_percent", format!("{:.2}", mean(&cpu)));
        push(
            "cpu_max_percent",
            format!("{:.2}", cpu.iter().copied().fold(0.0_f64, f64::max)),
        );
        push("rss_mean_kb", format!("{:.0}", mean_u64(&rss)));
        push(
            "rss_max_kb",
            rss.iter().copied().max().unwrap_or(0).to_string(),
        );
        let drawn_fps = self.drawn_frames as f64 / window;
        push("drawn_fps", format!("{drawn_fps:.3}"));
        push("drawn_total", self.drawn_frames.to_string());
        if self.visible && self.drawn_frames > 0 {
            push("presented_fps", format!("{drawn_fps:.3}"));
            push("presented_frames", self.drawn_frames.to_string());
            push(
                "presented_note",
                "the window is visible, so every redraw the compositor asked for was drawn and presented"
                    .to_string(),
            );
        } else {
            push("presented_fps", "unavailable".to_string());
            push("presented_frames", "unavailable".to_string());
            push(
                "presented_note",
                if self.visible {
                    "the window was visible but the compositor asked for no redraw, so nothing was displayed"
                        .to_string()
                } else {
                    "the window is hidden, so the compositor asked for no redraw and nothing was displayed; count uploaded frames instead"
                        .to_string()
                },
            );
        }
        push(
            "gpu_note",
            "uploaded counts queue.write_texture calls observed by video_surface::prepare; drawn counts RedrawRequested events from iced::window::frames"
                .to_string(),
        );
        push(
            "failure",
            self.failure.clone().unwrap_or_else(|| {
                if self.first_delivered.is_none() {
                    "no decoded frame arrived".to_string()
                } else {
                    "none".to_string()
                }
            }),
        );
        summary_csv(&rows)
    }
}

pub fn sample_process(pid: u32) -> Option<(f64, u64)> {
    let output = Command::new("ps")
        .args(["-o", "%cpu=,rss=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    let text = String::from_utf8(output.stdout).ok()?;
    let mut values = text.split_whitespace();
    Some((values.next()?.parse().ok()?, values.next()?.parse().ok()?))
}

pub fn percentile(values: &[f64], fraction: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
    let index = ((sorted.len() - 1) as f64 * fraction.clamp(0.0, 1.0)).round() as usize;
    sorted[index]
}

pub fn mean(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.iter().sum::<f64>() / values.len() as f64
}

pub fn mean_u64(values: &[u64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.iter().sum::<u64>() as f64 / values.len() as f64
}

pub fn summary_csv(rows: &[(String, String)]) -> String {
    let mut out = String::from("key,value\n");
    for (key, value) in rows {
        out.push_str(&quote(key));
        out.push(',');
        out.push_str(&quote(value));
        out.push('\n');
    }
    out
}

pub fn sample_rows(samples: &[Sample]) -> String {
    let mut out =
        String::from("seconds,cpu_percent,rss_kb,decoded_total,delivered_total,uploaded_total\n");
    for sample in samples {
        out.push_str(&format!(
            "{:.3},{:.2},{},{},{},{}\n",
            sample.at, sample.cpu, sample.rss_kb, sample.decoded, sample.delivered, sample.uploaded
        ));
    }
    out
}

fn quote(value: &str) -> String {
    if value.contains([',', '"', '\n']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Case {
        media_dir: tempfile::TempDir,
        report_dir: tempfile::TempDir,
        media: PathBuf,
    }

    impl Case {
        fn new() -> Self {
            let media_dir = tempfile::TempDir::new().expect("media dir");
            let media = media_dir.path().join("clip.mp4");
            std::fs::write(&media, b"not really a video").expect("media");
            let report_dir = tempfile::TempDir::new().expect("report dir");
            Self {
                media_dir,
                report_dir,
                media,
            }
        }

        fn report(&self) -> PathBuf {
            self.report_dir.path().join("report.csv")
        }
    }

    #[test]
    fn a_missing_report_variable_means_no_check_at_all() {
        assert!(Config::resolve(None, None, None).expect("fine").is_none());
    }

    #[test]
    fn media_without_a_report_path_is_refused_rather_than_measured_silently() {
        let case = Case::new();
        let error = Config::resolve(None, Some(case.media.clone()), None).expect_err("refused");
        assert!(error.contains(MEDIA_ENV));
    }

    #[test]
    fn a_relative_or_missing_media_path_is_refused() {
        let error = Config::resolve(
            Some(PathBuf::from("/tmp/report.csv")),
            Some(PathBuf::from("clip.mp4")),
            None,
        )
        .expect_err("relative media is refused");
        assert!(error.contains("absolute"));

        let error = Config::resolve(
            Some(PathBuf::from("/tmp/report.csv")),
            Some(PathBuf::from("/definitely/not/here.mp4")),
            None,
        )
        .expect_err("missing media is refused");
        assert!(error.contains("readable"));
    }

    #[test]
    fn a_report_inside_the_media_folder_is_refused() {
        let case = Case::new();
        let error = Config::resolve(
            Some(case.media_dir.path().join("report.csv")),
            Some(case.media.clone()),
            None,
        )
        .expect_err("the report may not land in the media folder");
        assert!(error.contains("outside the media folder"));
    }

    #[test]
    fn a_report_beside_the_media_folder_is_allowed() {
        let case = Case::new();
        let report = case.report();
        let config = Config::resolve(Some(report.clone()), Some(case.media.clone()), None)
            .expect("valid")
            .expect("configured");
        assert_eq!(config.report(), report);
        assert!((config.seconds() - DEFAULT_SECONDS).abs() < 1e-9);
    }

    #[test]
    fn the_duration_is_bounded_and_rejects_nonsense() {
        let case = Case::new();
        let media = case.media.clone();
        let report = case.report();
        let short = Config::resolve(Some(report.clone()), Some(media.clone()), Some("1".into()))
            .expect("valid")
            .expect("configured");
        assert!((short.seconds() - MIN_SECONDS).abs() < 1e-9);
        let long = Config::resolve(
            Some(report.clone()),
            Some(media.clone()),
            Some("9999".into()),
        )
        .expect("valid")
        .expect("configured");
        assert!((long.seconds() - MAX_SECONDS).abs() < 1e-9);
        assert!(
            Config::resolve(
                Some(report.clone()),
                Some(media.clone()),
                Some("soon".into())
            )
            .is_err()
        );
        assert!(Config::resolve(Some(report), Some(media), Some("-4".into())).is_err());
    }

    #[test]
    fn percentiles_and_means_report_the_expected_shape() {
        assert_eq!(percentile(&[], 0.95), 0.0);
        assert_eq!(percentile(&[41.0], 0.95), 41.0);
        let values: Vec<f64> = (1..=100).map(f64::from).collect();
        assert!((percentile(&values, 0.95) - 95.0).abs() < 1.0);
        assert!((mean(&values) - 50.5).abs() < 1e-9);
        assert!((mean_u64(&[10, 20]) - 15.0).abs() < 1e-9);
    }

    fn value(summary: &str, key: &str) -> f64 {
        summary
            .lines()
            .find_map(|line| line.strip_prefix(&format!("{key},")))
            .and_then(|rest| rest.trim_matches('"').parse().ok())
            .unwrap_or_else(|| panic!("no {key} in {summary}"))
    }

    fn run_with(samples: Vec<Sample>, gaps: Vec<f64>) -> Run {
        let case = Case::new();
        let config = Config::resolve(
            Some(case.report()),
            Some(case.media.clone()),
            Some("10".into()),
        )
        .expect("valid")
        .expect("configured");
        let mut run = Run::new(config);
        run.started = Some(Instant::now() - Duration::from_secs(10));
        run.samples = samples;
        run.gaps_ms = gaps;
        run.first_delivered = Some(Instant::now() - Duration::from_secs(10));
        run.rendered = (3840, 2160);
        run
    }

    #[test]
    fn the_summary_separates_decoded_delivered_and_uploaded_rates() {
        let samples = vec![
            Sample {
                at: 0.0,
                cpu: 100.0,
                rss_kb: 100_000,
                decoded: 0,
                delivered: 0,
                uploaded: 0,
            },
            Sample {
                at: 10.0,
                cpu: 200.0,
                rss_kb: 200_000,
                decoded: 240,
                delivered: 232,
                uploaded: 0,
            },
        ];
        let summary = run_with(samples, vec![41.0, 42.0, 300.0]).summary_rows(Instant::now());
        assert!(
            (value(&summary, "decoded_fps") - 24.0).abs() < 0.05,
            "{summary}"
        );
        assert!(
            (value(&summary, "delivered_fps") - 23.2).abs() < 0.05,
            "{summary}"
        );
        assert_eq!(value(&summary, "uploaded_fps"), 0.0);
        assert_eq!(value(&summary, "worst_gap_ms"), 300.0);
        assert!(
            summary.contains("presented_frames,unavailable"),
            "{summary}"
        );
        assert!(summary.contains("render_width,3840"), "{summary}");
        assert_eq!(value(&summary, "cpu_max_percent"), 200.0);
    }

    #[test]
    fn a_hidden_window_is_never_reported_as_presenting_frames() {
        let summary = run_with(Vec::new(), Vec::new()).summary_rows(Instant::now());
        assert!(!summary.contains("presented_frames,0"), "{summary}");
        assert!(
            summary.contains("presented_frames,unavailable"),
            "a hidden window must not claim a displayed frame: {summary}"
        );
        assert!(
            summary.contains("no redraw and nothing was displayed"),
            "the report must say why presented frames are unknown: {summary}"
        );
    }

    #[test]
    fn a_visible_window_reports_the_frames_it_presented() {
        let mut run = run_with(Vec::new(), Vec::new());
        run.note_window(true);
        let start = run.started.expect("the run is under way");
        run.note_drawn(100, start);
        run.note_drawn(124, start + Duration::from_secs(1));
        let summary = run.summary_rows(Instant::now());
        assert!(summary.contains("presented_frames,24"), "{summary}");
        assert!(
            !summary.contains("presented_fps,unavailable"),
            "a visible window reports a rate instead of refusing to: {summary}"
        );
        assert!(
            (value(&summary, "presented_fps") - 2.4).abs() < 0.05,
            "{summary}"
        );
        assert!(summary.contains("drawn_total,24"), "{summary}");
    }

    #[test]
    fn a_visible_window_that_was_never_redrawn_still_reports_nothing_displayed() {
        let mut run = run_with(Vec::new(), Vec::new());
        run.note_window(true);
        let summary = run.summary_rows(Instant::now());
        assert!(
            summary.contains("presented_frames,unavailable"),
            "{summary}"
        );
        assert!(summary.contains("drawn_fps,0.000"), "{summary}");
    }

    #[test]
    fn draws_before_the_run_began_are_baselined_and_a_flat_total_adds_nothing() {
        let mut run = run_with(Vec::new(), Vec::new());
        run.note_window(true);
        let start = run.started.expect("the run is under way");
        run.note_drawn(7, start);
        assert_eq!(run.drawn_frames, 0, "the run starts from its own baseline");
        run.note_drawn(7, start + Duration::from_secs(1));
        assert_eq!(
            run.drawn_frames, 0,
            "a draw that did not happen counts as none"
        );
        run.note_drawn(10, start + Duration::from_secs(2));
        assert_eq!(run.drawn_frames, 3);
    }

    #[test]
    fn a_failure_is_written_into_the_summary_rather_than_hidden() {
        let mut run = run_with(Vec::new(), Vec::new());
        run.fail("the player could not start".to_string());
        let summary = run.summary_rows(Instant::now());
        assert!(
            summary.contains("failure,the player could not start"),
            "{summary}"
        );
    }

    #[test]
    fn values_with_commas_are_quoted_so_the_csv_stays_parseable() {
        let rows = vec![("media".to_string(), "/a,b/clip.mp4".to_string())];
        assert_eq!(summary_csv(&rows), "key,value\nmedia,\"/a,b/clip.mp4\"\n");
    }

    #[test]
    fn the_samples_file_has_a_header_even_when_nothing_was_sampled() {
        assert_eq!(
            sample_rows(&[]),
            "seconds,cpu_percent,rss_kb,decoded_total,delivered_total,uploaded_total\n"
        );
    }

    #[test]
    fn a_new_frame_records_one_gap_and_repeats_record_none() {
        let mut run = run_with(Vec::new(), Vec::new());
        let now = Instant::now();
        run.note(now, Some(1), 0);
        assert!(run.gaps_ms.is_empty());
        run.note(now, Some(1), 0);
        assert!(run.gaps_ms.is_empty());
        run.note(now + Duration::from_millis(41), Some(2), 0);
        assert_eq!(run.gaps_ms.len(), 1);
    }

    #[test]
    fn ticks_are_counted_so_a_throttled_loop_can_be_told_apart_from_a_slow_decoder() {
        let mut run = run_with(Vec::new(), Vec::new());
        run.ticks = 0;
        run.note_tick();
        run.note_tick();
        run.note_tick();
        let summary = run.summary_rows(Instant::now());
        assert!(
            (value(&summary, "ticks_per_second") - 0.3).abs() < 0.05,
            "{summary}"
        );
    }

    #[test]
    fn the_first_frame_opens_the_window_instead_of_becoming_one_long_gap() {
        let mut run = run_with(Vec::new(), Vec::new());
        run.first_delivered = None;
        let start = Instant::now();
        run.begin(start);
        run.note(start + Duration::from_secs(4), Some(7), 0);
        assert!(run.gaps_ms.is_empty());
        let summary = run.summary_rows(start + Duration::from_secs(14));
        assert!(summary.contains("startup_ms,4000.0"), "{summary}");
        assert!(
            (value(&summary, "measured_seconds") - 10.0).abs() < 0.05,
            "{summary}"
        );
    }

    #[test]
    fn the_run_finishes_after_the_requested_duration() {
        let mut run = run_with(Vec::new(), Vec::new());
        let now = Instant::now();
        run.begin(now);
        assert!(!run.finished(now));
        assert!(run.finished(now + Duration::from_secs(11)));
    }

    #[test]
    fn a_report_is_written_to_the_requested_path_with_its_samples() {
        let case = Case::new();
        let report = case.report();
        let config = Config::resolve(
            Some(report.clone()),
            Some(case.media.clone()),
            Some("5".into()),
        )
        .expect("valid")
        .expect("configured");
        let mut run = Run::new(config);
        run.begin(Instant::now());
        let written = run.report(Instant::now()).expect("written");
        assert_eq!(written[0], report);
        assert!(report.is_file());
        assert!(
            written[1]
                .to_string_lossy()
                .ends_with("report.csv.samples.csv")
        );
    }
}
