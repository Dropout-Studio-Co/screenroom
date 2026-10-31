#!/usr/bin/env bash

set -uo pipefail

screenroom_require_foreground() {
  local purpose="${1:-this check}"
  if [ "${SCREENROOM_ALLOW_FOREGROUND:-0}" = "1" ]; then
    return 0
  fi
  cat >&2 <<NOTE
Refusing to take over the desktop.

  $purpose.

  That needs a visible Screenroom window on the active desktop: it activates the
  application, may switch Spaces, and sends synthetic keyboard or pointer events.
  While it runs, this Mac is not yours.

  To allow it once, run the same command with SCREENROOM_ALLOW_FOREGROUND=1.

  Most checks do not need this. In order of what they cost the machine:

    cargo test --workspace                     # no window at all
    cargo run -p screenroom-player --example capability
    scripts/check-quit-routing.sh              # hidden window, no focus taken
    scripts/verify-quit.sh                     # needs SCREENROOM_ALLOW_FOREGROUND=1

  See docs/TEST_PLAN.md, "Testing without taking over the desktop".
NOTE
  exit 3
}
