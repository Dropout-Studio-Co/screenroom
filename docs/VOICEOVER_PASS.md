# The VoiceOver pass

A ten-minute listening pass, surface by surface, so that one real VoiceOver run
settles what a receipt cannot: how macOS actually speaks the tree. The tree is
built in [`../apps/desktop/src/accessibility.rs`](../apps/desktop/src/accessibility.rs)
from `App::accessible_screen()` in [`../apps/desktop/src/app.rs`](../apps/desktop/src/app.rs).

**Nothing here replaces the owner's own listen.** Everything in "Already proved"
was produced by the application's receipt mechanism
(`SCREENROOM_AX_CHECK`) and by the desktop test suite, both of which read the
published tree rather than the spoken output.

## Keys used below

| Key | Does |
| --- | --- |
| ⌘F5 | start or stop VoiceOver |
| VO | Control + Option |
| VO + → / ← | next / previous item in the tree |
| VO + Space | press the selected item |
| VO + Shift + Down | interact with the selected item (needed for a slider) |
| ↑ / ↓ (while interacting) | change a slider by one step |
| VO + Shift + Up | stop interacting |

Start with a library that has at least one film and one series, because an empty
library draws invitations instead of cards.

## Already proved, so it can be skipped

Re-run any of these instead of listening for it:

```bash
cargo test -p screenroom-desktop --lib          # 331 tests, including the tree's own
SCREENROOM_LIBRARY=<library.db> SCREENROOM_WINDOW=hidden \
SCREENROOM_AX_CHECK=/tmp/ax-home.txt SCREENROOM_AX_CHECK_SCREEN=home \
  ./target/debug/screenroom-desktop             # browse|settings|detail|player
```

Each receipt prints `labels_match`, `values_match`, `enabled_match`,
`press_actions_match`, `adjust_actions_match`, `focus_match` and
`focused_count` for the **real** `NSAccessibility` tree read back from AppKit,
compared element by element against the application's own snapshot. On a
five-item test library all of them are `true` on all five surfaces, with one
focused control everywhere except the player, which draws no focus.

That proves, per surface: every published control is currently drawn or drawn by
the same state (no phantom buttons), the order matches the visual order, labels
are non-empty and unique as keys, the value a control carries is the value the
screen shows, `AXPress` exists exactly where a press is drawn, `AXIncrement` /
`AXDecrement` exist exactly on the two drawn sliders, and the focused element is
the one the app marks. Counts: Home 19, Library 23, Detail 30, Settings 33
(Playback), Player 16.

Not proved, and therefore what this pass is for:

1. **The wording.** VoiceOver composes "role, label, value" its own way; the
   receipt proves what was handed to it, not how it sounds.
2. **The announcements.** The status line posts an
   `NSAccessibilityAnnouncementRequestedNotification` when its text changes. The
   code path is tested; the delivery has never been heard.
3. **Per-widget geometry.** Every element still carries the window frame (iced
   exposes no layout geometry, ADR-017), so the VoiceOver cursor highlights the
   whole window rather than one card.

## The pass, in order

### 1. Home (about 90 seconds)

1. ⌘F5, then VO + → from the top.
2. Expect, in this order: **Home, current** · **Library** · **Settings** ·
   **Refresh** · **Search** · the hero title · **RECENTLY ADDED** or
   **CONTINUE WATCHING** · a metadata line · **Play** or **Resume** ·
   **Details**.
3. Then each rail: a heading with a count, then its cards. With the five-item
   test library that is **Recently added, 3** · **Films, 2** · **Series, 1**.
4. Press **Play** on the hero with VO + Space: the player must open and play the
   film the hero names.

Listen for: the hero's **Play** naming the action, not just "button"; the rail
headings reading as headings with their counts; the same title appearing more
than once **and** a rail name immediately before each group (that repetition is
correct — the same film is drawn in more than one rail).

### 2. Library (about 2 minutes)

1. VO + → from the top: **Library, current** · **Refresh** · **Search** · the
   heading **Library** · the count (**5 titles**) · **Search, empty** (or the
   typed query) · four **Sort by …** options · **All / Films / Series** ·
   **Unwatched** · **Available offline** · **Watchlist** · **Favourites** · the
   source line (**1 folder · 5 files · scanned …**) · the cards.
2. On **Films**, VO + Space. Listen that the grid changes and that VoiceOver
   says "selected" on Films and "unselected" on All.
3. On **Sort by recently added**, VO + Space. The same films, reordered, and the
   sort option reads as selected.

Listen for: a chip saying its state as a **checkbox** or **radio button** rather
than as a plain button; the search field being announced as text with the query
it holds (**it cannot be typed into through VoiceOver — this is known**); the
pager appearing only when there is more than one page.

