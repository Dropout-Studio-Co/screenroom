# WS-2 — Settings focus and measured subtitle labels (2026-11-21)

Branch `fix/macos-ui-focus-and-fit`, worktree `.worktrees/macos-ui`, cut from
`origin/main` at `7ac4273`. macOS only; Windows and the Tizen client are
untouched.

## Item 2 — the focused Settings row is a semantic fill inversion

- `ui::screens::settings::Settings` carries `focus: usize`; `app.rs::settings_screen`
  passes `self.settings_focus`.
- `settings::row_style(focused)` is the whole focus treatment: the focused row
  gets `palette.focus_surface` as its background and `palette.focus_foreground`
  as its text colour, `border.width == 0`, a transparent border colour, no
  shadow, `Radius::Small`. Unfocused rows draw no fill and keep `theme::ink()`.
  There is no ring, outline or stroke anywhere, per the Dropout Design System.
- The row's controls follow the inversion instead of competing with it:
  - toggle: focused track `focus_foreground` when on, 24% `focus_foreground` when
    off; knob `focus_surface` when on, `focus_foreground` when off;
  - slider: focused rail filled `focus_foreground`, unfilled 24%
    `focus_foreground`; the value label uses `focus_foreground`;
  - value text uses `focus_foreground`;
  - action buttons draw no fill when focused (transparent at rest, 12%
    `focus_foreground` on hover/press) with `focus_foreground` text.
  - hints use `focus_foreground` at 78% alpha, which measures 9.48:1 on the
    focus surface.
- The hairline below a row is skipped when that row is the focused one, so no
  rule cuts through the inverted fill.
- The content `scrollable` now carries `Id::from("settings-content")`.
  `settings_row_offset(rows, index, width)` returns the distance from the top of
  the settings content column (heading band included) to row `index`, using
  measured label and hint heights and the taller of text block and control.
  `settings_scroll_offset(offset, viewport, row_height)` returns the scroll
  target that keeps the row fully inside the viewport with one `Gap::Group`
  (13 px) of lead, and never scrolls when the row already fits.
  `content_viewport(window_height)` subtracts the real chrome (shell bar, page
  padding, title line, section gap, one status line) computed from the tokens.
  `app.rs::move_settings_focus` now returns `scrollable::scroll_to` after the
  focus change, and the Settings ArrowUp/ArrowDown arms return that task.
- `SCREENROOM_SETTINGS_FOCUS=<n>` sets the initial Settings focus when
  `SCREENROOM_OPEN_SCREEN=settings` opens the screen, clamped to the drawn rows.
  It is inert unless set. The first resize after layout re-applies it and
  scrolls, because a scroll task issued before the first layout has no widget
  tree to act on.

## Item 3 — subtitle labels are measured, not estimated

- New `ui/text_fit.rs`: `ensure_fonts()` loads the three bundled Inter faces into
  `iced::advanced::graphics::text::font_system()` exactly once behind a
  `OnceLock`; `measured_width` lays out a `Paragraph` with `Wrapping::None` and
  returns `min_width()`; `measured_height` uses `Wrapping::Word`; `fit_measured`
  binary-searches the char boundaries and appends `…`, guaranteeing the measured
  width never exceeds the room, with explicit behaviour for empty text, zero or
  negative room, NaN/infinite room, and a room narrower than the ellipsis itself.
- `fit_label` and its 7 px per-ASCII-character estimate are deleted, along with
  the acceptance test that re-implemented the estimate.
- `subtitle_result_text(window_width, result)` measures the suffix
  `" · {cues} cues"` first, reserves it, and takes the label budget from
  `subtitle_label_room(window_width)` (the unchanged `(window − 260)px`
  clamped to `120..=720`) minus the measured `Download` button (label, both
  paddings) and the row spacing and cursor marker. The name is fitted into what
  is left; the suffix is always preserved. Below 900 px the compact
  `"<language> · <n> cues"` label is unchanged.
- `iced` gains its own `advanced` feature in `apps/desktop/Cargo.toml`
  (`features = ["wgpu", "image", "canvas", "advanced"]`) to reach
  `iced::advanced::graphics::text`. No new crate is added or compiled.

## Contrast numbers

Measured from `design::tokens().palette` (the neutral fallback, since this
checkout has no pinned `ds_pinned.rs`), WCAG 2.x relative luminance:

