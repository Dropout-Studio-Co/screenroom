use crate::app::Message;
use crate::design;
use crate::ui::icons::{self, Icon};
use crate::ui::route::Nav;
use crate::ui::shell::BAR_HEIGHT;
use crate::ui::theme::{self, Breakpoint, Gap, Radius};
use iced::widget::canvas::gradient::Linear;
use iced::widget::canvas::{self, Canvas, Program};
use iced::widget::text::Wrapping;
use iced::widget::{Space, button, column, container, image, row, scrollable, stack, text};
use iced::{
    Alignment, Background, Border, Color, ContentFit, Element, Length, Padding, Point, Rectangle,
    Renderer, Shadow, Size, Theme, mouse,
};
use screenroom_core::{LibraryItem, MediaKind};
use std::collections::HashMap;

const HERO_HEIGHT: f32 = 452.0;

const SYNOPSIS_WIDTH: f32 = 600.0;

const SYNOPSIS_COLUMNS: usize = 78;

const SYNOPSIS_LINE: f32 = 1.65;

const EPISODE_COLUMNS: usize = 96;

const STILL: (f32, f32) = (120.0, 68.0);

const INDEX_WIDTH: f32 = 20.0;

const ROUND_ACTION: f32 = 44.0;

const PLAY_CHIP: f32 = 36.0;

const HEADLINE_CHARS: usize = 96;

const BACKDROP_BANDS: u16 = 48;

pub struct Detail<'a> {
    pub item: &'a LibraryItem,

    pub episodes: &'a [LibraryItem],

    pub seasons: &'a [i64],

    pub season: i64,

    pub nav: &'a Nav,

    pub facts: String,

    pub art: &'a HashMap<String, image::Handle>,

    pub width: f32,

    pub extra: Option<Element<'a, Message>>,
}

impl<'a> Detail<'a> {
    pub fn view(mut self) -> Element<'a, Message> {
        let breakpoint = self.breakpoint();
        let hero = self.hero();

        let mut rest: Vec<Element<'a, Message>> = vec![gutter(self.under_hero())];
        if shows_episodes(self.item, self.episodes) {
            rest.push(gutter(self.season_header()));
            rest.push(gutter(self.episode_list()));
        }
        if let Some(extra) = self.extra.take() {
            rest.push(gutter(extra));
        }

        let rest = capped(
            iced::widget::Column::with_children(rest)
                .width(Length::Fill)
                .into(),
            breakpoint,
        );

