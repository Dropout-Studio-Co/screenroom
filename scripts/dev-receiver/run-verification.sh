#!/bin/bash
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/../target}"
export SCREENROOM_SKIP_REBUILD=1
export SCREENROOM_ALLOW_FOREGROUND=0

WORK="${1:-}"
if [ -z "$WORK" ]; then
  WORK="$(mktemp -d "${TMPDIR:-/tmp}/screenroom-cast-verify.XXXXXX")"
fi
mkdir -p "$WORK/evidence" "$WORK/evidence-dropped"
MEDIA="$WORK/clip.mp4"
PROBE="$CARGO_TARGET_DIR/debug/examples/cast_lan_probe"
RECEIVER="$ROOT/scripts/dev-receiver/receiver.py"

failures=0
fail() {
  echo "FAIL $*"
  failures=$((failures + 1))
}
pass() {
  echo "PASS $*"
}

if [ ! -x "$PROBE" ]; then
  echo "building the casting probe"
  (cd "$ROOT" && cargo build -p screenroom-desktop --example cast_lan_probe) || exit 1
fi

probe_binary="$PROBE"
if [ ! -x "$probe_binary" ]; then
  probe_binary="$CARGO_TARGET_DIR/debug/examples/cast_lan_probe"
fi

if [ ! -s "$MEDIA" ]; then
  echo "generating the rights-cleared test clip"
  ffmpeg -hide_banner -loglevel error -y \
    -f lavfi -i "testsrc2=size=640x360:rate=25:duration=20" \
    -f lavfi -i "sine=frequency=440:duration=20" \
    -c:v libx264 -preset veryfast -pix_fmt yuv420p -c:a aac -b:a 96k -shortest \
    -movflags +faststart "$MEDIA" || exit 1
fi

SOURCE_BYTES="$(stat -f%z "$MEDIA")"

read_port() {
  python3 - "$1" <<'PY'
import json, sys
try:
    with open(sys.argv[1]) as handle:
        print(json.load(handle).get("http_port") or "")
except (OSError, ValueError):
    print("")
PY
}

wait_listening() {
  local status="$1"
  local deadline=$((SECONDS + 15))
  while [ "$SECONDS" -lt "$deadline" ]; do
    if [ "$(python3 - "$status" <<'PY'
import json, sys
try:
    with open(sys.argv[1]) as handle:
        print("yes" if json.load(handle).get("listening") else "no")
except (OSError, ValueError):
    print("no")
PY
)" = "yes" ]; then
      return 0
    fi
    sleep 0.1
  done
  return 1
}

echo "== full session =="
python3 "$RECEIVER" \
  --status "$WORK/receiver-status.json" \
  --log "$WORK/receiver.log" \
  --pull-dir "$WORK" > "$WORK/receiver.stdout" 2>&1 &
RECEIVER_PID=$!
if ! wait_listening "$WORK/receiver-status.json"; then
  fail "the test receiver did not start listening"
  kill -TERM "$RECEIVER_PID" 2>/dev/null
  exit 1
fi
RECEIVER_PORT="$(read_port "$WORK/receiver-status.json")"
pass "test receiver listening on tcp/$RECEIVER_PORT and udp/1900"

"$probe_binary" --mode full --media "$MEDIA" --evidence "$WORK/evidence" --title "Screenroom verification clip" \
  > "$WORK/probe.log" 2>&1
PROBE_STATUS=$?
if [ "$PROBE_STATUS" -ne 0 ]; then
  fail "the full probe exited $PROBE_STATUS; see $WORK/probe.log"
else
  pass "the full probe walked discovery, description, connect, pause, seek, resume, stop, disconnect"
fi
cat "$WORK/probe.log"

sleep 0.5
kill -TERM "$RECEIVER_PID" 2>/dev/null
wait "$RECEIVER_PID" 2>/dev/null
RECEIVER_EXIT=$?

if [ "$RECEIVER_EXIT" -eq 0 ]; then
  pass "the test receiver shut down cleanly"
else
  fail "the test receiver exited $RECEIVER_EXIT"
