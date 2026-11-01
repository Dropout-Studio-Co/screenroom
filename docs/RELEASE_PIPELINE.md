# Release and deployment pipeline

- **Date:** 2026-11-02
- **Owner of those files:** `.github/workflows/`, `scripts/release.sh`, `scripts/ship.sh`
- **Decision it implements:** GitHub Actions on a **self-hosted runner on the MacBook**,
  because a cloud runner cannot reach the home LAN, and the three target devices live on it.

Three targets, three lanes, one rule: **a device that is off is skipped, never faked**.

| Lane | Where it runs | What it does | Fails the pipeline? |
| --- | --- | --- | --- |
| `macos` | the MacBook (the runner) | build `Screenroom.app`, install it, publish the artefact | yes |
| `windows` | the Windows ultrawide, only when it hosts a runner | build the release binary there | yes, but it is off until enabled |
| `tv` | the MacBook, talking to the television over `sdb` | `pnpm build`, sign the widget, install and launch it | only when enabled |

## 1. Workflows

### `ci.yml`

Runs on every push to `main`, every pull request targeting `main`, and as a
reusable workflow (`workflow_call`). One job on `[self-hosted, macOS]`:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
python3 scripts/no_comments.py check
cd apps/tv && pnpm install --frozen-lockfile && pnpm typecheck && pnpm test
```

No secrets. It needs `cargo`/`rustup`, `node` and `pnpm` on the runner.

### `release.yml`

Runs on a push to `main` (after CI) and by hand (`workflow_dispatch`).

1. `ci` — calls `ci.yml` as a reusable workflow, so the release cannot start on a
   tree that has not passed. This means a push to `main` runs the checks twice:
   once from `ci.yml`, once inside the release. On a laptop that is the honest cost
   of a real `needs:` gate; `Swatinem/rust-cache@v2` keeps the second run warm.
2. `macos` — `scripts/release.sh macos --out artifacts`, then
   `scripts/release.sh install` into `~/Applications` **without launching it**, then
   uploads the `.zip` and its manifest fragment.
3. `windows` — `runs-on: [self-hosted, windows]`, gated on the repository variable
   `SCREENROOM_WINDOWS_RUNNER == 'true'`. Windows needs MSVC and cannot be built on
   macOS, so while that runner does not exist the job is **skipped, not greenwashed**:
   the run shows `windows` as skipped and the release is published macOS-only.
4. `tv` — gated on `SCREENROOM_DEPLOY_TV == 'true'`, runs `scripts/ship.sh --only tv`.
   A television that is off prints `skipped` and the job still succeeds.
5. `publish` — downloads both artefact sets, merges the fragments into
   `manifest.json`, signs it when the key secret exists, and creates or updates the
   GitHub Release `v<version>`.

### Signing

Nothing is signed with a paid certificate. The app stays **ad-hoc signed**, as
`scripts/package-macos.sh` has always done it.

The **manifest** can carry an Ed25519 detached signature:
`ssh-keygen -Y sign -n screenroom`, verified with `ssh-keygen -Y verify`. That is
the only cryptography in the pipeline, and it is optional — with
`SCREENROOM_ED25519_KEY` unset the manifest is published unsigned and the job
still succeeds, printing `manifest published unsigned`.

```bash
ssh-keygen -Y verify -f allowed_signers -I screenroom -n screenroom \
  -s manifest.json.sig < manifest.json
