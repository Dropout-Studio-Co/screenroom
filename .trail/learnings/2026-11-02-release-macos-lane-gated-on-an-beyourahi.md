---
title: release macOS lane gated on an unset variable
author: Beyourahi
date: 2026-11-02
branch: fix/release-and-licensing
cost: { minutes: 45 }
id: l-31628e45
scope: repo
status: active
src: agent
touches:
  - ".github/workflows/release.yml"
  - docs/RELEASE_PIPELINE.md
---

## Decided

The `macos` job in `release.yml` carried `if: vars.SCREENROOM_MACOS_RUNNER ==
'true'` and no self-hosted runner was ever registered, so packaging and
publishing skipped on every push to `main`: the pipeline produced no release at
all, and the run looked green because a skipped job is not a failed one.

The gate was there because the device lanes genuinely need the home LAN. A macOS
`.app` and `.zip` do not. They now run on GitHub's hosted `macos-latest`, which
costs nothing on a public repository and needs only `brew install mpv ffmpeg` for
`libmpv`; `scripts/package-macos.sh` finds it at `/opt/homebrew/lib/libmpv.2.dylib`
exactly as it does on this Mac. Self-hosted stays only for the television lane and
the Windows box, which is what the LAN requirement was ever about.

## Watch out

Publishing had to become opt-in in the same change: a hosted runner plus a
`push: main` trigger would otherwise cut a GitHub Release on every merge. It now
needs a pushed `v*` tag or a dispatch with `publish` ticked, and a pushed tag is
checked against `scripts/release.sh version` first. Installing the app on the
owner's Mac was a step only a self-hosted runner could do, so it left the workflow
for `scripts/ship.sh --only macos`.
