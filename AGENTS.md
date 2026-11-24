# Screenroom — agent instructions

**Screenroom Personal Edition** is one person's own media on that person's own devices, in one monorepo at `~/Desktop/projects/screenroom`. No submodules, sibling repos or unwritten local dependencies: every first-party app, crate, script, test, design source and decision lives here, while SDKs, OS runtimes, caches and user videos stay external and out of Git.

**Governance.** [`docs/ADR-002_RUST_FIRST_STACK.md`](docs/ADR-002_RUST_FIRST_STACK.md) is the stack decision, amended by [`docs/ADR-003_DESKTOP_AND_TV_SCOPE.md`](docs/ADR-003_DESKTOP_AND_TV_SCOPE.md) and [`docs/ADR-004_PERSONAL_EDITION_SCOPE.md`](docs/ADR-004_PERSONAL_EDITION_SCOPE.md) (which platforms exist), and by [`docs/ADR-005_SUBTITLE_PROVIDER_AND_FEATURE_REMOVALS.md`](docs/ADR-005_SUBTITLE_PROVIDER_AND_FEATURE_REMOVALS.md) (subtitles, VoiceOver, HDR).

**macOS only, until the owner says otherwise (2026-11-05).** The sole active objective is Screenroom complete, polished and production-ready on the owner's MacBook Air M4; features, UI, performance, fixes, tests and evidence are scoped to macOS. The Windows ultrawide desktop ([`docs/WINDOWS_VALIDATION.md`](docs/WINDOWS_VALIDATION.md)) and the Samsung UA50AU8000RXER television (`apps/tv`, [`docs/TIZEN_DEVICE_HANDOFF.md`](docs/TIZEN_DEVICE_HANDOFF.md)) are **deferred**: preserve their code, contracts, adapters and documentation, but do not plan, implement, tune, install or verify anything for them. Their gates stay `BLOCKED`, never `PENDING`, and no session claims progress there until the owner asks for that platform. Do the macOS part of a mixed request and say plainly that the rest is deferred.

**Pins, verified in `Cargo.toml` / `Cargo.lock`.** Rust edition 2024; iced 0.14 (0.14.0); wgpu 27 (27.0.1), and it must match iced; rusqlite 0.41 (bundled); libmpv loaded at runtime, never linked into Git; FFmpeg/FFprobe for media work; MIT for original first-party code.

**Superseded and archived, never in force.** "Flutter is the selected stack" (ADR-001), "Svelte/Tauri is the baseline", every iOS/Android phone or tablet requirement (ADR-003), the ADR-003 platform list with Linux, LG webOS and Android TV (ADR-004), OpenSubtitles as the subtitle provider (ADR-010), the HDR presentation path (ADR-019) and the VoiceOver accessibility bridge (ADR-017). Do not act on them; do not delete them — the archive is the audit trail.

**Subtitles resolve from the file outwards.** An embedded track, then a sidecar beside the media, and only then The Subtitle DB — the single online provider, with no second service and no fallback chain. The key comes from `SCREENROOM_TSDB_API_KEY`, `SCREENROOM_TSDB_CONFIG` or `subtitles.key` in the platform config directory; the value never enters Git, a log, a test or a document. [`docs/SUBTITLE_PROVIDER.md`](docs/SUBTITLE_PROVIDER.md) is the reference, and every live call is `PENDING` until a recorded run says otherwise.

**SDR only; screen-reader output is not a feature.** No HDR, and no accessibility tree is published. The accessibility requirements are keyboard navigation, focus handling, reduced motion and subtitle styling.

## Operating boundaries

**Installed local app contract.** Every target is an installed local app: the desktop executable in `apps/desktop`, or a locally signed Tizen `.wgt` for the deferred TV. Never a hosted website, PWA, browser tab, always-on server or localhost HTTP server; the owner needs no browser, internet or server for normal playback, browsing, resume or lists.

**Design authority: the private Dropout Studio Design System.** It is the canonical visual and component source. Before any UI work, inspect its actual `DESIGN.md`, `GUIDELINES.md`, `README.md`, canonical DTCG `src/lib/tokens/tokens.json`, component lifecycle and current commit. Vendor it only through its approved sync path at an immutable, reviewed, commit-pinned revision — never a public registry mirror, an unreviewed paste, or private source and tokens in Git, planning bundles, public packages or screenshots. The desktop pin works like this:

- `apps/desktop/examples/export_ds_tokens.rs` reads the private DS at its pinned commit and writes `apps/desktop/src/design/ds_pinned.rs`.
- That generated file is Git-ignored; the exporter and the neutral fallback in `apps/desktop/src/design/mod.rs` are committed.
- `apps/desktop/build.rs` exposes the pin when the file is present and the fallback otherwise, so a clean checkout builds without the DS.
- The active theme string `dropout-design-system@<commit> semantic.dark` is visible in the running application, so a screenshot shows the pin used.

