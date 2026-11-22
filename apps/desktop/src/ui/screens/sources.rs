use crate::app::Message;
use crate::design;
use crate::ui::icons;
use crate::ui::theme::{self, Breakpoint, Gap};
use iced::widget::{button, column, container, row, scrollable, text, text_input};
use iced::{Alignment, Element, Length, Padding};
use screenroom_core::SourceState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderIssue {
    Available,

    Missing,

    PermissionLost,

    Unsafe,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderEntry {
    pub relative_path: String,

    pub name: String,

    pub media: usize,

    pub subfolders: usize,

    pub issue: FolderIssue,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceCard {
    pub id: String,

    pub label: String,

    pub root_path: String,

    pub state: SourceState,

    pub folders: Vec<String>,

    pub folders_configured: bool,

    pub indexed: usize,

    pub total: usize,

    pub scanning: bool,

    pub last_scan: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Inclusion {
    Excluded,

    Selected,

    Included,
}

pub fn inclusion(selected: &[String], relative: &str) -> Inclusion {
    if selected.iter().any(|entry| entry == relative) {
        return Inclusion::Selected;
    }
    if selected
        .iter()
        .any(|entry| entry.is_empty() || relative.starts_with(&format!("{entry}/")))
    {
        return Inclusion::Included;
    }
    Inclusion::Excluded
}

pub fn normalize(selected: &[String]) -> Vec<String> {
    let mut kept: Vec<String> = Vec::new();
    for entry in selected {
        let trimmed = entry.trim_matches('/').to_string();
        if kept.iter().any(|done| {
            done.is_empty() || trimmed == *done || trimmed.starts_with(&format!("{done}/"))
        }) {
            continue;
        }
        kept.retain(|done| !done.starts_with(&format!("{trimmed}/")));
        kept.push(trimmed);
    }
    kept.sort();
    kept.dedup();
    kept
}

pub fn parent_of(relative: &str) -> String {
    match relative.rsplit_once('/') {
        Some((parent, _)) => parent.to_string(),
        None => String::new(),
    }
}

pub fn folder_label(relative: &str) -> String {
    if relative.is_empty() {
        return "Everything in this folder".to_string();
    }
    relative.rsplit('/').next().unwrap_or(relative).to_string()
}

pub fn breadcrumb_for(relative: &str) -> Vec<(String, String)> {
    let mut trail = vec![("This location".to_string(), String::new())];
    if relative.is_empty() {
        return trail;
    }
    let mut path = String::new();
    for part in relative.split('/') {
        if part.is_empty() {
            continue;
        }
        if !path.is_empty() {
            path.push('/');
        }
        path.push_str(part);
        trail.push((part.to_string(), path.clone()));
    }
    trail
}

pub struct Welcome<'a> {
    pub width: f32,

    pub path: &'a str,

    pub busy: bool,

    pub error: Option<&'a str>,
}

pub fn welcome<'a>(view: Welcome<'a>) -> Element<'a, Message> {
    let tokens = design::tokens();
    let body = column![
        text("SCREENROOM")
            .size(tokens.type_scale.caption)
            .font(theme::semibold())
            .color(theme::muted()),
        text("Your media, at home here.")
            .size(tokens.type_scale.title)
            .font(theme::semibold())
            .color(theme::ink()),
        text(
            "Add a folder, an external drive or any connected storage. Screenroom reads \
             your files in place and only looks where you tell it to."
        )
        .size(tokens.type_scale.body_lg)
        .color(theme::muted())
        .width(Length::Fixed(620.0)),
        row![
            button(text("Add a media location").size(tokens.type_scale.label))
                .on_press(Message::ChooseFolder)
                .style(design::action_style(false))
                .padding([tokens.spacing.field_y, tokens.spacing.field_x]),
            text_input("…or type an absolute path", view.path)
                .on_input(Message::NewRootChanged)
                .on_submit(Message::AddFolder)
                .style(design::input_style())
                .padding(tokens.spacing.field_y)
                .width(Length::Fixed(340.0)),
        ]
        .spacing(tokens.spacing.field_x)
        .align_y(Alignment::Center),
    ]
    .spacing(theme::gap(Gap::Rail))
    .width(Length::Fill);

    centred(body.into(), view.width)
}

pub struct EmptyLibrary {
    pub cards: Vec<SourceCard>,

    pub width: f32,

    pub scanning: bool,
}

