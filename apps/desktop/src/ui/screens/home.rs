use crate::app::Message;
use crate::design;
use crate::ui::components::poster::{self, Poster};
use crate::ui::components::rail;
use crate::ui::icons::{self, Icon};
use crate::ui::route::{FocusPos, Nav};
use crate::ui::theme::{self, Breakpoint, Gap, Radius};
use iced::widget::canvas::gradient::Linear;
use iced::widget::canvas::{self, Canvas, Program};
use iced::widget::{Space, button, column, container, image, row, scrollable, stack, text};
use iced::{
    Alignment, Background, Border, ContentFit, Element, Length, Padding, Point, Rectangle,
    Renderer, Shadow, Theme, mouse,
};
use screenroom_core::{LibraryItem, MediaKind};
use std::collections::{HashMap, HashSet};

const CONTINUE: &str = "Continue watching";

const RECENT: &str = "Recently added";

const FILMS: &str = "Films";

const SERIES: &str = "Series";

const RAIL_WINDOW: usize = 24;

const PROGRESS_HEIGHT: f32 = 2.0;

const INVITATION_WIDTH: f32 = 560.0;

const FALLBACK_WIDTH: f32 = 1440.0;

pub struct Home<'a> {
    pub continue_watching: &'a [LibraryItem],

    pub suggested: Vec<&'a LibraryItem>,

    pub resume: Option<&'a LibraryItem>,

    pub nav: &'a Nav,

    pub empty: bool,

    pub art: &'a HashMap<String, image::Handle>,

    pub waiting: &'a HashSet<String>,

    pub width: f32,
}

pub fn rails(items: &[LibraryItem]) -> Vec<(&'static str, Vec<&LibraryItem>)> {
    shelves(&items.iter().collect::<Vec<_>>())
}

pub fn hero_item<'a>(
    resume: Option<&'a LibraryItem>,
    continue_watching: &'a [LibraryItem],
    suggested: &[&'a LibraryItem],
) -> Option<&'a LibraryItem> {
    if let Some(item) = resume.filter(|item| item.available) {
        return Some(item);
    }

    if let Some(item) = continue_watching
        .iter()
        .chain(suggested.iter().copied())
        .find(|item| item.available)
    {
        return Some(item);
    }

    resume
        .or_else(|| continue_watching.first())
        .or_else(|| suggested.first().copied())
}

pub fn rails_for<'a>(
    continue_watching: &'a [LibraryItem],
    suggested: &[&'a LibraryItem],
) -> Vec<(&'static str, Vec<&'a LibraryItem>)> {
    let items: Vec<&'a LibraryItem> = continue_watching
        .iter()
        .chain(suggested.iter().copied())
        .collect();
    shelves(&items)
}

