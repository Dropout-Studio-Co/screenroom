#!/usr/bin/env bash
set -euo pipefail

root="repo"
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
media="${1:-/Volumes/bleh/movies & shows}"
database="$(mktemp -d "${TMPDIR:-/tmp}/screenroom-media.XXXXXX")/library.db"
failures=0

export PATH="$HOME/.cargo/bin:/opt/homebrew/bin:$PATH"

pass() { printf 'PASS  %s\n' "$1"; }
fail() { printf 'FAIL  %s\n' "$1"; failures=$((failures + 1)); }
note() { printf 'NOTE  %s\n' "$1"; }

cleanup() { rm -rf "$(dirname "$database")"; }
trap cleanup EXIT

if [[ ! -d "$media" ]]; then
    fail "the media folder is not there: $media"
    exit 1
fi

if ! mount | grep -q "$(dirname "$media")"; then
    note "the folder is not on a mounted external volume; scanning anyway"
fi

count() {
    find "$media" -type f \( -iname '*.mkv' -o -iname '*.mp4' -o -iname '*.m4v' \
        -o -iname '*.avi' -o -iname '*.mov' -o -iname '*.webm' \) 2>/dev/null \
        | grep -vE '/\._|/\.' | wc -l | tr -d ' '
}

files="$(count)"
if [[ "$files" -lt 1 ]]; then
    fail "no media files were found under $media"
    exit 1
fi
pass "found $files media files"

if ! command -v sqlite3 >/dev/null 2>&1; then
    fail "sqlite3 is needed to read the scanned library"
    exit 1
fi

started=$(python3 -c 'import time;print(time.time())')
"$root/target/release/examples/prepare_library" "$database" "$media" "media check" >/dev/null \
    || "$root/target/release/screenroom-core" >/dev/null 2>&1
elapsed=$(python3 -c "import time;print(round(time.time()-$started,2))")

imported=$(sqlite3 "$database" "select count(*) from files;" 2>/dev/null || echo 0)
works=$(sqlite3 "$database" "select count(*) from works;" 2>/dev/null || echo 0)
movies=$(sqlite3 "$database" "select count(*) from works where kind='movie';" 2>/dev/null || echo 0)
series=$(sqlite3 "$database" "select count(*) from works where kind='series';" 2>/dev/null || echo 0)
seasons=$(sqlite3 "$database" "select count(*) from seasons;" 2>/dev/null || echo 0)
episodes=$(sqlite3 "$database" "select count(*) from episodes;" 2>/dev/null || echo 0)
mapped=$(sqlite3 "$database" "select count(*) from file_episodes;" 2>/dev/null || echo 0)

pass "scanned $imported files in ${elapsed}s into $works titles ($movies films, $series series)"
pass "$series series hold $seasons seasons and $episodes episodes"

if [[ "$imported" -lt "$files" ]]; then
    fail "only $imported of $files files reached the library"
else
    pass "every media file reached the library"
fi

unreachable=$((imported - movies - mapped))
if [[ "$unreachable" -ne 0 ]]; then
    fail "$unreachable files belong to neither a film nor an episode"
else
    pass "every file belongs to a film or an episode"
fi

no_year=$(sqlite3 "$database" "select count(*) from works where year is null;" 2>/dev/null || echo 0)
if [[ "$no_year" -ne 0 ]]; then
    note "$no_year titles were identified without a year"
else
    pass "every title was identified with a year"
fi

duplicates=$(sqlite3 "$database" "select count(*) from (select identity_key from works group by identity_key having count(*) > 1);" 2>/dev/null || echo 0)
if [[ "$duplicates" -ne 0 ]]; then
    fail "$duplicates titles share an identity, so one film is listed twice"
else
    pass "no title is listed twice"
fi

if [[ -x "$root/target/release/examples/hwdec_measure" ]]; then
    largest="$(find "$media" -type f \( -iname '*.mkv' -o -iname '*.mp4' \) -print0 2>/dev/null \
        | xargs -0 ls -S 2>/dev/null | head -1)"
    if [[ -n "$largest" ]]; then
        line="$("$root/target/release/examples/hwdec_measure" auto-copy real-media "$largest" 22 2>/dev/null | tail -1)"
        fps="$(printf '%s\n' "$line" | cut -d, -f5)"
        decoder="$(printf '%s\n' "$line" | cut -d, -f11)"
        if [[ -n "$fps" ]] && python3 -c "import sys; sys.exit(0 if float('$fps') >= 23 else 1)"; then
            pass "played the largest file at ${fps} fps through ${decoder}"
        else
            fail "playback of the largest file measured ${fps:-nothing} fps through ${decoder:-nothing}"
        fi
    fi
else
    note "build the player example first: cargo build --release -p screenroom-player --example hwdec_measure"
fi

note "the media folder is only ever read; nothing here writes to it"
if [[ "$failures" -ne 0 ]]; then
    printf '\n%d check(s) failed\n' "$failures"
    exit 1
fi
printf '\nall checks passed\n'
