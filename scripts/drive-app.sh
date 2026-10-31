#!/usr/bin/env bash

set -uo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

source "$root/scripts/foreground-guard.sh"
screenroom_require_foreground "driving the Screenroom window"

process="${SCREENROOM_PROCESS:-screenroom-desktop}"
action="${1:?usage: drive-app.sh <key|capture> [name]}"

raise_block() {
  local script
  script=$(cat <<APPLESCRIPT
tell application "System Events"
  tell process "$process"
    set frontmost to true
    try
      perform action "AXRaise" of window 1
    end try
    delay 0.45
$1
  end tell
end tell
APPLESCRIPT
)
  local attempt=1
  while [ "$attempt" -le 4 ]; do
    if osascript <<<"$script" 2>/dev/null; then
      return 0
    fi
    sleep 0.5
    attempt=$((attempt + 1))
  done
  echo "failed to drive $process after 4 attempts" >&2
  return 1
}

case "$action" in
  enter) raise_block 'key code 36' ;;
  space) raise_block 'key code 49' ;;
  tab) raise_block 'key code 48' ;;
  backspace) raise_block 'key code 51' ;;
  escape) raise_block 'key code 53' ;;
  left) raise_block 'key code 123' ;;
  right) raise_block 'key code 124' ;;
  up) raise_block 'key code 126' ;;
  down) raise_block 'key code 125' ;;
  home) raise_block 'keystroke "1" using command down' ;;
  library) raise_block 'keystroke "2" using command down' ;;
  settings) raise_block 'keystroke "3" using command down' ;;
  refresh) raise_block 'keystroke "r" using command down' ;;
  close) raise_block 'keystroke "w" using command down' ;;
  quit) raise_block 'keystroke "q" using command down' ;;
  type:*)
    raise_block "keystroke \"${action#type:}\"" ;;
  capture)
    name="${2:?usage: drive-app.sh capture NAME}"
    raise_block 'delay 0.2'
    ./scripts/capture-window.sh ".artifacts/evidence/$name"
    sips -Z 1000 ".artifacts/evidence/$name" \
      --out ".artifacts/evidence/${name%.png}-small.png" >/dev/null
    echo "captured $name"
    ;;
  *)
    echo "unknown action: $action" >&2
    exit 2 ;;
esac