impl<'a> Home<'a> {
    pub fn view(self) -> Element<'a, Message> {
        self.page()
    }

    fn page(&self) -> Element<'a, Message> {
        if self.is_empty() {
            return invitation();
        }

        let width = span(self.width);
        let breakpoint = Breakpoint::at(width);

        let mut contents: Vec<Element<'a, Message>> = Vec::new();

        if let Some(item) = self.hero_item() {
            let reason = if in_progress(item) {
                "CONTINUE WATCHING"
            } else {
                "RECENTLY ADDED"
            };
            let hero: Element<'a, Message> = self.hero(item, breakpoint, reason);
            contents.push(hero);
            contents.push(
                Space::new()
                    .height(Length::Fixed(theme::gap(Gap::Card)))
                    .into(),
            );
        }

        let sections = rails_for(self.continue_watching, &self.suggested);
        let focus = self.nav.focus();
        let last = sections.len().saturating_sub(1);

        let mut rails: Vec<Element<'a, Message>> = Vec::new();
        for (index, (label, entries)) in sections.iter().enumerate() {
            let shelf: Element<'a, Message> =
                self.shelf(index, label, entries, focus, breakpoint.card());
            rails.push(gutter(shelf));
            if index < last {
                rails.push(
                    Space::new()
                        .height(Length::Fixed(theme::gap(Gap::Section)))
                        .into(),
                );
            }
        }
        rails.push(
            Space::new()
                .height(Length::Fixed(theme::gap(Gap::Rail)))
                .into(),
        );
        contents.push(capped(
            iced::widget::Column::with_children(rails)
                .width(Length::Fill)
                .into(),
            breakpoint,
        ));

        let page: Element<'a, Message> = scrollable(
            iced::widget::Column::with_children(contents)
                .width(Length::Fill)
                .spacing(0.0),
        )
        .height(Length::Fill)
        .style(theme::scrollbar)
        .into();
        page
    }

    fn is_empty(&self) -> bool {
        (self.empty || self.hero_item().is_none())
            && self.continue_watching.is_empty()
            && self.suggested.is_empty()
    }

    fn hero_item(&self) -> Option<&'a LibraryItem> {
        hero_item(self.resume, self.continue_watching, &self.suggested)
    }

    fn hero(
        &self,
        item: &'a LibraryItem,
        breakpoint: Breakpoint,
        reason: &'static str,
    ) -> Element<'a, Message> {
        let tokens = design::tokens();
        let height = breakpoint.hero();
        let content_width = span(self.width) - 2.0 * theme::PAD;
        let column_width = (content_width * 0.36).clamp(260.0, 560.0);

        let heading =
            text(poster::truncate(item.title.as_str(), title_chars(column_width)).to_string())
                .size(tokens.type_scale.title_lg)
                .font(theme::semibold())
                .color(theme::ink())
                .width(Length::Fixed(column_width));

        let mut blocks = column![
            text(reason.to_string())
                .size(tokens.type_scale.micro)
                .font(theme::semibold())
                .color(theme::muted()),
            heading,
            text(metadata(item))
                .size(tokens.type_scale.caption)
                .font(theme::regular())
                .color(theme::muted()),
        ]
        .spacing(theme::gap(Gap::Rail))
        .width(Length::Fill);

        if let Some(line) = left_label(item) {
            blocks = blocks.push(hero_progress(item, &line, breakpoint.card()));
        }

        blocks = blocks.push(self.hero_actions(item));

        let inset = theme::content_inset(self.width);

        let content = container(blocks)
            .width(Length::Fill)
            .height(Length::Fixed(height))
            .align_y(Alignment::End)
            .padding(Padding {
                top: 0.0,
                bottom: theme::gap(Gap::Section),
                left: theme::PAD + inset,
                right: theme::PAD + inset,
            });

        let backdrop: Element<'a, Message> = match self.art.get(&item.work_id) {
            Some(handle) => stack![
                image(handle.clone())
                    .width(Length::Fill)
                    .height(Length::Fixed(height))
                    .content_fit(ContentFit::Cover),
                Canvas::new(Scrim)
                    .width(Length::Fill)
                    .height(Length::Fixed(height)),
            ]
            .into(),
            None => Canvas::new(Backdrop)
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

    fn hero_actions(&self, item: &'a LibraryItem) -> Element<'a, Message> {
        let label = if in_progress(item) { "Resume" } else { "Play" };

        row![
            primary_button(label, Icon::Play, Message::Play(item.id.clone())),
            secondary_button("Details", Message::OpenDetail(item.id.clone())),
        ]
        .spacing(theme::gap(Gap::Group))
        .align_y(Alignment::Center)
        .into()
    }

    fn shelf(
        &self,
        index: usize,
        label: &'static str,
        entries: &[&'a LibraryItem],
        focus: FocusPos,
        card_width: f32,
    ) -> Element<'a, Message> {
        let focused_here = focus.rail == index;
        let start = if focused_here {
            focus
                .column
                .saturating_sub(RAIL_WINDOW / 3)
                .min(entries.len().saturating_sub(RAIL_WINDOW))
        } else {
            0
        };

        let mut cards: Vec<(Element<'a, Message>, usize)> = Vec::new();

        if start > 0 {
            cards.push((
                Space::new()
                    .width(Length::Fixed(rail::focus_offset(start, card_width)))
                    .into(),
                start,
            ));
        }

        for (position, item) in entries.iter().enumerate().skip(start).take(RAIL_WINDOW) {
            let focused = focused_here && focus.column == position;
            let mut card = Poster::new(item.title.as_str(), caption(item))
                .art(self.art.get(&item.work_id).cloned())
                .waiting(self.waiting.contains(&item.work_id))
                .width(card_width)
                .focused(focused)
                .on_press(Message::OpenDetail(item.id.clone()));

            if let Some(progress) = progress_fraction(item) {
                card = card.progress(Some(progress));
            }

            cards.push((card.into_element(), position));
        }

        let strip: Element<'a, Message> = rail::rail(
            label,
            entries.len(),
            cards,
            if focused_here { focus.column } else { 0 },
            card_width,
        );
        strip
    }
}

