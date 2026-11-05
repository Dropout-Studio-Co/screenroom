---
title: "Screenroom has no CI: everything builds, checks and publishes on the owner's Mac"
author: Beyourahi
date: 2026-11-05
branch: main
cost: { minutes: 3, tokens: 10361 }
id: l-bc95255d
scope: repo
status: active
replaces: l-5d36e068
src: agent
touches:
  - AGENTS.md
  - scripts/publish-update.sh
---

## Decided
- On 2026-11-05 the owner removed all GitHub Actions workflows and Dependabot. GitHub only hosts the source, the releases and the Sparkle feed.
- Run the checks from `AGENTS.md` locally. `scripts/publish-update.sh` signs with the Keychain key (account `screenroom`) and uploads the release.
- The GitHub secret `SCREENROOM_SPARKLE_ED_KEY`, the variable `SCREENROOM_MACOS_RUNNER` and the self-hosted runner were deleted.

## Watch out
- Don't add a workflow, runner or Dependabot back without the owner asking. Older learnings about `release.yml` and `ci-linux.yml` are history.
