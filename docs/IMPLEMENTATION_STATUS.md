# Implementation status ledger

Live record of what exists, what was actually verified, and what is blocked.
Future sessions resume from here instead of repeating completed work.

Last updated: **2026-11-03**, host **macOS ARM64 (Apple silicon), Command Line
Tools only, no full Xcode**.

**Scope (2026-10-26, [ADR-003](ADR-003_DESKTOP_AND_TV_SCOPE.md)):** desktop/laptop
and smart TVs only, with equal priority. iOS/Android phone and tablet apps, UniFFI
and the Flutter fallback are out of scope; none had been built, so no code was
removed. The development host's Flutter, flutter-tizen and .NET toolchains were
removed. The Flutter candidate's `archive/*` branches were deleted on 2026-10-29.

---

## Owner decisions, 2026-11-03

Recorded in [`ADR-005`](ADR-005_SUBTITLE_PROVIDER_AND_FEATURE_REMOVALS.md). These
supersede the current-behaviour statements below them; every dated receipt further
down is kept as written, because it records something that really ran.

| Decision | What it changes here | Detail |
| --- | --- | --- |
| **The Subtitle DB is the only online subtitle provider**; OpenSubtitles is removed | F15's provider is TSDB, not OpenSubtitles. Its own path has one recorded live run (2026-11-03, `SUBTITLE_PROVIDER.md` §8); a downloaded subtitle drawn in the player and offline reuse after a restart stay **PENDING** | [`SUBTITLE_PROVIDER.md`](SUBTITLE_PROVIDER.md) |
| **Subtitles resolve file-first**: embedded track, then sidecar, then the provider | F13/F15 acceptance includes the order itself: the online action returns before any network call when a track or a sidecar exists, and there is no second service or fallback chain | [`SUBTITLE_PROVIDER.md`](SUBTITLE_PROVIDER.md) §1 |
| **VoiceOver-specific functionality removed** — the `NSAccessibility` bridge, its announcements and its `SCREENROOM_AX_CHECK` receipt | Q07's screen-reader half is **removed**, not complete. The keyboard, focus, reduced-motion and subtitle-styling requirements are unchanged | [`VOICEOVER_PASS.md`](VOICEOVER_PASS.md) (retired) |
| **HDR removed completely** — no mode, no extended-range output, no setting, no receipt; SDR is the only output | F07's HDR half and the P1-03 HDR task are **removed**, not complete. The HDR measurements of 2026-11-01/02 stay as history | [`HDR_VERIFICATION.md`](HDR_VERIFICATION.md), [`ADR-019`](ADR-019_HDR_PRESENTATION.md) (both retired) |

What this does **not** claim: that any of it has run. Removing a feature needs no
device test. The TSDB paths need the owner's key and the commands in
`SUBTITLE_PROVIDER.md` §8; the provider's own path has now been run there once
(2026-11-03), and the two steps that need a visible window and a person are still
PENDING.

### Removals completed on this host, 2026-11-03

| Removed | Evidence it is gone | What replaced it |
| --- | --- | --- |
| `apps/desktop/src/accessibility.rs` (2,703 lines) and its call sites | the file is deleted; no `NSAccessibility` selector, `ScreenFacts`, `Role`, `Action`, `SCREENROOM_AX_CHECK` or `mod accessibility` remains, and `grep -rn "NSAccessibility" apps/desktop/src` is empty (the one surviving word is `accessibilityDisplayShouldReduceMotion`, the reduced-motion system query moved to `motion.rs`); `cargo test -p screenroom-desktop --lib` is 351 tests, the 26 AX tests gone and the keyboard/focus ones intact | `apps/desktop/src/motion.rs` keeps `system_reduce_motion` for the three callers that used it |
| HDR mode, extended-range output, the setting and `hdr_check` | `grep -rniE "SCREENROOM_HDR\|HdrMode\|hdr_extended\|extended_range\|Rgba16Float\|is_extended" apps crates` returns nothing; `crates/screenroom-player/src/display.rs` deleted | SDR only: libmpv still decodes 10-bit/4K and tone-maps into the eight-bit surface |
| `vendor/iced_wgpu` and the root `[patch.crates-io]` entry | directory deleted; `Cargo.toml` has no patch section; `Cargo.lock` records `source = "registry+https://github.com/rust-lang/crates.io-index"` and checksum `ff144a99…` | crates.io `iced_wgpu` 0.14.0 |

SDR presentation still runs after the compositor fork was dropped. The
application's own `SCREENROOM_PLAY_CHECK`, release build, hidden window, on a
generated 1920×1080 test pattern:

```bash
SCREENROOM_ALLOW_FOREGROUND=0 SCREENROOM_PLAY_CHECK=/tmp/sdr-check.csv \
SCREENROOM_PLAY_CHECK_MEDIA=".artifacts/media/Screenroom.Test.Pattern.2026.1080p.mp4" \
SCREENROOM_PLAY_CHECK_SECONDS=8 ./target/release/screenroom-desktop
```

observed: `picture_surface Bgra8Unorm`, `delivered 26.991 fps` over 199 frames,
`drawn 29.840 fps`, `startup_ms 637.7`, `worst_gap_ms 53.5`, `cpu_mean 17.81 %`,
`rss_mean 206722 kB`, `failure none`. `presented_fps` is `unavailable` because a
hidden window is given no surface to present into, so this is delivered and drawn
frames, not a claim about a visible screen.

---

## Preserved work (before any modification)

Three Codex worktrees contained uncommitted implementation. Nothing was deleted,
overwritten or moved; each was committed to a local archive branch so the tree
state is recoverable from Git as well as from the working directory.

| Worktree | Branch | Commit | Content |
| --- | --- | --- | --- |
| `.worktrees/full-app` | `archive/full-app-2026-10-25` | `15b3372` | Flutter app + packages + full docs, licensing, CI templates (149 files) |
| `.worktrees/source-check` | `archive/source-check-2026-10-25` | `fa6d145` | Flutter app + packages from the source/permission check pass (111 files) |
| `.worktrees/cache-controls` | `archive/cache-controls-2026-10-25` | `c1145aa` | Same plus artwork-cache work (113 files) |

Checks on those snapshots: no private Dropout tokens (`dropout_tokens.g.dart`),
no `vendor/dropout-design-system`, no `.env`/certificates/keystores and no user
media were committed. Build caches (`build/`, `.dart_tool/`, `.artifacts/`) were
excluded by the worktrees' own `.gitignore`. Largest committed blobs are the
pre-existing `reference/prototype-v0.1.zip` (573 KB) and a pre-existing test
fixture (127 KB).

**Deleted 2026-10-29:** the `archive/*` branches and `.worktrees/` were removed at the owner's request (local-only, never pushed); the Flutter source is no longer recoverable.

---

## M0 — Foundation and feasibility

**Status: host-verified.** Not device-verified.

| Action | Command | Result |
| --- | --- | --- |
| Workspace builds | `cargo check --workspace` | ok |
| Unit + integration tests | `cargo test --workspace` | 53 passed, 0 failed |
| Lints | `cargo clippy --workspace --all-targets` | no warnings from first-party code |
| Formatting | `cargo fmt --all --check` | clean |
| libmpv present | `cargo run -p screenroom-player --example capability` | `available: true`, `api version: 2.5`, `/opt/homebrew/lib/libmpv.2.dylib`, bridge `OpenGl` |
| libmpv decodes a real file | `cargo test -p screenroom-player --test libmpv_smoke -- --nocapture` | FILE_LOADED received, `duration` property returns a 2 s clip's duration |
| iced + wgpu shell compiles | `cargo check -p screenroom-desktop` | ok with iced 0.14.0 / iced_wgpu 0.14.0 / wgpu 27.0.1 |
| Video bridge compiles | `cargo check -p screenroom-desktop` | `video_surface.rs` type-checks: texture upload in `Primitive::prepare`, WGSL letterbox shader |

Toolchain observed: `rustc 1.99.1`, `cargo 1.99.1`, `ffmpeg/ffprobe 9.0.2`,
`node 24.20.0`, `python3 3.14.3`, `flutter 3.47.8` (only relevant to the archive;
removed from the host on 2026-10-26).

The clippy run prints one *dependency* warning: `block 0.1.6` will be rejected by
a future Rust version (transitive through the iced stack, not first-party code).

### What M0 does **not** prove

- No frame has been presented through wgpu from libmpv. The bridge compiles and
  the aspect logic is unit-tested; presentation on real hardware is M2.
- No GPU decode, 4K, HDR, audio output or subtitle rendering was exercised.
- Windows, Linux, Tizen, webOS and Android TV were not built.

---

## M1 — Shared core library

**Status: host-verified.** SQLite and filesystem behaviour tested on macOS only.

Implemented in `crates/screenroom-core`:

| Area | Public surface | Tests |
| --- | --- | --- |
| Sources | `add_source`, `sources`, `source`, `rename_source`, `relink_source`, `remove_source`, `set_source_state` | overlap rejection, invalid/relative roots, disconnect keeps history, re-grant restores identity |
| Profiles | `profiles`, `add_profile`, `rename_profile`, `remove_profile` | last-profile guard |
| Scanning | `LibraryScanner::scan`, `ScanToken` | media discovery, hidden dirs, sidecar/young files, cancellation, complete-rescan availability withdrawal, symlink refusal, single-scan lock, streaming updates |
| Ingestion | `ingest_file` (+`Ingest`) | idempotent rescan, replaced-file edition, small-file rename identity, unsafe locator rejection |
| Library | `items`, `count`, `item`, `continue_watching`, `seasons_for_work`, `known_genres`, `next_episode` | paging bounds, literal search escaping, work grouping, season filter/sort, gap-safe next episode |
| Playback | `start_session`, `begin_seek`, `checkpoint`, `end_session` | monotonic sequence, stale session, seek generation, impossible values, ended semantics |
| Lists | `set_favorite`, `set_watchlisted`, `mark_watched`, `hide_continue` | lists survive disconnect, Continue Watching ordering |
| Preferences | `preferences`, `save_preferences` | defaults, survive a reopen, BCP-47 validation rejects an injected tag and saves nothing, unknown keys preserved |
| Backup | `export_backup`, `inspect_backup`, `import_backup` | every table round-trips into a fresh library, the restore stamp is stripped, the previous library is kept beside the new one, refuses to overwrite the live database, refuses a non-Screenroom SQLite file and a newer format, a failed backup leaves no partial file |
| Schema | `open`, `open_in_memory` | future schema fails closed without data loss; integrity check; crash normalisation |
| Recovery | reopening after a crash | an open session is closed, a running job is requeued, progress survives the whole recovery sequence, a damaged database is reported rather than replaced |

Test totals: 38 unit (`filename`, `fingerprint`, `artwork`, `markers`,
`recommend`, library migration), 27 store (`tests/library.rs`), 12 scanner
(`tests/scanner.rs`), 19 storage (`tests/storage.rs`), 7 recovery
(`tests/recovery.rs`), 2 scale (`tests/scale.rs`).

### 10,000-item budget run (2026-10-28)

```bash
cargo run --release -p screenroom-core --example library_bench
cargo test -p screenroom-core --test scale
```

Generated 10,000-entry library, release build, page size 24:

| Operation | Time |
| --- | --- |
| Ingest 10,000 entries | 825 ms (12,118 entries/s) |
| First page | 18.4 ms |
| **Last page** (worst case for an OFFSET scan) | **43.5 ms** |
| Added ordering | 34.4 ms |
| Year ordering | 16.6 ms |
| Progress ordering | 15.5 ms |
| Literal search (1 hit / no hit) | 16.4 ms / 16.6 ms |
| `count` | 14–18 ms |

The slowest paged query is 43.5 ms, well inside a frame at 30 fps for a UI that
pages rather than scans. This is the M3 budget run for the store layer; it measures
queries, not rendering.

#### Repeat run and scaling (2026-10-28, later the same day)

Re-run on the same machine to check the numbers above were not a single lucky
sample, and extended to larger libraries to find where it stops feeling instant:

| Library | First page | Last page | `count` | Literal search |
| --- | --- | --- | --- | --- |
| 100 | 0.37 ms | 0.36 ms | 0.11 ms | 0.21 ms |
| 1,000 | 0.75 ms | 1.38 ms | 0.44 ms | 0.57 ms |
| **10,000** | **6.4 ms** | **15.3 ms** | **5.3 ms** | **6.1 ms** |
| 50,000 | 48.3 ms | 146.1 ms | 44.7 ms | 71.5 ms |

Two things this settles, and one it does not:

- The cost is genuinely **proportional to library size**, not a fixed overhead: it
  rises ~17× from 1,000 to 10,000 and ~8× again from 10,000 to 50,000 entries. An
  earlier reading of an identical ~10 ms across every query suggested a constant
  setup cost; measuring across sizes showed that was sampling noise, not the shape.
- **10,000 entries is comfortably inside a frame.** A page plus its count — what
  the Browse screen runs per keystroke — is ~11.7 ms against 16.7 ms at 60 fps.
- **50,000 is not**, at ~93 ms together, so a very large personal library would need
  keyset paging instead of `OFFSET`. That is recorded as a risk rather than fixed
  ahead of a library that size existing.

Run-to-run variance is real and is not hidden: the first ledger run measured
18.4 ms / 43.5 ms for first/last page where this one measured 6.4 ms / 15.3 ms on
the same machine and input. Both are recorded; neither supersedes the other. Any
future comparison should re-run `library_bench` rather than quote a memory of it.

#### The catalogue facet stall, and schema 4 (2026-10-29)

The paging numbers above were never the whole story: opening Browse on a 10,000-entry
library was unusable, and it was not `items` or `count` that did it. `known_genres`
runs on every refresh, before the screen draws a card, and its `EXISTS` subquery
joined `editions` on `work_id` — a column with no index. Measured with
the same generated library, release build:

| `known_genres` on 10,000 entries | Time |
| --- | --- |
| Before the indexes (schema 3) | **44.2 s** |
| After the indexes (schema 4) | **13.7 ms** |

That is the class of failure the loose bounds in `tests/scale.rs` exist to catch, and
it was found by using the app on a large library rather than by the benchmark: the
bench did not ask the catalogue for its facets at all. It does now — `library_bench`
times `known_genres` and `continue_watching` next to paging, which is what put the
44-second figure in the table above instead of a memory of it.

Schema 4 is that fix: indexes on the join columns every catalogue query walks
(`editions_work`, `files_edition`, `file_episodes_episode`, `markers_edition`). It is
additive like schema 3 — no table or row is rewritten — so a schema 2 or 3 library is
migrated in place, in one transaction, and a schema 2 library takes both steps at
once. Both paths are exercised: `tests/library.rs` opens a schema 2 library, and a
unit test in `library/mod.rs` opens a schema 3 library and asserts the four indexes
exist and the profile survives. The ingest cost of maintaining them is real but
small: 10,000 entries in 394 ms with the indexes against 440 ms without in one
comparable pair of runs.

### The real library on a USB SSD (2026-10-29)

Everything above was measured on generated fixtures. This is the owner's actual
movie and TV library on an exFAT USB drive, read-only, scanned into a **separate
test database** (`SCREENROOM_LIBRARY`), with the real library's checksum taken before
and after and unchanged. The drive was never written to: no thumbnails, databases,
logs or temporary files, and no mount, permission or repair operation.

**Inventory, built from the mounted filesystem rather than from the earlier tree
snapshot:** 830 media files (729 `.mkv`, 101 `.mp4`), 941.1 GiB, mean 1,161 MiB;
19 show folders, 291 films; 539 files with an `SxxEyy` name; 21 sidecar `.srt`; two
filenames with non-ASCII characters, 20 with apostrophes, 3 with colons; two files at
depth 6 inside a `Duology` folder. `AppleDouble` `._*` companions, `Auto Data Backup`
and the hidden directories were excluded from the inventory by rule.

**Scan (release build, metadata only — no media content read):**

| Run | Visited | Imported | Issues | Wall | Peak RSS |
| --- | --- | --- | --- | --- | --- |
| First scan | 1,002 | 830 | 0 | 3.59 s | 8.2 MB |
| Rescan of the same library | 1,002 | 830 | 0 | 2.31 s | — |

Resulting catalogue: 830 files, 291 films, 18 series, 53 seasons, 539 episodes,
539 file↔episode links, **zero** files whose stored season/episode disagrees with
its episode row, and 830 editions after the rescan (no duplicates). Every episode
count per show matches an independent count of the folder: 149/149 The Mentalist,
72/72 Suits, 40/40 From, 30/30 Jack Ryan, 26/26 What If, 26/26 House of the Dragon,
and so on for all 18.

`scripts/probe-library.sh` then read every file's container, codecs and streams with
ffprobe (bounded to four concurrent reads, output outside the media root):

| | Count |
| --- | --- |
| Probed | 830 |
| Readable | 827 |
| **Unreadable (truncated MP4, `moov atom not found`)** | **3** |
| Video — HEVC Main 10 / HEVC Main | 396 / 251 |
| Video — H.264 High / Main / High 10 | 119 / 16 / 1 |
| Video — AV1 Main 10 / MPEG-4 Simple | 43 / 1 |
| Files with embedded subtitle streams | 646 (3,785 SubRip, 375 PGS, 210 ASS, 164 DVD, 9 mov_text) |
| Audio tracks per file — 1 / 2 / 3 / 4 | 659 / 160 / 7 / 1 |
| Audio codecs | AAC, AC-3, E-AC-3, Opus, DTS, Vorbis, MP3 |
| Anamorphic (SAR ≠ 1:1) / ≥ 3840 px wide | 8 / 8 |
| Files with embedded cover art | 115 (68 JPEG, 47 PNG) |

The three unreadable files are the Cloverfield trilogy. They are catalogued (the
scanner sees a file and records it) and the player reports the failure honestly:
`decode_failed — unrecognized file format`. They are not silently skipped, and the
scan is not treated as incomplete because of them.

**Playback, offscreen, through the real worker** (`examples/play_file.rs`, muted,
four-second windows):