```

## 2. One-time setup on the MacBook

1. Install the runner as the owner, from GitHub **Settings → Actions → Runners →
   New self-hosted runner → macOS**. Use the arm64 tarball and unpack it outside
   this repository, for example `~/actions-runner`.
2. Register it with the labels the workflows ask for:

   ```bash
   ./config.sh --url https://github.com/Dropout-Studio-Co/screenroom --token <registration-token> --labels macOS --name macbook
   ```

   `self-hosted` and `macOS` are what `runs-on: [self-hosted, macOS]` matches.
   The runner is a separate clone under `~/actions-runner/_work`; it never touches
   the working copy in `~/Desktop/projects/screenroom`.
3. Make sure the tools it shells out to are where the workflow looks:
   `/opt/homebrew/bin` (`gh`, `node`, `pnpm`, `mpv`, `ffmpeg`) and
   `~/.cargo/bin` (`cargo`, `rustup`). The workflows append those to `PATH`
   themselves; `brew install gh` is the only one that is not already a project
   dependency.
4. Install it as a background service so a reboot does not leave the pipeline dead:

   ```bash
   ./svc.sh install "$USER"
   ./svc.sh start
   ```

5. Secrets — **Settings → Secrets and variables → Actions → Secrets**:
   - `SCREENROOM_ED25519_KEY` — optional. A passphrase-less Ed25519 private key in
     OpenSSH format, newline intact, used only to sign `manifest.json`. Generate it
     with `ssh-keygen -t ed25519 -N '' -C screenroom-release -f release-key`; keep
     the private key nowhere but that secret, and embed the public key in the
     application when the updater is built.
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
   - `SCREENROOM_DEPLOY_MACOS` = `false` to stop the release installing on this
     Mac. Unset means it installs.
7. Accept Gatekeeper once for the installed app: the first launch of an unsigned
   build shows "unidentified developer", so right-click → **Open** once. The
   pipeline never does that for you, and it never launches the app.

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
intentional for a personal build; it is not a permanent archive of every push.

## 5. What is verified, and what is not

**Verified here (2026-11-02):**

- Both workflows parse as YAML (`PyYAML` in a throwaway virtualenv).
- `bash -n scripts/ship.sh` and `bash -n scripts/release.sh`.
- The no-comments rule, run through the project's own lexers: `lex_shell` finds
  zero comments in both scripts and `lex_hash` finds zero in both workflows
  (`python3 scripts/no_comments.py check` reads `git ls-files`, so it covers them
  from the moment they are committed).
- `scripts/release.sh manifest` — merges macOS and Windows fragments, writes the
  document above, publishes unsigned with no key, and with a throwaway Ed25519 key
  produces a `manifest.json.sig` that `ssh-keygen -Y verify` accepts.
- `scripts/release.sh install` — the atomic bundle swap, including replacing an
  existing copy with nothing left behind.
- `scripts/ship.sh --only tv` / `--only windows` with no device — prints the table,
  `skipped`, and exits 0. Bad arguments exit 2.

**Not verified — and cannot be from here:**

- **No GitHub Actions run has happened.** The runner is not installed yet, so no
  job, step or expression in either workflow has been executed by GitHub.
- `scripts/release.sh macos` end to end could not be run on 2026-11-02: the working
  tree was being edited concurrently and `crates/screenroom-core` did not compile
  (`sources.rs` imports `crate::volume`, which `lib.rs` did not yet declare). It is a
  real build of `scripts/package-macos.sh` plus one `ditto` zip, unchanged by this work.
- The Windows job has never run: no Windows runner exists.
- The TV lane was exercised only up to `no TV answered on sdb`; nothing has been
  signed, pushed or installed by it.

**Two things that will look wrong on the first run, and are not:**

- The runner's checkout is a **fresh clone**, so the Git-ignored
  `apps/desktop/src/design/ds_pinned.rs` is absent and the release build uses the
  neutral fallback tokens from `apps/desktop/src/design/mod.rs`. The theme string in
  the running app says which one it got. `scripts/ship.sh` builds in the owner's own
  checkout and therefore carries the pinned design system. If releases must carry the
  private pin, the DS must be vendored at an immutable commit inside this repository;
  it is deliberately not.
- A push to `main` runs the checks twice (see §1). Nothing is being run twice by
  accident.

## 6. Device gates stay where they were

This pipeline ships bits; it does not certify hardware.
[`FEASIBILITY_GATES.md`](FEASIBILITY_GATES.md) still governs: a macOS `.zip` that
built is not a presented libmpv frame, a Windows `.exe` that built is not a decoded
frame on the Arc B580, and a `.wgt` that installed is not a television receipt. The
Windows and Tizen gates remain **BLOCKED** until they pass on the real devices, and
a skipped lane is never evidence of anything.
