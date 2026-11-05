---
title: "Hosted macOS runners run scripts under bash 3.2, where an empty array fails set -u"
author: Beyourahi
date: 2026-11-05
branch: main
cost: { minutes: 2, tokens: 12137 }
id: l-cc6d56dd
scope: repo
status: active
src: agent
touches:
  - scripts/check-updater.sh
  - scripts/package-macos.sh
  - scripts/ship.sh
---

## Decided
- Expand any array that can be empty as `${name[@]+"${name[@]}"}` (fixed in `scripts/package-macos.sh`, `scripts/check-updater.sh`, `scripts/ship.sh`).

## Watch out
- The owner's Mac resolves `#!/usr/bin/env bash` to Homebrew bash 5, which accepts `"${empty[@]}"` under `set -u`. GitHub's `macos-latest` run used bash 3.2 and failed with `sign_options[@]: unbound variable` during ad-hoc signing.
- To check a script the way CI runs it, run `/bin/bash scripts/<name>.sh`, not `bash` or `./`.
