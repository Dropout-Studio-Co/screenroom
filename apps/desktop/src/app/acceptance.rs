use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::*;

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
    let stride = frame.stride as usize;
    let bytes = frame.bytes();
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
        cards_while_scanning > 0,
        "cards must appear before the scan finishes, not only after"
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
        let (choices, selected) = app.track_chooser(kind);
        let offered: Vec<String> = choices.iter().map(|choice| choice.to_string()).collect();
        assert!(
            offered.iter().any(|label| label.contains("fre")),
            "{kind:?} picker must offer the French track, offered {offered:?}"
        );
        let current = selected.expect("the picker starts on a track").id;

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

    let actions = app.settings_actions();
    assert!(
        actions.len() >= 4,
        "Settings must offer actions, found {}",
        actions.len()
    );

    let autoplay = app.profiles.len().saturating_sub(1);
    assert!(autoplay < actions.len(), "the autoplay action must exist");
    let before = app.preferences.autoplay_next;
    app.settings_focus = autoplay;
    let _ = app.update(key("enter"));
    assert_ne!(
        app.preferences.autoplay_next, before,
        "Enter must run the focused Settings action instead of doing nothing"
    );

    let scale_before = app.preferences.subtitle_scale_percent;
    app.settings_focus = app
        .settings_actions()
        .iter()
        .position(|(label, _)| label.starts_with("Larger subtitles"))
        .expect("the subtitle action must exist");
    let _ = app.update(key("enter"));
    assert!(
        app.preferences.subtitle_scale_percent > scale_before,
        "a keyboard action must raise the subtitle size without a pointer"
    );

    app.settings_focus = 0;
    let _ = app.update(key("down"));
    assert_eq!(app.settings_focus, 1, "Down moves to the next action");
    let _ = app.update(key("up"));
    assert_eq!(app.settings_focus, 0, "Up returns to the first action");
    let _ = app.update(key("up"));
    assert_eq!(app.settings_focus, 0, "the cursor never leaves the list");

    for _ in 0..24 {
        let _ = app.update(key("down"));
    }
    assert!(
        app.settings_focus + 1 == app.settings_actions().len(),
        "repeated Down stops on the last action rather than running past it"
    );

    let _ = app.update(key("escape"));
    assert_eq!(app.screen, Screen::Home, "Escape still leaves Settings");
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

    let labels: Vec<String> = app
        .settings_actions()
        .into_iter()
        .map(|(label, _)| label)
        .collect();
    assert!(
        labels
            .iter()
            .any(|label| label.contains("converted copies")),
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
fn step_20_the_mini_player_keeps_playing_while_you_browse_and_reopens_the_full_one() {
    if !player_ready() {
        return;
    }
    let media = Media::new();
    let path = media.path("Mini Test (2020).mkv");
    assert!(clip(&path, 8.0), "a generated clip is needed");
    settle(&path);
    let mut app = fresh_app();
    scan_folder(&mut app, &media.root);
    let _ = app.update(Message::Tick);
    let id = app.browse.items[0].id.clone();

    assert!(play(&mut app, &id), "the clip plays");
    let load = app.active.as_ref().expect("active").load_id;
    let before = app.snapshot.position;

    let _ = app.update(Message::ReduceToMini);
    assert!(app.mini, "the mini player is on");
    assert_ne!(app.screen, Screen::Player, "and the full player is left");
    assert!(app.player.is_some(), "the decoder keeps running");
    assert_eq!(
        app.active.as_ref().expect("still active").load_id,
        load,
        "shrinking must not reload the file"
    );
    let _ = app.view();

    assert!(
        pump(&mut app, Duration::from_secs(10), |app| app
            .snapshot
            .position
            > before + 0.2),
        "playback must keep running while another screen is shown"
    );
    assert!(app.frame.is_some(), "and a frame is still presented");

    let _ = app.update(Message::ExpandPlayer);
    assert!(!app.mini);
    assert_eq!(app.screen, Screen::Player, "the full player is back");
    assert_eq!(
        app.active.as_ref().expect("same file").load_id,
        load,
        "expanding must not reload the file either"
    );

    let _ = app.update(Message::ReduceToMini);
    let _ = app.update(key("escape"));
    assert!(!app.mini, "escape closes the mini player");
    assert!(app.active.is_none(), "and stops the file");
    assert!(app.frame.is_none(), "with no frame left behind");
    assert!(!app.snapshot.loaded);
    let kept = app
        .library
        .item(&id, Some(&app.profile_id))
        .expect("read")
        .expect("kept");
    assert!(
        kept.position_ms > 0,
        "the place it reached was stored, not thrown away: {kept:#?}"
    );
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
