# ADR-006 — Automatic updates on macOS with Sparkle 2

- **Status:** Accepted 2026-11-05. Implemented and verified against an isolated
  signed feed on this MacBook Air M4. **Public publication remains BLOCKED** on
  the libmpv licensing decision in [`LICENSING.md`](LICENSING.md); nothing in this
  ADR grants permission to publish a GPL-linked bundle.
- **Supersedes:** the updater half of `manifest.json` in
  [`RELEASE_PIPELINE.md`](RELEASE_PIPELINE.md). The SHA-256 manifest is kept as
  release metadata; it is no longer an update channel.
- **Scope:** macOS only. Windows and the Samsung television are deferred
  (ADR-004) and receive no update mechanism here.

## Decision

### 1. Sparkle 2 is the update engine

Screenroom embeds **Sparkle 2.10.1** (`Sparkle-2.10.1.tar.xz`, SHA-256
`1a856d2a9edb8e628de88121d7be04cf0e2167f8641650b78fca00b14244cf27`), fetched by
`scripts/fetch-sparkle.sh`, Git-ignored under `vendor/sparkle/`, and copied into
`Contents/Frameworks/Sparkle.framework` with its symlinks intact.

*Why.* Sparkle is the maintained macOS updater: it already implements the appcast
protocol, Ed25519 archive verification, feed verification, signed-URL HTTPS
download, atomic replacement of a running bundle, an installer that waits for the
old process to exit, and downgrade and architecture checks. Writing any of that
again would be a second, less reviewed security-critical component.

Sparkle 2.10 requires macOS 12.0, so `LSMinimumSystemVersion` moves from 11.0 to
12.0. That is a real scope cost and it is accepted: the owner's MacBook Air M4
runs a current macOS.

2.10.1 rather than 2.10.0: 2.10.1 is the newest stable release (2026-10-25) and
fixes a premature install for NFC-encoded paths. Both need macOS 12.

### 2. A small Objective-C bridge, and a Rust policy layer

- `apps/desktop/native/screenroom_updater.{h,m}` is a C header plus one
  Objective-C file. It creates an `SPUUpdater` with a custom **`SPUUserDriver`**
  that draws nothing: every state is pushed to Rust as a JSON line through a
  function pointer. It is the only file in the repository that imports Sparkle.
- `apps/desktop/src/updater.rs` owns the state machine, the policy, and the
  decoding of those events. It talks to the bridge through a `Backend` trait so
  every rule is unit-tested without a window, a network or a framework.
- `build.rs` compiles the bridge with the Command Line Tools' `clang`
  (`-fobjc-arc`, `-fmodules`) only when `Sparkle.framework` is present, and emits
  `cfg(screenroom_updater)`. It invokes `clang` directly rather than through a
  build-dependency, so `Cargo.lock` is untouched and the CI cache key does not
  move. The framework is linked **weakly** with an
  `@executable_path/../Frameworks` runpath, so `cargo test` and `cargo run` never
  need the framework; a build without it reports updates as unavailable instead
  of failing to launch.

*Why a custom driver rather than `SPUStandardUpdaterController`.* The standard
driver shows AppKit windows. Screenroom has one window, a 10-foot-style iced UI
and a hard rule that nothing may take focus or draw over a film. Sparkle's own
documentation says a custom `SPUUserDriver` is the supported way to present the
update UI yourself, and states the driver must present a user interface — here it
is the application's own interface, not a dialog.

### 3. What the owner sees, and when nothing may happen

- One scheduled check a day (`SUScheduledCheckInterval` 86400) and no check on
  every launch.
- A manual **Check now** in Settings → Updates (keyboard operable like every other
  Settings row), plus a single-line banner above the content on Home, Library,
  Detail and Settings.
- The banner and every update action are **absent on the player screen** and the
  policy refuses `InstallNow` while the player screen is open. Nothing can stop,
  restart, pause or cover a film.