| File | Codec / container | First frame | Delivery | Audio clock | Seek |
| --- | --- | --- | --- | --- | --- |
| Superman (2025) | HEVC 10-bit, PGS + 7 SubRip | 4.5 s | 23.7 fps, worst gap 51 ms | 4.01 s / 4.00 s | 88 ms |
| Avengers Infinity War | HEVC 4K → 1920×800 target | 2.1 s | 24.0 fps | 3.96 s / 4.00 s | 116 ms |
| Suits S07E02 | AV1 10-bit, Opus 5.1 | 0.4 s | 24.0 fps | 3.86 s / 4.00 s | 90 ms |
| Tulsa King S01E01 | HEVC, external `.srt` picked up | 1.5 s | 24.0 fps | 4.00 s / 4.00 s | 88 ms |
| The Shining | H.264 High 10, DTS 5.1 | 3.6 s | 24.0 fps | 3.00 s / 3.00 s | — |
| Predators | H.264, 4 audio tracks | 0.2 s | 24.3 fps | 3.00 s / 3.00 s | — |
| The Hitman's Bodyguard | MPEG-4 Simple, 1280×536 | 1.4 s | 24.0 fps | 3.02 s / 3.00 s | — |
| Terrifier (2016) | H.264 High, AC-3 | 2.2 s | 23.7 fps | 4.00 s / 4.00 s | 76 ms |
| Cloverfield (2008) | broken MP4 | **failed** — `decode_failed` | — | — | — |

Audio track switching (to the last of four) and subtitle `sub-scale`/`sub-pos`
read-back were confirmed on every file that has them, and external sidecar SRTs are
loaded by libmpv itself and appear as an `external=true` subtitle track. Anamorphic
content arrives letterboxed inside the frame (Tulsa King: 1920×1080 with 60 px bars),
not stretched.

**Posters.** The hidden-window application ran against the test database and wrote
21 JPEG posters (400 px wide, 180 KB total) for the works on screen, from the real
files through ffmpeg. Sampled by hand, one is a real frame rather than a card.

#### Defects found by the real library, and fixed

| Symptom | Cause | Fix |
| --- | --- | --- |
| 21 extra entries with titles like `._12 Angry Men - 1957`, and 3 phantom episodes | macOS AppleDouble companions keep the media extension; the walk skipped hidden **directories** but not hidden **files** | skip dot-files in the walk, silently (`scanner.rs`); regression test `apple_double_companions_are_not_media` |
| All 18 series had a null year | the year only came from the episode filename, and episode names never carry it — the show folder does | an episodic file with no year of its own takes the show folder's (`filename.rs`); a film in a collection folder does not, because `Duology (2009-2011)` is a range, not a release year. Test: two `The Office` shows at different years stay two works |
| A cover-art JPEG appeared as a selectable **video** track | `track-list` reports embedded artwork as video; it was listed like a second angle | skip video tracks with `image=yes` (`worker.rs`); `Predators` now lists one video track, not two |
| Only the first 16 posters of a 24-card page were ever extracted; the rest stayed blank until a key was pressed | the queue is 16 deep and nothing refilled it when an extraction finished | refill the freed slot on the next poll, for works on screen only — not a library-wide marathon (`app.rs`) |
| `LibraryItem.poster_path` was documented as "app-owned poster path, if artwork bytes were retained" and was `None` on all 830 rows **even for the 21 works whose posters had been stored** | the field read a legacy `works.poster_path` column that nothing writes, because posters are stored as bytes and read with `artwork()`/`artworks_for()`. A client trusting it would have drawn no posters at all | the field is removed from `LibraryItem` and the projection (`domain.rs`, `catalog.rs`), since its only reader was the illusion; the column stays for schema compatibility. Nothing in the app, the TV contract or the tests referenced it |

Measured after the fixes: 830 imported (was 851), 18 series with years (was 0), one
video track on a file with cover art (was two), 21 posters for the visible works
(was 16).

#### The catalogue and the posters, checked on the real library (2026-10-29)

Two examples run by hand against a copy of the scanned library, because the
questions only exist on a library built from real files:

```bash
cargo run --release -p screenroom-core --example library_check -- <copy.db>
cargo run --release -p screenroom-core --example artwork_check -- <copy.db> [fresh]
```

`library_check` — **27 passed, 0 failed, 1 skipped**:

- paging walks all **830 rows exactly once** in two 500-row pages, none repeated;
- a 100 000-row page is refused (`invalid_page`) rather than honoured or truncated;
- all four orderings return a full page, and Title is genuinely ascending across 200
  compared rows;
- search finds an existing title (1 row), a word inside one (30 rows), treats `%`
  as a literal, and finds nothing for nonsense;
- the kind partition is exact: **291 films + 539 episodes + 0 unknown = 830**; an
  unknown kind is refused (`invalid_filter`);
- seasons are read from a real series and an episode page contains only that
  season's episodes (4 in season 1);
- a checkpoint becomes a Continue Watching row; after a seek the **pre-seek
generation is refused** and the post-seek one is accepted;
- favourite and watchlist filters see their own profile's flags and clear again;
- suggestions are explained and deduplicated (2 suggestions, 2 distinct works, both
  with a reason);
- preferences (subtitle scale, autoplay, volume) survive a close and reopen;
- profiles add, rename and remove;
- a backup of the real catalogue is **1 490 944 bytes, 830 files, 309 works**, restores
  into a fresh library at **830 of 830 rows**, and a restore over existing data writes
  a safety copy.

Skipped, honestly: the genre filter, because a scanned library stores no genres until
enrichment exists. It is covered by unit tests on synthetic labels, which is not the
same claim.

`artwork_check` — **all checks passed**:

- all **21 stored posters** were written back out and decoded by ffprobe as JPEG at
  the width and height the row claims (**180 731 bytes** total, 0 rejected);
- **200 of the 288** works still without a poster offer a candidate, and **every one**
  points at a file that exists and is readable — so the queue never spins on a work
  that can never be drawn;
- `fresh` extracted, stored and re-decoded **5 posters from real films in ~1.2 s each**,
  and classified the one refusal correctly: one file is a **truncated MP4**
  (`moov atom not found`), which the app records as a failed work and stops asking,
  rather than a pipeline failure. (Titles are deliberately not repeated here; the raw
  logs hold them and stay local.)

Also unverified on the real catalogue, and why: **durations** (none stored, see below,
so every candidate reports `duration_ms = 0` and the sampler seeks blind), and the
**115 embedded cover images** that would beat a film frame as a poster.

#### macOS features exercised on isolated fixtures (2026-10-29)

Three application-level journeys were added to `apps/desktop/src/app.rs`, each on a
temporary folder the test owns — no window, no focus, no drive:

| Test | What it drives, and what it asserts |
| --- | --- |
| `a_first_run_scan_builds_the_library_and_writes_nothing_to_the_media_folder` | an empty library → grant a folder through `Message::AddFolder` → the real scan thread finishes → the film appears in Browse with title, year and kind read from the filename. Then the folder's own listing (names, sizes, modification times, recursively) must be **identical** to what it was before the scan |
| `a_folder_that_moved_keeps_its_history_until_it_is_relinked` | scan a folder, store a resume point at 240 s, then `rename` the folder to a new name — the removable-drive case. A rescan keeps the row and the position while marking it unavailable; `RelinkTargetChosen` → `RelinkPathChanged` → `RelinkSource` makes it available again **at the same row**, with the position intact and no duplicate |
| `a_restore_from_a_damaged_backup_changes_nothing` | a file that is not a backup is offered twice (arm, then apply): the error names the backup, and the catalogue, the profiles and the 120 s resume point are unchanged |

Also already covered by the existing application tests, so not repeated: profile
switching and the last-profile rule, preference editing with a refused language tag,
folder removal asking first and keeping history, backup → inspect → restore through
the Settings screen, quitting with a file open storing the final position, keyboard
focus, paging and search.

The **settle window** is worth knowing about here: the scanner ignores a file modified
within 2 s of the walk, because it may still be copying. A test that writes a fixture
and scans at once imports nothing, which is why both journey tests backdate their
files an hour. That is real behaviour, not a test artefact.

#### What the real library still does not do

Updated 2026-10-29: durations (827 of 830), embedded cover art, anamorphic sizing
and copy-back hardware decoding were closed by the macOS audit below. What remains:

- **A 3240×2160 HEVC film is not real-time**: about 19 fps of 23.976 with copy-back
  decode under mixed load. The limit is the CPU RGBA conversion and upload, so it
  needs a GPU render bridge, not more decode tuning.
- **Genres are hidden**: a scanned library stores none until enrichment exists.
- **On-screen behaviour is unverified.** Every number above is offscreen: no window,
  no GPU surface, nothing heard. Resume-after-quit, full-screen, the D-pad-free
  keyboard journey and the on-screen frame budget still need a permissioned session.

Design choice worth keeping: the Rust schema is byte-compatible with the layout the
Flutter candidate wrote (`user_version = 2`), so an existing on-device library opens
and is migrated forward to the current version instead of being reset.

### Deliberately not ported yet

These existed in the Dart candidate (source deleted 2026-10-29) and must be
reimplemented or explicitly dropped before M3/M6 claims them:

| Dart API | Reason not ported now | Target |
| --- | --- | --- |
| `updateMetadata`, `updateEpisodeMetadata`, `clearMatch`, `setExternalIdentity`, `metadataRevision` | belongs with enrichment (M6), needs the provider client | M6 |
| ~~`saveMarker`, `markers`, `removeMarker`~~ | **ported 2026-10-29** as `set_marker`/`mark_segment`/`markers`/`remove_marker`; intervals are validated against the edition's stored duration | done |
| `relinkCandidates`, `confirmFileRelink` | needs the relink UI flow | M3 |
| `exportBackup`, `importBackup` | validated versioned JSON format; port alongside the settings UI | M3 |
| `queueJob`, `jobs`, `updateJob` | store exists in schema; scheduling belongs with enrichment | M6 |
| Local NFO parsing | bounded parser; port with metadata ingestion | M6 |
| ~~`recommendations`~~ | **ported 2026-10-29** as `Library::recommendations`, explainable over owned media only | done |
| `authorizedPath` | platform adapter concern | M2 |

---

## M2 — Desktop playback slice

**Status: host-verified on macOS, including on-screen presentation.**
Everything below ran on this macOS host against libmpv 0.41 / client API 2.5.

| Area | What exists | Verification |
| --- | --- | --- |
| Player worker (`screenroom-player/src/worker.rs`) | One thread owns libmpv; commands in, immutable `PlayerSnapshot` out; lifecycle mapped onto `PlayerState` | `tests/worker.rs` — 10 tests |
| Software render bridge (`src/render.rs`) | mpv `sw` renderer straight into a pooled `Frame` (no second copy), 256-byte aligned stride and a 64-byte aligned pointer recorded as the frame's offset, native size up to 3840 px | real frames asserted non-blank with colour; `target_size`/`row_stride`/`pointer_offset` unit tests; frame-rate assertion |
| Seek / pause / resume-at-load | `seek`, `set_paused`, `load_at(path, Some(seconds))` | position moves after seek, holds while paused, resumes after load |
| Audio and A/V sync | `audio-pts`, mpv's own `avsync` and `total-avsync-change` in `PlayerSnapshot` | source volume measured, the audio clock advances in real time (0.982× wall), libmpv `avsync` 0.0 ms with a 12 ms cumulative adjustment |
| Tracks | track list with language/title/selected read from libmpv; `set_audio_track`, `set_subtitle_track` | two audio and two subtitle tracks enumerated and switched; selection read back rather than assumed |
| Failure states | `file_missing`, `load_failed`, `decode_failed`, `render_failed`, `seek_failed`, `pause_failed` with libmpv's own message | missing file test; malformed file reported rather than shown as an empty player; end-file error path mapped |
| Durable resume (`apps/desktop/src/playback.rs`) | Session open → throttled checkpoints → seek generations → close; finished media resumes from the start | `tests/playback.rs` — 6 tests, including one that decodes a real clip, checkpoints, reopens the SQLite library and gets the same position back |
| Player screen (`apps/desktop/src/app.rs`) | Library list with progress, Back/Play-Pause/scrub/commit, resume label, failure text, Escape and Space keys, audio/subtitle pickers, next-episode autoplay | `clock` unit tests; open/refuse-to-open tests; window observation (below) |

Totals after M2: **106 workspace tests, 0 failures**; `clippy --workspace --all-targets
-- -D warnings` clean; `fmt --all --check` clean.
### Audio and sync measured (2026-10-28)

Hearing a file was never going to be repeatable, so the player now reads libmpv's
own instrumentation and a test asserts on it. Measured on this host, generated
fixture with a known audio track:

| Measurement | Observed |
| --- | --- |
| Source has real audio | `volumedetect` mean −24.1 dB |
| Audio clock over a 9 s window | 8.87 s of audio against 9.13 s of video, **0.982× wall clock** |
| libmpv `avsync` | **0.0 ms**, cumulative adjustment 12 ms |
| Mistimed frames | none reported |
| Audio output buffer vs video playhead | −262 ms, moving −267 ms across the window |

That last row is the one worth remembering: `audio-pts` is a **buffer position,
not a lip-sync indicator**. It drifts to −262 ms on a file that libmpv reports as
perfectly in sync, so asserting on it would have been a permanent false failure.
The sync assertion uses `avsync`; the buffer gap is printed as an observation only.

This does not close gate G3b on its own: nobody has yet listened to output through
speakers. It does turn "audio probably works" into a measured 0.982× real-time audio
clock and a 0.0 ms sync error, which is the evidence a listening pass would confirm.

### On-screen presentation measured (2026-10-27)

```bash
cargo run -p screenroom-desktop          # window, signed-in desktops only, no browser
DB=~/Library/Application\ Support/screenroom/library.db
```

Observed, in the app's own window on this host, with the rights-cleared generated
fixture (not owner media):

| Observation | Evidence |
| --- | --- |
| A real decoded frame drawn through wgpu in the iced window | window-ID capture `34-playing-after-fix.png`), `.artifacts/evidence/` (Git-ignored) |
| Correct aspect: source pillarboxed inside a 3440×1440 window, no stretch, no colour inversion | same capture |
| Transport: play, pause, ±10 s seek, and end-of-file | `worker.rs` integration test; captures 24–30 |
| Resume across quit and relaunch: closed at 0:20, reopened playing at 0:26 with the burn-in timecode agreeing | capture `34-playing-after-fix.png`; `playback.rs` test decodes, checkpoints, reopens SQLite and reads the same position |
| **30.0 fps published, zero dropped frames, playhead 1.00×** at 1080p in a 3440×1440 window | `cargo test -p screenroom-player --test worker -- --nocapture` frame-rate assertion |
| Shipping configuration (`--release`) also measures **120 frames in 4.00 s = 30.0 fps at 1920×1080, playhead 1.00×** | `cargo test --release -p screenroom-player --test worker -- --nocapture` |
| Debug-build cost: idle ~3–5 % CPU, RSS plateau ~117 MB after playback ended (no leak in the sample); during software-bridge playback ~1 core / ~275 MB at 1080p | sampled with `ps`/`top` on a background run |
| Release-build cost at 1080p: playback steady at **~70 % of one core, RSS 254–255 MB flat across 12 s of samples** (no growth); after the clip ended, CPU fell to **~3 %** and RSS settled to **108 MB** | `ps -o %cpu=,rss=` against the release process |

### The frame-rate defect found and fixed here

Presentation initially measured **11.5 fps** against a 30 fps source, in a realtime
playhead (so frames were *dropped*, not merely slow). Instrumentation split the
cost: libmpv's `mpv_render_context_render` ≈ **3 ms**, the first-party `pack()` step
≈ **40 ms**. `pack()` copied pixel-by-pixel through a per-pixel `push`/alpha write in
the frame packer. Replacing the pixel loop with a chunked copy-and-stamp brought the
bridge to **30.0 fps with zero drops**. A separate speculative change to the worker's
wait slice was tried, measured to make no difference, and reverted — the fix is only
the pack path. The alpha channel is written as opaque during the pack rather than
assumed by the presentation shader.

### What M2 has **not** proven

- **Audible output** has not been *heard*: the audio clock, volume and `avsync` are
  now measured (above), which is instrument evidence rather than a listening test.
- **Audio/video sync** has no measured tolerance (gate G3b).
- **Offline behaviour** was proven by holding zero network sockets during playback,
  not by disabling this machine's networking.
- **4K / hardware decode**: measured on 2026-10-29 and corrected on 2026-11-01
  (`HWDEC_EVIDENCE.md`). macOS decodes with copy-back (`auto-copy`); the bridge now
  delivers **native 4K** and paced offscreen delivery is 23.7–23.8 fps of 24. What
  is still unproven is **on-screen** 4K presentation and HDR, so smoothness in the
  window is not claimed.
- **Windows**: nothing here ran on Windows, including the D3D11 path (unbuilt) and
  the case-insensitive path folding. `WINDOWS_VALIDATION.md` is the handoff that
  turns each of these into a runnable check.

### Release-build measurement (2026-10-27)

```bash
cargo build --release -p screenroom-desktop
./target/release/screenroom-desktop      # same library, same fixture
cargo test --release -p screenroom-player --test worker -- --nocapture
```

| Metric | Release |
| --- | --- |
| Frame rate (software bridge, 1920×1080) | **30.0 fps, playhead 1.00×** |
| CPU during 1080p playback | ~66–76 % of one core |
| RSS during playback | 254–255 MB, flat across 12 s of samples |
| CPU after playback ended | ~3 % |
| RSS after playback ended | 108 MB |

The release figure is the one that ships. The software bridge is one full-frame CPU
write per frame (decode is copy-back on macOS since 2026-10-29), and since 2026-11-01
the second copy into the frame pool is gone: libmpv draws into the pooled buffer
directly, with the 64-byte pointer alignment `render.h` asks for recorded on the frame.
Measured on 2026-11-01, that copy was **not** what limits 4K: native 3840×2160 delivery
costs 5.97 ms p50 for HEVC Main10, while `mpv_render_context_render` itself costs
**41.0–41.2 ms** at that size — the same with `hwdec=no` and `auto-copy`. mpv documents
that renderer as single-threaded CPU colour conversion and accepts only single-plane
RGB as an alternative output format, so the conversion cannot be narrowed here. What is
left is a GPU render API, which libmpv offers on macOS only as OpenGL (no Metal) and
would need an IOSurface bridge into iced's wgpu surface, and HDR tone mapping, which
this path does not do. The same change stopped redrawing when no new picture exists:
draws fell from 51.5/s to 13.7–14.1/s at 4K and to 29.9/s for 28.7 delivered at 1080p.

Two libmpv contract bugs were found and fixed while proving the above, and are
worth remembering: `MPV_RENDER_PARAM_SW_POINTER` takes the pixel buffer address
itself, and the render context must be freed before the mpv core or libmpv aborts
the process.

---

## M3 — Desktop core UI and design-system mapping

**Status: the essential desktop surface is delivered and exercised on macOS; a few
M3 items are still absent, and they are named below rather than implied.**

Delivered and observable in the running app:

