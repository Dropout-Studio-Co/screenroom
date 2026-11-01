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

| Measure | Result |
| --- | --- |
| Applicable macOS requirements | **50** (25 functional + 10 quality + 15 acceptance steps) |
| Complete | **34** |
| Partial | **15** |
| Missing | **0** |
| Broken | **0** |
| Unverified | **1** (acceptance step 8: 4K on battery) |
| Blocked | **0 rows**; four partial rows carry a named blocker (below) |
| Not applicable to macOS | **0** of these 50 (the PRD's Windows and Samsung criteria, and the retired iOS/Android/Linux/webOS/Android-TV platforms, are outside this set) |
| Outstanding implementation tasks | **11 P1 + 5 P2** |
| Outstanding human/device tasks | **9** (8 owner observations + 1 owner hardware run) |

The four measures the brief asks to keep apart:

| | Value | Meaning |
| --- | --- | --- |
| **Implementation** | 27 of 35 product/quality areas implemented in full (77 %), 8 partial (23 %), 0 absent | every feature area has working code behind it |
| **Automated verification** | 30 of 35 areas have an automated check that ran | the 5 exceptions are the online halves of F05 and F15, casting (F19), the shipped source directory (F23) and OS-level PiP (F18) |
| **Human/device acceptance** | 7 of 15 checklist steps closed; 7 half-closed; 1 needs the owner and the hardware | see §5 |
| **Everyday readiness** | **usable now** for 1080p; 4K plays at 19–24 fps using 116–186 % CPU; HDR is tone-mapped to SDR; no OS-wide PiP; no online enrichment or subtitle search | see §3 for the exact limits |

A single headline percentage would hide the two rows that decide how the app feels
in use — 4K cost and HDR — so the four measures above are reported instead.

---

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
| F05 | Whole-library enrichment | **PARTIAL** — offline NFO only; nothing is fetched | `step_18` | step 2 | provider + key + image-retention terms |
| F06 | Offline artwork bytes | COMPLETE | 21 posters re-decoded by ffprobe, 200 candidates readable, `a_retained_poster_is_cached_for_its_card` | step 2 | — |
| F07 | Video playback | **PARTIAL** — 1080p 30 fps; 4K 19–24 fps at high CPU; HDR tone-mapped to SDR | `worker.rs`, `apps/desktop/tests/playback.rs`, `SCREENROOM_PLAY_CHECK` | steps 4, 6, 7, 8 | GPU presentation (P1-01/02), HDR (P1-03) |
| F08 | Profiles and progress | COMPLETE | `step_10`, `step_14`, checkpoint tests | steps 10, 14 | — |
| F09 | Home and Continue Watching | COMPLETE | `step_11`, `step_13` | step 11 | — |
| F10 | Next-episode autoplay | COMPLETE | `step_9`, gap/season tests | step 9 | — |
| F11 | Search, filters, sort, paging | COMPLETE | 830-row library paging, `step_18` genre | — | — |
| F12 | Watchlists and favourites | COMPLETE | toggle/filter tests | — | — |
| F13 | Audio tracks and subtitles | COMPLETE | `step_5`, `step_21`, track tests | step 5 | Bengali glyph coverage unmeasured (P1-10) |
| F14 | Skip intro and credits | COMPLETE (manual markers only) | marker tests, auto-skip guard tests | — | — |
| F15 | Subtitle search and management | **PARTIAL** — import, reuse and removal work; no search | `step_21` | step 5 | OpenSubtitles key + licence decision (P1-07) |
| F16 | Local recommendations | COMPLETE | recommendation tests | — | — |
| F17 | Browse previews | COMPLETE | `step_17` (dwell, one decoder, reduced motion, cleanup) | look at a card | — |
| F18 | Picture-in-Picture | **PARTIAL** — in-app mini player, labelled as such; no macOS PiP | `step_20` | — | iced/wgpu has no `AVPlayerLayer` (P1-05) |
| F19 | Casting | **PARTIAL** — opt-in SSDP discovery only | `step_22`, parser tests | — | a real receiver + LAN endpoint decision (P1-08) |
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
| Q02 | Performance: startup, first frame, 4K, memory, no runaway jobs | **PARTIAL** | startup 443–621 ms; first frame measured; paging at 10k/50k; 4K 19–24 fps | 4K cost, HDR (P1-01/02/03) |
| Q03 | Fidelity: aspect, anamorphic, timebase, honest codec claims | COMPLETE | `step_7`, `anamorphic_video_renders_at_its_display_aspect_without_letterbox_rows` | the *look* of step 7 is the owner's |
| Q04 | Portability: one monorepo, isolated platform adapters | COMPLETE | layout, `cfg` boundaries | — |
| Q05 | Security and privacy | COMPLETE | argument vectors (no shell), root canonicalisation, `contained_media_path` tests, no credentials anywhere | — |
| Q06 | Durability: migrations, transactions, no silent resets | COMPLETE | migration tests, integrity check after migrate, backup/restore | — |
| Q07 | Accessibility and design quality | **PARTIAL** | keyboard journey, Settings action strip, contrast tokens | iced 0.14 exposes no accessibility tree to VoiceOver (P1-09) |
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

#### P1-01 · Present 4K frames through iced without a CPU copy

- **Requirement:** F07, Q02 (G2 on-screen half).
- **Status:** PARTIAL — the isolated path is proven, the integration is not written.
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
- **Status:** MISSING — the probe calls `glFinish` after every frame, which is
  correct but serialises.
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

#### P1-03 · Show HDR10 at its mastered luminance

- **Requirement:** F07, Q03, Q02.
- **Status:** PARTIAL — HDR10 is decoded and tone-mapped to 8-bit SDR; the picture is
  correct as SDR and is not HDR.
- **What is missing:** an extended-range surface (16-bit float or 10-bit with
  `wantsExtendedDynamicRangeContent`), a colour space handed to the window, and a
  tone-mapping choice that is labelled rather than inherited.
- **Where:** `render.rs`/the new GL backend, `video_surface.rs`, the window creation
  in `app.rs`.
- **Why it matters:** the PRD forbids pretending HDR works. Today the app is honest
  but SDR-only; on an HDR display the highlights are compressed.
- **Work:** request `MPV_RENDER_PARAM_TARGET_COLORSPACE`/HDR passthrough on the GL
  backend, an `RGBA16Float` IOSurface and an EDR-enabled `CAMetalLayer`, plus a
  Settings note that names what the display is actually receiving.
- **Depends on:** P1-01.
- **Acceptance:** a generated HDR10 fixture measured through the output surface
  reports more than 8 bits of precision and a peak above SDR white on an HDR
  display; the SDR path is unchanged when the display or file is SDR.
- **Automated verification:** the delivered surface's format and range are asserted;
  a luma comparison against mpv's own libplacebo render within 2 %.
- **Human verification:** view an HDR film on an HDR display (needs one; the
  MacBook's panel is XDR, so this is possible on the owner's machine).

---

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

#### P1-05 · Decide macOS Picture-in-Picture (F18)

- **Requirement:** F18.
- **Status:** PARTIAL — a labelled in-app mini player exists and is honest about what
  it is; system-wide PiP does not exist.
- **What is missing:** either real macOS PiP or a recorded, owned decision that it is
  out of reach with iced/wgpu.
- **Where:** `apps/desktop/src/app.rs` (`reduce_to_mini`, `mini_view`).
- **Why it matters:** the PRD allows "authentic OS behaviour **or** a distinctly
  labelled mini-player"; the app chose the second. That choice is currently a
  sentence in a status ledger, not a decision with an owner.
- **Work:** confirm what macOS PiP requires (an `AVPlayerLayer` with an AVPlayer; a
  wgpu surface cannot be its source), then record an ADR-style note in
  `DECISIONS_AND_RESEARCH.md` naming the limit and the fallback. Only attempt PiP if
  a supported path is found that keeps the wgpu renderer.
- **Depends on:** nothing.
- **Acceptance:** a written decision with the API constraint quoted, referenced from
  F18 in `CAPABILITY_MATRIX.md`.
- **Automated verification:** the mini player's existing `step_20` stays green.
- **Human verification:** look at the mini panel once (part of the owner session).

---

#### P1-06 · Whole-library online metadata enrichment (F05)

- **Requirement:** F05.
- **Status:** BLOCKED — no provider, account, key or image-retention decision exists;
  nothing is fetched, guessed or scraped, and that is deliberate.
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

#### P1-07 · Online subtitle search and download (F15)

- **Requirement:** F15.
- **Status:** BLOCKED — no OpenSubtitles account, key or licence decision. The app
  deliberately shows no search control rather than a dead one.
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

#### P1-08 · Casting session with a real receiver (F19)

- **Requirement:** F19.
- **Status:** BLOCKED — discovery only, on purpose.
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
- **Status:** PARTIAL — everything is operable by keyboard; iced 0.14 exposes no
  accessibility tree, so VoiceOver sees one opaque surface.
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

#### P1-10 · Prove Unicode and Bengali subtitles render

- **Requirement:** F13, Q07 (subtitle legibility).
- **Status:** UNVERIFIED for Bengali specifically. libmpv draws subtitles with its
  own renderer and a bundled font; no Bengali file has been shown.
- **What is missing:** a generated fixture with Bengali and mixed-script cues, and a
  check that the glyphs are not boxes.
- **Where:** the player's subtitle path (`sub-add`, `sub-scale`, `sub-pos`).
- **Why it matters:** the owner's language is named explicitly in the PRD.
- **Work:** generate an SRT with Bengali, Devanagari and Latin cues, play it, and
  capture one frame headlessly through the software path; assert the frame differs
  from the same frame with no subtitle and record the image for the owner to look at.
- **Depends on:** nothing.
- **Acceptance:** the rendered frame contains non-background pixels in the subtitle
  band for all three scripts, and the captured image shows real glyphs when a person
  looks at it.
- **Automated verification:** the "subtitles changed the frame" assertion.
- **Human verification:** one look at the captured image.

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
- **Status:** COMPLETE by construction (the app contains no network calls except the
  opt-in SSDP probe and handing an http address to the browser), but never executed
  with Wi-Fi disabled.
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
- **Status:** PARTIAL and deliberately blocked (ADR-011): bookmarks carry provenance
  and nothing claims nationwide access; no list is shipped.
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

#### P2-01 · Move backup and restore off the UI thread

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

#### P2-02 · Escape during the fullscreen animation (recorded as L6)

- **Requirement:** F07, step 6. **Status:** known limit.
  **Where:** `app.rs` key routing (`ToggleFullscreen`, `ModeChanged`).
  **Work:** ignore an Escape that arrives while the mode change is in flight, or
  queue it until `ModeChanged` settles. **Acceptance:** a rapid `f` then Escape
  leaves fullscreen and stays in the film. **Automated:** the existing mode tests
  extended with an in-flight case.

#### P2-03 · Partial-scan stat checks outside the write transaction (L4)

- **Requirement:** F02, Q06. **Status:** known limit; a sleeping drive can stall the
  UI briefly. **Where:** the scanner's partial-scan path.
  **Work:** collect the stat results first, then open the transaction.
  **Acceptance:** a scan of an unresponsive root does not hold a write transaction
  while it waits. **Automated:** a scanner test with a simulated slow filesystem.

#### P2-04 · Close the session on Dock Quit, logout and shutdown

- **Requirement:** F08. **Status:** accepted limit; the loss is bounded by the 1 s
  checkpoint and the stale session is closed on the next open. **Where:**
  `app.rs` `Message::Close`, `route_quit`.
  **Work:** install a handler for `applicationShouldTerminate`-equivalent events
  (winit's `LoopExiting`/`CloseRequested`) that runs the same stop path.
  **Acceptance:** Dock Quit mid-film stores the position and closes the session.
  **Automated:** `scripts/check-quit-routing.sh` extended to the Dock path if the
  platform allows it headlessly.

#### P2-05 · Re-check large-library paging on the grouped query

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
