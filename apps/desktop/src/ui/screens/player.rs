use crate::app::{Message, PlayAction, play_action};
use crate::design;
use crate::ui::components::poster;
use crate::ui::icons::{self, Icon};
use crate::ui::motion::reveal_scale;
use crate::ui::route::Nav;
use crate::ui::theme::{self, Gap};
use crate::video_surface::VideoSurface;
use iced::widget::shader;
use iced::widget::{Space, button, column, container, pick_list, row, slider, stack, text};
use iced::{
    Alignment, Background, Border, Color, Element, Length, Padding, Radians, Shadow, Theme,
    gradient,
};
use screenroom_player::{MediaTrack, NO_TRACK, PlayerSnapshot, TrackKind};
use std::time::Duration;

const TOP_SCRIM_HEIGHT: f32 = 200.0;

pub const BOTTOM_SCRIM_HEIGHT: f32 = 330.0;

const TOP_INSET: f32 = 32.0;

const SCRUB_GAP: f32 = 38.0;

const CHROME_GAP: f32 = 16.0;

const CONTROL_GAP: f32 = 30.0;

const TRACK_GAP: f32 = 26.0;

const CLUSTER_GAP: f32 = 9.0;

const CLOCK_WIDTH: f32 = 44.0;

const RAIL_THICKNESS: f32 = 3.0;

const RAIL_REST_ALPHA: f32 = 0.2;

const HANDLE_RADIUS: f32 = 5.5;

const ICON_SIZE: f32 = 20.0;

const SMALL_ICON_SIZE: f32 = 18.0;

const ICON_PADDING: f32 = 4.0;

const VOLUME_WIDTH: f32 = 80.0;

const SEEK_SECONDS: f64 = 10.0;

const TITLE_CHARS: usize = 56;

const EPISODE_CHARS: usize = 40;

const OUTPUT_CHARS: usize = 16;

const DEFAULT_WIDTH: f32 = 1440.0;

const WIDE: f32 = 1200.0;

const STANDARD: f32 = 1000.0;

const COMPACT: f32 = 900.0;

pub const CHROME_IDLE_AFTER: Duration = Duration::from_millis(2_500);

pub const MARK_HINT: &str =
    "i marks an intro · c marks credits · s skips a marked segment · t finds subtitles";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Control {
    Scrub,

    Subtitles,

    Audio,

    Volume,
}

pub fn control_visible(width: f32, control: Control) -> bool {
    let width = if width.is_finite() {
        width.max(0.0)
    } else {
        0.0
    };

    match control {
        Control::Scrub => true,
        Control::Subtitles => width >= WIDE,
        Control::Audio => width >= STANDARD,
        Control::Volume => width >= COMPACT,
    }
}

pub fn chrome_visible(playing: bool, since_last_input: Duration) -> bool {
    if !playing {
        return true;
    }

    since_last_input < CHROME_IDLE_AFTER
}

pub fn scrub_fraction(position: f64, duration: f64) -> f32 {
    if !position.is_finite() || !duration.is_finite() || duration <= 0.0 {
        return 0.0;
    }

    (position / duration).clamp(0.0, 1.0) as f32
}

pub fn clock(seconds: f64) -> String {
    if !seconds.is_finite() || seconds < 0.0 {
        return "0:00".to_string();
    }

    let total = seconds.round() as u64;
    let (hours, minutes, seconds) = (total / 3600, (total % 3600) / 60, total % 60);

    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackOption {
    pub id: i64,

    pub label: String,
}

impl std::fmt::Display for TrackOption {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.label)
    }
}

pub fn track_options(snapshot: &PlayerSnapshot, kind: TrackKind) -> Vec<TrackOption> {
    let mut options: Vec<TrackOption> = snapshot
        .tracks
        .iter()
        .filter(|track| track.kind == kind)
        .map(|track: &MediaTrack| TrackOption {
            id: track.id,
            label: track.label(),
        })
        .collect();

    if kind == TrackKind::Subtitle && !options.is_empty() {
        options.insert(
            0,
            TrackOption {
                id: NO_TRACK,
                label: "Off".to_string(),
            },
        );
    }

    options
}