fn shelves<'a>(items: &[&'a LibraryItem]) -> Vec<(&'static str, Vec<&'a LibraryItem>)> {
    let mut seen = std::collections::HashSet::new();
    let mut everything: Vec<&'a LibraryItem> = Vec::new();

    for item in items {
        if seen.insert(item.id.as_str()) {
            everything.push(item);
        }
    }

    let mut result: Vec<(&'static str, Vec<&'a LibraryItem>)> = Vec::new();

    let watching: Vec<&'a LibraryItem> = everything
        .iter()
        .copied()
        .filter(|item| in_progress(item))
        .collect();
    push_shelf(&mut result, CONTINUE, watching);

    let mut recent = everything.clone();
    recent.sort_by(|a, b| b.added_ms.cmp(&a.added_ms).then_with(|| a.id.cmp(&b.id)));
    push_shelf(&mut result, RECENT, recent);

    for (kind, label) in [(MediaKind::Movie, FILMS), (MediaKind::Series, SERIES)] {
        let cards: Vec<&'a LibraryItem> = everything
            .iter()
            .copied()
            .filter(|item| item.kind == kind)
            .collect();
        push_shelf(&mut result, label, cards);
    }

    result
}

fn push_shelf<'a>(
    shelves: &mut Vec<(&'static str, Vec<&'a LibraryItem>)>,
    label: &'static str,
    cards: Vec<&'a LibraryItem>,
) {
    if !cards.is_empty() {
        shelves.push((label, cards));
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

fn invitation() -> Element<'static, Message> {
    let tokens = design::tokens();

    let copy = column![
        text("Bring your own films")
            .size(tokens.type_scale.subtitle)
            .font(theme::semibold())
            .color(theme::ink())
            .width(Length::Fill)
            .align_x(Alignment::Center),
        text(
            "Screenroom plays the files you already own, where they already live. \
             Choose a folder of films or series and it reads them in place: nothing is \
             moved, renamed or uploaded, and nothing leaves this machine."
        )
        .size(tokens.type_scale.body)
        .font(theme::regular())
        .color(theme::muted())
        .width(Length::Fill)
        .align_x(Alignment::Center),
        row![
            primary_button("Choose a folder…", Icon::Plus, Message::ChooseFolder),
            secondary_button("Refresh", Message::Refresh),
        ]
        .spacing(theme::gap(Gap::Group))
        .align_y(Alignment::Center),
    ]
    .spacing(theme::gap(Gap::Rail))
    .align_x(Alignment::Center)
    .width(Length::Fixed(INVITATION_WIDTH));

    container(copy)
        .width(Length::Fill)
        .height(Length::Fill)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .padding(Padding {
            top: 0.0,
            bottom: 0.0,
            left: theme::PAD,
            right: theme::PAD,
        })
        .into()
}

fn primary_button(label: &str, icon: Icon, message: Message) -> Element<'static, Message> {
    let tokens = design::tokens();

    button(
        row![
            icons::icon(icon, 14.0, theme::signal_foreground()),
            text(label.to_string())
                .size(tokens.type_scale.label)
                .font(theme::medium())
                .color(theme::signal_foreground()),
        ]
        .spacing(theme::gap(Gap::Card))
        .align_y(Alignment::Center),
    )
    .on_press(message)
    .height(Length::Fixed(tokens.control.lg))
    .padding(Padding {
        top: 0.0,
        bottom: 0.0,
        left: theme::gap(Gap::Rail),
        right: theme::gap(Gap::Rail),
    })
    .style(primary_style)
    .into()
}

