# Screenroom — agent instructions

**CURRENT, SUPERSEDING GOVERNANCE (2026-10-27):** Screenroom is the approved
product name, the only project root is `~/Desktop/projects/screenroom`, and the
product is **Screenroom Personal Edition**: one person's own media on that
person's own three devices.
[`docs/ADR-002_RUST_FIRST_STACK.md`](docs/ADR-002_RUST_FIRST_STACK.md) is the
authoritative stack decision, amended by
[`docs/ADR-003_DESKTOP_AND_TV_SCOPE.md`](docs/ADR-003_DESKTOP_AND_TV_SCOPE.md)
and then by [`docs/ADR-004_PERSONAL_EDITION_SCOPE.md`](docs/ADR-004_PERSONAL_EDITION_SCOPE.md),
which is the current word on **which platforms exist**, and by
[`docs/ADR-005_SUBTITLE_PROVIDER_AND_FEATURE_REMOVALS.md`](docs/ADR-005_SUBTITLE_PROVIDER_AND_FEATURE_REMOVALS.md)
(2026-11-03), which is the current word on **subtitles, VoiceOver and HDR**.

**Scope: exactly three targets.** (1) the owner's MacBook Air M4 on macOS,
(2) the owner's Windows ultrawide desktop with an Intel Arc B580, (3) the owner's
Samsung UA50AU8000RXER television. Linux, LG webOS, Android TV, phones and
tablets, Apple TV, Roku, UNiFFI, the Flutter fallback, public distribution and
commercial infrastructure are out of scope (ADR-004).

**Development priority (2026-11-05): macOS first, and only macOS until the owner
says otherwise.** The sole active objective is making Screenroom complete, polished,
stable and production-ready on the owner's MacBook Air M4. Everything a session does
— features, UI and UX, performance, bug fixes, tests, evidence, reviews — is scoped
to macOS. The Windows desktop and the Samsung television are **deferred**: preserve
their existing code, contracts, adapters and documentation, but do not plan,
implement, tune, install or verify anything for them, and do not proactively revisit
them or propose work on their behalf. Their gates stay `BLOCKED` (never `PENDING`)
and no session may claim progress on them until the owner asks for that platform
explicitly. If a request would require work on another platform, do the macOS part
and say plainly that the rest is deferred rather than starting it.

Rust-first: shared Rust core, iced + wgpu + libmpv on desktop, LightningJS 3 +
Blits + TypeScript with Tizen AVPlay playback on the TV (WebGL/Canvas rendering),
SQLite where supported, FFmpeg/FFprobe for media work, Dropout Design System for
visuals, Cargo + pnpm + the Tizen Studio CLI for builds, MIT for original
first-party code.

Older statements are **SUPERSEDED** and are history, not requirements:
“Flutter is the selected stack” (ADR-001), “Svelte/Tauri is the baseline”,
“Screenroom is a backup name”, “Samsung TV cannot run `.tpk`”, “the UI must be a
WebView”, every iOS/Android phone or tablet requirement (ADR-003), the
ADR-003 platform list that included Linux, LG webOS and Android TV (ADR-004),
OpenSubtitles as the subtitle provider (ADR-010), the HDR presentation path
(ADR-019) and the VoiceOver accessibility bridge (ADR-017) — all three removed on
2026-11-03 by ADR-005. Do not act on them. Do not delete them: the archive is the
audit trail.

Keep one monorepo, offline-first/zero-paid constraints and every hardware gate.
Never claim a test, a device result or a rendering receipt that did not happen.

**Subtitles resolve from the file outwards** (ADR-005): an embedded track, then a
sidecar beside the media, and only when neither exists may the app ask The Subtitle
DB — the single online subtitle provider, with no second service and no fallback
chain. The key is read from `SCREENROOM_TSDB_API_KEY`, `SCREENROOM_TSDB_CONFIG` or
`subtitles.key` in the platform config directory; its value never enters Git, a
log, a test or a document. `docs/SUBTITLE_PROVIDER.md` is the reference, and every
live provider call is PENDING until a recorded run says otherwise.