pub fn selected_track(
    snapshot: &PlayerSnapshot,
    kind: TrackKind,
    options: &[TrackOption],
) -> Option<TrackOption> {
    let in_use = snapshot
        .tracks
        .iter()
        .find(|track| track.kind == kind && track.selected)
        .map(|track| track.id);

    options
        .iter()
        .find(|option| Some(option.id) == in_use)
        .cloned()
        .or_else(|| options.first().cloned())
}

pub struct Player<'a> {
    pub snapshot: &'a PlayerSnapshot,

    pub title: String,

    pub episode: Option<String>,

    pub output: Option<String>,

    pub nav: &'a Nav,

    pub width: f32,

    pub position: f64,

    pub volume: f64,

    pub chrome: bool,

    pub reveal: f32,

    pub overlay: Option<Element<'a, Message>>,

    pub compact: bool,
}

impl<'a> Player<'a> {
    pub fn new(snapshot: &'a PlayerSnapshot, title: &str, nav: &'a Nav) -> Self {
        Self {
            snapshot,
            title: title.to_string(),
            episode: None,
            output: None,
            nav,
            width: DEFAULT_WIDTH,
            position: snapshot.position,
            volume: snapshot.volume.unwrap_or(0.0),
            chrome: true,
            reveal: 1.0,
            overlay: None,
            compact: false,
        }
    }

    pub fn view(self) -> Element<'a, Message> {
        if !self.chrome {
            return iced::widget::mouse_area(self.picture())
                .on_press(Message::PlayerWake)
                .on_move(|_| Message::PlayerWake)
                .into();
        }