| Pair | Ratio |
| --- | ---: |
| `focus_foreground` on `focus_surface` | 18.34:1 |
| `focus_foreground` at 78% on `focus_surface` (hint) | 9.48:1 |
| `ink` on `focus_surface` (rejected pairing) | 1.05:1 |

The last row is why every text and control inside a focused row is recoloured:
the normal ink would be invisible on the inverted fill.

## Tests added

`ui::text_fit`: wide glyph vs narrow glyph, CJK vs ASCII, size scaling, fitted
label ends with `…` and measures inside its room, an already-fitting label is
returned untouched, zero/negative/NaN room and empty text, a room narrower than
the ellipsis, truncation never splits a multi-byte character, wrapped height
grows as the column narrows.

`ui::screens::settings`: `row_style(true)` is `focus_surface`/`focus_foreground`
with `border.width == 0` and a transparent border and no shadow, `row_style(false)`
is transparent, the focus pair clears 4.5:1, focus is not the rest surface, row
offsets are monotonic and clamp at the last row, a 30-row section keeps the
focused row inside a 600 px viewport while scrolling forward only, a short
section stays at the top, a shorter window has a smaller viewport, the content
id is stable, and the page composes with a focused row.

`app`: a subtitle label keeps its suffix and its action room at 640/900/1280/3440,
a multi-byte name is never split, `SCREENROOM_SETTINGS_FOCUS` parses only a plain
number, the Settings focus walks its section and stops at the end, a focus
request clamps to the drawn rows, and the content scrolls only when the focused
row is below the fold.

`app::acceptance`: `a_wide_subtitle_label_is_measured_not_counted` replaces the
old estimate-based test and asserts, at 640×480 and 1280×800, that the cue
suffix survives, no replacement character appears, and the measured label fits
the space left beside the action.

## Commands and results (this machine, 2026-11-21)

```bash
cargo test -p screenroom-desktop --lib ui::          # 114 passed, 0 failed
cargo test -p screenroom-desktop --lib               # 429 passed, 0 failed
cargo test --workspace                               # 851 passed, 0 failed
cargo clippy --workspace --all-targets -- -D warnings  # clean
cargo fmt --all --check                              # clean
python3 scripts/no_comments.py check                 # "no comments"
```

Hidden-window smoke run (no foreground, no activation):

```bash
CARGO_TARGET_DIR=... SCREENROOM_SKIP_REBUILD=1 SCREENROOM_ALLOW_FOREGROUND=0 \
SCREENROOM_LIBRARY="$(mktemp -d)/room.db" SCREENROOM_OPEN_SCREEN=settings \
SCREENROOM_SETTINGS_FOCUS=3 SCREENROOM_WINDOW=hidden \
  ./target/debug/screenroom-desktop
```

The process built its library and stayed up for 8 s on the Settings screen with
row 3 focused, then exited on SIGTERM (143) with an empty log — no panic, no
diagnostic. The temporary library was deleted.

## Left for the integrator

A visible capture of the inverted focused row and of the subtitle row at
640×480 / 1280×800 needs the owner's foreground permission
(`SCREENROOM_ALLOW_FOREGROUND=1`), which this session did not set. The exact
commands:

```bash
# Settings, fourth row (index 3) focused and scrolled into view
SCREENROOM_ALLOW_FOREGROUND=1 SCREENROOM_OPEN_SCREEN=settings \
SCREENROOM_SETTINGS_FOCUS=3 ./target/debug/screenroom-desktop &
scripts/capture-window.sh .artifacts/ws2-settings-focus.png

# Subtitle label beside its Download button: open a film with online results,
# then capture at 640x480 and 1280x800.
SCREENROOM_ALLOW_FOREGROUND=1 SCREENROOM_WINDOW_WIDTH=640 \
SCREENROOM_WINDOW_HEIGHT=480 SCREENROOM_OPEN_SCREEN=player \
SCREENROOM_LIBRARY=/tmp/room/library.db ./target/debug/screenroom-desktop &
scripts/capture-window.sh .artifacts/ws2-subtitles-640.png
```

`scripts/capture-window.sh` is passive (window-number capture) but the resize
above is a foreground action, so the whole sequence waits for the owner's
`SCREENROOM_ALLOW_FOREGROUND=1`. Until then the item-2/3 visual acceptance is
**PENDING**, not passed: what is verified here is the token maths, the layout
reasoning and the unit/contract tests.

Also note for the integrator: `docs/UI_REBUILD.md` still says Settings rows draw
no focus ring; that sentence is now history and should be updated centrally when
this branch lands.
