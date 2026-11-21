#!/usr/bin/env bash

set -uo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root" || exit 1

scratch="${1:-$root/.artifacts/macos-completion/durability/abrupt}"
binary="${SCREENROOM_DESKTOP_BIN:-${CARGO_TARGET_DIR:-$root/target}/debug/screenroom-desktop}"
database="$scratch/library.db"
media="$scratch/media"
receipt="$scratch/recovery-receipt.txt"

abort() {
  echo "FAIL: $*" >&2
  exit 1
}

[ -x "$binary" ] || abort "no desktop binary at $binary (run cargo build -p screenroom-desktop)"
[ -f "$database" ] || abort "no scratch library at $database"
[ -d "$media" ] || abort "no scratch media folder at $media"

resolved="$(cd "$scratch" && pwd -P)"
case "$resolved" in
  "$root"/.artifacts/*) ;;
  *) abort "refusing to run outside $root/.artifacts (got $resolved)" ;;
esac

real_library="$HOME/Library/Application Support/Screenroom/library.db"
if [ "$(cd "$(dirname "$database")" && pwd -P)/$(basename "$database")" = "$real_library" ]; then
  abort "refusing to run against the real library at $real_library"
fi

sql() { sqlite3 -cmd ".timeout 5000" "$database" "$1"; }
now_epoch() { python3 -c 'import time; print(f"{time.time():.6f}")'; }
utc_stamp() { date -u +%Y-%m-%dT%H:%M:%SZ; }
frontmost() { lsappinfo info -only name "$(lsappinfo front)" 2>/dev/null | head -1 | sed 's/ ASN:.*//'; }

snapshot() {
  local tag="$1" file="$2"
  {
    echo "tag=$tag"
    echo "at_utc=$(utc_stamp)"
    echo "integrity_check=$(sql 'PRAGMA integrity_check;' | tr '\n' ' ' | sed 's/ *$//')"
    echo "quick_check=$(sql 'PRAGMA quick_check;' | tr '\n' ' ' | sed 's/ *$//')"
    echo "journal_mode=$(sql 'PRAGMA journal_mode;')"
    echo "user_version=$(sql 'PRAGMA user_version;')"
    echo "files_total=$(sql 'SELECT COUNT(*) FROM files;')"
    echo "files_available=$(sql 'SELECT COUNT(*) FROM files WHERE available = 1;')"
    echo "works_total=$(sql 'SELECT COUNT(*) FROM works;')"
    echo "sources_total=$(sql 'SELECT COUNT(*) FROM sources;')"
    echo "sessions_total=$(sql 'SELECT COUNT(*) FROM sessions;')"
    echo "sessions_active=$(sql "SELECT COUNT(*) FROM sessions WHERE status = 'active';")"
    echo "sessions_closed=$(sql "SELECT COUNT(*) FROM sessions WHERE status = 'closed';")"
    echo "jobs_total=$(sql 'SELECT COUNT(*) FROM jobs;')"
    echo "jobs_running=$(sql "SELECT COUNT(*) FROM jobs WHERE status = 'running';")"
    echo "jobs_queued=$(sql "SELECT COUNT(*) FROM jobs WHERE status = 'queued';")"
    echo "conversions_total=$(sql 'SELECT COUNT(*) FROM conversion_jobs;')"
    echo "conversion_probe=$(sql "SELECT COALESCE((SELECT status || '|' || message FROM conversion_jobs WHERE id = 'ws7-running-conversion'), 'absent');")"
    echo "job_probe=$(sql "SELECT COALESCE((SELECT status FROM jobs WHERE id = 'ws7-running-job'), 'absent');")"
    echo "progress_position_ms=$(sql 'SELECT COALESCE(MAX(position_ms), -1) FROM progress;')"
    echo "progress_duration_ms=$(sql 'SELECT COALESCE(MAX(duration_ms), -1) FROM progress;')"
    echo "progress_rows=$(sql 'SELECT COUNT(*) FROM progress;')"
    echo "continue_watching_rows=$(sql 'SELECT COUNT(*) FROM progress WHERE position_ms > 0 AND watched = 0;')"
    echo "progress_last_session=$(sql "SELECT COALESCE(MAX(last_session_id), 'none') FROM progress;")"
    echo "browse_rows=$(sql 'SELECT COUNT(*) FROM files f JOIN editions e ON e.id = f.edition_id JOIN works w ON w.id = e.work_id WHERE f.available = 1 ORDER BY w.title LIMIT 50;')"
    echo "browse_titles=$(sql "SELECT COALESCE(group_concat(title, ' ; '), '') FROM (SELECT w.title AS title FROM files f JOIN editions e ON e.id = f.edition_id JOIN works w ON w.id = e.work_id WHERE f.available = 1 ORDER BY w.title COLLATE NOCASE LIMIT 50);")"
    echo "journal_file=$(test -f "$database-journal" && echo present || echo absent)"
  } >"$file"
}

