# Screenroom frontend rebuild Implementation Plan


**Goal:** Rebuild the macOS client's shell and five screens into a media application — hero, rails, poster-first cards — in the Dropout Design System's black-and-white language, with no behaviour lost.

**Architecture:** A new `apps/desktop/src/ui/` tree owns theme, routing, focus, components and screens. `app.rs` keeps its state machine, messages and all behaviour; its `*_view` functions are deleted once their replacement is live. Routing and focus are built and unit-tested before any screen moves, because every screen and the accessibility snapshot consume them.

**Tech Stack:** Rust edition 2024, iced 0.14 (adding the `canvas` feature for vector icons), wgpu 27, the pinned Dropout DS tokens already in `apps/desktop/src/design/ds_pinned.rs`, Inter (SIL OFL) bundled as an asset.

**Spec:** `docs/specs/2026-11-02-frontend-rebuild-design.md`

## Global Constraints

- Dark theme only. Ground `#171717`, ink `#f7f7f7`, muted `#a3a3a3`, surface1 `#262626`, surface2 `#2b2b2b`, surface3 `#333333`, hair `rgba(255,255,255,0.10)`, signal `#ffffff`, focus surface `#f7f7f7` on `#171717` ink, danger `#ff6467`. Values come from `design::tokens()`, never from a literal in a screen.
- Screen padding is exactly 40 px horizontally. Radii 8 / 12 / 14 / 16. Vertical gaps come from `{4, 9, 13, 22, 26, 34}`.
- **No comments in any code.** `python3 scripts/no_comments.py check` must stay clean.
- No new runtime dependency beyond the iced `canvas` feature. No web view, no CSS, no remote asset, no network on any screen.
- Inter is bundled offline from a SIL OFL file; the font licence text ships beside it.
- Every surface stays reachable by keyboard; the focused control is visible without a ring.
- `SCREENROOM_AX_CHECK` must report a non-empty tree with matching labels on every screen, and exactly one focused control.
- `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --all --check` clean at every commit.

## Review Focus

1. **An empty library.** Home must show one centred invitation, not a hero over nothing and not five empty rails.
2. **A 10,000-item library.** Rails must not materialise every card; paging must still reach every row.
3. **A rail with one item.** Left/Right at either end must clamp, never wrap, never panic, never scroll past content.
4. **A title with no poster.** The card must hold its box with a placeholder; the rail must not reflow or collapse.
5. **A very long title or a filename with emoji and combining marks.** Labels must truncate inside their column without overflowing or panicking.

---

### Task 1: Theme adapter, Inter, and the canvas feature

**Files:**
- Modify: `apps/desktop/Cargo.toml:14`
- Create: `apps/desktop/src/ui/mod.rs`, `apps/desktop/src/ui/theme.rs`
- Create: `apps/desktop/assets/fonts/Inter-Regular.ttf`, `Inter-Medium.ttf`, `Inter-SemiBold.ttf`, `assets/fonts/OFL.txt`
- Modify: `apps/desktop/src/lib.rs` (add `pub mod ui;`)
- Test: `apps/desktop/src/ui/theme.rs` (inline `#[cfg(test)]`)

**Interfaces:**
- Consumes: `crate::design::{tokens, Tokens, Palette}`.
- Produces: `pub mod theme` with `pub const PAD: f32 = 40.0`, `pub fn ground() -> Color`, `pub fn ink() -> Color`, `pub fn muted() -> Color`, `pub fn surface(level: u8) -> Color`, `pub fn hair() -> Color`, `pub fn danger() -> Color`, `pub fn radius(step: Radius) -> f32`, `pub enum Radius { Small, Control, Surface, Overlay }`, `pub fn gap(step: Gap) -> f32`, `pub enum Gap { Tight, Card, Group, Rail, Section }`, `pub fn fonts() -> Vec<Cow<'static, str>>`, `pub const FONT: &str = "Inter"`.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn theme_values_come_from_the_pinned_tokens() {
    let tokens = crate::design::tokens();
    assert_eq!(theme::ground(), tokens.palette.background);
    assert_eq!(theme::ink(), tokens.palette.foreground);
    assert_eq!(theme::surface(1), tokens.palette.surface1);
    assert_eq!(theme::surface(2), tokens.palette.surface2);
    assert_eq!(theme::danger(), tokens.palette.destructive);
}