        let reveal = self.reveal.clamp(0.0, 1.0);
        let chrome: Element<'a, Message> = stack![
            self.picture(),
            rising(self.top_scrim(), reveal, -(1.0 - reveal) * RISE),
            rising(self.bottom_scrim(), reveal, (1.0 - reveal) * RISE),
        ]
        .width(Length::Fill)
        .height(Length::Fill)
        .into();

        let visible: Element<'a, Message> = match self.overlay {
            Some(overlay) => {
                let overlay = rising(overlay, reveal, (1.0 - reveal) * RISE);
                stack![
                    chrome,
                    container(column![Space::new().height(Length::Fill), overlay])
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .padding(Padding {
                            top: 0.0,
                            right: theme::PAD,
                            bottom: BOTTOM_SCRIM_HEIGHT,
                            left: theme::PAD,
                        })
                ]
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
            }
            None => chrome,
        };

        iced::widget::mouse_area(visible)
            .on_move(|_| Message::PlayerWake)
            .into()
    }

    fn shows(&self, control: Control) -> bool {
        control_visible(self.width, control)
    }

    fn picture(&self) -> Element<'a, Message> {
        let tokens = design::tokens();

        match (&self.snapshot.failure, self.snapshot.frame.clone()) {
            (Some(failure), _) => container(
                column![
                    text("Playback failed")
                        .size(tokens.type_scale.label)
                        .font(theme::medium())
                        .color(theme::danger()),
                    text(failure.message.clone())
                        .size(tokens.type_scale.caption)
                        .color(theme::muted()),
                ]
                .spacing(theme::gap(Gap::Group))
                .align_x(Alignment::Center),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .style(|_theme| surface(Background::Color(theme::ground())))
            .into(),
            (None, Some(frame)) => shader::Shader::new(VideoSurface::new(Some(frame)))
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
            (None, None) => container(
                text("Preparing streams…")
                    .size(tokens.type_scale.caption)
                    .color(theme::muted()),
            )
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .style(|_theme| surface(Background::Color(theme::ground())))
            .into(),
        }
    }

    fn top_scrim(&self) -> Element<'a, Message> {
        let reveal = self.reveal.clamp(0.0, 1.0);
        let tokens = design::tokens();

        if self.compact {
            return Space::new()
                .width(Length::Fill)
                .height(Length::Shrink)
                .into();
        }

        let mut bar = row![
            icon_button(Icon::Back, ICON_SIZE, Message::BackFromPlayer),
            text(poster::truncate(&self.title, TITLE_CHARS).to_string())
                .size(tokens.type_scale.label)
                .font(theme::medium())
                .color(theme::ink()),
        ]
        .spacing(CHROME_GAP)
        .align_y(Alignment::Center);

        if let Some(episode) = &self.episode {
            bar = bar.push(
                text(poster::truncate(episode, EPISODE_CHARS).to_string())
                    .size(tokens.type_scale.caption)
                    .color(theme::muted()),
            );
        }

        bar = bar.push(Space::new().width(Length::Fill));

        if let Some(output) = &self.output {
            bar = bar.push(
                text(poster::truncate(output, OUTPUT_CHARS).to_string())
                    .size(tokens.type_scale.micro)
                    .font(theme::semibold())
                    .color(theme::muted()),
            );
        }

        container(bar)
            .width(Length::Fill)
            .height(Length::Fixed(TOP_SCRIM_HEIGHT))
            .padding(Padding {
                top: TOP_INSET,
                right: theme::PAD,
                bottom: 0.0,
                left: theme::PAD,
            })
            .style(move |_theme| scrim(TOP_SCRIM, true, reveal))
            .into()
    }

    fn bottom_scrim(&self) -> Element<'a, Message> {
        let tokens = design::tokens();
        let reveal = self.reveal.clamp(0.0, 1.0);
        let duration = self.snapshot.duration.max(0.0);
        let position = if self.position.is_finite() {
            self.position.clamp(0.0, duration)
        } else {
            0.0
        };

        let scrub = row![
            text(clock(position))
                .size(tokens.type_scale.caption)
                .color(theme::ink())
                .width(Length::Fixed(CLOCK_WIDTH)),
            slider(0.0..=duration.max(1.0), position, Message::Scrub)
                .on_release(Message::SeekCommit)
                .step(0.1)
                .width(Length::Fill)
                .style(scrub_style),
            text(clock(duration))
                .size(tokens.type_scale.caption)
                .color(theme::muted())
                .width(Length::Fixed(CLOCK_WIDTH)),
        ]
        .spacing(CHROME_GAP)
        .align_y(Alignment::Center);

        let mut transport = row![
            self.play_button(),
            skip_button(Icon::SkipBack, -SEEK_SECONDS),
            skip_button(Icon::SkipForward, SEEK_SECONDS),
        ]
        .spacing(CONTROL_GAP)
        .align_y(Alignment::Center);

        if self.shows(Control::Volume) {
            transport = transport.push(self.volume_cluster());
        }

        let mut tracks = row![].spacing(TRACK_GAP).align_y(Alignment::Center);

        if self.shows(Control::Subtitles) {
            tracks = tracks.push(self.track_cluster(TrackKind::Subtitle));
        }

        if self.shows(Control::Audio) {
            tracks = tracks.push(self.track_cluster(TrackKind::Audio));
        }

        tracks = tracks.push(icon_button(
            Icon::Fullscreen,
            ICON_SIZE,
            Message::ToggleFullscreen,
        ));

        let controls =
            row![transport, Space::new().width(Length::Fill), tracks,].align_y(Alignment::Center);

        let hint = text(MARK_HINT)
            .size(tokens.type_scale.micro)
            .font(theme::regular())
            .color(theme::muted());

        let block = container(
            column![
                scrub,
                Space::new().height(Length::Fixed(SCRUB_GAP)),
                controls,
                Space::new().height(Length::Fixed(CHROME_GAP)),
                hint,
            ]
            .width(Length::Fill)
            .align_x(Alignment::Start)
            .padding(Padding {
                top: 0.0,
                right: theme::PAD,
                bottom: theme::gap(Gap::Section),
                left: theme::PAD,
            }),
        )
        .width(Length::Fill)
        .height(Length::Fixed(BOTTOM_SCRIM_HEIGHT))
        .align_y(Alignment::End)
        .style(move |_theme| scrim(BOTTOM_SCRIM, false, reveal));

        column![Space::new().height(Length::Fill), block]
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn play_button(&self) -> Element<'a, Message> {
        let icon = match play_action(self.snapshot.state) {
            PlayAction::Pause => Icon::Pause,
            PlayAction::Resume | PlayAction::Replay => Icon::Play,
        };

        icon_button(icon, ICON_SIZE, Message::TogglePlay)
    }

    fn volume_cluster(&self) -> Element<'a, Message> {
        let muted = self.snapshot.muted.unwrap_or(false);
        let volume = if self.volume.is_finite() {
            self.volume.clamp(0.0, 100.0)
        } else {
            0.0
        };

        row![
            icon_button_in(
                Icon::Volume,
                SMALL_ICON_SIZE,
                if muted { theme::muted() } else { theme::ink() },
                Message::ToggleMute,
            ),
            slider(0.0..=100.0, volume, Message::Volume)
                .on_release(Message::VolumeCommit)
                .step(1.0)
                .width(Length::Fixed(VOLUME_WIDTH))
                .style(scrub_style),
        ]
        .spacing(CLUSTER_GAP)
        .align_y(Alignment::Center)
        .into()
    }

    fn track_cluster(&self, kind: TrackKind) -> Element<'a, Message> {
        let tokens = design::tokens();
        let options = track_options(self.snapshot, kind);
        let selected = selected_track(self.snapshot, kind, &options);

        let (caption, mark) = match kind {
            TrackKind::Subtitle => ("Subtitles", Some(Icon::Subtitles)),
            TrackKind::Audio => ("Audio", None),
            TrackKind::Video => ("Video", None),
        };

        if options.is_empty() {
            return text(format!("{caption} · none"))
                .size(tokens.type_scale.caption)
                .color(theme::muted())
                .into();
        }

        let list = pick_list(options, selected, move |option: TrackOption| {
            Message::SelectTrack(kind, option.id)
        })
        .handle(pick_list::Handle::None)
        .padding(0)
        .text_size(tokens.type_scale.caption)
        .font(theme::regular())
        .style(track_style)
        .menu_style(track_menu_style);

        match mark {
            Some(icon) => row![icons::icon(icon, SMALL_ICON_SIZE, theme::muted()), list]
                .spacing(CLUSTER_GAP)
                .align_y(Alignment::Center)
                .into(),
            None => row![
                text(format!("{caption} ·"))
                    .size(tokens.type_scale.caption)
                    .color(theme::muted()),
                list,
            ]
            .spacing(theme::gap(Gap::Tight))
            .align_y(Alignment::Center)
            .into(),
        }
    }
}