See [`docs/DROPOUT_DESIGN_INTEGRATION.md`](docs/DROPOUT_DESIGN_INTEGRATION.md).

## Repository layout

- `crates/screenroom-core` — domain, SQLite library, scanning, filename interpretation and fingerprints. No UI, network or platform player.
- `crates/screenroom-player` — the libmpv runtime boundary and player state machine.
- `apps/desktop` — the iced + wgpu client and the libmpv → wgpu presentation bridge.
- `apps/tv` — the LightningJS 3 + Blits client for the deferred TV; it follows the documented core contract.
- `docs/` — ADRs, architecture, roadmap, gates, licensing; the macOS backlog is [`docs/MACOS_REMAINING_WORK.md`](docs/MACOS_REMAINING_WORK.md).
- `reference/` — preserved historical prototype, read-only source of ideas and never the active build.

## Hard rules

- Never modify, rename, delete, move or overwrite user media: scan and read in place, and report failures instead of cleaning up destructively. Conversions need explicit user intent and write to separate output.
- No required paid service, SaaS, cloud account, always-on host, AI inference fee or hosted sync; never enable automatic purchases, submissions, posts or deployment.
- No pirate-site integration, scraping, authentication automation, access-control bypass or distribution of protected works without a rights check. A user-listed source is a bookmark and a capability question, never assumed permission.
- Source state is not user approval and not hardware certification. No invented Samsung, Tizen, Windows or macOS receipts; a mock or browser-only test is not a device test.
- Security: parameterized arguments without shell interpolation of filenames, validated IPC and bindings, canonicalized authorized media roots, bounded parsers and inputs, safe URL allowlists, and no credentials, tokens, certificates or private viewing data in Git, logs, backups or exports. No destructive cleanup on scan misses or disconnects.
- Verify version-sensitive APIs against the locked `Cargo.lock` release and use `scripts/docs-lookup.sh` for current official documentation.
- Preserve uncommitted work, record decisions as dated ADRs with evidence, and update the capability matrix and status ledgers. Domain IDs stay independent of file path and provider ID; persist transactionally, migrate without silent resets, and keep per-device state independent — there is no cross-device sync requirement.
- Bound runtime work and queue heavy jobs outside watching. Prove offline behaviour with the network disabled and the app restarted, persisting permitted actual image bytes rather than remote URLs. An unsupported per-platform combination gets a truthful reason and a recovery path — no dead buttons, fake data or pretend format support.
- Use generated or rights-cleared test media, document reproducible commands and results, and never label a mock-only test a device check. A milestone closes only with satisfied acceptance checks or an explicit owner-accepted exception, reported as Completed / Verified / Blocked / Next with evidence.

## No code comments

No comment of any kind in any active implementation, test, script or config file: no line, block, inline or doc comment, no commented-out code, and no `TODO`, `FIXME`, `HACK` or `NOTE` markers. Only a shebang, a legally required licence header, or a build-essential toolchain directive may remain, and that exception is named in the commit message. Documentation belongs in `docs/`, `AGENTS.md` or a README, never in source; write self-explanatory code instead, and never reintroduce a comment because it was there before.

Enforced, not merely stated: `python3 scripts/no_comments.py check` lexes every tracked source file with the same per-language lexer as `strip` and exits non-zero on a comment. Run it with the other checks before committing.

## Background-first testing

Development and verification run on the owner's MacBook while they use it. A check may not bring Screenroom to the front, switch Spaces, move the owner's active window, or take the keyboard and mouse. In order of preference:

1. Headless — `cargo test --workspace` and the `--example` measurements open no window; new coverage defaults here.
2. Hidden window — launch with `SCREENROOM_WINDOW=hidden`, never call `activate`, and verify with `SCREENROOM_ALLOW_FOREGROUND=0` that the frontmost application does not change. `scripts/check-quit-routing.sh` is the model: real process, hidden window, outcome written to a file, exit.
3. Programmatic entry points — prefer a deterministic, environment-gated check, a database assertion or a published frame over automating the visible UI.
4. Isolated graphical session — only when a visible window is genuinely needed and an isolated environment is available; no heavyweight VM and no global macOS setting changes.

**Scratch libraries.** Any check, script or development launch that adds a media source must set `SCREENROOM_LIBRARY` to a scratch database, never the installed application's `~/Library/Application Support/Screenroom/library.db`. Recover a residual test source non-destructively — back up the database, then set `files.available = 0` for that source and `sources.state = 'removed'` — and never delete media or rows. `scripts/verify-watch.sh` is the model.

