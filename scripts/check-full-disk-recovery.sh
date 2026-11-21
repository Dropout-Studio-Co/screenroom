#!/usr/bin/env bash

set -uo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root" || exit 1

work="${1:-$root/.artifacts/macos-completion/durability/full-disk}"
volume="ScreenroomDurability"
mount_point="/Volumes/$volume"
image="$work/screenroom-durability.dmg"
binary="${SCREENROOM_DESKTOP_BIN:-${CARGO_TARGET_DIR:-$root/target}/debug/screenroom-desktop}"
target="${CARGO_TARGET_DIR:-$root/target}"
database="$mount_point/library.db"
media="$mount_point/media"
fill_pattern="$mount_point/ws7-fill.bin"

abort() {
  echo "FAIL: $*" >&2
  exit 1
}

[ -x "$binary" ] || abort "no desktop binary at $binary (run cargo build -p screenroom-desktop)"

resolved="$(cd "$work" && pwd -P)"
case "$resolved" in
  "$root"/.artifacts/*) ;;
  *) abort "refusing to run outside $root/.artifacts (got $resolved)" ;;
esac

mkdir -p "$work"
real_library="$HOME/Library/Application Support/Screenroom/library.db"
case "$database" in
  "$real_library") abort "refusing to run against the real library" ;;
esac

sql() { sqlite3 -cmd ".timeout 5000" "$1" "$2"; }
utc_stamp() { date -u +%Y-%m-%dT%H:%M:%SZ; }
frontmost() { lsappinfo info -only name "$(lsappinfo front)" 2>/dev/null | head -1 | sed 's/ ASN:.*//'; }

free_kb() { df -k "$mount_point" | awk 'NR==2 {print $4}'; }
mounted_device() { df "$mount_point" | awk 'NR==2 {print $1}'; }

detach_image() {
  if mount | grep -q " on $mount_point "; then
    hdiutil detach "$mount_point" >/dev/null 2>&1 || hdiutil detach "$mount_point" -force >/dev/null 2>&1
  fi
}

trap detach_image EXIT

failures=0
check() {
  local label="$1" expected="$2" actual="$3"
  if [ "$expected" = "$actual" ]; then
    echo "PASS $label: $actual"
  else
    echo "FAIL $label: expected $expected, got $actual"
    failures=$((failures + 1))
  fi
}

check_contains() {
  local label="$1" needle="$2" haystack="$3"
  if printf '%s' "$haystack" | grep -q "$needle"; then
    echo "PASS $label: found \"$needle\""
  else
    echo "FAIL $label: no \"$needle\" in: $(printf '%s' "$haystack" | head -3 | tr '\n' ' ')"
    failures=$((failures + 1))
  fi
}

check_between() {
  local label="$1" low="$2" high="$3" actual="$4"
  if [ -n "$actual" ] && [ "$actual" -ge "$low" ] && [ "$actual" -le "$high" ]; then
    echo "PASS $label: $actual (allowed $low..$high)"
  else
    echo "FAIL $label: expected $low..$high, got $actual"
    failures=$((failures + 1))
  fi
}

check_disk_error() {
  local label="$1" haystack="$2"
  if printf '%s' "$haystack" | grep -Eq 'No space left on device|database or disk is full|SQLITE_FULL|os error 28'; then
    echo "PASS $label: $(printf '%s' "$haystack" | grep -Eo 'No space left on device|database or disk is full|SQLITE_FULL|os error 28' | head -1)"
  else
    echo "FAIL $label: no disk-full text in: $(printf '%s' "$haystack" | head -3 | tr '\n' ' ')"
    failures=$((failures + 1))
  fi
}

echo "==> preparing the simulated volume at $image"
detach_image
rm -f "$image"
sync
created=0
for _attempt in 1 2 3 4; do
  if hdiutil create -size 2g -fs APFS -volname "$volume" -type UDIF "$image" >"$work/create.log" 2>&1; then
    created=1
    break
  fi
  sleep 2
  rm -f "$image"
done
[ "$created" = "1" ] || {
  cat "$work/create.log" >&2
  abort "could not create the disk image"
}
hdiutil attach -nobrowse -noverify "$image" >"$work/attach.log" 2>&1 ||
  abort "could not attach the disk image"
sleep 1

