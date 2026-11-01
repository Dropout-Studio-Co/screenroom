#!/usr/bin/env bash

set -uo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

if [ "$(uname -s)" != "Darwin" ]; then
  echo "this check is about AppKit's terminate path; nothing to check on $(uname -s)"
  exit 0
fi

work="${TMPDIR:-/tmp}/screenroom-terminate-check"
rm -rf "$work"
mkdir -p "$work"

report="$work/terminate.txt"
library="$work/library.db"
binary="target/debug/screenroom-desktop"

echo "==> building"
cargo build -q -p screenroom-desktop || exit 1

SCREENROOM_LIBRARY="$library" SCREENROOM_WINDOW=hidden \
  SCREENROOM_TERMINATE_CHECK="$report" \
  "$binary" >"$work/app.log" 2>&1 &
app=$!

exited=0
for _ in $(seq 1 60); do
  if ! kill -0 "$app" 2>/dev/null; then
    exited=1
    break
  fi
  sleep 0.25
done

if [ "$exited" -eq 0 ]; then
  echo "FAIL: a terminate request did not close the window within 15s" >&2
  kill "$app" 2>/dev/null
  wait "$app" 2>/dev/null
  sed 's/^/    /' "$work/app.log" >&2 2>/dev/null || true
  exit 1
fi

wait "$app" 2>/dev/null
status=$?

if [ ! -s "$report" ]; then
  echo "FAIL: the application exited without running its close path (status $status)" >&2
  sed 's/^/    /' "$work/app.log" >&2 2>/dev/null || true
  exit 1
fi

receipt="$(tr -d '\r' <"$report" | head -1)"
echo "  receipt: $receipt"

case "$receipt" in
  "closed routed"*)
    echo "PASS: AppKit's terminate is routed through the window close, and the close path ran"
    ;;
  "closed unsupported"*)
    echo "FAIL: terminate routing was reported unsupported on macOS" >&2
    exit 1
    ;;
  "closed no-"*)
    echo "FAIL: the terminate method could not be replaced ($receipt)" >&2
    exit 1
    ;;
  *)
    echo "FAIL: unexpected receipt ($receipt)" >&2
    exit 1
    ;;
esac
