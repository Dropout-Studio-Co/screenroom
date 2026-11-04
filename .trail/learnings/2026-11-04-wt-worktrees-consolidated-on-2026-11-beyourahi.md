---
title: "wt/* worktrees consolidated on 2026-11-05; HDR evidence kept in .artifacts/from-wt-hdr"
author: Beyourahi
date: 2026-11-04
branch: main
cost: { minutes: 4, tokens: 127668 }
id: l-24764795
scope: repo
status: active
src: agent
---

## Decided
The owner approved removing the seven `~/.cache/sr-wt/*` worktrees and their `wt/*` branches (a11y, gpu, hdr, motion, nopip, tv, visual). All seven were already fully merged into main at a6b32fc, with no unique commits and no untracked files. Their ignored `target/`, `apps/tv/node_modules|build|dist` and `ds_pinned.rs` (identical to main's) were discarded as regenerable. The 22 MB of HDR measurement output (`.artifacts/hdr-lib`, generated test clips) from the hdr worktree was copied into main's gitignored `.artifacts/from-wt-hdr/` before removal.

## Tried and ruled out
Deleting the HDR output outright. HDR was removed by ADR-005, but that output was the only copy of the old measurements, so it was kept locally.

## Watch out
`~/actions-runner/_work/screenroom/screenroom` is the self-hosted CI runner's checkout, not a stray clone. Consolidation must not delete it.
