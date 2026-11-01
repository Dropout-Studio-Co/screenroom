#!/usr/bin/env bash

set -uo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

if [ "$(uname -s)" != "Darwin" ]; then
  echo "this check uses lsof against a macOS process; nothing to check on $(uname -s)"
  exit 0
fi

media="${1:-.artifacts/media/Screenroom.Test.Pattern.2026.1080p.mp4}"
seconds="${2:-8}"

case "$media" in
  /*) ;;
  *) media="$root/$media" ;;
esac

if [ ! -f "$media" ]; then
  echo "FAIL: no media at $media" >&2
  exit 1
fi

work="${TMPDIR:-/tmp}/screenroom-offline-audit"
rm -rf "$work"
mkdir -p "$work"

binary="target/release/screenroom-desktop"
if [ ! -x "$binary" ]; then
  echo "==> building"
  cargo build -q --release -p screenroom-desktop || exit 1
fi

echo "==> playing $media for ${seconds}s while watching every socket the process holds"

SCREENROOM_PLAY_CHECK="$work/report" \
  SCREENROOM_PLAY_CHECK_MEDIA="$media" \
  SCREENROOM_PLAY_CHECK_SECONDS="$seconds" \
  SCREENROOM_LIBRARY="$work/library.db" \
  "$binary" >"$work/app.log" 2>&1 &
app=$!

samples=0
noisy=0
while kill -0 "$app" 2>/dev/null; do
  samples=$((samples + 1))
  held="$(lsof -nP -a -p "$app" -i 2>/dev/null | tail -n +2)"
  if [ -n "$held" ]; then
    noisy=$((noisy + 1))
    echo "  socket: $held"
  fi
  sleep 0.2
done

wait "$app" 2>/dev/null
status=$?

if [ "$samples" -lt 5 ]; then
  echo "FAIL: the run was too short to audit ($samples samples)" >&2
  exit 1
fi

echo "  samples: $samples, samples holding any socket: $noisy"

if [ "$noisy" -ne 0 ]; then
  echo "FAIL: the application held a network socket during playback" >&2
  exit 1
fi

if [ "$status" -ne 0 ]; then
  echo "FAIL: the application exited with $status" >&2
  sed 's/^/    /' "$work/app.log" >&2 2>/dev/null || true
  exit 1
fi

if ! grep -q "delivered_fps" "$work/report" 2>/dev/null; then
  echo "FAIL: the run produced no delivery report" >&2
  exit 1
fi

echo "PASS: playback opened no TCP or UDP socket at all"
echo
echo "This is not the same as running with Wi-Fi off. It proves the playback path"
echo "opens no socket; it cannot prove behaviour when the system itself is offline."
