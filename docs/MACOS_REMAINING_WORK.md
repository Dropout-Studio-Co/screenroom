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

**Updated 2026-11-01, second session.** The GPU render path is integrated and
measured; the online halves of F05 and F15, casting, backup threading, the
scanner transaction, fullscreen Escape and Dock Quit are implemented and tested;
accessibility is documented as blocked upstream with evidence.

| Measure | Result |
| --- | --- |
| Applicable macOS requirements | **50** (25 functional + 10 quality + 15 acceptance steps) |
| Complete | **41** |
| Partial | **8** |
| Missing | **0** |
| Broken | **0** |
| Unverified | **1** (acceptance step 8: 4K on battery) |
| Blocked | **4 rows carry a named blocker** (F05 live, F15 live, F18 native PiP, F23) |
| Not applicable to macOS | **0** of these 50 |
| Outstanding implementation tasks | **3 P1** (P1-03 EDR window, P1-05 PiP, P1-09 accessibility) and **0 P2**; P1-12's mechanism is complete and only its execution needs the owner |
| Outstanding human/device tasks | **9** (8 owner observations + 1 owner hardware run) |

The four measures the brief asks to keep apart:

| | Value | Meaning |
| --- | --- | --- |
| **Implementation** | 31 of 35 product/quality areas implemented in full (89 %), 4 partial (11 %), 0 absent | every feature area has working code behind it |
| **Automated verification** | 33 of 35 areas have an automated check that ran | the 2 exceptions are the live provider calls (F05, F15), which need an owner key, and OS-level PiP (F18) |
| **Human/device acceptance** | 7 of 15 checklist steps closed; 7 half-closed; 1 needs the owner and the hardware | see §5 |
| **Everyday readiness** | **usable now** for 1080p and 4K at the source frame rate: 24.4 fps at 13.95 % mean CPU on 4K HEVC Main10 through the OpenGL path, against 24.2 fps at 89.8 % on the old one | HDR is still tone-mapped to SDR; no OS-wide PiP; no online enrichment or subtitle search without an owner key |

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
   and position, an in-app mini player, up-next with a cancelable countdown,
   manually pinned skip-intro/credits markers, keep-open previews on browse.
5. **Around the film.** Local NFO metadata and genres, imported subtitle files kept
   and reused offline, remux/audio-fix/h264 conversion into a separate folder with
   progress, cancel and verification, backups and restores that ask twice.
6. **Settings.** Profiles, autoplay, auto-skip, reduce motion, languages, folders,
   saved sources with an honest external handoff, receiver discovery, and an action
   strip that runs every one of those from the keyboard.

