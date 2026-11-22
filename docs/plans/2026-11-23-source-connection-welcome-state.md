# Source connection / welcome state — a residual test source, not a gating defect

**Date:** 2026-11-23
**Scope:** macOS only (ADR-004; development priority 2026-11-05). The owner's installed
library, plus repo regression coverage and one operating rule.
**Question:** the last session showed six titles on a first-run-looking screen instead of the
welcome state. Is the onboarding gate wrong?
**Outcome:** the gate is correct and no UX change is justified. One GIT-ignored test folder is
still registered as an available source in the installed library. Remove that source with the
app's own non-destructive transition, add regression coverage for "removing the last available
source", and stop probe media from ever reaching the installed library.

---

## 1. Evidence

### 1.1 The installed library holds a residual test source

```bash
sqlite3 -readonly "$HOME/Library/Application Support/Screenroom/library.db" \
  "select id,label,root_path,state,last_scan_ms from sources;
   select source_id, sum(available), count(*) from files group by source_id;"
```

```
de4f8263-…-bafce9c9c094|Screenroom Test Media|/Users/beyourahi/Desktop/projects/screenroom/.artifacts/media|available|1795382779353
de48acb2-…-ad764ea44d4b|bleh|/Volumes/bleh|missing|1793920048550
de48acb2-…|0|861
de4f8263-…|6|6
```

- `Screenroom Test Media` is `available`, 6 of 6 files available, root present, and its files are
  the six benchmark clips in `.artifacts/media` (including `Screenroom.Resume.Check.2026.1080p.mp4`) —
  exactly the six titles on the observed screen.
- `bleh` is a missing USB volume: 0 of 861 available, and `/Volumes/bleh` does not exist.
- `last_scan_ms` equals the live database's mtime, so a real scan ran against this database.

### 1.2 The gate and the welcome already behave correctly

- `App::onboarding_holds` (`apps/desktop/src/app.rs:3938`) holds when `media_total == 0` and the
  screen is not Sources/Settings/Player/Detail; `view` (`:6708`) then draws `sources_screen`.
- `sources_screen` (`:6830`) renders `welcome` only when `self.sources.is_empty()`, and the
  purpose-built `empty_library` state when sources exist but `media_total == 0`.
- `available_media_count` (`crates/screenroom-core/src/library/source_folders.rs:121`) counts only
  `files.available = 1 AND sources.state = 'available'`, so the missing USB source contributes 0;
  `Library::sources` (`crates/screenroom-core/src/library/sources.rs:11`) hides `state = 'removed'`.
- `ui::shell::destinations` (`apps/desktop/src/ui/shell.rs:66`) shows only Sources/Settings when
  `has_media` is false.
- A temp copy with both sources removed launches the installed 0.1.12 binary with zero non-removed
  sources, no auto re-add and no failure; with only the test source removed it has one missing source
  and zero available media.

### 1.3 Why the row exists, and why it is not owner content

- `.artifacts/` is GIT-ignored; `.artifacts/media` is generated probe media, never a viewing library.
- No tracked script adds a source to the default database — every `prepare_library` caller passes a
  scratch path (`scripts/verify-watch.sh:34`, `scripts/verify-media.sh:7`, `scripts/verify-quit.sh:49`)
  — so the row came from an ad-hoc or development launch on the default library path.
- No tracked file mentions "Screenroom Test Media", so nothing depends on that label.

### 1.4 Safety net already in place

`.artifacts/onboarding-welcome-fix/library.db.before-cleanup` exists and its sha256 matches the live
database (`0b4816d2…3907695`). `pragma integrity_check` returns `ok`; no desktop process is running.

---

## 2. Decision

### 2.1 No UX change

The welcome screen, the offline/no-media state, the nav gate and the onboarding hold are all correct
and already cover both post-cleanup outcomes; changing them would hide the real state. No source file
may special-case `.artifacts`, a test title or a file name.

### 2.2 Remove the residual source, using the app's own transition (executable)

Preconditions, all of which must hold before writing:

