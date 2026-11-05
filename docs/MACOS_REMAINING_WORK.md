# macOS remaining work

The execution backlog for finishing Screenroom on macOS (Personal Edition,
[ADR-004](ADR-004_PERSONAL_EDITION_SCOPE.md)). Written **2026-11-01** against
`main` at `e56e7ac`, then extended by the work recorded at the end of this file.

This document is the single list of what is left. `CAPABILITY_MATRIX.md` says what
each capability can do, `IMPLEMENTATION_STATUS.md` carries the history and the
measurements, `MACOS_ACCEPTANCE_CHECKLIST.md` carries the 15 human steps. Nothing
here duplicates a report; each row points at the evidence instead.

**How to read a status.** `COMPLETE` means implemented, reachable in the running
application and verified by something that actually ran. `PARTIAL` means something
real exists and something named is missing. `UNVERIFIED` means the code exists but
the check that would settle it has not run. `BLOCKED` means a named external thing
is missing. A passing test never stands in for a person looking at a picture or
hearing a film; those live in the acceptance table and stay `PENDING` until the
owner says otherwise.

---

## 1. How this audit was done, and what it does not cover

Read and cross-checked against the code, not against the plan: `docs/PRD.md`,
`docs/DESIGN.md`, `docs/CAPABILITY_MATRIX.md`, `docs/IMPLEMENTATION_STATUS.md`,
`docs/MACOS_ACCEPTANCE_CHECKLIST.md`, `docs/MACOS_ACCEPTANCE_EVIDENCE.md`,
`docs/ARCHITECTURE.md`, ADR-002/003/004, `crates/screenroom-player/HWDEC_EVIDENCE.md`,
`apps/desktop/src/app.rs` (messages, screens, key routing, Settings actions),
`apps/desktop/src/app/acceptance.rs`, the player worker and renderer, the packaging
scripts, and `cargo test --workspace`.

**Executed in this session:** `cargo test --workspace` three times end to end
(358 tests, 0 failures each), the acceptance suite eight times, the player worker
suite six times at full parallelism, `cargo fmt --all --check`,
`cargo clippy --workspace --all-targets -D warnings`,
`python3 scripts/no_comments.py check`, and the OpenGL/IOSurface/Metal probe on four
4K fixtures.

**Not covered here, deliberately:** the Windows target and the Samsung TV target.
They are different devices with their own gates (`WINDOWS_VALIDATION.md`,
`TIZEN_DEVICE_HANDOFF.md`) and this document is macOS only. Their source-level
parity is noted where a macOS change could disturb it.

**What no agent can settle from here:** whether a poster looks like its film,
whether speech matches lips, whether fullscreen fills the display, whether a 2:1
picture looks wide, pulling a cable out of an SSD, and the fan/battery cost of a 4K
film on the owner's lap. Those are the owner's steps and they are named as such.

---

## 2. Completion report

Counted from the 25 product requirements (F01–F25), the 10 quality requirements
(Q00, Q00B, Q01–Q08) and the 15 macOS acceptance steps: **50 tracked rows**.

**Updated 2026-11-01, second session; amended 2026-11-03.** The GPU render path is
integrated and measured; the online halves of F05 and F15, casting, backup
threading, the scanner transaction, fullscreen Escape and Dock Quit are implemented
and tested; the HDR path and the VoiceOver bridge were removed on 2026-11-03
([ADR-005](ADR-005_SUBTITLE_PROVIDER_AND_FEATURE_REMOVALS.md)).

