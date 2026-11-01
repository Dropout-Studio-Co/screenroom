# Screenroom — frontend rebuild design

- **Date:** 2026-11-02
- **Status:** proposed; awaiting owner approval before any product code changes
- **Supersedes:** the view layer composition described in `docs/DESIGN.md`
  ("Primary information architecture and screens") where the two disagree on
  layout; the Dropout Design System authority itself is unchanged
- **Scope:** the macOS desktop client's interface only (`apps/desktop`). The Rust
  core, the player, casting, providers, packaging and the Samsung TV client are
  untouched.

## 1. Intent, and why

The application works and looks like a control panel. The owner's words were that
the interface is "completely unusable"; the screenshot taken on 2026-11-02 shows
what that means in practice: stacked headings, a large empty "Continue watching"
panel occupying the fold, a single row of three cards followed by dead space, a
16:9 frame letterboxed inside a tall card so half of every card is empty air, a
filename printed under every title, an "Add another folder" form sitting on the
browse surface, one typographic weight throughout, and a rounded header panel
eating 110 px of height.

The goal is a **media application** experience — poster-forward, cinematic,
browsable in a glance — rendered in the **Dropout Design System's visual
language**. The owner chose explicitly: *"the experience only, in Dropout's
black-and-white language."* Netflix's information architecture, pacing and
interaction model are the reference. Its visual identity, brand colour and chrome
are not, and `docs/DESIGN.md` forbids copying them.

The owner also chose to **rebuild the shell and screens from scratch** rather than
recompose the existing view functions, accepting the larger risk in exchange for a
clean architecture.

## 2. Goals

1. A **hero** on Home: the focused or in-progress title as large backdrop art with
   a title, a horizontal progress reading, and a primary action.
2. **Horizontal rails** that fill the width and invite scrolling, instead of a
   fixed grid row that stops after three items.
3. **Poster-forward cards**: artwork is the element; metadata is a quiet caption;
   the filename never appears in the browse surface.
4. A **nav bar** that is a thin strip over the artwork, not a panel.
5. **Focus as light**: the focused control gets a stronger surface and contrasting
   ink, per the design system, never a ring.
6. A **title page** that feels like a film: backdrop, synopsis, facts, and an
   episode list that reads as a list of episodes rather than rows of a table.
7. A **player** whose chrome recedes: a thin scrub, a small control cluster,
   legible subtitles, nothing else.
8. **Settings** as a sectioned two-column page, not one long scroll of panels.
9. Every behaviour that works today keeps working, and every acceptance test that
   passes today still passes.

## 3. Non-goals

- No change to the Rust core, the player, the casting session, the providers, the
  SQLite schema, the packaging scripts or the Samsung TV client.
- No new runtime dependency, no web view, no CSS, no hosted asset.
- No light theme. The application is dark; the design system's `.dark` theme is
  the only theme.
- No icon set beyond what can be drawn as small vector paths or taken from the
  design system's approved icons.
- No animation for its own sake: no idle motion, no autoplaying backdrop video.

## 4. Anti-goals

- Reproducing Netflix's red, its logotype, its chrome or its exact card geometry.
- A "big marketing fold" that buries the actual library (`docs/DESIGN.md` forbids
  this explicitly).
- Boxing everything in panels. The design system asks for information to live on
  surfaces.
- A rebuild that loses the accessibility tree, keyboard operation or the offline
  guarantees built in the previous session.

## 5. Constraints the builder cannot negotiate

1. **Dropout Design System is the visual authority.** Values come from the pinned
   DTCG tokens already in the working tree (`apps/desktop/src/design/ds_pinned.rs`,
   generated from the private DS at its pinned commit). Dark theme only:
   ground `#171717`, ink `#f7f7f7`, muted `#a3a3a3`, surface1 `#262626`,
   surface2 `#2b2b2b`, surface3 `#333`, hair `rgba(255,255,255,0.10)`, signal
   `#ffffff`, focus surface `#f7f7f7` with `#171717` foreground, danger
   `#ff6467`. Type scale 10/12/14/16/17/18/24/30/36/48/60 rem-based. Radii
   8/12/14/16. No borderless-ring focus, ever.
2. **No comments in code.** Enforced by `scripts/no_comments.py check`.
3. **iced 0.14 + wgpu 27.** Layout is flexbox-like, there is no CSS. A rail is a
   horizontal `scrollable`; a hero is a `stack` of image and scrim; focus styling
   is per-widget `.style()`.