        let page = column![hero, rest].width(Length::Fill);
        scrollable(page).height(Length::Fill).into()
    }

    fn breakpoint(&self) -> Breakpoint {
        Breakpoint::at(if self.width.is_finite() && self.width > 0.0 {
            self.width
        } else {
            1440.0
        })
    }

    fn hero(&self) -> Element<'a, Message> {
        let tokens = design::tokens();

        let back = button(
            row![
                icons::icon(Icon::Back, 16.0, theme::muted()),
                text("Back to library")
                    .size(tokens.type_scale.caption)
                    .font(theme::regular())
                    .color(theme::muted()),
            ]
            .spacing(theme::gap(Gap::Card))
            .align_y(Alignment::Center),
        )
        .on_press(Message::Back)
        .padding(0)
        .style(quiet_button);

        let eyebrow = text(eyebrow(self.item.kind))
            .size(tokens.type_scale.micro)
            .font(theme::medium())
            .color(theme::muted());

        let headline = text(headline(&self.item.title))
            .size(tokens.type_scale.title_lg)
            .font(theme::semibold())
            .color(theme::ink())
            .width(Length::Fill)
            .wrapping(Wrapping::WordOrGlyph);

        let heading = column![eyebrow, headline].spacing(theme::gap(Gap::Tight));

        let metadata = text(metadata_line(self.item, self.seasons, self.episodes))
            .size(tokens.type_scale.label)
            .font(theme::regular())
            .color(theme::muted());

        let mut actions = row![self.primary()].spacing(theme::gap(Gap::Card));
        actions = actions
            .push(self.round_action(
                if self.item.watchlisted {
                    Icon::Check
                } else {
                    Icon::Plus
                },
                self.item.watchlisted,
                Message::ToggleWatchlist(self.item.work_id.clone()),
            ))
            .push(self.round_action(
                Icon::Check,
                self.item.watched,
                Message::ToggleWatched(self.item.edition_id.clone()),
            ))
            .align_y(Alignment::Center);

        let lower = column![metadata, actions].spacing(theme::gap(Gap::Rail));

        let block = column![heading, lower].spacing(theme::gap(Gap::Card));

        let height = self.breakpoint().hero().min(HERO_HEIGHT);

        let content = column![back, Space::new().height(Length::Fill), block]
            .width(Length::Fill)
            .height(Length::Fixed(height))
            .padding(Padding {
                top: BAR_HEIGHT + theme::gap(Gap::Group),
                bottom: theme::gap(Gap::Section),
                left: theme::PAD + theme::content_inset(self.width),
                right: theme::PAD + theme::content_inset(self.width),
            });

        let backdrop: Element<'a, Message> = match self.art.get(&self.item.work_id) {
            Some(handle) => stack![
                image(handle.clone())
                    .width(Length::Fill)
                    .height(Length::Fixed(height))
                    .content_fit(ContentFit::Cover),
                Canvas::new(HeroScrim)
                    .width(Length::Fill)
                    .height(Length::Fixed(height)),
            ]
            .into(),
            None => Canvas::new(HeroBackdrop)
                .width(Length::Fill)
                .height(Length::Fixed(height))
                .into(),
        };

        container(stack![backdrop, content])
            .width(Length::Fill)
            .height(Length::Fixed(height))
            .clip(true)
            .into()
    }

    fn primary(&self) -> Element<'a, Message> {
        let tokens = design::tokens();
        let (label, message) = primary_action(self.item);

        button(
            row![
                icons::icon(Icon::Play, 16.0, theme::ground()),
                text(label)
                    .size(tokens.type_scale.label)
                    .font(theme::medium())
                    .color(theme::signal_foreground()),
            ]
            .spacing(theme::gap(Gap::Card))
            .align_y(Alignment::Center),
        )
        .on_press_maybe(self.item.available.then_some(message))
        .padding(Padding {
            top: theme::gap(Gap::Group),
            bottom: theme::gap(Gap::Group),
            left: theme::gap(Gap::Rail),
            right: theme::gap(Gap::Rail),
        })
        .style(primary_button)
        .into()
    }

    fn round_action(&self, icon: Icon, active: bool, message: Message) -> Element<'a, Message> {
        let colour = if active {
            theme::ground()
        } else {
            theme::ink()
        };

        button(
            container(icons::icon(icon, 16.0, colour))
                .width(Length::Fixed(ROUND_ACTION))
                .height(Length::Fixed(ROUND_ACTION))
                .align_x(Alignment::Center)
                .align_y(Alignment::Center),
        )
        .on_press(message)
        .padding(0)
        .style(move |_theme, status| round_button(active, status))
        .into()
    }

    fn under_hero(&self) -> Element<'a, Message> {
        let tokens = design::tokens();

        let mut synopsis = column![]
            .width(Length::Fixed(synopsis_width(self.width, self.breakpoint())))
            .spacing(0.0);

        let stored = if self.item.overview.trim().is_empty() {
            "No synopsis stored for this title yet.".to_string()
        } else {
            self.item.overview.clone()
        };

        for line in wrap(&stored, SYNOPSIS_COLUMNS) {
            synopsis = synopsis.push(
                text(line)
                    .size(tokens.type_scale.label)
                    .font(theme::regular())
                    .color(theme::muted())
                    .line_height(SYNOPSIS_LINE)
                    .width(Length::Fill)
                    .wrapping(Wrapping::WordOrGlyph),
            );
        }

        let mut facts = column![].spacing(theme::gap(Gap::Card));
        for line in fact_lines(&self.facts) {
            facts = facts.push(
                text(line)
                    .size(tokens.type_scale.caption)
                    .font(theme::regular())
                    .color(theme::muted())
                    .width(Length::Fill)
                    .wrapping(Wrapping::WordOrGlyph),
            );
        }

        let columns = row![synopsis, facts]
            .spacing(theme::gap(Gap::Section))
            .align_y(Alignment::Start)
            .width(Length::Fill);

        let mut body = column![].spacing(theme::gap(Gap::Card));

        if !self.item.available {
            body = body.push(
                text("This file is not readable right now. Relink its source and rescan.")
                    .size(tokens.type_scale.label)
                    .font(theme::regular())
                    .color(theme::danger()),
            );
        }

        body.push(columns)
            .padding(Padding {
                top: theme::gap(Gap::Section),
                bottom: theme::gap(Gap::Section),
                left: 0.0,
                right: 0.0,
            })
            .width(Length::Fill)
            .into()
    }

    fn season_header(&self) -> Element<'a, Message> {
        let tokens = design::tokens();
        let held = self.episodes.len();

        let heading = row![
            text(format!("Season {}", self.season))
                .size(tokens.type_scale.label)
                .font(theme::semibold())
                .color(theme::ink()),
            text(if held == 1 {
                "1 episode".to_string()
            } else {
                format!("{held} episodes")
            })
            .size(tokens.type_scale.caption)
            .font(theme::regular())
            .color(theme::muted()),
        ]
        .spacing(theme::gap(Gap::Card))
        .align_y(Alignment::Center);

        let mut chips = row![]
            .spacing(theme::gap(Gap::Tight))
            .align_y(Alignment::Center);
        for season in self.seasons {
            let message = (*season != self.season).then_some(Message::SeasonChosen(*season));
            chips = chips.push(self.season_chip(*season, *season == self.season, message));
        }

        let selector: Element<'_, Message> = if self.seasons.len() > 1 {
            chips.into()
        } else {
            Space::new().width(Length::Shrink).into()
        };

        row![heading, Space::new().width(Length::Fill), selector]
            .align_y(Alignment::Center)
            .width(Length::Fill)
            .into()
    }

    fn season_chip(
        &self,
        season: i64,
        current: bool,
        message: Option<Message>,
    ) -> Element<'a, Message> {
        let tokens = design::tokens();

        button(
            text(format!("Season {season}"))
                .size(tokens.type_scale.caption)
                .font(if current {
                    theme::medium()
                } else {
                    theme::regular()
                })
                .color(if current {
                    theme::ink()
                } else {
                    theme::muted()
                }),
        )
        .on_press_maybe(message)
        .padding(Padding {
            top: theme::gap(Gap::Tight),
            bottom: theme::gap(Gap::Tight),
            left: theme::gap(Gap::Group),
            right: theme::gap(Gap::Group),
        })
        .style(move |_theme, status| chip_button(current, status))
        .into()
    }

    fn episode_list(&self) -> Element<'a, Message> {
        let focused = self.nav.focus().column;

        let mut list = column![]
            .width(Length::Fill)
            .spacing(theme::gap(Gap::Tight));
        for (index, episode) in self.episodes.iter().enumerate() {
            list = list.push(self.episode_row(index, episode, focused == index));
        }

        list.padding(Padding {
            top: 0.0,
            bottom: theme::gap(Gap::Section),
            left: 0.0,
            right: 0.0,
        })
        .into()
    }

    fn episode_row(
        &self,
        index: usize,
        episode: &LibraryItem,
        current: bool,
    ) -> Element<'a, Message> {
        let tokens = design::tokens();

        let number = text(format!("{}", index + 1))
            .size(tokens.type_scale.micro)
            .font(theme::regular())
            .color(theme::muted())
            .width(Length::Fixed(INDEX_WIDTH));

        let still: Element<'a, Message> = match self.art.get(&episode.work_id) {
            Some(handle) => container(
                image(handle.clone())
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .content_fit(ContentFit::Cover),
            )
            .width(Length::Fixed(STILL.0))
            .height(Length::Fixed(STILL.1))
            .clip(true)
            .style(still_frame)
            .into(),
            None => container(Space::new().width(Length::Fill).height(Length::Fill))
                .width(Length::Fixed(STILL.0))
                .height(Length::Fixed(STILL.1))
                .style(still_frame)
                .into(),
        };

        let name = if episode.episode_title.trim().is_empty() {
            episode.title.clone()
        } else {
            episode.episode_title.clone()
        };

        let heading = row![
            text(truncate(&name, 64).to_string())
                .size(tokens.type_scale.label)
                .font(theme::medium())
                .color(if episode.watched {
                    theme::muted()
                } else {
                    theme::ink()
                })
                .wrapping(Wrapping::None),
            text(runtime(episode.duration_ms))
                .size(tokens.type_scale.caption)
                .font(theme::regular())
                .color(theme::muted()),
        ]
        .spacing(theme::gap(Gap::Card))
        .align_y(Alignment::Center);

        let stored = if episode.episode_overview.trim().is_empty() {
            episode.overview.clone()
        } else {
            episode.episode_overview.clone()
        };

        let mut lines = column![heading]
            .width(Length::Fill)
            .spacing(theme::gap(Gap::Tight));
        for line in two_lines(&stored, EPISODE_COLUMNS) {
            lines = lines.push(
                text(line)
                    .size(tokens.type_scale.caption)
                    .font(theme::regular())
                    .color(theme::muted())
                    .width(Length::Fill)
                    .wrapping(Wrapping::WordOrGlyph),
            );
        }

        let chip_colour = if current {
            theme::ground()
        } else {
            theme::muted()
        };
        let chip = container(icons::icon(Icon::Play, 14.0, chip_colour))
            .width(Length::Fixed(PLAY_CHIP))
            .height(Length::Fixed(PLAY_CHIP))
            .align_x(Alignment::Center)
            .align_y(Alignment::Center)
            .style(move |_theme| play_chip(current));

        let content = row![number, still, lines, chip]
            .spacing(theme::gap(Gap::Group))
            .align_y(Alignment::Center)
            .width(Length::Fill);

        button(content)
            .on_press_maybe(episode.available.then(|| Message::Play(episode.id.clone())))
            .width(Length::Fill)
            .padding(Padding {
                top: theme::gap(Gap::Group),
                bottom: theme::gap(Gap::Group),
                left: theme::gap(Gap::Card),
                right: theme::gap(Gap::Group),
            })
            .style(move |_theme, status| episode_button(current, status))
            .into()
    }
}