### 3. Detail (about 2 minutes)

1. VO + Space on any card.
2. Expect: **Back to library** · **FILM** or **SERIES** · the title as a heading ·
   the metadata line · **Play** / **Play S01E03** / **Resume 12 min in** ·
   **Watchlist** with its state · **Watched** with its state · the synopsis ·
   each reported fact as its own line · **Season 1** · **2 episodes** ·
   one radio per season · then the episodes · then **Start over** ·
   **Favourite** with its state · **Hide from Continue** · **Convert a copy** and
   one **Convert · …** button per profile.
3. On **Watchlist**, VO + Space: listen for the state flipping to "checked" and
   the round icon filling in.

Listen for: **Start over** and **Favourite** being spoken **after** the episode
list (they are drawn below it), and **Play S01E03** naming the episode rather
than a bare "Play".

### 4. Settings (about 2 minutes)

1. VO + → from the top: **Settings, current** · **Refresh** · **Search** ·
   the heading **Settings** · then the nine section rows, with the current one
   **Playback, current, dimmed** (pressing it would do nothing, so it is
   disabled rather than silently inert).
2. Choose **Data and backups**, VO + Space. Expect the heading **Data and
   backups**, then **Choose backup file…** · **Backup file path** with its value ·
   **Back up now** · **Choose backup to restore…** · **Backup file to restore
   from** · **Check file** · the restore note · **Saved sources** · the three
   source fields · **Save source** · **Choose conversion folder…** · the
   conversion folder · the conversion note.
3. Back on **Playback**: every row that toggles says **checked / unchecked**;
   **Subtitle size** and **Subtitle position** say a percentage and are
   **adjustable** — select one, VO + Shift + Down, ↑ or ↓, VO + Shift + Up.
   Then **Picture** and its two lines.

Listen for: the slider announcing its value and changing by 5% per arrow press;
a row's hint being read immediately after the row, not before it; **dimmed** on a
row whose action is unavailable.

### 5. Player (about 2 minutes)

1. Press **Play** on Detail.
2. VO + → from the top: **Back** · the film title as a heading · the episode ·
   the output line (**8 bits per channel, srgb, …**) · **Preparing streams…**
   until the first frame arrives · **Position** as a slider
   (**0:14 of 1:32**) · **Pause** · **Back 10 seconds** · **Forward 10 seconds** ·
   **Mute** · **Volume** as a slider (**100%**) · **Subtitles** options or
   **Subtitles · none** · **Audio** options or **Audio · none** · **Fullscreen** ·
   the marking hint · **Mini player**.
3. On **Position**, VO + Shift + Down then ↑ / ↓: the film moves ten seconds per
   press. On **Volume**: five per cent per press.

Listen for: the player announcing **no** Home/Library/Settings/Refresh/Search —
the player draws no shell — and **Mini player** being the last item even though
it is drawn in the overlay.

Press **Mini player**, then VO + →: the panel at the foot of the screen is
announced after the page's own content and before the status line (**Nothing
playing** or the title · the clock · **Position** as a slider · **Pause** ·
**Back 10 seconds** · **Forward 10 seconds** · **Full player** · **Close**).

While a scan runs, the strip under the navigation says **Scanning** with its
counts and offers **Cancel scan**; it is announced after the navigation and
before the page's own heading, which is where it is drawn.

### 6. The status line (about 30 seconds)

1. Go to Library, press **Refresh** with VO + Space.
2. Do **not** move the VoiceOver cursor. Listen for the status line
   (**Scanned …** or the error) being **spoken on its own** when it appears.
3. Provoke an error once — for example by backing up to a folder that cannot be
   written — and listen for the error being spoken the same way.

This is the one announcement that cannot be proved by a receipt. If it is not
spoken, the status line is still readable with VO + → at the very end of the
tree, but it is then only readable by going to look — say so and it will be
moved to a real announcement with priority.

## What is deliberately not in the tree

Anything not drawn as a control: the wordmark, the profile avatar and the poster
artwork. A picker drawn as a drop-down is announced as the options it opens onto
(sort, genre, relink, audio, subtitles), because an AX menu cannot be opened
under the pointer; text fields are announced as text with the value they hold,
because typing stays with the keyboard.

## When something is wrong

Record the surface, the exact wording VoiceOver used, and the key from the
receipt (`grep` the label in `/tmp/ax-<surface>.txt`). A wording or ordering
fault is fixed in `accessibility.rs`; a fault that needs the screen itself to
change — a visible focus ring on a Settings row, for example — is a drawn change
and belongs with the interface rebuild, not with the tree.