4. **Offline and self-contained.** No remote font, no CDN, no network for any
   screen.
5. **The accessibility bridge must keep working.** `apps/desktop/src/accessibility.rs`
   publishes a `ScreenFacts` per screen and drives actions back into the message
   loop. Every rebuilt screen must publish the equivalent facts, or the VoiceOver
   work is lost.
6. **Keyboard first.** Every action reachable without a pointer; focus order
   deterministic; focus restored on return.
7. **The existing acceptance suite is the behaviour contract.** 569 tests pass
   today. Where a test asserts a message or a status string it should keep
   passing; where it asserts a *layout* detail that the redesign intentionally
   changes, the test is updated with its reason recorded, never deleted.

## 6. Approaches considered

**A. Rebuild the view layer, keep the state machine.** Replace the view functions
with a component set behind a new shell. Contained risk; the "unusable" part is
composition, not function.
**B. Rebuild the shell and screens from scratch (chosen).** New navigation, new
screen modules, new component library, re-plumbed message and focus handling.
Cleanest architecture; risks the 569 passing tests and a long migration.
**C. Restyle in place.** Cheapest; delivers no hero and no rails, so it does not
answer the request.

The owner chose **B** after being told A was recommended and that B puts the
passing suite at risk. The design below is therefore written for a clean rebuild,
with the mitigation in §9 (behaviour parity is a hard gate, not an aspiration).

## 7. The design

Mockups are the visual source of truth, produced in Paper before implementation:
`Screenroom / Inventive yew` — artboards `01 Home`, `02 Library`, `03 Detail`,
`04 Player`, `05 Settings`. Every measurement below is taken from them.

### 7.1 Shell

A 76 px top bar, transparent over artwork and solid `#171717` elsewhere: wordmark
left in 12 px semibold with 0.22 em tracking; primary navigation beside it (13 px,
active `#ffffff` medium, inactive `#8f8f8f` regular); right side a search affordance
and a 30 px profile circle. No panel, no box, no divider unless the content scrolls
under it.

Screen padding is a single constant: 40 px horizontally. Vertical rhythm comes from
a small set of gaps (4, 9, 13, 22, 26, 34) rather than per-screen invention.

### 7.2 Home

Hero **530 px**: backdrop art with a vertical scrim that resolves to `#171717` at
its foot. Content anchored bottom-left at x=40: a 10 px eyebrow with 0.20 em
tracking naming *why* this title is here (`CONTINUE WATCHING`), a 56 px/1.03
semibold title with −0.035 em tracking, a 12.5 px metadata line with `·`
separators, a 2 px progress rail with "34 min left", then a white 44 px primary
action and a `#2b2b2b` secondary.

Below it, **rails**: a 15 px semibold section label with a small count, then a
horizontally scrolling row of 240×135 cards at 16:9 with a 10 px radius, 16 px
gaps, a 13 px medium title and an 11 px muted caption. The first card in a rail is
the focused one and carries a 2 px white edge plus white title. In-progress cards
carry a 2 px progress bar along their lower edge. A rail overflows the right edge
deliberately: it is the affordance that says "there is more".

Rails on Home, in order: **Continue watching**, **Recently added**, then one per
kind present in the library. When the library is empty, Home is a single centred
invitation, not an empty grid.

### 7.3 Library

A 34 px title with a small `318 films · 18 series` count beside it; on the right a
250 px search field and a sort control, both on `#212121` at a 10 px radius. Below,
filter chips (32 px, fully rounded, active = white fill with `#171717` ink) and, at
the far right, an honest source line: *2 folders · 775 files · scanned 2 hours ago*.

The grid is 206×116 tiles, 6 per row at 1440, 18 px column gaps and 26 px row gaps,
each with a 13 px title and 11 px caption. Paging replaces infinite scroll; the
existing paging, sort and filter logic is reused unchanged.

### 7.4 Title page

A 452 px backdrop with a back affordance at the top left, then: a 10 px kind
eyebrow (`SERIES` / `FILM`), a 54 px title, a metadata line, and a primary
action that names what it will do (`Play S01E03`, or `Resume 34 min in`), with two
44 px circular secondaries for watchlist and watched.