pub fn primary_action(item: &LibraryItem) -> (String, Message) {
    let label = if item.kind == MediaKind::Series {
        let episode = item.episode_label();
        if episode.is_empty() {
            "Play".to_string()
        } else {
            format!("Play {episode}")
        }
    } else if item.position_ms > 0 {
        format!("Resume {} min in", resume_minutes(item.position_ms))
    } else {
        "Play".to_string()
    };

    (label, Message::Play(item.id.clone()))
}

pub fn resume_minutes(position_ms: i64) -> i64 {
    let minutes = ((position_ms.max(0) as f64) / 60_000.0).round() as i64;
    minutes.max(1)
}

pub fn shows_episodes(item: &LibraryItem, episodes: &[LibraryItem]) -> bool {
    item.kind == MediaKind::Series && !episodes.is_empty()
}

pub fn metadata_line(item: &LibraryItem, seasons: &[i64], episodes: &[LibraryItem]) -> String {
    let mut parts: Vec<String> = Vec::new();

    if let Some(year) = item.year {
        parts.push(year.to_string());
    }

    if item.kind == MediaKind::Series {
        let count = season_count(seasons, episodes);
        if count > 0 {
            parts.push(if count == 1 {
                "1 season".to_string()
            } else {
                format!("{count} seasons")
            });
        }
        if !episodes.is_empty() {
            parts.push(if episodes.len() == 1 {
                "1 episode".to_string()
            } else {
                format!("{} episodes", episodes.len())
            });
        }
    } else if item.duration_ms > 0 {
        parts.push(runtime(item.duration_ms));
    }

    if !item.genres.is_empty() {
        parts.push(item.genres.join(", "));
    }

    parts.join(" \u{b7} ")
}

