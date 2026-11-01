# Workflows

The pipeline for Screenroom's three devices. The full setup, the secrets and the
honest limits are in [`docs/RELEASE_PIPELINE.md`](../../docs/RELEASE_PIPELINE.md);
this file is the short version.

- `ci.yml` — format, clippy, tests, the no-comments check, and the TV client's
  typecheck and tests. Runs on `[self-hosted, macOS]`, on push to `main`, on pull
  requests to `main`, and as the reusable gate the release calls. **No secrets.**
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
   `SCREENROOM_WINDOWS_RUNNER=true` once a Windows runner is registered with the
   `windows` label, `SCREENROOM_DEPLOY_TV=true` to deploy to the television,
   `SCREENROOM_DEPLOY_MACOS=false` to stop the release installing on this Mac.

There is no Tizen secret. The signing profile (`screenroom`) is created once with
the owner's Samsung account and already lives on this Mac; it is never committed,
uploaded or copied anywhere, and no DUID or certificate material enters Git.

A device that is powered off is **skipped**, never faked. Windows needs a Windows
runner and MSVC, so until that runner exists its job is skipped and the release is
macOS-only — deliberately, and visibly in the run.