**HDR and screen-reader output are not features.** SDR is the only desktop output
and no accessibility tree is published; keyboard navigation, focus handling,
reduced motion and subtitle styling are the accessibility requirements.

## Operating boundaries

**Installed application contract.** Every target is an installed local app: the
desktop executable (`apps/desktop`) or a locally signed Tizen `.wgt`. Never a
hosted website, PWA or browser tab. The owner must not need a browser, internet,
an always-on server or a localhost HTTP server for normal playback, browsing,
resume or lists.

**Design authority.** Follow the owner's private **Dropout Studio Design System**
as the canonical visual/component source. Before any UI work, inspect its actual
`DESIGN.md`, `GUIDELINES.md`, `README.md`, canonical DTCG
`src/lib/tokens/tokens.json`, component lifecycle and current commit. Apply
reviewed, immutable-commit-pinned private vendoring inside this monorepo through
the DS's approved sync path. Never fetch a public registry mirror, paste an
unreviewed copy, or publish private DS source in planning bundles, public
packages or screenshots. Each platform adapts the same semantic tokens and
documented control behaviour; no platform may silently fork the design language.
The original pale-green prototype palette is superseded.

The desktop integration is already built and works like this:

- `apps/desktop/examples/export_ds_tokens.rs` reads the private DS at its pinned
  commit and writes a flat Rust pin to `apps/desktop/src/design/ds_pinned.rs`.
- That generated file is **Git-ignored**; only the exporter and the neutral
  fallback in `apps/desktop/src/design/mod.rs` are committed.
- `apps/desktop/build.rs` puts the file on the include path when it is present
  and otherwise exposes the fallback, so a clean checkout builds without the DS
  and a machine with the DS gets the real tokens.
- The active theme string (`dropout-design-system@<commit> semantic.dark`) is
  visible in the running application, so a screenshot shows which pin was used.

See [`docs/DROPOUT_DESIGN_INTEGRATION.md`](docs/DROPOUT_DESIGN_INTEGRATION.md).

**Name.** Screenroom is finalized. Historical reference material keeps the old
labels; active code and docs use Screenroom.

## Repository layout

- `crates/screenroom-core` — Rust domain, SQLite library, scanning, filename
  interpretation, fingerprints. No UI, no network, no platform player.
- `crates/screenroom-player` — libmpv runtime boundary and the player state
  machine. libmpv is loaded at runtime, never linked into Git.
- `apps/desktop` — iced + wgpu desktop client (macOS and Windows), including the
  libmpv → wgpu presentation bridge.
- `apps/tv` — the LightningJS 3 + Blits 2.15 client for the owner's Samsung Tizen
  TV (pnpm; tests in `apps/tv/test`). It does not run the Rust core; it follows
  the documented core contract. Device state lives in
  [`docs/TIZEN_DEVICE_HANDOFF.md`](docs/TIZEN_DEVICE_HANDOFF.md).
- `docs/` — ADRs, architecture, roadmap, progress ledger, gates, licensing. The
  macOS backlog is [`docs/MACOS_REMAINING_WORK.md`](docs/MACOS_REMAINING_WORK.md):
  every outstanding requirement, its evidence, its blocker and what closes it.
- `reference/` — preserved historical prototype and handoff. Read-only source of
  ideas; never the active build.

Keep **every first-party** app, crate, package, script, test, design source,
decision and development document in this single root. No submodules, no sibling
repos, no unwritten local dependencies. SDKs, OS runtimes, caches and user videos
are external and are not part of Git.

## Hard rules

- Never modify, rename, delete, move or overwrite user media. Scan and read in
  place; report failures instead of cleaning up destructively. Conversion jobs
  need explicit user intent and write to separate output.
- No required paid service, SaaS infrastructure, cloud account, always-on host,
  AI inference fee or hosted sync. Never enable automatic purchases, submissions,
  posts or deployment.