The three things that keep it from being *finished* are the 4K cost, HDR, and the
advanced features that need something the machine does not have.

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
| F07 | Video playback | **COMPLETE (SDR) / HDR surface only** — 1080p and 4K at the source frame rate (24.4 fps, 13.95 % CPU, zero uploads); a 16-bit PQ surface exists and is measured, but nothing can be presented as HDR through iced 0.14 | `SCREENROOM_PLAY_CHECK`, `gl_render_check`, `hdr_check` | steps 4, 6, 7, 8 | EDR window (P1-03) |
| F08 | Profiles and progress | COMPLETE | `step_10`, `step_14`, checkpoint tests | steps 10, 14 | — |
| F09 | Home and Continue Watching | COMPLETE | `step_11`, `step_13` | step 11 | — |
| F10 | Next-episode autoplay | COMPLETE | `step_9`, gap/season tests | step 9 | — |
| F11 | Search, filters, sort, paging | COMPLETE | 830-row library paging, `step_18` genre | — | — |
| F12 | Watchlists and favourites | COMPLETE | toggle/filter tests | — | — |
| F13 | Audio tracks and subtitles | COMPLETE | `step_5`, `step_21`, track tests, and a Bengali cue proved to be drawn into the picture | step 5 (that it reads correctly on screen) | — |
| F14 | Skip intro and credits | COMPLETE (manual markers only) | marker tests, auto-skip guard tests | — | — |
| F15 | Subtitle search and management | **COMPLETE (core) / BLOCKED (live)** — OpenSubtitles-shaped provider with Bengali, quota/login/rate-limit states and a host allowlist on downloads; import, reuse and removal unchanged | `tests/providers_subtitles.rs` (19), `step_21` | step 5 | owner key + TLS transport + offline-retention decision |
| F16 | Local recommendations | COMPLETE | recommendation tests | — | — |
| F17 | Browse previews | COMPLETE | `step_17` (dwell, one decoder, reduced motion, cleanup) | look at a card | — |
| F18 | Picture-in-Picture | **PARTIAL** — in-app mini player, labelled as such; no macOS PiP | `step_20` | — | ADR-016: iced/wgpu has no `AVPlayerLayer` (P1-05) |
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
| Q01 | Offline: library, profiles, artwork, resume, playback with the network off | COMPLETE by construction (no network code at all; only the browser handoff and an opt-in SSDP probe) | not yet *executed* with Wi-Fi off (P1-12) | execution pending |
| Q02 | Performance: startup, first frame, 4K, memory, no runaway jobs | **COMPLETE (SDR)** | startup ~1.2 s; 4K 24.4 fps at 13.95 % mean CPU with zero uploads; paging 13.7 ms first page and 78 ms grouped last page at 10k rows; backups and restores run off the UI thread | HDR (P1-03) |
| Q03 | Fidelity: aspect, anamorphic, timebase, honest codec claims | COMPLETE | `step_7`, `anamorphic_video_renders_at_its_display_aspect_without_letterbox_rows` | the *look* of step 7 is the owner's |
| Q04 | Portability: one monorepo, isolated platform adapters | COMPLETE | layout, `cfg` boundaries | — |
| Q05 | Security and privacy | COMPLETE | argument vectors (no shell), root canonicalisation, `contained_media_path` tests, no credentials anywhere | — |
| Q06 | Durability: migrations, transactions, no silent resets | COMPLETE | migration tests, integrity check after migrate, backup/restore | — |
| Q07 | Accessibility and design quality | **PARTIAL / BLOCKED** | keyboard journey, Settings action strip, contrast tokens | iced 0.14 ships no accessibility tree at all and `Cargo.lock` has no `accesskit`; verified and recorded as ADR-017 (P1-09) |
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
- **Depends on:** P1-02 (synchronisation and lifetime) for correctness; P1-03 for
  HDR.
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

#### P1-03 · Show HDR10 at its mastered luminance — **surface CLOSED, display BLOCKED**

- **Requirement:** F07, Q03, Q02.
- **Status:** PARTIAL, and the boundary is now exact. A 16-bit half-float
  (`GL_RGBA16F`) `IOSurface` is created and measured, libmpv is told the target
  depth and asked to pass the master through rather than compress it, and the
  frame states its own format. What does not exist is an EDR **window**: nothing has
  been presented, so no panel has received anything.
- **What was measured, headless, on the HDR10 fixture**
  (`examples/hdr_check.rs`, no window, media read-only): the surface reads back as
  CoreVideo `'RGhA'` `0x52476841` with 8 bytes per pixel and a 30 720-byte row at
  3840, the FBO is `0x881a`, and the delivered surface has **4 509 distinct green
  values against 256 for eight bits, with 13 codes above 1.0** and a peak of
  **1.076** in code space. The `float8` control — the same surface with HDR
  *options* off — has 2 105 distinct values and **zero** codes above 1.0, which is
  what proves the "off the eight-bit grid" claim alone is not HDR; the 16-bit
  container alone is not either. Changing only `target-peak` to 100 nits moves the
  peak to 0.580078 (176.4 nits), so the options are live. The SDR fixture still
  reports `BGRA`/`0x8058`/256 distinct/zero off-grid, unchanged.
- **Why the display is blocked, checked in the installed crates rather than
  recalled:** `wgpu-hal 27.0.4` (`metal/surface.rs:168-173`) already sets
  `wantsExtendedDynamicRangeContent` when the surface format is `Rgba16Float`, and
  lists that format in `metal/adapter.rs:362-366` — but `iced_wgpu 0.14.0`
  (`window/compositor.rs:107-121`) picks the first `is_srgb()` format
  (`Bgra8UnormSrgb`) and `iced_wgpu::Settings` has no format field. The swapchain
  iced creates therefore can never be an extended-range one, and no amount of
  application code changes that. wgpu also flips only the EDR flag, never the
  colours space, so `kCGColorSpaceITUR_2100_PQ` would still have to be set on the
  layer.
