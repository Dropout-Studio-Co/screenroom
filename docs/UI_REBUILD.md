# The interface rebuild

How the desktop shell is composed, what the five screens are, how the layout
responds to window width, and how to verify any of it. The design rationale is
in [`specs/2026-11-02-frontend-rebuild-design.md`](specs/2026-11-02-frontend-rebuild-design.md);
the design-system rules it obeys are in [`DESIGN.md`](DESIGN.md) and
[`DROPOUT_DESIGN_INTEGRATION.md`](DROPOUT_DESIGN_INTEGRATION.md). Where the spec
and `DESIGN.md`'s "Primary information architecture and screens" disagree about
layout, the spec and this document are the current description; the Dropout
Design System authority is unchanged.

**Last updated: 2026-11-02.** Source: `apps/desktop/src/ui/`.

## What it replaced

The five screens used to be `*_view` functions in `apps/desktop/src/app.rs`
under a 110 px header panel. They are now composed from one shell, a small
component set (`ui/components/poster.rs`, `rail.rs`, `ui/icons.rs`) and one
module per screen (`ui/screens/`), reading their geometry from the breakpoint
rather than from a mockup's 1440-wide constants.

Nothing below the view layer moved. `app.rs` still owns the library, scanning,
the player, conversion, casting, providers, preferences and checkpoints, and the
`Message` enum keeps its meaning. The Rust core, the player, the packaging
scripts and the Samsung TV client are unchanged by the rebuild.

## The shell

`apps/desktop/src/ui/shell.rs` draws one bar, 76 px high (`BAR_HEIGHT`), over
40 px gutters (`theme::PAD`):

- the wordmark, `SCREENROOM`, in the caption size and semibold; below the
  Standard breakpoint it shortens to `SR`;
- three destinations — Home, Library, Settings — with the active one in ink and
  the others muted; below Standard the labels shorten to `Home`, `Lib`, `Prefs`;
- a Refresh action, a search control that routes to Library, and the viewer's
  profile avatar.

On Home and Detail the bar is stacked over the artwork with a vertical
gradient, so it never takes a band out of the hero; on Library and Settings it is
a solid ground band above the content. The player has no shell. Status and error
copy is a single line at the foot of the window (`shell::status_bar`), shown only
when there is something to say.

Type is Inter, bundled (SIL OFL) and registered at start-up. Spacing is the
fixed scale 4 / 9 / 13 / 26 / 34, radii are 8 / 12 / 14 / 16 / full, and every
colour comes from the pinned Dropout Design System tokens through `ui::theme`.
Dark is the only theme.

## The five screens

| Screen | What it draws |
| --- | --- |
| Home (`ui/screens/home.rs`) | A full-bleed hero over horizontal rails. The hero is the first Continue Watching title, falling back to the first suggestion, with a `CONTINUE WATCHING` or `RECENTLY ADDED` eyebrow, title, metadata line, progress and a primary action. Rails follow in order: Continue watching, Recently added, Films, Series, each built from the same items and deduplicated. An empty library is one centred invitation, not an empty grid. |
| Library (`ui/screens/library.rs`) | A title with a count, a search field and a sort control, then filter chips (All, Films, Series, Unwatched, Available offline, plus Watchlist, Favourites and a genre pick-list when the library has genres), and at the right an honest source line — folders, files and when it was last scanned. The grid is a poster grid paging 24 tiles at a time. |
| Detail (`ui/screens/detail.rs`) | A title page: backdrop (at most 452 px), back affordance, a `FILM` or `SERIES` eyebrow, the title, a metadata line and one primary action that names what it will do (`Play`, `Play S01E03`, `Resume 25 min in`), with two round toggles for watchlist and watched. Below it a synopsis column, a facts block naming what the player really reports, a season selector and an episode list. |
| Player (`ui/screens/player.rs`) | Full-bleed picture with no frame. Chrome is a top scrim (back, title, episode) and a bottom scrim (scrub with elapsed and total, one control row). Chrome recedes after 2.5 s without input while playing (`CHROME_IDLE_AFTER`) and never recedes while paused. |
| Settings (`ui/screens/settings.rs`) | A 200 px section list (160 px at Narrow) beside a content column capped at 780 px: Playback, Casting, Profiles, Folders, Online metadata, Subtitles, Accessibility, Data and backups, About. A row is a label, an optional hint, and a right-aligned toggle, slider or value, separated by 1 px hairlines rather than boxes. Playback carries a *Picture* card stating what the surface actually carries. |

Navigation is one back stack of `(Screen, FocusPos)` in `ui/route.rs`, so Escape
from Detail returns to the card it was opened from rather than the first card.
Home is addressed as `(rail, column)`; Left and Right stay inside a rail, Up and
Down change rail and clamp the column to the shorter rail's length.

## Breakpoints

Defined once, in `ui/theme.rs` (`Breakpoint::at`), and read by every screen.

| Name | Width | Grid columns | Hero | Rail card | Nav |
| --- | --- | ---: | ---: | ---: | --- |
| Narrow | < 900 | 2 | 340 | 176 | compact labels, `SR` |
| Compact | 900–1099 | 4 | 430 | 208 | compact labels, `SR` |
| Standard | 1100–1439 | 5 | 480 | 224 | full labels, `SCREENROOM` |
| Wide | 1440–1759 | 6 | 530 | 240 | full labels, `SCREENROOM` |
| Ultra | ≥ 1760 | 7 | 600 | 268 | full labels, content capped at 1760 and centred |