- No pirate-site integration, scraping, authentication automation, access-control
  bypass or distribution of protected works without a rights check. A user-listed
  source is a bookmark and a capability question, never an assumed permission.
- **Source state ≠ user approval ≠ hardware certification.** No invented Samsung,
  Tizen, Windows or macOS receipts. A mock or a browser-only test is
  not a device test.
- Verify version-sensitive work against current official docs for the *actual*
  selected release, and respect the lockfiles. Current provisional pins: iced
  0.14 / wgpu 27 (must match iced), Lightning 3 / Blits 2.x, Rust edition
  2024.
- Security: parameterized arguments (no shell interpolation of filenames),
  validated IPC/bindings, strict media-root authorization and canonicalization,
  bounded parser and file inputs, safe URL allowlists, no credentials in
  Git/logs/backups/exports. No destructive cleanup on scan misses or disconnects.
- The private Dropout Design System source and generated private token values
  stay out of Git and out of any public artifact.

## No code comments — mandatory, every agent

This repository has **no comments in its code**, and none may be added. The rule
covers every language, every file and every kind of change — implementation,
refactor, debug, review, test, script, configuration — and every agent or
subagent that works here, whoever launched it.

- Never write a comment: no line, block, inline, doc or explanatory comment, in
  any language.
- Never commit commented-out code, and never add `TODO`, `FIXME`, `HACK`, `NOTE`
  or similar markers.
- Write self-explanatory code instead: names that say what a thing is, small
  functions, types that carry the meaning, structure that shows the intent.
- Documentation belongs in `docs/`, `AGENTS.md` or a README — never in source.
  The rationale that used to sit above a function belongs in the document that
  covers that area.
- Never reintroduce a comment "because it was there before". The tree has none.

Permitted, because they are not commentary: a shebang line; a legally required
licence header; and a directive the toolchain needs in order to work at all —
where removing it would change what the build does. Name such an exception in the
commit message. Nothing in the current tree needs one.

**Enforced, not merely stated.** `python3 scripts/no_comments.py check` lexes
every active source file with a per-language lexer — a `//` inside a string, a
`#` inside `${var#pattern}` and a `/*` inside a TypeScript regex are not mistakes
for comments — and exits non-zero if it finds one. `strip` is the same lexer in
rewrite mode, so the check cannot disagree with the rewrite. Both read
`git ls-files`, so `docs/`, `reference/`, generated output and lockfiles are out
of scope. Run `check` with the other checks before committing.

## Testing without taking over the desktop

**Background-first, always.** Development and verification run on the owner's own
MacBook while they are using it. A test may not bring Screenroom to the front,
switch Spaces, move the owner's active window, or take the keyboard and mouse —
not even briefly, and not "just to check something".

In order of preference:

1. **Headless.** `cargo test --workspace` and the player examples never open a
   window. Prefer a unit, contract or integration test here. New coverage should
   default to this tier.
2. **Hidden window.** Where a check genuinely needs the running application's own
   platform machinery — AppKit's menu bar, the event loop, the real database —
   launch with `SCREENROOM_WINDOW=hidden`, or a check mode that implies it, and
   must never call `activate`. `scripts/check-quit-routing.sh` is the model: real
   process, hidden window, outcome written to a file, exit. Verify with
   `SCREENROOM_ALLOW_FOREGROUND=0` that the frontmost application does not change.
3. **Programmatic test interfaces.** When a behaviour currently needs a keystroke
   and a visible window, prefer replacing that with a deterministic entry point
   (an environment-gated check, an assertion over the database or the published
   frame) over automating the visible UI. Keep the assertions at least as strong
   as the interaction they replace.
4. **Isolated graphical session.** Only for what genuinely needs a visible window,
   and only after checking that a supported isolated environment is available. Do
   not add heavyweight virtual machines, do not change global macOS settings, and
   do not disturb unrelated applications.