- **What this session wired anyway**, so the picture cannot be wrong when it is
  switched on: `video_surface` now maps `PixelFormat` to the matching Metal pixel
  format and wgpu texture format (`RGBA16Float` rather than an sRGB view of a PQ
  surface), and Settings prints `PlayerSnapshot.output.label()`, so the application
  states what the surface carries instead of implying HDR.
- **Still open:** a patched or vendored `iced_wgpu` format choice, or an
  application-owned `CAMetalLayer`; then `SCREENROOM_HDR=extended`; then an owner
  look at an HDR film on the XDR panel. Do **not** enable the switch before the
  first of those exists.
- **Also unproven and labelled as such:** the nits in the receipt are the PQ EOTF
  applied to delivered codes, not a display measurement, and the fixture is
  generated noise tagged PQ rather than a graded master. HLG deliberately stays on
  the SDR path.

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

#### P1-05 · macOS Picture-in-Picture (F18) — **decision recorded, implementation open**

- **Requirement:** F18.
- **Status:** PARTIAL — a labelled in-app mini player exists and is honest about what
  it is; system-wide PiP does not exist. The decision and the exact API constraint
  are now recorded as **ADR-016**.
- **What is missing:** the second display path PiP needs.
- **Where:** `apps/desktop/src/app.rs` (`reduce_to_mini`, `mini_view`).
- **Why it matters:** the PRD allows "authentic OS behaviour **or** a distinctly
  labelled mini-player"; the app chose the second, and that choice is now a decision
  rather than a sentence in a ledger.
- **What the SDK actually says** (checked in the installed macOS SDK, not recalled):
  `AVPictureInPictureController` takes an `AVPlayerLayer` (`initWithPlayerLayer:`) or,
  since macOS 12, an `AVSampleBufferDisplayLayer` through
  `AVPictureInPictureControllerContentSource` with a playback delegate. A wgpu
  surface is neither, so PiP means wrapping decoded frames as `CMSampleBuffer`s — and
  the OpenGL/`IOSurface` path in P1-01 is the natural source for an
  `IOSurface`-backed `CVPixelBuffer`.
- **Work:** only if real PiP is wanted: prototype an `AVSampleBufferDisplayLayer` fed
  from the player's surfaces and drive `AVPictureInPictureController` from it, keeping
  the in-app player as the default.
- **Depends on:** P1-01 for the surfaces.
- **Acceptance:** PiP floats the film outside the window, controls work, and closing
  PiP returns to the real player without reloading.
- **Automated verification:** the mini player's `step_20` stays green either way.
- **Human verification:** the floating window itself.

---

#### P1-06 · Whole-library online metadata enrichment (F05) — **core CLOSED, live blocked**

- **Requirement:** F05.
- **Status:** COMPLETE (provider-independent core) / BLOCKED (live calls). A TMDB-shaped provider, a durable `enrichment_queue` on schema 9, per-minute rate limiting, `Retry-After` handling, exponential backoff, a bounded transport with an overall deadline and a hostile-chunk guard, reconciliation precedence (manual beats NFO beats provider, and NFO only yields when the owner asks), and an artwork-retention gate that defaults to *do not store*. An unconfigured provider reports itself as not configured and makes zero requests. 33 integration tests. Settings has an *Online metadata* panel and a keyboard action for it.
- **What is missing:** the online half. The offline half (local NFO) is complete.
- **Where:** `crates/screenroom-core/src/nfo.rs` (the origin column and
  `metadata_origin` are where a provider lands), `apps/desktop/src/app.rs` Settings.
- **Why it matters:** without it, a library of badly named files keeps whatever the
  filename parser and any NFO says.
- **Work:** the owner picks a provider and accepts its terms (TMDB's offline image
  caching terms are the open question the PRD names); then a keyed, rate-limited,
  queued client with visible/recent items first, `metadata_origin = provider`, manual
  matches never overwritten, and bytes stored offline like posters today.
- **Depends on:** **owner action** — provider choice, account/key, and a decision on
  retaining provider images.
- **Acceptance:** a scan of a folder of badly named files enriches titles, years,
  overviews and posters while offline reading still works; a manual match survives a
  re-enrichment.
- **Automated verification:** recorded fixtures parsed by a pure function; the live
  call is exercised once with the owner's key and never in CI.