fn secondary_button(label: &str, message: Message) -> Element<'static, Message> {
    let tokens = design::tokens();

    button(
        text(label.to_string())
            .size(tokens.type_scale.label)
            .font(theme::medium())
            .color(theme::ink()),
    )
    .on_press(message)
    .height(Length::Fixed(tokens.control.lg))
    .padding(Padding {
        top: 0.0,
        bottom: 0.0,
        left: theme::gap(Gap::Rail),
        right: theme::gap(Gap::Rail),
    })
    .style(secondary_style)
    .into()
}

fn hero_progress(item: &LibraryItem, line: &str, width: f32) -> Element<'static, Message> {
    let tokens = design::tokens();
    let fraction = progress_fraction(item).unwrap_or(0.0);
    let filled = (width * fraction).max(0.0);
    let rest = (width - filled).max(0.0);

    let bar = row![
        container(
            Space::new()
                .width(Length::Fixed(filled))
                .height(Length::Fixed(PROGRESS_HEIGHT))
        )
        .width(Length::Fixed(filled))
        .height(Length::Fixed(PROGRESS_HEIGHT))
        .style(|_theme| container::Style {
            background: Some(Background::Color(theme::signal())),
            ..container::Style::default()
        }),
        container(
            Space::new()
                .width(Length::Fixed(rest))
                .height(Length::Fixed(PROGRESS_HEIGHT))
        )
        .width(Length::Fixed(rest))
        .height(Length::Fixed(PROGRESS_HEIGHT))
        .style(|_theme| container::Style {
            background: Some(Background::Color(theme::surface(3))),
            ..container::Style::default()
        }),
    ]
    .spacing(0.0);

    row![
        bar,
        text(line.to_string())
            .size(tokens.type_scale.caption)
            .font(theme::regular())
            .color(theme::muted()),
    ]
    .spacing(theme::gap(Gap::Group))
    .align_y(Alignment::Center)
    .into()
}

fn primary_style(_theme: &Theme, status: button::Status) -> button::Style {
    let tinted = matches!(status, button::Status::Hovered | button::Status::Pressed);

    button::Style {
        background: Some(Background::Color(if tinted {
            fade(theme::signal(), 0.88)
        } else {
            theme::signal()
        })),
        text_color: theme::signal_foreground(),
        border: Border {
            color: theme::signal(),
            width: 0.0,
            radius: theme::radius(Radius::Control).into(),
        },
        shadow: Shadow::default(),
        snap: false,
    }
}

fn secondary_style(_theme: &Theme, status: button::Status) -> button::Style {
    let tinted = matches!(status, button::Status::Hovered | button::Status::Pressed);

    button::Style {
        background: Some(Background::Color(if tinted {
            theme::surface(3)
        } else {
            theme::surface(2)
        })),
        text_color: theme::ink(),
        border: Border {
            color: theme::hair(),
            width: 0.0,
            radius: theme::radius(Radius::Control).into(),
        },
        shadow: Shadow::default(),
        snap: false,
    }
}

fn fade(color: iced::Color, alpha: f32) -> iced::Color {
    iced::Color {
        a: color.a * alpha.clamp(0.0, 1.0),
        ..color
    }
}

fn span(width: f32) -> f32 {
    if width.is_finite() && width > 0.0 {
        width
    } else {
        FALLBACK_WIDTH
    }
}

fn title_chars(column_width: f32) -> usize {
    let per_line = (column_width / 24.0).max(10.0) as usize;

    (per_line * 2).clamp(20, 64)
}

pub fn in_progress(item: &LibraryItem) -> bool {
    item.position_ms > 0
        && item.duration_ms > 0
        && item.position_ms < item.duration_ms
        && !item.watched
}