**Foreground UI automation is opt-in.** `scripts/drive-app.sh`, the raise in
`scripts/capture-window.sh` and `scripts/verify-quit.sh` are gated on
`SCREENROOM_ALLOW_FOREGROUND=1` through `scripts/foreground-guard.sh`, and refuse
otherwise. Do not set that variable on the owner's behalf. When a check needs a
visible window and permission has not been given, record it as **PENDING** with
its exact steps and consolidate it into a later owner-approved session — never as
passed, and never quietly skipped.

Capturing a window (`screencapture -l <window>`) is passive and does not need
permission; only activating the application does.

## Required evidence gates

Read [`docs/FEASIBILITY_GATES.md`](docs/FEASIBILITY_GATES.md) first. Gates are per
capability and per device, never per framework:

1. **Desktop presentation** — a libmpv frame must be presented through wgpu in an
   iced window on real Windows hardware (and macOS for the OpenGL bridge).
   Compiling the bridge is not presenting a frame.
2. **Desktop playback quality** — hardware decode, 1080p/4K, audio and
   subtitle switching measured on the owner's GPU. SDR only: HDR was removed on
   2026-11-03 (ADR-005) and is not a gate.
3. **Samsung Tizen (physical UA50AU8000RXER)** — signed `.wgt` install, Lightning 3
   bundle running on the Tizen 6.0 engine, USB enumeration, AVPlay local playback,
   lifecycle/focus and durable storage/artwork. The certificate profile is created
   locally with the owner's own Samsung account; no package or key material is
   committed.
4. **Concurrency and durability** — interruption, power loss, low storage and
   stale player callbacks on the real device.
5. **macOS automatic updates** (G10) — a signed newer bundle found, verified and
   installed through the real installed app, plus refusal of an altered archive,
   an unsigned feed and a downgrade. A local signed feed counts; a public release
   needs the libmpv licence cleared first and is not evidence of anything until it
   exists.

Retired gates: LG webOS, Android TV (ADR-004) and iOS (ADR-003). Do not reopen one
without a new ADR.

If a device is inaccessible, mark its gate **BLOCKED** and keep working on what is
independently verifiable. Never greenwash a missing gate.

## Implementation discipline

1. Check actual repo state and uncommitted work before editing. Preserve, do not
   overwrite.
2. Record design/architecture/provider decisions as dated ADR entries with
   evidence; update the capability matrix and progress ledger when behaviour
   changes.
3. Build thin vertical slices with runnable tests and typed boundaries. Priority
   order: authorized source → real playable file → durable progress → parity on
   each priority device.
4. Domain IDs stay independent of file path and provider ID. Persist
   transactionally and migrate without silent resets. Per-device state is
   independent by design; there is no cross-device sync requirement.
5. Prove offline behaviour with the network disabled and the app restarted.
   Persist **permitted actual image bytes**, not remote URLs.
6. Capabilities are per platform. An unsupported combination gets a truthful
   reason and an achievable recovery path. No dead buttons, fake data, false empty
   results, or pretend format support.
7. Bound runtime work; queue heavy jobs outside watching; measure on the actual
   device. TV D-pad focus must be deterministic and must restore predictably.
8. Use generated or rights-cleared test media. Never commit private viewing
   content, accounts, tokens, DUIDs, certificates, passwords or addresses.
9. Document reproducible commands and results for unit, contract, integration and
   physical-device checks. Do not label mock-only tests as device checks.
10. A milestone is complete only with satisfied acceptance checks or an explicit
    user-accepted exception. Report **Completed / Verified / Blocked / Next**
    separately, with links to evidence.

## Commands

```bash
cargo test --workspace          # core + player + desktop logic tests
cargo clippy --workspace --all-targets
cargo fmt --all --check
cargo run -p screenroom-player --example capability   # libmpv on this device
cargo run --release -p screenroom-core --example library_bench      # paging budget
cargo run --release -p screenroom-player --example render_bench     # offscreen frame budget
cargo run --release -p screenroom-player --example hwdec_measure    # decode rate per file
cargo run --release -p screenroom-desktop                           # desktop shell (opens a window)

cd apps/tv && pnpm install && pnpm typecheck && pnpm test   # TV logic, node --test
cd apps/tv && pnpm build        # typecheck + Vite + Tizen staging into dist/
```