fn icon_button<'a>(icon: Icon, size: f32, message: Message) -> Element<'a, Message> {
    icon_button_in(icon, size, theme::ink(), message)
}

fn icon_button_in<'a>(
    icon: Icon,
    size: f32,
    color: Color,
    message: Message,
) -> Element<'a, Message> {
    button(icons::icon(icon, size, color))
        .on_press(message)
        .padding(ICON_PADDING)
        .style(icon_button_style)
        .into()
}

fn skip_button<'a>(icon: Icon, seconds: f64) -> Element<'a, Message> {
    icon_button(icon, ICON_SIZE, Message::SeekStep(seconds))
}

fn scrim(stops: [(f32, f32); 3], from_top: bool, reveal: f32) -> container::Style {
    let ground = theme::ground();
    let angle = if from_top {
        Radians(std::f32::consts::PI)
    } else {
        Radians(0.0)
    };

    let linear = stops
        .into_iter()
        .fold(gradient::Linear::new(angle), |linear, (offset, alpha)| {
            linear.add_stop(offset, ground.scale_alpha(alpha * reveal))
        });

    surface(Background::from(linear))
}

const RISE: f32 = 14.0;

fn rising<'a>(element: Element<'a, Message>, reveal: f32, offset: f32) -> Element<'a, Message> {
    if offset.abs() < 0.01 && (reveal - 1.0).abs() < 1.0e-3 {
        return element;
    }
    iced::widget::Float::new(element)
        .scale(reveal_scale(reveal))
        .translate(move |_bounds, _viewport| iced::Vector::new(0.0, offset))
        .style(|_theme| iced::widget::float::Style::default())
        .into()
}