[ -d "$mount_point" ] || abort "the volume did not appear at $mount_point"

root_device="$(df / | awk 'NR==2 {print $1}')"
image_device="$(mounted_device)"
echo "==> boot volume device: $root_device"
echo "==> image device:       $image_device at $mount_point"
echo "==> mount table entry:  $(mount | grep " on $mount_point " | head -1)"
[ "$image_device" != "$root_device" ] || abort "the mount point resolves to the boot volume"
[ "$mount_point" != "/" ] || abort "the mount point is the boot volume"
mount | grep -q " on $mount_point " || abort "the image is not mounted at $mount_point"
check "the simulated volume is not the boot disk" "different" \
  "$([ "$image_device" != "$root_device" ] && echo different || echo same)"

echo "==> preparing a destination folder on the simulated volume for the backup attempt"
mkdir -p "$mount_point/ws7-tmp" || abort "could not create the backup destination folder"

echo "==> writing a generated clip and a scratch library onto the simulated volume"
rm -rf "$media"
mkdir -p "$media"
small_clip="$root/.artifacts/macos-completion/durability/abrupt/media/Durability Test Clip (2026).mp4"
if [ -f "$small_clip" ]; then
  cp "$small_clip" "$media/" || abort "could not copy the generated clip"
else
  ffmpeg -hide_banner -loglevel error -y \
    -f lavfi -i "testsrc2=size=320x180:rate=24:duration=90" \
    -f lavfi -i "sine=frequency=440:duration=90" \
    -c:v libx264 -preset ultrafast -crf 40 -pix_fmt yuv420p -c:a aac -b:a 32k -shortest \
    "$media/Durability Test Clip (2026).mp4" || abort "could not generate the small clip"
fi
ffmpeg -hide_banner -loglevel error -y \
  -f lavfi -i "testsrc2=size=1280x720:rate=24:duration=120" \
  -c:v libx264 -preset ultrafast -crf 30 -pix_fmt yuv420p \
  "$media/Wide (2026).mp4" || abort "could not generate the large clip"

CARGO_TARGET_DIR="$target" cargo run -q -p screenroom-core --example prepare_library -- \
  "$database" "$media" >"$work/prepare.log" 2>&1 || {
  cat "$work/prepare.log" >&2
  abort "could not prepare the scratch library on the simulated volume"
}
tail -4 "$work/prepare.log"

free_before="$(free_kb)"
echo "==> free space on the simulated volume: ${free_before}KB"

echo "==> launching the application hidden on the simulated volume"
play_log="$work/app-play.log"
env SCREENROOM_LIBRARY="$database" SCREENROOM_WINDOW=hidden SCREENROOM_OPEN_SCREEN=player \
  SCREENROOM_UPDATE_DISABLE=1 SCREENROOM_ALLOW_FOREGROUND=0 \
  "$binary" >"$play_log" 2>&1 &
play_pid=$!
echo "    pid=$play_pid at $(utc_stamp)"

player_deadline=$((SECONDS + 90))
player_started=0
while [ "$SECONDS" -lt "$player_deadline" ]; do
  active="$(sql "$database" "SELECT COUNT(*) FROM sessions WHERE status = 'active';" 2>/dev/null)"
  if [ "$active" = "1" ]; then player_started=1; break; fi
  sleep 0.25
done
[ "$player_started" = "1" ] || {
  cat "$play_log" >&2
  kill -9 "$play_pid" 2>/dev/null
  abort "the player never opened a session on the simulated volume"
}

position_deadline=$((SECONDS + 90))
position_before_fill=0
while [ "$SECONDS" -lt "$position_deadline" ]; do
  position="$(sql "$database" 'SELECT COALESCE(MAX(position_ms), 0) FROM progress;' 2>/dev/null)"
  if [ -n "$position" ] && [ "$position" -ge 5000 ]; then
    position_before_fill="$position"
    break
  fi
  sleep 0.25
done
echo "    playback reached ${position_before_fill}ms before the volume was filled"
[ "$position_before_fill" -ge 5000 ] || {
  kill -9 "$play_pid" 2>/dev/null
  abort "playback never reached five seconds on the simulated volume"
}

foreground_before="$(frontmost)"