Under the hero, two columns: a 600 px synopsis at 12.5 px/1.65 in `#b4b4b4`, and a
compact facts block naming the real video, audio and subtitle facts the player
reports. Then a season selector and the episode list: each row is an index, a
120×68 still, a title with its runtime, and a two-line synopsis; the current
episode sits on `#232323` with a white play chip, watched episodes are muted.

### 7.5 Player

Full-bleed picture, no frame, no border. Chrome appears only on movement: a 200 px
top scrim with back, title and episode; a 330 px bottom scrim holding a 3 px scrub
rail with elapsed and total at its ends, and one control row — play/pause, −10 s,
+10 s, volume, then subtitles, audio and fullscreen on the right. Subtitles sit at
20 % from the bottom, 24 px, white, with a soft shadow, never inside a box.

Every control is a vector path, not a typographic glyph.

### 7.6 Settings

A 200 px section list on the left (Playback, Folders, Online metadata, Subtitles,
Accessibility, Data and backups, About) with the active section on `#2b2b2b`, and a
780 px content column of rows. A row is a label, an optional one-line explanation in
`#7a7a7a`, and a right-aligned control: a 40×23 toggle, a slider with its value, or
a value. Rows are separated by 1 px `rgba(255,255,255,0.06)` hairlines, not boxes.

A "Picture" card at the foot of Playback states what the surface actually carries
(`16 bits per channel · pq / bt.709 · target peak 406 nits`) instead of implying
HDR.

## 8. Architecture

```
apps/desktop/src/ui/
  mod.rs          Theme adapter: DS tokens -> iced colours, text, spacing
  shell.rs        Top bar, screen frame, global shortcuts
  route.rs        Screen enum + back stack + focus restoration
  components/
    poster.rs     Card: art, title, caption, progress, focus
    rail.rs       Horizontal scroller with a section label
    hero.rs       Backdrop, scrim, title block, actions
    button.rs     Primary, secondary, quiet, danger
    toggle.rs     Switch
    chip.rs       Filter chip
    field.rs      Search / text input
    empty.rs      Empty and error states
  screens/
    home.rs  library.rs  detail.rs  player.rs  settings.rs
```

`app.rs` keeps: library access, scanning, playback control, conversion, casting,
providers, preferences, checkpoints, and the `Message` enum's *meaning*. What moves
out is every `*_view` function and the ad-hoc styling scattered through them.

`route.rs` owns one back stack with the previous focus per route, which is what
makes "Escape returns to the card you left" a property rather than a per-screen
patch.

## 9. Behaviour parity is a hard gate

The rebuild is not done when it looks right. It is done when:

- `cargo test --workspace` passes with no test deleted. Where a test asserted a
  layout detail the design changes, the test is updated and the change recorded.
- `scripts/check-terminate-routing.sh`, `scripts/check-quit-routing.sh` and
  `scripts/check-no-network.sh` still pass.
- The accessibility check (`SCREENROOM_AX_CHECK`) still reports a non-empty tree
  with matching labels on all five screens.
- `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all --check`
  and `python3 scripts/no_comments.py check` are clean.
- Playback still delivers the source frame rate with zero uploads, measured by
  `SCREENROOM_PLAY_CHECK` before and after.

## 10. Slice order

1. Theme adapter, shell, route, and the component set, proven by rebuilding Home.
2. Library, then the title page.
3. Player chrome.
4. Settings.
5. Parity pass: accessibility facts, keyboard journey, acceptance tests.

Each slice is committed separately and must leave the tree green.

## 11. Risks

- **iced's widget set is the real constraint.** Rails, scrims and focus fills are
  all achievable, but there is no CSS transition; anything animated is manual. If a
  mockup detail cannot be expressed, the builder says so rather than faking it.
- **The rebuild touches every acceptance test that navigates.** Mitigation: the
  route and focus model is built first and unit-tested before screens move.
- **Accessibility can regress silently.** Mitigation: the AX check is run per
  slice, not at the end.

## 12. What is left to the builder

Icon shapes, exact card hover states, the wording of empty and error states, the
loading treatment for a scan in progress, and how a very large library pages in the
new grid. Each is a judgement call inside the constraints above.

## 13. Decisions taken after the builder check

An independent builder read this document cold and raised six gaps. Three are
settled here as the builder's calls; three are put to the owner because a wrong
guess would change what the application is, not just how it is built.

### Settled by the builder