| Area | What exists |
| --- | --- |
| Home | Continue Watching first, then recent items; folder granting; scan status |
| Library | Paged grid (24/page) with search, kind and list filters, four sorts and paging |
| Detail | Work/season structure, editions, progress, resume/restart, watched, favourite, watchlist |
| Player | Play/pause, ±10 s seek, scrub, resume label, **audio and subtitle pickers** (with a real “Off” for subtitles), failure copy |
| Next episode | Autoplay driven by the stored preference and the core's gap-safe ordering; a gap stops the chain instead of skipping it; an unreadable next episode is refused with its reason |
| Keyboard | Arrows, Enter, Esc, ⌘1/⌘2/⌘3 (Home/Browse/Settings), f/w/m, Space, ←/→ seek in the player |
| Settings | Viewer profiles with per-profile progress, lists and Continue Watching; autoplay and language preferences; folders with availability, distinct recovery copy, rescan, relink and remove; backup and restore |
| Design | Dropout adapter with the private, commit-pinned, Git-ignored token export (`apps/desktop/examples/export_ds_tokens.rs`); the running app names the pin that produced its theme |

Still absent from M3: previews, and the **on-screen** rendering/input budget. The
store half is done (see M1) and the offscreen half is measured below; what remains
is the iced window itself, which needs a visible window and is therefore pending.

### Subtitle styling delivered (2026-10-28)

Size and vertical position are now preferences, applied as libmpv's own `sub-scale`
and `sub-pos`. Both are stored as whole percentages and offered as sliders in
Settings (50–300% size, 0–100% position, where 100% is the bottom of the picture).

The player **reads both properties back** after setting them rather than echoing the
request, so `PlayerSnapshot::subtitle_scale`/`subtitle_position` are evidence of what
libmpv accepted. Verified against a real clip:

| Check | Result |
| --- | --- |
| Style reaches libmpv (1.75×, 88%) | read back as requested, within 1e-2 |
| Style survives a file change | unchanged across `load` |
| Impossible style (−5.0, 9000) | clamped to `SUB_SCALE_RANGE.0` / `SUB_POSITION_RANGE.1` |
| Non-finite style (`NaN`, `inf`) | skipped; last good value stands; player not failed |
| Stored value outside range | refused on write (`subtitle_scale_out_of_range`) |
| Unusable stored value | clamped on read; the library still opens |

Measured along the way: libmpv holds `sub-scale` in **single precision**, so 0.1
reads back as `0.10000000149`. That is why the interface shows a whole percentage and
why the assertions are tolerances rather than equalities.

Honest limitation, carried into the UI copy: a subtitle script that positions its own
lines — a styled ASS release — keeps that positioning, because Screenroom leaves
`sub-ass-override` alone. Size still applies; placement may not.

### Offscreen render and command latency measured (2026-10-28)

A window was previously the only way to answer “can this machine keep a 1080p file
moving”. `crates/screenroom-player/examples/render_bench.rs` answers it with no
window at all: it builds its own moving fixture with FFmpeg, drives the real decode →
libmpv render → CPU frame path, and counts **distinct** frames by libmpv's monotonic
`generation` so a repeated snapshot cannot inflate the rate.

```bash
cargo run --release -p screenroom-player --example render_bench
cargo run --release -p screenroom-player --example render_bench -- 1920x1080 12
```

MacBook Air M4, release build, libmpv 2.5 (OpenGL bridge, software frame target):

| Source | Time to first frame | Sustained delivery | Worst frame gap |
| --- | --- | --- | --- |
| 1280×720 | 283 ms | 24.0 fps of a 24 fps source | 47.7 ms |
| **1920×1080** | **95–278 ms** | **24.0–24.1 fps of a 24 fps source** | **47.9–54.0 ms** |
| 3840×2160 | 132 ms | 24.1 fps | 53.8 ms |

| Command | Round trip to the playhead arriving |
| --- | --- |
| 1080p seek to 2.0 s / 5.0 s / 1.0 s | 72.7 ms / 60.0 ms / 58.7 ms |
| 4K-source seek (see caveat) | 62.3 ms / 111.7 ms / 205.6 ms |

`avsync` held at **+0.0000 s** across every run, and the playhead stayed at 1.00×
wall clock (9.04 s of playhead in 9.00 s of wall).

Three things this does **not** prove, stated because the numbers look better than
they are:

- **It is not an on-screen frame rate.** iced, the wgpu upload and the compositor
  add cost this measurement cannot see. These are a ceiling on what the display can
  reach, not a substitute for measuring the window. The on-screen budget at 10,000
  items is recorded as **PENDING** in `docs/TEST_PLAN.md`.
- **The 4K row above predates native delivery.** It was recorded while
  `render::MAX_DIMENSION` was 1920, so that run decoded 4K but rendered
  1920×1080. `MAX_DIMENSION` is now 3840 and the same command reports
  `frame size: 3840x2160`; the 2026-11-01 table further down carries those numbers.
  **On-screen** 4K presentation is still unmeasured and stays behind gate G2.
- **Time to first frame has a wide spread** (95 ms to 278 ms across runs on the same
  input). It is the least stable number here and should not be quoted to three
  significant figures.

The seek figures are also worth reading carefully: an earlier version of this example
reported **sub-microsecond** seek latency, which was a broken measurement — the check
was “playhead is past the target”, and a playhead already 11 s in satisfied it before
the seek took effect. The condition is now “arrived at the target *and* moved”, which
a backwards seek cannot satisfy early. The first number was wrong and is recorded here
so the correction is visible rather than silently replaced.

Two behaviours were corrected while exercising the finished surface, both from
watching the real app rather than the tests:

- The subtitle picker was inert when a file had no subtitle tracks. It now says
  “none in this file” and is not offered as a control that does nothing.
- “resumed from 0:33” stayed on screen next to “Ended”. Resume copy is now only
  shown while there is something to resume.

### Closing the window now ends the viewing (2026-10-28)

A quit used to leave the session row `active` and lose up to a second of progress.
The cause was that neither ⌘Q nor the window close box reached the update loop at
all — the app exited underneath it. iced's `window::close_requests()` subscription
is the hook that fires before teardown, and it now runs the same graceful close as
Back: final checkpoint, then `end_session`.

Measured in the packaged app against the real database:

| Exit path | Final progress | Session |
| --- | --- | --- |
| Window close box | **52.0 s, exact** | `active = 0` |
| ⌘Q (**after** routing, below) | **as the close box: both paths identical** | `active = 0` |

⌘Q initially lost under a second and left the row `active`: the Quit item AppKit
installs sends `terminate:`, which ends the process *below* iced, so no close request
is ever delivered and nobody finalises the viewing. The crash-recovery normalisation
from M1 covered it — the next launch was clean rather than stuck — but "recovered on
the next launch" is not "stored".