wait_for_player() {
  local deadline=$((SECONDS + 90))
  while [ "$SECONDS" -lt "$deadline" ]; do
    if [ "$(sql "SELECT COUNT(*) FROM sessions WHERE status = 'active';" 2>/dev/null)" = "1" ]; then
      return 0
    fi
    sleep 0.25
  done
  return 1
}

wait_for_position() {
  local target_ms="$1"
  local deadline=$((SECONDS + 120))
  while [ "$SECONDS" -lt "$deadline" ]; do
    local position
    position="$(sql 'SELECT COALESCE(MAX(position_ms), 0) FROM progress;' 2>/dev/null)"
    if [ -n "$position" ] && [ "$position" -ge "$target_ms" ]; then
      echo "$position"
      return 0
    fi
    sleep 0.25
  done
  return 1
}

wait_for_recovery() {
  local deadline=$((SECONDS + 60))
  while [ "$SECONDS" -lt "$deadline" ]; do
    if [ "$(sql "SELECT COUNT(*) FROM sessions WHERE status = 'active';" 2>/dev/null)" = "0" ]; then
      return 0
    fi
    sleep 0.25
  done
  return 1
}

launch_app() {
  local log="$1"
  shift
  env SCREENROOM_LIBRARY="$database" SCREENROOM_WINDOW=hidden \
    SCREENROOM_UPDATE_DISABLE=1 SCREENROOM_ALLOW_FOREGROUND=0 "$@" \
    "$binary" >"$log" 2>&1 &
  app_pid=$!
}

failures=0
check_contains() {
  local label="$1" needle="$2" haystack="$3"
  if printf '%s' "$haystack" | grep -q "$needle"; then
    echo "PASS $label"
  else
    echo "FAIL $label: no \"$needle\" in: $(printf '%s' "$haystack" | tr '\n' ' ')"
    failures=$((failures + 1))
  fi
}

check() {
  local label="$1" expected="$2" actual="$3"
  if [ "$expected" = "$actual" ]; then
    echo "PASS $label: $actual"
  else
    echo "FAIL $label: expected $expected, got $actual"
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

echo "==> scratch library: $database"
echo "==> real library (must stay untouched): $real_library"
echo "==> binary: $binary"

snapshot "before-launch" "$scratch/state-0-before.txt"
cat "$scratch/state-0-before.txt"

file_id="$(sql 'SELECT id FROM files ORDER BY added_ms LIMIT 1;')"
[ -n "$file_id" ] || abort "the scratch library holds no file"

foreground_before="$(frontmost)"
echo "==> frontmost application before launch: $foreground_before"

echo "==> launching the application hidden with the player screen"
play_log="$scratch/app-play.log"
launch_app "$play_log" SCREENROOM_OPEN_SCREEN=player
play_pid="$app_pid"
play_started="$(now_epoch)"
echo "    pid=$play_pid started_epoch=$play_started started_utc=$(utc_stamp)"

wait_for_player || {
  cat "$play_log" >&2
  kill -9 "$play_pid" 2>/dev/null
  abort "the player never opened a session within 90s"
}
echo "    session opened at $(utc_stamp)"

foreground_during="$(frontmost)"
echo "    frontmost application during playback: $foreground_during"

position="$(wait_for_position 14000)" || {
  cat "$play_log" >&2
  kill -9 "$play_pid" 2>/dev/null
  abort "no checkpoint reached 14s of playback within 120s"
}
echo "    playback checkpoint reached ${position}ms at $(utc_stamp)"

conversion_now="$(sql 'SELECT CAST(strftime("%s","now") AS INTEGER) * 1000;')"
sql "DELETE FROM jobs WHERE id = 'ws7-running-job';" ||
  abort "could not clear the previous running job row"
sql "DELETE FROM conversion_jobs WHERE id = 'ws7-running-conversion';" ||
  abort "could not clear the previous running conversion row"
sql "INSERT INTO jobs(id, kind, target, status, priority, attempts, next_retry_ms)
     VALUES('ws7-running-job', 'probe', '$file_id', 'running', 0, 1, 0);" ||
  abort "could not stage the running job row"
sql "INSERT INTO conversion_jobs(id, file_id, title, profile, status, progress, message,
       source_path, output_path, source_size, source_modified_ms, output_bytes,
       created_ms, updated_ms)
     VALUES('ws7-running-conversion', '$file_id', 'Durability Test Clip', 'remux', 'running', 40,
       'encoding', '$media/Durability Test Clip (2026).mp4', '$scratch/converted.mp4', 2304316, 0, 0,
       $conversion_now, $conversion_now);" ||
  abort "could not stage the running conversion row"
