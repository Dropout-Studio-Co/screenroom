---
title: "Release tag runs failed on a source download reset, not on test time"
author: Beyourahi
date: 2026-11-05
branch: main
cost: { minutes: 6, tokens: 121381 }
id: l-5d36e068
scope: repo
status: active
src: agent
touches:
  - ".github/workflows/README.md"
  - ".github/workflows/release.yml"
  - docs/RELEASE_PIPELINE.md
  - scripts/build-lgpl-media.sh
  - scripts/fetch-sparkle.sh
  - scripts/publish-update.sh
---

## Decided
- No CI runs on the owner's Mac (owner, 2026-11-05). The self-hosted runner `screenroom-macbook` was unregistered and `SCREENROOM_MACOS_RUNNER` deleted, so `ci.yml` stays idle.
- `.github/workflows/release.yml` is `workflow_dispatch` only. `ci-linux.yml` is the only automatic check. Releases come from `scripts/publish-update.sh`.

## Tried and ruled out
- "The hosted runner is too slow for the Rust suite": false once the cache is warm. Tests finish in about 4 min. The v0.1.5/v0.1.6 tag runs failed in `scripts/build-lgpl-media.sh` with `curl: (35) Connection reset by peer`.

## Watch out
- `curl --retry` does not retry a connection reset. You need `--retry-all-errors`, now set in `build-lgpl-media.sh` and `fetch-sparkle.sh`.
- `actions/cache` saves `vendor/lgpl` only when the job succeeds, so a failed run leaves no cache and the next run rebuilds FFmpeg/mpv from source.
- `release-${{ github.ref }}` concurrency has `cancel-in-progress: false`, so a manual run queues behind any older run on the same ref.
