---
title: Suppressing the app rebuild hook
author: Beyourahi
date: 2026-11-04
branch: main
id: l-ed63e38a
scope: repo
status: active
src: agent
touches:
  - scripts/git-hooks/post-commit
  - scripts/rebuild-app.sh
---

SCREENROOM_SKIP_REBUILD only works if it is exported into the `git commit` process. Passing it to an agent shell tool's own env/parameter field is not enough: in this harness that field did not reach the command at all (`echo "${SCREENROOM_SKIP_REBUILD:-unset}"` printed `unset`), so six commits made in a dependency-upgrade worktree each fired post-commit, rebuilt HEAD and atomically swapped the branch build into ~/Applications/Screenroom.app. Nothing was corrupted - the swap is atomic and the running app keeps its old code - but the installed app silently tracked an unmerged branch, which is exactly what the guard exists to prevent.

Verify the variable reaches the process before relying on it (`SCREENROOM_SKIP_REBUILD=1 sh -c 'echo $SCREENROOM_SKIP_REBUILD'` should print 1), and prefer the prefix form over an env field: `SCREENROOM_SKIP_REBUILD=1 git commit ...`. After landing, `scripts/rebuild-app.sh --status` shows whether the recorded install matches HEAD; a rebase changes commit IDs but not content, so a correct record can move forward without a rebuild.
