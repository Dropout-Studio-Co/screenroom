# Release and deployment pipeline

- **Date:** 2026-11-02, revised the same day when packaging moved to a hosted runner.
- **Owner of those files:** `.github/workflows/`, `scripts/release.sh`, `scripts/ship.sh`
- **Decision it implements:** **packaging runs on GitHub-hosted runners; only the
  device lanes stay self-hosted.** A cloud runner cannot reach the home LAN, so the
  television lane still needs the MacBook. A `.app` and a `.zip` need nothing on the
  LAN, and on a public repository `macos-latest` costs no minutes, so packaging no
  longer waits for a self-hosted runner. The earlier version gated the `macos` job on
  `vars.SCREENROOM_MACOS_RUNNER`, which nothing had ever set, so the job skipped on
  every push and no release was automated at all.

Three targets, three lanes, one rule: **a device that is off is skipped, never faked**.

| Lane | Where it runs | What it does | Fails the pipeline? |
| --- | --- | --- | --- |
| `macos` | GitHub-hosted `macos-latest` (arm64) | build `Screenroom.app` and archive it | yes |
| `windows` | the Windows ultrawide, only when it hosts a runner | build the release binary there | yes, but it is off until enabled |
| `tv` | the MacBook, talking to the television over `sdb` | `pnpm build`, sign the widget, install and launch it | only when enabled |

## 1. Workflows

### `ci-linux.yml`

The gate every push to `main` and every pull request runs, on a hosted
`ubuntu-latest` runner that needs nothing of the owner's switched on:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
gitleaks git                       # history scan, 8.30.1, SHA256-verified
cargo test --workspace --all-targets
cd apps/tv && pnpm install --frozen-lockfile && pnpm typecheck && pnpm test
```

Six tests are skipped by name because a headless hosted runner cannot measure what
they measure. No secrets. `release.yml` calls this file as its reusable gate.

### `ci.yml`

The owner's macOS gate. It runs on `[self-hosted, macOS]` and only when the
repository variable `SCREENROOM_MACOS_RUNNER` is `'true'`: without it the job is
**skipped, not queued**, so a Mac that is switched off leaves no permanently
pending check behind. It no longer triggers on a push or a pull request — that is
`ci-linux.yml`'s job — and stays reachable through `workflow_call` and
`workflow_dispatch`. One job:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
python3 scripts/no_comments.py check
cd apps/tv && pnpm install --frozen-lockfile && pnpm typecheck && pnpm test
```

No secrets. It needs `cargo`/`rustup`, `node` and `pnpm` on the runner.

### `release.yml`

Runs on a push to `main`, on a pushed tag matching `v*`, and by hand
(`workflow_dispatch`, with a `publish` checkbox that is off unless ticked).

1. `ci` — calls `ci-linux.yml` as a reusable workflow, so the release cannot start
   on a tree that has not passed, and it does not depend on any self-hosted machine.
   A push to `main` therefore runs the hosted checks twice: once from
   `ci-linux.yml`'s own push trigger, once inside the release. That repetition is
   the honest cost of a real `needs:` gate.
2. `macos` — `runs-on: macos-latest` on an arm64 image, with **no variable gate**.
   It installs Homebrew `mpv` (which supplies `libmpv.2.dylib`) and `ffmpeg`, runs
   `scripts/release.sh macos --out artifacts`, and uploads the `.zip` and its
   manifest fragment. The bundle is **ad-hoc signed**, exactly as
   `scripts/package-macos.sh` has always done it. It does not install the app
   anywhere: a hosted runner is discarded when the job ends, and installing on the
   owner's Mac is `scripts/ship.sh --only macos` (or `scripts/release.sh install`).
3. `windows` — `runs-on: [self-hosted, windows]`, gated on the repository variable
   `SCREENROOM_WINDOWS_RUNNER == 'true'`. Windows needs MSVC and cannot be built on
   macOS, so while that runner does not exist the job is **skipped, not greenwashed**:
   the run shows `windows` as skipped and the release is published macOS-only.
4. `tv` — gated on `SCREENROOM_DEPLOY_TV == 'true'`, runs `scripts/ship.sh --only tv`
   on the MacBook's runner. A television that is off prints `skipped` and the job
   still succeeds.
5. `publish` — `runs-on: ubuntu-latest`, and only when the trigger asked for it: a
   pushed tag `v*`, or a manual run started **from `main`** with the `publish`
   checkbox ticked. A normal push to `main` builds and keeps the artefacts for 14
   days and **publishes nothing**. When it does run, it checks a pushed tag against
   `scripts/release.sh version`, downloads both artefact sets, merges the fragments
   into `manifest.json`, signs it when the key secret exists, and creates or updates
   the GitHub Release `v<version>`.