pub fn empty_library(view: EmptyLibrary) -> Element<'static, Message> {
    let tokens = design::tokens();
    let mut list = column![].spacing(tokens.spacing.field_y);
    for card in &view.cards {
        list = list.push(card_row(card));
    }
    let body = column![
        text("No media found yet")
            .size(tokens.type_scale.title_sm)
            .font(theme::semibold())
            .color(theme::ink()),
        text(
            "Your locations are ready, but the folders you chose hold no playable file \
             yet. Review the folders, add another location, or scan again."
        )
        .size(tokens.type_scale.body_lg)
        .color(theme::muted())
        .width(Length::Fixed(680.0)),
        list,
        row![
            button(text("Add another location").size(tokens.type_scale.label))
                .on_press(Message::ChooseFolder)
                .style(design::action_style(false))
                .padding([tokens.spacing.field_y, tokens.spacing.field_x]),
            button(text("Scan all locations").size(tokens.type_scale.label))
                .on_press(Message::ScanAllSources)
                .style(design::quiet_style(false))
                .padding([tokens.spacing.field_y, tokens.spacing.field_x]),
        ]
        .spacing(tokens.spacing.field_x),
    ]
    .spacing(theme::gap(Gap::Rail))
    .width(Length::Fill);

    centred(body.into(), view.width)
}

pub struct Manager {
    pub cards: Vec<SourceCard>,

    pub selected: Option<String>,

    pub browser: Option<FolderBrowser>,

    pub width: f32,
}

pub struct FolderBrowser {
    pub source_id: String,

    pub source_label: String,

    pub source_root: String,

    pub state: SourceState,

    pub current: String,

    pub breadcrumb: Vec<(String, String)>,

    pub entries: Vec<FolderEntry>,

    pub media_here: usize,

    pub selected: Vec<String>,

    pub folders_configured: bool,

    pub relink_path: String,

    pub unreadable: Option<String>,
}

pub fn manager(view: Manager) -> Element<'static, Message> {
    let tokens = design::tokens();
    let mut rail = column![
        text("Media sources")
            .size(tokens.type_scale.title_sm)
            .font(theme::semibold())
            .color(theme::ink()),
    ]
    .spacing(tokens.spacing.field_y)
    .width(Length::Fixed(280.0));

    for card in &view.cards {
        let active = view.selected.as_deref() == Some(card.id.as_str());
        rail = rail.push(
            button(
                column![
                    text(card.label.clone())
                        .size(tokens.type_scale.label)
                        .color(theme::ink()),
                    text(card.state_label())
                        .size(tokens.type_scale.caption)
                        .color(theme::muted()),
                ]
                .spacing(2.0),
            )
            .on_press(Message::SourceChosen(card.id.clone()))
            .style(move |theme, status| {
                if active {
                    design::action_style(false)(theme, status)
                } else {
                    design::quiet_style(false)(theme, status)
                }
            })
            .width(Length::Fill)
            .padding(tokens.spacing.field_y),
        );
    }

    rail = rail.push(
        button(text("Add a location").size(tokens.type_scale.label))
            .on_press(Message::ChooseFolder)
            .style(design::quiet_style(false))
            .padding(tokens.spacing.field_y),
    );

    let panel: Element<'static, Message> = match view.browser {
        Some(browser) => folder_panel(browser),
        None => text("Choose a source to manage its folders.")
            .size(tokens.type_scale.body)
            .color(theme::muted())
            .into(),
    };

    let layout = row![rail, panel]
        .spacing(theme::gap(Gap::Rail))
        .width(Length::Fill)
        .height(Length::Fill);

    container(scrollable(layout).style(theme::scrollbar))
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(Padding {
            top: theme::gap(Gap::Section),
            bottom: theme::gap(Gap::Section),
            left: 0.0,
            right: 0.0,
        })
        .into()
}

fn folder_panel(browser: FolderBrowser) -> Element<'static, Message> {
    let tokens = design::tokens();
    let mut crumbs = row![].spacing(tokens.spacing.field_x);
    for (label, path) in &browser.breadcrumb {
        crumbs = crumbs.push(
            button(text(label.clone()).size(tokens.type_scale.caption))
                .on_press(Message::FolderBrowseInto(path.clone()))
                .style(design::quiet_style(false))
                .padding(4.0),
        );
    }

    let mut list = column![].spacing(tokens.spacing.field_y);
    let current = inclusion(&browser.selected, &browser.current);
    list = list.push(
        button(
            row![
                icons::icon(icons::Icon::Check, 14.0, theme::ink()),
                text("Everything in this folder")
                    .size(tokens.type_scale.label)
                    .color(theme::ink()),
                text(format!("{} here", browser.media_here))
                    .size(tokens.type_scale.caption)
                    .color(theme::muted()),
            ]
            .spacing(tokens.spacing.field_x)
            .align_y(Alignment::Center),
        )
        .on_press(Message::FolderToggle(browser.current.clone()))
        .style(move |theme, status| {
            if current == Inclusion::Excluded {
                design::quiet_style(false)(theme, status)
            } else {
                design::action_style(false)(theme, status)
            }
        })
        .width(Length::Fill)
        .padding(tokens.spacing.field_y),
    );

    for entry in &browser.entries {
        let state = inclusion(&browser.selected, &entry.relative_path);
        list = list.push(
            row![
                button(text(entry.name.clone()).size(tokens.type_scale.label))
                    .on_press(Message::FolderToggle(entry.relative_path.clone()))
                    .style(move |theme, status| {
                        if state == Inclusion::Excluded {
                            design::quiet_style(false)(theme, status)
                        } else {
                            design::action_style(false)(theme, status)
                        }
                    })
                    .padding(tokens.spacing.field_y)
                    .width(Length::Fill),
                text(format!("{} media", entry.media))
                    .size(tokens.type_scale.caption)
                    .color(theme::muted()),
                button(text("Open").size(tokens.type_scale.caption))
                    .on_press(Message::FolderBrowseInto(entry.relative_path.clone()))
                    .style(design::quiet_style(false))
                    .padding(6.0),
            ]
            .spacing(tokens.spacing.field_x)
            .align_y(Alignment::Center),
        );
    }

    let body = column![
        text(browser.source_label)
            .size(tokens.type_scale.title_sm)
            .font(theme::semibold())
            .color(theme::ink()),
        text(browser.source_root)
            .size(tokens.type_scale.caption)
            .color(theme::muted()),
        crumbs,
        list,
        row![
            button(text("Scan selected folders").size(tokens.type_scale.label))
                .on_press(Message::SaveSourceFolders)
                .style(design::action_style(false))
                .padding([tokens.spacing.field_y, tokens.spacing.field_x]),
            button(text("Clear selection").size(tokens.type_scale.label))
                .on_press(Message::FolderClearSelection)
                .style(design::quiet_style(false))
                .padding([tokens.spacing.field_y, tokens.spacing.field_x]),
            button(text("Rescan").size(tokens.type_scale.label))
                .on_press(Message::RescanSource(browser.source_id.clone()))
                .style(design::quiet_style(false))
                .padding([tokens.spacing.field_y, tokens.spacing.field_x]),
        ]
        .spacing(tokens.spacing.field_x),
    ]
    .spacing(tokens.spacing.field_y)
    .width(Length::Fill);

    body.into()
}