pub fn season_count(seasons: &[i64], episodes: &[LibraryItem]) -> usize {
    let mut found: Vec<i64> = seasons.to_vec();
    for episode in episodes {
        if let Some(season) = episode.season
            && !found.contains(&season)
        {
            found.push(season);
        }
    }
    found.len()
}

pub fn runtime(duration_ms: i64) -> String {
    if duration_ms <= 0 {
        return String::new();
    }
    let minutes = ((duration_ms as f64) / 60_000.0).round() as i64;
    if minutes >= 60 {
        format!("{}h {:02}m", minutes / 60, minutes % 60)
    } else {
        format!("{minutes}m")
    }
}

pub fn wrap(text: &str, max_chars: usize) -> Vec<String> {
    if max_chars == 0 {
        return Vec::new();
    }

    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        let mut current = String::new();
        for word in paragraph.split_whitespace() {
            for piece in hard_split(word, max_chars) {
                let width = current.chars().count();
                let projected = if width == 0 {
                    piece.chars().count()
                } else {
                    width + 1 + piece.chars().count()
                };
                if projected > max_chars && width > 0 {
                    lines.push(std::mem::take(&mut current));
                }
                if !current.is_empty() {
                    current.push(' ');
                }
                current.push_str(piece);
            }
        }
        lines.push(current);
    }
    lines
}