Prefer the `--example` measurements over opening the app: they report real numbers
and need no window, so they never interrupt the owner. Sign and install the `.wgt`
through the Tizen skills (`tizen-build-project`, `tizen-install-app`), never by hand.

Helpers (`scripts/`, all local, nothing published):

```bash
scripts/package-macos.sh [DEST]              # dist/Screenroom.app, ad-hoc signed, bundled libmpv
scripts/verify-bundle.sh [APP]               # bundle checks without opening it
scripts/check-quit-routing.sh                # hidden-window model check
scripts/check-terminate-routing.sh           # Dock Quit / logout routes through the close path
scripts/check-no-network.sh [MEDIA] [SEC]    # playback opens no socket (not a Wi-Fi-off proof)
scripts/probe-library.sh ROOT OUT.jsonl      # read-only ffprobe of a real library
scripts/capture-window.sh OUT.png            # Screenshot of Screenroom's own window
scripts/drive-app.sh library|enter|space|left|right|capture NAME   # foreground, gated
cargo run -p screenroom-core --example prepare_library -- LIB.DB MEDIA_ROOT

python3 scripts/no_comments.py check         # the no-comments rule, lexed per language
```

## Every application change is published, by the agent

**Rule (2026-11-05, owner): the agent publishes the update; the owner never does it by
hand.** After any commit that changes the application, and after pushing it, run:

```bash
scripts/publish-update.sh --push --if-changed
```

It pushes the commit, bumps the version, builds the LGPL media stack if needed,
packages, signs the appcast from the login keychain, publishes the GitHub release
with the corresponding source, and reinstalls this Mac's copy. The installed app
then updates itself on its next launch.

- `--if-changed` makes it safe to run after every commit: it exits without a release
  when nothing under `apps/`, `crates/`, `Cargo.*` or `assets/` moved since the last
  one, so documentation, decisions and script-only commits publish nothing.
- A version is never reused and a published asset is never replaced; the command
  refuses a dirty tree and an existing release.
- It waits for the self-rebuild lock, so it can follow a commit immediately.
- Do not ask the owner whether to publish, and do not leave an application change
  unpublished at the end of a session. If publishing is impossible (no network, the
  keychain locked), say so plainly and leave the commit pushed.

## The installed macOS app rebuilds itself

On this MacBook every relevant commit rebuilds and reinstalls the application at
`~/Applications/Screenroom.app`, with no build command to remember. This is macOS
only: Windows and the Tizen client are untouched.

- `scripts/git-hooks/` holds the tracked hooks, and `scripts/install-app-hooks.sh`
  points `core.hooksPath` at it (idempotent; run it once per clone).
- `post-commit`, `post-merge` and `post-rewrite` start `scripts/rebuild-app.sh`
  detached, so Git never waits and never fails because of a build.
- One build runs at a time. A trigger that arrives mid-build queues a single
  follow-up and then waits for the builder, and the installed commit recorded beside
  the Git directory is the arbiter: nothing is built twice, and no commit is left
  unbuilt. `--status` reports the app, the lock and the queue; `--unlock` clears a
  lock left by a killed run.
- The worker builds `HEAD`, verifies the bundle, and only then swaps the installed
  app with `renamex_np(RENAME_SWAP)` — one atomic exchange with the staged build in
  `~/Applications/.screenroom-builds/`. **A failed or interrupted build never leaves
  a missing or half-written app**; it is logged under `.artifacts/builds/` and
  retried at the next commit.
- A commit touching nothing under `apps/`, `crates/`, `assets/`, `scripts/`,
  `vendor/` or `Cargo.*` is recorded without a rebuild.
- `scripts/rebuild-app.sh --status` reports the installed commit, `HEAD` and the
  last result; `--force` rebuilds anyway. `SCREENROOM_SKIP_REBUILD=1` skips one
  commit deliberately (a bisect, say).