fi

if [ -z "$(lsof -nP -iTCP:"$RECEIVER_PORT" 2>/dev/null)" ]; then
  pass "the receiver's tcp/$RECEIVER_PORT listener is closed after exit"
else
  fail "something still listens on tcp/$RECEIVER_PORT"
fi

python3 - "$WORK/receiver-status.json" "$WORK/evidence/probe-full.json" "$SOURCE_BYTES" <<'PY'
import json, os, sys

status_path, probe_path, source_bytes = sys.argv[1], sys.argv[2], int(sys.argv[3])
problems = []

def load(path):
    with open(path) as handle:
        return json.load(handle)

try:
    status = load(status_path)
    probe = load(probe_path)
except (OSError, ValueError) as error:
    print(f"FAIL cannot read evidence: {error}")
    raise SystemExit(1)

def check(condition, message):
    if not condition:
        problems.append(message)

check(status.get("listening") is False, "the receiver still reports listening")
check(status.get("closed_at") is not None, "the receiver recorded no close time")
fetched = status.get("fetched", {})
check(fetched.get("http_status") == 206, f"the receiver fetched HTTP {fetched.get('http_status')} instead of 206")
check(fetched.get("bytes") == source_bytes, f"the receiver fetched {fetched.get('bytes')} of {source_bytes} bytes")
check(bool(fetched.get("content_range")), "the receiver saw no Content-Range header")
probe_info = fetched.get("ffprobe", {})
check(probe_info.get("video_codec") == "h264", f"ffprobe read {probe_info.get('video_codec')} video")
check(probe_info.get("audio_codec") == "aac", f"ffprobe read {probe_info.get('audio_codec')} audio")
check(probe_info.get("width") == 640 and probe_info.get("height") == 360, "ffprobe read unexpected dimensions")
check(probe_info.get("size_bytes") == source_bytes, "ffprobe read a different size")
pulled = fetched.get("path") or ""
check(bool(pulled) and os.path.isfile(pulled), "the receiver kept no fetched file")
if pulled and os.path.isfile(pulled):
    check(os.path.getsize(pulled) == source_bytes, "the fetched file has different bytes on disk")

actions = status.get("actions", {})
for action in ("SetAVTransportURI", "Play", "Pause", "Seek", "Stop", "GetTransportInfo", "GetPositionInfo"):
    check(actions.get(action, 0) > 0, f"the receiver never handled {action}")
check(status.get("state") in ("STOPPED", "PAUSED_PLAYBACK"), f"the receiver ended in {status.get('state')}")

states = probe.get("states", {})
expected = {
    "after_connect": "playing",
    "after_pause": "paused",
    "after_seek": "paused",
    "after_resume": "playing",
    "after_stop": "stopped",
}
for name, want in expected.items():
    check(states.get(name) == want, f"{name} read back {states.get(name)} instead of {want}")

shutdown = probe.get("shutdown", {})
check(shutdown.get("listener_closed") is True, "the app did not record a closed listener after stop")
check(bool(probe.get("session", {}).get("listeners_before_stop")), "the app held no listener while streaming")

direct = probe.get("direct_endpoint", {})
check(direct.get("range_status") == 206, f"the direct endpoint answered {direct.get('range_status')} to a range")
check(direct.get("range_bytes") == source_bytes, "the direct endpoint served different bytes")
check(direct.get("root_status") == 404, "the direct endpoint answered on /")
check(direct.get("library_status") == 404, "the direct endpoint answered on /library")
check(direct.get("wrong_token_status") == 404, "the direct endpoint answered a wrong token")

session = probe.get("session_endpoint", {})
check(session.get("range_status") == 206, f"the session endpoint answered {session.get('range_status')} to a range")
check(session.get("root_status") == 404, "the session endpoint answered on /")
check(session.get("library_status") == 404, "the session endpoint answered on /library")

receiver = probe.get("discovery", {})
check("Test Receiver" in receiver.get("name", ""), "discovery did not report the test receiver's name")
check("DLNA" in receiver.get("kind", ""), "discovery did not report a renderer kind")
check("MediaRenderer" in probe.get("description", {}).get("friendly_name", "") or bool(probe.get("description", {}).get("services")), "the description yielded no services")