#### What a normal push to `main` does, exactly

- runs `ci-linux.yml` (format, clippy, gitleaks, Rust tests, TV typecheck and tests),
  then repeats it inside `release.yml` as `ci`;
- builds and archives `dist/Screenroom.app` on `macos-latest` and uploads the `.zip`
  plus `manifest-macos-arm64.json` as a workflow artefact, kept 14 days;
- **does not** create or update any GitHub Release, tag or asset, and **does not**
  publish `manifest.json`, because `publish` is skipped;
- skips `windows` and `tv` unless their variables are set.

A release happens only when someone pushes a `v*` tag at the commit whose
`Cargo.toml` version matches it, or dispatches the workflow on `main` with
`publish` ticked.

### Signing

Nothing is signed with a paid certificate. The app stays **ad-hoc signed**, as
`scripts/package-macos.sh` has always done it, and that is all the hosted runner
can do: notarisation needs an Apple Developer ID certificate plus an
`notarytool` credential, neither of which exists here. **A published macOS `.zip`
is therefore not notarised**, and the first launch on a machine that did not build
it shows Gatekeeper's "unidentified developer" prompt. Do not describe a release
as signed or notarised until those credentials exist and the packaging script uses
them.

The **manifest** can carry an Ed25519 detached signature:
`ssh-keygen -Y sign -n screenroom`, verified with `ssh-keygen -Y verify`. That is
the only cryptography in the pipeline, and it is optional — with
`SCREENROOM_ED25519_KEY` unset the manifest is published unsigned and the job
still succeeds, printing `manifest published unsigned`.

```bash
ssh-keygen -Y verify -f allowed_signers -I screenroom -n screenroom \
  -s manifest.json.sig < manifest.json
```

### What must be settled before the first real publish

The macOS job bundles whatever `libmpv` it built against. On a GitHub runner that
is Homebrew's `mpv`, which is **GPL-2.0-or-later** with `ffmpeg` compiled
`--enable-gpl`, and [`LICENSING.md`](LICENSING.md) classifies exactly that build as
*development only — do not bundle or ship it*. The workflow is therefore able to
produce an artefact, and able to publish one on an explicit tag or a ticked
dispatch, but **a publish that uploads this bundle redistributes a GPL-linked
binary**, which needs the LGPL-only libmpv decision or a GPL release posture first.
That decision is the release gate's, not this pipeline's. Publishing is opt-in for
that reason as well as for tag hygiene.

## 2. One-time setup

**Packaging needs none of this.** The `macos` job runs on GitHub's own arm64 macOS
image, installs `mpv` and `ffmpeg` from Homebrew there, and ad-hoc signs the bundle.
There is no signing certificate, no notarisation and no secret in that lane, and
therefore nothing to configure for a release build to happen.

A self-hosted runner on the MacBook is needed only for the two lanes that touch the
owner's hardware: `tv` (`scripts/ship.sh --only tv`, which needs `sdb`, the Tizen CLI
and the Samsung signing profile that lives on this Mac), and `ci.yml`, the optional
full-suite gate. Skip steps 1–4 if only packaging matters.

1. Install the runner as the owner, from GitHub **Settings → Actions → Runners →
   New self-hosted runner → macOS**. Use the arm64 tarball and unpack it outside
   this repository, for example `~/actions-runner`.
2. Register it with the labels the workflows ask for:

   ```bash
   ./config.sh --url https://github.com/Dropout-Studio-Co/screenroom --token <registration-token> --labels macOS --name macbook
   ```

   `self-hosted` and `macOS` are what `runs-on: [self-hosted, macOS]` matches for the
   `tv` job. The runner is a separate clone under `~/actions-runner/_work`; it never
   touches the working copy in `~/Desktop/projects/screenroom`.
3. Make sure the tools the `tv` lane shells out to are where the workflow looks:
   `/opt/homebrew/bin` (`sdb`, `node`, `pnpm`, `mpv`, `ffmpeg`) and
   `~/.cargo/bin` (`cargo`, `rustup`). The workflow appends those to `PATH` itself.
4. Install it as a background service so a reboot does not leave the device lanes dead:

   ```bash
   ./svc.sh install "$USER"
   ./svc.sh start
   ```