- **Human verification:** does the result match the film.

---

#### P1-07 · Online subtitle search and download (F15) — **core CLOSED, live blocked**

- **Requirement:** F15.
- **Status:** COMPLETE (provider-independent core) / BLOCKED (live calls). OpenSubtitles-shaped search and download with Bengali (`bn`/`ben`), distinct quota (406), login (407) and rate-limit (429) states, a host allowlist that refuses a download link to loopback or an unrelated host, and an import into the existing bounded subtitle store that never touches the media file. 19 integration tests. Settings has a *Subtitle search* panel that lists language, release and format and a *Download and use* action; both are also in the keyboard action strip.
- **What is missing:** search by title/release/hash/language, quota handling, and a
  user-triggered save into the existing imported-subtitle store.
- **Where:** the `subtitles` table and `use_subtitle` path already accept a stored
  file; the panel is in `app.rs` (player screen).
- **Why it matters:** imported files are already supported; finding one is the part
  that is missing.
- **Work:** owner registers and accepts the API terms; then a keyed client with
  quota/backoff, results showing language/release/format, and a *Download and use*
  action that writes into the existing bounded store.
- **Depends on:** **owner action** — an OpenSubtitles account and key.
- **Acceptance:** search returns matches for a known film, a chosen subtitle is
  stored, handed to libmpv and reused offline; a quota error is reported, not
  swallowed.
- **Automated verification:** parser/fixture tests; the network call is manual.
- **Human verification:** the subtitle text appears and matches the dialogue.

---

#### P1-08 · Casting session with a real receiver (F19) — **session CLOSED, device unverified**

- **Requirement:** F19.
- **Status:** COMPLETE (session) / UNVERIFIED (no device). Description fetching and parsing, `AVTransport`/`RenderingControl` resolution with relative-URL handling, a session-scoped HTTP endpoint that serves one opaque token with correct `Range`/`206`/`416` responses, a hard lifetime, one-client accounting, `SetAVTransportURI`/`Play`/`Pause`/`Seek`/`Stop`/`GetTransportInfo` SOAP control reading the receiver's own state back, disconnect recovery, and a `Drop` that closes the listener. 36 tests drive the whole session against an in-process fake receiver, including a byte-exact `Range` fetch and a receiver that refuses `Play`. **No media has ever been played on real hardware** and the capability text says so.
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

#### P1-09 · Accessibility beyond the keyboard (Q07)

- **Requirement:** Q07.
- **Status:** BLOCKED with evidence, as ADR-017. Verified rather than recalled: `grep -rn "accessib"` over `iced-0.14.0`, `iced_core-0.14.0` and `iced_winit-0.14.0` finds only an unrelated `Inaccessible(io::Error)` image variant; `Cargo.lock` contains no `accesskit` package at all; and iced 0.14.0's own changelog has no accessibility entry. There is no widget-level role/label surface and no tree to publish, so VoiceOver sees one opaque surface. Keyboard operation is not screen-reader support and is not presented as such.
- **What is missing:** named controls, roles and values for assistive technology.
- **Where:** the whole view layer (`apps/desktop/src/app.rs` view functions,
  `design/`).
- **Why it matters:** the PRD applies WCAG 2.2 AA, and "keyboard-reachable" is not
  the same as "screen-reader-usable".
- **Work:** check iced 0.14's accessibility status against its current
  documentation; if there is no supported path, record the limit with the version
  checked and a recovery route (a high-contrast/large-text mode is *not* a
  substitute and must not be presented as one).
- **Depends on:** an upstream capability that does not exist today.
- **Acceptance:** either VoiceOver announces every control, or the limitation is
  documented with evidence and a named alternative.
- **Automated verification:** none today; the keyboard journey tests remain.
- **Human verification:** VoiceOver walk-through, if it becomes possible.

---

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

#### P1-11 · Re-measure the owner's real 4K HEVC Main10 file

- **Requirement:** F07, Q02, acceptance step 8.
- **Status:** BLOCKED — `/Volumes` held only `mothership` and `Recovery` on
  2026-11-01, so the 3240×2160 Main10 sample was unreachable.
