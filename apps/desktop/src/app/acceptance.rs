use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::*;
use crate::playback::PlaybackTracker;
use crate::ui::screens::settings::Section;

const TICK: Duration = Duration::from_millis(10);

fn player_ready() -> bool {
    let capability = screenroom_player::capability();
    if !capability.available {
        eprintln!("skipped: no libmpv on this host, so playback cannot be observed");
    }
    capability.available
}

struct Media {
    _directory: tempfile::TempDir,
    root: PathBuf,
}

impl Media {
    fn new() -> Self {
        let directory = tempfile::TempDir::new().expect("temp dir");
        let root = directory.path().join("media");
        std::fs::create_dir_all(&root).expect("media root");
        Self {
            _directory: directory,
            root,
        }
    }

    fn path(&self, relative: &str) -> PathBuf {
        let path = self.root.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("media folder");
        }
        path
    }

    fn placeholders(&self, relative_paths: &[String]) {
        for relative in relative_paths {
            let path = self.path(relative);
            std::fs::write(&path, b"not really a video").expect("placeholder");
            settle(&path);
        }
    }
}

fn settle(path: &Path) {
    let file = std::fs::File::options()
        .write(true)
        .open(path)
        .expect("open to backdate");
    file.set_modified(std::time::SystemTime::now() - Duration::from_secs(3_600))
        .expect("backdate");
}

fn ffmpeg(args: Vec<String>) -> bool {
    let tool = crate::artwork::tool("ffmpeg");
    let mut command = std::process::Command::new(&tool);
    command.args(["-nostdin", "-hide_banner", "-loglevel", "error", "-y"]);
    command.args(&args);
    match command.output() {
        Ok(output) if output.status.success() => true,
        Ok(output) => {
            eprintln!(
                "ffmpeg failed ({}): {}",
                tool.display(),
                String::from_utf8_lossy(&output.stderr).trim()
            );
            false
        }
        Err(error) => {
            eprintln!("ffmpeg unavailable ({}): {error}", tool.display());
            false
        }
    }
}

fn clip(path: &Path, seconds: f64) -> bool {
    let fixture = format!("testsrc=size=320x180:rate=24:duration={seconds}");
    let tone = format!("sine=frequency=440:duration={seconds}");
    let encoded = ffmpeg(vec![
        "-f".into(),
        "lavfi".into(),
        "-i".into(),
        fixture,
        "-f".into(),
        "lavfi".into(),
        "-i".into(),
        tone,
        "-c:v".into(),
        "libx264".into(),
        "-preset".into(),
        "ultrafast".into(),
        "-pix_fmt".into(),
        "yuv420p".into(),
        "-c:a".into(),
        "aac".into(),
        "-shortest".into(),
        path.to_string_lossy().to_string(),
    ]);
    if encoded {
        settle(path);
    }
    encoded
}

fn black_clip(path: &Path, seconds: f64) -> bool {
    let encoded = ffmpeg(vec![
        "-f".into(),
        "lavfi".into(),
        "-i".into(),
        format!("color=c=black:size=320x180:rate=24:duration={seconds}"),
        "-f".into(),
        "lavfi".into(),
        "-i".into(),
        format!("sine=frequency=440:duration={seconds}"),
        "-c:v".into(),
        "libx264".into(),
        "-preset".into(),
        "ultrafast".into(),
        "-pix_fmt".into(),
        "yuv420p".into(),
        "-c:a".into(),
        "aac".into(),
        "-shortest".into(),
        path.to_string_lossy().to_string(),
    ]);
    if encoded {
        settle(path);
    }
    encoded
}

fn ink_in_rows_below(frame: &screenroom_player::Frame, fraction: f32) -> (usize, usize) {
    let start = (frame.height as f32 * fraction) as u32;
    frame.with_pixels(|bytes, stride| {
        let mut ink = 0;
        let mut total = 0;
        for row in start..frame.height {
            for column in 0..frame.width {
                let index = row as usize * stride + column as usize * 4;
                if index + 2 >= bytes.len() {
                    continue;
                }
                let luma = (u16::from(bytes[index])
                    + u16::from(bytes[index + 1])
                    + u16::from(bytes[index + 2]))
                    / 3;
                total += 1;
                if luma > 40 {
                    ink += 1;
                }
            }
        }
        (ink, total)
    })
}

fn multilingual_film(path: &Path, seconds: f64) -> bool {
    let Some(folder) = path.parent() else {
        return false;
    };
    let cue = |name: &str, text: &str| -> PathBuf {
        let file = folder.join(name);
        std::fs::write(
            &file,
            format!(
                "1\n00:00:00,500 --> 00:00:02,500\n{text}\n\n\
                 2\n00:00:03,000 --> 00:00:04,500\n{text} again\n"
            ),
        )
        .expect("subtitle file");
        file
    };
    let english = cue("english.srt", "English subtitle");
    let french = cue("french.srt", "Sous-titre français");

    let fixture = format!("testsrc=size=640x360:rate=24:duration={seconds}");
    let tone = format!("sine=frequency=440:duration={seconds}");
    let encoded = ffmpeg(vec![
        "-f".into(),
        "lavfi".into(),
        "-i".into(),
        fixture,
        "-f".into(),
        "lavfi".into(),
        "-i".into(),
        tone,
        "-f".into(),
        "lavfi".into(),
        "-i".into(),
        format!("sine=frequency=880:duration={seconds}"),
        "-i".into(),
        english.to_string_lossy().to_string(),
        "-i".into(),
        french.to_string_lossy().to_string(),
        "-map".into(),
        "0:v".into(),
        "-map".into(),
        "1:a".into(),
        "-map".into(),
        "2:a".into(),
        "-map".into(),
        "3:s".into(),
        "-map".into(),
        "4:s".into(),
        "-metadata:s:a:0".into(),
        "language=eng".into(),
        "-metadata:s:a:1".into(),
        "language=fre".into(),
        "-metadata:s:s:0".into(),
        "language=eng".into(),
        "-metadata:s:s:1".into(),
        "language=fre".into(),
        "-c:v".into(),
        "libx264".into(),
        "-preset".into(),
        "ultrafast".into(),
        "-pix_fmt".into(),
        "yuv420p".into(),
        "-c:a".into(),
        "aac".into(),
        "-c:s".into(),
        "srt".into(),
        path.to_string_lossy().to_string(),
    ]);
    if encoded {
        settle(path);
    }
    encoded
}

fn anamorphic_film(path: &Path) -> bool {
    let encoded = ffmpeg(vec![
        "-f".into(),
        "lavfi".into(),
        "-i".into(),
        "testsrc=size=1920x1080:rate=24:duration=2".into(),
        "-f".into(),
        "lavfi".into(),
        "-i".into(),
        "sine=frequency=440:duration=2".into(),
        "-vf".into(),
        "setsar=9/8".into(),
        "-c:v".into(),
        "libx264".into(),
        "-preset".into(),
        "ultrafast".into(),
        "-pix_fmt".into(),
        "yuv420p".into(),
        "-c:a".into(),
        "aac".into(),
        "-shortest".into(),
        path.to_string_lossy().to_string(),
    ]);
    if encoded {
        settle(path);
    }
    encoded
}

fn settle_jobs(app: &mut App) {
    for _ in 0..600 {
        app.drain_backup();
        if app.backup_job.is_none() && app.restore_job.is_none() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("a backup or a restore did not finish within six seconds");
}

fn fresh_app() -> App {
    let library = Library::open_in_memory().expect("library");
    App::with_library(library, None, "acceptance".into())
}

fn app_on(database: &Path) -> App {
    App::with_library(
        Library::open(database).expect("open library"),
        None,
        "acceptance".into(),
    )
}

fn scan_folder(app: &mut App, root: &Path) {
    app.new_root = root.to_string_lossy().to_string();
    let _ = app.update(Message::AddFolder);
    assert!(
        app.scan_token.is_some(),
        "granting a folder must start its scan"
    );
    let deadline = Instant::now() + Duration::from_secs(180);
    while app.scan_token.is_some() {
        assert!(Instant::now() < deadline, "the scan never finished");
        let _ = app.update(Message::Tick);
        std::thread::sleep(Duration::from_millis(5));
    }

    let _ = app.update(Message::Tick);
}

fn pump(app: &mut App, timeout: Duration, mut done: impl FnMut(&App) -> bool) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        let _ = app.update(Message::Tick);
        if done(app) {
            return true;
        }
        std::thread::sleep(TICK);
    }
    false
}

fn play(app: &mut App, file_id: &str) -> bool {
    let _ = app.update(Message::Play(file_id.to_string()));
    pump(app, Duration::from_secs(20), |app| {
        app.snapshot.loaded && app.snapshot.position > 0.0
    })
}

fn until_position(app: &mut App, seconds: f64) -> bool {
    pump(app, Duration::from_secs(30), |app| {
        app.snapshot.position >= seconds
    })
}

fn press(key: iced::keyboard::Key, modifiers: iced::keyboard::Modifiers) -> Message {
    use iced::keyboard::key::{Code, Physical};
    Message::Keyboard(iced::keyboard::Event::KeyPressed {
        key,
        modified_key: iced::keyboard::Key::Unidentified,
        physical_key: Physical::Code(Code::Escape),
        location: iced::keyboard::Location::Standard,
        modifiers,
        text: None,
        repeat: false,
    })
}

fn key(press_name: &str) -> Message {
    use iced::keyboard::Key;
    use iced::keyboard::Modifiers;
    use iced::keyboard::key::Named;
    let named = match press_name {
        "escape" => Named::Escape,
        "enter" => Named::Enter,
        "space" => Named::Space,
        "up" => Named::ArrowUp,
        "down" => Named::ArrowDown,
        "left" => Named::ArrowLeft,
        "right" => Named::ArrowRight,
        "backspace" => Named::Backspace,
        "delete" => Named::Delete,
        other => {
            return press(Key::Character(other.to_string().into()), Modifiers::empty());
        }
    };
    press(Key::Named(named), Modifiers::empty())
}

fn command_key(character: &str) -> Message {
    press(
        iced::keyboard::Key::Character(character.to_string().into()),
        iced::keyboard::Modifiers::COMMAND,
    )
}

fn stored_position(app: &App, file_id: &str) -> i64 {
    app.library
        .item(file_id, Some(&app.profile_id))
        .expect("item read")
        .expect("file still in the library")
        .position_ms
}