5. Secrets — **Settings → Secrets and variables → Actions → Secrets**:
   - `SCREENROOM_ED25519_KEY` — optional. A passphrase-less Ed25519 private key in
     OpenSSH format, newline intact, used only to sign `manifest.json`. Generate it
     with `ssh-keygen -t ed25519 -N '' -C screenroom-release -f release-key`; keep
     the private key nowhere but that secret, and embed the public key in the
     application when the updater is built. With it unset, publishing still works and
     the manifest is served unsigned.
   - Nothing else. The Tizen signing profile is **not** a secret: it already exists
     and is authorised on this Mac (`~/.local/share/screenroom-toolchains/tizen-data/profile/profiles.xml`,
     profile `screenroom`), created once with the owner's Samsung account through
     Certificate Manager. It is never committed, never uploaded, and never leaves
     this machine. A CI job cannot answer a password prompt, so a profile that has
     not been authorised once locally cannot sign in a job — authorise it by running
     `scripts/ship.sh --only tv` by hand once after any key or DUID change.
6. Variables — **Settings → Secrets and variables → Actions → Variables**:
   - `SCREENROOM_WINDOWS_RUNNER` = `true` **only** once a Windows runner is
     registered with the label `windows`. Until then leave it unset; the job is
     skipped and the log says so.
   - `SCREENROOM_DEPLOY_TV` = `true` to let a release install on the television.
     Leave it unset if the set is usually off.
   - `SCREENROOM_MACOS_RUNNER` = `true` **only** while a runner with the label
     `macOS` is registered and running. It now selects `ci.yml` alone: the release's
     `macos` packaging job ignores it and runs on GitHub's runner either way.
     `SCREENROOM_DEPLOY_MACOS` is gone with the install step it gated.
7. Accept Gatekeeper once for the app: the first launch of an ad-hoc signed build
   shows "unidentified developer", so right-click → **Open** once. Neither the
   pipeline nor `scripts/ship.sh` can do that for you, and neither launches the app
   on its own.

## 3. `scripts/ship.sh` — the owner's one command

```bash
scripts/ship.sh                       # all three lanes, concurrently
scripts/ship.sh --only macos          # just this machine
scripts/ship.sh --only tv --no-relaunch
```

It needs no GitHub, no network and no runner. Each lane runs at once in its own
log; the table at the end is the result:

```
LANE      RESULT   NOTE
------    ------   ----
macos     ok       installed /Users/…/Applications/Screenroom.app and relaunched it
windows   ok       rebuilt the release binary on owner@192.168.0.40
tv        ok       installed ScreenroomTV-0.1.0.wgt on 192.168.0.172:26101 and launched it
```

- **It exits non-zero only when the macOS lane failed.** A powered-off television
  or an unreachable Windows box prints `skipped` and the command still succeeds,
  because those devices are not always on. `SCREENROOM_SHIP_STRICT=1` makes any lane
  that *ran and failed* fatal as well; `release.yml` sets it for the TV job so a
  broken deployment is not green there.
- It never disables networking, never reboots anything and never writes to media.
  The only files it touches are `dist/`, `apps/tv/build/`, the install directory
  and the Tizen package on the TV.

Configuration is read from the environment or from
`~/.config/screenroom/devices.env` (override the path with
`SCREENROOM_DEVICES_FILE`). That file is untracked and outside the repository:

```bash
SCREENROOM_WINDOWS_HOST=owner@192.168.0.40
SCREENROOM_WINDOWS_REPO=C:/Users/owner/screenroom
SCREENROOM_TV_ADDRESS=192.168.0.172:26101
SCREENROOM_MACOS_INSTALL_DIR=/Users/owner/Applications
```

The Windows lane needs OpenSSH Server on that machine and key-based login
(`BatchMode=yes`, a five-second timeout). Unreachable is `skipped`, not an error.
The TV lane uses `sdb` and the Tizen CLI from
`~/.local/share/screenroom-toolchains/tizen`; it pushes the `.wgt` and installs it
with `vd_appinstall`, because on this firmware `sdb install` only copies the file.

## 4. `manifest.json`

The in-app updater reads this. Shape:

```json
{
  "product": "screenroom",
  "version": "0.1.0",
  "tag": "v0.1.0",
  "generated_at": "2026-11-02T10:00:00Z",
  "artifacts": [
    {
      "platform": "macos",
      "arch": "arm64",
      "version": "0.1.0",
      "filename": "Screenroom-0.1.0-macos-arm64.zip",
      "sha256": "…",
      "size": 41234567
    }
  ]
}
```

`platform` is `macos` or `windows`, `arch` is `arm64` or `x64`, `size` is bytes, and
`sha256` is the lowercase hex digest of the file named beside it. The signature, when
present, is `manifest.json.sig` — a separate file, never a field inside the manifest,
because a field would change the bytes that were signed.