echo "    staged a running job and a running conversion row at $(utc_stamp)"

last_seen=0
for _ in $(seq 1 12); do
  current="$(sql 'SELECT COALESCE(MAX(position_ms), 0) FROM progress;')"
  if [ -n "$current" ]; then
    last_seen="$current"
    echo "    checkpoint $(utc_stamp) position_ms=$current"
  fi
  sleep 0.5
done

snapshot "before-kill" "$scratch/state-1-before-kill.txt"

kill_moment="$(now_epoch)"
kill -9 "$play_pid"
wait "$play_pid" 2>/dev/null
kill_status=$?
echo "==> killed pid $play_pid with SIGKILL at epoch $kill_moment ($(utc_stamp)); wait status=$kill_status"

snapshot "after-kill" "$scratch/state-2-after-kill.txt"
cat "$scratch/state-2-after-kill.txt"

echo "==> relaunching hidden on the same scratch library"
reopen_log="$scratch/app-reopen.log"
launch_app "$reopen_log"
reopen_pid="$app_pid"
echo "    pid=$reopen_pid launched at $(utc_stamp)"

wait_for_recovery || {
  cat "$reopen_log" >&2
  kill -9 "$reopen_pid" 2>/dev/null
  abort "the reopened application never closed the crashed session within 60s"
}
sleep 2

snapshot "after-reopen" "$scratch/state-3-after-reopen.txt"
cat "$scratch/state-3-after-reopen.txt"

kill -9 "$reopen_pid" 2>/dev/null
wait "$reopen_pid" 2>/dev/null

echo "==> reading the recovered catalogue through the core query path on a copy"
catalog_copy="$scratch/verify-copy.db"
cp "$database" "$catalog_copy" || abort "could not copy the recovered library"
catalog_receipt="$(CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$root/target}" \
  cargo run -q -p screenroom-core --example library_check -- "$catalog_copy" 2>&1 |
  grep -E 'catalogue has entries|paging reaches every entry exactly once')"
printf '%s\n' "$catalog_receipt" >"$scratch/catalog-receipt.txt"
printf '    %s\n' "$(printf '%s' "$catalog_receipt" | tr '\n' ' ')"

echo "==> launching hidden on the library screen to read the recovered catalogue"
browse_log="$scratch/app-browse.log"
launch_app "$browse_log" SCREENROOM_OPEN_SCREEN=library
browse_pid="$app_pid"
sleep 6
if kill -0 "$browse_pid" 2>/dev/null; then
  browse_outcome="alive"
else
  browse_outcome="exited"
fi
kill -9 "$browse_pid" 2>/dev/null
wait "$browse_pid" 2>/dev/null
echo "    the library screen stayed open: $browse_outcome"
snapshot "after-browse" "$scratch/state-4-browse.txt"

echo "==> launching hidden to play again on the recovered library"
replay_log="$scratch/app-replay.log"
session_before_replay="$(sql 'SELECT COALESCE(MAX(last_session_id), 0) FROM progress;')"
launch_app "$replay_log" SCREENROOM_OPEN_SCREEN=player
replay_pid="$app_pid"
replay_outcome="no session"
replay_position=0
if wait_for_player; then
  replay_deadline=$((SECONDS + 60))
  while [ "$SECONDS" -lt "$replay_deadline" ]; do
    current_session="$(sql 'SELECT COALESCE(MAX(last_session_id), 0) FROM progress;')"
    current_position="$(sql 'SELECT COALESCE(MAX(position_ms), 0) FROM progress;')"
    if [ "$current_session" != "$session_before_replay" ] && [ -n "$current_position" ] && [ "$current_position" -ge 3000 ]; then
      replay_outcome="played"
      replay_position="$current_position"
      break
    fi
    sleep 0.25
  done
