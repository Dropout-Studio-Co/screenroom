# Workflows

The pipeline for Screenroom's three devices. The full setup, the secrets and the
honest limits are in [`docs/RELEASE_PIPELINE.md`](../../docs/RELEASE_PIPELINE.md);
this file is the short version.

- `ci-linux.yml` — the gate every push to `main` and every pull request runs:
  format, clippy, a gitleaks history scan and the Rust tests on a hosted Ubuntu
  runner, plus the TV client's typecheck and tests. It is also the reusable gate
  `release.yml` calls. **No secrets.**
- `ci.yml` — the owner's macOS gate: format, clippy, the full test suite, the
  no-comments check, and the TV client's typecheck and tests. Runs on
  `[self-hosted, macOS]` only when the repository variable
  `SCREENROOM_MACOS_RUNNER=true`, by hand, or as a reusable workflow. It does not
  trigger on a push, so an offline Mac leaves no queued check behind.
  **No secrets.**
- `release.yml` — on push to `main` and by hand: builds the macOS `.app`, optionally
  installs it on this Mac, builds Windows when that runner exists, packages the
  television when enabled, and publishes GitHub Release `v<version>` with
  `manifest.json` and an optional Ed25519 signature.
- `scripts/ship.sh` — the same three lanes from a terminal, with no GitHub involved.

## The one-time setup

1. Install a self-hosted runner from **Settings → Actions → Runners** on the MacBook,
   unpacked outside this repository (for example `~/actions-runner`).
2. Register it with the label the jobs ask for:
   `./config.sh --url https://github.com/Dropout-Studio-Co/screenroom --token <token> --labels macOS --name macbook`.
3. Keep `/opt/homebrew/bin` and `~/.cargo/bin` populated — the workflows add both to
   `PATH` themselves. `brew install gh` is needed for publishing.
4. Run it as a service: `./svc.sh install "$USER"` then `./svc.sh start`.
5. Set the optional secret `SCREENROOM_ED25519_KEY` (a passphrase-less Ed25519
   private key in OpenSSH format) to sign `manifest.json`. With it unset the
   manifest is published unsigned and the job still succeeds.
6. Set the repository variables only when the device exists:
   `SCREENROOM_MACOS_RUNNER=true` while the runner above is registered and
   running (leave it unset otherwise, so its jobs are skipped instead of queued),
   `SCREENROOM_WINDOWS_RUNNER=true` once a Windows runner is registered with the
   `windows` label, `SCREENROOM_DEPLOY_TV=true` to deploy to the television,
   `SCREENROOM_DEPLOY_MACOS=false` to stop the release installing on this Mac.

There is no Tizen secret. The signing profile (`screenroom`) is created once with
the owner's Samsung account and already lives on this Mac; it is never committed,
uploaded or copied anywhere, and no DUID or certificate material enters Git.

A device that is powered off is **skipped**, never faked. Windows needs a Windows
runner and MSVC, so until that runner exists its job is skipped and the release is
macOS-only — deliberately, and visibly in the run.

## Two gates, and why

`ci-linux.yml` is the portable gate: a hosted `ubuntu-latest` runner that needs no
machine of the owner's to be switched on, running formatting, clippy, a secret
scan, the Rust tests and the TV's tests. It is also the reusable workflow
`release.yml` calls, so the release gate never waits for the Mac to wake. `ci.yml`
is the owner's self-hosted macOS gate, reached by hand or through `workflow_call`,
and its job is skipped unless the repository variable
`SCREENROOM_MACOS_RUNNER=true`. They are separate files on purpose, so neither can
break the other, and a pull request gets a result even when the owner's machine is
asleep.

## Pinned actions

The workflows pin exact commits rather than tags, and this repository forbids
comments, so the mapping lives here. Dependabot (`.github/dependabot.yml`) is what
moves them.

| Action | Version | Commit |
| --- | --- | --- |
| `actions/checkout` | v7.0.1 | `3d3c42e5aac5ba805825da76410c181273ba90b1` |
| `actions/cache` | v6.1.0 | `55cc8345863c7cc4c66a329aec7e433d2d1c52a9` |
| `actions/setup-node` | v7.1.0 | `949feb2413d6458794dcd2491c4babbbce0c15c1` |
| `actions/upload-artifact` | v4.6.2 | `ea165f8d65b6e75b540449e92b4886f43607fa02` |
| `actions/download-artifact` | v4.3.0 | `d3f86a106a0bac45b974a628896c90dbdf5c8093` |
| `dtolnay/rust-toolchain` | `stable` branch head | `b725eda525362e7cf34af9a7243cac1e3f01a7ec` |
| `Swatinem/rust-cache` | v2 | `d1527c8f67ef9af357290475d4af4b0fab8f12e0` |

`ci-linux.yml`'s two cache steps share the `-cargo-` restore prefix, so the test
job does restore the `target/` the lint job wrote. A clippy build is not a test
binary set, but the cargo registry and the compiled dependency graph inside it are
the hits that restore is for, which is why the prefix stays broad.

The pinned action SHAs move only through Dependabot (`.github/dependabot.yml`),
because the workflows pin exact commits rather than tags. `apps/tv` is the only
pnpm manifest in the repository, which is why Dependabot watches that one
directory.