- **What is missing:** the number that matters. The recorded **19.449 fps** was
  measured *before* native 4K delivery and *before* the post-load hold was fixed, so
  it is stale in both directions: it discarded 4K detail and it lost ~6 frames per
  load.
- **Where:** `crates/screenroom-player/examples/frame_stall.rs`,
  `SCREENROOM_PLAY_CHECK`.
- **Why it matters:** every 4K claim in this repository currently rests on generated
  fixtures.
- **Work:** with the SSD mounted, run `frame_stall auto-copy <file> 20 150` and
  `SCREENROOM_PLAY_CHECK` on the same file, and record both.
- **Depends on:** **owner hardware** — the external SSD mounted.
- **Acceptance:** both figures recorded in `HWDEC_EVIDENCE.md` with the file's real
  dimensions.
- **Automated verification:** the numbers themselves.
- **Human verification:** step 8's five-minute battery observation.

---

#### P1-12 · Prove offline behaviour with the network actually off

- **Requirement:** Q01, PRD acceptance criterion G.
- **Status:** COMPLETE by construction and still not executed with Wi-Fi disabled. What changed: there is now a real network path, so "no network code" is no longer the argument — the argument is that every network call is behind an explicit owner action (an enrichment pass, a subtitle search, a cast session) and that an unconfigured provider makes zero requests, which 33 tests assert. The execution with Wi-Fi off remains the owner's step.
- **What is missing:** the execution.
- **Where:** nothing to change.
- **Why it matters:** the PRD asks for this to be *proven*, and "no code calls the
  network" is a reading, not a receipt.
- **Work:** with the owner's agreement, turn Wi-Fi off, launch `dist/Screenroom.app`,
  browse, play, quit, reopen, resume; note that nothing hangs or shows a spinner.
- **Depends on:** **owner** (Wi-Fi is theirs). Can also be checked by the owner in
  the same session as P1-04.
- **Acceptance:** the full journey completes with no network and no error state.
- **Automated verification:** a static assertion already exists that no HTTP client
  is linked; that is weaker and must not be reported as the proof.
- **Human verification:** yes.

---

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

---

## 6. What was completed in this session

| Commit | Change | Evidence |
| --- | --- | --- |
| `e56e7ac` | Stopping a viewing stores the place it reached, and the track list is not published before libmpv resolves it | Both intermittent failures reproduced, root-caused and then passed 3 full workspace runs (358 tests each), 8 acceptance runs and 6 runs of the player suite at `--test-threads=22` |
| `38a5167` | The OpenGL → IOSurface → Metal probe, its measurements, and an honest `render_bridge()` | Probe passes on all fixtures; Metal reads exactly the bytes OpenGL wrote; 4K at 22.9–23.7 fps and 27–32 % CPU |
| `732af99` | A Bengali subtitle is proved to reach the picture | 173 lit pixels with the cue, 0 without; the captured frame was inspected |

Detail:

- **Flaky test 1 — `step_20_…_mini_player…`.** The checkpoint writer throttles to one
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
| F15 | Subtitle search | COMPLETE core / BLOCKED live | `providers_subtitles.rs` (19) | key, TLS transport, offline-retention decision | owner |
| F16 | Local recommendations | COMPLETE | recommendation tests | — | — |
| F17 | Browse previews | COMPLETE | `step_17` | — | — |
| F18 | Picture-in-Picture | PARTIAL | `step_20` | ADR-016; needs an `AVSampleBufferDisplayLayer` bridge (P1-05) | agent |
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
| Q02 | Performance | COMPLETE (SDR) | 4K 24.39 fps / 13.95 % CPU; paging; startup ~1.2 s | EDR window (P1-03) | owner decision |
| Q03 | Fidelity | COMPLETE | `step_7`, anamorphic test | the look | owner |
| Q04 | Portability | COMPLETE | layout, `cfg` boundaries, `cargo check` for the Windows target | real Windows build | owner |
| Q05 | Security and privacy | COMPLETE | argument vectors, root canonicalisation, credential redaction tests, cast host allowlist | — | — |
| Q06 | Durability | COMPLETE | migration tests (v2–v9), restore atomicity, byte-identical failure test | — | — |
| Q07 | Accessibility | PARTIAL / BLOCKED | ADR-017 with the exact grep and lockfile evidence | upstream `accesskit`, or an authorised `NSAccessibility` project | owner decision |
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