- A running Screenroom keeps its old code until relaunched.

## Gotchas

Full context for each is in `.trail/learnings/`.

- mpv event ids come from `mpv/client.h`, never memory. A seek or resume is done
  only after `PLAYBACK_RESTART` for the same entry; store progress only from a
  snapshot matching the current `load_id` with `position_ready` and a high enough
  `seek_serial`, or a fresh load writes position 0 over the resume point. Every
  early return in `Worker::seek` must still settle its serial, or checkpoints stop.
- Any automatic, position-driven action in the desktop `poll()` (skip intro, skip
  credits, chapter jumps) must check `snapshot_is_current` first.
- macOS decodes with `auto-copy` hwdec: the software render bridge needs CPU frames.
- A Dock-launched app has no Homebrew `PATH`; resolve `ffmpeg`/`ffprobe` through
  `artwork::tool` (PATH, then `/opt/homebrew/bin` and `/usr/local/bin`).
- Windows cannot be cross-checked from macOS (bundled SQLite needs MSVC headers);
  its gate stays BLOCKED until it runs on the Windows machine.
- Blits only re-renders attributes prefixed with `:`; a bare `content="$x"` is read
  once at creation and shows nothing set later.
- A Tizen widget is a `file://` origin, so `type="module"` scripts never run (black
  screen); the staging step ships a classic script. `webapis` is a sibling of
  `tizen`, not a child.
- TV app source typechecks with no ambient types; only tests see Node globals.
- On the TV over TCP, `sdb shell` is whitelisted and `sdb dlog` returns nothing; the
  app's own storage (`sdb pull`) is the only evidence channel.

Capture the **window number**, never a screen rectangle: a rectangle records
whatever is on top of it, so on a multi-display desktop it can silently contain a
different application — including the owner's own viewing. If a capture is not
Screenroom's UI, delete it; never keep it as evidence.

<!-- trail:start -->
## Trail — repo context graph

This repo is indexed in `.trail/cache/`: small linked markdown nodes that explain each
system and carry exact file:line spans, kept in sync with the code through git.

For ANY task here — understanding how something works, finding where code lives,
or scoping a change — get context from the graph before grepping or opening
source files. Re-ask freely (it's cheap) and reuse literal identifiers you
already have (symbol, error string, file name) as the query. New to this repo?
Run `trail map` first — a token-budgeted orientation (dir clusters, hubs,
hotspots), no LLM, no key.

- Run `trail ask "<your question>" --source` → ranked nodes with the relevant
  code spans inlined (each hit's ≤8-line crux by default; `--full` for whole
  definitions when the crux isn't enough). Match the tool to the task shape:
  for understanding or editing, the top node IS the answer — cite its
  `covers:` file:line spans and edit straight from `--source`. For
  exhaustive tasks ("every occurrence / every caller of this pattern"), ranked
  results are top-N, not complete — run `trail grep "<literal>"` instead
  (exhaustive over indexed files, grouped by enclosing symbol), falling back
  to raw `grep -rn` only for unindexed files.
- `trail skeleton <file>` → every definition's signature + span, ~10× cheaper
  than reading the file; use it to skim an API surface.
- `trail callers <symbol>` gives precomputed, exact edges — who calls this.
  Add `--direction out` for what it calls, or `--depth N` to walk
  transitively for the full blast radius. For structural questions, skip
  ranking and use this directly.
- Or browse: `.trail/cache/INDEX.md` lists every node; follow the links.
- Monorepos and folders of multiple repos rank fairly across sub-projects —
  hits carry `[scope/]` labels naming which one they're from. Narrow with
  `trail ask "<task>" --in <scope>/` once you know where you're working.

If a returned span is truncated ("+N more lines"), open the file at that exact
range before finalizing. Only open source files when a node genuinely lacks a
needed detail, and then at the exact file:line the node points to — never
re-read whole files.

The graph refreshes automatically before each query, so no manual rebuild is
needed after a big change — just ask again.
<!-- trail:end -->