fn card_row(card: &SourceCard) -> Element<'static, Message> {
    let tokens = design::tokens();
    container(
        row![
            column![
                text(card.label.clone())
                    .size(tokens.type_scale.label)
                    .color(theme::ink()),
                text(card.root_path.clone())
                    .size(tokens.type_scale.caption)
                    .color(theme::muted()),
            ]
            .spacing(2.0)
            .width(Length::Fill),
            text(card.state_label())
                .size(tokens.type_scale.caption)
                .color(theme::muted()),
            button(text("Review folders").size(tokens.type_scale.caption))
                .on_press(Message::SourceChosen(card.id.clone()))
                .style(design::quiet_style(false))
                .padding(6.0),
        ]
        .spacing(tokens.spacing.field_x)
        .align_y(Alignment::Center),
    )
    .style(|_theme| design::container_style(design::Level::Panel, false))
    .padding(tokens.spacing.content_pad)
    .width(Length::Fill)
    .into()
}

impl SourceCard {
    pub fn state_label(&self) -> &'static str {
        match self.state {
            SourceState::Available => {
                if self.indexed > 0 {
                    "Available"
                } else {
                    "No media indexed"
                }
            }
            SourceState::Disconnected => "Disconnected",
            SourceState::PermissionLost => "Access lost",
            SourceState::Missing => "Folder moved or missing",
            SourceState::Removed => "Removed",
        }
    }
}

fn centred<'a>(body: Element<'a, Message>, width: f32) -> Element<'a, Message> {
    let breakpoint = Breakpoint::at(width);
    container(
        container(body)
            .width(Length::Fixed(match breakpoint {
                Breakpoint::Narrow => 560.0,
                _ => 720.0,
            }))
            .padding(theme::gap(Gap::Section)),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .center_x(Length::Fill)
    .center_y(Length::Fill)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_parent_selection_includes_its_descendants() {
        let selected = vec!["Films".to_string()];
        assert_eq!(inclusion(&selected, "Films"), Inclusion::Selected);
        assert_eq!(inclusion(&selected, "Films/2024"), Inclusion::Included);
        assert_eq!(inclusion(&selected, "Series"), Inclusion::Excluded);
        assert_eq!(
            inclusion(&vec![String::new()], "Anything"),
            Inclusion::Included
        );
    }

    #[test]
    fn a_root_selection_absorbs_every_other_selection() {
        assert_eq!(
            normalize(&["Films".to_string(), String::new(), "Series".to_string()]),
            vec![String::new()]
        );
    }

    #[test]
    fn sibling_selections_survive_normalisation() {
        assert_eq!(
            normalize(&[
                "Films".to_string(),
                "Series".to_string(),
                "Films".to_string()
            ]),
            vec!["Films".to_string(), "Series".to_string()]
        );
    }

    #[test]
    fn a_breadcrumb_always_starts_at_the_location() {
        assert_eq!(
            breadcrumb_for(""),
            vec![("This location".to_string(), String::new())]
        );
        assert_eq!(
            breadcrumb_for("Films/2024"),
            vec![
                ("This location".to_string(), String::new()),
                ("Films".to_string(), "Films".to_string()),
                ("2024".to_string(), "Films/2024".to_string()),
            ]
        );
    }
}
