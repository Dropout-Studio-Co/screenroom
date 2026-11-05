#!/usr/bin/env bash

set -uo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
cd "$root"

work="$(mktemp -d "${TMPDIR:-/tmp}/screenroom-live-update.XXXXXX")"
install_dir="$work/applications"
receipt="$work/receipt.jsonl"
library="$work/library.db"
expected="${SCREENROOM_LIVE_TARGET:-0.1.0}"
source_version="${SCREENROOM_LIVE_SOURCE:-0.0.9}"
keep="${SCREENROOM_KEEP_TEST_WORK:-0}"

cleanup() {
  rm -rf "$work"
}

if [ "$keep" = "1" ]; then
  cleanup() { printf 'kept %s\n' "$work"; }
fi

trap cleanup EXIT

feed="https://github.com/Dropout-Studio-Co/screenroom/releases/latest/download/appcast.xml"

if ! curl -fsSL --proto '=https' --max-time 30 -o "$work/feed.xml" "$feed"; then
  echo "no published appcast answers at $feed; publish a release first" >&2
  exit 2
fi

if ! grep -q "sparkle-signatures" "$work/feed.xml"; then
  echo "the published feed carries no signature" >&2
  exit 1
fi

advertised="$(sed -n 's:.*<sparkle:shortVersionString>\([^<]*\)</sparkle:shortVersionString>.*:\1:p' "$work/feed.xml" | head -1)"
if [ -z "$advertised" ]; then
  echo "the published feed carries no version" >&2
  exit 1
fi

if [ "$advertised" != "$expected" ]; then
  echo "the feed offers $advertised, not the expected $expected" >&2
  exit 1
fi

echo "==> the published feed is signed and offers $advertised"

echo "==> building $source_version against the real feed"
SCREENROOM_ALLOW_VERSION_OVERRIDE=1 \
  SCREENROOM_VERSION="$source_version" \
  scripts/package-macos.sh "$install_dir" >"$work/build.log" 2>&1 \
  || { tail -20 "$work/build.log" >&2; echo "the $source_version build failed" >&2; exit 1; }

installed="$(plutil -extract CFBundleShortVersionString raw "$install_dir/Screenroom.app/Contents/Info.plist")"
echo "  installed $installed into $install_dir"

env SCREENROOM_WINDOW=hidden \
  SCREENROOM_LIBRARY="$library" \
  SCREENROOM_UPDATE_CHECK="$receipt" \
  SCREENROOM_UPDATE_INSTALL=quit \
  SCREENROOM_UPDATE_TIMEOUT=240 \
  SCREENROOM_UPDATE_DEBUG=1 \
  "$install_dir/Screenroom.app/Contents/MacOS/Screenroom" >"$work/run.log" 2>&1 &
pid=$!
waited=0
while kill -0 "$pid" 2>/dev/null && [ "$waited" -lt 300 ]; do
  sleep 1
  waited=$((waited + 1))
done
if kill -0 "$pid" 2>/dev/null; then
  kill "$pid" 2>/dev/null || true
  wait "$pid" 2>/dev/null || true
  echo "the update run did not finish within 300s" >&2
  exit 1
fi
wait "$pid" 2>/dev/null

waited=0
while [ "$waited" -lt 120 ]; do
  now="$(plutil -extract CFBundleShortVersionString raw "$install_dir/Screenroom.app/Contents/Info.plist" 2>/dev/null || printf 'missing')"
  [ "$now" = "$advertised" ] && break
  sleep 1
  waited=$((waited + 1))
done

final="$(plutil -extract CFBundleShortVersionString raw "$install_dir/Screenroom.app/Contents/Info.plist" 2>/dev/null || printf 'missing')"

echo
echo "==> receipt"
sed 's/^/  /' "$receipt" 2>/dev/null || echo "  (none)"
echo
echo "==> GitHub events"
grep -E "updater event" "$work/run.log" | sed 's/^/  /' | tail -12
echo

failures=0
if [ "$final" = "$advertised" ]; then
  echo "  ok:   the installed app is now $final, downloaded from the published release"
else
  echo "  FAIL: the installed app is $final, not $advertised" >&2
  failures=$((failures + 1))
fi
if [ "$(sqlite3 "$library" 'pragma integrity_check' 2>/dev/null)" = "ok" ]; then
  echo "  ok:   the library database survived the upgrade intact"
else
  echo "  FAIL: the library database did not survive" >&2
  failures=$((failures + 1))
fi
if pgrep -f "$install_dir/Screenroom.app/Contents/MacOS/Screenroom" >/dev/null 2>&1; then
  echo "  FAIL: the updated app relaunched itself" >&2
  pkill -f "$install_dir/Screenroom.app/Contents/MacOS/Screenroom" 2>/dev/null || true
  failures=$((failures + 1))
else
  echo "  ok:   the update did not relaunch Screenroom"
fi
if grep -q '"status":"quit_ready"' "$receipt" 2>/dev/null; then
  echo "  ok:   the update was armed for quit before the app closed"
else
  echo "  FAIL: the quit install was never armed" >&2
  failures=$((failures + 1))
fi

echo
if [ "$failures" -ne 0 ]; then
  exit 1
fi
echo "The published release updated an older installed build over the real feed."