**Foreground automation is opt-in.** `scripts/drive-app.sh`, the raise in `scripts/capture-window.sh` and `scripts/verify-quit.sh` require `SCREENROOM_ALLOW_FOREGROUND=1` through `scripts/foreground-guard.sh`; never set it on the owner's behalf. An unapproved visible check is `PENDING` with its exact steps, never passed and never quietly skipped. Passive `screencapture -l <window>` needs no permission.

## Gates

Read [`docs/FEASIBILITY_GATES.md`](docs/FEASIBILITY_GATES.md) first; gates are per capability and per device, never per framework. Active macOS gates are presentation (a libmpv frame through wgpu in an iced window), SDR playback quality, concurrency and durability, and **G10** macOS automatic updates in [`docs/ADR-006_AUTOMATIC_UPDATES.md`](docs/ADR-006_AUTOMATIC_UPDATES.md): a signed newer bundle found, verified and installed through the real installed app, plus refusal of a tampered archive, an unsigned feed and a downgrade. Windows and Tizen gates are `BLOCKED` (deferred); LG webOS, Android TV and iOS gates are retired. An inaccessible device stays `BLOCKED` — never greenwash a missing gate.

## Commands

```bash
scripts/verify.sh --fast        # repository check suite (--full for everything)
scripts/docs-lookup.sh          # version-checked official docs for a dependency
python3 scripts/no_comments.py check
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p screenroom-player --example capability
cargo run --release -p screenroom-core --example library_bench
cargo run --release -p screenroom-player --example render_bench
cargo run --release -p screenroom-player --example hwdec_measure
cargo run -p screenroom-core --example prepare_library -- LIB.DB MEDIA_ROOT
```

Prefer the `--example` measurements over opening the app: they report real numbers and need no window. Local macOS helpers, nothing published:

```bash
scripts/package-macos.sh [DEST]      scripts/verify-bundle.sh [APP]
scripts/check-quit-routing.sh        scripts/check-terminate-routing.sh
scripts/check-no-network.sh [MEDIA]  scripts/probe-library.sh ROOT OUT.jsonl
scripts/capture-window.sh OUT.png    scripts/drive-app.sh library|capture NAME
```

## Publishing and auto-rebuild

After any commit that changes the application, and after pushing it, the agent runs `scripts/publish-update.sh --push --if-changed`: it pushes, bumps the version, builds the LGPL media stack when needed, packages, signs the appcast from the login keychain, publishes the GitHub release with the corresponding source and reinstalls this Mac's copy. `--if-changed` makes it a no-op for documentation- or script-only commits; it refuses a dirty tree and a reused version, and waits for the rebuild lock. Never ask the owner whether to publish. If network or keychain blocks it, say so plainly and leave the commit pushed.

`scripts/install-app-hooks.sh` points `core.hooksPath` at the tracked `scripts/git-hooks/` once per clone. `post-commit`, `post-merge` and `post-rewrite` start `scripts/rebuild-app.sh` detached, so one build runs at a time and no commit is left unbuilt; `--status` reports state, `--unlock` clears a lock left by a killed run, `--force` rebuilds anyway, and `SCREENROOM_SKIP_REBUILD=1` skips one commit. The worker builds `HEAD`, verifies the bundle and atomically swaps `~/Applications/Screenroom.app` with `renamex_np(RENAME_SWAP)`; a failed build never leaves a half-written app. A running application keeps its old code until relaunched. Details in [`docs/APP_REBUILD_AUTOMATION.md`](docs/APP_REBUILD_AUTOMATION.md).

## Gotchas

Full context for each item lives in `.trail/learnings/`.

- mpv event ids come from `mpv/client.h`, never memory. Seek or resume only after `PLAYBACK_RESTART` for the same entry, and store progress only from a snapshot matching the current `load_id` with `position_ready` and a high enough `seek_serial`; every early return in `Worker::seek` must settle its serial, or checkpoints stop.
- Any automatic, position-driven action in the desktop `poll()` — skip intro, skip credits, chapter jumps — must check `snapshot_is_current` first.
- macOS decodes with `auto-copy` hwdec: the software render bridge needs CPU frames.
- A Dock-launched app has no Homebrew `PATH`; resolve `ffmpeg`/`ffprobe` through `artwork::tool` (PATH, then `/opt/homebrew/bin` and `/usr/local/bin`).
- Capture by window number (`screencapture -l`), never a screen rectangle: a rectangle records whatever is on top of it, including the owner's own viewing. Delete any capture that is not Screenroom's UI; never keep it as evidence.

**Trail rebuilds are automatic.** The graph refreshes before each query; never run
`trail build` by hand after a change. Only a genuinely missing map (first query in a fresh
checkout, or an explicit failure) may be recovered with a single build. This rule sits outside
the generated block below, so it survives a Trail wiring refresh.

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