`scripts/release.sh` builds the fragments:
`macos --out DIR` writes `DIR/manifest-macos-<arch>.json`, the Windows job writes
`manifest-windows-x64.json`, and `manifest --out DIR` merges them and signs the
result. The version comes from `apps/desktop/Cargo.toml`, which inherits
`[workspace.package] version` from the root `Cargo.toml`.

Because the version only changes when someone changes it, repeated releases of
`0.1.0` reuse the `v0.1.0` tag and **overwrite** its assets (`--clobber`). That is
intentional for a personal build; it is not a permanent archive of every push. A
pushed tag is checked against `scripts/release.sh version` first, so a `v0.2.0` tag
on a tree that still says `0.1.0` fails the publish job instead of quietly
publishing the wrong tag. The artefacts are `arm64` only, because `macos-latest` is
an Apple-silicon image; an Intel bundle would need a second runner and a second
fragment.

## 5. What is verified, and what is not

**Verified here (2026-11-02):**

- All three workflow files parse as YAML (`PyYAML` in a throwaway virtualenv) and
  `actionlint` exits 0 on them, with no expression, context or `shellcheck` finding.
- `bash -n scripts/ship.sh` and `bash -n scripts/release.sh`.
- The no-comments rule, run through the project's own lexers: `lex_shell` finds
  zero comments in both scripts and `lex_hash` finds zero in both workflows
  (`python3 scripts/no_comments.py check` reads `git ls-files`, so it covers them
  from the moment they are committed).
- `scripts/release.sh macos --out …` end to end on this Mac on 2026-11-02, which is
  the exact command the hosted job runs: built `dist/Screenroom.app` (24 MB), ad-hoc
  signed and verified it, archived `Screenroom-0.1.0-macos-arm64.zip` (10,514,449
  bytes, sha256 `3491b1a2…61b1`) and wrote `manifest-macos-arm64.json`. Only the
  Homebrew prefix is the same here and on the runner, so the `brew install mpv
  ffmpeg` step is the one part of that lane no local run can cover.
- `scripts/release.sh manifest` over that real fragment — merges it into
  `manifest.json` with `tag: v0.1.0` and the digest above, publishing unsigned with
  no key. Earlier in the same day the same command with a throwaway Ed25519 key
  produced a `manifest.json.sig` that `ssh-keygen -Y verify` accepted.
- `scripts/release.sh install` — the atomic bundle swap, including replacing an
  existing copy with nothing left behind.
- `scripts/ship.sh --only tv` / `--only windows` with no device — prints the table,
  `skipped`, and exits 0. Bad arguments exit 2.

**Not verified — and cannot be from here:**

- **No GitHub Actions run has happened.** No job, step or expression in any
  workflow has been executed by GitHub, including the new hosted `macos` lane:
  `brew install mpv ffmpeg` on the runner image, the `publish` gate and the tag
  guard are written and linted, not run.
- The publish gate cannot be exercised here without creating a tag and a GitHub
  Release, which this change deliberately does not do. What is verified about it is
  the expression's shape under `actionlint` and the fact that `publish` is skipped
  by a `push` to `main`, where `github.ref` is a branch and `inputs.publish` is
  empty.
- The Windows job has never run: no Windows runner exists.
- The TV lane was exercised only up to `no TV answered on sdb`; nothing has been
  signed, pushed or installed by it.

**Things that will look wrong on the first run, and are not:**

- A runner's checkout is a **fresh clone**, so the Git-ignored
  `apps/desktop/src/design/ds_pinned.rs` is absent and the release build uses the
  neutral fallback tokens from `apps/desktop/src/design/mod.rs`. The theme string in
  the running app says which one it got. `scripts/ship.sh` builds in the owner's own
  checkout and therefore carries the pinned design system. If releases must carry the
  private pin, the DS must be vendored at an immutable commit inside this repository;
  it is deliberately not.
- A push to `main` runs the checks twice (see §1). Nothing is being run twice by
  accident.
- The hosted job installs `mpv` and `ffmpeg` from Homebrew, which is a **different
  libmpv build** from the one this Mac has. The `macos` artefact therefore bundles
  whatever that formula shipped, and `docs/LICENSING.md` still governs whether it
  may be redistributed at all.

## 6. Device gates stay where they were

This pipeline ships bits; it does not certify hardware.
[`FEASIBILITY_GATES.md`](FEASIBILITY_GATES.md) still governs: a macOS `.zip` that
built is not a presented libmpv frame, a Windows `.exe` that built is not a decoded
frame on the Arc B580, and a `.wgt` that installed is not a television receipt. The
Windows and Tizen gates remain **BLOCKED** until they pass on the real devices, and
a skipped lane is never evidence of anything.