const TOP_SCRIM: [(f32, f32); 3] = [(0.0, 0.75), (0.55, 0.28), (1.0, 0.0)];

const BOTTOM_SCRIM: [(f32, f32); 3] = [(0.0, 0.94), (0.44, 0.58), (1.0, 0.0)];

fn surface(background: Background) -> container::Style {
    container::Style {
        background: Some(background),
        text_color: Some(theme::ink()),
        border: Border::default(),
        shadow: Shadow::default(),
        snap: true,
    }
}

fn scrub_style(_theme: &Theme, _status: slider::Status) -> slider::Style {
    slider::Style {
        rail: slider::Rail {
            backgrounds: (
                Background::Color(theme::signal()),
                Background::Color(theme::signal().scale_alpha(RAIL_REST_ALPHA)),
            ),
            width: RAIL_THICKNESS,
            border: Border {
                radius: (RAIL_THICKNESS / 2.0).into(),
                ..Border::default()
            },
        },
        handle: slider::Handle {
            shape: slider::HandleShape::Circle {
                radius: HANDLE_RADIUS,
            },
            background: Background::Color(theme::signal()),
            border_width: 0.0,
            border_color: Color::TRANSPARENT,
        },
    }
}

fn icon_button_style(_theme: &Theme, status: button::Status) -> button::Style {
    let background = match status {
        button::Status::Hovered | button::Status::Pressed => {
            Some(Background::Color(theme::signal().scale_alpha(0.12)))
        }
        _ => None,
    };

    button::Style {
        background,
        text_color: theme::ink(),
        border: Border {
            radius: theme::radius(theme::Radius::Small).into(),
            ..Border::default()
        },
        shadow: Shadow::default(),
        snap: false,
    }
}

fn track_style(_theme: &Theme, status: pick_list::Status) -> pick_list::Style {
    let background = match status {
        pick_list::Status::Hovered | pick_list::Status::Opened { .. } => {
            Background::Color(theme::signal().scale_alpha(0.10))
        }
        pick_list::Status::Active => Background::Color(Color::TRANSPARENT),
    };

    pick_list::Style {
        text_color: theme::ink(),
        placeholder_color: theme::muted(),
        handle_color: theme::muted(),
        background,
        border: Border {
            radius: theme::radius(theme::Radius::Small).into(),
            ..Border::default()
        },
    }
}