pub fn fact_lines(facts: &str) -> Vec<String> {
    let lines: Vec<String> = facts
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect();

    if lines.is_empty() {
        vec!["No track facts reported yet.".to_string()]
    } else {
        lines
    }
}

fn hard_split(word: &str, max_chars: usize) -> Vec<&str> {
    if word.chars().count() <= max_chars {
        return vec![word];
    }

    let mut pieces = Vec::new();
    let mut rest = word;
    while rest.chars().count() > max_chars {
        let index = rest
            .char_indices()
            .nth(max_chars)
            .map(|(index, _)| index)
            .unwrap_or(rest.len());
        pieces.push(&rest[..index]);
        rest = &rest[index..];
    }
    if !rest.is_empty() {
        pieces.push(rest);
    }
    pieces
}

fn two_lines(text: &str, max_chars: usize) -> Vec<String> {
    let mut lines = wrap(text, max_chars);
    if lines.len() > 2 {
        lines.truncate(2);
        if let Some(last) = lines.last_mut() {
            last.push('\u{2026}');
        }
    }
    lines
}

pub fn eyebrow(kind: MediaKind) -> &'static str {
    match kind {
        MediaKind::Movie => "FILM",
        MediaKind::Series => "SERIES",
        MediaKind::Unknown => "TITLE",
    }
}

fn headline(title: &str) -> String {
    let total = title.chars().count();
    if total <= HEADLINE_CHARS {
        return title.to_string();
    }
    let cut: String = title.chars().take(HEADLINE_CHARS - 1).collect();
    format!("{}\u{2026}", cut.trim_end())
}

fn truncate(text: &str, max_chars: usize) -> &str {
    match text.char_indices().nth(max_chars) {
        Some((index, _)) => &text[..index],
        None => text,
    }
}

fn capped<'a>(body: Element<'a, Message>, breakpoint: Breakpoint) -> Element<'a, Message> {
    match breakpoint.content_cap() {
        Some(cap) => container(container(body).width(Length::Fill).max_width(cap))
            .width(Length::Fill)
            .center_x(Length::Fill)
            .into(),
        None => body,
    }
}

fn gutter<'a>(body: Element<'a, Message>) -> Element<'a, Message> {
    container(body)
        .width(Length::Fill)
        .padding(Padding {
            top: 0.0,
            bottom: 0.0,
            left: theme::PAD,
            right: theme::PAD,
        })
        .into()
}

fn synopsis_width(width: f32, breakpoint: Breakpoint) -> f32 {
    let window = if width.is_finite() && width > 0.0 {
        width
    } else {
        1440.0
    };
    let content = (window - 2.0 * theme::PAD).max(320.0);
    let capped = breakpoint
        .content_cap()
        .map_or(content, |cap| content.min(cap));
    (capped * 0.44).clamp(320.0, SYNOPSIS_WIDTH)
}

struct HeroScrim;

impl Program<Message> for HeroScrim {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let scrim = |alpha: f32| Color {
            a: alpha,
            ..theme::ground()
        };

        frame.fill_rectangle(
            Point::ORIGIN,
            bounds.size(),
            canvas::Fill::from(canvas::Gradient::from(
                Linear::new(Point::ORIGIN, Point::new(0.0, bounds.height))
                    .add_stop(0.0, scrim(0.35))
                    .add_stop(0.5, scrim(0.7))
                    .add_stop(1.0, scrim(1.0)),
            )),
        );

        vec![frame.into_geometry()]
    }
}

struct HeroBackdrop;

impl Program<Message> for HeroBackdrop {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let top = theme::surface(2);
        let bottom = theme::ground();
        let band = bounds.height / f32::from(BACKDROP_BANDS);