A non-finite width is treated as Standard. The 40 px gutter, the type scale and
the card shape do not change with width; a card's title truncates rather than
wrapping. The player drops controls as the window narrows — volume below 900,
audio below 1000, subtitles below 1200 — and never drops the scrub. The Library
pages rather than scrolling without end, at every width.

## Verifying it

Geometry and behaviour are unit-tested. `apps/desktop/src/ui/` holds 54 tests:
the breakpoint table and content cap, the nav-label and wordmark rules, focus
clamping and restoration, the idle-chrome rule, control visibility by width, the
source and count lines, chip and row semantics, and the shell composing at every
breakpoint including unknown widths.

```bash
cargo test -p screenroom-desktop --lib            # the desktop library suite
cargo test --workspace
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
python3 scripts/no_comments.py check
```

The application reads the following environment variables, each confirmed in the
source that consumes it:

| Variable | Effect | Where |
| --- | --- | --- |
| `SCREENROOM_LIBRARY=<file>` | opens that SQLite library instead of the default one | `app.rs` |
| `SCREENROOM_OPEN_SCREEN=library\|settings\|detail\|player` | starts on that screen; Detail opens the first Library item, Player opens it and plays | `app.rs` (`open_requested_screen`) |
| `SCREENROOM_WINDOW=hidden` | builds the window hidden and never activates it | `play_check.rs` (`requests_hidden_window`) |
| `SCREENROOM_WINDOW_WIDTH=<px>` | opens at that width, floored at the minimum the layout needs, so a breakpoint can be seen rather than inferred | `app.rs` (`verification_size`) |
| `SCREENROOM_WINDOW_HEIGHT=<px>` | opens at that height | `app.rs` (`verification_size`) |
| `SCREENROOM_AX_CHECK`, `SCREENROOM_AX_CHECK_SCREEN` | **removed 2026-11-03** with the VoiceOver bridge ([ADR-005](ADR-005_SUBTITLE_PROVIDER_AND_FEATURE_REMOVALS.md)); no accessibility receipt is written any more | — |

Example, with a library already built:

```bash
SCREENROOM_LIBRARY=/tmp/room/library.db \
SCREENROOM_OPEN_SCREEN=library \
SCREENROOM_WINDOW=hidden \
  ./target/debug/screenroom-desktop
```

A screenshot is taken with `scripts/capture-window.sh <out.png>`, which resolves
the application's window number and captures that window only. There is **no
environment variable for the window size**: the window opens at 1280×800
(`iced::window_size` in `app.rs`) and records each resize it is given. Capturing
900, 1440 or 3440 therefore means resizing the window while it is visible, which
is a foreground action and needs `SCREENROOM_ALLOW_FOREGROUND=1` from the owner.

## What is verified, and what is not

Recorded as passing on this host in the rebuild session, and re-runnable with the
commands above:

- the `ui/` unit tests above, the desktop library suite (276 tests) and
  `cargo test --workspace`, with `cargo fmt --all --check`, `cargo clippy
  --workspace --all-targets -- -D warnings` and `python3 scripts/no_comments.py
  check`;
- the five breakpoints' geometry, as values, by unit test.

The rebuild session also ran `SCREENROOM_AX_CHECK` on Home, Library, Detail and
Settings and read the accessibility tree back from AppKit. **That check was
removed on 2026-11-03** with the `NSAccessibility` bridge (ADR-005), so it is
history: it is no longer re-runnable, and no VoiceOver receipt is produced. What
the rebuild's keyboard and focus rules still prove is in the `ui/` tests, and the
accessibility that remains is keyboard navigation, focus order and restoration,
reduced motion and subtitle styling.

Not verified:

- **Nobody has said the interface looks right.** Window captures were taken at
  900, 1280, 1440 and 3440 during the rebuild and inspected by the session that
  built it, but they live in `.artifacts/` (Git-ignored) and are not part of a
  checkout, so a clone cannot re-inspect them. The owner's look at the rebuilt
  screens is **PENDING** and tracked as P2-06 in
  [`MACOS_REMAINING_WORK.md`](MACOS_REMAINING_WORK.md).
- **The player's chrome has not been watched receding** in a window; the rule is
  unit-tested and its wiring is in `app.rs`, which is not the same claim.
- **Accessibility remains window-level.** The bridge publishes roles, labels,
  values and focus, but every element carries the window frame rather than its
  own geometry, because iced exposes no layout geometry (ADR-017). A picker is
  published as the options it opens onto and a text field as the value it holds,
  because neither can be driven through the tree.
- **Settings rows have no visible focus ring.** Arrow keys move a focus that the
  tree publishes, and Enter runs the row, but the screen draws no ring on the
  row, so the tree's focus has nothing on screen to agree with. That is a drawn
  change, recorded rather than fixed here.
- **Nothing in this document has run in CI.** The workflows exist; see
  `RELEASE_PIPELINE.md`.

## Deliberate limits

- Dark theme only; there is no light theme to fall out of sync.
- No animation beyond the chrome receding; no idle motion and no autoplaying
  backdrop.
- No icon set beyond the paths in `ui/icons.rs`.
- One rule kept from the previous interface: Settings rows and every screen
  action are reachable from the keyboard. Typed fields and the two sliders still
  need the pointer or typing.