fi
kill -9 "$replay_pid" 2>/dev/null
wait "$replay_pid" 2>/dev/null
echo "    replay after recovery: $replay_outcome at ${replay_position}ms in a new session"
snapshot "after-replay" "$scratch/state-5-replay.txt"

foreground_after="$(frontmost)"
echo "    frontmost application after the run: $foreground_after"

before="$scratch/state-0-before.txt"
after_kill="$scratch/state-2-after-kill.txt"
after="$scratch/state-3-after-reopen.txt"
value() { sed -n "s/^$2=//p" "$1"; }

echo
echo "== assertions =="
check "integrity after the kill" "ok" "$(value "$after_kill" integrity_check)"
check "integrity after the reopen" "ok" "$(value "$after" integrity_check)"
check "session was active at the kill" "1" "$(value "$after_kill" sessions_active)"
check "no session stays active after the reopen" "0" "$(value "$after" sessions_active)"
check "every session row is closed after the reopen" "$(value "$after" sessions_total)" "$(value "$after" sessions_closed)"
check "the interrupted job is queued again" "queued" "$(value "$after" job_probe)"
check "the interrupted conversion is failed" "failed|The app closed before this finished." "$(value "$after" conversion_probe)"
check "the file rows are unchanged" "$(value "$before" files_total)" "$(value "$after" files_total)"
check "the work rows are unchanged" "$(value "$before" works_total)" "$(value "$after" works_total)"
check "the source rows are unchanged" "$(value "$before" sources_total)" "$(value "$after" sources_total)"
check "the library is still browsable" "1" "$(value "$after" browse_rows)"
check "the interrupted viewing is offered again" "1" "$(value "$after" continue_watching_rows)"
check_contains "the core catalogue query reaches the file" "PASS catalogue has entries" "$catalog_receipt"
check_contains "the core paging query reaches every entry" "PASS paging reaches every entry exactly once" "$catalog_receipt"
check_between "the checkpoint survived the kill" "$last_seen" "$((last_seen + 1200))" "$(value "$after_kill" progress_position_ms)"
check_between "the checkpoint survived the reopen" "$last_seen" "$((last_seen + 1200))" "$(value "$after" progress_position_ms)"
check "the stored duration survived" "$(value "$after_kill" progress_duration_ms)" "$(value "$after" progress_duration_ms)"
check "the opened library screen stayed up" "alive" "$browse_outcome"
check "playback after the recovery started a new session" "played" "$replay_outcome"
check_between "the new session reached 3s" "3000" "60000" "$replay_position"
check "the frontmost application did not change" "$foreground_before" "$foreground_after"

{
  echo "abrupt-death recovery, generated $(utc_stamp)"
  echo "branch=$(git -C "$root" rev-parse --abbrev-ref HEAD)"
  echo "commit=$(git -C "$root" rev-parse HEAD)"
  echo "binary=$binary"
  echo "scratch=$resolved"
  echo "database=$database"
  echo "media=$media"
  echo "player_pid=$play_pid"
  echo "player_started_epoch=$play_started"
  echo "kill_signal=SIGKILL(9)"
  echo "kill_epoch=$kill_moment"
  echo "kill_utc=$(utc_stamp)"
  echo "wait_status=$kill_status"
  echo "frontmost_before=$foreground_before"
  echo "frontmost_during=$foreground_during"
  echo "frontmost_after=$foreground_after"
  echo "last_observed_checkpoint_ms=$last_seen"
  echo "reopen_pid=$reopen_pid"
  echo "catalog_receipt=$(printf '%s' "$catalog_receipt" | tr '\n' ' ')"
  echo "browse_outcome=$browse_outcome"
  echo "replay_outcome=$replay_outcome"
  echo "failures=$failures"
} >"$receipt"

echo
if [ "$failures" -eq 0 ]; then
  echo "PASS: the checkpoint survived, the library stayed browsable, nothing was deleted"
  echo "receipt: $receipt"
  exit 0
fi
echo "FAIL: $failures assertion(s) failed; see $scratch/state-*.txt" >&2
exit 1