        for index in 0..BACKDROP_BANDS {
            let ratio = f32::from(index) / f32::from(BACKDROP_BANDS - 1);
            let wash = ratio * ratio;
            let colour = Color {
                r: mood(top.r, bottom.r, wash),
                g: mood(top.g, bottom.g, wash),
                b: mood(top.b, bottom.b, wash),
                a: mood(top.a, bottom.a, wash),
            };
            frame.fill_rectangle(
                Point::new(0.0, f32::from(index) * band),
                Size::new(bounds.width, band + 1.0),
                colour,
            );
        }

        vec![frame.into_geometry()]
    }
}

fn mood(from: f32, to: f32, ratio: f32) -> f32 {
    from + (to - from) * ratio
}

fn primary_button(_theme: &Theme, status: button::Status) -> button::Style {
    let background = match status {
        button::Status::Hovered | button::Status::Pressed => theme::ink(),
        button::Status::Disabled => theme::surface(3),
        _ => theme::signal(),
    };

    button::Style {
        background: Some(Background::Color(background)),
        text_color: theme::ground(),
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: theme::radius(Radius::Small).into(),
        },
        shadow: Shadow::default(),
        snap: true,
    }
}

fn round_button(active: bool, status: button::Status) -> button::Style {
    let background = if active {
        theme::ink()
    } else {
        match status {
            button::Status::Hovered | button::Status::Pressed => theme::surface(3),
            _ => theme::surface(2),
        }
    };

    button::Style {
        background: Some(Background::Color(background)),
        text_color: theme::ink(),
        border: Border {
            color: theme::hair(),
            width: 1.0,
            radius: (ROUND_ACTION / 2.0).into(),
        },
        shadow: Shadow::default(),
        snap: true,
    }
}

fn chip_button(current: bool, status: button::Status) -> button::Style {
    let background = if current {
        theme::surface(3)
    } else {
        match status {
            button::Status::Hovered | button::Status::Pressed => theme::surface(2),
            _ => Color::TRANSPARENT,
        }
    };

    button::Style {
        background: Some(Background::Color(background)),
        text_color: theme::ink(),
        border: Border {
            color: if current {
                Color::TRANSPARENT
            } else {
                theme::hair()
            },
            width: 1.0,
            radius: theme::radius(Radius::Small).into(),
        },
        shadow: Shadow::default(),
        snap: true,
    }
}

fn episode_button(current: bool, status: button::Status) -> button::Style {
    let background = if current {
        theme::surface(2)
    } else {
        match status {
            button::Status::Hovered | button::Status::Pressed => theme::surface(1),
            _ => Color::TRANSPARENT,
        }
    };

    button::Style {
        background: Some(Background::Color(background)),
        text_color: theme::ink(),
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: theme::radius(Radius::Small).into(),
        },
        shadow: Shadow::default(),
        snap: true,
    }
}

fn play_chip(current: bool) -> container::Style {
    container::Style {
        background: Some(Background::Color(if current {
            theme::ink()
        } else {
            theme::surface(1)
        })),
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: (PLAY_CHIP / 2.0).into(),
        },
        text_color: Some(theme::ink()),
        shadow: Shadow::default(),
        snap: true,
    }
}

fn still_frame(_theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(theme::surface(2))),
        border: Border {
            color: theme::hair(),
            width: 1.0,
            radius: theme::radius(Radius::Small).into(),
        },
        text_color: Some(theme::ink()),
        shadow: Shadow::default(),
        snap: true,
    }
}