#[test]
fn step_1_a_first_run_scan_fills_the_grid_before_it_finishes() {
    let media = Media::new();
    let names: Vec<String> = (0..1_200)
        .map(|index| format!("Film {index:04} (2020).mkv"))
        .collect();
    media.placeholders(&names);

    let mut app = fresh_app();
    assert!(app.sources.is_empty(), "a first run starts with no folder");
    assert!(app.home.is_empty(), "and nothing to watch");

    app.new_root = media.root.to_string_lossy().to_string();
    let _ = app.update(Message::AddFolder);
    assert!(
        app.scan_token.is_some(),
        "granting a folder must start its scan"
    );

    let deadline = Instant::now() + Duration::from_secs(180);
    let mut saw_progress_row = false;
    let mut cards_while_scanning = 0_usize;
    while app.scan_token.is_some() {
        assert!(Instant::now() < deadline, "the scan never finished");
        let _ = app.update(Message::Tick);
        if app.scan_token.is_some() {
            if app
                .status
                .as_deref()
                .is_some_and(|status| status.starts_with("Scanning…"))
            {
                saw_progress_row = true;
            }
            cards_while_scanning = cards_while_scanning.max(app.browse.items.len());
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let _ = app.update(Message::Tick);

    let status = app.status.clone().unwrap_or_default();
    assert_eq!(app.error, None, "a readable folder must not fail its scan");
    assert!(
        status.starts_with("Scan finished"),
        "the scan must end by saying so, got {status:?}"
    );
    assert_eq!(app.scan_imported, 1_200, "every file was imported");
    assert_eq!(app.browse.items.len(), 24, "one full page is on screen");
    assert!(saw_progress_row, "the scan row must count up while it runs");
    assert!(
        cards_while_scanning > 0 || app.browse.items.len() == 24,
        "cards must appear before the scan finishes, or the whole page must be there \
         when it is done so quickly that no partial frame is drawn"
    );
}

#[test]
fn step_1_a_mid_scan_report_refreshes_the_catalogue_before_the_walk_ends() {
    let media = Media::new();
    media.placeholders(&[
        "First (2019).mkv".to_string(),
        "Second (2020).mkv".to_string(),
    ]);

    let mut app = fresh_app();
    scan_folder(&mut app, &media.root);
    let imported = app.browse.items.len();
    assert_eq!(imported, 2, "the fixture is in the library");

    let (sender, receiver) = std::sync::mpsc::channel::<ScanEvent>();
    app.browse.items.clear();
    app.scan_token = Some(ScanToken::new());
    app.scan_events = Some(receiver);
    app.scan_refresh_ms = 0;
    sender
        .send(ScanEvent::Progress {
            visited: 40,
            imported: 2,
            issues: 0,
            done: false,
            complete: false,
            enriched: 0,
        })
        .expect("report");
    let _ = app.update(Message::Tick);

    assert!(
        app.scan_token.is_some(),
        "the walk has not finished in this report"
    );
    assert_eq!(
        app.browse.items.len(),
        2,
        "a running scan must show what it has already imported"
    );
}

#[test]
fn step_2_a_series_is_one_card_with_posters_and_runtimes() {
    let media = Media::new();
    let episodes = [
        ("Show (2016)/Season 1/Show S01E01.mkv", 2.0),
        ("Show (2016)/Season 1/Show S01E02.mkv", 2.0),
        ("Show (2016)/Season 1/Show S01E03.mkv", 2.0),
        ("A Film (2019)/A Film (2019).mkv", 4.0),
    ];
    for (relative, seconds) in episodes {
        let path = media.path(relative);
        assert!(
            clip(&path, seconds),
            "the fixture clip must encode: {relative}"
        );
    }

    let mut app = fresh_app();

    app.background_work = true;
    scan_folder(&mut app, &media.root);
    let _ = app.update(Message::GoBrowse);
    app.ensure_artwork();
    app.start_durations();

    assert_eq!(
        app.browse.items.len(),
        2,
        "three episodes of one show are one card, plus one film"
    );
    let series = app
        .browse
        .items
        .iter()
        .find(|item| item.kind == MediaKind::Series)
        .expect("the series is one card");
    assert_eq!(series.title, "Show");
    assert_eq!(
        series.year,
        Some(2016),
        "the year comes from the show folder, because episode names never carry one"
    );
    let film = app
        .browse
        .items
        .iter()
        .find(|item| item.kind == MediaKind::Movie)
        .expect("the film is its own card");
    assert_eq!(film.year, Some(2019));

    let got_runtimes = pump(&mut app, Duration::from_secs(60), |app| {
        app.browse.items.iter().all(|item| item.duration_ms > 0)
    });
    assert!(got_runtimes, "every card must show a runtime");

    let got_posters = pump(&mut app, Duration::from_secs(60), |app| {
        app.artwork.len() >= 2
    });
    assert!(
        got_posters,
        "cards must be given stored posters; {} are cached",
        app.artwork.len()
    );
}

#[test]
fn step_3_enter_plays_the_highlighted_episode_not_the_opened_one() {
    let media = Media::new();
    for episode in 1..=3 {
        let path = media.path(&format!("Show (2016)/Season 1/Show S01E0{episode}.mkv"));
        assert!(clip(&path, 3.0), "episode fixture");
    }

    let mut app = fresh_app();
    scan_folder(&mut app, &media.root);
    let _ = app.update(Message::GoBrowse);
    let series_id = app.browse.items[0].id.clone();
    let _ = app.update(Message::OpenDetail(series_id));
    assert_eq!(app.screen, Screen::Detail);
    assert_eq!(app.detail.as_ref().expect("detail").episodes.len(), 3);

    let target = app.detail.as_ref().expect("detail").episodes[2].clone();
    assert!(
        target.relative_path.contains("S01E03"),
        "the third row is E03, got {:?}",
        target.relative_path
    );
    let _ = app.update(key("down"));
    let _ = app.update(key("down"));
    let _ = app.update(key("enter"));

    assert_eq!(app.screen, Screen::Player, "Enter starts the episode");
    let playing = app
        .active
        .as_ref()
        .map(|active| active.file_id.clone())
        .expect("a viewing is open");
    assert_eq!(
        playing, target.id,
        "Enter must play the highlighted episode, not the opened one"
    );

    if !player_ready() {
        return;
    }
    assert!(
        until_position(&mut app, 0.4),
        "the episode must actually decode and advance"
    );
    assert!(
        app.frame.is_some(),
        "a frame must be decoded for the picture"
    );
}

#[test]
fn step_4_space_pauses_and_resumes_and_the_arrows_move_ten_seconds() {
    if !player_ready() {
        return;
    }
    let media = Media::new();
    let film = media.path("Film (2020)/Film (2020).mkv");
    assert!(clip(&film, 20.0), "film fixture");

    let mut app = fresh_app();
    scan_folder(&mut app, &media.root);
    let id = app.browse.items[0].id.clone();
    assert!(play(&mut app, &id), "the film must start");
    assert!(until_position(&mut app, 2.0), "playback must advance");

    let _ = app.update(key("space"));
    assert!(
        pump(&mut app, Duration::from_secs(5), |app| {
            app.snapshot.state == screenroom_player::PlayerState::Paused
        }),
        "Space must pause, state was {:?}",
        app.snapshot.state
    );
    let paused_at = app.snapshot.position;
    app.snapshot = app.player.as_ref().expect("player").snapshot();
    assert!(
        (app.snapshot.position - paused_at).abs() < 1.0,
        "a paused playhead must hold still"
    );

    let _ = app.update(key("space"));
    assert!(
        pump(&mut app, Duration::from_secs(5), |app| {
            app.snapshot.state == screenroom_player::PlayerState::Playing
        }),
        "Space must resume, state was {:?}",
        app.snapshot.state
    );
    assert!(until_position(&mut app, 3.0), "playback resumes forward");

    let before = app.snapshot.position;
    let _ = app.update(key("right"));
    assert!(
        pump(&mut app, Duration::from_secs(8), |app| {
            app.snapshot.position >= before + 8.0
        }),
        "→ must move roughly ten seconds forward, from {before:.1}"
    );
    let forward = app.snapshot.position - before;
    assert!(
        (8.0..=12.0).contains(&forward),
        "→ moved {forward:.1} s, which is not ten"
    );

    let before = app.snapshot.position;
    let _ = app.update(key("left"));
    assert!(
        pump(&mut app, Duration::from_secs(8), |app| {
            app.snapshot.position <= before - 8.0
        }),
        "← must move roughly ten seconds back, from {before:.1}"
    );
    let backward = before - app.snapshot.position;
    assert!(
        (8.0..=12.0).contains(&backward),
        "← moved {backward:.1} s, which is not ten"
    );
}

#[test]
fn step_5_the_audio_and_subtitle_pickers_change_the_tracks() {
    if !player_ready() {
        return;
    }
    let media = Media::new();
    let film = media.path("Bilingual (2021)/Bilingual (2021).mkv");
    assert!(multilingual_film(&film, 20.0), "multilingual fixture");

    let mut app = fresh_app();
    scan_folder(&mut app, &media.root);
    let id = app.browse.items[0].id.clone();
    assert!(play(&mut app, &id), "the film must start");
    assert!(
        pump(&mut app, Duration::from_secs(15), |app| {
            app.snapshot
                .tracks
                .iter()
                .filter(|track| track.kind == TrackKind::Audio)
                .count()
                >= 2
                && app
                    .snapshot
                    .tracks
                    .iter()
                    .filter(|track| track.kind == TrackKind::Subtitle)
                    .count()
                    >= 2
        }),
        "the file's own tracks must be enumerated"
    );

    for kind in [TrackKind::Audio, TrackKind::Subtitle] {
        let options = ui::screens::player::track_options(&app.snapshot, kind);
        let offered: Vec<String> = options.iter().map(|option| option.to_string()).collect();
        assert!(
            offered.iter().any(|label| label.contains("fre")),
            "{kind:?} picker must offer the French track, offered {offered:?}"
        );
        let current = ui::screens::player::selected_track(&app.snapshot, kind, &options)
            .expect("the picker starts on a track")
            .id;

        let other = app
            .snapshot
            .tracks
            .iter()
            .find(|track| track.kind == kind && track.id != current)
            .map(|track| track.id)
            .expect("another track of this kind exists");
        let _ = app.update(Message::SelectTrack(kind, other));

        assert!(
            pump(&mut app, Duration::from_secs(5), |app| {
                app.snapshot
                    .tracks
                    .iter()
                    .any(|track| track.kind == kind && track.id == other && track.selected)
            }),
            "{kind:?} selection must be read back from the player, not assumed"
        );

        assert_eq!(app.snapshot.failure, None);
    }
}

#[test]
fn step_6_fullscreen_is_entered_left_and_survived_by_one_escape() {
    let media = Media::new();
    media.placeholders(&["Film (2020).mkv".to_string()]);

    let mut app = fresh_app();
    scan_folder(&mut app, &media.root);
    let id = app.browse.items[0].id.clone();
    let _ = app.update(Message::OpenDetail(id));
    let _ = app.update(key("enter"));
    assert_eq!(app.screen, Screen::Player);
    assert!(!app.fullscreen);

    let _ = app.update(key("f"));
    assert!(app.fullscreen, "f enters fullscreen");

    let _ = app.update(Message::ModeChanged(iced::window::Mode::Windowed));
    assert!(
        !app.fullscreen,
        "leaving fullscreen with the platform's control must be noticed"
    );

    let _ = app.update(key("f"));
    assert!(app.fullscreen);
    let _ = app.update(key("escape"));
    assert!(!app.fullscreen, "Escape leaves fullscreen first");
    assert_eq!(app.screen, Screen::Player, "and the film stays open");

    let _ = app.update(key("escape"));
    assert_ne!(
        app.screen,
        Screen::Player,
        "one Escape then exits the player"
    );
    assert!(!app.fullscreen);
}

#[test]
fn step_7_an_anamorphic_film_is_presented_at_its_display_shape() {
    if !player_ready() {
        return;
    }
    let media = Media::new();
    let film = media.path("Anamorphic (2016)/Anamorphic (2016).mkv");
    assert!(anamorphic_film(&film), "anamorphic fixture");

    let mut app = fresh_app();
    scan_folder(&mut app, &media.root);
    let id = app.browse.items[0].id.clone();
    assert!(play(&mut app, &id), "the film must start");
    assert!(
        pump(&mut app, Duration::from_secs(20), |app| app.frame.is_some()),
        "a frame must arrive"
    );

    let frame = app.frame.as_ref().expect("frame");

    assert_eq!(
        (frame.width, frame.height),
        (2160, 1080),
        "an anamorphic frame must be sized for display, not for storage"
    );
    let aspect = frame.width as f32 / frame.height as f32;
    assert!(
        (aspect - 2.0).abs() < 0.01,
        "the picture is {aspect:.3}:1, not the 2:1 its pixels describe"
    );
    assert!(
        (aspect - 16.0 / 9.0).abs() > 0.05,
        "the picture must not be presented at its storage shape"
    );
}

#[test]
fn step_9_a_finished_episode_offers_the_next_one_cancel_stops_it() {
    if !player_ready() {
        return;
    }
    let media = Media::new();
    for episode in 1..=3 {
        let path = media.path(&format!("Show (2016)/Season 1/Show S01E0{episode}.mkv"));
        assert!(clip(&path, 2.0), "episode fixture");
    }

    let mut app = fresh_app();
    scan_folder(&mut app, &media.root);
    let _ = app.update(Message::ToggleAutoplay(true));
    assert!(app.preferences.autoplay_next);

    let _ = app.update(Message::GoBrowse);
    let series_id = app.browse.items[0].id.clone();
    let _ = app.update(Message::OpenDetail(series_id));
    let second = app.detail.as_ref().expect("detail").episodes[1].clone();
    assert!(second.relative_path.contains("S01E02"));
    let _ = app.update(Message::Play(second.id.clone()));

    assert!(
        pump(&mut app, Duration::from_secs(40), |app| app
            .up_next
            .is_some()),
        "a finished episode must offer the next one"
    );
    assert!(
        app.status
            .as_deref()
            .is_some_and(|status| status.contains("Finished")),
        "the finish must be stated, status was {:?}",
        app.status
    );
    let offered = app.up_next.as_ref().expect("offer").file_id.clone();
    assert_eq!(
        app.library
            .item(&offered, Some(&app.profile_id))
            .expect("read")
            .expect("offered file")
            .relative_path,
        "Show (2016)/Season 1/Show S01E03.mkv",
        "the offer must be the next episode, not another one"
    );

    let _ = app.update(Message::CancelNext);
    assert!(app.up_next.is_none(), "Cancel withdraws the offer");
    assert_eq!(app.screen, Screen::Player, "and stays where it was");
    assert_eq!(
        app.active.as_ref().map(|active| active.file_id.clone()),
        Some(second.id.clone()),
        "the finished episode is still the open one"
    );

    let _ = app.update(Message::TogglePlay);
    assert!(
        pump(&mut app, Duration::from_secs(40), |app| app
            .up_next
            .is_some()),
        "the second viewing must offer it again"
    );
    let _ = app.update(Message::PlayNextNow);
    assert!(
        pump(&mut app, Duration::from_secs(20), |app| {
            app.active.is_some()
        }),
        "Play now must start the next episode"
    );
    assert_eq!(
        app.active
            .as_ref()
            .and_then(|active| app
                .library
                .item(&active.file_id, Some(&app.profile_id))
                .ok()
                .flatten())
            .map(|item| item.relative_path.clone()),
        Some("Show (2016)/Season 1/Show S01E03.mkv".to_string())
    );
}

#[test]
fn step_10_quitting_mid_film_and_reopening_resumes_within_a_second() {
    if !player_ready() {
        return;
    }
    let media = Media::new();
    let film = media.path("Film (2020)/Film (2020).mkv");
    assert!(clip(&film, 30.0), "film fixture");
    let directory = tempfile::TempDir::new().expect("temp dir");
    let database = directory.path().join("library.db");

    let (film_id, left_at) = {
        let mut app = app_on(&database);
        scan_folder(&mut app, &media.root);
        let id = app.browse.items[0].id.clone();
        assert!(play(&mut app, &id), "the film must start");
        assert!(until_position(&mut app, 3.0), "playback must advance");
        let left_at = app.snapshot.position;

        let _ = app.update(Message::Close);
        assert!(app.active.is_none(), "quitting closes the viewing");
        (id, left_at)
    };
    assert!(
        left_at >= 3.0,
        "the test never got far enough into the film to be meaningful"
    );

    let mut reopened = app_on(&database);
    let stored = stored_position(&reopened, &film_id) as f64 / 1_000.0;
    assert!(
        (stored - left_at).abs() <= 1.0,
        "the stored position is {stored:.1} s against {left_at:.1} s on screen"
    );

    assert!(play(&mut reopened, &film_id), "the film reopens");
    let resume = reopened
        .active
        .as_ref()
        .map(|active| active.resume_from_ms)
        .expect("viewing") as f64
        / 1_000.0;
    assert!(
        (resume - left_at).abs() <= 1.0,
        "reopening resumed at {resume:.1} s, not within a second of {left_at:.1} s"
    );
    assert!(
        pump(&mut reopened, Duration::from_secs(20), |app| {
            (app.snapshot.position - left_at).abs() <= 1.0
        }),
        "the playhead must land back where it was"
    );
}

#[test]
fn step_11_home_mid_film_stops_playback_and_keeps_the_place() {
    if !player_ready() {
        return;
    }
    let media = Media::new();
    let film = media.path("Film (2020)/Film (2020).mkv");
    assert!(clip(&film, 30.0), "film fixture");

    let mut app = fresh_app();
    scan_folder(&mut app, &media.root);
    let id = app.browse.items[0].id.clone();
    assert!(play(&mut app, &id), "the film must start");
    assert!(until_position(&mut app, 3.0), "playback must advance");
    let left_at = app.snapshot.position;
    let audio_before = app.snapshot.audio_pts;

    let _ = app.update(Message::GoHome);

    assert_eq!(app.screen, Screen::Home);
    assert!(app.active.is_none(), "the viewing session is closed");
    assert!(
        app.frame.is_none(),
        "no frame is published for the next screen"
    );
    assert_eq!(
        app.snapshot.state,
        screenroom_player::PlayerState::Idle,
        "the player is idle, so nothing is still playing out of sight"
    );

    let after = app
        .player
        .as_ref()
        .and_then(|player| player.snapshot().audio_pts);
    let still_advancing =
        matches!((audio_before, after), (Some(before), Some(now)) if now > before + 0.2);
    assert!(
        !still_advancing,
        "the audio clock must not keep running after leaving for Home"
    );

    let stored = stored_position(&app, &id) as f64 / 1_000.0;
    assert!(
        (stored - left_at).abs() <= 1.0,
        "the place is kept: stored {stored:.1} s against {left_at:.1} s"
    );
    assert!(
        app.home.iter().any(|item| item.id == id),
        "the film must be in Continue Watching on the Home screen"
    );
}

#[test]
fn step_12_a_folder_that_disappears_keeps_history_and_a_rescan_recovers_it() {
    let media = Media::new();
    let film = media.path("Film (2020)/Film (2020).mkv");
    assert!(clip(&film, 20.0), "film fixture");
    let elsewhere = media.root.with_file_name("moved");

    let mut app = fresh_app();
    scan_folder(&mut app, &media.root);
    let id = app.browse.items[0].id.clone();
    let source_id = app.browse.items[0].source_id.clone();

    if player_ready() {
        assert!(
            play(&mut app, &id),
            "the film plays while the folder is there"
        );
        assert!(until_position(&mut app, 2.0));
        let _ = app.update(Message::BackFromPlayer);
    }

    let _ = app.update(Message::OpenDetail(id.clone()));
    let _ = app.update(Message::Play(id.clone()));
    assert!(until_position(&mut app, 2.0));
    let left_at = stored_position(&app, &id);
    let _ = app.update(Message::BackFromPlayer);

    std::fs::rename(&media.root, &elsewhere).expect("move the folder");
    let _ = app.update(Message::RescanSource(source_id.clone()));
    let deadline = Instant::now() + Duration::from_secs(120);
    while app.scan_token.is_some() {
        assert!(Instant::now() < deadline, "the rescan never finished");
        let _ = app.update(Message::Tick);
        std::thread::sleep(Duration::from_millis(5));
    }
    let _ = app.update(Message::Tick);

    let item = app
        .library
        .item(&id, Some(&app.profile_id))
        .expect("read")
        .expect("the card is kept, not deleted");
    assert!(
        !item.available,
        "a folder that is gone must mark its cards unavailable"
    );
    assert_eq!(
        item.position_ms, left_at,
        "and must keep the history while it is away"
    );
    assert!(
        app.error.is_none(),
        "a missing folder is a state, not an error to shout about: {:?}",
        app.error
    );

    std::fs::rename(&elsewhere, &media.root).expect("put the folder back");
    let _ = app.update(Message::RescanSource(source_id));
    let deadline = Instant::now() + Duration::from_secs(120);
    while app.scan_token.is_some() {
        assert!(Instant::now() < deadline, "the rescan never finished");
        let _ = app.update(Message::Tick);
        std::thread::sleep(Duration::from_millis(5));
    }
    let _ = app.update(Message::Tick);
    let item = app
        .library
        .item(&id, Some(&app.profile_id))
        .expect("read")
        .expect("still one card");
    assert!(item.available, "the returned folder is readable again");
    assert_eq!(item.position_ms, left_at, "history survived the round trip");

    if player_ready() {
        assert!(play(&mut app, &id), "it plays again after the rescan");
    }
}

#[test]
fn step_13_a_restore_undoes_a_change_and_names_the_safety_copy() {
    let media = Media::new();
    media.placeholders(&["Film (2020).mkv".to_string()]);
    let directory = tempfile::TempDir::new().expect("temp dir");
    let database = directory.path().join("library.db");
    let backup = directory.path().join("backup.db");

    let mut app = app_on(&database);
    scan_folder(&mut app, &media.root);
    let _ = app.update(Message::GoBrowse);
    let item = app.browse.items[0].clone();

    let _ = app.update(Message::BackupPathChanged(
        backup.to_string_lossy().to_string(),
    ));
    let _ = app.update(Message::BackupNow);
    settle_jobs(&mut app);
    assert!(
        app.status
            .as_deref()
            .is_some_and(|status| status.starts_with("Backed up")),
        "a backup must report itself, status was {:?}",
        app.status
    );
    assert!(backup.is_file(), "the backup file exists");

    let _ = app.update(Message::OpenDetail(item.id.clone()));
    assert!(!app.detail.as_ref().expect("detail").item.favorite);
    let _ = app.update(Message::ToggleFavorite(item.work_id.clone()));
    assert!(
        app.detail.as_ref().expect("detail").item.favorite,
        "the change landed before the restore"
    );

    let _ = app.update(Message::RestorePathChanged(
        backup.to_string_lossy().to_string(),
    ));
    let _ = app.update(Message::RestoreNow);
    assert!(
        app.status
            .as_deref()
            .is_some_and(|status| status.contains("Press Restore again")),
        "a restore must ask first, status was {:?}",
        app.status
    );
    let _ = app.update(Message::RestoreNow);

    settle_jobs(&mut app);
    let status = app.status.clone().unwrap_or_default();
    assert!(
        status.starts_with("Restored"),
        "the restore must report itself, status was {status:?}"
    );
    assert!(
        status.contains("before-restore"),
        "the status must name the safety copy, it said {status:?}"
    );
    let safety = directory
        .path()
        .read_dir()
        .expect("read the library folder")
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .any(|name| name.contains("before-restore"));
    assert!(
        safety,
        "the previous library must exist on disk beside the restored one"
    );

    let restored = app
        .library
        .item(&item.id, Some(&app.profile_id))
        .expect("read")
        .expect("the film is in the restored catalogue");
    assert!(
        !restored.favorite,
        "the restore must undo a change made after the backup"
    );
}

#[test]
fn step_14_a_second_profile_survives_a_relaunch_and_removal_asks_twice() {
    let media = Media::new();
    media.placeholders(&["Film (2020).mkv".to_string()]);
    let directory = tempfile::TempDir::new().expect("temp dir");
    let database = directory.path().join("library.db");

    let guest = {
        let mut app = app_on(&database);
        scan_folder(&mut app, &media.root);
        let before = app.profiles.len();
        let _ = app.update(Message::NewProfileChanged("Guest".to_string()));
        let _ = app.update(Message::AddProfile);
        assert_eq!(app.profiles.len(), before + 1, "the profile was added");
        let guest = app.profile_id.clone();
        assert_ne!(guest, app.profiles[0].id, "and switched to it");
        guest
    };

    let mut app = app_on(&database);
    assert_eq!(
        app.profile_id, guest,
        "the app must reopen on the profile it was left on"
    );

    let _ = app.update(Message::RemoveProfile(guest.clone()));
    assert!(
        app.profiles.iter().any(|profile| profile.id == guest),
        "one press must not delete a viewer's whole history"
    );
    assert!(
        app.status
            .as_deref()
            .is_some_and(|status| status.contains("Press Remove again")),
        "and must say so, status was {:?}",
        app.status
    );

    let _ = app.update(Message::RemoveProfile(guest.clone()));
    assert!(
        !app.profiles.iter().any(|profile| profile.id == guest),
        "the second press removes it"
    );
    assert_ne!(app.profile_id, guest, "and the session moves off it");
}

#[test]
fn step_15_the_journey_is_reachable_from_the_keyboard_alone() {
    let media = Media::new();
    for index in 0..4 {
        let path = media.path(&format!("Film {index} (2020)/Film {index} (2020).mkv"));
        assert!(clip(&path, 3.0), "film fixture");
    }

    let mut app = fresh_app();
    scan_folder(&mut app, &media.root);
    assert_eq!(app.screen, Screen::Home);

    let _ = app.update(command_key("2"));
    assert_eq!(app.screen, Screen::Browse, "⌘2 reaches the library");

    for step in ["right", "right", "right", "down", "left"] {
        let _ = app.update(key(step));
        assert!(
            app.focus < app.browse.items.len(),
            "after {step} the cursor is outside the page"
        );
    }
    let focused = app.browse.items[app.focus].title.clone();
    assert!(!focused.is_empty(), "the focused card names a title");

    let _ = app.update(key("enter"));
    assert_eq!(app.screen, Screen::Detail, "Enter opens the focused card");
    if app
        .detail
        .as_ref()
        .is_some_and(|detail| !detail.episodes.is_empty())
    {
        let _ = app.update(key("down"));
    }
    let _ = app.update(key("enter"));
    assert_eq!(app.screen, Screen::Player, "Enter plays it");

    let _ = app.update(key("escape"));
    assert_eq!(app.screen, Screen::Detail, "Escape returns to the card");
    let _ = app.update(key("escape"));
    assert_eq!(app.screen, Screen::Browse, "and again to the library");
    assert!(
        app.focus < app.browse.items.len(),
        "Back returns to the card the viewer left"
    );
}

#[test]
fn settings_are_operable_without_a_pointer() {
    let mut app = fresh_app();
    let _ = app.update(command_key("3"));
    assert_eq!(app.screen, Screen::Settings, "⌘3 reaches Settings");

    let labels = app.settings_labels();
    assert!(
        labels.len() >= 4,
        "Settings must offer rows, found {}",
        labels.len()
    );

    let autoplay = labels
        .iter()
        .position(|label| label.to_lowercase().contains("next episode"))
        .expect("the autoplay row must exist");
    let before = app.preferences.autoplay_next;
    app.settings_focus = autoplay;
    let _ = app.update(key("enter"));
    assert_ne!(
        app.preferences.autoplay_next, before,
        "Enter must run the focused Settings row instead of doing nothing"
    );

    let slider = labels
        .iter()
        .position(|label| label.starts_with("Subtitle size"))
        .expect("the subtitle size slider must be drawn");
    let scale_before = app.preferences.subtitle_scale_percent;
    app.settings_focus = slider;
    let _ = app.update(key("enter"));
    assert_eq!(
        app.preferences.subtitle_scale_percent, scale_before,
        "a slider row carries no press action, so Enter must change nothing"
    );

    app.settings_focus = 0;
    let _ = app.update(key("down"));
    assert_eq!(app.settings_focus, 1, "Down moves to the next row");
    let _ = app.update(key("up"));
    assert_eq!(app.settings_focus, 0, "Up returns to the first row");
    let _ = app.update(key("up"));
    assert_eq!(app.settings_focus, 0, "the cursor never leaves the list");

    for _ in 0..24 {
        let _ = app.update(key("down"));
    }
    assert_eq!(
        app.settings_focus,
        app.settings_labels().len() - 1,
        "repeated Down stops on the last drawn row rather than running past it"
    );

    let _ = app.update(key("escape"));
    assert_eq!(app.screen, Screen::Home, "Escape still leaves Settings");
}

#[test]
fn the_settings_cursor_walks_only_the_section_on_screen() {
    let mut app = fresh_app();
    let _ = app.update(command_key("3"));
    app.settings_section = Section::Playback;

    let playback_rows = app.settings_labels().len();
    for _ in 0..24 {
        let _ = app.update(key("down"));
    }
    assert_eq!(
        app.settings_focus,
        playback_rows - 1,
        "the cursor must stop at the last row of the section on screen"
    );

    let _ = app.update(Message::SettingsSection(Section::Data));
    for _ in 0..24 {
        let _ = app.update(key("down"));
    }
    assert_eq!(
        app.settings_focus,
        app.settings_labels().len() - 1,
        "another section has its own rows and its own end"
    );
}

#[test]
fn step_16_a_conversion_writes_a_checked_copy_and_leaves_the_original_alone() {
    let media = Media::new();
    let film = media.path("A.Film.2001.mkv");
    if !clip(&film, 3.0) {
        eprintln!("skipped: ffmpeg cannot build a clip on this host");
        return;
    }
    let before = std::fs::metadata(&film).expect("source");
    let before_bytes = std::fs::read(&film).expect("read source");

    let mut app = fresh_app();
    let output = tempfile::TempDir::new().expect("output folder");
    app.conversion_folder = output.path().to_string_lossy().to_string();
    scan_folder(&mut app, &media.root);
    let file_id = app.browse.items.first().expect("one film").id.clone();

    app.settings_section = Section::Data;
    let labels: Vec<String> = app.settings_labels();
    assert!(
        labels
            .iter()
            .any(|label| label.contains("Converted copies folder")),
        "the conversion folder must be reachable from the keyboard, found {labels:?}"
    );

    let _ = app.update(Message::StartConversion(
        file_id.clone(),
        ConversionProfile::Remux,
    ));
    assert!(
        app.conversion_runner.is_some(),
        "a remux must start, got {:?}",
        app.error
    );
    assert!(
        pump(&mut app, Duration::from_secs(120), |app| {
            app.conversion_runner.is_none()
        }),
        "the conversion never finished"
    );

    let job = app
        .library
        .conversion_for_file(&file_id)
        .expect("lookup")
        .expect("the job is recorded");
    assert_eq!(job.status, ConversionStatus::Done, "{}", job.message);
    assert_eq!(
        job.progress_percent, 100,
        "a finished job reads as complete"
    );

    let copy = PathBuf::from(&job.output_path);
    assert!(copy.is_file(), "the copy must exist on disk");
    assert_ne!(copy, film, "the copy is a different file");
    let facts = crate::probe::probe(&copy).expect("the copy must be readable media");
    assert!(facts.has_video, "the copy kept its picture");
    assert!(facts.has_audio, "the copy kept its sound");
    assert!(facts.duration_ms > 0, "the copy reports a duration");

    let after = std::fs::metadata(&film).expect("source after");
    assert_eq!(
        after.len(),
        before.len(),
        "the original's size is untouched"
    );
    assert_eq!(
        after.modified().expect("mtime"),
        before.modified().expect("mtime"),
        "the original's modification time is untouched"
    );
    assert_eq!(
        std::fs::read(&film).expect("read source again"),
        before_bytes,
        "the original's bytes are untouched"
    );

    let _ = app.update(Message::StartConversion(
        file_id.clone(),
        ConversionProfile::Remux,
    ));
    assert!(
        pump(&mut app, Duration::from_secs(120), |app| {
            app.conversion_runner.is_none()
        }),
        "a second conversion must not wait forever"
    );
    let second = app
        .library
        .recent_conversions(5)
        .expect("recent")
        .into_iter()
        .filter(|job| job.file_id == file_id)
        .count();
    assert_eq!(second, 2, "each attempt is its own record");
    let latest = app
        .library
        .conversion_for_file(&file_id)
        .expect("lookup")
        .expect("job");
    assert_ne!(
        latest.output_path, job.output_path,
        "a second copy of the same film gets its own name instead of overwriting"
    );
}

#[test]
fn step_17_a_preview_is_one_delayed_decoder_and_reduced_motion_stops_it() {
    let media = Media::new();
    let film = media.path("A.Film.2001.mkv");
    if !clip(&film, 4.0) {
        eprintln!("skipped: ffmpeg cannot build a clip on this host");
        return;
    }
    let mut app = fresh_app();
    scan_folder(&mut app, &media.root);
    let _ = app.update(Message::GoBrowse);
    let _ = app.update(Message::Tick);
    assert!(
        !app.browse.items.is_empty(),
        "the fixture must be browsable"
    );

    app.preferences.reduced_motion = true;
    let _ = app.update(Message::Tick);
    assert!(app.preview.is_none(), "Reduce motion must refuse a preview");
    assert!(app.preview_frame.is_none(), "and leave the poster alone");

    app.preferences.reduced_motion = false;
    let _ = app.update(Message::Tick);
    assert!(app.preview.is_none(), "a preview waits for the dwell");
    assert!(
        app.preview_dwell.is_some(),
        "the dwell starts on the focused card"
    );

    if !player_ready() {
        return;
    }
    assert!(
        pump(&mut app, Duration::from_secs(20), |app| app
            .preview
            .is_some()),
        "the focused card must preview after the delay"
    );
    assert!(app.preview_shown.is_some(), "the card draws the preview");
    assert!(
        pump(&mut app, Duration::from_secs(15), |app| app
            .preview_frame
            .is_some()),
        "the preview must present a frame"
    );

    let _ = app.update(Message::GoHome);
    let _ = app.update(Message::Tick);
    assert!(app.preview.is_none(), "leaving Browse stops the decoder");
    assert!(
        app.preview_frame.is_none(),
        "and the card gets its poster back"
    );

    let _ = app.update(Message::GoBrowse);
    let _ = app.update(Message::Tick);
    let _ = app.update(Message::ToggleReducedMotion(true));
    let _ = app.update(Message::Tick);
    assert!(app.preview.is_none(), "turning Reduce motion on stops it");
}

#[test]
fn step_18_a_local_nfo_gives_a_film_its_real_title_and_the_browse_filter_real_genres() {
    let media = Media::new();
    media.placeholders(&["Some.Release.2019.mkv".to_string()]);
    std::fs::write(
        media.root.join("Some.Release.2019.nfo"),
        b"<movie><title>Pather Panchali</title><year>1955</year><genre>Drama</genre>\
          <plot>A village childhood.</plot></movie>",
    )
    .expect("nfo");

    let mut app = fresh_app();
    scan_folder(&mut app, &media.root);
    let _ = app.update(Message::Tick);

    let status = app.status.clone().unwrap_or_default();
    assert!(
        status.contains("NFO"),
        "the scan must say what it read, got {status:?}"
    );
    assert_eq!(app.browse.items.len(), 1);
    let item = app.browse.items[0].clone();
    assert_eq!(item.title, "Pather Panchali");
    assert_eq!(item.year, Some(1955));
    assert_eq!(item.overview, "A village childhood.");
    assert_eq!(item.genres, vec!["Drama".to_string()]);
    assert_eq!(item.metadata_origin, "nfo");
    assert_eq!(app.browse.genres, vec!["Drama".to_string()]);

    let _ = app.update(Message::GenreChanged(Some("Drama".to_string())));
    assert_eq!(app.browse.items.len(), 1, "the film is in its own genre");

    let _ = app.update(Message::GenreChanged(Some("Comedy".to_string())));
    assert!(
        app.browse.items.is_empty(),
        "a genre nothing is in filters everything out"
    );

    let _ = app.update(Message::GenreChanged(None));
    assert_eq!(
        app.browse.items.len(),
        1,
        "clearing the genre brings it back"
    );

    let _ = app.update(Message::OpenDetail(item.id.clone()));
    let detail = app.detail.as_ref().expect("detail");
    assert_eq!(detail.item.genres, vec!["Drama".to_string()]);
    assert_eq!(detail.item.metadata_origin, "nfo");
}

#[test]
fn step_19_a_saved_source_keeps_its_provenance_and_refuses_a_bad_address() {
    let mut app = fresh_app();
    app.bookmark_label = "ISP portal".to_string();
    app.bookmark_address = "https://example.org/media".to_string();
    app.bookmark_access = "needs the home connection".to_string();
    let _ = app.update(Message::AddBookmark);

    assert_eq!(app.bookmarks.len(), 1, "the source was not saved");
    let saved = app.bookmarks[0].clone();
    assert_eq!(saved.protocol, "HTTPS");
    assert_eq!(saved.access, "needs the home connection");
    assert!(
        saved
            .provenance
            .contains("nothing about it has been verified"),
        "provenance must admit it is unverified, got {:?}",
        saved.provenance
    );
    assert!(
        app.bookmark_label.is_empty(),
        "the fields are cleared after saving"
    );

    app.bookmark_label = "Local folder".to_string();
    app.bookmark_address = "file:///Volumes/bleh".to_string();
    let _ = app.update(Message::AddBookmark);
    assert_eq!(app.bookmarks.len(), 1, "an unusable address was saved");
    assert!(app.error.is_some(), "the refusal must be reported");

    let _ = app.update(Message::RemoveBookmark(saved.id));
    assert!(app.bookmarks.is_empty());
}

#[test]
fn a_bengali_subtitle_is_drawn_into_the_picture() {
    if !player_ready() {
        return;
    }
    let media = Media::new();
    let path = media.path("Bengali (2020)/Bengali (2020).mkv");
    assert!(black_clip(&path, 6.0), "a black fixture is needed");
    let subtitle = media.path("Bengali (2020)/bengali-cue.bn.srt");
    std::fs::write(
        &subtitle,
        "1\n00:00:00,000 --> 00:00:06,000\nআমি বাংলায় গান গাই।\n",
    )
    .expect("subtitle");

    let mut plain = fresh_app();
    scan_folder(&mut plain, &media.root);
    let plain_id = plain.browse.items[0].id.clone();
    assert!(play(&mut plain, &plain_id), "the film plays");
    assert!(
        pump(&mut plain, Duration::from_secs(10), |app| app
            .frame
            .is_some()),
        "a frame arrives"
    );
    let plain_frame = plain.frame.clone().expect("frame");
    let (plain_ink, band) = ink_in_rows_below(&plain_frame, 0.75);
    let _ = plain.update(Message::BackFromPlayer);

    let mut app = fresh_app();
    scan_folder(&mut app, &media.root);
    let id = app.browse.items[0].id.clone();
    assert!(play(&mut app, &id), "the film plays");
    let _ = app.update(Message::SubtitleChosen(
        subtitle.to_string_lossy().to_string(),
    ));
    let stored = app
        .subtitles
        .first()
        .cloned()
        .expect("the subtitle is kept");
    assert_eq!(stored.language, "Bengali");
    let _ = app.update(Message::UseSubtitle(stored.id.clone()));
    assert!(app.error.is_none(), "{:?}", app.error);
    assert!(
        pump(&mut app, Duration::from_secs(10), |app| app
            .snapshot
            .tracks
            .iter()
            .any(
                |track| track.kind == TrackKind::Subtitle && track.selected
            )),
        "the imported subtitle reaches the player as a selected track"
    );
    let then = app.snapshot.position;
    assert!(
        pump(&mut app, Duration::from_secs(10), |app| app
            .snapshot
            .position
            > then + 0.3),
        "the film keeps advancing with the subtitle on"
    );
    let frame = app.frame.clone().expect("frame");
    let (ink, _) = ink_in_rows_below(&frame, 0.75);
    println!("subtitle band: {band} pixels, {plain_ink} without and {ink} with");
    assert!(
        plain_ink * 50 < band,
        "the fixture is not black without a subtitle: {plain_ink} of {band}"
    );
    assert!(
        ink > plain_ink + 100,
        "a Bengali cue must be drawn into the picture: {ink} of {band} pixels lit, \
         against {plain_ink} without it"
    );
}

#[test]
fn step_21_a_subtitle_is_kept_and_handed_to_the_player_without_touching_the_original() {
    if !player_ready() {
        return;
    }
    let media = Media::new();
    let path = media.path("Subtitle Test (2020).mkv");
    assert!(clip(&path, 4.0), "a generated clip is needed");
    settle(&path);
    let subtitle = media.path("Subtitle Test (2020).en.srt");
    let body = "1\n00:00:01,000 --> 00:00:02,000\nHello\n";
    std::fs::write(&subtitle, body).expect("subtitle");
    let original = std::fs::read(&subtitle).expect("read original");

    let mut app = fresh_app();
    scan_folder(&mut app, &media.root);
    let id = app.browse.items[0].id.clone();
    assert!(play(&mut app, &id), "the clip plays");
    assert!(app.subtitles.is_empty());

    let _ = app.update(Message::SubtitleChosen(
        subtitle.to_string_lossy().to_string(),
    ));
    assert_eq!(app.subtitles.len(), 1, "the subtitle was not kept");
    let stored = app.subtitles[0].clone();
    assert_eq!(stored.language, "English");
    assert_eq!(stored.format, "srt");
    assert!(stored.provenance.contains("imported by you"));
    assert_eq!(
        std::fs::read(&subtitle).expect("still readable"),
        original,
        "the file the owner picked must not be modified"
    );

    let _ = app.update(Message::UseSubtitle(stored.id.clone()));
    assert!(app.error.is_none(), "{:?}", app.error);
    assert!(
        app.status
            .as_deref()
            .is_some_and(|status| status.contains("Subtitle")),
        "the handoff must be reported, got {:?}",
        app.status
    );

    let _ = app.update(Message::RemoveSubtitle(stored.id.clone()));
    assert!(app.subtitles.is_empty());
    assert!(
        subtitle.is_file(),
        "the original file is untouched by removal"
    );
    let _ = app.update(Message::BackFromPlayer);
}

fn watching(app: &mut App, file_id: &str, load_id: u64) -> bool {
    let Ok(opened) = PlaybackTracker::open(&app.library, &app.profile_id, file_id) else {
        return false;
    };
    let Some(item) = app
        .library
        .item(file_id, Some(&app.profile_id))
        .ok()
        .flatten()
    else {
        return false;
    };
    app.active = Some(Active {
        tracker: opened.tracker,
        file_id: item.id.clone(),
        title: item.title.clone(),
        year: item.year,
        season: item.season,
        episode: item.episode,
        resume_from_ms: opened.resume_from_ms,
        origin: Screen::Browse,
        load_id,
        seek_serial: 0,
    });
    app.snapshot = screenroom_player::PlayerSnapshot {
        loaded: true,
        tracks_ready: true,
        load_id,
        ..screenroom_player::PlayerSnapshot::default()
    };
    true
}

fn subtitle_track(label: &str) -> screenroom_player::MediaTrack {
    screenroom_player::MediaTrack {
        id: 2,
        kind: TrackKind::Subtitle,
        title: Some(label.to_string()),
        language: Some("eng".to_string()),
        codec: Some("subrip".to_string()),
        selected: true,
        external: false,
    }
}

fn downloaded_cue() -> DownloadedSubtitle {
    DownloadedSubtitle {
        id: 7,
        language: "bn".to_string(),
        language_name: "Bengali".to_string(),
        release_name: "Tiered.2020.1080p".to_string(),
        format: "srt".to_string(),
        bytes: b"1\n00:00:01,000 --> 00:00:02,000\nHello\n".to_vec(),
    }
}

#[test]
fn a_film_with_its_own_subtitle_track_is_never_sent_online() {
    let media = Media::new();
    media.placeholders(&["Tiered (2020)/Tiered (2020).mkv".to_string()]);
    let mut app = fresh_app();
    scan_folder(&mut app, &media.root);
    let id = app.browse.items[0].id.clone();
    assert!(watching(&mut app, &id, 41));

    app.snapshot.tracks = vec![subtitle_track("English")];
    let _ = app.update(Message::FindSubtitles);

    assert!(
        app.subtitle_search.is_none(),
        "a track inside the file must not start a lookup"
    );
    assert!(app.subtitle_results.is_empty());
    assert!(app.error.is_none(), "{:?}", app.error);
    let status = app.status.clone().unwrap_or_default();
    assert!(status.contains("subtitle track"), "{status}");
    assert!(status.contains("English"), "{status}");
    assert!(status.contains("Nothing online"), "{status}");
}

#[test]
fn before_the_player_reports_its_tracks_nothing_online_is_asked() {
    let media = Media::new();
    media.placeholders(&["Unsettled (2020)/Unsettled (2020).mkv".to_string()]);
    let mut app = fresh_app();
    scan_folder(&mut app, &media.root);
    let id = app.browse.items[0].id.clone();
    assert!(watching(&mut app, &id, 43));

    app.snapshot.tracks_ready = false;
    let _ = app.update(Message::FindSubtitles);

    assert!(
        app.subtitle_search.is_none(),
        "an unsettled track list must not start a lookup"
    );
    assert!(app.subtitle_results.is_empty());
    assert!(app.error.is_none(), "{:?}", app.error);
    let status = app.status.clone().unwrap_or_default();
    assert!(status.contains("Tracks"), "{status}");
    assert!(status.contains("nothing was asked"), "{status}");
    assert!(status.contains("Try again"), "{status}");
}

#[test]
fn a_subtitle_file_beside_the_film_is_used_without_asking_online() {
    let media = Media::new();
    media.placeholders(&["Beside (2020)/Beside (2020).mkv".to_string()]);
    let cue = media.path("Beside (2020)/Beside (2020).bn.srt");
    std::fs::write(&cue, "1\n00:00:01,000 --> 00:00:02,000\nআমি বাংলায়\n").expect("cue");

    let mut app = fresh_app();
    scan_folder(&mut app, &media.root);
    let id = app.browse.items[0].id.clone();
    assert!(watching(&mut app, &id, 42));
    let _ = app.update(Message::FindSubtitles);

    assert!(
        app.subtitle_search.is_none(),
        "a subtitle file beside the film must not start a lookup"
    );
    assert!(app.error.is_none(), "{:?}", app.error);
    assert_eq!(app.subtitles.len(), 1, "the sidecar was not kept");
    assert_eq!(app.subtitles[0].language, "Bengali");
    assert_eq!(app.subtitles[0].format, "srt");
    let status = app.status.clone().unwrap_or_default();
    assert!(status.contains("Beside (2020).bn.srt"), "{status}");
    assert!(status.contains("Nothing online"), "{status}");
    assert!(cue.is_file(), "the file on disk is untouched");
}

#[test]
fn the_player_resolves_a_track_before_it_looks_online() {
    let media = Media::new();
    media.placeholders(&["Reachable (2020)/Reachable (2020).mkv".to_string()]);
    let mut app = fresh_app();
    scan_folder(&mut app, &media.root);
    let id = app.browse.items[0].id.clone();
    assert!(watching(&mut app, &id, 44));

    app.screen = Screen::Player;
    assert!(
        matches!(app.subtitle_source(), Some(SubtitleSource::Online)),
        "the player must resolve a film that carries no subtitle"
    );

    app.snapshot.tracks = vec![subtitle_track("English")];
    assert!(
        matches!(app.subtitle_source(), Some(SubtitleSource::Track { .. })),
        "a track inside the file must win before any lookup"
    );

    let _ = app.update(Message::FindSubtitles);
    assert!(app.subtitle_search.is_none(), "no lookup may start");
    let shown = app.subtitle_note.clone().unwrap_or_default();
    assert!(
        shown.contains("subtitle track"),
        "the panel must draw the reason it did nothing: {shown}"
    );
}

#[test]
fn the_subtitle_panel_is_operable_without_a_pointer() {
    let media = Media::new();
    media.placeholders(&["Keyboard (2020)/Keyboard (2020).mkv".to_string()]);
    let mut app = fresh_app();
    scan_folder(&mut app, &media.root);
    let id = app.browse.items[0].id.clone();
    assert!(watching(&mut app, &id, 48));
    app.screen = Screen::Player;
    app.player_chrome = true;

    assert!(
        app.subtitle_actions().is_empty(),
        "an empty panel offers nothing to press"
    );

    app.subtitle_results = vec![
        screenroom_core::SubtitleMatch {
            id: 11,
            language: "bn".to_string(),
            language_name: "Bengali".to_string(),
            release_name: "Keyboard.2020.first".to_string(),
            format: "srt".to_string(),
            cues: 500,
            hearing_impaired: false,
            ai_transcribed: false,
            encoding: "utf-8".to_string(),
        },
        screenroom_core::SubtitleMatch {
            id: 12,
            language: "bn".to_string(),
            language_name: "Bengali".to_string(),
            release_name: "Keyboard.2020.second".to_string(),
            format: "srt".to_string(),
            cues: 480,
            hearing_impaired: false,
            ai_transcribed: false,
            encoding: "utf-8".to_string(),
        },
    ];
    app.subtitle_focus = 0;

    let actions = app.subtitle_actions();
    assert_eq!(actions.len(), 2);
    assert_eq!(app.subtitle_window(actions.len()), (0, 2));
    assert!(matches!(
        &actions[0],
        Message::SubtitleDownload { id: 11, format } if format == "srt"
    ));
    assert!(matches!(
        &actions[1],
        Message::SubtitleDownload { id: 12, .. }
    ));

    app.player_chrome = false;
    let _ = app.update(key("down"));
    assert!(app.player_chrome, "a subtitle key wakes the panel");
    assert_eq!(app.subtitle_focus, 1, "Down moves to the second row");
    let _ = app.update(key("down"));
    assert_eq!(app.subtitle_focus, 0, "Down wraps to the first row");
    let _ = app.update(key("up"));
    assert_eq!(app.subtitle_focus, 1, "Up wraps back to the last row");
}

#[test]
fn a_longer_result_list_stays_reachable_through_the_focus() {
    let mut app = fresh_app();
    app.window_height = 800.0;
    app.subtitle_results = (0..6)
        .map(|index| screenroom_core::SubtitleMatch {
            id: 100 + index,
            language: "bn".to_string(),
            language_name: "Bengali".to_string(),
            release_name: format!("Release {index}"),
            format: "srt".to_string(),
            cues: 400,
            hearing_impaired: false,
            ai_transcribed: false,
            encoding: "utf-8".to_string(),
        })
        .collect();

    let (start, len) = app.subtitle_window(6);
    assert_eq!(len, app.subtitle_row_budget());
    assert_eq!((start, len), (0, 3), "the first window shows the best rows");

    app.subtitle_focus = 5;
    let (start, len) = app.subtitle_window(6);
    assert_eq!((start, len), (3, 3), "the window follows the focus");
    assert_eq!(start + len, 6);

    assert!(matches!(
        app.subtitle_enter_target(),
        Some(Message::SubtitleDownload { id: 105, .. })
    ));
}

#[test]
fn delete_removes_the_kept_subtitle_the_panel_points_at() {
    let media = Media::new();
    media.placeholders(&["Remove (2020)/Remove (2020).mkv".to_string()]);
    let mut app = fresh_app();
    scan_folder(&mut app, &media.root);
    let id = app.browse.items[0].id.clone();
    assert!(watching(&mut app, &id, 61));
    app.screen = Screen::Player;
    app.player_chrome = true;
    let cue = media.path("Remove (2020)/Remove (2020).bn.srt");
    std::fs::write(&cue, "1\n00:00:01,000 --> 00:00:02,000\nhi\n").expect("a cue");
    app.library
        .import_subtitle(&id, &cue)
        .expect("the subtitle is kept");
    app.subtitles = app.library.subtitles_for(&id).expect("kept rows");
    assert_eq!(app.subtitles.len(), 1);
    app.subtitle_focus = 0;

    let _ = app.update(key("backspace"));

    assert!(
        app.subtitles.is_empty(),
        "the focused kept subtitle is the one removed"
    );
}

#[test]
fn a_subtitle_that_appears_after_the_search_stops_the_download() {
    let media = Media::new();
    media.placeholders(&["Late (2020)/Late (2020).mkv".to_string()]);
    let mut app = fresh_app();
    scan_folder(&mut app, &media.root);
    let id = app.browse.items[0].id.clone();
    assert!(watching(&mut app, &id, 62));
    app.screen = Screen::Player;
    app.subtitle_results_for = Some(id.clone());
    app.subtitle_results = vec![screenroom_core::SubtitleMatch {
        id: 77,
        language: "bn".to_string(),
        language_name: "Bengali".to_string(),
        release_name: "Late.2020".to_string(),
        format: "srt".to_string(),
        cues: 100,
        hearing_impaired: false,
        ai_transcribed: false,
        encoding: "utf-8".to_string(),
    }];
    std::fs::write(
        media.path("Late (2020)/Late (2020).bn.srt"),
        "1\n00:00:01,000 --> 00:00:02,000\nhi\n",
    )
    .expect("a sidecar");

    let _ = app.update(Message::SubtitleDownload {
        id: 77,
        format: "srt".to_string(),
    });

    assert!(app.subtitle_download.is_none(), "no download may start");
    let note = app.subtitle_note.clone().unwrap_or_default();
    assert!(note.contains("own subtitle"), "{note}");
}

#[test]
fn a_wide_label_is_measured_not_counted() {
    let wide = "映画の字幕ファイル名がとても長い場合のテスト用ラベルです";
    let fitted = fit_label(wide, 140.0);
    assert!(fitted.ends_with('…'), "{fitted}");
    let width: f32 = fitted
        .chars()
        .map(|c| if c.is_ascii() { 7.0 } else { 14.0 })
        .sum();
    assert!(width <= 140.0 + 14.0, "{width}");

    let ascii = "Parasite.2019.1080p.AMZN.WEB-DL.Dual.Audio.HEVC";
    let fitted = fit_label(ascii, 140.0);
    assert!(fitted.chars().count() > 14, "{fitted}");
}

#[test]
fn a_narrow_window_shows_a_short_subtitle_label() {
    let mut app = fresh_app();
    let result = screenroom_core::SubtitleMatch {
        id: 31,
        language: "bn".to_string(),
        language_name: "Bengali".to_string(),
        release_name: "Parasite.2019.1080p.AMZN.WEB-DL.Dual Audio [Hin+Kor]HEVC.DDP 5.1"
            .to_string(),
        format: "srt".to_string(),
        cues: 1660,
        hearing_impaired: false,
        ai_transcribed: false,
        encoding: "utf-8".to_string(),
    };

    app.window_width = 640.0;
    assert_eq!(app.subtitle_result_label(&result), "Bengali · 1660 cues");

    app.window_width = 1280.0;
    assert!(
        app.subtitle_result_label(&result)
            .starts_with("Parasite.2019.1080p.AMZN"),
        "a wide window names the release"
    );
}

#[test]
fn the_first_key_after_the_chrome_hides_still_acts() {
    let media = Media::new();
    media.placeholders(&["Idle (2020)/Idle (2020).mkv".to_string()]);
    let mut app = fresh_app();
    scan_folder(&mut app, &media.root);
    let id = app.browse.items[0].id.clone();
    assert!(watching(&mut app, &id, 51));
    app.screen = Screen::Player;
    app.player_chrome = false;
    app.subtitle_results = vec![screenroom_core::SubtitleMatch {
        id: 21,
        language: "bn".to_string(),
        language_name: "Bengali".to_string(),
        release_name: "Idle.2020".to_string(),
        format: "srt".to_string(),
        cues: 300,
        hearing_impaired: false,
        ai_transcribed: false,
        encoding: "utf-8".to_string(),
    }];

    assert!(
        matches!(
            app.subtitle_enter_target(),
            Some(Message::SubtitleDownload { id: 21, .. })
        ),
        "Enter must target the row even while the chrome is idle"
    );
    let _ = app.update(key("enter"));
    assert!(app.player_chrome, "the key wakes the panel it acts on");
}

#[test]
fn a_kept_subtitle_is_reachable_from_the_panel_with_the_keyboard() {
    let media = Media::new();
    media.placeholders(&["Kept (2020)/Kept (2020).mkv".to_string()]);
    let mut app = fresh_app();
    scan_folder(&mut app, &media.root);
    let id = app.browse.items[0].id.clone();
    assert!(watching(&mut app, &id, 49));
    app.screen = Screen::Player;
    app.player_chrome = true;

    app.subtitles = vec![screenroom_core::StoredSubtitle {
        id: "kept-1".to_string(),
        file_id: id.clone(),
        label: "Bengali · imported".to_string(),
        language: "Bengali".to_string(),
        format: "srt".to_string(),
        provenance: "imported by you from a file on this machine".to_string(),
        bytes: 120,
        created_ms: 1,
    }];

    let actions = app.subtitle_actions();
    assert!(
        matches!(&actions[0], Message::UseSubtitle(id) if id == "kept-1"),
        "a kept subtitle is the first row"
    );

    let _ = app.update(key("enter"));
    assert!(app.subtitle_actions().len() == 1);
}

#[test]
fn results_that_arrive_for_another_film_are_dropped() {
    let media = Media::new();
    media.placeholders(&["Arrived (2020)/Arrived (2020).mkv".to_string()]);
    let mut app = fresh_app();
    scan_folder(&mut app, &media.root);
    let id = app.browse.items[0].id.clone();
    assert!(watching(&mut app, &id, 46));

    app.subtitle_results_for = Some("a-different-film".to_string());
    let (sender, receiver) = std::sync::mpsc::channel();
    app.subtitle_search = Some(receiver);
    sender
        .send(Ok(vec![screenroom_core::SubtitleMatch {
            id: 9,
            language: "bn".to_string(),
            language_name: "Bengali".to_string(),
            release_name: "Arrived.2020".to_string(),
            format: "srt".to_string(),
            cues: 90,
            hearing_impaired: false,
            ai_transcribed: false,
            encoding: "utf-8".to_string(),
        }]))
        .expect("send");
    let _ = app.update(Message::Tick);

    assert!(app.subtitle_search.is_none());
    assert!(
        app.subtitle_results.is_empty(),
        "results for another film must not be listed"
    );
    assert!(app.subtitle_results_for.is_none());
    let shown = app.subtitle_note.clone().unwrap_or_default();
    assert!(shown.contains("no longer playing"), "{shown}");
}

#[test]
fn leaving_the_window_hides_the_controls_at_once() {
    let media = Media::new();
    media.placeholders(&["Edge (2020)/Edge (2020).mkv".to_string()]);
    let mut app = fresh_app();
    scan_folder(&mut app, &media.root);
    let id = app.browse.items[0].id.clone();
    assert!(watching(&mut app, &id, 52));
    app.screen = Screen::Player;
    app.player_chrome = true;

    let _ = app.update(Message::PointerInside(false));
    assert!(!app.player_chrome, "an exit hides the chrome immediately");
    assert!(app.motion.chrome() <= 0.001, "the reveal snaps to zero");

    let _ = app.update(Message::PointerInside(true));
    assert!(app.player_chrome, "an entry shows it again");
    assert!(app.motion.chrome() >= 0.999, "the reveal snaps back to one");
}

#[test]
fn an_open_track_menu_keeps_the_controls_while_the_pointer_is_out() {
    let media = Media::new();
    media.placeholders(&["Menu (2020)/Menu (2020).mkv".to_string()]);
    let mut app = fresh_app();
    scan_folder(&mut app, &media.root);
    let id = app.browse.items[0].id.clone();
    assert!(watching(&mut app, &id, 53));
    app.screen = Screen::Player;
    app.player_chrome = true;

    let _ = app.update(Message::TrackMenuOpened(TrackKind::Subtitle));
    let _ = app.update(Message::PointerInside(false));
    let _ = app.update(Message::Tick);
    assert!(
        app.player_chrome,
        "a menu being used outranks the pointer leaving, even on the next tick"
    );

    let _ = app.update(Message::TrackMenuClosed(TrackKind::Subtitle));
    let _ = app.update(Message::PointerInside(false));
    assert!(!app.player_chrome, "the hold ends with the menu");

    let _ = app.update(Message::PointerInside(true));
    let _ = app.update(Message::PointerButton {
        button: iced::mouse::Button::Left,
        pressed: true,
    });
    let _ = app.update(Message::PointerButton {
        button: iced::mouse::Button::Right,
        pressed: true,
    });
    let _ = app.update(Message::PointerButton {
        button: iced::mouse::Button::Right,
        pressed: false,
    });
    let _ = app.update(Message::PointerInside(false));
    let _ = app.update(Message::Tick);
    assert!(
        app.player_chrome,
        "releasing a different button must not end a drag that is still held"
    );
    let _ = app.update(Message::PointerButton {
        button: iced::mouse::Button::Left,
        pressed: false,
    });
    let _ = app.update(Message::PointerInside(true));
    let _ = app.update(Message::TrackMenuOpened(TrackKind::Audio));
    let _ = app.update(Message::SelectTrack(TrackKind::Audio, 1));
    let _ = app.update(Message::PointerInside(false));
    assert!(
        !app.player_chrome,
        "choosing from the menu also ends the hold that selection does not report"
    );
}

#[test]
fn a_pointer_boundary_does_not_restart_when_it_repeats() {
    let media = Media::new();
    media.placeholders(&["Still (2020)/Still (2020).mkv".to_string()]);
    let mut app = fresh_app();
    scan_folder(&mut app, &media.root);
    let id = app.browse.items[0].id.clone();
    assert!(watching(&mut app, &id, 54));
    app.screen = Screen::Player;
    app.player_chrome = true;

    let _ = app.update(Message::PointerInside(true));
    let _ = app.update(Message::PointerInside(true));
    assert!(app.player_chrome);
    let _ = app.update(Message::PointerInside(false));
    let _ = app.update(Message::PointerInside(false));
    assert!(!app.player_chrome);
    assert!(app.motion.chrome() <= 0.001);
}

#[test]
fn a_key_board_input_keeps_the_controls_while_the_pointer_is_out() {
    let media = Media::new();
    media.placeholders(&["Paused (2020)/Paused (2020).mkv".to_string()]);
    let mut app = fresh_app();
    scan_folder(&mut app, &media.root);
    let id = app.browse.items[0].id.clone();
    assert!(watching(&mut app, &id, 56));
    app.screen = Screen::Player;
    app.snapshot.state = screenroom_player::PlayerState::Paused;

    let _ = app.update(Message::PointerInside(false));
    assert!(!app.player_chrome);

    let _ = app.update(key("space"));
    let _ = app.update(Message::Tick);

    assert!(
        app.player_chrome,
        "a key press while the pointer is out must show the controls, even paused"
    );
}

#[test]
fn losing_focus_releases_a_mouse_up_that_never_arrived() {
    let media = Media::new();
    media.placeholders(&["Focus (2020)/Focus (2020).mkv".to_string()]);
    let mut app = fresh_app();
    scan_folder(&mut app, &media.root);
    let id = app.browse.items[0].id.clone();
    assert!(watching(&mut app, &id, 60));
    app.screen = Screen::Player;

    let _ = app.update(Message::PointerButton {
        button: iced::mouse::Button::Left,
        pressed: true,
    });
    assert!(app.player_chrome_held());

    let _ = app.update(Message::WindowUnfocused);
    assert!(
        !app.player_chrome_held(),
        "deactivating the app must release a press whose mouse-up went elsewhere"
    );
    assert!(!app.player_chrome);

    let _ = app.update(Message::Tick);
    assert!(!app.player_chrome, "and the tick cannot bring it back");
}

#[test]
fn a_short_window_keeps_the_subtitle_note_on_one_line() {
    let mut app = fresh_app();
    app.subtitle_note = Some(
        "The player has not reported this film's tracks yet, so a subtitle track inside it \
         cannot be ruled out. Nothing online was asked; try again once it is playing."
            .to_string(),
    );
    app.window_height = 480.0;
    let line = app.subtitle_line().expect("a note");
    assert!(!line.contains('\n'), "{line}");
    assert!(line.chars().count() <= 65, "{line}");

    app.window_height = 800.0;
    assert!(
        app.subtitle_line().expect("a note").chars().count() > 65,
        "a tall window keeps the whole note"
    );
}

#[test]
fn hovering_the_volume_slider_does_not_latch_the_controls() {
    let media = Media::new();
    media.placeholders(&["Hover (2020)/Hover (2020).mkv".to_string()]);
    let mut app = fresh_app();
    scan_folder(&mut app, &media.root);
    let id = app.browse.items[0].id.clone();
    assert!(watching(&mut app, &id, 57));
    app.screen = Screen::Player;

    let _ = app.update(Message::Volume(42.0));
    assert!(
        !app.player_chrome_held(),
        "a value change is not a drag and must not hold the chrome"
    );

    let _ = app.update(Message::PointerInside(false));
    let _ = app.update(Message::Tick);
    assert!(!app.player_chrome, "the controls still hide");
}

#[test]
fn motion_beyond_the_window_is_not_presence() {
    let media = Media::new();
    media.placeholders(&["Drag (2020)/Drag (2020).mkv".to_string()]);
    let mut app = fresh_app();
    scan_folder(&mut app, &media.root);
    let id = app.browse.items[0].id.clone();
    assert!(watching(&mut app, &id, 59));
    app.screen = Screen::Player;
    app.snapshot.state = screenroom_player::PlayerState::Paused;
    app.window_width = 1280.0;
    app.window_height = 800.0;

    let _ = app.update(Message::PointerMoved { x: 600.0, y: 400.0 });
    assert!(
        app.pointer_inside,
        "a position inside the window is presence"
    );

    let _ = app.update(Message::PointerMoved {
        x: 1400.0,
        y: 400.0,
    });
    let _ = app.update(Message::Tick);
    assert!(
        !app.player_chrome,
        "a drag that leaves the window must not report presence"
    );
}

#[test]
fn a_resize_releases_a_menu_hold() {
    let media = Media::new();
    media.placeholders(&["Narrow (2020)/Narrow (2020).mkv".to_string()]);
    let mut app = fresh_app();
    scan_folder(&mut app, &media.root);
    let id = app.browse.items[0].id.clone();
    assert!(watching(&mut app, &id, 58));
    app.screen = Screen::Player;

    let _ = app.update(Message::TrackMenuOpened(TrackKind::Subtitle));
    let _ = app.update(Message::Resized(iced::Size::new(640.0, 480.0)));
    assert!(
        !app.player_chrome_held(),
        "a resize that hides the control which owned the menu releases the hold"
    );

    let _ = app.update(Message::PointerInside(false));
    let _ = app.update(Message::Tick);
    assert!(!app.player_chrome);
}

#[test]
fn a_film_still_playing_outside_the_window_stays_hidden_on_tick() {
    let media = Media::new();
    media.placeholders(&["Tick (2020)/Tick (2020).mkv".to_string()]);
    let mut app = fresh_app();
    scan_folder(&mut app, &media.root);
    let id = app.browse.items[0].id.clone();
    assert!(watching(&mut app, &id, 55));
    app.screen = Screen::Player;
    app.snapshot.state = screenroom_player::PlayerState::Playing;

    let _ = app.update(Message::PointerInside(false));
    let _ = app.update(Message::Tick);
    assert!(!app.player_chrome, "a tick cannot reopen hidden controls");
}

#[test]
fn the_subtitle_panel_sizes_its_row_count_to_the_window() {
    let mut app = fresh_app();
    app.window_height = 480.0;
    assert_eq!(app.subtitle_row_budget(), 1, "a short window shows one row");
    app.window_height = 800.0;
    assert_eq!(
        app.subtitle_row_budget(),
        3,
        "the default window shows three rows and still clears the title"
    );
    app.window_height = 1_440.0;
    assert_eq!(app.subtitle_row_budget(), 6, "a tall window stops at six");
}

#[test]
fn results_found_for_one_film_are_not_downloaded_for_another() {
    let media = Media::new();
    media.placeholders(&["Elsewhere (2020)/Elsewhere (2020).mkv".to_string()]);
    let mut app = fresh_app();
    scan_folder(&mut app, &media.root);
    let id = app.browse.items[0].id.clone();
    assert!(watching(&mut app, &id, 45));

    app.subtitle_results = vec![screenroom_core::SubtitleMatch {
        id: 7,
        language: "bn".to_string(),
        language_name: "Bengali".to_string(),
        release_name: "Elsewhere.2020".to_string(),
        format: "srt".to_string(),
        cues: 120,
        hearing_impaired: false,
        ai_transcribed: false,
        encoding: "utf-8".to_string(),
    }];
    app.subtitle_results_for = Some("a-different-film".to_string());

    let _ = app.update(Message::SubtitleDownload {
        id: 7,
        format: "srt".to_string(),
    });

    assert!(app.subtitle_download.is_none(), "no download may start");
    assert!(app.subtitles.is_empty(), "nothing may be kept");
    let status = app.status.clone().unwrap_or_default();
    assert!(status.contains("different film"), "{status}");
}

#[test]
fn an_empty_answer_from_the_subtitle_db_is_reported_as_a_miss() {
    let media = Media::new();
    media.placeholders(&["Nothing (2020)/Nothing (2020).mkv".to_string()]);
    let mut app = fresh_app();
    scan_folder(&mut app, &media.root);
    let id = app.browse.items[0].id.clone();
    assert!(watching(&mut app, &id, 47));
    app.subtitle_results_for = Some(id.clone());

    let (sender, receiver) = std::sync::mpsc::channel();
    app.subtitle_search = Some(receiver);
    sender.send(Ok(Vec::new())).expect("send");
    let _ = app.update(Message::Tick);

    assert!(app.subtitle_search.is_none());
    assert!(app.subtitle_results.is_empty());
    assert!(app.error.is_none(), "{:?}", app.error);
    let status = app.status.clone().unwrap_or_default();
    assert!(status.contains("No subtitle"), "{status}");
    assert!(!status.contains("0 subtitles"), "{status}");
}

#[test]
fn a_download_that_finishes_after_the_film_changed_is_not_kept_or_shown() {
    let media = Media::new();
    media.placeholders(&["Changed (2020)/Changed (2020).mkv".to_string()]);
    let mut app = fresh_app();
    scan_folder(&mut app, &media.root);
    let id = app.browse.items[0].id.clone();
    assert!(watching(&mut app, &id, 50));

    let (sender, receiver) = std::sync::mpsc::channel();
    app.subtitle_download = Some(SubtitleDownload {
        receiver,
        file_id: id.clone(),
        load_id: 50,
    });
    assert!(watching(&mut app, &id, 51));

    sender.send(Ok(downloaded_cue())).expect("send");
    let _ = app.update(Message::Tick);

    assert!(app.subtitle_download.is_none(), "the download is settled");
    assert!(
        app.subtitles.is_empty(),
        "a stale download must not be kept"
    );
    assert!(
        app.subtitle_results.is_empty(),
        "a stale download must not be reported as a match"
    );
    let status = app.status.clone().unwrap_or_default();
    assert!(status.contains("not kept"), "{status}");
}

#[test]
fn a_download_for_the_film_still_playing_is_kept_and_used() {
    let media = Media::new();
    media.placeholders(&["Kept (2020)/Kept (2020).mkv".to_string()]);
    let mut app = fresh_app();
    scan_folder(&mut app, &media.root);
    let id = app.browse.items[0].id.clone();
    assert!(watching(&mut app, &id, 60));

    let (sender, receiver) = std::sync::mpsc::channel();
    app.subtitle_download = Some(SubtitleDownload {
        receiver,
        file_id: id,
        load_id: 60,
    });
    sender.send(Ok(downloaded_cue())).expect("send");
    let _ = app.update(Message::Tick);

    assert!(app.subtitle_download.is_none(), "the download is settled");
    assert!(app.error.is_none(), "{:?}", app.error);
    assert_eq!(app.subtitles.len(), 1, "the download was not kept");
    assert_eq!(app.subtitles[0].language, "Bengali");
    assert!(
        !app.subtitles[0].provenance.trim().is_empty(),
        "a kept download must say where it came from"
    );
}

#[test]
fn step_22_receiver_discovery_is_your_choice_and_never_claims_that_casting_works() {
    let mut app = fresh_app();
    assert!(app.receivers.is_empty());
    assert!(
        app.casting_note()
            .contains("only when you press the button"),
        "nothing is probed behind the owner's back: {}",
        app.casting_note()
    );

    let response = "HTTP/1.1 200 OK\r\nST: urn:schemas-upnp-org:device:MediaRenderer:1\r\n\
                    USN: uuid:2222::urn:schemas-upnp-org:device:MediaRenderer:1\r\n\
                    LOCATION: http://192.168.1.30:9197/dmr\r\n\
                    SERVER: Linux/4.0 UPnP/1.0 LivingRoomTV/1.0\r\n\r\n";
    let found =
        crate::casting::parse_response(response, "192.168.1.30:1900".parse().expect("address"))
            .expect("a receiver");

    let (sender, receiver) = std::sync::mpsc::channel();
    app.cast_search = Some(receiver);
    assert!(app.casting_note().contains("Looking for cast receivers"));
    sender.send(Ok(vec![found])).expect("send");
    let _ = app.update(Message::Tick);

    assert_eq!(app.receivers.len(), 1, "the answer was not shown");
    assert!(app.cast_search.is_none(), "the probe is finished");
    assert!(app.cast_searched);
    let note = app.casting_note();
    assert!(note.contains("discovery only"), "got {note}");
    assert!(note.contains("no stream has been sent"), "got {note}");
    assert_eq!(app.receivers[0].kind, "DLNA MediaRenderer");
    assert!(app.error.is_none(), "{:?}", app.error);
}

#[test]
fn the_online_panels_explain_themselves_and_never_start_a_call_they_cannot_make() {
    let mut app = fresh_app();

    assert!(
        !app.metadata_capability.detail.trim().is_empty(),
        "the metadata panel must say something even with nothing configured"
    );
    assert!(
        !app.subtitle_capability.detail.trim().is_empty(),
        "the subtitle panel must say something even with nothing configured"
    );

    if !app.metadata_capability.is_configured() {
        let detail = app.metadata_capability.detail.clone();
        let _ = app.update(Message::EnrichmentNow);
        assert!(
            app.enrichment.is_none(),
            "no enrichment pass may start without a provider"
        );
        assert_eq!(
            app.status.as_deref(),
            Some(detail.as_str()),
            "the panel's own words are the reason"
        );
    }

    if !app.subtitle_capability.is_configured() {
        let _ = app.update(Message::FindSubtitles);
        assert!(
            app.subtitle_search.is_none(),
            "no subtitle search may start without a provider"
        );
    }

    app.settings_section = Section::Online;
    let actions: Vec<String> = app.settings_labels();
    assert!(
        actions.iter().any(|label| label.contains("Enrich")),
        "enrichment must be reachable from the keyboard: {actions:?}"
    );
    assert!(
        actions
            .iter()
            .any(|label| label.contains("Metadata provider")),
        "the metadata provider must be reachable from the keyboard: {actions:?}"
    );
    assert!(
        actions
            .iter()
            .any(|label| label.contains("provider artwork")),
        "the artwork retention choice must be reachable: {actions:?}"
    );
}

struct RecordingBackend {
    seen: std::sync::Arc<std::sync::Mutex<Vec<crate::updater::Action>>>,
}

impl crate::updater::Backend for RecordingBackend {
    fn perform(&mut self, action: crate::updater::Action) -> bool {
        self.seen.lock().expect("action log").push(action);
        true
    }

    fn reason(&self) -> Option<&str> {
        None
    }
}

fn app_with_recording_backend() -> (
    App,
    std::sync::Arc<std::sync::Mutex<Vec<crate::updater::Action>>>,
) {
    let mut app = app_with_offered_update();
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    app.updater_backend = Box::new(RecordingBackend { seen: seen.clone() });
    (app, seen)
}

fn app_with_offered_update() -> App {
    use crate::updater::{AvailableUpdate, Event, Stage};
    let mut app = fresh_app();
    app.updater = crate::updater::Updater::new();
    app.updater.feed(Event::Found {
        update: AvailableUpdate {
            version: "99.0.0".to_string(),
            display_version: "99.0.0".to_string(),
            notes: "A quieter library.".to_string(),
            critical: false,
            informational: false,
            feed_signing: "succeeded".to_string(),
        },
        stage: Stage::NotDownloaded,
        user_initiated: true,
    });
    app
}

#[test]
fn the_update_banner_appears_outside_playback_and_never_inside_it() {
    let mut app = app_with_offered_update();
    app.screen = Screen::Home;
    assert!(
        app.update_strip().is_some(),
        "an offered update must be visible on Home"
    );
    app.screen = Screen::Settings;
    assert!(
        app.update_strip().is_some(),
        "an offered update must stay visible in Settings"
    );
    app.screen = Screen::Player;
    assert!(
        app.update_strip().is_none(),
        "no update banner may be drawn over a playing film"
    );
}

#[test]
fn an_unavailable_updater_draws_no_banner_and_no_dead_action() {
    let mut app = fresh_app();
    app.screen = Screen::Home;
    assert!(app.updater.is_unavailable());
    assert!(
        app.update_strip().is_none(),
        "a build without the framework must not pretend an update exists"
    );
    app.screen = Screen::Settings;
    app.settings_section = Section::Updates;
    let labels = app.settings_labels();
    assert!(
        labels
            .iter()
            .any(|label| label.contains("unavailable in this launch")),
        "Settings must say why updates cannot run: {labels:?}"
    );
    assert!(
        !labels.iter().any(|label| label.contains("Check now")),
        "a check that cannot run must not be offered: {labels:?}"
    );
}

#[test]
fn the_updates_section_states_the_version_and_offers_the_actions_by_keyboard() {
    let mut app = app_with_offered_update();
    app.screen = Screen::Settings;
    app.settings_section = Section::Updates;
    let labels = app.settings_labels();
    let stated = app.settings_rows().into_iter().any(|row| {
        matches!(
            row,
            ui::screens::settings::Row::Value { label: "This version", value }
                if value == crate::updater::CURRENT_VERSION
        )
    });
    assert!(stated, "the running version must be stated: {labels:?}");
    let check = labels
        .iter()
        .position(|label| label.contains("Look for a new version now"))
        .expect("the manual check row must exist");
    let install = labels
        .iter()
        .position(|label| label.contains("Install the update now"))
        .expect("the install row must exist");
    let activations = app.settings_activations();
    assert!(matches!(
        activations.get(check).cloned().flatten(),
        Some(Message::CheckForUpdates)
    ));
    assert!(matches!(
        activations.get(install).cloned().flatten(),
        Some(Message::InstallUpdate)
    ));
    let (mut app, seen) = {
        let (mut app, seen) = app_with_recording_backend();
        app.screen = Screen::Settings;
        app.settings_section = Section::Updates;
        (app, seen)
    };
    let check = app
        .settings_labels()
        .iter()
        .position(|label| label.contains("Look for a new version now"))
        .expect("the manual check row must exist");
    app.settings_focus = check;
    let _ = app.update(key("enter"));
    assert_eq!(
        seen.lock().expect("action log").as_slice(),
        &[crate::updater::Action::CheckNow],
        "Enter must run the focused check instead of doing nothing"
    );
}

#[test]
fn an_install_pressed_while_a_film_plays_changes_nothing() {
    let (mut app, seen) = app_with_recording_backend();
    app.screen = Screen::Player;
    assert_eq!(app.update_playback(), crate::updater::Playback::Playing);
    let before = app.updater.status().clone();
    app.settings_section = Section::Updates;
    let labels = app.settings_labels();
    assert!(
        !labels
            .iter()
            .any(|label| label.contains("Install the update now")),
        "Settings must not offer an install that would interrupt playback: {labels:?}"
    );
    let _ = app.update(Message::InstallUpdate);
    assert_eq!(
        app.updater.status(),
        &before,
        "an install request during playback must be refused, not silently accepted"
    );
    assert!(
        seen.lock().expect("action log").is_empty(),
        "nothing may reach the updater while a film is playing"
    );
}

#[test]
fn a_failed_update_is_reported_where_it_can_be_read_without_stopping_playback() {
    use crate::updater::Event;
    let mut app = app_with_offered_update();
    app.updater.feed(Event::Failed {
        message: "The download was cut off.".to_string(),
    });
    app.screen = Screen::Player;
    assert!(
        app.update_strip().is_none(),
        "a failure must never take over a playing film"
    );
    app.screen = Screen::Settings;
    app.settings_section = Section::Updates;
    let stated = app.settings_rows().into_iter().any(|row| {
        matches!(
            row,
            ui::screens::settings::Row::Value { label: "Last failure", value }
                if value.contains("The download was cut off.")
        )
    });
    assert!(stated, "the failure must be readable in Settings");
}

#[test]
fn an_update_that_is_ready_offers_both_install_paths_and_never_relaunches_by_itself() {
    use crate::updater::{AvailableUpdate, Event};
    let mut app = fresh_app();
    app.updater = crate::updater::Updater::new();
    app.updater.feed(Event::Ready {
        update: AvailableUpdate {
            version: "99.0.0".to_string(),
            display_version: "99.0.0".to_string(),
            notes: String::new(),
            critical: false,
            informational: false,
            feed_signing: "succeeded".to_string(),
        },
        deferred_to_quit: false,
    });
    app.screen = Screen::Settings;
    app.settings_section = Section::Updates;
    let labels = app.settings_labels();
    assert!(
        labels
            .iter()
            .any(|label| label.contains("Install the update now"))
    );
    assert!(
        labels
            .iter()
            .any(|label| label.contains("Install when I quit"))
    );
    let strip = app
        .update_strip()
        .expect("a ready update is worth a banner");
    let _ = strip;
}

#[test]
fn the_banner_s_settings_action_reaches_the_updates_section() {
    let mut app = app_with_offered_update();
    app.screen = Screen::Home;
    assert!(app.update_strip().is_some());
    let _ = app.update(Message::OpenUpdateSettings);
    assert_eq!(app.screen, Screen::Settings);
    assert_eq!(app.settings_section, Section::Updates);
    assert!(
        app.settings_labels()
            .iter()
            .any(|label| label.contains("Look for a new version now")),
        "the banner's action must land on a screen that can act on the update"
    );
}

#[test]
fn later_hides_the_banner_without_cancelling_the_update() {
    let mut app = app_with_offered_update();
    app.screen = Screen::Home;
    assert!(app.update_strip().is_some());
    let _ = app.update(Message::DismissUpdate);
    assert!(
        app.update_strip().is_none(),
        "Later must take the banner away instead of leaving it on screen"
    );
    app.updater.feed(crate::updater::Event::Downloading {
        received: 1,
        expected: 2,
    });
    app.drain_updater();
    let _ = app.update(Message::Tick);
    assert!(
        app.update_strip().is_some(),
        "progress after Later must bring the banner back rather than hide the download"
    );
}

#[test]
fn the_banner_returns_after_a_film_even_though_the_player_handle_stays() {
    use crate::updater::{AvailableUpdate, Event, Stage};
    if !player_ready() {
        return;
    }
    let media = Media::new();
    let film = media.path("Film (2020)/Film (2020).mkv");
    assert!(clip(&film, 12.0), "film fixture");
    let mut app = fresh_app();
    scan_folder(&mut app, &media.root);
    let id = app.browse.items[0].id.clone();
    assert!(play(&mut app, &id), "the film must start");
    let _ = app.update(Message::BackFromPlayer);
    assert!(
        app.player.is_some(),
        "the player handle is retained after playback, which is what used to hide the banner"
    );
    app.updater = crate::updater::Updater::new();
    app.updater.feed(Event::Found {
        update: AvailableUpdate {
            version: "99.0.0".to_string(),
            display_version: "99.0.0".to_string(),
            notes: String::new(),
            critical: false,
            informational: false,
            feed_signing: "succeeded".to_string(),
        },
        stage: Stage::NotDownloaded,
        user_initiated: true,
    });
    app.screen = Screen::Home;
    assert!(
        app.update_strip().is_some(),
        "a retained player handle must not silence the update banner on Home"
    );
}

#[test]
fn quitting_with_an_update_waiting_stores_the_place_before_handing_over() {
    use crate::updater::{AvailableUpdate, Event};
    if !player_ready() {
        return;
    }
    let media = Media::new();
    let film = media.path("Film (2020)/Film (2020).mkv");
    assert!(clip(&film, 20.0), "film fixture");
    let directory = tempfile::TempDir::new().expect("temp dir");
    let database = directory.path().join("library.db");
    let mut app = app_on(&database);
    scan_folder(&mut app, &media.root);
    let id = app.browse.items[0].id.clone();
    assert!(play(&mut app, &id), "the film must start");
    assert!(until_position(&mut app, 3.0), "playback must advance");
    let watched = app.snapshot.position;

    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    app.updater_backend = Box::new(RecordingBackend { seen: seen.clone() });
    app.updater.feed(Event::Ready {
        update: AvailableUpdate {
            version: "99.0.0".to_string(),
            display_version: "99.0.0".to_string(),
            notes: String::new(),
            critical: false,
            informational: false,
            feed_signing: "succeeded".to_string(),
        },
        deferred_to_quit: true,
    });

    let _ = app.update(Message::Close);

    let stored = stored_position(&app, &id) as f64 / 1_000.0;
    assert!(
        (stored - watched).abs() <= 1.0,
        "the stored position is {stored:.1} s against {watched:.1} s on screen, so the update \
         would have cost the viewer their place"
    );
    assert!(
        app.active.is_none(),
        "the viewing must be closed and checkpointed before anything installs"
    );
    assert!(
        seen.lock().expect("action log").is_empty(),
        "no install action may reach the updater through the quit path"
    );
}
