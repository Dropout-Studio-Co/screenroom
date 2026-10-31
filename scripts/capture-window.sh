#!/usr/bin/env bash

set -euo pipefail

out="${1:?usage: capture-window.sh <output.png>}"
process="${SCREENROOM_PROCESS:-screenroom-desktop}"
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source_file="$root/scripts/capture-window.swift"
binary="$root/target/devtools/capture-window"

if [ ! -x "$binary" ] || [ "$source_file" -nt "$binary" ]; then
  mkdir -p "$(dirname "$binary")"
  xcrun swiftc -O -swift-version 5 -o "$binary" "$source_file" >/dev/null
fi

if [ "${SCREENROOM_ALLOW_FOREGROUND:-0}" = "1" ]; then
  osascript >/dev/null 2>&1 <<APPLESCRIPT || true
tell application "System Events"
  tell process "$process"
    set frontmost to true
    try
      perform action "AXRaise" of window 1
    end try
  end tell
end tell
APPLESCRIPT
fi

mkdir -p "$(dirname "$out")"
listing="$("$binary" "$process" --list 2>/dev/null)" || {
  echo "no window for \"$process\" — is Screenroom running?" >&2
  exit 1
}
window_number="$(printf '%s\n' "$listing" | head -1 | cut -f1)"
if [ -z "$window_number" ]; then
  echo "could not resolve a window number for \"$process\"" >&2
  exit 1
fi

screencapture -x -o -l"$window_number" "$out"
printf 'captured window %s of %s -> %s\n' "$window_number" "$process" "$out"