print(json.dumps({
    "states": states,
    "receiver_actions": actions,
    "fetched": {"status": fetched.get("http_status"), "bytes": fetched.get("bytes"), "ffprobe": probe_info},
    "endpoint": {"direct": direct, "session": session},
    "listener_closed": shutdown.get("listener_closed"),
}, indent=2))

if problems:
    for problem in problems:
        print(f"FAIL {problem}")
    raise SystemExit(1)
print("PASS the full session matches every casting claim")
PY
if [ $? -ne 0 ]; then
  failures=$((failures + 1))
fi

echo "== dropped session =="
python3 "$RECEIVER" \
  --status "$WORK/receiver2-status.json" \
  --log "$WORK/receiver2.log" \
  --pull-dir "$WORK/second" > "$WORK/receiver2.stdout" 2>&1 &
RECEIVER2_PID=$!
if ! wait_listening "$WORK/receiver2-status.json"; then
  fail "the second test receiver did not start"
  exit 1
fi
RECEIVER2_PORT="$(read_port "$WORK/receiver2-status.json")"
rm -f "$WORK/ready" "$WORK/go"

"$probe_binary" --mode dropped --media "$MEDIA" --evidence "$WORK/evidence-dropped" \
  --ready "$WORK/ready" --go "$WORK/go" --wait 30 \
  > "$WORK/probe-dropped.log" 2>&1 &
PROBE2_PID=$!

deadline=$((SECONDS + 20))
while [ ! -f "$WORK/ready" ] && [ "$SECONDS" -lt "$deadline" ]; do
  sleep 0.1
done
if [ ! -f "$WORK/ready" ]; then
  fail "the dropped-session probe never reported a live session"
else
  kill -TERM "$RECEIVER2_PID" 2>/dev/null
  wait "$RECEIVER2_PID" 2>/dev/null
  touch "$WORK/go"
fi
wait "$PROBE2_PID"
PROBE2_STATUS=$?
cat "$WORK/probe-dropped.log"
if [ "$PROBE2_STATUS" -eq 0 ]; then
  pass "a receiver killed mid-session produced a clear failure and closed the endpoint"
else
  fail "the dropped-session probe exited $PROBE2_STATUS"
  failures=$((failures + 1))
fi

if [ -z "$(lsof -nP -iTCP:"$RECEIVER2_PORT" 2>/dev/null)" ]; then
  pass "the second receiver's listener is closed after the kill"
else
  fail "the second receiver still listens on tcp/$RECEIVER2_PORT"
fi

echo "== token hygiene =="
TOKEN_FILE="$WORK/evidence/token.txt"
if [ -s "$TOKEN_FILE" ]; then
  TOKEN="$(cat "$TOKEN_FILE")"
  leaks=0
  for file in "$WORK/probe.log" "$WORK/receiver.log" "$WORK/receiver-status.json" "$WORK/probe-dropped.log" "$WORK/receiver2.log" "$WORK/receiver2-status.json" "$WORK/evidence/probe-full.json" "$WORK/evidence-dropped/probe-dropped.json"; do
    [ -f "$file" ] || continue
    count="$(grep -F -c -- "$TOKEN" "$file" 2>/dev/null || true)"
    if [ "${count:-0}" != "0" ]; then
      fail "the stream token appears $count time(s) in $file"
      leaks=$((leaks + 1))
    fi
  done
  rm -f "$TOKEN_FILE"
  if [ "$leaks" -eq 0 ]; then
    pass "the stream token appears in no log, status file or evidence file"
  fi
else
  fail "the probe recorded no token, so hygiene could not be checked"
fi

echo
if [ "$failures" -eq 0 ]; then
  echo "RESULT every casting check passed against the first-party test receiver"
  echo "WORK $WORK"
  exit 0
fi
echo "RESULT $failures casting check(s) failed"
echo "WORK $WORK"
exit 1