```bash
cd /Users/beyourahi/Desktop/projects/screenroom
pgrep -fl screenroom-desktop                     # must print nothing
shasum -a 256 "$HOME/Library/Application Support/Screenroom/library.db" \
              .artifacts/onboarding-welcome-fix/library.db.before-cleanup   # one line, equal digests
sqlite3 -readonly "$HOME/Library/Application Support/Screenroom/library.db" \
  "pragma integrity_check;
   select count(*) from source_folders where source_id='de4f8263-1ae2-4584-9625-bafce9c9c094';"
```

Expected: `ok`, then `0`. If any precondition fails, stop and report — do not improvise.

The write is the same transition as `Library::set_source_state` (`sources.rs:217`): hide the files,
mark the source removed, one transaction, no row deletion, no media touched.

```bash
DB="$HOME/Library/Application Support/Screenroom/library.db"
SRC=de4f8263-1ae2-4584-9625-bafce9c9c094
sqlite3 -bail "$DB" "BEGIN IMMEDIATE;
  UPDATE files SET available = 0 WHERE source_id = '$SRC';
  UPDATE sources SET state = 'removed' WHERE id = '$SRC';
  COMMIT;"
```

Postconditions:

| query | expected |
|---|---|
| `select count(*) from sources where state != 'removed';` | `1` |
| `select label,state from sources order by label;` | `bleh\|missing`, `Screenroom Test Media\|removed` |
| `select source_id,sum(available),count(*) from files group by source_id;` | `bleh 0\|861`, test source `0\|6` |
| `select count(*) from files f join sources s on s.id=f.source_id where f.available=1 and s.state='available';` | `0` |
| `select count(*) from works;` | unchanged (metadata and watch history are kept) |
| `pragma integrity_check;` | `ok` |

Do not delete rows from `files`, `works`, `progress`, `source_folders` or `subtitles`; removal is
reversible by re-admitting the source, which is why the existing removal test can still find the
favourite afterwards.

### 2.3 Expected states after the cleanup

| library | screen | nav | actions |
|---|---|---|---|
| USB source still configured, `/Volumes/bleh` absent (`media_total = 0`, one source) | `empty_library`: "Your media is offline", copy naming the unreadable location and "Nothing was deleted." | Home and Library hidden; Sources and Settings only | `Review folders in bleh`, `Add another location`, `Scan all locations` |
| every source removed (`sources` empty) | `welcome`: first-run source picker | Home and Library hidden; Sources and Settings only | choose a folder, `Add another location` |

The USB row is deliberately kept so relinking the drive restores 861 files without re-adding it.

### 2.4 Regression coverage in the repo

Strengthen the existing test rather than adding an abstraction:
`apps/desktop/src/app.rs`, `removing_a_folder_asks_first_and_keeps_the_watch_history` (`:9846`). Its
`app_with(2)` library has one source, so the second `RemoveSource` already removes the last available
source. Insert after the `"the folder leaves the visible list"` assertion:

```rust
assert_eq!(app.media_total, 0, "no available media remains");
assert_eq!(
    app.library.available_media_count().expect("count"),
    0,
    "an available count must not survive the removal"
);
assert!(app.onboarding_holds(), "removing the last source gates the library again");
assert_eq!(app.effective_screen(), Screen::Sources);
assert!(app.sources.is_empty(), "no source is listed, so the sources screen has no list to draw");
let visible: Vec<&str> = crate::ui::shell::destinations(false)
    .iter()
    .map(|destination| destination.label)
    .collect();
assert_eq!(visible, vec!["Sources", "Settings"], "Home and Library stay hidden");
```

`app.sources.is_empty()` is the exact condition `sources_screen` branches on, so with
`effective_screen() == Screen::Sources` the no-source welcome is reachable; the welcome's own render
is already covered in `apps/desktop/src/ui/screens/sources.rs` (`:2817`). Do not add a second test
for the same transition, and do not assert a screenshot or an `Element` shape.

Also assert the offline case is untouched by relying on the existing
`a_disconnected_source_leaves_the_connected_library_usable` and
`a_watcher_that_stops_marks_the_source_unavailable_immediately` tests; if either stops covering
`media_total == 0` with a source still configured, fix that one rather than adding scaffolding.

### 2.5 Prevention rule in `AGENTS.md`