fn quiet_button(_theme: &Theme, _status: button::Status) -> button::Style {
    button::Style {
        background: None,
        text_color: theme::ink(),
        border: Border::default(),
        shadow: Shadow::default(),
        snap: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Screen;
    use crate::ui::route::Nav;
    use crate::ui::screens::detail;
    use screenroom_core::{LibraryItem, MediaKind};
    use std::collections::HashMap;

    fn item(kind: MediaKind, position: Option<i64>) -> LibraryItem {
        LibraryItem {
            id: format!("{}-{}", kind.as_str(), position.unwrap_or_default()),
            work_id: "tulsa-king".to_string(),
            edition_id: "edition".to_string(),
            source_id: "source".to_string(),
            title: "Tulsa King".to_string(),
            filename: "Tulsa King S01E03.mkv".to_string(),
            relative_path: "Tulsa King/Season 1/Tulsa King S01E03.mkv".to_string(),
            kind,
            overview: "A New York mobster is exiled to Tulsa.".to_string(),
            episode_title: "Caprice".to_string(),
            episode_overview: "Dwight takes a meeting that changes the shape of his plans."
                .to_string(),
            genres: vec!["Crime".to_string()],
            year: Some(2022),
            season: (kind == MediaKind::Series).then_some(1),
            episode: (kind == MediaKind::Series).then_some(3),
            episode_end: None,
            duration_ms: 2_760_000,
            position_ms: position.unwrap_or(0) * 1_000,
            added_ms: 0,
            available: true,
            watched: false,
            favorite: false,
            watchlisted: false,
            manual_match: false,
            metadata_origin: "nfo".to_string(),
        }
    }

    #[test]
    fn the_primary_action_names_what_it_will_do() {
        let film = item(MediaKind::Movie, Some(1_500));
        assert_eq!(detail::primary_action(&film).0, "Resume 25 min in");
        let fresh = item(MediaKind::Movie, Some(0));
        assert_eq!(detail::primary_action(&fresh).0, "Play");
        let episode = item(MediaKind::Series, Some(0));
        assert_eq!(detail::primary_action(&episode).0, "Play S01E03");
    }

    #[test]
    fn a_series_with_no_episodes_renders_the_film_page_instead_of_an_empty_section() {
        let series = item(MediaKind::Series, Some(0));
        let nav = Nav::new(Screen::Detail);
        let art = HashMap::new();
        let page = Detail {
            item: &series,
            episodes: &[],
            seasons: &[],
            season: 1,
            nav: &nav,
            facts: "Video \u{b7} HEVC 10-bit \u{b7} 3840\u{d7}2160".to_string(),
            art: &art,
            width: 1440.0,
            extra: None,
        };

        let _ = page.view();

        assert!(!detail::shows_episodes(&series, &[]));
        assert_eq!(
            detail::metadata_line(&series, &[], &[]),
            "2022 \u{b7} Crime"
        );
        assert_eq!(detail::season_count(&[], &[]), 0);
    }

    #[test]
    fn a_series_with_one_episode_still_lists_it() {
        let series = item(MediaKind::Series, Some(0));
        let episodes = [series.clone()];
        let nav = Nav::new(Screen::Detail);
        let art = HashMap::new();
        let page = Detail {
            item: &series,
            episodes: &episodes,
            seasons: &[1],
            season: 1,
            nav: &nav,
            facts: String::new(),
            art: &art,
            width: 1440.0,
            extra: None,
        };

        let _ = page.view();

        assert!(detail::shows_episodes(&series, &episodes));
        assert_eq!(
            detail::metadata_line(&series, &[1], &episodes),
            "2022 \u{b7} 1 season \u{b7} 1 episode \u{b7} Crime"
        );
        assert_eq!(
            detail::fact_lines(""),
            vec!["No track facts reported yet.".to_string()]
        );
    }

    #[test]
    fn a_long_synopsis_wraps_inside_its_column() {
        let stored = "A New York mobster is exiled to Tulsa, Oklahoma, where he has to build a new \
                      criminal empire from the ground up with a group of unlikely allies, and the \
                      people he meets along the way are not always glad to see him arrive.";

        let lines = detail::wrap(stored, detail::SYNOPSIS_COLUMNS);
        assert!(
            lines.len() > 1,
            "a long synopsis must occupy more than one line"
        );
        assert!(
            lines
                .iter()
                .all(|line| line.chars().count() <= detail::SYNOPSIS_COLUMNS),
            "no line may exceed the column"
        );
        assert_eq!(lines.join(" "), stored);

        let unbroken = "x".repeat(detail::SYNOPSIS_COLUMNS * 3);
        let pieces = detail::wrap(&unbroken, detail::SYNOPSIS_COLUMNS);
        assert!(pieces.len() >= 3);
        assert!(
            pieces
                .iter()
                .all(|line| line.chars().count() <= detail::SYNOPSIS_COLUMNS)
        );
        assert_eq!(pieces.concat(), unbroken);
    }
}