fn track_menu_style(_theme: &Theme) -> iced::widget::overlay::menu::Style {
    iced::widget::overlay::menu::Style {
        background: Background::Color(theme::surface(3)),
        border: Border {
            color: theme::hair(),
            width: 1.0,
            radius: theme::radius(theme::Radius::Control).into(),
        },
        text_color: theme::ink(),
        selected_text_color: theme::ink(),
        selected_background: Background::Color(theme::surface(3)),
        shadow: Shadow::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Screen;

    #[test]
    fn a_zero_or_negative_duration_never_produces_a_nan_scrub() {
        assert_eq!(scrub_fraction(10.0, 0.0), 0.0);
        assert_eq!(scrub_fraction(-5.0, 100.0), 0.0);
        assert_eq!(scrub_fraction(250.0, 100.0), 1.0);
        assert!(scrub_fraction(50.0, 100.0).is_finite());
    }

    #[test]
    fn a_position_that_is_not_a_number_never_produces_a_nan_fraction() {
        assert_eq!(scrub_fraction(f64::NAN, 100.0), 0.0);
        assert_eq!(scrub_fraction(50.0, f64::NAN), 0.0);
        assert_eq!(scrub_fraction(f64::NAN, f64::NAN), 0.0);
        assert!(scrub_fraction(f64::INFINITY, 100.0).is_finite());
        assert!(scrub_fraction(50.0, f64::INFINITY).is_finite());
        assert!(scrub_fraction(f64::NEG_INFINITY, f64::NEG_INFINITY).is_finite());
        assert_eq!(scrub_fraction(0.0, 100.0), 0.0);
        assert_eq!(scrub_fraction(100.0, 100.0), 1.0);
    }

    #[test]
    fn the_clock_reads_like_a_clock() {
        assert_eq!(clock(0.0), "0:00");
        assert_eq!(clock(65.0), "1:05");
        assert_eq!(clock(3725.0), "1:02:05");
        assert_eq!(clock(-4.0), "0:00");
        assert_eq!(clock(f64::NAN), "0:00");
    }

    #[test]
    fn the_control_row_drops_the_right_controls_at_the_narrow_breakpoint() {
        for control in [Control::Subtitles, Control::Audio, Control::Volume] {
            assert!(
                !control_visible(800.0, control),
                "{control:?} must leave a narrow window"
            );
            assert!(
                control_visible(1440.0, control),
                "{control:?} must stay in a wide window"
            );
        }

        assert!(
            control_visible(320.0, Control::Scrub) && control_visible(800.0, Control::Scrub),
            "the scrub is never dropped"
        );

        assert!(control_visible(900.0, Control::Volume));
        assert!(!control_visible(900.0, Control::Audio));
        assert!(!control_visible(900.0, Control::Subtitles));

        assert!(control_visible(1000.0, Control::Audio));
        assert!(!control_visible(1000.0, Control::Subtitles));

        assert!(control_visible(1200.0, Control::Subtitles));
    }

    #[test]
    fn a_paused_film_keeps_its_controls_no_matter_how_long_nothing_happens() {
        for idle in [
            Duration::ZERO,
            Duration::from_millis(400),
            CHROME_IDLE_AFTER,
            Duration::from_secs(90),
        ] {
            assert!(
                chrome_visible(false, idle),
                "a paused film must not become a dead end after {idle:?}"
            );
        }
    }

    #[test]
    fn input_within_the_idle_window_keeps_the_chrome_up_during_playback() {
        for idle in [
            Duration::ZERO,
            Duration::from_millis(1),
            Duration::from_millis(1_200),
            CHROME_IDLE_AFTER - Duration::from_millis(1),
        ] {
            assert!(
                chrome_visible(true, idle),
                "the chrome must answer input that is only {idle:?} old"
            );
        }
    }

    #[test]
    fn a_playing_film_recedes_once_the_idle_window_has_passed() {
        assert!(!chrome_visible(true, CHROME_IDLE_AFTER));
        assert!(!chrome_visible(
            true,
            CHROME_IDLE_AFTER + Duration::from_millis(1)
        ));
        assert!(!chrome_visible(true, Duration::from_secs(30)));
    }

    #[test]
    fn the_idle_window_is_the_two_and_a_half_seconds_the_spec_asks_for() {
        assert_eq!(CHROME_IDLE_AFTER, Duration::from_millis(2_500));
    }

    #[test]
    fn the_chrome_composes_over_the_picture_at_every_breakpoint() {
        let snapshot = PlayerSnapshot::default();
        let nav = Nav::new(Screen::Player);

        for width in [800.0, 1000.0, 1280.0, 1440.0, 3440.0] {
            let player = Player {
                episode: Some("S01E03 · Caprice".to_string()),
                output: Some("2160p".to_string()),
                width,
                ..Player::new(&snapshot, "Northern Lights", &nav)
            };
            let _ = player.view();

            let bare = Player {
                chrome: false,
                ..Player::new(&snapshot, "Northern Lights", &nav)
            };
            let _ = bare.view();
        }
    }
}