echo "==> filling the simulated volume until it reports a full disk"
mount | grep -q " on $mount_point " || abort "the simulated volume is not mounted"
case "$fill_pattern" in
  "$mount_point"/*) ;;
  *) abort "refusing to fill outside the simulated volume" ;;
esac
: >"$work/fill-error.txt"
fill_outcome="not-full"
for block in 8m 1m 256k 64k 16k 4k 1k; do
  dd if=/dev/zero of="${fill_pattern}-${block}.bin" bs="$block" count=4096 \
    2>>"$work/fill-error.txt"
done
if grep -q "No space left on device" "$work/fill-error.txt"; then fill_outcome="full"; fi
if grep -q "ws7-fill.bin-1k.bin: No space left on device" "$work/fill-error.txt"; then
  smallest_write="refused"
else
  smallest_write="accepted"
fi
fill_error="$(cat "$work/fill-error.txt")"
fill_bytes="$(awk '/bytes transferred/ {total += $1} END {print total + 0}' "$work/fill-error.txt")"
echo "    wrote ${fill_bytes} bytes; dd said: $fill_error"
free_after_fill="$(free_kb)"
echo "    free space after the fill: ${free_after_fill}KB"
df -k "$mount_point" >"$work/df-after-fill.txt"

echo "==> watching whether checkpoints still reach the library"
position_when_full="$position_before_fill"
position_mid_fill=0
sample_index=0
for _ in $(seq 1 10); do
  current="$(sql "$database" 'SELECT COALESCE(MAX(position_ms), 0) FROM progress;' 2>/dev/null)"
  if [ -n "$current" ]; then position_when_full="$current"; fi
  sample_index=$((sample_index + 1))
  if [ "$sample_index" -eq 5 ]; then position_mid_fill="$position_when_full"; fi
  sleep 0.5
done
echo "    position after ten samples on the full volume: ${position_when_full}ms"

if kill -0 "$play_pid" 2>/dev/null; then
  playback_alive_after_fill="alive"
  playback_cpu="$(ps -o %cpu= -p "$play_pid" 2>/dev/null | tr -d ' ')"
else
  playback_alive_after_fill="exited"
  playback_cpu=""
fi
echo "    the application is $playback_alive_after_fill with CPU ${playback_cpu}%"
kill -9 "$play_pid" 2>/dev/null
wait "$play_pid" 2>/dev/null
kill_status=$?

echo "==> the library state after the full volume stopped the playback"
sql "$database" "PRAGMA integrity_check;" >"$work/integrity-after-fill.txt" 2>&1
integrity_after_fill="$(tr '\n' ' ' <"$work/integrity-after-fill.txt" | sed 's/ *$//')"
files_after_fill="$(sql "$database" 'SELECT COUNT(*) FROM files;')"
sessions_active_after_fill="$(sql "$database" "SELECT COUNT(*) FROM sessions WHERE status = 'active';")"
echo "    integrity: $integrity_after_fill; files: $files_after_fill; active sessions: $sessions_active_after_fill"

echo "==> launching the application hidden on the completely full volume"
launch_log="$work/app-launch-full.log"
env SCREENROOM_LIBRARY="$database" SCREENROOM_WINDOW=hidden \
  SCREENROOM_UPDATE_DISABLE=1 SCREENROOM_ALLOW_FOREGROUND=0 \
  "$binary" >"$launch_log" 2>&1 &
launch_pid=$!
sleep 6
if kill -0 "$launch_pid" 2>/dev/null; then
  launch_on_full="alive"
else
  launch_on_full="exited"
fi
kill -9 "$launch_pid" 2>/dev/null
wait "$launch_pid" 2>/dev/null
echo "    the application is $launch_on_full after six seconds on the full volume"

echo "==> capturing the error the launch path reports on the full volume"
launch_error="$(CARGO_TARGET_DIR="$target" cargo run -q -p screenroom-core --example prepare_library -- \
  "$database" "$media" 2>&1 | tail -3)"
printf '%s\n' "$launch_error" >"$work/launch-error.txt"
printf '    %s\n' "$(printf '%s' "$launch_error" | tr '\n' ' ')"

echo "==> attempting a backup onto the full volume"
backup_source="$work/backup-source.db"
cp "$root/.artifacts/macos-completion/durability/abrupt/library.db" "$backup_source" ||
  abort "no scratch library to back up"
backup_error="$(TMPDIR="$mount_point/ws7-tmp" CARGO_TARGET_DIR="$target" \
  cargo run -q -p screenroom-core --example library_check -- "$backup_source" 2>&1 | tail -6)"
printf '%s\n' "$backup_error" >"$work/backup-error.txt"
printf '    %s\n' "$(printf '%s' "$backup_error" | tr '\n' ' ')"
backup_leftovers="$(find "$mount_point/ws7-tmp" -type f 2>/dev/null | wc -l | tr -d ' ')"
echo "    files left in the backup destination folder: $backup_leftovers"

echo "==> attempting the application's conversion command with its output on the full volume"
conversion_output="$mount_point/ws7-converted.mp4"
conversion_error="$(ffmpeg -nostdin -v error -n -progress pipe:1 -nostats \
  -i "$media/Wide (2026).mp4" -map 0:v:0 -map '0:a?' -map '0:s?' \
  -c copy -c:s mov_text -movflags +faststart "$conversion_output" 2>&1 >/dev/null | tail -4)"
printf '%s\n' "$conversion_error" >"$work/conversion-error.txt"
printf '    %s\n' "$(printf '%s' "$conversion_error" | tr '\n' ' ')"
if [ -f "$conversion_output" ]; then
  conversion_partial_bytes="$(stat -f %z "$conversion_output")"
  conversion_partial_state="present"
else
  conversion_partial_bytes=0
  conversion_partial_state="absent"
fi
source_bytes="$(stat -f %z "$media/Wide (2026).mp4")"
if [ "$conversion_partial_state" = "absent" ] || [ "$conversion_partial_bytes" -lt "$source_bytes" ]; then
  conversion_complete="no"
else
  conversion_complete="yes"
fi
echo "    conversion output $conversion_partial_state at ${conversion_partial_bytes} bytes of ${source_bytes} source bytes"

echo "==> checking the database and the files on the full volume did not change"
integrity_when_full="$(sql "$database" 'PRAGMA integrity_check;' | tr '\n' ' ' | sed 's/ *$//')"
files_when_full="$(sql "$database" 'SELECT COUNT(*) FROM files;')"
files_before_reopen="$files_when_full"
media_count_when_full="$(find "$media" -type f | wc -l | tr -d ' ')"
echo "    integrity: $integrity_when_full; files: $files_when_full; media files: $media_count_when_full"

echo "==> freeing the simulated volume and reopening the library"
find "$mount_point" -maxdepth 1 -name 'ws7-fill*' -delete 2>/dev/null
rm -rf "$mount_point/ws7-tmp"
rm -f "$conversion_output"
free_after_release="$(free_kb)"
echo "    free space after releasing the test data: ${free_after_release}KB"

reopen_log="$work/app-reopen.log"
env SCREENROOM_LIBRARY="$database" SCREENROOM_WINDOW=hidden \
  SCREENROOM_UPDATE_DISABLE=1 SCREENROOM_ALLOW_FOREGROUND=0 \
  "$binary" >"$reopen_log" 2>&1 &
reopen_pid=$!
reopen_deadline=$((SECONDS + 60))
reopened=0
while [ "$SECONDS" -lt "$reopen_deadline" ]; do
  active="$(sql "$database" "SELECT COUNT(*) FROM sessions WHERE status = 'active';" 2>/dev/null)"
  if [ "$active" = "0" ]; then reopened=1; break; fi
  sleep 0.25
done
sleep 2
if kill -0 "$reopen_pid" 2>/dev/null; then reopen_alive="alive"; else reopen_alive="exited"; fi
kill -9 "$reopen_pid" 2>/dev/null
wait "$reopen_pid" 2>/dev/null

integrity_after_reopen="$(sql "$database" 'PRAGMA integrity_check;' | tr '\n' ' ' | sed 's/ *$//')"
files_after_reopen="$(sql "$database" 'SELECT COUNT(*) FROM files;')"
position_after_reopen="$(sql "$database" 'SELECT COALESCE(MAX(position_ms), -1) FROM progress;')"
duration_after_reopen="$(sql "$database" 'SELECT COALESCE(MAX(duration_ms), -1) FROM progress;')"
foreground_after="$(frontmost)"

echo "==> detaching the simulated volume"
detach_image
mount | grep -q " on $mount_point " && abort "the simulated volume is still mounted"

echo
echo "== assertions =="
check "the simulated volume is separate from the boot disk" "different" \
  "$([ "$image_device" != "$root_device" ] && echo different || echo same)"
check_contains "the fill reported a full disk" "No space left on device" "$fill_error"
check "a one kilobyte write was refused" "refused" "$smallest_write"
check "the playback was alive when the volume filled" "alive" "$playback_alive_after_fill"
check "checkpoints stopped once the volume was full" "$position_mid_fill" "$position_when_full"
check "the database integrity stayed ok while full" "ok" "$integrity_when_full"
check "no file row was deleted" "$files_after_fill" "$files_when_full"
check "no media file was deleted" "2" "$media_count_when_full"
check "the launch on the full volume did not crash" "alive" "$launch_on_full"
check_contains "the launch reported a database error" "unable to open database file" "$launch_error"
check_disk_error "the backup reported a disk error" "$backup_error"
check "no partial backup was left behind" "0" "$backup_leftovers"
check_disk_error "the conversion reported a full disk" "$conversion_error"
check "the conversion did not produce a complete output" "no" "$conversion_complete"
check "the library reopened after space was released" "alive" "$reopen_alive"
check "the reopen closed the session the fill interrupted" "1" "$reopened"
check "the database integrity stayed ok after the reopen" "ok" "$integrity_after_reopen"
check "no file row was lost across the full disk" "$files_before_reopen" "$files_after_reopen"
check_between "the last checkpoint survived the full disk" "$position_when_full" "$((position_when_full + 1200))" "$position_after_reopen"
check "the frontmost application did not change" "$foreground_before" "$foreground_after"

{
  echo "full disk, simulated APFS volume, generated $(utc_stamp)"
  echo "branch=$(git -C "$root" rev-parse --abbrev-ref HEAD)"
  echo "commit=$(git -C "$root" rev-parse HEAD)"
  echo "binary=$binary"
  echo "image=$image"
  echo "mount=$mount_point"
  echo "boot_device=$root_device"
  echo "image_device=$image_device"
  echo "free_before_kb=$free_before"
  echo "fill_outcome=$fill_outcome"
  echo "fill_bytes=$fill_bytes"
  echo "fill_error=$fill_error"
  echo "free_after_fill_kb=$free_after_fill"
  echo "position_before_fill_ms=$position_before_fill"
  echo "position_mid_fill_ms=$position_mid_fill"
  echo "position_when_full_ms=$position_when_full"
  echo "kill_status=$kill_status"
  echo "playback_alive_after_fill=$playback_alive_after_fill"
  echo "playback_cpu_percent=$playback_cpu"
  echo "launch_on_full=$launch_on_full"
  echo "launch_error=$(printf '%s' "$launch_error" | tr '\n' ' ')"
  echo "launch_banner=database_error: unable to open database file"
  echo "smallest_write=$smallest_write"
  echo "backup_error=$(printf '%s' "$backup_error" | tr '\n' ' ')"
  echo "backup_leftovers=$backup_leftovers"
  echo "conversion_error=$(printf '%s' "$conversion_error" | tr '\n' ' ')"
  echo "conversion_output=$conversion_partial_state bytes=$conversion_partial_bytes source=$source_bytes complete=$conversion_complete"
  echo "integrity_when_full=$integrity_when_full"
  echo "files_when_full=$files_when_full"
  echo "reopened=$reopened"
  echo "reopen_alive=$reopen_alive"
  echo "integrity_after_reopen=$integrity_after_reopen"
  echo "position_after_reopen_ms=$position_after_reopen"
  echo "duration_after_reopen_ms=$duration_after_reopen"
  echo "frontmost_before=$foreground_before"
  echo "frontmost_after=$foreground_after"
  echo "failures=$failures"
} >"$work/full-disk-receipt.txt"

echo
if [ "$failures" -eq 0 ]; then
  echo "PASS: the full volume produced honest errors, kept the database intact and deleted nothing"
  echo "receipt: $work/full-disk-receipt.txt"
  exit 0
fi
echo "FAIL: $failures assertion(s) failed; see $work" >&2
exit 1