- Automatic download is on by default (`SUAutomaticallyUpdate`), so a new version
  arrives in the background and installs **when Screenroom quits**, which is the
  documented Sparkle behaviour for a downloaded update that is dismissed. The
  install happens after `stop_playback()` has written the checkpoint.
- **Update now** closes Screenroom and installs; it does not relaunch. The delegate
  `updaterShouldRelaunchApplication` answers `NO`, so a restart is never imposed.
  Reopening the app shows the new version.
- Preferences are Sparkle's own (`automaticallyChecksForUpdates`,
  `automaticallyDownloadsUpdates` in `NSUserDefaults`), read back through the
  bridge. They are **not** duplicated in SQLite: two stores for one switch is how
  a setting starts disagreeing with itself.

### 4. Security posture

| Setting | Value | Effect |
| --- | --- | --- |
| `SUPublicEDKey` | committed public key | only archives signed by the owner's private key are accepted |
| `SUVerifyUpdateBeforeExtraction` | `YES` | the archive is verified before anything is unpacked |
| `SURequireSignedFeed` | `YES` | an unsigned or altered appcast is refused |
| `SUSignedFeedFailureExpirationInterval` | `0` | a feed-signing failure never expires into an unsigned fallback |
| `SUEnableSystemProfiling` | `NO` | no system profile is sent |
| `SUFeedURL` | GitHub `releases/latest/download/appcast.xml` | HTTPS only, no separate update server |

The private key was generated with Sparkle's `generate_keys` into the login
Keychain (account `screenroom`) and is also held, base64, in the GitHub Actions
secret `SCREENROOM_SPARKLE_ED_KEY`, which the release job passes to
`generate_appcast --ed-key-file -` on standard input. It is never written to the
repository, a build artefact, a log or a document. Recovery and rotation are in
[`RELEASE_PIPELINE.md`](RELEASE_PIPELINE.md) §7.

The public key lives in `apps/desktop/updater/ed25519_public_key.txt` and is
copied into `Info.plist` at packaging time; `scripts/package-macos.sh` refuses to
build a bundle without it, and `scripts/verify-bundle.sh` fails a bundle whose
feed is not HTTPS or whose verification flags are off.

### 5. Release channel

- The feed is
  `https://github.com/Dropout-Studio-Co/screenroom/releases/latest/download/appcast.xml`,
  which resolves to the newest **non-prerelease** release asset. No server is
  hosted; nothing needs the owner's Mac to be awake.
- Only an explicit `v*` tag, or a manual dispatch from `main` with `publish`
  ticked, creates a release. A push to `main` publishes nothing.
- `scripts/release.sh appcast` signs the feed with `generate_appcast` and fails if
  the key is missing or the result carries no `sparkle-signatures` block.
- Published assets are immutable: the publish job compares an existing asset byte
  for byte and refuses to replace it, and a tag that is not newer than the latest
  published tag is refused.
- The archive is a `ditto` zip of the app alone, so no read-only mount or
  translocation surprise hides an update.

### 6. What this ADR does not claim

No Developer ID, no hardened runtime, no notarisation: this Mac has no code-signing
identity, so the bundle is ad-hoc signed. Sparkle's archive and feed signatures are
independent of Apple code signing and are what the update path relies on. If a
Developer ID becomes available, `SCREENROOM_CODESIGN_IDENTITY` signs every nested
component inside-out without another code change; notarisation then needs
`notarytool` credentials and a workflow step that does not exist yet.

A public release also still needs a legally cleared libmpv (see `LICENSING.md`).
The updater is proved end to end against a local signed feed, which is the same code
path a GitHub release would use, and GitHub's own hosted runner has built the
bundle and signed the appcast (`Release` run 41961636886). No tag, release or
published asset exists.

## Evidence

`scripts/check-updater.sh` builds a signed 0.1.0 and 0.2.0, serves a signed feed
from `127.0.0.1`, installs 0.1.0 into a temporary directory, and runs the real
installed binary with a hidden window. Results on 2026-11-05 are recorded in
[`MACOS_ACCEPTANCE_EVIDENCE.md`](MACOS_ACCEPTANCE_EVIDENCE.md).