| Measure | Result |
| --- | --- |
| Applicable macOS requirements | **49** (24 functional + 10 quality + 15 acceptance steps; F18 PiP was withdrawn on 2026-11-02) |
| Complete | **45** |
| Partial | **3** |
| Missing | **0** |
| Broken | **0** |
| Unverified | **1** (acceptance step 8: 4K on battery) |
| Blocked | **2 rows carry a named blocker** (F05 live, F23), plus F15's live half, which is PENDING on a run with the owner's key |
| Not applicable to macOS | **0** of these 50 |
| Outstanding implementation tasks | **1 open P1** (P1-02's fence — a correctness margin, not a defect) and **0 P2**; P1-03 (HDR) and P1-09 (VoiceOver) were removed on 2026-11-03; P1-07's live calls are PENDING on a run with the owner's key; P1-12 closed 2026-11-03: the owner played titles with Wi-Fi off and it behaved |
| Outstanding human/device tasks | **9** (8 owner observations + 1 owner hardware run) |

The four measures the brief asks to keep apart:

| | Value | Meaning |
| --- | --- | --- |
| **Implementation** | 31 of 34 product/quality areas implemented in full (91 %), 3 partial (9 %), 0 absent | every feature area has working code behind it |
| **Automated verification** | 33 of 34 areas have an automated check that ran | the exception is the live provider calls (F05, F15), which need an owner key |
| **Human/device acceptance** | 7 of 15 checklist steps closed; 7 half-closed; 1 needs the owner and the hardware | see §5 |
| **Everyday readiness** | **usable now** for 1080p and 4K at the source frame rate: 24.4 fps at 13.95 % mean CPU on 4K HEVC Main10 through the OpenGL path, against 24.2 fps at 89.8 % on the old one | SDR is the only output (HDR removed 2026-11-03); no PiP at all (withdrawn 2026-11-02); no online enrichment without an owner key, and the live subtitle provider is PENDING its four commands |

**What changed in the second session, measured rather than asserted.** The
application's own `SCREENROOM_PLAY_CHECK`, same 4K HEVC Main10 fixture, 24 fps
source, release build:

| | delivered fps | mean CPU | peak CPU | GPU uploads |
| --- | ---: | ---: | ---: | ---: |
| OpenGL → IOSurface → wgpu | **24.39** | **13.95 %** | 53.10 % | **0** |
| software renderer | 24.24 | 89.82 % | 190.50 % | 139 |

The target was `delivered_fps ≥ 23.0`, `uploaded_fps = 0` and CPU ≤ 60 %, and the
OpenGL path meets all three on this machine.

## 3. What is actually working, in the order a person meets it

1. **Install.** `scripts/package-macos.sh` produces `dist/Screenroom.app`, ad-hoc
   signed inside-out, with libmpv bundled and all 20 dependencies checked by
   `scripts/verify-bundle.sh`. It launches outside Cargo and outside a terminal.
2. **First run.** Home says "Add a folder", the native picker grants it, the scan
   walks the
   folder on a thread, the status row counts up and cards appear while it is still
   walking. Relink, rescan, cancel and remove all exist.
3. **Library.** One card per work, posters extracted from the media itself and
   stored as bytes in SQLite, runtimes probed in the background, search, kind and
   genre filters, four sorts, paging that reaches every row, watchlist, favourites,
   watched/unwatched, available-only.
4. **Detail and playback.** Seasons and episodes in the right order, Enter plays the
   highlighted episode, resume from the stored place, pause/seek ±10 s/scrub,
   fullscreen, audio and subtitle track pickers read back from libmpv, subtitle size
   and position, up-next with a cancelable countdown,
   manually pinned skip-intro/credits markers, keep-open previews on browse.
5. **Around the film.** Local NFO metadata and genres, imported subtitle files kept
   and reused offline, remux/audio-fix/h264 conversion into a separate folder with
   progress, cancel and verification, backups and restores that ask twice.
6. **Settings.** Profiles, autoplay, auto-skip, reduce motion, languages, folders,
   saved sources with an honest external handoff, receiver discovery, and an action
   strip that runs every one of those from the keyboard.

The two things that keep it from being *finished* are the 4K cost and the advanced
features that need something the machine does not have.

### The interface rebuild (2026-11-02)

The five screens were rebuilt as a view-layer change, and the detail — the shell,
each screen, the breakpoint table and the verification commands — is in
[`UI_REBUILD.md`](UI_REBUILD.md). In one paragraph: a 76 px bar over the content
instead of a 110 px control panel, Home a hero over horizontal rails, Library a
poster grid with filter chips and an honest source line, Detail a title page with
one action that names what it does, a player whose chrome recedes after 2.5 s of
no input while playing and never while paused, and Settings as sections with
hairline rows. The layout reads one breakpoint table (Narrow < 900, Compact
900–1099, Standard 1100–1439, Wide 1440–1759, Ultra ≥ 1760); the navigation drops
its labels and shortens the wordmark below Standard, and the grid runs from two
columns to seven.

**No requirement changed state.** F09, F11, F17, F21 and F22 are the rows whose
screens moved, and each still rests on the same acceptance steps and the same
automated checks it did before; the screens' geometry gained 54 unit tests in
`apps/desktop/src/ui/`, and the desktop library suite is 276 tests. Home and
Detail draw retained posters, Detail's primary action is real rather than a label,
and Settings keeps its keyboard action strip.

**What is not proven:** the interface has not been looked at by the owner, and the
session's own window captures at 900, 1280, 1440 and 3440 live in `.artifacts/`
(Git-ignored, absent from a checkout), so nobody else can re-inspect them. The
player's chrome receding has not been watched in a window. That is P2-06 below.

---

## 4. Requirement-by-requirement status

### 4.1 Functional (F01–F25)

| ID | Capability | Impl | Automated | Human | Blocker |
| --- | --- | --- | --- | --- | --- |
| F01 | Sources: grant, label, scan, rescan, relink, remove, connection state | COMPLETE | `step_1`, `step_12`, `a_folder_that_moved_keeps_its_history_until_it_is_relinked` | steps 1, 12 | — |
| F02 | Discovery: incremental, bounded, cancelable, restartable, partial results | COMPLETE | `step_1` (1,200 files, cards mid-walk), scan tests in `screenroom-core/tests/scanner.rs` | step 1 | — |
| F03 | Structured library: work/series/season/episode/edition/file, unknowns kept | COMPLETE | `described_episodes`, `screenroom-core/tests/library.rs`, TV contract tests | step 2 | — |
| F04 | Identity matching: filename/folder, local NFO, manual match, protected override | COMPLETE (local only) | parsing tests, `step_18` (NFO), manual-match tests | step 2 | online IDs absent (F05) |
| F05 | Whole-library enrichment | **COMPLETE (core) / BLOCKED (live)** — TMDB-shaped provider, durable queue, rate limits, backoff, reconciliation precedence, artwork retention gate; nothing is fetched until a key is configured | `tests/providers.rs` (28), `tests/providers_http.rs` (5), drain tests | step 2 | owner key + TLS transport + retention decision |
| F06 | Offline artwork bytes | COMPLETE | 21 posters re-decoded by ffprobe, 200 candidates readable, `a_retained_poster_is_cached_for_its_card` | step 2 | — |
| F07 | Video playback | **COMPLETE (SDR)** — 1080p and 4K at the source frame rate (24.4 fps, 13.95 % CPU, zero uploads). **HDR was removed on 2026-11-03** ([ADR-005](ADR-005_SUBTITLE_PROVIDER_AND_FEATURE_REMOVALS.md)): SDR is the only output, so there is no HDR row, setting or receipt, and the 2026-11-02 extended-range measurements stand only as history | `SCREENROOM_PLAY_CHECK`, `gl_render_check` | — | — |
| F08 | Profiles and progress | COMPLETE | `step_10`, `step_14`, checkpoint tests | steps 10, 14 | — |
| F09 | Home and Continue Watching | COMPLETE | `step_11`, `step_13` | step 11 | — |
| F10 | Next-episode autoplay | COMPLETE | `step_9`, gap/season tests | step 9 | — |
| F11 | Search, filters, sort, paging | COMPLETE | 830-row library paging, `step_18` genre | — | — |
| F12 | Watchlists and favourites | COMPLETE | toggle/filter tests | — | — |
| F13 | Audio tracks and subtitles | COMPLETE | `step_5`, `step_21`, track tests, and a Bengali cue proved to be drawn into the picture | step 5 (that it reads correctly on screen) | — |
| F14 | Skip intro and credits | COMPLETE (manual markers only) | marker tests, auto-skip guard tests | — | — |
| F15 | Subtitles: file first, then The Subtitle DB | **COMPLETE and verified in the running application (2026-11-03)** — embedded track → sidecar → The Subtitle DB, the only online provider ([ADR-005](ADR-005_SUBTITLE_PROVIDER_AND_FEATURE_REMOVALS.md)). A real Bengali subtitle was downloaded, applied and drawn over the playing picture; the cue changed with the playhead; after a restart with the provider unreachable the kept copy still applied and rendered, with no socket open; the panel is operable and unclipped at 640×480 | store tests, `step_21`, resolution-order tests, plus the screenshots and run recorded in [`SUBTITLE_PROVIDER.md`](SUBTITLE_PROVIDER.md) §8 | Windows run | owner |
| F16 | Local recommendations | COMPLETE | recommendation tests | — | — |
| F17 | Browse previews | COMPLETE | `step_17` (dwell, one decoder, reduced motion, cleanup) | look at a card | — |
| F18 | Picture-in-Picture | **WITHDRAWN (2026-11-02)** — not offered: no OS PiP and no mini player | — | — | not wanted |
| F19 | Casting | **COMPLETE (session) / UNVERIFIED (device)** — discovery, description parsing, session-scoped stream endpoint, SOAP AVTransport control, disconnect recovery, all proven against an in-process fake receiver | `casting`/`cast_stream`/`cast_session` tests (36) | — | no real receiver on this network (P1-08) |
| F20 | Backup and recovery | COMPLETE | `step_13`, restore-safety tests | step 13 | — |
| F21 | Settings and secrets | COMPLETE | `settings_are_operable_without_a_pointer`, every action reachable | step 15 | — |
| F22 | Failure and availability states | COMPLETE | `step_12` half, error-code tests | step 12 | — |
| F23 | Source directory | **PARTIAL** — owner-written bookmarks with provenance; no shipped list | `step_19` | — | no verified list or operator permission (ADR-011) |
| F24 | Acquisition handoff | COMPLETE — http/https/ftp only, to the system browser | `step_19`, handoff tests | — | — |
| F25 | Local conversion | COMPLETE | `step_16` (real remux, verification, original untouched) | watch the panel once | — |

### 4.2 Quality (Q00–Q08)

| ID | Requirement | Impl | Evidence | Gap |
| --- | --- | --- | --- | --- |
| Q00 | Installed local app, no browser, no server, no localhost runtime | COMPLETE | bundle walked end to end; `verify-bundle.sh` | — |
| Q00B | Dropout Design System, pinned, private | COMPLETE | `ds_pinned.rs` generated at a pinned commit; theme string visible; neutral fallback | the DS sync is machine-local; a clean checkout builds without it |
| Q01 | Offline: library, profiles, artwork, resume, playback with the network off | COMPLETE by construction (no network code at all; only the browser handoff and an opt-in SSDP probe) | executed with Wi-Fi off on 2026-11-03; playback, rail scrolling and embedded subtitle switching all worked | PASS (owner) |
| Q02 | Performance: startup, first frame, 4K, memory, no runaway jobs | **COMPLETE (SDR)** | startup ~1.2 s; 4K 24.4 fps at 13.95 % mean CPU with zero uploads; paging 13.7 ms first page and 78 ms grouped last page at 10k rows; backups and restores run off the UI thread. HDR was removed on 2026-11-03, so its 2026-11-02 figures are history only | single-run CPU samples |
| Q03 | Fidelity: aspect, anamorphic, timebase, honest codec claims | COMPLETE | `step_7`, `anamorphic_video_renders_at_its_display_aspect_without_letterbox_rows` | the *look* of step 7 is the owner's |
| Q04 | Portability: one monorepo, isolated platform adapters | COMPLETE | layout, `cfg` boundaries | — |
| Q05 | Security and privacy | COMPLETE for the code this repository writes; six advisories remain in the iced dependency tree | argument vectors (no shell), root canonicalisation, `contained_media_path` tests, no credentials anywhere; OSV over `Cargo.lock`: one memory-safety advisory (`lru` 0.16.4) and five unmaintained notices, all in the iced/wgpu/metal stack that iced 0.14 pins, none fixable inside it | P2-09, blocked upstream |
| Q06 | Durability: migrations, transactions, no silent resets | COMPLETE | migration tests, integrity check after migrate, backup/restore | — |
| Q07 | Accessibility and design quality | **COMPLETE for keyboard and focus; screen-reader support removed** | keyboard journey, Settings action strip, contrast tokens, reduced motion, subtitle scale/position read-back. The `NSAccessibility` bridge and its headless receipts were removed on 2026-11-03 ([ADR-005](ADR-005_SUBTITLE_PROVIDER_AND_FEATURE_REMOVALS.md)); no VoiceOver pass is run | — |
| Q08 | Honest reporting | COMPLETE | this document set; `preferred_bridge()` corrected this session | — |

### 4.3 macOS acceptance steps (1–15)

| # | Step | Automated | Human |
| --- | --- | --- | --- |
| 1 | Add the folder, cards before the scan ends | PASS | **PENDING** (OS permission dialogs) |
| 2 | Posters, cover art, runtimes, one card per series | PARTIAL | **PENDING** (does it look right) |
| 3 | Series → ↓↓ → Enter plays episode 3 | PASS | — |
| 4 | Space, ←/→, and speech in sync | PASS (mechanism) | **PENDING** (listen) |
| 5 | Audio and subtitle pickers change the language | PARTIAL | **PENDING** (see and hear it) |
| 6 | `f`, fullscreen, the way out, header returns | PARTIAL | **PENDING** (the fill) |
| 7 | 2:1 anamorphic looks wide and unstretched | PARTIAL | **PENDING** (the look) |
| 8 | 4K HEVC for five minutes on battery | **UNVERIFIED** | **REQUIRED** (owner + the SSD) |
| 9 | Up next, Cancel, Play now | PASS | — |
| 10 | Mid-film ⌘Q, reopen resumes | PASS | — |
| 11 | Home mid-film stops the sound and keeps the place | PASS | — |
| 12 | Unplug the SSD, replug, Rescan | PARTIAL | **PENDING** (pull the cable) |
| 13 | Back up, change, restore | PASS | — |
| 14 | Second profile survives a quit | PASS | — |
| 15 | Keyboard-only journey | PARTIAL | **PENDING** (readability) |

---

## 5. The backlog

Nothing in this list is speculative: each row is either a PRD requirement, a
recorded accepted limit, or an open defect. Priority is about *everyday use*, not
effort.

### P0 — Essential

**Empty.** No known defect stops the application from launching, playing media,
preserving data or completing a core workflow. This is a claim about the evidence
gathered on 2026-11-01, not a promise: any owner observation that fails becomes a
P0 immediately, is written down in `IMPLEMENTATION_STATUS.md`, and is fixed rather
than reclassified.

### P1 — Required

---

#### P1-01 · Present 4K frames through iced without a CPU copy — **CLOSED**

- **Requirement:** F07, Q02 (G2 on-screen half).
- **Status:** COMPLETE. The application renders through `MPV_RENDER_API_TYPE_OPENGL` into a pool of `IOSurface`s and samples them as `wgpu` textures via `texture_from_raw`/`create_texture_from_hal`, with no CPU copy. Acceptance met on the owner's M4: 24.39 fps delivered, 13.95 % mean CPU, 53.10 % peak, `uploaded_fps = 0`, against 24.24 fps at 89.82 % mean and 190.50 % peak for the software path. Implementation in `crates/screenroom-player/src/{gl_render,surface}.rs` and `apps/desktop/src/video_surface.rs`, selected in `worker.rs`; `SCREENROOM_RENDER=software` forces the old path, which is otherwise the fallback when the GL context cannot be created.
- **Four defects found while integrating it, each now fixed and each the kind that looks like something else:** `FLIP_Y` must be 0 for a texture-backed FBO read top-down (with 1 the picture is upside down, and a symmetric pattern hides it — the Bengali subtitle check found it by lighting pixels in the top band); enabling `ADVANCED_CONTROL` hard-deadlocks the mpv core when the render thread also calls other libmpv functions, which this worker does, and libmpv said so itself (`mpv_render_context_render() not being called or stuck`); a three-slot pool starves once published frames are held, so the pool now grows to sixteen and reaps only when all slots are free; and `hwdec=auto-copy` is only needed by the software path, so the OpenGL path uses zero-copy decode, which is what took CPU from 28 % to 14 %.
- **What is missing:** the application still converts every frame on the CPU and
  uploads 33 MB per 4K frame. Measured: 18.7–22.0 fps delivered at 116–186 % CPU
  (`SCREENROOM_PLAY_CHECK`), against a probe that delivers 22.9–23.7 fps at
  27–32 % CPU through OpenGL → IOSurface → Metal (`HWDEC_EVIDENCE.md`,
  "The zero-copy GPU path, measured").
- **Where:** `crates/screenroom-player/src/render.rs` (software renderer),
  `crates/screenroom-player/src/worker.rs` (`present`),
  `apps/desktop/src/video_surface.rs` (texture upload), `apps/desktop/src/app.rs`
  (`Message::Tick`, frame publication).
- **Why it matters:** 4K is the owner's one known playback defect, and the CPU cost
  is the battery and fan cost of every 4K film. It is also what stands between
  "plays" and "plays like a media player".
- **Work:**
  1. Add an OpenGL render backend behind the existing `render.rs` boundary: headless
     `CGL` context, `IOSurface` sized to the delivered frame, `CGLTexImageIOSurface2D`,
     FBO, `mpv_render_context` created with `MPV_RENDER_API_TYPE_OPENGL`.
  2. Publish the surface, not pixels: a `Frame` variant that carries the
     `IOSurface` id/size/stride instead of a pooled buffer.
  3. In `video_surface`'s `Primitive::prepare`, wrap the surface as a
     `wgpu::Texture` via `device.as_hal::<wgpu::hal::api::Metal>()` +
     `wgpu_hal::metal::texture_from_raw` + `create_texture_from_hal`, and draw it
     with the existing fit shader.
  4. Keep the software renderer as the fallback and as `SCREENROOM_RENDER=software`,
     selected when the GL context or `IOSurface` cannot be created.
- **Depends on:** P1-02 (synchronisation and lifetime) for correctness. The old
  HDR dependency is gone: HDR was removed on 2026-11-03 (ADR-005).
- **Acceptance:** `SCREENROOM_PLAY_CHECK` on the four generated 4K fixtures reports
  `delivered_fps ≥ 23.0` for a 24 fps source, `uploaded_fps = 0` (no texture
  write), CPU mean at or below 60 %, and the probe's frame still matches the
  software path's picture within 1 % mean luma.
- **Automated verification:** extend the existing check with an assertion on
  delivered fps and CPU; a hidden-mode run must also prove the fallback path still
  works when OpenGL is refused.
- **Human verification:** checklist step 8 (smooth motion, battery percentage).
- **Note:** do not remove the software path, and do not claim the win until the
  app's own numbers move.

---

#### P1-02 · Synchronise OpenGL and Metal without a per-frame stall

- **Requirement:** F07, Q02.
- **Status:** PARTIAL — `glFinish` after every frame is what guarantees the published surface is complete before `wgpu` samples it, so the picture is correct, but it serialises the pipeline, and it does not order Metal's reads against the *next* GL write into a recycled slot. A published surface is now held one extra frame by `GlRenderer`, so a slot cannot be reused in the same frame it was shown, which bounds the hazard at one frame period rather than removing it. The measured cost is inside budget (24.39 fps against a 41.6 ms frame), so this is a correctness margin, not a performance blocker.
- **What is missing:** a fence so Metal samples only after the GL writes land, and
  a lifetime rule so a surface is not recycled while the GPU still reads it.
- **Where:** the new GL backend (P1-01) and `video_surface`.
- **Why it matters:** `glFinish` per frame caps the pipeline at one frame of CPU/GPU
  round-trip; without it, tearing or stale frames appear instead. A wrong answer
  here is a visible bug, not a slow one.
- **Work:** two or three `IOSurface`s in a pool; `glFlush` then a shared
  `MTLSharedEvent` (or `mpv_render_context_report_swap` plus a queued Metal command
  buffer's `waitUntilCompleted` boundary) to signal completion; a "surface in use"
  flag cleared by the completion handler.
- **Depends on:** P1-01.
- **Acceptance:** with `glFinish` removed, 30 minutes of continuous 4K playback shows
  no torn or repeated frame in a frame-by-frame capture, and `mistimed-frame-count`
  stays 0.
- **Automated verification:** a headless test that renders N frames and asserts every
  surface generation is consumed exactly once.
- **Human verification:** checklist step 8's smoothness half.

---

#### P1-03 · Show HDR10 at its mastered luminance — **REMOVED (2026-11-03)**

- **Requirement:** none. HDR was removed completely on 2026-11-03: no HDR mode, no
  extended-range output, no preference or setting, no HDR receipt. **SDR is the
  only desktop output.** See
  [ADR-005](ADR-005_SUBTITLE_PROVIDER_AND_FEATURE_REMOVALS.md).
- **Status:** removed, not deferred. Nothing here is owed, and the owner's look at
  an HDR film is no longer wanted.
- **Left as history:** the 2026-11-01 and 2026-11-02 measurements — the 16-bit PQ
  surface, the `Rgba16Float` swapchain, the extended sRGB layer, the 406-nit target
  and the hidden-window runs — are kept as written in
  [`HDR_VERIFICATION.md`](HDR_VERIFICATION.md) and
  [`ADR-019_HDR_PRESENTATION.md`](ADR-019_HDR_PRESENTATION.md), both retired in
  place. They record what existed until this date, not what ships.
- **Everything below in this section is the original 2026-11-02 record, kept
  verbatim.** Read it in the past tense: it is why the path existed and what it
  measured, not a description of the application today.
- **What the panel reports** (Swift probe, no window): macOS 27.0.1 on Apple M4,
  built-in display in a 2940×1912 mode at scale 2.0,
  `maximumPotentialExtendedDynamicRangeColorComponentValue` **2.0**,
  `maximumExtendedDynamicRangeColorComponentValue` **1.0** (nothing EDR on screen),
  `maximumReferenceExtendedDynamicRangeColorComponentValue` 0.0, no reference
  preset engaged. `rgba16Float` is renderable on the device.
- **What was measured, headless** (`examples/hdr_check.rs`): the surface reads back
  `'RGhA'` `0x52476841`, 8 bytes per pixel, 30 720-byte row at 3840, FBO `0x881a`,
  **1 261 distinct green values against 246 for eight bits**, 138 175 samples off
  the 8-bit grid, peak luma **380.6 nits = 1.875× the 203-nit reference white**
  against a display ceiling of 2.0, and **84 875 of 169 641** sampled pixels above
  reference white. The `float8` control (2 105 distinct, zero codes above 1.0)
  keeps "off the eight-bit grid" from being confused with HDR. The SDR fixture is
  unchanged: `BGRA`/`0x8058`/256 distinct/zero off-grid.
- **The layer, read back from a real `CAMetalLayer`**
  (`vendor/iced_wgpu/examples/metal_extended_range.rs`): half float swapchain →
  `MTLPixelFormatRGBA16Float` (115), `wantsExtendedDynamicRangeContent` **true**,
  colour space **`kCGColorSpaceExtendedSRGB`**; the eight-bit control stays at 81,
  false, `kCGColorSpaceSRGB`.
- **The whole application, hidden, release build:** `SCREENROOM_HDR=extended`
  reports its own swapchain as `Rgba16Float` and delivers **24.506 fps at
  14.27 % mean CPU** with zero uploads, against 24.626 fps at 14.17 % for the SDR
  control — the same picture rate as the recorded baseline (24.4 fps, 13.95 %).
- **How it works, in one line:** a vendored `iced_wgpu` (semantic diff in
  `window/compositor.rs` only) selects `Rgba16Float` and re-tags the layer
  extended sRGB; libmpv tone maps and gamut maps to `203 × headroom` nits; the
  picture shader converts PQ to nits, scales by reference white and applies the
  sRGB transfer function, so highlights above 1.0 are the extended range.
- **A defect found and fixed on the way:** the picture was imported through an
  sRGB view and the decoded value written into a non-sRGB target, so a mid-grey
  code of 128 reached the display as 55. The UI was unaffected; the picture was
  visibly too dark. The import is now non-sRGB and a test asserts 128 → 128. This
  is a deliberate, measured change to the shipped SDR picture and is recorded in
  ADR-019.
- **Still open:** (1) the owner's look at an HDR10 film — the delivered headroom
  cannot be read headlessly, because `maximumExtendedDynamicRangeColorComponentValue`
  only rises while EDR content is on screen; (2) gamut is BT.709, because the
  drawable is shared with the UI and has one colour space; lifting that needs the
  picture's own layer, which ADR-019 rejects for now. `SCREENROOM_HDR` stays off
  by default until (1). HLG deliberately stays on the SDR path.
- **Also unproven and labelled as such:** the nits are the ST 2084 EOTF applied to
  delivered codes, not a display measurement, and the fixture is generated noise
  tagged PQ rather than a graded master.

#### P1-04 · Close the remaining owner acceptance session

- **Requirement:** every acceptance step marked PENDING in §4.3.
- **Status:** BLOCKED on the owner (8 observations, one session).
- **What is missing:** eyes, ears and a cable pull; nothing an agent may do without
  taking the desktop.
- **Where:** `MACOS_ACCEPTANCE_CHECKLIST.md` ("The remaining manual session").
- **Why it matters:** 7 of the 15 steps are closed by machinery alone; the rest are
  the difference between "the code path ran" and "the film looks right".
- **Work:** run the ordered session (package → add folder → posters → listen →
  tracks → fullscreen → anamorphic → 4K on battery → unplug → keyboard), record each
  result under "Owner session" in `IMPLEMENTATION_STATUS.md`.
- **Depends on:** nothing.
- **Acceptance:** every step has a recorded pass or a defect with what was seen.
- **Automated verification:** none possible; that is the point.
- **Human verification:** all eight.

---

#### P1-05 · macOS Picture-in-Picture (F18) — **WITHDRAWN, nothing to do**

- **Requirement:** none. F18 was withdrawn on 2026-11-02: the owner does not want
  Picture-in-Picture, so neither the OS feature nor the in-app mini player that
  stood in for it is offered.
- **Status:** removed, not deferred. There is no task, no blocker and no code. ADR-016
  is marked WITHDRAWN and nothing else depends on it.
- **Left as history:** a mini player did exist between 2026-11-01 and 2026-11-02, with
  its own acceptance step; both were deleted. The SDK constraint recorded in ADR-016
  (a wgpu surface is neither an `AVPlayerLayer` nor an `AVSampleBufferDisplayLayer`,
  so real PiP would need a second display path) remains true and remains unused.

---

#### P1-06 · Whole-library online metadata enrichment (F05) — **core CLOSED, live blocked**

- **Requirement:** F05.
- **Status:** COMPLETE (core and transport) / BLOCKED only on an owner key. A TMDB-shaped provider, a durable `enrichment_queue` on schema 9, per-minute rate limiting, `Retry-After` handling, exponential backoff, a bounded transport with an overall deadline and a hostile-chunk guard, reconciliation precedence (manual beats NFO beats provider, and NFO only yields when the owner asks), and an artwork-retention gate that defaults to *do not store*. An unconfigured provider reports itself as not configured and makes zero requests. 33 integration tests, plus a real TLS transport (`rustls` + `webpki-roots`) that was proved against the live hosts without a key: `https://api.themoviedb.org/3/movie/550` answered **401** and `https://api.opensubtitles.com/api/v1/subtitles` answered **403** — the handshake, the request and the response all working and the provider correctly refusing an unauthenticated call. (The second host is the provider that was removed on 2026-11-03; that half of the receipt is history and is kept as evidence about the transport.) Redirects are capped at 3, only same-scheme same-host same-port, and an `https`→`http` redirect is refused. Settings has an *Online metadata* panel and a keyboard action for it.
- **What is missing:** the online half. The offline half (local NFO) is complete.
- **Where:** `crates/screenroom-core/src/nfo.rs` (the origin column and
  `metadata_origin` are where a provider lands), `apps/desktop/src/app.rs` Settings.
- **Why it matters:** without it, a library of badly named files keeps whatever the
  filename parser and any NFO says.
- **Work:** the owner picks a provider and accepts its terms (TMDB's offline image
  caching terms are the open question the PRD names); then a keyed, rate-limited,
  queued client with visible/recent items first, `metadata_origin = provider`, manual
  matches never overwritten, and bytes stored offline like posters today.
- **Depends on:** **owner action** — a TMDB key, and a decision on retaining
  provider images (the app defaults to *do not store*).
- **Acceptance:** a scan of a folder of badly named files enriches titles, years,
  overviews and posters while offline reading still works; a manual match survives a
  re-enrichment.
- **Automated verification:** recorded fixtures parsed by a pure function; the live
  call is exercised once with the owner's key and never in CI.
- **Human verification:** does the result match the film.

---

#### P1-07 · Subtitles: resolve from the file, then The Subtitle DB (F15) — **store and order complete, live PENDING**

- **Requirement:** F15.
- **Status:** COMPLETE for the resolution order and the store. **PENDING for every
  live call.** The provider is The Subtitle DB and only that
  ([ADR-005](ADR-005_SUBTITLE_PROVIDER_AND_FEATURE_REMOVALS.md),
  [`SUBTITLE_PROVIDER.md`](SUBTITLE_PROVIDER.md)); OpenSubtitles is removed.
- **The order, which is the requirement:** an embedded track in the file, then a
  sidecar beside the media, and only when neither exists may the app ask TSDB. The
  control is *Find subtitles online* and it returns before any network call when a
  track or a sidecar is present. There is no second online service and no fallback
  chain.
- **What is implemented and tested:** the bounded store (4 MiB, text-only, eight
  formats, provenance), import, *Use* through `sub-add`, removal that leaves the
  original file untouched (`step_21`). The file-first gate is a binding rule of
  the change; proving the transport is never reached when a track or a sidecar
  exists is its acceptance condition, not a receipt that exists yet.
- **What was run, on this host (2026-11-03):** an anonymous lookup, a keyed
  `/v1/by-search`, a `/get/{id}` download through its 302, the downloaded Bengali
  subtitle drawn over the picture with the cue changing with the playhead, a
  restart with the provider pointed at a dead address where the kept copy still
  applied and rendered with **no socket open**, and the panel at 640×480. The
  commands, observations and screenshots are in
  [`SUBTITLE_PROVIDER.md`](SUBTITLE_PROVIDER.md) §8.
- **Where:** `crates/screenroom-core/src/library/provider_subtitles.rs`,
  `provider_http.rs`, `subtitles.rs`, and the player panel in `app.rs`.
- **Acceptance:** on a film with no track and no sidecar, a search returns matches
  with language, release and format; keeping one stores it, hands it to libmpv and
  reuses it offline; a 404 lookup reads as no match, a 404 download as unavailable,
  and a 401 says which key file is missing — never which key.
- **Automated verification:** store and order tests, plus parsers over synthetic
  fixtures. The live calls stay manual and explicitly invoked.

---

#### P1-08 · Casting session with a real receiver (F19) — **session CLOSED, device unverified**

- **Requirement:** F19.
- **Status:** COMPLETE (session) / BLOCKED (no device on this network). A LAN survey on
  2026-11-02 found **no MediaRenderer anywhere**: an mDNS service enumeration showed
  only this Mac (AirPlay/RAOP, `model=Mac16,12`), a /24 sweep found six hosts of which
  every non-router host exposes no media port and no mDNS name, and the owner's
  Samsung TV did not advertise at all — it was not powered on. The app's own
  `discover()` also could not *send* its search from a terminal-launched binary
  (`EHOSTUNREACH 65`, macOS attributing local-network access to the terminal bundle),
  which is why the failure message now says plainly that nothing was looked for
  rather than reporting an empty network, and why the packaged app now carries
  `NSLocalNetworkUsageDescription`. **No receiver has ever received a byte from
  Screenroom.** Description fetching and parsing, `AVTransport`/`RenderingControl` resolution with relative-URL handling, a session-scoped HTTP endpoint that serves one opaque token with correct `Range`/`206`/`416` responses, a hard lifetime, one-client accounting, `SetAVTransportURI`/`Play`/`Pause`/`Seek`/`Stop`/`GetTransportInfo` SOAP control reading the receiver's own state back, disconnect recovery, and a `Drop` that closes the listener. 36 tests drive the whole session against an in-process fake receiver, including a byte-exact `Range` fetch and a receiver that refuses `Play`. **No media has ever been played on real hardware** and the capability text says so.
- **What is missing:** a reachable permitted stream, DESCRIBE/SetAVTransport control,
  codec compatibility, disconnect recovery — and a receiver to test against.
- **Where:** `apps/desktop/src/casting.rs`.
- **Why it matters:** the PRD asks for honest capability, and "found a receiver" is
  not "can cast".
- **Work:** with a receiver present, decide whether a temporary session-scoped LAN
  endpoint is acceptable (it is currently refused); if yes, confine it to one file,
  one session, off by default, and close it on disconnect.
- **Depends on:** **device** — a DLNA/Chromecast-class receiver on the LAN, plus the
  endpoint decision.
- **Acceptance:** a film plays on the receiver with working pause/seek and recovers
  from a dropped connection; with no receiver, the app says so and offers nothing
  false.
- **Automated verification:** the existing parser and refusal tests stay green; the
  session logic gets a fake-transport test.
- **Human verification:** watch it on the receiver.

---

#### P1-09 · Accessibility beyond the keyboard (Q07) — **REMOVED (2026-11-03)**

- **Requirement:** Q07, keyboard/focus/reduced-motion/subtitle-styling half only.
- **Status:** the VoiceOver-specific work is removed, not deferred. The
  `NSAccessibility` tree bridge (`apps/desktop/src/accessibility.rs`), its
  announcements and its `SCREENROOM_AX_CHECK` receipt were deleted on 2026-11-03
  ([ADR-005](ADR-005_SUBTITLE_PROVIDER_AND_FEATURE_REMOVALS.md)). The
  [`VOICEOVER_PASS.md`](VOICEOVER_PASS.md) procedure is retired and no further
  VoiceOver receipt is produced.
- **What still stands, unchanged:** keyboard navigation on every screen, the
  Settings action strip, a deterministic focus order with restoration, the
  reduced-motion setting, and subtitle size and position read back from libmpv.
- **What is not offered, stated rather than implied:** the application publishes no
  screen-reader tree, so VoiceOver sees what AppKit infers from a window with no
  accessibility children — nothing it can drive.
- **Left as history:** the bridge was verified headlessly on all five surfaces before
it was removed — 0 controls before it attached, then 19 on Home, 23 on Library, 30
on Detail, 33 on Settings and 16 on the Player, with labels, values, enabled state,
`AXPress`, slider adjustments and focus matching the application's own snapshot,
and the known gap that every element carried the window's frame rather than
per-widget geometry. Those numbers describe a tree that no longer exists; the dated
receipts stay in [`IMPLEMENTATION_STATUS.md`](IMPLEMENTATION_STATUS.md).

#### P1-10 · Prove Unicode and Bengali subtitles render — **CLOSED 2026-11-01**

- **Requirement:** F13, Q07 (subtitle legibility).
- **Status:** COMPLETE. `a_bengali_subtitle_is_drawn_into_the_picture` plays a black
  film with an imported Bengali cue and finds **173 lit pixels in the subtitle band
  where the same film without the cue has none**; the frame was also dumped and
  inspected, and the glyphs are real Bengali rather than boxes.
- **Two things learned while doing it, both recorded because they will confuse the
  next person:** libmpv auto-loads a sidecar named after the video
  (`<name>.bn.srt`), so a "no subtitle" control run needs a sidecar with a different
  name; and `--sub-file` as a startup option did not take effect in these probes,
  while `sub-add … select` — what the application actually calls — did.
- **Left for the owner:** whether the size, position and contrast read well on the
  screen (checklist step 5).

---

#### P1-11 · Re-measure the owner's real media — **CLOSED, and the row was wrong**

- **Requirement:** F07, Q02, acceptance step 8.
- **Status:** COMPLETE for what the drive actually holds, and the premise is
  corrected rather than quietly satisfied. **There is no 3240×2160 file on the
  SSD.** `find` over the whole volume at any depth finds no 4K-named file, and a
  bounded ffprobe pass over the library reports no stream wider than 1920. The
  library is 1080p: mostly HEVC 10-bit, with a large anamorphic group (1920×960 =
  2:1, 1920×1040, 1920×800, 1920×804). The recorded "3240×2160 sample" is no
  longer on this machine.
- **What was measured instead, on the owner's real films through the application**
  (release build, `SCREENROOM_PLAY_CHECK`, 12-second window):

  | file | source | bridge | fps | mean CPU | uploads |
  | --- | --- | --- | ---: | ---: | ---: |
  | A Bronx Tale (HEVC 10-bit 1920×1080) | 23.976 fps | OpenGL | 22.58 | **13.11 %** | 0 |
  | A Bronx Tale | 23.976 fps | software | 22.81 | 41.91 % | 260 |
  | Green Book (HEVC 10-bit, 2:1 anamorphic) | 23.976 fps | OpenGL | 22.96 | **11.39 %** | 0 |

  On the material the owner actually watches, the GPU path delivers the source
  rate at roughly a third of the CPU, and the 2:1 film is presented at 1920×960 —
  its own display aspect ratio — rather than letterboxed.
- **The whole library scans clean.** `prepare_library` over the real folder: 937
  files visited, 775 imported, complete, **0 issues**, producing 300 movies, 18
  series and 475 episodes with no work left untitled, 983 GB of media, every item
  resolving in 0 ms.
- **Poster extraction works on real media**: 1280×720 for a 16:9 film, **1440×720**
  for the 2:1 film, 1330×720 for a 1.85:1 film — each at its own aspect.
- **Still the owner's:** step 8's five-minute battery observation, which needs a
  4K file this library does not contain. If a 4K sample is wanted, it has to be
  provided.

#### P1-12 · Prove offline behaviour — **CLOSED (owner, 2026-11-03)**

- **Requirement:** Q01, PRD acceptance criterion G.
- **Status:** CLOSED. The owner turned the radio off on 2026-11-03 and reports that
  playback, rail scrolling and switching embedded subtitles all worked. That is the
  owner's own observation, recorded here; the exact steps are the ones this section
  used to list — packaged app, an existing library, Wi-Fi off, browse, play, quit,
  reopen, resume — and the behaviour was as expected.
- **The independent mechanism check, which still runs on demand:**
  `scripts/check-no-network.sh` plays a file for eight seconds and samples every
  socket the process holds, 5 times a second. Result: **37 samples, 0 samples
  holding any socket at all** — no TCP, no UDP, not even a listener — while the
  report confirms frames were delivered. Every network call the app can make (an
  enrichment pass, a subtitle lookup, a cast session, receiver discovery) is behind
  an explicit press, and the provider tests assert that an unconfigured provider
  issues zero requests.
- **What is still not proven:** offline behaviour on Windows and on the TV, which
  need those devices. The macOS half is closed.

#### P1-13 · A verified source directory (F23)

- **Requirement:** F23.
- **Status:** BLOCKED by ADR-011: no verified nationwide list, operator permission or measured ISP reachability exists, so the directory contains only what the owner types and nothing claims otherwise.
- **What is missing:** a list someone verified, with operator permission and measured
  reachability.
- **Where:** `crates/screenroom-core/src/library` (`source_bookmarks`),
  `app.rs` Settings → Saved sources.
- **Why it matters:** the PRD wants help *finding* permitted sources, not just a
  place to type one.
- **Work:** research is an owner/legal question first; only ship an entry that has a
  verified provenance record. Until then the current behaviour stands.
- **Depends on:** **owner decision and research**.
- **Acceptance:** every shipped entry has a recorded provenance, protocol, access
  requirement and last verification result.
- **Automated verification:** schema/validation tests already exist.
- **Human verification:** none.

### P2 — Refinement

#### P2-01 · Move backup and restore off the UI thread — **CLOSED**

- **Requirement:** Q02 (no freeze during heavy work). **Status:** accepted limit.
  **Where:** `app.rs` `backup_now`/`inspect_restore`/`restore_now`.
- **Why:** 1.5 MB is instant; a 50,000-item library would freeze the window.
  **Work:** run the same staged `VACUUM INTO`/validate/restore sequence on a worker
  with progress, like the scan and conversion jobs already do. A restore that
  replaces the open library needs the same "one job at a time" rule the scan uses.
  **Acceptance:** the window keeps drawing during a backup of a synthetic 50k
  library, and a failed restore still leaves the current library untouched.
  **Automated:** a test that a backup started while a scan runs is queued, not
  interleaved.

#### P2-02 · Escape during the fullscreen animation (recorded as L6) — **CLOSED**

- **Requirement:** F07, step 6. **Status:** known limit.
  **Where:** `app.rs` key routing (`ToggleFullscreen`, `ModeChanged`).
  **Work:** ignore an Escape that arrives while the mode change is in flight, or
  queue it until `ModeChanged` settles. **Acceptance:** a rapid `f` then Escape
  leaves fullscreen and stays in the film. **Automated:** the existing mode tests
  extended with an in-flight case.

#### P2-03 · Partial-scan stat checks outside the write transaction (L4) — **CLOSED**

- **Requirement:** F02, Q06. **Status:** known limit; a sleeping drive can stall the
  UI briefly. **Where:** the scanner's partial-scan path.
  **Work:** collect the stat results first, then open the transaction.
  **Acceptance:** a scan of an unresponsive root does not hold a write transaction
  while it waits. **Automated:** a scanner test with a simulated slow filesystem.

#### P2-04 · Close the session on Dock Quit, logout and shutdown — **CLOSED**

- **Requirement:** F08. **Status:** accepted limit; the loss is bounded by the 1 s
  checkpoint and the stale session is closed on the next open. **Where:**
  `app.rs` `Message::Close`, `route_quit`.
  **Work:** install a handler for `applicationShouldTerminate`-equivalent events
  (winit's `LoopExiting`/`CloseRequested`) that runs the same stop path.
  **Acceptance:** Dock Quit mid-film stores the position and closes the session.
  **Automated:** `scripts/check-quit-routing.sh` extended to the Dock path if the
  platform allows it headlessly.

#### P2-05 · Re-check large-library paging on the grouped query — **CLOSED**

- **Requirement:** F11, PRD criterion D (10,000 rows; 50,000 is the stretch).
  **Status:** measured 44/64 ms at 10k and 194/487 ms at 50k; no regression since.
  **Work:** re-run `library_bench` after any query change and record the numbers.
  **Acceptance:** first page under 100 ms at 10k.

#### P2-06 · Look at the rebuilt interface at five widths — **PENDING the owner**

- **Requirement:** Q03, Q08, and the "does it look right" halves of acceptance
  steps 2, 6 and 15.
- **Status:** PENDING. The geometry is unit-tested at all five breakpoints
  ([`UI_REBUILD.md`](UI_REBUILD.md)), and captures were taken at 900, 1280, 1440
  and 3440 during the rebuild, but they are Git-ignored local artefacts and no
  owner look has happened.
- **What is missing:** eyes. `SCREENROOM_WINDOW_WIDTH` and
  `SCREENROOM_WINDOW_HEIGHT` now open the window at a chosen size, so a
  breakpoint can be captured without a resize; the review below still needs a
  person.
- **Where:** `apps/desktop/src/ui/` (all of it), `scripts/capture-window.sh`.
- **Why it matters:** the rebuild exists because the previous composition read as
  a control panel. "Reads as a media application" is a judgement no test makes.
- **Work:** launch the packaged build, resize from 900 to 3440, and look at Home,
  Library, Detail, the player's chrome and Settings. Capture what is wrong in
  words rather than a screenshot of anything else on the display.
- **Acceptance:** each screen reads correctly at Narrow, Standard and Ultra; focus
  is unmistakable without a ring; no card clips its own title; no screen ends in a
  dead band.
- **Automated verification:** `cargo test -p screenroom-desktop --lib ui::` — but
  it cannot settle the question.
- **Human verification:** the owner, once.

#### P2-07 · Posters are blank for the first half-minute on a cold library — **CLOSED**

- **Requirement:** Q03 and Q08; a cold launch must not look broken.
- **Status:** CLOSED. Extraction now runs on three bounded workers instead of one,
  the visible cards are queued first, and a card without its poster yet draws a
  waiting face rather than an empty one.
- **Evidence:** a capture of Home at 14 seconds, on the owner's 318-title library
  with its artwork table emptied first, shows all six rail posters drawn. Throughput
  depends on what is being decoded, so both measurements are recorded: on that real
  library the example drew thirty posters at about 50 a minute with the first at
  653 ms, while on a scratch library of thirty short clips the same example reached
  320 a minute. A whole library is therefore not instant — the first screen is what
  the queue and the waiting card are for.
- **Where:** `apps/desktop/src/artwork.rs`, `apps/desktop/src/ui/components/poster.rs`,
  `queue_artwork` in `apps/desktop/src/app.rs`.
- **Automated verification:** `cargo test -p screenroom-desktop --lib ui::components`
  and the pool's own tests; `cargo run --release -p screenroom-desktop --example
  artwork_bench -- <library.db> 30` for the timing.
- **Human verification:** the capture above.

#### P2-08 · A deletion reaches the library on the reconcile pass, not immediately — **CLOSED**

- **Requirement:** automatic discovery must not leave a film listed that is no
  longer there, and must never delete history.
- **Status:** CLOSED. The cause was not a lost job. macOS reports a deletion as a
  remove event followed by that same path's metadata and content events, and the
  batch collapse kept the last non-removal, so the removal was discarded before any
  job was planned. A removal is now sticky — only a real arrival supersedes it — and
  it withdraws that one locator without a walk, behind the same guards: the
  volume-identity check remains the only authority, and a missing or replaced volume
  withdraws nothing.
- **Evidence:** `scripts/verify-watch.sh` prints both lines —
  `the library picked up the new file in 4.66s without a rescan` and
  `the removed file stopped being available in 0.08s and kept its history`; repeated
  on the same commit it reported 1.4 seconds for the removal, so the honest figure is
  about a second rather than a fixed one.
- **Where:** `apps/desktop/src/watch.rs`, `App::handle_watched` and `apply_work`,
  `crates/screenroom-core/src/scanner.rs` (per-path withdraw),
  `crates/screenroom-core/src/library/catalog.rs`.
- **Automated verification:** the crate and desktop suites, plus the script.
- **Human verification:** none needed beyond the script.

#### P2-09 · Six advisories in the iced/wgpu dependency tree — **BLOCKED upstream**

- **Requirement:** no known vulnerability or unmaintained crate in the shipped
  binary. **Status:** BLOCKED; five are "unmaintained" notices with no fix to take,
  one is a real memory-safety advisory whose fix is outside the range iced allows,
  and all six belong to the iced/wgpu/metal stack that iced 0.14 pins.
- **The six, as `Cargo.lock` stood at `5ccfb61`:** `RUSTSEC-2026-0253` — `lru`
  0.16.4, a potential use-after-free when `LruCache::pop()` unwinds; fixed in
  `lru` 0.18.2, reached here as `cryoglyph` → `iced_wgpu` on `lru = "0.16"`, and
  0.16.4 is the last 0.16 release, so no patch inside the allowed range exists.
  `RUSTSEC-2024-0436` (`paste` 1.0.15, unmaintained, through `metal`),
  `RUSTSEC-2026-0192` (`ttf-parser` 0.25.1, unmaintained, through `fontdb` →
  `cosmic-text`), and `RUSTSEC-2026-0371`/`0370`/`0372` (`zbus_names` 4.3.5 and
  `zvariant`/`zvariant_derive` 5.15.1, unmaintained, through `mundy` →
  `iced_winit`, Linux only).
- **Why it is blocked:** five of the six are transitive dependencies of iced 0.14,
  and `wgpu`/`metal` must stay in step with iced (`AGENTS.md`: iced 0.14 / wgpu 27,
  wgpu must match iced). `paste` arrives through `metal`, which this repository also
  declares directly for macOS, and `metal` 0.33 still requires `paste ^1`, so there
  is no version to move to. Taking `lru` 0.18 would mean forking or patching a
  third-party crate, which this repository forbids.
- **Work:** re-check when iced publishes a release on a newer `wgpu` and a wider
  `lru`; the check is `cargo update --dry-run` plus an OSV query over `Cargo.lock`.
- **Acceptance:** an OSV query over `Cargo.lock` returns no advisory that is not an
  "unmaintained" notice, or each remaining one is a documented accepted risk.
- **Evidence:** the 2026-11-04 dependency-upgrade run queried OSV for every
  crates.io package in `Cargo.lock` before and after the upgrade — the same six
  before, the same six after, none introduced by it — and `pnpm audit` reported no
  JavaScript advisories; `cargo-audit` is not installed on this machine.

---

## 6. What was completed in this session

| Commit | Change | Evidence |
| --- | --- | --- |
| `e56e7ac` | Stopping a viewing stores the place it reached, and the track list is not published before libmpv resolves it | Both intermittent failures reproduced, root-caused and then passed 3 full workspace runs (358 tests each), 8 acceptance runs and 6 runs of the player suite at `--test-threads=22` |
| `38a5167` | The OpenGL → IOSurface → Metal probe, its measurements, and an honest `render_bridge()` | Probe passes on all fixtures; Metal reads exactly the bytes OpenGL wrote; 4K at 22.9–23.7 fps and 27–32 % CPU |
| `732af99` | A Bengali subtitle is proved to reach the picture | 173 lit pixels with the cue, 0 without; the captured frame was inspected |

Detail:

- **Flaky test 1 — the mini-player step since deleted.** The checkpoint writer throttles to one
  write per second, and the close path used the same throttle. A viewing that lasted
  a few hundred milliseconds therefore kept only its first checkpoint, which lands at
  0 ms, so the stored place was 0. `record` now takes a `force` flag and the two
  stopping paths pass it. New regression test
  `stopping_a_viewing_stores_where_it_reached_even_inside_the_throttle`.
- **Flaky test 2 — `tracks_are_enumerated_and_can_be_switched`.** The metadata
  refresh published the track list as soon as `FILE_LOADED` arrived, which can be
  before libmpv resolves its default audio selection; the published list then showed
  no audio track selected, and nothing corrected it if no further `track-list` change
  arrived. The refresh now waits for `PLAYBACK_RESTART` (playback has started), reads
  a list with no selection up to six times until it settles, and keeps the previous
  list rather than publishing an unresolved one. An explicit "no audio" choice is
  remembered so a deliberate deselect is not mistaken for an unresolved one.
- **4K probe.** See §3 of `HWDEC_EVIDENCE.md` for the numbers and §4 for what it does
  not prove.

---


---

## 9. Task matrix — every row, its evidence and its remaining action

One line per tracked item, so nothing can be quietly dropped. `Evidence` names the
check that actually ran; `Action` is what is still owed and by whom.

| ID | Requirement | State | Evidence | Action / blocker | Owner |
| --- | --- | --- | --- | --- | --- |
| F01 | Sources | COMPLETE | `step_1`, `step_12`, relink test | — | — |
| F02 | Discovery | COMPLETE | `step_1` (1,200 files), `scanner.rs` tests (16) | — | — |
| F03 | Structured library | COMPLETE | `described_episodes`, `library.rs` (28) | — | — |
| F04 | Identity matching | COMPLETE (local) | parsing, NFO, manual-match tests | online ids arrive with F05 | — |
| F05 | Online enrichment | COMPLETE core / BLOCKED live | `providers*.rs` (33), drain tests | key, TLS transport, retention decision | owner |
| F06 | Offline artwork | COMPLETE | poster re-decode, cache test | — | — |

| F08 | Profiles and progress | COMPLETE | `step_10`, `step_14`, checkpoint tests | — | — |
| F09 | Home and Continue Watching | COMPLETE | `step_11`, `step_13` | — | — |
| F10 | Next-episode autoplay | COMPLETE | `step_9`, gap/season tests | — | — |
| F11 | Search, filter, sort, paging | COMPLETE | 10k paging (13.7 ms first, 78 ms grouped last) | — | — |
| F12 | Watchlists and favourites | COMPLETE | toggle/filter tests | — | — |
| F13 | Audio tracks and subtitles | COMPLETE | `step_5`, `step_21`, Bengali pixel test | on-screen legibility | owner |
| F14 | Skip intro and credits | COMPLETE (manual) | marker and guard tests | — | — |
| F15 | Subtitles: file first, then The Subtitle DB | COMPLETE store + order / PENDING live | store tests, `step_21` | run `SUBTITLE_PROVIDER.md` §8 with the owner's key | owner |
| F16 | Local recommendations | COMPLETE | recommendation tests | — | — |
| F17 | Browse previews | COMPLETE | `step_17` | — | — |
| F18 | Picture-in-Picture | WITHDRAWN (2026-11-02) | — | not wanted; ADR-016 withdrawn | — |
| F19 | Casting | COMPLETE session / UNVERIFIED device | `casting`/`cast_stream`/`cast_session` (36) | a real receiver on the LAN (P1-08) | owner |
| F20 | Backup and recovery | COMPLETE | `step_13`, recovery tests (9), every-table round-trip | — | — |
| F21 | Settings and secrets | COMPLETE | `settings_are_operable_without_a_pointer`, action strip | — | — |
| F22 | Failure and availability | COMPLETE | `step_12`, error-code tests | — | — |
| F23 | Source directory | BLOCKED | `step_19` | ADR-011: no verified list or operator permission | owner |
| F24 | Acquisition handoff | COMPLETE | `step_19` | — | — |
| F25 | Local conversion | COMPLETE | `step_16` | — | — |
| Q00 | Installed local app | COMPLETE | bundle walk, `verify-bundle.sh` | — | — |
| Q00B | Design system | COMPLETE | `ds_pinned.rs` at a pinned commit, visible theme string | machine-local sync | — |
| Q01 | Offline | COMPLETE by construction | 33 provider tests assert zero requests when unconfigured | execute with Wi-Fi off | owner |
| Q02 | Performance | COMPLETE (SDR) | 4K 24.39 fps / 13.95 % CPU; paging; startup ~1.2 s. HDR removed 2026-11-03; its figures are history | — | — |
| Q03 | Fidelity | COMPLETE | `step_7`, anamorphic test | the look | owner |
| Q04 | Portability | COMPLETE | layout, `cfg` boundaries, `cargo check` for the Windows target | real Windows build | owner |
| Q05 | Security and privacy | COMPLETE for this repository's code; six iced-tree advisories open | argument vectors, root canonicalisation, credential redaction tests, cast host allowlist; OSV over `Cargo.lock` shows one memory-safety advisory and five unmaintained notices, all in the iced/wgpu/metal stack that iced 0.14 pins | P2-09 | — |
| Q06 | Durability | COMPLETE | migration tests (v2–v9), restore atomicity, byte-identical failure test | — | — |
| Q07 | Accessibility | COMPLETE for keyboard and focus; screen-reader bridge removed 2026-11-03 (ADR-005) | keyboard journey, action strip, focus tests, subtitle styling | — | — |
| Q08 | Honest reporting | COMPLETE | this document set | — | — |

Acceptance steps 1–15 keep their status in `MACOS_ACCEPTANCE_CHECKLIST.md`; the
mechanisms are green and seven steps still need the owner's eyes, ears and cable.

---

## 7. Reproducing the evidence

```bash
cargo test --workspace                                   # 358 tests
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
python3 scripts/no_comments.py check
scripts/package-macos.sh && scripts/verify-bundle.sh

cargo run --release -p screenroom-player --example frame_stall auto-copy FILE 20 150
SCREENROOM_PLAY_CHECK=/tmp/check.csv SCREENROOM_PLAY_CHECK_MEDIA=FILE \
SCREENROOM_PLAY_CHECK_SECONDS=10 ./target/release/screenroom-desktop
cargo run --release -p screenroom-player --example render_bench -- 3840 2160
cargo run --release -p screenroom-core --example library_bench

cd crates/screenroom-player/examples/gl-path-probe && \
  clang -O2 -DGL_SILENCE_DEPRECATION -o probe probe.m \
    -I/opt/homebrew/include -L/opt/homebrew/lib -lmpv \
    -framework OpenGL -framework IOSurface -framework CoreFoundation -framework Metal && \
  /usr/bin/time -l ./probe ../../../../.artifacts/media/bench-4k-hevc-10bit.mp4 8
```

## 8. Continuation point

**Updated 2026-11-03.** HDR and the VoiceOver bridge are removed
([ADR-005](ADR-005_SUBTITLE_PROVIDER_AND_FEATURE_REMOVALS.md)), so the two items
this section used to open with are gone: SDR is the only output and no screen-reader
tree is published. The notes below are kept as written, because they record what
was done and when.

**2026-11-02, the interface and the pipeline.** The desktop's five screens are
rebuilt as a view-layer change ([`UI_REBUILD.md`](UI_REBUILD.md)), and the
repository now carries GitHub Actions workflows on a self-hosted runner, a
release script and `scripts/ship.sh` ([`RELEASE_PIPELINE.md`](RELEASE_PIPELINE.md)).
**No GitHub Actions run has happened**: the runner is not installed, so the
workflows are written and parsed, not proven. The Windows job is declared and
disabled, so the Windows gate stays BLOCKED; no Tizen install was part of the
rebuild either.

**2026-11-01, end of the second session.** HEAD was `1deff6d`, the tree was
clean, 521 tests passed (three consecutive full runs), and `cargo fmt --check`,
`cargo clippy -D warnings`, `python3 scripts/no_comments.py check`,
`scripts/package-macos.sh`, `scripts/verify-bundle.sh`,
`scripts/check-quit-routing.sh` and `scripts/check-terminate-routing.sh` all passed.

What a next session should pick up, in this order:

1. **P1-07's live calls.** TSDB is the provider; run the four commands in
   [`SUBTITLE_PROVIDER.md`](SUBTITLE_PROVIDER.md) §8 with the owner's existing key,
   record the receipts, and then close F15's live half. Nothing there is claimed
   until it runs.
2. **P1-02's fence**, which is a correctness margin rather than a defect, and
   **P2-05's spirit, not its letter:** paging is measured at 10k and 50k; re-run
   `library_bench` after any query change and keep the numbers in this document.
3. **Owner-gated, and only the owner can close them:** P1-04 (the eight
   observations), P1-11 (the SSD and the real 3240×2160 Main10 file), P2-06 (the
   rebuilt interface at five widths), and the live metadata calls in P1-06.

Nothing in this list is speculative, and nothing above has been relabelled to make
the counts look better than they are.

## 9. Task matrix — every row, its evidence and its remaining action

One line per tracked item, so nothing can be quietly dropped. `Evidence` names the
check that actually ran; `Action` is what is still owed and by whom.

| ID | Requirement | State | Evidence | Action / blocker | Owner |
| --- | --- | --- | --- | --- | --- |
| F01 | Sources | COMPLETE | `step_1`, `step_12`, relink test | — | — |
| F02 | Discovery | COMPLETE | `step_1` (1,200 files), `scanner.rs` tests (16) | — | — |
| F03 | Structured library | COMPLETE | `described_episodes`, `library.rs` (28) | — | — |
| F04 | Identity matching | COMPLETE (local) | parsing, NFO, manual-match tests | online ids arrive with F05 | — |
| F05 | Online enrichment | COMPLETE core / BLOCKED live | `providers*.rs` (33), drain tests | key, TLS transport, retention decision | owner |
| F06 | Offline artwork | COMPLETE | poster re-decode, cache test | — | — |

| F08 | Profiles and progress | COMPLETE | `step_10`, `step_14`, checkpoint tests | — | — |
| F09 | Home and Continue Watching | COMPLETE | `step_11`, `step_13` | — | — |
| F10 | Next-episode autoplay | COMPLETE | `step_9`, gap/season tests | — | — |
| F11 | Search, filter, sort, paging | COMPLETE | 10k paging (13.7 ms first, 78 ms grouped last) | — | — |
| F12 | Watchlists and favourites | COMPLETE | toggle/filter tests | — | — |
| F13 | Audio tracks and subtitles | COMPLETE | `step_5`, `step_21`, Bengali pixel test | on-screen legibility | owner |
| F14 | Skip intro and credits | COMPLETE (manual) | marker and guard tests | — | — |
| F15 | Subtitles: file first, then The Subtitle DB | COMPLETE store + order / PENDING live | store tests, `step_21` | run `SUBTITLE_PROVIDER.md` §8 with the owner's key | owner |
| F16 | Local recommendations | COMPLETE | recommendation tests | — | — |
| F17 | Browse previews | COMPLETE | `step_17` | — | — |
| F18 | Picture-in-Picture | WITHDRAWN (2026-11-02) | — | not wanted; ADR-016 withdrawn | — |
| F19 | Casting | COMPLETE session / UNVERIFIED device | `casting`/`cast_stream`/`cast_session` (36) | a real receiver on the LAN (P1-08) | owner |
| F20 | Backup and recovery | COMPLETE | `step_13`, recovery tests (9), every-table round-trip | — | — |
| F21 | Settings and secrets | COMPLETE | `settings_are_operable_without_a_pointer`, action strip | — | — |
| F22 | Failure and availability | COMPLETE | `step_12`, error-code tests | — | — |
| F23 | Source directory | BLOCKED | `step_19` | ADR-011: no verified list or operator permission | owner |
| F24 | Acquisition handoff | COMPLETE | `step_19` | — | — |
| F25 | Local conversion | COMPLETE | `step_16` | — | — |
| Q00 | Installed local app | COMPLETE | bundle walk, `verify-bundle.sh` | — | — |
| Q00B | Design system | COMPLETE | `ds_pinned.rs` at a pinned commit, visible theme string | machine-local sync | — |
| Q01 | Offline | COMPLETE by construction | 33 provider tests assert zero requests when unconfigured | execute with Wi-Fi off | owner |
| Q02 | Performance | COMPLETE (SDR) | 4K 24.39 fps / 13.95 % CPU; paging; startup ~1.2 s. HDR removed 2026-11-03; its figures are history | — | — |
| Q03 | Fidelity | COMPLETE | `step_7`, anamorphic test | the look | owner |
| Q04 | Portability | COMPLETE | layout, `cfg` boundaries, `cargo check` for the Windows target | real Windows build | owner |
| Q05 | Security and privacy | COMPLETE for this repository's code; six iced-tree advisories open | argument vectors, root canonicalisation, credential redaction tests, cast host allowlist; OSV over `Cargo.lock` shows one memory-safety advisory and five unmaintained notices, all in the iced/wgpu/metal stack that iced 0.14 pins | P2-09 | — |
| Q06 | Durability | COMPLETE | migration tests (v2–v9), restore atomicity, byte-identical failure test | — | — |
| Q07 | Accessibility | COMPLETE for keyboard and focus; screen-reader bridge removed 2026-11-03 (ADR-005) | keyboard journey, action strip, focus tests, subtitle styling | — | — |
| Q08 | Honest reporting | COMPLETE | this document set | — | — |

Acceptance steps 1–15 keep their status in `MACOS_ACCEPTANCE_CHECKLIST.md`; the
mechanisms are green and seven steps still need the owner's eyes, ears and cable.

---

## 7. Reproducing the evidence

```bash
cargo test --workspace                                   # 358 tests
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
python3 scripts/no_comments.py check
scripts/package-macos.sh && scripts/verify-bundle.sh

cargo run --release -p screenroom-player --example frame_stall auto-copy FILE 20 150
SCREENROOM_PLAY_CHECK=/tmp/check.csv SCREENROOM_PLAY_CHECK_MEDIA=FILE \
SCREENROOM_PLAY_CHECK_SECONDS=10 ./target/release/screenroom-desktop
cargo run --release -p screenroom-player --example render_bench -- 3840 2160
cargo run --release -p screenroom-core --example library_bench

cd crates/screenroom-player/examples/gl-path-probe && \
  clang -O2 -DGL_SILENCE_DEPRECATION -o probe probe.m \
    -I/opt/homebrew/include -L/opt/homebrew/lib -lmpv \
    -framework OpenGL -framework IOSurface -framework CoreFoundation -framework Metal && \
  /usr/bin/time -l ./probe ../../../../.artifacts/media/bench-4k-hevc-10bit.mp4 8
```

## 8. Continuation point

If this session stops here, the next one starts at **P1-01/P1-02**: the probe
proves the path, the numbers justify it, and the integration is a bounded change
behind `render.rs` with the software renderer kept as the fallback. Do not change the
shipped default until `SCREENROOM_PLAY_CHECK` on the application itself reports
`delivered_fps ≥ 23.0` and CPU at or below 60 %.

Everything in §5 marked `BLOCKED` needs something only the owner can supply — a
provider account, a receiver, the SSD, or a physical observation — and each says
exactly what.

## Automatic updates, added 2026-11-05

Not in the original 50 rows: the owner asked for a complete automatic updater after
this backlog was written. Its design decision is
[`ADR-006`](ADR-006_AUTOMATIC_UPDATES.md), its gate is **G10** in
[`FEASIBILITY_GATES.md`](FEASIBILITY_GATES.md), and its evidence is in
[`MACOS_ACCEPTANCE_EVIDENCE.md`](MACOS_ACCEPTANCE_EVIDENCE.md).

| Row | Status | What settles it |
| --- | --- | --- |
| Check, download, verify and install a signed newer version | **COMPLETE** | `scripts/check-updater.sh`: 0.1.0 → 0.2.0 through the real installed binary |
| Refuse a tampered archive, an unsigned feed and a downgrade | **COMPLETE** | the same script, three negative cases |
| Settings → Updates, keyboard-operable, with a banner that never covers a film | **COMPLETE** | `apps/desktop/src/app/acceptance.rs`, six tests |
| Serve the feed from GitHub Releases | **COMPLETE** | `v0.1.0` is published with the LGPL stack, the signed appcast and the corresponding source; a 0.0.9 build updated from it over HTTPS |
| Developer ID signature and notarisation | **NOT PURSUED** | the owner declined the $99/yr Apple Developer Program; the bundle stays ad-hoc, and `scripts/notarize.sh` is ready for the day that changes |
| The `Update now` action | **COMPLETE** | one click installs and reopens the new version; only the pointer press itself is unobserved |
| A GitHub Actions run of the updated release job | **PARTIAL** | run 41961636886 built, packaged and signed the feed on GitHub's runner; the *release* run for `v0.1.0` is blocked by the hosted runner's capacity on a cold cache, so 0.1.0 was packaged on this Mac with the same script |

The updater does not change any other row: playback, scanning, subtitles, casting,
backup and the design system are untouched, and `cargo test --workspace` is green.
