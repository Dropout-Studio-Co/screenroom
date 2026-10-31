#!/usr/bin/env bash

set -uo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

if [ "$(uname -s)" != "Darwin" ]; then
  echo "this check is about the macOS menu bar; nothing to check on $(uname -s)"
  exit 0
fi

work="${TMPDIR:-/tmp}/screenroom-routing-check"
rm -rf "$work"
mkdir -p "$work"

report="$work/routing.txt"
library="$work/library.db"
binary="target/debug/screenroom-desktop"

echo "==> building"
cargo build -q -p screenroom-desktop || exit 1

SCREENROOM_LIBRARY="$library" SCREENROOM_QUIT_CHECK="$report" \
  "$binary" >"$work/app.log" 2>&1 &
app=$!

for _ in $(seq 1 40); do
  [ -s "$report" ] && break
  sleep 0.25
done

if [ ! -s "$report" ]; then
  echo "FAIL: the application did not report within 10s" >&2
  sed 's/^/    /' "$work/app.log" >&2 2>/dev/null || true
  kill "$app" 2>/dev/null
  exit 1
fi

outcome="$(tr -d '[:space:]' <"$report")"
kill "$app" 2>/dev/null
wait "$app" 2>/dev/null

echo "  reported: $outcome"
case "$outcome" in
  routed)
    echo "PASS: the menu bar's Quit performs the window's close action"
    ;;
  unsupported)
    echo "FAIL: the routing was reported unsupported on macOS" >&2
    exit 1
    ;;
  *)
    echo "FAIL: the Quit item was not routed ($outcome)" >&2
    echo "      See the note above 'route_quit' in apps/desktop/src/app.rs." >&2
    exit 1
    ;;
esac
