# The installed macOS app rebuilds itself

**Scope: this MacBook only.** Windows packaging and the Tizen `.wgt` are untouched
by this automation; they keep their own, separately invoked commands.

## What was configured

| Piece | Path | Role |
| --- | --- | --- |
| Hook set (tracked) | `scripts/git-hooks/` | `pre-commit` keeps the no-comments gate; `post-commit`, `post-merge` and `post-rewrite` start a rebuild |
| Worker | `scripts/rebuild-app.sh` | Build, verify, install, record; one at a time, resumable |
| Installer | `scripts/install-app-hooks.sh` | Points `core.hooksPath` at `scripts/git-hooks` and makes the hooks executable |
| Build | `scripts/package-macos.sh` | The same release build and bundle it always produced |
| Verify | `scripts/verify-bundle.sh` | Structure, signature, bundled libmpv and its dependencies |
| Receipts | `.artifacts/builds/` | One log per build plus `last.json`; Git-ignored |

`git config core.hooksPath` lives in the repository's own config, so the behaviour
is identical from any terminal, editor, AI agent or worktree on this machine, and
nothing in it depends on which harness ran Git.

## How a commit becomes an installed app

1. Git finishes the commit and runs `post-commit` (or `post-merge` for a pull, or
   `post-rewrite` after an amend or rebase).
2. The hook starts `scripts/rebuild-app.sh --background` detached and exits, so the
   commit returns immediately and a build can never fail or block a Git operation.
3. The worker takes the lock beside the Git directory. If a build is already
   running it leaves one `pending` marker and exits; the running worker picks that
   up and builds `HEAD` again if it moved.
4. It compares `HEAD` with the installed commit. Equal → nothing to do. A commit
   touching no app path (`apps/`, `crates/`, `assets/`, `scripts/`, `vendor/`,
   `Cargo.*`, `.cargo/`) is recorded without a rebuild.
5. Otherwise it runs the release build, verifies the bundle, then swaps
   `~/Applications/Screenroom.app` for the new one through a staged copy, keeping
   the previous bundle until the swap has succeeded and restoring it if the swap
   fails.
6. The installed commit, the result and a link to the log are written to
   `.artifacts/builds/last.json`, and the installed SHA to
   `screenroom-installed.sha` beside the Git directory.

## Properties

- **A failed build never touches a working app.** The install step runs only after
  the build and verification succeeded; a failure is recorded, the old bundle
  stays, and the next commit retries because the installed SHA was not advanced.
- **No duplicate work.** One build at a time, with a single queued follow-up, and
  the installed SHA as the arbiter.
- **No lost updates.** The worker always builds the `HEAD` it finds, and loops once
  more if `HEAD` moved while it was building.
- **Interruptions are safe.** A killed build leaves the lock behind; the next run
  sees a dead PID and reclaims it. Nothing half-copied is ever moved into place.
- **Cheap.** An incremental rebuild of an unchanged tree takes seconds; a docs-only
  commit costs no build at all.
- **The running app is not disturbed.** It keeps the code it started with until it
  is relaunched.

## Receipts (2026-11-03)

| Check | Observed |
| --- | --- |
| Hook fires on commit | Committing `fe3739d` started a detached build of `fe3739d` with no command typed |
| Install | `~/Applications/Screenroom.app` binary SHA-256 equals `dist/Screenroom.app` (`30192305…`) |
| Verification | `scripts/verify-bundle.sh ~/Applications/Screenroom.app` — all checks passed |
| Launch | The installed bundle started with `SCREENROOM_WINDOW=hidden`, stayed up, and quit |
| Overlap | A second trigger during a build printed *"a build is already running; queued this state behind it"* and the first finished |
| Failure safety | With `cargo` removed from `PATH` the build failed, exited 1, recorded `failed`, and the installed binary's hash was unchanged |
| Docs-only commit | Recorded the new SHA with `result: skipped`, no build |
| Status | `scripts/rebuild-app.sh --status` reports installed SHA, `HEAD` and the last result |

## Commands

```bash
scripts/install-app-hooks.sh          # once per clone; idempotent
scripts/rebuild-app.sh --status       # what is installed, what HEAD is, last result
scripts/rebuild-app.sh --force        # build and install right now
SCREENROOM_SKIP_REBUILD=1 git commit  # skip one commit deliberately
SCREENROOM_INSTALL_DIR=…              # install somewhere other than ~/Applications
```

The build logs are in `.artifacts/builds/`, which is Git-ignored and local to this
checkout.
