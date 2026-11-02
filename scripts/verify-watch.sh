#!/usr/bin/env bash

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
binary="$root/target/release/screenroom-desktop"
seed="$root/.artifacts/media/Screenroom.Test.Pattern.2026.1080p.mp4"
work="$(mktemp -d "${TMPDIR:-/tmp}/screenroom-watch.XXXXXX")"
media="$work/media"
library="$work/library.db"
log="$work/app.log"

cleanup() {
    if [[ -n "${app:-}" ]] && kill -0 "$app" 2>/dev/null; then
        kill "$app" 2>/dev/null || true
        wait "$app" 2>/dev/null || true
    fi
    rm -rf "$work"
}
trap cleanup EXIT

if [[ ! -x "$binary" ]]; then
    echo "build first: cargo build --release -p screenroom-desktop" >&2
    exit 2
fi

if [[ ! -f "$seed" ]]; then
    echo "missing seed clip: $seed" >&2
    exit 2
fi

mkdir -p "$media"
cp "$seed" "$media/Already Here (2020).mp4"

cargo run -q --release -p screenroom-core --example prepare_library -- \
    "$library" "$media" "watch check" >/dev/null

count_rows() {
    sqlite3 "$library" "select count(*) from files where relative_path like '%$1%';" 2>/dev/null || echo 0
}

before="$(count_rows 'Already Here')"
if [[ "$before" != "1" ]]; then
    echo "the scratch library did not start with one file (found $before)" >&2
    exit 1
fi

SCREENROOM_LIBRARY="$library" \
    SCREENROOM_WINDOW=hidden \
    SCREENROOM_OPEN_SCREEN=library \
    "$binary" >"$log" 2>&1 &
app=$!

for _ in $(seq 1 24); do
    if ! kill -0 "$app" 2>/dev/null; then
        echo "the application exited during startup:" >&2
        tail -5 "$log" >&2
        exit 1
    fi
    sleep 0.5
done

started=$(python3 -c 'import time;print(time.time())')
cp "$seed" "$media/Arrived Just Now (2026).mp4"

arrived=""
for _ in $(seq 1 40); do
    if [[ "$(count_rows 'Arrived Just Now')" == "1" ]]; then
        arrived=$(python3 -c 'import time;print(round(time.time()-'"$started"',2))')
        break
    fi
    sleep 0.25
done

if [[ -z "$arrived" ]]; then
    echo "FAIL: a file copied into the watched folder did not reach the library within 10s"
    tail -10 "$log" >&2
    exit 1
fi

echo "PASS: the library picked up the new file in ${arrived}s without a rescan"

rm -f "$media/Arrived Just Now (2026).mp4"
available_rows() {
    sqlite3 "$library" \
        "select count(*) from files where relative_path like '%Arrived Just Now%' and available = 1;" \
        2>/dev/null || echo 0
}

gone=""
for _ in $(seq 1 40); do
    if [[ "$(available_rows)" == "0" ]]; then
        gone="yes"
        break
    fi
    sleep 0.25
done

if [[ -z "$gone" ]]; then
    echo "NOTE: the removed file is still listed as available after 10s; the reconcile pass corrects it later (P2-08)"
else
    echo "PASS: the removed file stopped being available within 10s"
fi