`apps/desktop/src/quit.rs` now finds the application menu's Quit item **by selector,
never by title** (the title is localised and carries the app's name), and re-points it
at `performClose:` — the exact action the close button performs. The menu item and the
close button are therefore one operation and cannot disagree about whether the open
file was stored. A menu whose Quit item cannot be identified is left untouched, because
a half-rewritten menu bar is worse than the behaviour it replaces.

The first attempt at this reported `Routed` and still exited via `terminate:`: the
guard treated the initial `Unsupported` value as "already graceful", so routing never
ran. Both the routing and the two dismissal paths were then re-verified, and
`scripts/verify-quit.sh` keeps it reproducible.

**Verification status of this one, stated plainly:** the two dismissal paths were
confirmed with a visible window and real key presses, which is now gated behind
`SCREENROOM_ALLOW_FOREGROUND=1` (2026-10-28 policy). The evidence stands as recorded,
but it has **not** been re-run under the background-first regime. `scripts/check-quit-routing.sh`
re-checks the routing half headlessly, with a hidden window, on every run.

## macOS application bundle (2026-10-28)

```bash
scripts/package-macos.sh                  # -> dist/Screenroom.app
scripts/package-macos.sh /Applications     # install into a folder as well
```

The bundle carries its own copy of libmpv in `Contents/Frameworks`, and the player
looks there after the system paths (`packaged_candidates` in
`crates/screenroom-player`), so an installed app keeps working when the Homebrew
prefix changes or a development copy is removed.

| Observation | Result |
| --- | --- |
| Bundle assembles, `Info.plist` valid, Gatekeeper quarantine cleared for a locally built copy | yes |
| Runs **outside cargo**, from `dist/Screenroom.app` | yes — library, playback, Settings all exercised |
| libmpv loaded from the bundle, client API 2.5 | yes |
| Private DS pin in the bundled theme | `dropout-design-system@c000425 semantic-dark` |
| Progress written while playing | yes, 10.4 s → 16.8 s in the live database |

**What the bundle does not do:** libmpv's own dependencies (FFmpeg, libass,
libplacebo) still resolve by absolute Homebrew install names, so the app runs on a
machine with the same `mpv` installed. Vendoring FFmpeg is a licensing decision
(FFmpeg's build options decide which codecs may be redistributed) and is not
implied by a personal install; the script reports the dependency list instead of
hiding it. There is **no signing and no notarization**, which is correct for a
personally built app and is recorded as a blocked gate for distribution.

#### Runtime dependencies, named (2026-10-28)

Rebuilt and re-verified headlessly with `scripts/verify-bundle.sh`, which takes no
focus and opens nothing. The bundle is 17 MB and the binary is arm64.

The bundled `libmpv.2.dylib` (4.3 MB, a **real file** — `cp -L` follows Homebrew's
symlink so an in-place upgrade cannot dangle it) declares **20 absolute
dependencies**. All 20 must exist on the machine that runs the app:

| Dependency | Homebrew formula |
| --- | --- |
| `libmpv.2.dylib` | `mpv` |
| `libavcodec.63`, `libavfilter.12`, `libavformat.63`, `libavutil.61`, `libswresample.7`, `libswscale.10`, `libavdevice.63` | `ffmpeg` |
| `libass.9` | `libass` |
| `libplacebo.387` | `libplacebo` |
| `libmujs` | `mujs` |
| `liblcms2.2` | `little-cms2` |
| `libarchive.13` | `libarchive` |
| `libbluray.4` | `libbluray` |
| `libluajit-5.1.2` | `luajit` |
| `librubberband.3` | `rubberband` |
| `libuchardet.0` | `uchardet` |
| `libzimg.2` | `zimg` |
| `libjpeg.8` | `jpeg-turbo` |
| `libvulkan.1` | `vulkan-loader` |

All 20 resolved on this host. `brew install mpv` pulls the whole set, which is why
the packaging script can check them and report before anything is installed.

`scripts/verify-bundle.sh` checks, and each one can actually fail:

| Check | Result |
| --- | --- |
| Structure: `Info.plist` and an executable `Contents/MacOS/Screenroom` | pass |
| `Info.plist` parses; `CFBundleExecutable` and `CFBundleIdentifier` are set | pass (`com.screenroom.desktop`) |
| Bundled libmpv is a real file, not a symlink | pass |
| Bundled libmpv is a Mach-O arm64 library | pass |
| Every absolute dependency resolves | pass (20/20) |
| **libmpv from inside the bundle loads and creates a core** | pass — API 2.5, path reported as the bundle's own |

The last row is the one worth having. `capability()` prefers the system paths, so on
a development machine it always finds Homebrew's copy and never touches the bundle's
— which is exactly the copy that has to work on a machine *without* Homebrew. The
check therefore names the path explicitly (`capability_at`), which is what makes it
evidence about the bundle rather than about the host.

Negative tests were run too, because a check that cannot fail proves nothing:

| Input | Exit | Result |
| --- | --- | --- |
| No bundle at the given path | `2` | refuses to guess |
| A bundle with no executable and an unparsable plist | `1` | reports 3 failed checks |
| `/bin/ls` named as libmpv | `1` | `does not export mpv_create: dlsym failed` |
| The real bundle | `0` | all checks pass |

**Still not proven, by design:** that a frame reaches the screen, or that the window
opens at all. Both need a visible window and an owner-approved session.

## M4 — Samsung Tizen client (`apps/tv`), 2026-10-28

The TV client is built. It is a **new application**, not a port: the television does
not run the Rust core, so the client speaks the core's entities and invariants over its
own storage adapter. What was written, and what each part rests on:

| Piece | What it is |
| --- | --- |
| `src/core/` | the contract ported from `crates/screenroom-core/src/domain.rs`, plus the rules that decide things: the D-pad focus machine (the desktop's rule, exactly), the session/generation/sequence progress guard, the durable-document store, relative-locator validation, the scanner's rules (bounded walk, identity across rescans, withdrawal only after a complete walk), filename interpretation, the track menu and decoder preferences, and the verified next-episode rule |
| `src/platform/` | the Tizen boundary: `webapis.avplay` behind a playback port, remote-key registration, `localStorage` behind the storage port, removable-storage enumeration, the bounded cancellable walk, and `tizen.filesystem.toURI` for the decoder's address. The host probe reports what it found rather than assuming |
| `src/design/` | the same semantic tokens the desktop uses, multiplied for viewing distance, and the card/grid model the screens draw from |
| `src/screens/` | Home, Browse, Detail, Settings and Player, each owning one focus zone |
| `config.xml`, `vite.config.ts`, `scripts/prepare-tizen-dist.mjs` | the widget manifest, the engine-targeted bundle, and the step that turns the bundle into an installable widget directory |

### Verified here (2026-10-28)

```bash
cd apps/tv
pnpm typecheck    # clean; application source sees no ambient Node types at all
pnpm test         # 189 tests, 0 failures
pnpm build        # 484,867-byte main.js + the staging step
```

**2026-10-28, later: the screen was actually looked at, and it was broken.** The build
installed on the set drew one line of text on black. Reproduced in a browser at
1920×1080 against the *built* widget, which is what made it debuggable at all:

| Defect | Cause | Fix |
| --- | --- | --- |
| Only static text painted; every value set in `ready()` stayed empty | Blits makes an attribute reactive only when it starts with `:` (`isReactiveKey = (str) => str.startsWith(':')` in its code generator), so `content="$subheading"` was read once, at creation | every state-bound attribute now carries `:` |
| Home, Browse and Detail placed nodes at `undefined` | their templates read geometry members off a *computed object* as if they were flat state | the geometry is copied into state by `applyLayout()` before the first paint, the way Settings and Player already did |
| A card's title was drawn past the card's bottom edge | the title strip was guessed (`body * 2.4`) and shorter than the two lines under the art | `gridFor` derives the strip from the type scale and exposes `titleY`/`subtitleY` |
| Browse's third row was drawn below the set | the grid was sized from the width alone | `gridFor` takes the band it has to fit and shrinks the cards; Browse pages 5×2 |
| A file that would not open said so into an invisible node | the whole player template's alpha was the overlay's alpha, which starts at 0 | the scrim is its own layer; failures go through `reportFailure`, which keeps the message up |

Guarded by `test/templates.test.ts` (no template may read a name the component does not
declare) and the new `test/cards.test.ts` case (a card fits its own text; a page fits its
band). **Verified in the browser against the built widget only — not yet on the set.**

The signed package was produced through the `tizen-build-project` runner:
`apps/tv/build/ScreenroomTV-0.1.0.wgt`, 157,202 bytes, sha256
`b7d82c49755ae1501bffe62040cd2263aaa181510d477d3c4196d8b5710ecc51`. Its envelope
carried **no** "signed with Tizen default developer certificates" warning, and the
`main.js` inside the package is byte-identical to the tested bundle (sha256
`9b7cbdaea4a9bc2b5b3b3855d6382593742a751032c8153b3c917176bae597c4`). One signing
profile exists and is active (`screenroom`); no certificate material was inspected.

### What the client does now, on and off the device

- **The library is populated by the TV itself.** Settings → *Scan for media* lists the
  platform's storages, walks every mounted removable one with a bounded, cancellable
  walk, parses film and episode identity from filenames, and writes the catalogue, the
  sources and the scan report in **one** document. A drive that is not mounted, or that
  will not list, withdraws nothing.
- **Progress survives a pulled cable.** Item ids are carried forward per path across
  rescans, so a stored position stays attached; a drive that leaves marks its rows
  unreadable and never deletes them, and the storage-state listener does that the
  moment the platform reports the change.
- **Playback has a track menu.** Up/Down opens audio tracks, *Subtitles off* and the
  subtitle tracks; a choice is remembered as this device's preference and applied when
  the next file becomes ready. `setSilentSubtitle` is what "off" means, because a track
  index cannot express it.
- **A sidecar subtitle file is used where the decoder takes one** (`.smi`, `.srt`,
  `.sub`, `.ttxt` beside the video, the four Samsung documents). The app draws the text
  the decoder hands back, and says so when the decoder refuses the file.
- **A finished episode offers the next one** only when the catalogue holds exactly one
  candidate one number up in the same season.
- **The remote actually reaches the screens.** Every screen declares its keys in an
  `input` block — the first build declared none, so arrow keys, Enter and Back landed
  nowhere — and the player claims Back so the track menu closes before the file does.

### What M4 does **not** prove

- **Nothing has run on a Tizen engine.** No frame rendered, no WebGL probe, no frame
  budget. A packaged `.wgt` is a file, not a device result.
- **No media has reached AVPlay.** No URI, codec, seek or track switch. The URI form is
  now the platform's own (`toURI`), which removes a guess but not the need to try it.
- **No drive has been walked by a television.** The walk is exercised against a
  simulated Tizen filesystem (`test/scan-pipeline.test.ts`), which proves the wiring and
  proves nothing about USB permissions or timing on the set.
- **The TV's DUID has not been checked against the distributor certificate**, which is
  what decides whether the package installs at all.
- **The television could not be reached on 2026-10-28.** A network sweep of
  `192.168.0.0/24` on SDB port 26101 found no device, `sdb devices` listed nothing, and
  the saved-remote list was empty — so the earlier session's connection is not
  recoverable from here and the next step needs the set switched on and in developer
  mode.

Gate G5 is **PARTIAL** — see `TIZEN_DEVICE_HANDOFF.md` for what the set itself reported; the checklist and the questions only the
TV can settle are in [`TIZEN_DEVICE_HANDOFF.md`](TIZEN_DEVICE_HANDOFF.md).

**2026-10-29 — the set was reachable again and the fixed build went on it.** Package
`01d60176…d2231`, 160,680 bytes, `install completed`, launched, and the TV's own record
reads `canvases=1 size=1920x1080 css=1920px/1080px client=1920x1080 viewport=1920x1080
dpr=1 mounted=1` — the blank screen is gone at the engine level. The owner then reported
the **home screen with readable text and cards**, so item 1 now passes on both halves.
Items 3–12 (drive, scan, playback, tracks, subtitles, next episode, focus, restart,
reinstall) remain open and need the remote in a hand: this firmware accepts no synthetic
key press, exposes no screenshot and no `dlog` over TCP.

### Test totals (updated 2026-10-29)

```bash
cargo test --workspace
cargo clippy --workspace --all-targets
cargo fmt --all --check
```

**280 tests, 0 failures** (2026-11-01, after the acceptance work below; 264 on
2026-10-29), clippy with no first-party warnings, formatting clean:

| Suite | Tests |
| --- | --- |
| `screenroom-core` unit (filename, fingerprint, path identity, lists, recommend, media info) | 46 |
| `core/tests/library.rs` (incl. per-viewer isolation, path semantics) | 28 |
| `core/tests/storage.rs` (preferences, subtitle style, backup, unique safety copies) | 20 |
| `core/tests/scanner.rs` (incl. an unplugged drive, a deleted folder, a returned folder) | 16 |
| `core/tests/recovery.rs` | 7 |
| `core/tests/scale.rs` (10,000 items) | 2 |
| `screenroom-player` unit (incl. the bundled-libmpv search order, decode policy) | 21 |
| `player/tests/worker.rs` (real decode, audio, tracks, seeks, resume, software fallback) | 22 |
| `player/tests/libmpv_smoke.rs` | 2 |
| `screenroom-desktop` unit (journeys, detail, up next, containment, fullscreen read-back) | 91 |
| `desktop/src/app/acceptance.rs` (the 15 checklist steps, real media, real player) | 15 |
| `desktop` focus regression (the episode-list step) | 1 |
| `desktop/tests/playback.rs` | 6 |
| `desktop/tests/artwork.rs` | 3 |

Evidence: `~/.local/state/dev-evidence/screenroom-macos-final/hwdec-integrated-test.log`.

The count is not the coverage. Two scanner tests have `#[cfg(unix)]` bodies, and the
fourteen player tests print `skipped: …` and pass vacuously when no libmpv is found.
Neither of those is a Windows-specific gap; both are recorded so the number is not
mistaken for evidence it does not carry.

Headless scripts, which are not part of `cargo test`:

| Script | What it proves |
| --- | --- |
| `scripts/check-quit-routing.sh` | the running app re-pointed the menu bar's Quit — hidden window, no focus taken |
| `scripts/verify-bundle.sh` | the packaged bundle's structure, plist, bundled libmpv and all 20 of its dependencies |
| `scripts/foreground-guard.sh` | sourced by every foreground script; refuses without `SCREENROOM_ALLOW_FOREGROUND=1` |

---

## Owner session — automated half (2026-11-01)

**Status: 7 of the 15 macOS acceptance steps now pass automatically; 7 have
their objective half closed with one human observation left; 1 needs the owner
and the hardware.** Full receipt, per-step observations, the two defects found
and the reduced manual pass: [`MACOS_ACCEPTANCE_EVIDENCE.md`](MACOS_ACCEPTANCE_EVIDENCE.md).

`apps/desktop/src/app/acceptance.rs` drives the real `App` — its update loop, scan
thread, libmpv worker, checkpoint policy and SQLite file — against FFmpeg-generated
media in a temporary folder. `cargo test -p screenroom-desktop --lib acceptance`.

| Step | Outcome |
| --- | --- |
| 1 first-run scan | **PASS** — 1,200 files, row counts up, cards on screen mid-walk, `Scan finished` |
| 2 grid, posters, runtimes, one card per series | **PARTIAL** — all asserted; "looks right" is the owner's |
| 3 ↓ to episode 3, Enter | **PASS** — after fixing a real defect (below) |
| 4 Space and ←/→ | **PASS** for the mechanism; the listening half is the owner's |
| 5 audio and subtitle pickers | **PARTIAL** — selection read back from the player; audible change is the owner's |
| 6 fullscreen | **PARTIAL** — mode, platform exit, Escape order, header return; the fill is the owner's |
| 7 anamorphic | **PARTIAL** — a 2:1 fixture arrives at its 2160×1080 display shape, not 1.78:1 |
| 8 4K HEVC on battery | **HUMAN REQUIRED / open defect** — still 19.4 of 24 fps |
| 9 up next | **PASS** — offer, cancel, replay, Play now |
| 10 ⌘Q and reopen | **PASS** — stored and resumed within 1 s |
| 11 Home mid-film | **PASS** — session closed, audio clock stopped, place kept |
| 12 drive away and back | **PARTIAL** — folder-away and rescan asserted; the cable is the owner's |
| 13 backup and restore | **PASS** — change undone, safety copy named |
| 14 second profile | **PASS** — reopens on it; removal asks twice |
| 15 keyboard journey | **PARTIAL** — the journey runs on key events; readability is the owner's |

### Defects found by the acceptance work, and fixed

| Severity | Defect | Fix / regression test |
| --- | --- | --- |
| High | ↑/↓ stepped by the card grid's **column count on every screen**. Detail draws its episodes in one column, so on a 1,280-wide window the step was 4 and **↓ did nothing at all on a three-episode season** — acceptance step 3 was unreachable from the keyboard, and Enter played the episode the screen opened on | the Detail list steps one row, grids step by their columns; `down_the_episode_list_moves_one_episode_not_one_grid_row` |
| Medium | The catalogue was only reloaded when a scan **ended**, so a grid stayed empty for the whole walk and then filled in one jump — step 1 asks for cards before it finishes | Home and Browse refresh part-way through a walk, at most every 500 ms so a 250,000-entry scan cannot flood the database; `step_1_a_first_run_scan_fills_the_grid_before_it_finishes`, `step_1_a_mid_scan_report_refreshes_the_catalogue_before_the_walk_ends` |

Neither was found by inspection. Nothing was hidden to make a step pass: the
4K limit is unchanged and unclaimed, and no macOS permission was granted.

---

## No code comments (2026-11-01)

**Status: removed across the active tree, and enforced from now on.** The rule is
in `AGENTS.md` ("No code comments — mandatory, every agent"); this is the receipt.

| | |
| --- | --- |
| Comments removed | **5,357** in **136** files — Rust, TypeScript, JavaScript, shell, Swift, CSS/HTML/XML, TOML/YAML/JSONC |
| Scanned | every tracked file under `crates/`, `apps/`, `scripts/` and the root, by extension |
| Excluded | `docs/` (documentation), `reference/` (archived prototype), `.trail/cache/`, `target/`, `node_modules/`, `dist/`, `build/`, lockfiles, `LICENSE`, binary assets |
| Preserved | shebangs, `LICENSE`, and every non-comment byte |
| Enforcement | `python3 scripts/no_comments.py check` — per-language lexers, exits non-zero on any comment |

Removal is lexer-based rather than pattern-based, because a `//` inside a string, a
`#` inside `${var#pattern}`, a `/*` inside a TypeScript regex literal and a Rust
lifetime that looks like an unterminated character literal all resemble comments.
Detection and rewriting share one lexer, which is why `check` and `strip` cannot
disagree.

No functional comment directive existed to preserve: no `@ts-expect-error`, no
lint-disable, no `@vite-ignore`, and no doc-test depended on a removed `///` (all
four crates reported zero doc-tests before and after). The only permitted
comment-like forms left are the shebangs at the top of the shell and Python tools.

Verification after removal: `cargo test --workspace` **280 passed, 0 failed**;
`cargo clippy --workspace --all-targets` clean; `cargo fmt --all --check` clean;
`pnpm typecheck`, `pnpm test` **228 passed**, `pnpm build` all exit 0;
`bash -n` on every script; `swiftc -parse`; `node --check`; `python3 -m json.tool`
on every JSON file; and `scripts/no_comments.py check` reporting none left. The
only added lines in the whole change are blank or reformatted by `cargo fmt`.

---

## M3b — retained posters, skip segments, local suggestions (2026-10-29)

**Status: host-verified on macOS.** Three gaps that made the desktop feel unfinished
are closed, and each one is closed without a network, a model or a guess.

| Capability | Store (`screenroom-core`) | Desktop (`apps/desktop`) |
| --- | --- | --- |
| Offline artwork (F06) | new `artwork` table (schema **3**), `store_artwork`, `artwork`, `artworks_for`, `artwork_candidates`, `artwork_candidates_in`, `missing_artwork_count`. JPEG only, ≤768 KiB, ≤4096 px per side, magic number checked, one poster per work, replacing is an overwrite | `src/artwork.rs` runs ffmpeg as a child process (`-nostdin`, arguments as separate values so a filename can never be a shell fragment), one frame per work, 20 s deadline per run, output capped, fallback to the first frame when a stored duration seeks past the end; a bounded 16-deep queue on its own thread, never while a file is playing. Cards, Detail and Home draw the retained bytes |
| Skip intro/credits (F14) | `set_marker`, `mark_segment` (duration taken from the edition, `duration_unknown` when it is still unknown), `markers`, `remove_marker`; one marker per kind per edition; an interval that does not fit the film is refused, not clamped | Player pins a segment at the playhead (`i` intro, `c` credits, again to end it; `s` skips), offers *Skip intro*/*Skip credits* only while the playhead is inside a stored marker, and `auto_skip_intro` (default **off**) skips once per viewing |
| Local recommendations (F16) | `Library::recommendations`: next episode of a series in progress or just finished, then the viewer's watchlist, then more of a genre already being watched, then recently added. One row per work, unavailable and watched files excluded, each row carries its reason | Home draws a *Suggested for you* shelf from the same cursor order as Continue watching, with the reason on each card |

Schema change is additive and migrated: a `user_version = 2` library gains the
`artwork` table in place and keeps every row (asserted by a test that builds a
schema-2 database and opens it). A backup written before the table existed restores
with no artwork rather than failing — the restore reads only the tables the file
actually holds.

### Evidence from the running application

Four generated clips (three films, one series episode) were scanned into a real
library, then the application was launched against it with a hidden window and no
foreground:

```bash
cargo run -p screenroom-core --example prepare_library -- /tmp/sr-art/library.db /tmp/sr-art/media Library
SCREENROOM_LIBRARY=/tmp/sr-art/library.db SCREENROOM_WINDOW=hidden ./target/debug/screenroom-desktop
sqlite3 /tmp/sr-art/library.db "SELECT w.title, a.width, a.height, a.sampled_at_ms, LENGTH(a.bytes) FROM artwork a JOIN works w ON w.id = a.work_id;"
```

Observed: four rows, each `400×226`, `8,883` bytes — one poster per work, produced by
the running application from real ffmpeg runs, not by a test fixture. `sampled_at_ms`
is `0` for all four because a 4-second clip has no stored duration, so the first probe
is at 120 s; the fallback to the first frame is what turned that into a poster rather
than a failure, which is the behaviour that test covers.

Checks: `cargo test --workspace` **211 passed, 0 failed**; `clippy --workspace
--all-targets` clean apart from the pre-existing transitive `block 0.1.6`
future-incompat note; `fmt --all --check` clean.

### What this does **not** prove

- Posters have been **stored and handed to the widget**, and the mapping is unit
tested, but nobody has looked at a visible window since they were added. Drawing is
**PENDING** an owner-approved foreground session, like the on-screen budget.
- No real personal media was sampled; the posters above come from generated test clips.
- The skip button has not been pressed during a real viewing; its interval rule, the
once-only auto-skip and the store's validation are tested.
- Recommendations are exercised against synthetic libraries; how they read with a
real 1,200-file library is unmeasured.

---

## macOS audit and completion (2026-10-29)

**Status: host-verified headlessly on macOS; the visible-window, listening and
drive-in-hand steps are PENDING in [`MACOS_ACCEPTANCE_CHECKLIST.md`](MACOS_ACCEPTANCE_CHECKLIST.md).**

Three independent read-only reviews (core/data, player/FFI, desktop UI and
packaging) audited everything from the first commit to `4719073`. Every finding
below was checked against the code before it was fixed. Where a fix is reachable
without a GPU device or a visible window it carries a regression test; the final
review found three tests that could not fail, and they were corrected or renamed
(see "Final independent review" below). Fixes came from the lead and from two
Codex workers in isolated worktrees, and every worker commit was compiled, tested
and reviewed by the lead before integration. The workers could not run Cargo in
their sandbox, and three of their new tests failed once run here: two had wrong
macOS temp-path fixtures, and one exposed a real numeric-title defect.

### Findings fixed, by severity

| Severity | Defect (root cause) | Fix / regression test |
| --- | --- | --- |
| Critical | Restoring the `.before-restore` safety copy overwrote that copy first, then "restored" the current library onto itself — the original was lost while success was reported | every safety copy has a unique name (`restore_safety_copies_are_unique_and_safe_to_restore_from`) |
| High | Leaving the player by the header (Home/Library/Settings, ⌘1–3) kept libmpv playing out of sight; the next title replaced the open session without storing its position | `end_viewing` + `open_player` finishes any open viewing (`leaving_the_player_by_the_header_stores_the_viewing`) |
| High | The installed bundle ran bare `ffmpeg`, which a Dock/Finder launch (launchd `PATH`) cannot find — every poster failed outside a terminal | `artwork::tool` resolves Homebrew prefixes; the packaged binary under launchd's `PATH` stored 30 posters and 827 runtimes |
| High | A title starting with a year (`2001 A Space Odyssey (1968)`, `1917 (2019)`, `Blade Runner 2049 (2017)`) lost its title and could merge unrelated films | a bracketed year wins; otherwise the first bare year, unless another year follows it directly (`Blade.Runner.2049.2017`); a numeric title beside a year is a film (`prefers_release_year_after_numeric_title_years`) |
| High | Scan root checked by normalised key, ingest by raw string: a differently spelled stored root failed every file and aborted the scan | ingest compares the stored root (`scan_ingest_accepts_the_stored_root_spelling`) |
| High | Removing a profile deleted its whole history in one click | two-press confirm (`removing_a_profile_asks_first_and_the_choice_survives_a_reopen`) |
| High | A restore left preferences stale in memory; the next unrelated save wrote the pre-restore set back | preferences, language fields and poster caches re-read (`a_restore_brings_back_the_backups_preferences_and_keeps_them`) |
| High (plausible) | The worker published the resume position before the seek landed; a quit within ~2 s could store 0 over a real resume point | load starts at the resume point (`loadfile … start=`), positions are "ready" only after `PLAYBACK_RESTART`, and the app stores only `snapshot_is_current` snapshots (`resume_position_is_honoured_when_loading`, sampled continuously) |
| Medium | Autoplay/replay could store the previous file's end position into the next session; a pre-seek position could be saved under the new seek generation | load id and seek serial carried in `PlayerSnapshot` and checked before every checkpoint (`only_the_current_loads_settled_position_is_stored`) |
| Medium | Anamorphic video letterboxed twice: the render target used storage size, not display size | `VIDEO_RECONFIG` + `dwidth/dheight`; Tulsa King S01E01 (1920×1080, SAR 9:8) now renders 1920×960 instead of 1920×1080 with 60 px bars |
| Medium | Event constants were wrong for client API 2.5: "track switched" (20) is `MPV_EVENT_SEEK`, 12 no longer exists | constants checked against `/opt/homebrew/include/mpv/client.h`; tracks follow the observed `track-list` |
| Medium | Ended/Failed were terminal, so a new load never left them | load resets to Idle (`a_new_load_leaves_ended_state_and_plays_again`, `…recovers_from_a_failed_state`) |
| Medium | One permanent scan issue blocked withdrawing deleted files forever; an incomplete scan still marked the source available | partial scans withdraw only stat-confirmed missing files **and only if the root still resolves afterwards** (a drive pulled mid-check withdraws nothing); state and `last_scan_ms` only on a complete scan |
| Medium | Non-UTF-8 parent directories produced lossy paths | whole relative path validated (`non_utf8_parent_directory_is_skipped_as_invalid_name`) |
| Medium | Recommendations re-offered titles in progress and late episodes of unstarted shows | in-progress works excluded, first episode chosen |
| Medium | `mark_watched` kept the resume position; a rewatch never returned to Continue Watching | both reset (`explicit_watch_decisions_clear_resume_and_rewatch_clears_watched`) |
| Medium | Enter on Detail played the opened file, not the highlighted episode; Back reset the grid to its first card; episode rows went stale after a viewing | `detail_starts_on_the_opened_episode_and_back_returns_to_its_card`, `back_from_detail_returns_to_the_card_and_the_watch_filter_narrows` |
| Medium | A second scan replaced the first's token and channel (uncancellable, unreported) | queued (`a_second_scan_waits_for_the_first_instead_of_orphaning_it`) |
| Medium | The genre picker was an always-empty dead control with no way back to "any" | hidden until genres exist, with an "Any genre" entry |
| Medium | The chosen profile was not remembered across launches | `opening_profile` reads the stored default |
| Medium | Paused picture stretched on resize/fullscreen | fit factors rewritten on every `prepare` (needs a GPU device to test; checklist step 6) |
| Medium | Focused cards drew near-white text on the near-white focus fill | focused lines use the focus foreground |
| Low | Arrow seeks with an unknown duration jumped to 0:00; the intro button was labelled "End credits…" while credits were pinned; preference saves erased unrelated errors; the window's own fullscreen exit was not noticed; playback could follow `..` or a link out of its granted folder; `Mpv::wait_event` was a data race from safe code; partial backups were left behind; resolution tokens (`720x480`) read as episodes; removed-source re-grant and default-profile clean-up; `.ok()` hiding SQLite errors; integrity check after migrations | fixed with tests where reachable (`playback_never_resolves_outside_the_granted_folder`, `the_window_mode_read_back_overrides_what_the_app_last_asked_for`, …) |

### Final independent review (2026-10-29)

A fresh read-only review of `4719073..2b71442` returned **accept with fixes**. Each
finding was checked against the code first.

| Severity | Finding | Resolution |
| --- | --- | --- |
| High | A seek sent while a file was still loading was dropped by the worker but never confirmed, so `snapshot_is_current` refused every checkpoint for the rest of that viewing (scrubbing during "Resolving" on a slow drive) | the worker queues an early seek until `FILE_LOADED`, and settles impossible or failed seeks at once (`a_seek_sent_before_the_file_loads_runs_once_it_has_and_is_confirmed`, `an_impossible_seek_is_settled_rather_than_left_pending`) |
| Medium | Replaying during the "Up next" countdown was replaced by the next episode 5 s later | every viewing that ends drops its offer (`a_finished_episode_offers_the_next_with_a_cancelable_countdown`, replay case) |
| Medium | A source that came back stayed unavailable while every rescan hit one recurring issue (a `.part` file), hiding it from autoplay and suggestions | a partial scan whose root still resolves marks the source available; `last_scan_ms` stays complete-only (`a_returned_folder_is_available_again_even_when_its_rescan_is_partial`) |
| Medium (plausible) | A seek into the end gets no `PLAYBACK_RESTART`, so the watched checkpoint could be refused | `END_FILE` for the current entry settles every outstanding seek (`a_seek_into_the_end_is_settled_when_the_file_ends`) |
| Low | `Alien.1979.Directors.Cut.2003` parsed as year 2003 | first bare year rule above |
| Low | The duration pass and poster candidates joined stored paths unchecked, so a `..` path from a restored backup reached ffprobe/ffmpeg | `under_root` refuses any non-plain relative path; removed sources are skipped |
| Low (plausible) | A pause sent before `START_FILE` was ignored | the worker records the requested pause |
| Tests | the tool-resolver test never narrowed `PATH`; the fit test passes on the old code; the scan-queue test asserted `≤ 1` | resolver tested under launchd's `PATH`; fit test renamed to what it checks; queue asserted exactly |

Recorded, not fixed: **L2** resume uses the four-argument `loadfile … start=` form,
which needs mpv 0.38 or newer (this Mac has 0.41; the Windows build must check it);
**L4** a partial scan's stat checks run inside the write transaction, so a sleeping
drive can stall the UI briefly; **L6** an Escape during macOS's fullscreen animation
can leave the player rather than fullscreen.

A second fresh review of `2b71442..0652d0f` (the fixes above and the decoding work)
confirmed the new event-driven player loop, then found:

| Severity | Finding | Resolution |
| --- | --- | --- |
| High | Queuing early seeks let auto-skip-intro fire on the 0:00 the player reports before a resume lands, so an intro marked from 0:00 replaced a 25:00 resume point | auto-skip runs only on a settled snapshot of the current load (`automatic_skipping_only_happens_once_and_only_when_asked_for`, resume case) |
| Medium (plausible) | A root that resolves but cannot be listed (privacy denial, failing disk) was marked available after a partial scan | the source is revived only when its root lists |
| Low | A seek after the file ended put the player into Failed; `Blade Runner 2049 - 2017` misread the year; probing and posters still ran against absent sources; a test assertion could not fail; the measurement example committed a home-directory path | settled without a command; any separator run counts as adjacent; candidates require an available source; the tool test plants a file on `PATH`; the example uses `SCREENROOM_EVIDENCE_DIR` or the temp dir |

Recorded: `hardware_decoder` in the snapshot is diagnostic and may stay empty for
a file if libmpv sends no change after load.

### Functionality completed

- **Runtimes before first play.** `Library::unprobed_files` / `record_duration`
  plus a bounded background ffprobe pass (`apps/desktop/src/probe.rs`), paused
  while a file plays and restarted after each scan. On the real library: **827 of
  830 durations in ~80 s** (the three truncated MP4s stay unknown, honestly).
- **Embedded cover art** is the poster when a file has one (115 of 830 do);
  otherwise a frame at a real 10 % instead of a two-minute guess
  (`examples/poster_check`: 2 covers at 400×600, 16 frames, 2 truncated files
  refused).
- **Browse shows one card per work** (a 149-episode show was 149 cards), with a
  watched/unwatched filter.
- **Cancelable autoplay countdown** (F10): "Up next … in 5 s", Play now (Enter),
  Cancel (Escape).
- **Copy-back hardware decoding** on macOS (`auto-copy`, measured above), with a
  software fallback for codecs VideoToolbox does not take and `SCREENROOM_HWDEC=no`
  to force CPU decode; the idle player now waits on libmpv's callbacks instead of a
  20 ms poll.
- **Packaging:** ad-hoc signed inside-out and verified strictly, removable-volume
  and documents usage descriptions, media-tool presence reported;
  `scripts/verify-bundle.sh` passes all checks.

### Measured

| Measure | Result |
| --- | --- |
| `cargo test --workspace` | see "Test totals" below |
| Grouped Browse page (the query Browse now runs) | 44 ms first / 64 ms last page + 9 ms count at 10,000; 194 / 487 ms at 50,000 (flat paging 6 / 18 ms at 10,000) |
| ffprobe per file on the USB SSD | ~80 ms |
| Audio-clock test, formerly flaky under load | 5 of 5 passes, one beside two other test binaries |
| Copy-back decode (`auto-copy`) vs software, 4K HEVC Main10 3240×2160 | CPU 344 % → 165 %, RSS 737 → 486 MiB, 13.5 → 19.4 fps (source 23.976), luma identical |
| Same, 4K HEVC 3840×1600 / 1080p H.264 | CPU −13 % / −31 %, RSS −30 % / −20 %, both 23.95 fps |
| Idle player | 0.00 % CPU before and after; 1,500 timer wakes per 30 s → 0 (event-driven) |
| SSD integrity after the session | 830 videos, 0 files newer than the session start |

### Accepted limits (recorded, not fixed)

- **Dock Quit, logout and shutdown** send `terminate:` to the application, not
  through the routed menu item, so the open session is not closed cleanly. The
  position loss is bounded by the 1 s checkpoint interval, and the stale session
  is closed on the next open.
- **Accessibility:** VoiceOver-specific support was removed on 2026-11-03
  ([ADR-005](ADR-005_SUBTITLE_PROVIDER_AND_FEATURE_REMOVALS.md)); the
  `NSAccessibility` bridge, its announcements and its `SCREENROOM_AX_CHECK`
  receipt no longer exist, so the desktop publishes no accessibility tree and no
  screen-reader pass is run. Everything else stays: since 2026-11-01 every
  Settings action is reachable with Up/Down and Enter through the focused action
  strip, Browse, Detail and the player are fully keyboard-driven, focus order and
  restoration are asserted, Reduce motion is honoured, and subtitle size and
  position are read back from libmpv. Typed fields (profile name, language codes,
  paths) and the two sliders still need the pointer or typing.
- **Backup and restore run on the UI thread.** The real catalogue's backup is
  1.5 MB, so the pause is short; a far larger library would need them moved off
  it.

---

## Blocked gates

| Gate | Status | Reason |
| --- | --- | --- |
| Desktop frame presentation on real Windows GPU | BLOCKED | no Windows hardware/Visual Studio on this host; every `cfg!(windows)` branch is unbuilt as well as unrun. The exact handoff — commands, expectations, acceptance checks and named falsifications — is `WINDOWS_VALIDATION.md` |
| Windows 4K/hardware decode (SDR) | BLOCKED | same. HDR is not part of this row: HDR was removed on 2026-11-03 (ADR-005), so no Windows HDR evidence is owed |
| Physical Samsung UA50AU8000RXER — install | **PASS** (2026-10-28) | connected at `192.168.0.172:26101`; `sdb capability` reports `vendor_name:Samsung`, `profile_name:tv`, `platform_version:6.0`, `cpu_arch:armv7`. The distributor chain is Samsung-issued with exactly one `deviceid` binding, and the DUID the set reports matches it (compared by hash; neither value printed). `vd_appinstall` reached `install completed` and `vd_applist` lists `PlYByk3psw.ScreenroomTV` 0.1.0 |
| Physical Samsung UA50AU8000RXER — playback, USB, durability | NOT YET RUN | needs a person with the remote: this firmware accepts no synthetic key press, exposes no screenshot, no `dlog` over TCP and no inspector port. Items 3–7 of G5 |
| Lightning 3 on the Tizen 6.0 engine | **PASS on the engine, PARTIAL on measurement** (2026-10-29) | the set's own record: `host=tizen avplay=1 filesystem=1 inputdevice=1 storage=1 keys=9/refused=0 model=UAU8000 firmware=T-NKLAUABC-2310.0` and `canvases=1 size=1920x1080 css=1920px/1080px client=1920x1080 viewport=1920x1080 dpr=1 mounted=1`, no `error` or `rejection` entries, engine `76.0.3809.146`. The bundle executes, Lightning mounts a full-size surface, and the **owner has seen the home screen with readable text and cards**. WebGL capability and frame pacing are still not measured |
| Audible output heard by a person | NOT YET VERIFIED | the audio clock and `avsync` are measured; nobody has listened |
| macOS bundle signing and notarization for distribution | BLOCKED | Command Line Tools only, no full Xcode; not required for a personal install |

LG webOS and Android TV were retired by [ADR-004](ADR-004_PERSONAL_EDITION_SCOPE.md);
their former rows are kept in Git history, not as live work.

A blocked gate is not a pass. Do not mark any of these complete without a real
receipt.

---

## Open risks

1. **libmpv → wgpu bridge.** Presentation is proven on macOS at 1080p. The software
   bridge costs one full-frame CPU copy per frame, so 4K performance is unknown and
   expected to be insufficient. Zero-copy D3D11 (Windows) / OpenGL (macOS) paths are
   designed but unbuilt.
2. **TV storage adapter.** Tizen may not allow SQLite; the fallback adapter and its
   durability are unproven on device.
3. **Lightning 3 frame budget** on a 2021 Tizen engine is unmeasured.
4. **Private design system boundary.** Mapping is implemented with the token export
   kept out of Git; a fully public build (no private pin at all) still has to be
   built once and proven to compile and run.
5. **Windows path semantics — the rules are closed, the platform is not.** The
   folding rule was a `cfg!` and is now a value (`PathSemantics`, chosen by
   [`Library::with_path_semantics`]): case-folding is a property of the *volume*, not
   of the operating system, and a case-sensitive APFS or NTFS volume exists. Both
   branches are tested from this Mac, and the key rule is asserted directly
   (`/Media/Shows` folds to `/media/shows` under `CaseInsensitive` and is left alone
   under `CaseSensitive`).

   The *spelling* rules were added on 2026-10-28 (`paths::location_key`, with
   `PathStyle` as a value for the same reason), because comparing paths literally
   was wrong on Windows in two ways: `std::fs::canonicalize` returns a verbatim path
   there, so the scanner's symlink guard refused **every** healthy Windows source as
   a replaced link, and `D:\Media` did not compare equal to `D:/Media`, so the same
   folder could be granted twice. Both are asserted from this Mac with
   Windows-shaped inputs.

   **Still open:** the file-system half. On this host's case-folding volume,
   `Media/SHOWS` resolves to `Media/Shows`, so the case-sensitive *grant* path cannot
   be reached through the file system here; the test detects that and asserts the
   reachable half instead of claiming the other. And **no Windows target compiles
   from this Mac at all**: `cargo check --target x86_64-pc-windows-msvc` fails inside
   `libsqlite3-sys`, whose bundled SQLite needs the MSVC SDK's headers
   (`fatal error: 'stdlib.h' file not found`). That is a toolchain gap, not a code
   failure, and it means every `cfg!(windows)` branch in the tree — the libmpv
   candidate names, the D3D11 bridge choice, the `WindowsPortable` folder logic — is
   still unbuilt and unrun. Windows remains a blocked gate; `WINDOWS_VALIDATION.md`
   is the handoff.

6. **Packaged binary vs development tree.** The `.app` bundles libmpv but not its
   transitive Homebrew dependencies, and it is unsigned. That is deliberate for a
   personal install; making it portable is a separate licensing and signing task.
7. ~~**⌘Q is ungraceful.**~~ **Fixed** — `apps/desktop/src/quit.rs` re-points the
   menu bar's Quit at `performClose:`, so the menu item and the close box are one
   operation. The two dismissal paths were confirmed identical with real key presses;
   that confirmation predates the background-first policy and has not been re-run
   under it, while `scripts/check-quit-routing.sh` re-checks the routing headlessly.
8. **Keyset paging for very large libraries.** The query budget is inside a frame at
   10,000 entries but not at 50,000 (~93 ms for a page plus count), where `OFFSET`
   must be replaced with keyset paging. Not fixed ahead of a library that size
   existing; measured in M1 above.
9. **A drive that returns under a different letter needs a relink.** Windows mints
   a new drive letter for the same volume, and the letter is part of the path
   spelling, so a USB source granted as `E:\Media` is not recognised as `F:\Media`:
   there is no volume-serial identity yet. The recovery is an explicit relink, which
   re-issues the grant and re-proves every file, so nothing is deleted and history,
   lists and resume positions survive. Recorded as a limit rather than a defect
   because the recovery path exists and is the honest one — that two letters are one
   volume is not something a string can know.
10. **A download sidecar in capitals is not noticed on a case-sensitive file
    system.** `has_download_sidecar` probes for the lowercase suffix with `exists()`,
    which finds `Movie.mkv.CRDOWNLOAD` on Windows and on this host's case-folding APFS
    volume but not on Linux. There the file is imported while still being written and
    is caught by the size/mtime settle check instead. Left alone because the
    Windows-side case — the one this session had to answer — already works, and the
    Linux half means matching case against the directory listing the walk already
    holds.

---

## Next milestone

**Finish M3's remainder, then the two honesty gaps that only the owner can close.**

1. Remaining M3: browse previews, and the **on-screen** rendering/input budget at
   10,000 items. The M3b posters also need one visible-window pass to be called
   drawn rather than stored. Subtitle styling is delivered (above); the store and offscreen
   halves of the budget are measured (M1 and the render section above). The on-screen
   half needs a visible window, so it is **PENDING** an owner-approved session.
2. Listening pass for gate G3b: hear a known audio track, and confirm the measured
   0.0 ms `avsync` figure matches what is heard.
3. Repeat the offline check with networking actually disabled rather than by
   observing zero sockets.
4. Native 4K presentation. The offscreen 4K row above measures a **1920×1080**
   render target because `render::MAX_DIMENSION` caps it, so gate G2's 4K half is
   untouched by it.

What is left before macOS acceptance, exactly (updated 2026-10-29, after the audit):

| # | Remaining | Why it cannot be closed here |
| --- | --- | --- |
| 1 | The owner's 15-step session in [`MACOS_ACCEPTANCE_CHECKLIST.md`](MACOS_ACCEPTANCE_CHECKLIST.md): listening, fullscreen fill, anamorphic look, 4K on battery, drive unplug, keyboard-card readability | needs ears, a visible screen and a cable in hand. The mechanisms behind all 15 are now asserted (above); these are the observations left. Calendar date, resume and profiles no longer need a person |
| 2 | Real-time 4K above ~3K wide | needs a GPU (zero-copy) render bridge; copy-back decode alone reaches ~19 of 24 fps on the hardest file |
| 3 | The genre filter on real data | no genres until enrichment exists; the control is hidden rather than dead |
| 4 | An offline check with networking actually off, and a battery measurement | changes this Mac's network and power state, which this session should not do unasked |
| 5 | Keyboard focus in Settings | removed from this list on 2026-11-03: the Settings action strip closed it, and the VoiceOver half it used to wait on was removed (ADR-005). A pointer is still needed for typed fields and the two sliders |

Closed by the audit: runtimes before first play, embedded cover art, anamorphic
sizing, copy-back hardware decoding (measured), and every finding in "macOS audit
and completion" below.

See [`IMPLEMENTATION_PLAN.md`](IMPLEMENTATION_PLAN.md) for the full definitions of
done.

## F25 — local conversion (2026-11-01)

**Completed on macOS, verified headlessly.**

- **Core** (`crates/screenroom-core/src/library/conversion.rs`, schema 5,
  migrated additively from 4): three profiles — remux, audio fix and H.264 — a
  plan that names one new output file and refuses to touch the original, a
  folder that is refused when it is read-only (so `/Volumes/bleh` cannot be
  written to even by accident), a forward-only job lifecycle with one active job
  per file, and a verification rule: the copy keeps its picture, keeps its audio
  when the source had any, reports a duration, and runs within the larger of 2 s
  or 1 % of the original.
- **Desktop** (`apps/desktop/src/conversion.rs` and the Detail/Settings UI): one
  conversion at a time; `ffmpeg -progress pipe:1` drives a percentage that is
  written to the job; `-n` and a proven-absent output name mean ffmpeg has no
  way to overwrite anything; *Stop* kills the process, records the stop and
  removes the partial copy; a finished run is probed before it is kept, and the
  original's size, modification time and bytes are re-read afterwards — a change
  there fails the job instead of calling the copy good.
- **Tests added:** five core tests (naming, read-only refusal, lifecycle,
  cancellation, verification) and four runner tests (arguments, per-profile
  codecs, progress parsing, time budget), plus `step_16`, an acceptance test that
  builds a real clip with ffmpeg, converts it through the app, asserts the copy
  probes with picture and sound, asserts the original's size, mtime and bytes are
  identical, and asserts a second conversion takes a new name.
- **Not proven:** the panel has not been looked at in a running window, and no
  conversion has run on a large 4K file, so throughput and peak disk use on the
  owner's hardware remain unmeasured.

## F17 — browse previews (2026-11-01)

**Completed on macOS, verified headlessly.**

- `apps/desktop/src/preview.rs` owns the rules: a 600 ms dwell, a start
  position inside the file (one fifth, bounded to 1–300 s and never past half
  the runtime), a 20 s cap, and one refusal function that answers *Reduce
  motion*, *something is playing*, *that file is unreadable* and *no player on
  this machine* with a reason rather than a silent no.
- The application holds at most **one** preview decoder: `App::preview` is a
  single `Option<Preview>`; a new card replaces it, leaving Browse, starting
  playback or starting a conversion drops it, and a preview is never opened
  while `player` or `active` exists. Ticks run at the player's 60 Hz cadence
  while a preview is live and at the status cadence otherwise.
- Still fallback is the default state, not an error state: the stored poster is
  what a card draws until a preview actually presents a frame, after the 20 s
  cap, after the clip ends, and for any file in the refused set. Nothing is
  drawn from a URL and nothing leaves the machine.
- *Reduce motion* is a stored preference (Settings checkbox plus a keyboard
  action), and turning it on stops a running preview immediately.
- **Tests added:** three rule tests (refusals, start position, delay bounds),
  `step_17` (a real clip, dwell → decoder → presented frame → leaving Browse
  cleans up → Reduce motion refuses), and the Settings keyboard test now finds
  its targets by label instead of by index.
- **Not proven:** the preview has not been watched in a window, so the feel of
  the dwell and the look of a video inside a card are unverified, and nothing
  measures the second decoder's memory or GPU cost on the owner's machine.

## F05 (offline half) and F11 — local NFO metadata and a genre filter with real data (2026-11-01)

**Completed on macOS, verified headlessly. Online enrichment stays BLOCKED.**

- `crates/screenroom-core/src/nfo.rs` reads a sidecar the owner already keeps
  beside a file — `<name>.nfo`, then `movie.nfo`, then `tvshow.nfo` — with a
  256 KiB input cap, 2 000-character fields, 20 genres of 40 characters,
  entity decoding, nested markup stripped, and no guessing: a file with no
  recognisable `movie`/`tvshow`/`episodedetails` root, or one whose values are
  all empty, yields nothing.
- `Library::enrich_from_sidecars` applies a sidecar inside one transaction:
  title, year, overview and genres on the work, title and overview on the
  episode the file belongs to. `metadata_revision` moves only when a value
  changes, so a rescan of an unchanged library writes nothing. A work marked as
  a **manual match is never overwritten**, and a file with no sidecar is left
  alone. Schema 6 adds `works.metadata_origin`, additively, and the migration is
  guarded by an explicit column check so a database that already has it — a
  fresh one, or a restored backup — migrates cleanly.
- The scan thread runs the enrichment for the folder it just walked, before it
  reports completion, and the status line says how many works gained details
  from a local NFO file. Detail names the provenance: *details from a local NFO
  file*.
- **F11 becomes real:** the genre filter is exercised end to end by `step_18`
  in the running application — a sidecar in a scanned folder gives the film its
  real title, year, overview and `Drama` genre, `Drama` keeps it, `Comedy`
  empties the grid, clearing brings it back, and Detail shows the origin.
- **Tests added:** seven sidecar tests (parsing, entities, episode sidecars,
  refusals, bounds, preference order, oversize), six enrichment tests
  (film, series plus episode, manual match untouched, idempotence, schema 5 → 6
  with a kept row), and `step_18`.
- **BLOCKED, not faked (state at 2026-11-01; superseded on 2026-11-03):** online
  enrichment (F05's provider half) and the subtitle provider (F15) had no account,
  key or chosen service, and nothing here invented one. The subtitle provider was
  then chosen — The Subtitle DB, [ADR-005](ADR-005_SUBTITLE_PROVIDER_AND_FEATURE_REMOVALS.md)
  — and its live calls are still PENDING; see F15 above.
- **Not proven:** no sidecar has been read from the owner's own library, because
  none of its files has an NFO beside it yet; the genre dropdown has not been
  looked at in a window.

## F23 and F24 — an owner-written source directory and an honest handoff (2026-11-01)

**Implemented on macOS, verified headlessly. The directory itself stays BLOCKED.**

- ADR-011 records the blocker plainly: no verified nationwide list of Bangladeshi
  sources, no operator permission, no measured ISP reachability. Rather than seed
  a list I cannot verify, the app now implements the part that is legitimate — a
  directory the owner writes themselves, which is exactly what ADR-011 asks for
  ("start from owner-added bookmarks and verifiable evidence only").
- `crates/screenroom-core/src/library/bookmarks.rs` stores a bookmark with its
  protocol and a fixed provenance sentence. Only `https`, `http` and `ftp` are
  accepted; an address with a space, a control character, `file:`, `magnet:` or
  `javascript:` is refused with a reason, and a duplicate address is refused
  rather than silently merged. Name, address and access note are all bounded.
  Schema 7 adds `source_bookmarks` additively, guarded by a table check so a
  database that already has it migrates cleanly.
- Settings → *Saved sources* is the whole interface: three typed fields, a
  *Save source* action, and a row per source showing name, protocol, access
  note, address and provenance with *Open* and *Remove*. Every typed field and
  every row action is also in the keyboard action list, so nothing here needs a
  pointer — this is the F21 rule applied to the new controls.
- The handoff (`apps/desktop/src/external.rs`) spawns the system opener with the
  address as a single argument — no shell, no interpolation — and refuses
  anything that is not a web or ftp address before spawning. `SCREENROOM_OPEN_LOG`
  records the exact address to a file in tests, so the handoff is proven without
  opening a browser or touching the owner's desktop.
- **Tests added:** five bookmark tests (saving with protocol and provenance,
  refused schemes and spaces, duplicate and bounds, removal, schema 6 → 7 with a
  kept row), two handoff tests (refusals, the log hook), and `step_19`.
- **Not proven, and not claimed:** no source in this build has been checked for
  reachability, catalogue, cost, legality or ISP exclusivity; there is no
  bundled list, no floating "verified" badge and no automatic probe. The panel
  has not been looked at in a window. Embedded BitTorrent remains the separate
  decision the PRD says it is, and is not implemented.

## F18 — Picture-in-Picture: withdrawn (2026-11-02)

**Not offered, on purpose. This supersedes the mini player recorded on 2026-11-01.**

- The owner decided on 2026-11-02 that Picture-in-Picture is not wanted in any
  form. Both the OS feature and the in-app mini player that stood in for it were
  removed: `mini`, `mini_view`, `reduce_to_mini`, `expand_from_mini`,
  `close_mini`, `Message::ReduceToMini`, `Message::ExpandPlayer`,
  `Message::CloseMini`, `Action::MiniPlayer`, the *Mini player* button and the
  Alt+V shortcut no longer exist, and its acceptance test (`step_20`) was deleted
  rather than replaced.
- No capability row, PRD requirement, test-plan row or setting claims the
  feature. ADR-016 is marked **WITHDRAWN** and nothing else depends on it.
- Everything the player actually needs is untouched and still proven: playing,
  pausing, seeking, the next episode, fullscreen, casting, chrome auto-hide and
  back navigation.

## F15 — subtitles: file first, then The Subtitle DB (updated 2026-11-03)

**The offline half is complete on macOS and unchanged. The online half is one
provider — The Subtitle DB — behind a file-first gate, and its live calls are
PENDING.**

- **Resolution order (binding, [ADR-005](ADR-005_SUBTITLE_PROVIDER_AND_FEATURE_REMOVALS.md)):**
  an embedded subtitle track in the file, then a sidecar beside the media
  (`.srt`, `.smi`, `.vtt`, `.ass`, `.ssa`, `.sub`, `.ttml`, `.dfxp`) or a kept
  copy in the library, and only when neither exists may the app ask The Subtitle
  DB. The control is *Find subtitles online* and it returns before any network
  call when a track or a sidecar is present. There is no second online service
  and no fallback chain; OpenSubtitles is removed.
- **The store is unchanged.** `crates/screenroom-core/src/library/subtitles.rs`
  keeps the bytes the owner chose **in the library** — the `subtitles` table
  (schema 8, additive, guarded) — so a subtitle survives the file moving and works
  with the network off. Bounds: 4 MiB, text-only (a NUL in the first 8 KiB is
  refused), eight known extensions, and the source file is read once, never
  written, moved or renamed. Language comes from the provider's own record for a
  download, or a filename token / `lang=` tag for an import, and is otherwise
  reported as *Unknown* rather than guessed.
- The player gained `Player::sub_add` → `PlayerCommand::SubAdd` → mpv
  `sub-add`, then re-reads the track list, so a kept subtitle appears in the
  existing subtitle picker. Removal deletes only the stored copy.
- **Provider key:** `SCREENROOM_TSDB_API_KEY`, then `SCREENROOM_TSDB_CONFIG`, then
  `subtitles.key` in the platform config directory, honouring
  `SCREENROOM_CONFIG_DIR`. The owner's key exists at the macOS path; its value is
  not recorded anywhere in this repository.
- **Tests that exist:** the five store tests above (round trip with
  language/format/provenance, Bengali and unknown languages, refused
  binaries/wrong extension/empty/oversize, removal leaving the original file,
  schema 7 → 8 with a kept row) and `step_21` (real clip, import, hand to the
  player, remove, original bytes compared before and after).
- **PENDING, and the reason the row is not green:** no anonymous lookup, keyed
  search, download or playback of a downloaded subtitle has a receipt. The four
  commands that settle it, and the installed-app run, are in
  [`SUBTITLE_PROVIDER.md`](SUBTITLE_PROVIDER.md) §8. A planning pass on
  2026-11-03 recorded live lookups answering and a `302` to the file host; a
  transcript is not a receipt for this application.
- **Also not proven:** the panel has not been looked at in a window, and the TV
  client has no online subtitle provider at all — it reads embedded tracks and the
  four sidecar documents its decoder accepts.

## F19 — receiver discovery, with casting itself refused (2026-11-01)

**Discovery is implemented and honest. Sending media to a receiver is not.**

- The PRD asks for *real receiver detection plus reachable, permitted stream and
  compatible codec*, and says any temporary LAN endpoint must be confined,
  session-scoped and off by default. The detection half exists; the streaming
  half does not, and the panel says so in words rather than offering a button
  that cannot work.
- `apps/desktop/src/casting.rs` sends one SSDP `M-SEARCH` for
  `urn:schemas-upnp-org:device:MediaRenderer:1`, waits at most 3 s and collects
  at most 12 answers. `parse_response` is pure and strict: `HTTP/1.1 200` only,
  a `MediaRenderer` `ST`/`NT`, an `http(s)` `LOCATION`, no answer over 8 KiB and
  no `file:` location. Duplicate USNs are dropped and the device's own `SERVER`
  header is what names it, otherwise the IP is shown.
- Settings → *Casting* shows the rule, the button and the summary. The probe runs
  **only** on that press — nothing is sent at startup — and the summary after the
  run says plainly that it is discovery only, that this build cannot cast and
  that no stream has been sent.
- `drain_cast_discovery` keeps the probe off the UI thread and reports failure
  as an error with the socket's own words.
- **Tests added:** four parser tests (a real-shaped renderer answer, other
  devices and malformed/oversized/`file:` answers refused, the exact probe
  shape, summary wording) and `step_22` (nothing probed by default, a result
  arrives through the drain, the flag clears, the honest summary).
- **Not proven:** no receiver was discovered on a real network, because that
  needs a live LAN and would be a network action this session did not take; no
  media has been cast, and no compatibility check exists to claim.

### Native 4K delivery and the frame-pacing correction (2026-11-01)

The 19.4-of-24-fps figure and the "40 ms of CPU YUV→RGB" explanation behind it were
both measurement artefacts: `mpv_render_context_render` blocks until the next frame
is due, so a paced run times the wait, not the work. Re-measured with
`untimed=yes` on the same M4 (libmpv 0.41.0, generated 3840×2160 24 fps fixtures,
`render_bench` with `SCREENROOM_PLAYER_TRACE=1`):

| Configuration | Render p50 | Paced delivery | Worst gap |
| --- | ---: | ---: | ---: |
| 4K H.264, 1920 cap (before) | 3.17 ms | 24.0 fps | 51.7 ms |
| 4K H.264, native 3840 (after) | 11.79 ms | 24.2 fps | 49.6 ms |
| 4K HEVC Main10, 1920 cap (before) | 2.74 ms | 23.8 fps | 51.3 ms |
| 4K HEVC Main10, native 3840 (after) | 5.97 ms | 23.7 fps | 48.8 ms |

`mistimed-frame-count` was 0 in every run, and scaler/dither options changed
nothing, which is what identified the wait. So `render::MAX_DIMENSION` is now 3840,
rows are 256-byte aligned for a direct `queue.write_texture`, `pack` is a single
memcpy, and browse previews cap at 960 px so a 4K card preview does not hold 4K
buffers. `SCREENROOM_RENDER_MAX` overrides the delivered size for A/B measurement.
Full detail and caveats: `HWDEC_EVIDENCE.md`.

Not proven: **on-screen** 4K presentation (the window was not opened), HDR tone
mapping (the software render API is single-plane RGB and does not map HDR), and the
owner's 3240×2160 Main10 sample, because `/Volumes/bleh` was not mounted.

### The one-off hold after a load, previously read as a periodic stall (2026-11-01)

A new `frame_stall` example reports every inter-frame gap with the playhead,
`avsync` and `mistimed-frame-count`, which turns "periodic ~290 ms stalls" into a
hold that can be placed in media time.

| Fixture | Runs | Delivered | The hold |
| --- | ---: | ---: | --- |
| 4K H.264, no audio track | 2 | 23.98 fps | none above 48 ms |
| 4K H.264, mono AAC | 6 | 23.25–23.40 fps | 277–302 ms at playhead 0.333 s |
| 4K H.264, stereo AAC | 3 | 23.40 / 24.00 fps | 290 ms in one run, none in two |

Ruled out by measurement rather than reasoning: `audio-buffer` (0.05–0.5 s, no
scaling), the CoreAudio device (`ao=null`, unchanged), mono channel layout
(`audio-channels=stereo`, unchanged), output size, hardware decode, and colour
conversion. mpv's own log names the side — `mpv_render_context_render() not being
called or stuck` appears twice during the hold — while `mpv_wait_event` returns in
microseconds, so libmpv is ready and this side is not.

Cause: `FILE_LOADED`, `PROPERTY_CHANGE`, `VIDEO_RECONFIG` and `PLAYBACK_RESTART`
issue synchronous libmpv property reads (`duration`, `hwdec-current`, fifteen
`track-list/*` reads, `dwidth`/`dheight`, `time-pos`) on the only thread that can
call `mpv_render_context_render`. mpv holds its core lock while it builds the 4K
video chain, so the read blocks, the due frame blocks with it, and about six
frames (0.25 s) are lost once per load. Phase timing behind
`SCREENROOM_PLAYER_TRACE` now attributes loop time to commands, events, render and
wait, which is how those handlers were named (393 ms and 231 ms of event time).

Servicing the render request before draining events did **not** help (302 ms
still), so loop order is not the fix. Next action: take `dwidth`, `dheight`,
`duration`, `hwdec-current` and `time-pos` from observed events instead of
synchronous reads, relaxing the `apply_property` gates for them, while
`position_ready` keeps waiting for a real `time-pos` so a fresh load cannot write
0 over a resume point. Full numbers: `HWDEC_EVIDENCE.md`.

### HDR10 measured on the software path (2026-11-01)

A generated 4K HEVC Main10 HDR10 fixture (`bt2020nc`/`smpte2084`, BT.2020, 1000-nit
MaxCLL) delivered **23.948 fps at native 3840×2160** with `videotoolbox-copy`,
CPU mean 269.7 %, RSS mean 528 MiB. One delivered frame matched mpv's own
libplacebo tone-mapped render to within 0.4 % of mean luma (129.2 vs 128.7) and
0.2 % at YHIGH, so the path receives a correctly tone-mapped SDR image already;
only the top of the range clips (255 vs 245.8).

The limitation is therefore not tone mapping but **HDR itself**: the output is
8-bit SDR RGB with no colour metadata, highlights are compressed into the SDR
range rather than presented at their mastered luminance, the curve is mpv's default
and unlabelled, and nothing here can ask macOS for an EDR surface. Not proven: a
real HDR10 film on a real HDR display, and what a person sees.

Gate G2's on-screen half is still open. The app's own path is now measured, hidden:
**21.8–22.0 fps delivered and uploaded at 3840×2160** against a 24 fps source, while
the same player headless delivers 24.00 fps — see the check-mode section below.
Displayed frames remain unobservable without a visible window, which needs an
owner-approved session.

### The post-load hold is fixed (2026-11-01)

The blocking reads named above are gone. `dwidth`/`dheight` are observed and
`update_target` uses the cached pair; `FILE_LOADED` marks metadata dirty instead of
reading duration, `hwdec-current`, the track list and dimensions; `PLAYBACK_RESTART`
takes the playhead from the last observed `time-pos`; a `track-list` change defers
the refresh to an iteration where no frame is pending. `position_ready` still only
becomes true once a real `time-pos` value has arrived, so a fresh load cannot
publish 0 over a resume point.

| Fixture, 8–20 s sampled | Before | After |
| --- | ---: | ---: |
| 4K H.264, mono AAC | 23.25–23.40 fps, 277–302 ms hold | 24.00–24.12 fps, no gap ≥ 150 ms |
| 4K H.264, stereo AAC | 23.40 fps with a 290 ms hold | 24.00 fps, no hold |
| 4K H.264, no audio | 23.98 fps | 24.12 fps |
| 4K HEVC Main10 HDR10 | 23.948 fps, worst gap 162 ms | 23.999 fps, worst gap 53 ms |
| 1080p H.264 | 24.0 fps | 24.0 fps |

The slowest remaining event is the deferred refresh at 12–28 ms, under one frame.
All 335 workspace tests pass, including the desktop acceptance suite; clippy and
rustfmt are clean. The owner's-media re-run stays **BLOCKED**: `/Volumes` holds only
`mothership` and `Recovery`, so no 3240×2160 Main10 sample was reachable.

### A preview no longer seeks blind (2026-11-01)

`Preview::start` loaded at `preview_position_ms(duration_ms)`, and a duration of 0
gives 30 s, so a file shorter than 30 s started past its own end: the player ended,
the preview was released and the card kept its poster. It worked only when the
library row already had a duration, which made
`step_17_a_preview_is_one_delayed_decoder_and_reduced_motion_stops_it` intermittent
and always failing when run alone. The preview now loads at the start and seeks
once, in `Preview::tick`, only after `snapshot.duration_ms()` is known, so the target
is inside the file. That test went from 15–16 s and failing to 1.9–3.2 s and passing
5 runs out of 5; the workspace is **352 tests, 0 failures**.

### The check mode exists, and it separates decoded from presented (2026-11-01)

Gate G2's on-screen half needed the application's own presentation path, not an
example. `SCREENROOM_PLAY_CHECK=<csv>` with `SCREENROOM_PLAY_CHECK_MEDIA=<file>`
and `SCREENROOM_PLAY_CHECK_SECONDS=<5..300>` now runs the real application, plays
that file through the real libmpv worker and the real `video_surface` upload, and
writes a machine-readable report plus a per-second samples file. Setting the check
variable forces the hidden window (`hides_its_window()`), so it cannot take focus,
switch Spaces or open anything visible; an invalid configuration is refused before
the window is built and exits with the reason on stderr.

```bash
SCREENROOM_PLAY_CHECK=/tmp/check.csv \
SCREENROOM_PLAY_CHECK_MEDIA="/path/to/film.mkv" \
SCREENROOM_PLAY_CHECK_SECONDS=10 \
  ./target/release/screenroom-desktop
```

What it reports, deliberately kept apart: `decoded_fps` and `delivered_fps` (frames
the player produced and the app picked up), `uploaded_fps` (calls to
`queue.write_texture`, the only place a frame reaches the GPU), `startup_ms`,
`measured_seconds`, `ticks_per_second`, worst and p95 frame gaps, CPU and RSS, the
source size and the render target. `presented_frames` is written as
**`unavailable`** with a note, because a hidden window gives this process no surface
present callback — an upload is not proof that anything was displayed.

It loads the file straight from its path (`Player::start` + `load_at`) instead of
scanning its folder as planned, so nothing touches the library or the media folder.
That deviation is what keeps the mode read-only and single-file.

| Release build, hidden window, 10 s | 4K H.264 mono | 4K H.264 stereo | 4K H.264 silent | 4K HEVC Main10 HDR10 |
| --- | ---: | ---: | ---: | ---: |
| startup to first frame | 618 ms | 621 ms | 443 ms | 449 ms |
| decoded / delivered fps | 22.010 | 21.816 | 21.826 | 21.962 |
| uploaded fps | 21.904 | 21.709 | 21.721 | 21.858 |
| worst / p95 gap | 70.9 / 58.0 ms | 67.2 / 58.3 ms | 67.4 / 59.3 ms | 72.0 / 61.5 ms |
| CPU mean | 116.3 % | 121.9 % | 130.9 % | 185.7 % |
| RSS mean | 402 MiB | 411 MiB | 380 MiB | 561 MiB |
| render target | 3840×2160 | 3840×2160 | 3840×2160 | 3840×2160 |

The same 4K mono file through `frame_stall auto-copy`, headless, reports **24.00 fps
in 10.00 s with no gap ≥ 150 ms**, and the application's loop ran at **48.3 Hz**, so
the 22 fps is neither decode nor tick-limited: the loss is in the app's own
frame-copy, upload and redraw path, about 2 fps of 24 — a reading the interleaved A/B
below corrects. Presentation on a visible window was unmeasured at this point; it is
counted in the next section.

Failure paths, each run against the release binary: missing media and a report path
inside the media folder are refused before any window exists (`play check refused:
…`, exit 0, no report written, nothing created in the media folder); a file that
decodes nothing finishes at the requested duration with `delivered_fps,0.000` and
`failure,no decoded frame arrived`. A run that starts but never finishes also fails
itself at `seconds + 30`. Thirteen unit tests cover the configuration rules, the CSV
shape, gap accounting, the first-frame window and the failure line.

Not proven at that point: displayed frames, because there was no callback for them;
and the owner's 3240×2160 Main10 file, because `/Volumes` still held only
`mothership` and `Recovery`. The first of those is now closed, below.

### Presented frames counted, and an installed bundle walked (2026-11-01)

`presented_frames` no longer has to be `unavailable`. The video primitive counts its
own draws (`video_surface` calls `play_check::note_draw` once per drawn frame), the
check reads that counter on every tick, and the report now separates three numbers:
`uploaded_fps` (texture writes), `drawn_fps` (times the video surface was drawn) and
`presented_fps` (the same draws, published only when the window was visible). A hidden
run keeps `presented_frames,unavailable` and says why.

The obvious implementation was wrong, and the measurement caught it. Subscribing to
`iced::window::frames()` to collect `RedrawRequested` fed those events back into
`update`, each redraw asked for another, and the application drew at **124.6 Hz** for
a 12.6 fps picture — an instrument that changed what it measured. The counter in the
primitive has no such loop. An interleaved A/B against the previous commit (three 4K
pairs, two 1080p pairs, same machine, same load) puts both builds inside each other's
noise: 4K delivered 19.212 / 18.992 / 18.936 before and 18.895 / 18.714 / 19.021
after; 1080p 26.435 / 26.449 before and 26.490 / 26.407 after.

That A/B also corrects the earlier reading of the ~2 fps gap. Today's 4K figure is
18.7–19.2 fps for both builds, `uploaded_fps` equals `delivered_fps` (exactly one
texture write per frame), and the machine's load average was 13–24 throughout. There
is no spare frame in the upload path; the difference from the 21.8–22.0 fps recorded
under a quieter machine is load, not code. What is genuinely left is the design change
named before: a GPU (zero-copy) render bridge, or presenting at the source rate
instead of the window's cadence.

A visible run needs the owner's go-ahead, so it is opt-in twice: the window is shown
only with `SCREENROOM_WINDOW=visible` **and** `SCREENROOM_ALLOW_FOREGROUND=1`. With
both set, a 4K run reported `drawn_fps,51.467` and `presented_fps,51.467` over 383
frames. CPU and RSS sampling also moved off the interface thread onto its own
one-second thread, so a measurement no longer forks `ps` between two frames.

The installed bundle was then walked with that approval: `dist/Screenroom.app`, a
scratch library of six generated files, Home → Library (six items, posters drawn) →
focus → Detail → Player (`bench-4k`, 0:04 / 0:06, audio `aac`) → Space to pause →
Settings → ⌘Q. The session opened while playing, **5 042 ms** was stored, and after
the quit the session count was 0 with the position still 5 042 ms. Its menu bar
enumerated as an Apple menu and a `Screenroom` menu, the menu-bar Quit mechanism
reported `routed` from the bundle binary itself, and the same bundle reported
`presented_fps,51.467` on a visible 4K run. Screens are in
`.artifacts/evidence/walk/`. The window did hold focus for the duration, which is what
was asked for, and the frontmost application was `Finder` afterwards rather than the
one that had it before.

### Two intermittent failures fixed, and the zero-copy path proven (2026-11-01)

Both failures that had been written off as load sensitivity were real, and both
were in the application rather than in the test harness.

**The mini-player step that has since been removed — a short viewing stored 0 ms.** `PlaybackTracker`
throttles checkpoints to one write per second, and the two stopping paths (leaving
the player, quitting) went through the same throttle. A
viewing shorter than a second therefore kept only its first checkpoint, which lands
at 0 ms, and `Continue Watching` resumed from the beginning. `record` now takes a
`force` flag that the stopping paths set, so the place actually reached is written
whatever the interval says. `stopping_a_viewing_stores_where_it_reached_even_inside_the_throttle`
holds it down.

**`tracks_are_enumerated_and_can_be_switched` — a track list with nothing
selected.** The metadata refresh ran on `FILE_LOADED`, which can arrive before
libmpv resolves its default audio selection, so the published list showed two audio
tracks and none selected; if no further `track-list` change arrived, it stayed that
way. The refresh now waits for `PLAYBACK_RESTART`, reads a list with no selection up
to six times until it settles (mpv resolves it within milliseconds of playback
starting), and keeps the previous list rather than publishing an unresolved one. A
deliberate "no audio" choice is remembered per kind so it is not mistaken for an
unresolved one.

Evidence: the failure reproduced 6 of 6 runs of the player suite at
`--test-threads=22` before the change and 0 of 6 after; that step failed on every
`cargo test --workspace` before and passed on three consecutive full runs after
(358 tests, 0 failures each; the test itself was deleted on 2026-11-02 with the
feature). `cargo fmt --all --check`, `cargo clippy --workspace
--all-targets -D warnings` and `python3 scripts/no_comments.py check` are clean.

**The GPU path is proven in isolation.** `examples/gl-path-probe/probe.m` renders
decoded frames through `MPV_RENDER_API_TYPE_OPENGL` into an `IOSurface` and reads
them back through a Metal texture created from that same surface: identical bytes,
no CPU copy. On generated 3840×2160 fixtures it delivers 22.91–23.74 fps at 26.7–32.1 %
CPU, against 18.7–22.0 fps at 116–186 % for the shipped software path. What it does
not prove — presentation through iced/wgpu, synchronisation without a per-frame
`glFinish`, HDR, and the owner's own file — is stated in `HWDEC_EVIDENCE.md` and
carried as P1-01/P1-02/P1-03/P1-11 in [`MACOS_REMAINING_WORK.md`](MACOS_REMAINING_WORK.md).

That document is now the canonical backlog: 25 functional requirements, 10 quality
requirements and the 15 acceptance steps, each with its status, its evidence, its
blocker and what would close it. It is the place to start the next session.

`preferred_bridge()` was also corrected: it answered `OpenGl` on every non-Windows
host while the player renders through libmpv's software API. It is now
`render_bridge()` and answers `Software`, which is what the code does.

### Two fullscreen and quit defects closed (2026-11-01, P2-02 and P2-04)

**P2-02 — an Escape during the fullscreen animation no longer dismisses the film.**
`toggle_fullscreen` used to invent the new state, ask the window for it, and let
`ModeChanged` correct it later. Two presses of Escape during the animation therefore
looked like "leave fullscreen" followed by "leave the player", and a second `f` could
reverse a change the window had not applied yet. The application now records the mode
it asked for with the instant it asked (`mode_change`), ignores a second request while
one is in flight, clears the record when the window reports that mode, expires it after
1.5 s so a lost report cannot wedge the key, and treats Escape during a transition as
"leave fullscreen" rather than "leave the film". Three tests hold it down:
`fullscreen_is_a_window_mode_the_player_can_leave_with_escape`,
`a_second_fullscreen_request_while_one_is_in_flight_is_ignored` and
`a_late_fullscreen_report_does_not_dismiss_a_film_that_was_being_left`.

**P2-04 — Dock Quit, AppleScript quit and logout run the same close path as ⌘Q.**
Re-pointing the menu bar's Quit item at `performClose:` only covered the menu item.
The Dock's Quit and `osascript -e 'quit app "Screenroom"'` send `-terminate:` to
`NSApplication` directly, which bypassed `Message::Close` and left the viewing to be
tidied up on the next launch. `quit::install_terminate_routing()` replaces the
implementation of `-[NSApplication terminate:]` once at start-up with one that closes
the key window, the main window, or — for a hidden window, which has neither — the
first closable window in `[NSApp windows]`. When no closable window exists it calls the
original implementation, so the application can never become unquittable. The close
request then arrives through the path that already stores the place reached and ends
the session.

`scripts/check-terminate-routing.sh` proves it without touching the desktop: it runs
the real binary with `SCREENROOM_WINDOW=hidden` and `SCREENROOM_TERMINATE_CHECK`, the
application asks AppKit to terminate itself, the swap turns that into a window close,
`Message::Close` writes `closed routed <seconds>` and exits. The script fails if the
process does not exit on its own or the receipt is missing.

## Second session: the GPU path, the online halves, and what review found

The first session ended with a canonical backlog and a probe that proved a path but
was not in the application. This session put it in the application, and then spent
most of its time on what that exposed.

### The 4K render path, measured in the application

`MPV_RENDER_API_TYPE_OPENGL` renders into a pool of `IOSurface`s; the desktop wraps
each one as a `wgpu::Texture` with `texture_from_raw` and `create_texture_from_hal`
and samples it in the existing fit shader, so no pixel crosses the CPU. The
application's own `SCREENROOM_PLAY_CHECK`, release build, 4K HEVC Main10 at 24 fps:

| | delivered fps | mean CPU | peak CPU | uploads |
| --- | ---: | ---: | ---: | ---: |
| OpenGL → IOSurface → wgpu | 24.39 | 13.95 % | 53.10 % | 0 |
| software renderer | 24.24 | 89.82 % | 190.50 % | 139 |

Four defects had to be found first, and every one of them looked like something
else:

1. **The picture was upside down.** `FLIP_Y` is 1 for an OpenGL *default*
   framebuffer and 0 for a texture-backed FBO that is read top-down. A symmetric
   test pattern cannot show this. The Bengali subtitle check did: the cue lit 173
   pixels in the top band where the software path lights them at the bottom.
2. **`ADVANCED_CONTROL` hard-deadlocked the mpv core.** libmpv's own header says a
   render thread that also calls other libmpv functions must not rely on it, and
   this worker calls `render` beside `mpv_command` and `mpv_get_property`. libmpv
   said so itself: `mpv_render_context_render() not being called or stuck`, with
   playback at 4 fps and the audio clock at 0.4× real time.
3. **A three-slot pool starved.** Once published frames were held by the snapshot
   and the widget, no slot was free, so `render` returned nothing *after* mpv had
   already been told the frame was consumed. The pool now grows on demand and
   reaps only when every slot is free, so the high-water mark survives playback
   instead of allocating an `IOSurface` per frame.
4. **`hwdec=auto-copy` is only needed by the software path.** The OpenGL path takes
   zero-copy decode, which is what moved CPU from 28 % to 14 % and fps from 22.5
   to 24.4.

`Frame` now carries an optional surface, `pixel` and `tight_pixels` normalise BGRA
to RGBA so both renderers hand out the same channel order, and
`SCREENROOM_RENDER=software` keeps the old path reachable, which is also the
fallback when the GL context cannot be created at start.

### What independent review changed

Five separate reviews ran against the real diffs. They found, and this session
fixed:

- **Backups lost data.** `TABLES` omitted `subtitles` and `source_bookmarks`, so
  every backup silently dropped imported subtitles and saved sources, and a restore
  could not commit at all once a subtitle existed for a file absent from the
  backup. Both tables are in `TABLES` now, with a test that populates all 21 tables.
- **A restore left work wedged.** Enrichment, job and conversion rows restored as
  `running` were never resumed until the next launch; the reset now happens inside
  the restore transaction.
- **A backup could fail a concurrent writer** on `SQLITE_BUSY` where it used to wait.
  The writer's grace is now bounded, raised for the copy and restored exactly, and
  the copy retries.
- **The HTTP reader could be crashed or stalled** by a hostile chunk size (an
  `usize` overflow that panics in debug and attempts a huge allocation in release)
  and by a server dribbling bytes, because only each `read` was timed. There is now
  an overall deadline, a trailer cap, and a test with a real socket for each.
- **A `failed` enrichment task could never be re-armed**, so "retry" was
  unimplementable. `failed` is no longer active, and re-enqueueing resets the
  attempt budget.
- **A cast endpoint could be starved** by any LAN peer, because a connection was
  charged against the slot limit before its token was checked; and a
  device-supplied `controlURL` could point at any internal host. Both are fixed.
- **Our own backup cancel reported `database_error`.** An interrupt is
  `SQLITE_INTERRUPT`; that is now mapped to `backup_cancelled` only when the cancel
  flag is set, so a real disk error that coincides with a cancel still reports
  itself.

### Reachable in the application

Settings gained *Online metadata* (the provider's own capability, an *Enrich the
library* action, and an artwork-retention switch that defaults to **do not store**),
*Find subtitles online* (the panel that lists matches with language/release/format
and offers to keep one; renamed from *Subtitle search* on 2026-11-03, and it now
returns before any network call when the file already has a track or a sidecar),
and *Casting* now lists receivers with *Cast the playing film* plus
pause/stop/disconnect. Every one of those is also in the keyboard action strip, and
an acceptance test asserts that the panels describe themselves honestly and that no
pass starts without a provider.

### What is still not true

HDR was removed on 2026-11-03 ([ADR-005](ADR-005_SUBTITLE_PROVIDER_AND_FEATURE_REMOVALS.md)):
SDR is the only output, the extended-range path described in the 2026-11-02
sections above is retired, and no HDR receipt will be produced again. That earlier
text stands as the record of what was built and measured
([`HDR_VERIFICATION.md`](HDR_VERIFICATION.md)). Picture-in-Picture is not offered
at all — neither OS-wide nor as an in-app mini player (withdrawn 2026-11-02). The
live subtitle provider has one recorded run (above, F15): an anonymous lookup, a
keyed search and a download through the redirect all answered. No media has been
played on a real cast receiver. The `NSAccessibility` bridge recorded as ADR-017
was removed on 2026-11-03, so no VoiceOver pass is run and no screen-reader tree is
published;
keyboard navigation, focus, reduced motion and subtitle styling remain. And the
owner's own 4K sample and five-minute battery observation still need the external
SSD and the owner.

## The owner's real library, and the real media measurements (2026-11-02)

The external SSD was connected for this session, so the checks that had been
waiting on it ran against the owner's actual media instead of generated fixtures.
Everything below is read-only: the media folder was scanned in place, no file was
written, moved or converted, and the library database lives in a temporary
directory.

**The recorded 3240×2160 sample is not on the drive.** `find` over the whole volume
finds no file at any 4K resolution, and a bounded ffprobe pass over the library
reports no stream wider than 1920. The library is **1080p**: mostly HEVC 10-bit,
with a large anamorphic group (1920×960 = 2:1, 1920×1040, 1920×800, 1920×804).
What is written below is therefore real-media evidence at 1080p, not the 4K
re-measurement the backlog had planned; that row is corrected rather than quietly
satisfied.

**Scan of the whole library.** `prepare_library` over
`/Volumes/bleh/movies & shows`: **937 files visited, 775 imported, complete, 0
issues**, producing **300 movies, 18 series and 475 episodes without a single work
left untitled**, 983 GB of media, and every item resolving in 0 ms with
`available = true`. Episode ordering from real folder names is right
(`Adolescence · S01E01` … `S01E04`).

**Playback of real media through the application**, release build, 12-second
window, by `SCREENROOM_PLAY_CHECK`:

| file | source | bridge | fps | mean CPU | uploads |
| --- | --- | --- | ---: | ---: | ---: |
| A Bronx Tale (HEVC 10-bit 1920×1080) | 23.976 fps | OpenGL | 22.58 | **13.11 %** | 0 |
| A Bronx Tale | 23.976 fps | software | 22.81 | 41.91 % | 260 |
| Green Book (HEVC 10-bit, 2:1 anamorphic) | 23.976 fps | OpenGL | 22.96 | **11.39 %** | 0 |

So on the material the owner actually watches, the GPU path delivers the source
frame rate at roughly a third of the CPU, and the 2:1 film is presented at
1920×960 — its own display aspect ratio — rather than letterboxed into 16:9.

**Poster extraction from real media.** One frame from three real films, at the
app's own scaling: `A Bronx Tale` → 1280×720 JPEG, `Green Book` → **1440×720**
(correct for 2:1), `28 Days Later` → 1330×720 (1.85:1). Aspect is preserved per
film rather than forced to a single poster shape.

## The providers can actually reach the internet now (2026-11-02)

Until this session the online halves of F05 and F15 were complete except for one
thing that made them unreachable: the only transport spoke plain HTTP and refused
`https`, and both providers default to an HTTPS base URL. That is fixed.

**The transport decision.** `rustls 0.23` with the `ring` provider and the
`webpki-roots` trust set, recorded as ADR-018. The platform HTTP client
(`NSURLSession`, WinHTTP) would give better OS trust-store and proxy behaviour, and
it was rejected for a reason worth writing down: neither accepts a caller-supplied
trust anchor, so the requirement that certificate validation is *real* could only be
demonstrated by weakening it. With `rustls` a test can inject a self-signed root and
also prove that an untrusted certificate **and** a correctly-trusted certificate
issued for the wrong name are both refused.

**What the transport guarantees.** One deadline spanning connect, handshake, write,
read and every redirect hop; every socket timeout re-armed from the remaining budget;
a response cap where a declared `Content-Length` over the cap is refused before any
body is read; redirects capped at 3, same scheme, same host, same port, GET only,
with an `https`→`http` redirect refused outright so a key can never be replayed to a
plaintext origin. There is no "insecure" mode at all; the only way to bypass
validation is a `#[cfg(test)]` constructor.

**It was proved against the real services, without a key.** On this machine:

```
https://api.themoviedb.org/3/movie/550            -> 401  "The provider rejected the configured key."
https://api.opensubtitles.com/api/v1/subtitles    -> 403  "The provider rejected the configured key."
```

A 401/403 from the real host is the handshake, the request and the response all
working, with the provider correctly refusing an unauthenticated call. What remains
is only a key. **Historical since 2026-11-03:** that second host is the provider
that was removed, and this dated receipt is kept as evidence about the TLS
transport, not as current behaviour. The transport is now proved separately against
TSDB in the commands recorded in
[`SUBTITLE_PROVIDER.md`](SUBTITLE_PROVIDER.md) §8 — which have not been run yet.

**Consequence.** `capability()` reports *configured* for a provider when a key is
present, instead of failing closed on a scheme it could not carry. Settings →
*Online metadata* and *Find subtitles online* are live the moment a key exists.

**Licensing.** `webpki-roots` is the first CDLA-Permissive-2.0 component in this
graph — a data licence over a certificate list, permissive, recorded in
`docs/LICENSING.md` because a release notice should name it.

### What the real library revealed (2026-11-02)

Scanning the owner's actual folder, not a fixture, produced three findings worth
keeping.

**The scanner is correct on a real exFAT drive.** The folder holds **162 macOS
AppleDouble files** (`._22 Jump Street - 2014.mp4` and so on) that look like media by
extension. The scan visited 937 entries, imported **775**, and the database contains
**zero** files whose path starts with `._` and **zero** hidden files: only `.mp4` and
`.mkv` are present. Importing those 162 stubs would have produced unplayable cards,
and it does not happen.

**Two of the owner's films are genuinely unreadable.** A decode probe over every
file found `10 Cloverfield Lane - 2016.mp4` and `Cloverfield - 2008.mp4` fail with
`moov atom not found` — truncated or unfinished downloads, not an application fault.
The application does the right thing: `artwork_check` reported *"10 Cloverfield Lane
has no usable picture: the file cannot be decoded at all (the app records it and
stops asking)"*, so they appear as unplayable rather than as blank cards, and the
scanner does not retry them forever. **The owner may want to re-download those two.**

**Poster extraction works on real media at real aspect ratios**: 1280×720 for a 16:9
film, 1440×720 for the 2:1 film, 1330×720 for the 1.85:1 film, each stored and
re-decoded by ffprobe.

## The interface rebuild (2026-11-02)

**Status: rebuilt, unit-tested and captured; the owner's look is PENDING.** The
full description, the breakpoint table and the verification commands are in
[`UI_REBUILD.md`](UI_REBUILD.md). This is the receipt.

The view layer was replaced; nothing below it moved. `app.rs` still owns the
library, scanning, the player, conversion, casting, providers, preferences and
checkpoints, the Rust core and the TV client are untouched, and behaviour parity
was a hard gate: no test was deleted, and the ones that asserted a changed layout
detail were rewritten and recorded.

| Area | What exists now |
| --- | --- |
| Shell | a 76 px bar (`shell.rs`, `BAR_HEIGHT`) over a 40 px gutter, stacked over the artwork on Home and Detail and a solid band elsewhere; wordmark `SCREENROOM`, shortened to `SR` below the Standard breakpoint; three destinations with compact labels below it; Refresh, search and the profile avatar; status and error copy as one line at the foot |
| Home | a full-bleed hero (the first Continue Watching title, else the first suggestion) over rails — Continue watching, Recently added, Films, Series — with an empty-library invitation instead of an empty grid |
| Library | title and count, search and sort, filter chips (All, Films, Series, Unwatched, Available offline, Watchlist, Favourites, genre when it exists) and an honest source line; a poster grid paging 24 tiles |
| Detail | a title page: backdrop, `FILM`/`SERIES` eyebrow, title, metadata, one primary action naming what it does (`Play`, `Play S01E03`, `Resume 25 min in`), two round list toggles, synopsis, facts, season selector and episode list |
| Player | full-bleed picture, top and bottom scrims, scrub and one control row; chrome recedes after 2.5 s without input **while playing** and never while paused (`CHROME_IDLE_AFTER`) |
| Settings | a 200 px section list (160 px at Narrow) and a 780 px content column over nine sections, rows separated by 1 px hairlines; the Playback section keeps the *Picture* card that says what the surface carries |
| Layout | one breakpoint table: Narrow < 900, Compact 900–1099, Standard 1100–1439, Wide 1440–1759, Ultra ≥ 1760, with 2/4/5/6/7 grid columns, 340–600 px heroes, 176–268 px rail cards and a 1760 px content cap at Ultra |
| Navigation | one back stack of `(Screen, FocusPos)`; Escape from Detail returns to the card it was opened from; Home is `(rail, column)` with the column clamped to a shorter rail |

**Measured and checked.** `apps/desktop/src/ui/` holds **54** unit tests
(breakpoint geometry and the content cap, nav labels and the wordmark, focus
clamping and restoration, the idle-chrome rule, control visibility by width,
source and count lines, chip and row semantics, and the shell composing at every
breakpoint including non-finite widths). The desktop library suite is **276**
tests. The rebuild session recorded `cargo test --workspace`, `cargo fmt --all
--check`, `cargo clippy --workspace --all-targets -- -D warnings` and
`python3 scripts/no_comments.py check` clean, and four hidden-window
`SCREENROOM_AX_CHECK` runs — Home, Library, Detail and Settings — with labels and
focus matching the app's own snapshot. **That receipt no longer exists**: the
`NSAccessibility` bridge and the `SCREENROOM_AX_CHECK` check were removed on
2026-11-03 (ADR-005), and the runs above are recorded as history, not as a check
anyone can repeat.

### Defects found by the rebuild, and fixed

Six were found, four by review and two by looking at captured screens. Each is
the kind that looks like something else.

| Symptom | Cause | Fix |
| --- | --- | --- |
| The navigation and the search mark disappeared over a bright frame | the bar was transparent over artwork, and then its scrim fell to a fifth of full strength by the middle of the bar | a vertical scrim that holds near-full strength across the bar and releases at its lower edge |
| Detail's primary action was an empty white pill | the round actions used `center_x(Length::Fill)`, which in iced *sets* the width, so two 44 px icon buttons stretched across the row and left the label no width | real squares for the round actions, and the primary says what it does |
| ↓ in the Library moved sideways | focus stepped by a column count computed from a 300 px target while the grid drew its columns from `Breakpoint` | both read the same source, and a test asserts they agree across six widths |
| Enter in Settings could fire a control that was not on the page | the keyboard walked a flat list of every action in every section while the screen draws one section | the cursor walks the drawn rows, and the accessibility tree publishes the same rows |
| VoiceOver could open a different film than the one it announced on Home | the accessibility tree published items in a different order from the rail model that resolves them | it publishes the rails' own order, and the invitation only when it is drawn |
| The Library source line read "scanned scanned 51 minutes ago" | the caller passed a phrase that already began with "scanned" into a line that prefixes one | the line is composed once, and the scrollbar that had become a bright 16 px stripe is a faint gutter |

The hero and the cards also drew no picture until Home and Detail gained an
artwork channel; the screens could not fetch posters by themselves, and a hero
that falls back to a gradient looks intentional enough to hide the defect.

### What this does not prove

- **Nobody has said it looks right.** Captures were taken at 900, 1280, 1440 and
  3440 during the rebuild and inspected there, but they live in `.artifacts/`
  (Git-ignored) and are not in a checkout, so a reviewer with a clone cannot see
  them. The owner's look is **PENDING** as P2-06 in
  [`MACOS_REMAINING_WORK.md`](MACOS_REMAINING_WORK.md).
- **The chrome has not been watched receding** in a visible window. The rule is
  unit-tested; the wiring is not the same claim.
- **There is no window-size override.** The window opens at 1280×800 and records
  each resize; reproducing a capture at another width means resizing it while it
  is visible, which needs the owner's `SCREENROOM_ALLOW_FOREGROUND=1`.
- **Accessibility is still window-level.** Roles, labels, values and focus are
  published; geometry is the window's (ADR-017).
- **No CI run exists** — see below.

## CI, release and the ship script (2026-11-02)

**Status: written, parsed and exercised locally; no GitHub Actions run has
happened.** The one-time setup — runner labels, secrets, the Tizen profile — and
the exact status are in [`RELEASE_PIPELINE.md`](RELEASE_PIPELINE.md).

The device lanes are self-hosted because every target there is a device on the
owner's home LAN, and a cloud runner cannot reach the television. **Packaging is
not**: the `macos` job moved to GitHub's hosted `macos-latest`, because a `.app`
and a `.zip` need nothing on the LAN, this repository is public so hosted minutes
cost nothing, and the self-hosted gate it used to wait for
(`vars.SCREENROOM_MACOS_RUNNER`) was never set — the job skipped on every push and
no release was automated at all. The hosted job installs `mpv` and `ffmpeg` from
Homebrew, builds `dist/Screenroom.app` ad-hoc signed (there is still no Developer
ID or notarisation credential, so nothing is notarised), and uploads the `.zip`
and its manifest fragment. It no longer installs the app on any machine.

| Piece | What it does | Evidence |
| --- | --- | --- |
| `.github/workflows/ci.yml` | on a pull request, a push to `main`, and as a callable workflow: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `python3 scripts/no_comments.py check`, then the TV's `pnpm typecheck` and `pnpm test` | parses; every command is one this repository already runs locally |
| `.github/workflows/release.yml` | calls CI, packages macOS on hosted `macos-latest` with `scripts/release.sh macos`, merges the per-platform fragments into `manifest.json` (version, tag, generated-at, and per artefact platform, arch, version, filename, sha256 and size), signs it with Ed25519 when the key exists, and publishes `v<version>` | parses; the manifest merge and signing paths were exercised locally; the packaging command was run end to end on this Mac |
| `release.yml` → `publish` job | runs on `ubuntu-latest` only for a pushed `v*` tag or a manual run from `main` with `publish` ticked, and refuses a tag that does not equal `v<version>`. **A push to `main` publishes nothing**; it keeps the artefacts for 14 days instead | the gate and the tag check are written and linted but have never been executed by GitHub |
| `release.yml` → `windows` job | declared, and skipped unless `vars.SCREENROOM_WINDOWS_RUNNER == 'true'` | **skipped, not greenwashed**: no Windows runner exists, so the Windows gate stays BLOCKED |
| `scripts/ship.sh` | the owner's command without GitHub: builds and replaces the installed Mac app, and reaches the Windows host over SSH and the TV over sdb at the same time, printing one result per device; a powered-off device is reported as skipped, and only the always-on Mac can fail the run | the three lanes, the configuration file and `SCREENROOM_SHIP_STRICT` are exercised locally; the device lanes depend on the devices being up |

Two claims are deliberately not made. **No job, step or expression has been
executed by GitHub**, so the hosted `macos` lane, the `publish` gate and the TV
lane are unrun configuration rather than a green build. And **no Windows build
exists** — the job is written so that enabling a runner is the only missing step,
and the gate remains blocked until a Windows machine runs it.

Nothing was added to any workflow that makes it an always-on service: no paid
certificate (the signing key is optional and the release publishes unsigned
without it), no automatic deployment outside the owner's own machines, and no
publish on a normal push. One open constraint belongs to the release gate rather
than to the pipeline: the hosted bundle embeds Homebrew's **GPL-2.0-or-later**
`libmpv`, which [`LICENSING.md`](LICENSING.md) classifies as development-only, so
a publish that uploads this build needs the LGPL-only libmpv decision first.

## Automatic media discovery: the core half (2026-11-02)

**Status: the defects are fixed and the pure logic exists; the desktop watcher
that feeds it is not wired yet.** Nothing here has run against a watcher, and the
macOS and Windows monitoring research below is design evidence, not a shipped
feature.

Three defects that already existed are fixed, each with a test that fails without
the fix:

| Defect | What went wrong | Fix |
| --- | --- | --- |
| A stale mount point could empty the library | a drive's mount point survives as an empty folder, so a genuinely complete walk found nothing and withdrew every file: rows kept, everything disabled | a source records the volume's own identity; a walk may only withdraw when that identity still matches |
| Every rescan re-read every file | about 1.28 GB read for ten thousand unchanged films | a file whose size and modification time still match is no longer fingerprinted |
| One transaction per ingested file | a two-thousand-file copy was two thousand commits | a scan's ingests share one transaction |

The new `watch` module holds the part of discovery that decides what a signal
means, with no I/O: the media and rejection lists, how a burst collapses to one
change per path inside a quiet window, and how changes become a bounded plan of
files to ingest, forget or treat as moved. A rename becomes one relocation rather
than a delete and an add. `scan_paths` ingests named files through the same
machinery as a walk, so a watcher will hand over paths rather than a second
pipeline.

What the measurements and the prior art settled, and what they rule out:

- Event-driven watching with a cheap stat-only reconciliation is the design. A
  stat pass is about 1 ms for 775 files, against a 10 ms walk; an idle watcher
  costs 0.028 % of one core and 6.4 MB. Polling a 10 ms walk every few seconds is
  both slower to notice a change and keeps an external SSD awake.
- Plex waits a 60-second quiet window and Jellyfin restarts a 60-second delay on
  every event with a 12-hour backstop scan; Kodi's core has no watcher at all. No
  serious media server treats continuous polling as its primary mechanism.
- `notify` 8.2.1 never reads the completion byte count on Windows, so a buffer
  overflow drops events silently. That is why a periodic stat pass is part of the
  design rather than an optimisation.

**The desktop half is wired** (`apps/desktop/src/watch.rs`): one `notify` 8.x
thread per authorised folder, a 1.2-second quiet window, batches handed to the
scanner by path, launch and wake and ten-minute reconciliation passes, and a
thread that reports when it stops rather than dying silently. `notify` is a
dependency now.

**Proven live:** `scripts/verify-watch.sh` copies a clip into a watched folder and
the library holds it 4.9 seconds later with no rescan. **Not proven:** a removal is
corrected by the reconcile pass rather than immediately (P2-08), a folder moved
into the library is not noticed (its rename has no media extension), a real
FSEvents callback cannot be observed from inside `cargo test`, and Windows stays
blocked until it runs there.

---

## Portable CI on a hosted Linux runner (2026-11-02)

The self-hosted macOS pipeline in `docs/RELEASE_PIPELINE.md` only runs when the
owner's MacBook is on. `.github/workflows/ci-linux.yml` is the gate that needs
nothing of the owner's: `pull_request`, `push: main`, `workflow_dispatch` and
`workflow_call`, with `contents: read` and no deploy, release or signing step. It
is the reusable gate `release.yml` calls, and `ci.yml` stays the owner's
self-hosted gate, so the two live in separate files and overwriting one cannot
break the other.

| Job | What it runs |
| --- | --- |
| Rust format, clippy and secret scan | `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `gitleaks git` 8.30.1 from a SHA256-verified download |
| Rust tests | `cargo test --workspace --all-targets` with `ffmpeg`, `libmpv-dev` and an ALSA `null` sink |
| TV client tests | Node 24, `pnpm@12.16.1`, `pnpm install --frozen-lockfile`, `pnpm typecheck`, `pnpm test` |

`apps/tv` is installed with pnpm because `apps/tv/pnpm-lock.yaml` is the lockfile
that exists. `actions/checkout`, `actions/setup-node` and `actions/cache` are
pinned to commit SHAs; the cache covers `~/.cargo` and `target/`.
`.github/dependabot.yml` keeps actions, cargo and the TV package current weekly.

**Evidence (macOS ARM64 host, `rustc 1.99.1`).** `cargo fmt --all --check` clean;
clippy exit 0, the only notice being the pre-existing `block 0.1.6` future
incompatibility inside the iced stack; `cargo test --workspace --all-targets`
**697 passed, 0 failed** in 86 s with a warm target directory; `pnpm typecheck`
clean in 9.9 s; `pnpm test` **228 passed, 0 failed** in 1.5 s.

**Evidence (Linux).** The same three Rust commands, the same packages, an
unprivileged user, in an `ubuntu:24.04` container: clippy clean and **666 passed,
0 failed, 0 ignored**, exit 0. The desktop package's own suite runs in a second step with
`--test-threads=1`: on the hosted x86_64 runner that binary died with SIGSEGV while
libtest ran its tests in parallel, which an arm64 `ubuntu:24.04` container does not
reproduce, so the serial form is the one the runner can finish.

Six tests are skipped by name in the workflow:
five measure something a headless runner cannot provide (mpv's audio clock in real
time, a one-second on-screen position on a shared vCPU, macOS launchd PATH
behaviour) and one is a 5 ms poll that can miss a whole 1,200-file scan on a fast
SSD. All six run on the macOS host. A seventh failure the first hosted run found —
`a_long_scan_publishes_what_it_has_before_it_finishes` counting commits through the
process-global `commit_observer` while libtest ran the binary's tests in parallel —
was fixed at the source instead: every test that installs the observer now holds a
test-only mutex, and six consecutive `cargo test -p screenroom-core --lib` runs
agree. Twenty-five further
tests are `cfg(target_os = "macos")` and do not exist on Linux.

**Fixed on the way.** `cargo fmt --all --check` used to abort on a clean checkout
with `couldn't find file … src/design/ds_pinned.rs`, because rustfmt resolves
module declarations without running `build.rs` and that file is generated and
Git-ignored; the declaration now carries `#[rustfmt::skip]`. The workspace also
failed clippy on Linux on two macOS-only items and an unused parameter, and two
path tests assumed the host folded case (a Windows volume does whatever the host
does; a case-sensitive volume test granted a directory it never created). All
fixed without changing behaviour.

## Notes that were comments (2026-11-02)

The repository forbids comments in source, and a merged pull request had added
eighty-two of them across eight files. They were stripped with the repository's
own lexer (`python3 scripts/no_comments.py strip`), and the ones that carried a
reason rather than a restatement are recorded here.

- `apps/desktop/src/design/mod.rs` declares the pinned token module with
  `#[rustfmt::skip]` because the generated file is Git-ignored: a clean checkout
  does not have it, and rustfmt resolves module declarations without running
  `build.rs`, so it would abort the format check on a file that is not there.
- `apps/desktop/src/quit.rs` reaches its Quit-item lookup from the macOS menu
  walk only, so on every other target it is reached from tests alone.
- `apps/desktop/src/video_surface.rs` caps its texture cache only on the macOS
  surface-import path; other targets upload the CPU frame and never import.
- `crates/screenroom-core/src/library/catalog.rs` has a process-global commit
  observer for tests. Two tests installing one would count each other's commits
  and clear each other's counter, so every test that uses it holds a lock for its
  duration.

## Real media, on the owner's own drive (2026-11-02)

`scripts/verify-media.sh` runs the application's own scanner and player against
the mounted library and reports what it proves. On the owner's 916 GB exFAT drive:

- 775 media files across 69 folders were scanned in **2.7 s** at **10 MB** resident,
  complete, with no issues.
- They resolved to **318 titles**: 300 films and 18 series holding 49 seasons and
  475 episodes. Every file belongs to a film or an episode; every title has a year;
  no title is listed twice.
- The 21 files the walk does not import are macOS `._` AppleDouble sidecars on the
  exFAT volume, which the scanner rightly refuses.
- Real playback of a 1080p HEVC film measured **24.09 fps** sustained through
  VideoToolbox, 14 % CPU, 72–86 MB resident, first frame at 1.0 s.
- The drive is only ever read. Nothing in this check writes to it.