fn progress_fraction(item: &LibraryItem) -> Option<f32> {
    if !in_progress(item) {
        return None;
    }

    Some((item.position_ms as f32 / item.duration_ms as f32).clamp(0.0, 1.0))
}

fn kind_label(kind: MediaKind) -> Option<&'static str> {
    match kind {
        MediaKind::Movie => Some("Film"),
        MediaKind::Series => Some("Series"),
        MediaKind::Unknown => None,
    }
}

fn runtime_label(duration_ms: i64) -> Option<String> {
    if duration_ms <= 0 {
        return None;
    }

    let minutes = (duration_ms + 59_999) / 60_000;
    let hours = minutes / 60;
    let rest = minutes % 60;

    Some(match (hours, rest) {
        (0, minutes) => format!("{minutes} min"),
        (hours, 0) => format!("{hours}h"),
        (hours, minutes) => format!("{hours}h {minutes}m"),
    })
}

pub fn left_label(item: &LibraryItem) -> Option<String> {
    if !in_progress(item) {
        return None;
    }

    let minutes = ((item.duration_ms - item.position_ms).max(0) + 59_999) / 60_000;
    let hours = minutes / 60;
    let rest = minutes % 60;

    Some(match (hours, rest) {
        (0, minutes) => format!("{minutes} min left"),
        (hours, 0) => format!("{hours}h left"),
        (hours, minutes) => format!("{hours}h {minutes}m left"),
    })
}

pub fn metadata(item: &LibraryItem) -> String {
    let mut facts: Vec<String> = Vec::new();

    if let Some(year) = item.year {
        facts.push(year.to_string());
    }

    let episode = item.episode_label();
    if !episode.is_empty() {
        facts.push(episode);
    } else if let Some(kind) = kind_label(item.kind) {
        facts.push(kind.to_string());
    }

    if let Some(runtime) = runtime_label(item.duration_ms) {
        facts.push(runtime);
    }

    facts.join(" · ")
}

fn caption(item: &LibraryItem) -> String {
    let rest = match left_label(item) {
        Some(left) => left,
        None if item.watched => "Watched".to_string(),
        None => match (item.year, kind_label(item.kind)) {
            (Some(year), _) => year.to_string(),
            (None, Some(kind)) => kind.to_string(),
            (None, None) => "In library".to_string(),
        },
    };

    let episode = item.episode_label();
    if episode.is_empty() {
        rest
    } else {
        format!("{episode} · {rest}")
    }
}

struct Scrim;

impl Program<Message> for Scrim {
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
        let scrim = |alpha: f32| iced::Color {
            a: alpha,
            ..theme::ground()
        };

        frame.fill_rectangle(
            Point::ORIGIN,
            bounds.size(),
            canvas::Fill::from(canvas::Gradient::from(
                Linear::new(Point::ORIGIN, Point::new(0.0, bounds.height))
                    .add_stop(0.0, scrim(0.15))
                    .add_stop(0.45, scrim(0.55))
                    .add_stop(0.78, scrim(0.88))
                    .add_stop(1.0, scrim(1.0)),
            )),
        );

        vec![frame.into_geometry()]
    }
}

struct Backdrop;

impl Program<Message> for Backdrop {
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

        frame.fill_rectangle(
            Point::ORIGIN,
            bounds.size(),
            canvas::Fill::from(canvas::Gradient::from(
                Linear::new(Point::ORIGIN, Point::new(0.0, bounds.height))
                    .add_stop(0.0, theme::surface(2))
                    .add_stop(0.55, theme::surface(1))
                    .add_stop(1.0, theme::ground()),
            )),
        );

        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use crate::app::Screen;
    use crate::ui::route::Nav;
    use crate::ui::screens::home::{self, Home};
    use screenroom_core::{LibraryItem, MediaKind};
    use std::collections::HashMap;
    use std::collections::HashSet;