**Focus model.** Home is `(rail, column)`, not a flat index. Up/Down move between
rails keeping the column where possible and clamping at the shorter rail's end;
Left/Right move within a rail and scroll it into view. The top bar and the hero's
actions are pointer-and-Tab targets, not part of the arrow-key ring, because a
keyboard user pressing Down at the top of a rail expects the next rail, not the
chrome. The hero does **not** follow focus: it shows the title you were last
watching, falling back to the most recently added. A hero that rewrote itself on
every arrow key would fight the reading of the row you are on, and
`docs/DESIGN.md` asks for Continue Watching at the top, which a reactive hero
would overwrite.

**Accessibility shape.** The flat `ScreenFacts` model is extended, not preserved
byte for byte: it gains an optional section label per control so a rail can
announce *Rail → card*, while keeping the invariant the AX check already proves —
one focused element, and a 1:1 ordered match of label and role between the
published snapshot and the real AppKit tree. Slice 1 therefore includes AX work,
and the check runs per slice.

**Icons.** The `canvas` feature of iced 0.14 is enabled and the player's controls
are drawn as paths. That adds `lyon` and its tessellation crates to the build
graph — free, permissively licensed, compile-time only — and is the smallest way
to get real vector controls. The alternative, a hand-written path renderer on top
of the video shader, is more code and more risk for the same pixels.

### Put to the owner

1. **Where the surfaces the mockups do not draw belong.** Casting, profiles,
   conversion, saved sources, genre filtering, subtitle-provider search and the
   global status/error line all exist today and appear in none of the five
   artboards. Proposed: Settings gains *Casting* and *Profiles* sections;
   conversion stays on the film's page; genre filtering joins the Library filter
   row as a chip that opens a menu; the status line becomes a slim transient bar
   at the foot of the window that only appears when there is something to say.
2. **Hero artwork.** Extraction is 400 px wide today; a 530 px hero at 1440 wide
   would be a 3× upscale and would look soft in the one element the rebuild exists
   to fix. Proposed: extract at 1280 px wide per work — roughly +50 MB for this
   library — re-extracted on the next scan, with existing libraries staying soft
   until then rather than blocking.
3. **Type.** Proposed: bundle Inter (SIL OFL, offline, no network) and drop the
   mockups' letter-spacing, because iced has no tracking and reaching it would
   mean a custom text widget for a refinement rather than a structure. Sizes snap
   to the design system's scale.

## 14. Responsiveness

The window is resizable from roughly 900×600 to an ultrawide 3440×1440, so the
layout is defined by breakpoints rather than by the 1440×900 mockup alone. The
mockup is the 1440 case; every other size is derived from it.

| Width | Grid columns | Hero height | Rail card | Nav |
| --- | --- | --- | --- | --- |
| ≥ 1760 | 7, content capped at 1760 and centred | 600 | 268 | full |
| 1440–1759 | 6 | 530 | 240 | full |
| 1200–1439 | 5 | 480 | 224 | full |
| 1000–1199 | 4 | 430 | 208 | nav labels collapse to icons |
| 900–999 | 3 | 380 | 192 | wordmark shortens to a mark |
| < 900 | 2 | 340 | 176 | nav behind one control |

Rules that hold at every size: the 40 px gutter is constant; a rail never shows a
partially clipped *last* card — it is either fully visible or fully off; a card's
title truncates rather than wrapping; the settings content column stops growing at
780 px and the section list stops shrinking at 160 px; the player's control row
drops subtitles-then-audio then volume as the width falls, never the scrub.

## 15. What "premium" means as a testable bar

Not an adjective. A build is premium when all of these hold, and each is checked
on a real screenshot rather than asserted:

1. **Optical alignment.** Every left edge in a screen sits on the 40 px gutter or
   on a multiple of the card pitch. No element is off by one or two pixels.
2. **One type rhythm.** Display sizes track tight; small labels track open; no
   three sizes within 2 px of each other competing in one block.
3. **Contrast at a glance.** Body and caption text clear WCAG AA on `#171717`;
   nothing important is below `#8f8f8f`.
4. **No orphan space.** A screen never ends in a large dead band; content either
   fills it or the composition is deliberately centred.
5. **Artwork is the colour.** Nothing in the chrome competes with a poster; the
   only saturated ink anywhere is the destructive red.
6. **Focus is unmistakable** without a ring, at every size, in a screenshot.