Insert into the "Testing without taking over the desktop" section, immediately after the
three-tier ordered list and before "**Foreground UI automation is opt-in.**":

```
**Scratch libraries for anything that adds a source.** A check, a script or a
development launch that adds a media location must point `SCREENROOM_LIBRARY` at a
scratch database. Never add probe media, test clips or a benchmark folder to the
library the installed application opens
(`~/Library/Application Support/Screenroom/library.db`); a GIT-ignored test folder is
still not owner content. If a residual test source does land there, remove it with the
app's own non-destructive transition — `files.available = 0` for that source and
`sources.state = 'removed'` — after backing the database up, and never by deleting the
media or the rows. `scripts/verify-watch.sh` is the model.
```

No comments are added to any source file; `python3 scripts/no_comments.py check` must stay clean.

---

## 3. Verification

### A. Workspace checks

```bash
cargo test --workspace
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
python3 scripts/no_comments.py check
```

The strengthened test must fail if any of its new assertions are removed — delete one assertion
locally, run `cargo test -p screenroom-desktop removing_a_folder`, and restore it.

### B. Hidden-window launches (never foreground)

```bash
cargo build -q -p screenroom-desktop
work="$(mktemp -d "${TMPDIR:-/tmp}/screenroom-welcome.XXXXXX")"
report="$work/quit.txt"; mkdir -p "$work/empty" "$work/live"

SCREENROOM_LIBRARY="$work/empty/library.db" SCREENROOM_WINDOW=hidden \
SCREENROOM_QUIT_CHECK="$report" target/debug/screenroom-desktop >"$work/app.log" 2>&1 &
pid=$!
for _ in $(seq 1 40); do [ -s "$report" ] && break; sleep 0.25; done
kill "$pid" 2>/dev/null; wait "$pid" 2>/dev/null
tr -d '[:space:]' <"$report"          # expect: routed
sqlite3 "$work/empty/library.db" "select count(*) from sources;"   # expect: 0, no auto-add
```

Repeat with `SCREENROOM_LIBRARY="$work/live/library.db"` after
`cp "$DB" "$work/live/library.db"`, asserting the copy keeps one missing source, zero available
files, and gains no source. Confirm with `SCREENROOM_ALLOW_FOREGROUND=0` that the frontmost
application never changes; do not set `SCREENROOM_ALLOW_FOREGROUND=1`.

Then relaunch `~/Applications/Screenroom.app` once and re-run the §2.2 postcondition table against
the installed library, plus `available_media_count` = `0` and
`select count(*) from sources where state='removed';` = `1`.

### C. Owner visual pass — PENDING

Whether the offline state and the welcome look right is a visual judgement about the owner's own
application. Record it PENDING: relaunch `~/Applications/Screenroom.app`, observe the Sources screen
with the USB source configured and then with it removed, and confirm Home/Library are absent from the
nav. No capture is taken; passive window capture would still need the owner's permission.

---

## 4. Rollback

1. Quit the application; confirm `pgrep -fl screenroom-desktop` prints nothing.
2. `cp .artifacts/onboarding-welcome-fix/library.db.before-cleanup "$HOME/Library/Application Support/Screenroom/library.db"`
   and confirm the sha256 matches `0b4816d2…3907695` and `pragma integrity_check` returns `ok`.
3. Alternatively re-add the folder at the saved path
   `/Users/beyourahi/Desktop/projects/screenroom/.artifacts/media`, or re-admit the row and rescan it
   (removal zeroes `files.available`, so a rescan is needed).
4. No media file, no artwork and no backup archive is touched by any step of this plan.

---

## 5. Delivery and definition of done

1. Cleanup applied, postconditions proven (§2.2, §3B).
2. Test strengthened and green (§2.4, §3A), `AGENTS.md` rule added (§2.5).
3. Commit as one Conventional Commit with no AI attribution, push, then run
   `scripts/publish-update.sh --push --if-changed` — the test lives under `apps/`, so the change
   publishes; the database cleanup is not a code change and ships nothing.
4. Report **Completed / Verified / Blocked / Next**; the §3C visual pass stays PENDING, never passed.