    fn item(
        id: &str,
        title: &str,
        kind: MediaKind,
        position_ms: i64,
        duration_ms: i64,
        added_ms: i64,
    ) -> LibraryItem {
        LibraryItem {
            id: id.to_string(),
            work_id: format!("work-{id}"),
            edition_id: format!("edition-{id}"),
            source_id: "source".to_string(),
            title: title.to_string(),
            filename: format!("{id}.mkv"),
            relative_path: format!("{id}.mkv"),
            kind,
            overview: String::new(),
            episode_title: String::new(),
            episode_overview: String::new(),
            genres: Vec::new(),
            year: Some(2021),
            season: None,
            episode: None,
            episode_end: None,
            duration_ms,
            position_ms,
            added_ms,
            available: true,
            watched: false,
            favorite: false,
            watchlisted: false,
            manual_match: false,
            metadata_origin: "filename".to_string(),
        }
    }

    fn fixtures() -> Vec<LibraryItem> {
        vec![
            item(
                "northern-lights",
                "Northern Lights",
                MediaKind::Movie,
                4_080_000,
                6_720_000,
                90,
            ),
            item(
                "green-book",
                "Green Book",
                MediaKind::Movie,
                0,
                7_800_000,
                80,
            ),
            item(
                "sherlock",
                "Sherlock",
                MediaKind::Series,
                1_200_000,
                2_400_000,
                70,
            ),
            item(
                "the-quiet-ocean",
                "The Quiet Ocean",
                MediaKind::Movie,
                0,
                6_000_000,
                60,
            ),
        ]
    }

    #[test]
    fn an_empty_library_offers_one_invitation_and_no_rails() {
        let art = HashMap::new();
        let home = Home {
            continue_watching: &[],
            suggested: Vec::new(),
            resume: None,
            nav: &Nav::new(Screen::Home),
            empty: true,
            art: &art,
            waiting: &HashSet::new(),
            width: 1440.0,
        };
        let _ = home.view();
        assert!(home::rails(&[]).is_empty());
    }

    #[test]
    fn rails_are_built_from_what_the_library_holds() {
        let items = fixtures();
        let rails = home::rails(&items);
        assert_eq!(rails[0].0, "Continue watching");
        assert!(rails.iter().any(|(label, _)| *label == "Films"));
        assert!(rails.iter().all(|(_, cards)| !cards.is_empty()));
    }

    #[test]
    fn a_rail_is_never_rendered_with_zero_cards() {
        let items = fixtures();
        for (label, cards) in home::rails(&items) {
            assert!(!cards.is_empty(), "{label} was offered with no cards");
        }

        let unknown = vec![item("mystery", "Mystery", MediaKind::Unknown, 0, 0, 10)];
        let rails = home::rails(&unknown);
        assert!(rails.iter().all(|(_, cards)| !cards.is_empty()));
        assert!(rails.iter().all(|(label, _)| *label != "Films"));
        assert!(rails.iter().all(|(label, _)| *label != "Series"));
    }

    #[test]
    fn an_empty_shelf_degrades_to_recently_added_instead_of_a_blank_rail() {
        let fresh = vec![item(
            "green-book",
            "Green Book",
            MediaKind::Movie,
            0,
            7_800_000,
            80,
        )];
        let rails = home::rails(&fresh);
        assert_eq!(rails[0].0, "Recently added");
        assert!(rails.iter().all(|(label, _)| *label != "Continue watching"));
    }

    #[test]
    fn home_renders_without_a_resume_point_or_suggestions() {
        let items = fixtures();
        let nav = Nav::new(Screen::Home);
        let art = HashMap::new();
        let home = Home {
            continue_watching: &items,
            suggested: Vec::new(),
            resume: None,
            nav: &nav,
            empty: false,
            art: &art,
            waiting: &HashSet::new(),
            width: 1280.0,
        };
        let _ = home.view();
    }

    #[test]
    fn a_title_appears_once_per_rail() {
        let items = fixtures();
        let rails = home::rails(&items);
        for (label, cards) in &rails {
            let mut ids: Vec<&str> = cards.iter().map(|item| item.id.as_str()).collect();
            ids.sort_unstable();
            ids.dedup();
            assert_eq!(ids.len(), cards.len(), "{label} repeated a title");
        }
    }
}