#[test]
fn the_gap_and_radius_scales_are_the_spec_values() {
    assert_eq!(theme::PAD, 40.0);
    assert_eq!([theme::gap(Gap::Tight), theme::gap(Gap::Card), theme::gap(Gap::Group),
                theme::gap(Gap::Rail), theme::gap(Gap::Section)], [4.0, 9.0, 13.0, 26.0, 34.0]);
    assert_eq!([theme::radius(Radius::Small), theme::radius(Radius::Control),
                theme::radius(Radius::Surface), theme::radius(Radius::Overlay)], [8.0, 12.0, 14.0, 16.0]);
}

#[test]
fn the_bundled_font_is_inter_and_the_licence_ships_with_it() {
    assert_eq!(theme::FONT, "Inter");
    assert!(theme::fonts().iter().any(|f| f.contains("Inter")));
    let ofl = include_str!("../../assets/fonts/OFL.txt");
    assert!(ofl.contains("SIL Open Font License"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p screenroom-desktop --lib ui::theme`
Expected: FAIL — module `ui` does not exist.

- [ ] **Step 3: Implement the module**

`ui/theme.rs` reads every colour from `crate::design::tokens()` and maps the design system's spacing and radius scales onto the enums above. `ui/mod.rs` declares `pub mod theme;`. Add `canvas` to the iced feature list and `pub mod ui;` to `lib.rs`. Download the three Inter weights and the OFL text once, commit them under `assets/fonts/`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p screenroom-desktop --lib ui::theme`
Expected: PASS, 3 tests.

- [ ] **Step 5: Commit**

```bash
git add apps/desktop/Cargo.toml apps/desktop/assets apps/desktop/src/ui apps/desktop/src/lib.rs
git commit -m "feat(ui): theme adapter over the pinned design tokens, with Inter bundled"
```

---

### Task 2: Vector icons

**Files:**
- Create: `apps/desktop/src/ui/icons.rs`
- Modify: `apps/desktop/src/ui/mod.rs`
- Test: `apps/desktop/src/ui/icons.rs`

**Interfaces:**
- Produces: `pub enum Icon { Back, Play, Pause, SkipBack, SkipForward, Volume, Subtitles, Fullscreen, Search, Plus, Check }`, `pub fn icon<'a>(icon: Icon, size: f32, color: Color) -> Element<'a, Message>`, `pub fn path(icon: Icon) -> &'static [Segment]` where `Segment` is the module's own `(Vec<Point>, bool)`-shaped path type.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn every_icon_has_a_drawable_path_inside_its_box() {
    for icon in Icon::ALL {
        let segments = icons::path(*icon);
        assert!(!segments.is_empty(), "{icon:?} has no path");
        for (points, _) in segments {
            assert!(points.len() >= 2, "{icon:?} has a segment with one point");
            for p in points {
                assert!((0.0..=24.0).contains(&p.x) && (0.0..=24.0).contains(&p.y),
                    "{icon:?} escapes its 24x24 box at {p:?}");
            }
        }
    }
}

#[test]
fn the_icon_set_covers_every_control_the_player_draws() {
    assert_eq!(Icon::ALL.len(), 11);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p screenroom-desktop --lib ui::icons`
Expected: FAIL — `Icon` not found.

- [ ] **Step 3: Implement**

Each icon is a list of polyline segments in a 24×24 box, stroked with `theme::ink()` at the requested size. `icon()` returns an `iced::widget::canvas::Canvas` wrapped in a sized container. Paths are authored from the Paper mockup's icon shapes.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p screenroom-desktop --lib ui::icons`
Expected: PASS, 2 tests.

- [ ] **Step 5: Commit**

```bash
git add apps/desktop/src/ui
git commit -m "feat(ui): vector icons drawn as paths instead of type glyphs"
```

---

### Task 3: Route and focus

**Files:**
- Create: `apps/desktop/src/ui/route.rs`
- Modify: `apps/desktop/src/ui/mod.rs`, `apps/desktop/src/app.rs` (replace `focus: usize` with `nav: Nav`, keep `screen` derived)
- Test: `apps/desktop/src/ui/route.rs`

**Interfaces:**
- Consumes: `crate::app::Screen`.
- Produces: `pub struct Nav`, `pub fn new(screen: Screen) -> Nav`, `pub fn screen(&self) -> Screen`, `pub fn push(&mut self, screen: Screen)`, `pub fn pop(&mut self) -> Option<Screen>`, `pub fn focus(&self) -> FocusPos`, `pub fn set_focus(&mut self, pos: FocusPos)`, `pub fn move_focus(&mut self, dir: Focus, rails: &[usize], columns: usize)`, `pub struct FocusPos { pub rail: usize, pub column: usize }`, `pub enum Focus { Left, Right, Up, Down }`.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn focus_clamps_inside_a_rail_instead_of_wrapping() {
    let mut nav = Nav::new(Screen::Home);
    nav.move_focus(Focus::Left, &[3, 3], 6);
    assert_eq!(nav.focus(), FocusPos { rail: 0, column: 0 }, "left at the start must not wrap");

    nav.move_focus(Focus::Right, &[3, 3], 6);
    nav.move_focus(Focus::Right, &[3, 3], 6);
    nav.move_focus(Focus::Right, &[3, 3], 6);
    assert_eq!(nav.focus(), FocusPos { rail: 0, column: 2 }, "right stops at the last card");
}

#[test]
fn moving_down_keeps_the_column_when_the_lower_rail_is_shorter() {
    let mut nav = Nav::new(Screen::Home);
    nav.set_focus(FocusPos { rail: 0, column: 5 });
    nav.move_focus(Focus::Down, &[8, 2], 6);
    assert_eq!(nav.focus(), FocusPos { rail: 1, column: 1 }, "the shorter rail clamps the column");
}

#[test]
fn a_single_item_rail_is_navigable_and_does_not_panic() {
    let mut nav = Nav::new(Screen::Home);
    nav.move_focus(Focus::Right, &[1], 6);
    assert_eq!(nav.focus(), FocusPos { rail: 0, column: 0 });
    nav.move_focus(Focus::Down, &[1], 6);
    assert_eq!(nav.focus(), FocusPos { rail: 0, column: 0 }, "the last rail absorbs Down");
}

#[test]
fn an_empty_library_has_a_focus_that_is_not_a_card() {
    let mut nav = Nav::new(Screen::Home);
    nav.move_focus(Focus::Down, &[], 6);
    assert_eq!(nav.focus(), FocusPos { rail: 0, column: 0 });
}

#[test]
fn going_back_restores_the_focus_the_screen_was_left_with() {
    let mut nav = Nav::new(Screen::Browse);
    nav.set_focus(FocusPos { rail: 0, column: 4 });
    nav.push(Screen::Detail);
    nav.set_focus(FocusPos { rail: 1, column: 2 });
    nav.pop();
    assert_eq!(nav.screen(), Screen::Browse);
    assert_eq!(nav.focus(), FocusPos { rail: 0, column: 4 });
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p screenroom-desktop --lib ui::route`
Expected: FAIL — `Nav` not found.

- [ ] **Step 3: Implement**

`Nav` holds `stack: Vec<(Screen, FocusPos)>`. `push` records the current focus and switches; `pop` restores it. `move_focus` treats `rails` as the card count per rail and clamps. `app.rs` keeps a `nav: Nav` and derives `screen` from it; every existing `self.screen = X` becomes `self.nav.push(X)` or `set`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p screenroom-desktop --lib ui::route`
Expected: PASS, 5 tests.

- [ ] **Step 5: Commit**

```bash
git add apps/desktop/src/ui apps/desktop/src/app.rs
git commit -m "feat(ui): a route and focus model that restores where you left off"
```

---

### Task 4: Poster card and rail

**Files:**
- Create: `apps/desktop/src/ui/components/mod.rs`, `poster.rs`, `rail.rs`
- Modify: `apps/desktop/src/ui/mod.rs`
- Test: `apps/desktop/src/ui/components/poster.rs`, `rail.rs`

**Interfaces:**
- Consumes: `theme`, `icons`, `crate::app::Message`, `crate::video_surface` (unchanged).
- Produces: `pub struct Poster<'a> { … }` with `pub fn new(title: &'a str, caption: &'a str) -> Self`, `.art(Option<Handle>)`, `.progress(Option<f32>)`, `.focused(bool)`, `.on_press(Message)`, `.into_element() -> Element<'a, Message>`; `pub fn rail<'a>(label: &'a str, count: usize, cards: Vec<(Element<'a, Message>, usize)>, focus: usize) -> Element<'a, Message>`; `pub fn card_columns(width: f32) -> usize`; `pub fn truncate(text: &str, max_chars: usize) -> &str`.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn a_card_holds_its_box_when_the_poster_is_missing() {
    let element = Poster::new("No Artwork", "2020").into_element();
    let _ = element;
}

#[test]
fn progress_is_clamped_and_a_finished_film_shows_no_bar() {
    assert_eq!(poster::bar_width(0.0), 0.0);
    assert_eq!(poster::bar_width(0.5), 120.0);
    assert_eq!(poster::bar_width(1.0), 240.0);
    assert_eq!(poster::bar_width(2.5), 240.0);
}

#[test]
fn a_very_long_title_truncates_instead_of_overflowing() {
    let long = "The Extraordinarily Long Title Of A Film That Nobody Would Ever Name This Way";
    let cut = poster::truncate(long, 28);
    assert!(cut.chars().count() <= 28);
    assert!(long.starts_with(cut));
    assert_eq!(poster::truncate("Short", 28), "Short");
}

#[test]
fn a_title_with_emoji_and_combining_marks_truncates_on_a_character_boundary() {
    let text = "ফিল্ম 🎬 café\u{0301} and more words than fit";
    let cut = poster::truncate(text, 8);
    assert_eq!(cut.chars().count(), 8, "no char boundary may be split");
    assert!(text.starts_with(cut));
}

#[test]
fn the_grid_column_count_follows_the_window_width() {
    assert_eq!(poster::card_columns(1440.0), 6);
    assert_eq!(poster::card_columns(1024.0), 4);
    assert_eq!(poster::card_columns(600.0), 2);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p screenroom-desktop --lib ui::components`
Expected: FAIL — `Poster` not found.

- [ ] **Step 3: Implement**

`Poster` is a `column` of a 16:9 art box (240×135 at the mockup size, scaled by the column width the caller passes), an optional 2 px progress bar pinned to the art's lower edge, a 13 px medium title and an 11 px muted caption. Focus is a 2 px white edge plus white title — no ring. Missing art draws `surface(2)` with a hairline. `rail` is a label row plus a horizontal `scrollable` of cards with 16 px gaps, `scroll_to` the focused column.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p screenroom-desktop --lib ui::components`
Expected: PASS, 5 tests.

- [ ] **Step 5: Commit**

```bash
git add apps/desktop/src/ui
git commit -m "feat(ui): poster cards and rails that fill the width"
```

---

### Task 5: Shell

**Files:**
- Create: `apps/desktop/src/ui/shell.rs`
- Modify: `apps/desktop/src/ui/mod.rs`, `apps/desktop/src/app.rs` (`view()` delegates to `ui::shell::frame`)
- Test: `apps/desktop/src/ui/shell.rs`

**Interfaces:**
- Produces: `pub fn frame<'a>(body: Element<'a, Message>, active: Option<Screen>, status: Option<&'a str>, error: Option<&'a str>) -> Element<'a, Message>`, `pub fn nav_bar<'a>(active: Option<Screen>) -> Element<'a, Message>`, `pub fn status_bar<'a>(status: Option<&'a str>, error: Option<&'a str>) -> Option<Element<'a, Message>>`.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn the_status_bar_is_absent_until_there_is_something_to_say() {
    assert!(shell::status_bar(None, None).is_none());
    assert!(shell::status_bar(Some("Backed up 1.5 MB").into(), None).is_some());
    assert!(shell::status_bar(None, Some("backup_failed: disk full").into()).is_some());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p screenroom-desktop --lib ui::shell`
Expected: FAIL — `shell` not found.

- [ ] **Step 3: Implement**

A 76 px bar: wordmark, three nav items, search affordance, profile circle. Transparent when the screen has a hero, solid `theme::ground()` otherwise. `frame` stacks the bar over the body and appends the status bar only when it has content.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p screenroom-desktop --lib ui::shell`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add apps/desktop/src/ui apps/desktop/src/app.rs
git commit -m "feat(ui): a thin shell instead of a header panel"
```

---

### Task 6: Home

**Files:**
- Create: `apps/desktop/src/ui/screens/mod.rs`, `home.rs`
- Modify: `apps/desktop/src/app.rs` (delete `home_view`)
- Test: `apps/desktop/src/app/acceptance.rs` (`step_1`, `step_2`, `step_11` must still pass), `apps/desktop/src/ui/screens/home.rs`

**Interfaces:**
- Consumes: `Poster`, `rail`, `Nav`, `theme`.
- Produces: `pub struct Home<'a> { pub continue_watching: &'a [LibraryItem], pub suggested: &'a [LibraryItem], pub resume: Option<&'a LibraryItem>, pub nav: &'a Nav, pub empty: bool }`, `impl Home<'_> { pub fn view(&self) -> Element<'_, Message> }`, `pub fn rails(items: &[LibraryItem]) -> Vec<(&'static str, Vec<&LibraryItem>)>`.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn an_empty_library_offers_one_invitation_and_no_rails() {
    let home = Home { continue_watching: &[], suggested: &[], resume: None, nav: &Nav::new(Screen::Home), empty: true };
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
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p screenroom-desktop --lib ui::screens::home`
Expected: FAIL — `Home` not found.

- [ ] **Step 3: Implement**

Hero followed by rails as specified in §7.2. With `empty`, one centred invitation replaces both.

- [ ] **Step 4: Run all of it**

Run: `cargo test -p screenroom-desktop`
Expected: PASS, including `step_1`, `step_2`, `step_11`.

- [ ] **Step 5: Commit**

```bash
git add apps/desktop/src/ui apps/desktop/src/app.rs
git commit -m "feat(ui): a home screen with a hero and rails"
```

---

### Task 7: Library

**Files:** Create `apps/desktop/src/ui/screens/library.rs`; modify `app.rs` (delete `browse_view`). Test: `library.rs`, plus `step_18`.

**Interfaces:** Produces `pub struct Library<'a> { pub items: &'a [LibraryItem], pub total: usize, pub page: u32, pub pages: u32, pub query: &'a str, pub filter: Filter, pub sort: Sort, pub source_line: String, pub nav: &'a Nav }` with `view()`, and `pub fn visible_tiles(items: &[LibraryItem], focused: FocusPos) -> Vec<&LibraryItem>`.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn a_ten_thousand_item_library_pages_instead_of_rendering_everything() {
    let items = many(10_000);
    let tiles = library::visible_tiles(&items[..24], FocusPos { rail: 0, column: 0 });
    assert_eq!(tiles.len(), 24, "only the page is built");
}

#[test]
fn the_source_line_states_folders_files_and_recency() {
    assert!(library::source_line(2, 775, Some("2 hours ago")).contains("2 folders"));
    assert!(library::source_line(0, 0, None).contains("no folders"));
}
```

- [ ] **Step 2–5:** fail, implement per §7.3 (title, count, search, sort, chips, source line, 6-across grid), verify `step_18` still passes, commit `feat(ui): a library that reads like a shelf`.

---

### Task 8: Detail

**Files:** Create `apps/desktop/src/ui/screens/detail.rs`; modify `app.rs` (delete `detail_view`). Test: `detail.rs`, plus `step_3`, `step_7`.

**Interfaces:** `pub struct Detail<'a> { pub item: &'a LibraryItem, pub episodes: &'a [LibraryItem], pub seasons: &'a [i64], pub season: i64, pub nav: &'a Nav, pub facts: &'a str }` with `view()`; `pub fn primary_action(item: &LibraryItem) -> (&'static str, Message)`.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn the_primary_action_names_what_it_will_do() {
    let film = item(MediaKind::Film, Some(1_500));
    assert_eq!(detail::primary_action(&film).0, "Resume 25 min in");
    let fresh = item(MediaKind::Film, Some(0));
    assert_eq!(detail::primary_action(&fresh).0, "Play");
    let episode = item(MediaKind::Series, Some(0));
    assert_eq!(detail::primary_action(&episode).0, "Play S01E03");
}
```

- [ ] **Step 2–5:** fail, implement per §7.4, verify `step_3` and `step_7`, commit `feat(ui): a title page that shows the film, not a record`.

---

### Task 9: Player

**Files:** Create `apps/desktop/src/ui/screens/player.rs`; modify `app.rs` (delete `player_view`, keep `mini_view` until Task 11). Test: `player.rs`, plus `step_4`, `step_6`.

**Interfaces:** `pub struct Player<'a> { pub snapshot: &'a PlayerSnapshot, pub title: &'a str, pub episode: Option<&'a str>, pub output: Option<&'a str>, pub nav: &'a Nav }` with `view()`; `pub fn scrub_fraction(position: f64, duration: f64) -> f32`; `pub fn clock(seconds: f64) -> String`.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn a_zero_or_negative_duration_never_produces_a_nan_scrub() {
    assert_eq!(player::scrub_fraction(10.0, 0.0), 0.0);
    assert_eq!(player::scrub_fraction(-5.0, 100.0), 0.0);
    assert_eq!(player::scrub_fraction(250.0, 100.0), 1.0);
    assert!(player::scrub_fraction(50.0, 100.0).is_finite());
}

#[test]
fn the_clock_reads_like_a_clock() {
    assert_eq!(player::clock(0.0), "0:00");
    assert_eq!(player::clock(65.0), "1:05");
    assert_eq!(player::clock(3725.0), "1:02:05");
}
```

- [ ] **Step 2–5:** fail, implement per §7.5 with `Icon` controls, verify `step_4` and `step_6`, commit `feat(ui): player chrome that recedes`.

---

### Task 10: Settings

**Files:** Create `apps/desktop/src/ui/screens/settings.rs`; modify `app.rs` (delete `settings_view`, keep `settings_actions`). Test: `settings.rs`, plus `settings_are_operable_without_a_pointer`.

**Interfaces:** `pub enum Section { Playback, Casting, Profiles, Folders, Online, Subtitles, Accessibility, Data, About }` with `ALL` and `label()`; `pub struct Settings<'a> { pub section: Section, pub rows: Vec<Row<'a>>, pub nav: &'a Nav }`; `pub enum Row<'a> { Toggle { label: &'a str, hint: Option<&'a str>, on: bool, message: Message }, Value { label: &'a str, value: String }, Slider { label: &'a str, hint: Option<&'a str>, value: f64, range: RangeInclusive<f64>, message: fn(f64) -> Message } }`.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn every_settings_action_has_a_row_somewhere() {
    let sections = Section::ALL;
    assert_eq!(sections.len(), 9);
    let labels: Vec<&str> = sections.iter().map(|s| s.label()).collect();
    assert!(labels.contains(&"Casting") && labels.contains(&"Profiles"));
}
```

- [ ] **Step 2–5:** fail, implement per §7.6, verify the keyboard test still passes, commit `feat(ui): settings as sections, not a scroll`.

---

### Task 11: The surfaces the mockups do not draw

**Files:** Modify `apps/desktop/src/ui/screens/settings.rs` (Casting, Profiles), `detail.rs` (conversion), `library.rs` (genre chip), `shell.rs` (status). Test: the corresponding `step_*` tests.

**Interfaces:** `pub fn casting_rows(state: &CastState) -> Vec<Row<'_>>`, `pub fn profile_rows(profiles: &[Profile], active: &str) -> Vec<Row<'_>>`.

- [ ] **Step 1–5:** Write a test that casting and profile controls are reachable; implement; verify `step_22`, `step_14`, `step_16`, `step_19`; commit `feat(ui): casting, profiles and conversion find their places`.

---

### Task 12: Artwork at 1280

**Files:** Modify `apps/desktop/src/artwork.rs:10,69,94,109,168`. Test: `apps/desktop/src/artwork.rs`.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn a_poster_is_extracted_wide_enough_for_the_hero() {
    assert!(artwork::POSTER_WIDTH >= 1280, "a 530px hero at 1440 must not upscale");
    assert!(artwork::MAX_POSTER_BYTES <= 2 * 1024 * 1024, "one poster must stay bounded");
}
```

- [ ] **Step 2–5:** raise `POSTER_WIDTH` to 1280 and `MAX_POSTER_BYTES` to 2 MiB, run `step_2`, commit `feat(artwork): extract wide enough for a hero`.

---

### Task 13: Accessibility sections

**Files:** Modify `apps/desktop/src/accessibility.rs`, `apps/desktop/src/app.rs` (`accessible_screen`). Test: `accessibility.rs`.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn a_control_can_carry_the_section_it_belongs_to() {
    let snapshot = Snapshot { title: "Home".into(), controls: vec![
        Control { section: Some("Continue watching".into()), ..control("Northern Lights") },
        Control { section: Some("Recently added".into()), ..control("Green Book") },
    ]};
    assert_eq!(snapshot.controls.iter().filter(|c| c.focused).count(), 0);
    assert_eq!(Snapshot::sections(&snapshot), vec!["Continue watching", "Recently added"]);
}
```

- [ ] **Step 2–5:** add `section: Option<String>` to `Control`, publish it per rail, keep the 1:1 label/role invariant, run `SCREENROOM_AX_CHECK` on all five screens, commit `feat(ui): rails announce their section`.

---

### Task 14: Parity pass

**Files:** Modify `apps/desktop/src/app.rs` (delete `mini_view`'s remaining ad-hoc styling if superseded), `apps/desktop/src/app/acceptance.rs` (any layout assertion the redesign changes), `docs/MACOS_REMAINING_WORK.md`, `docs/IMPLEMENTATION_STATUS.md`.

- [ ] **Step 1: Run every gate**

```bash
cargo test --workspace
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
python3 scripts/no_comments.py check
scripts/check-terminate-routing.sh
scripts/check-quit-routing.sh
scripts/check-no-network.sh
scripts/package-macos.sh && scripts/verify-bundle.sh
```

Expected: every command succeeds; the test count is at least the 569 that passed before the rebuild, minus any layout test that was replaced and re-added.

- [ ] **Step 2: Run the accessibility check on all five screens**

```bash
SCREENROOM_AX_CHECK=/tmp/ax.txt SCREENROOM_AX_CHECK_SCREEN=browse SCREENROOM_WINDOW=hidden \
  SCREENROOM_LIBRARY=/tmp/lib.db ./target/debug/screenroom-desktop
```

Expected: non-empty tree, `labels_match true`, `focus_match true`, exactly one focused control.

- [ ] **Step 3: Measure playback before and after**

```bash
SCREENROOM_PLAY_CHECK=/tmp/r SCREENROOM_PLAY_CHECK_MEDIA="<a 4K fixture>" \
  SCREENROOM_PLAY_CHECK_SECONDS=10 ./target/release/screenroom-desktop
```

Expected: `delivered_fps` within 1 fps of the pre-rebuild figure and `uploaded_total` still 0.

- [ ] **Step 4: Commit the parity record**

```bash
git add docs apps/desktop
git commit -m "feat(ui): the rebuilt interface, with behaviour parity recorded"
```

### Task 15: Responsiveness

**Files:** Modify `apps/desktop/src/ui/theme.rs` (a `Breakpoint` enum and `layout_for(width)`), `poster.rs`, `rail.rs`, `shell.rs`, each screen. Test: `theme.rs`, `poster.rs`.

**Interfaces:** `pub enum Breakpoint { Narrow, Compact, Standard, Wide, Ultra }` with `pub fn at(width: f32) -> Breakpoint`, `pub fn columns(self) -> usize`, `pub fn hero(self) -> f32`, `pub fn card(self) -> f32`, `pub fn content_cap(self) -> Option<f32>`.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn every_width_maps_to_a_breakpoint_with_sane_geometry() {
    assert_eq!(Breakpoint::at(800.0), Breakpoint::Narrow);
    assert_eq!(Breakpoint::at(960.0), Breakpoint::Compact);
    assert_eq!(Breakpoint::at(1100.0), Breakpoint::Standard);
    assert_eq!(Breakpoint::at(1500.0), Breakpoint::Wide);
    assert_eq!(Breakpoint::at(3440.0), Breakpoint::Ultra);
    for b in [Breakpoint::Narrow, Breakpoint::Compact, Breakpoint::Standard, Breakpoint::Wide, Breakpoint::Ultra] {
        assert!(b.columns() >= 2 && b.columns() <= 7);
        assert!(b.hero() > 0.0 && b.card() > 0.0);
    }
}

#[test]
fn an_ultrawide_caps_its_content_instead_of_stretching_cards() {
    assert_eq!(Breakpoint::Ultra.content_cap(), Some(1760.0));
    assert_eq!(Breakpoint::Wide.content_cap(), None);
}
```

- [ ] **Step 2: Run it to verify it fails.** `cargo test -p screenroom-desktop --lib ui::theme`
- [ ] **Step 3: Implement** the table in §14 exactly; every screen reads geometry from `Breakpoint::at(width)` rather than hard-coding the mockup's numbers.
- [ ] **Step 4: Verify** at 900×600, 1280×800, 1440×900 and 3440×1440 with the window captures, confirming §15's six points.
- [ ] **Step 5: Commit** `feat(ui): the layout adapts instead of assuming one window size`.
