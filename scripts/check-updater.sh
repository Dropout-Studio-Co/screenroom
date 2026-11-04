#!/usr/bin/env bash

set -uo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
cd "$root"

keep="${SCREENROOM_KEEP_TEST_WORK:-0}"
work="$(mktemp -d "${TMPDIR:-/tmp}/screenroom-updater.XXXXXX")"
install_root="$work/install"
release_root="$work/release"
receipt_root="$work/receipts"
data_root="$work/data"
server_pid=""
failures=0
port=""

cleanup() {
  rm -f "$work/ed25519.key"
  if [ -n "$server_pid" ] && kill -0 "$server_pid" 2>/dev/null; then
    kill "$server_pid" 2>/dev/null || true
    wait "$server_pid" 2>/dev/null || true
  fi
  if [ "$keep" = "1" ]; then
    printf 'kept %s\n' "$work"
  else
    rm -rf "$work"
  fi
}

trap cleanup EXIT

fail() {
  printf '  FAIL: %s\n' "$*" >&2
  failures=$((failures + 1))
}

ok() {
  printf '  ok:   %s\n' "$*"
}

note() {
  printf '  note: %s\n' "$*"
}

if [ "$(uname -s)" != "Darwin" ]; then
  echo "the updater is macOS only; nothing to check on $(uname -s)"
  exit 0
fi

[ -d vendor/sparkle/Sparkle.framework ] || {
  echo "run scripts/fetch-sparkle.sh first" >&2
  exit 2
}

private_key=""
if [ -n "${SCREENROOM_SPARKLE_ED_KEY:-}" ]; then
  private_key="$work/ed25519.key"
  umask 077
  printf '%s\n' "$SCREENROOM_SPARKLE_ED_KEY" >"$private_key"
else
  account="${SCREENROOM_SPARKLE_ACCOUNT:-screenroom}"
  private_key="$work/ed25519.key"
  if ! vendor/sparkle/bin/generate_keys --account "$account" -x "$private_key" >/dev/null 2>&1; then
    echo "no signing key: set SCREENROOM_SPARKLE_ED_KEY or import one into the Keychain account $account" >&2
    exit 2
  fi
fi

umask 077
if [ ! -s "$private_key" ]; then
  echo "the signing key is empty" >&2
  exit 2
fi

port="$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1]); s.close()')"

base="http://127.0.0.1:$port"
feed_base="$base/good/appcast.xml"

echo "==> building the two signed versions"
build_version() {
  local version="$1"
  SCREENROOM_ALLOW_TEST_FEED=1 \
    SCREENROOM_VERSION="$version" \
    SCREENROOM_UPDATE_FEED_URL="$feed_base" \
    scripts/package-macos.sh >"$work/build-$version.log" 2>&1 || return 1
}

build_version 0.1.0 || {
  tail -20 "$work/build-0.1.0.log" >&2
  echo "the 0.1.0 build failed" >&2
  exit 1
}

mkdir -p "$work/pristine"
ditto dist/Screenroom.app "$work/pristine/Screenroom.app"
ditto -c -k --sequesterRsrc --keepParent "$work/pristine/Screenroom.app" "$work/Screenroom-0.1.0-macos-arm64.zip"
ok "0.1.0 packaged and archived"

build_version 0.2.0 || {
  tail -20 "$work/build-0.2.0.log" >&2
  echo "the 0.2.0 build failed" >&2
  exit 1
}

mkdir -p "$work/v2"
ditto dist/Screenroom.app "$work/v2/Screenroom.app"
ditto -c -k --sequesterRsrc --keepParent "$work/v2/Screenroom.app" "$work/Screenroom-0.2.0-macos-arm64.zip"
version_two="$(plutil -extract CFBundleShortVersionString raw "$work/v2/Screenroom.app/Contents/Info.plist")"
ok "0.2.0 packaged and archived (CFBundleShortVersionString $version_two)"

make_feed() {
  local name="$1" archive="$2"
  local dir="$release_root/$name"
  mkdir -p "$dir"
  cp "$archive" "$dir/"
  cat "$private_key" | vendor/sparkle/bin/generate_appcast \
    --ed-key-file - \
    --maximum-deltas 0 \
    --maximum-versions 0 \
    --download-url-prefix "$base/$name/" \
    -o "$dir/appcast.xml" \
    "$dir" >"$work/appcast-$name.log" 2>&1 || {
      tail -10 "$work/appcast-$name.log" >&2
      return 1
    }
  [ -s "$dir/appcast.xml" ] || return 1
  if grep -q "signing warning" "$dir/appcast.xml" 2>/dev/null; then
    note "the $name feed carries the signing warning"
  fi
  return 0
}

make_feed good "$work/Screenroom-0.2.0-macos-arm64.zip" || {
  echo "the signed feed could not be generated" >&2
  exit 1
}
ok "signed feed generated"

make_feed downgrade "$work/Screenroom-0.1.0-macos-arm64.zip" || {
  echo "the downgrade feed could not be generated" >&2
  exit 1
}
ok "lower-version feed generated"

make_feed tampered "$work/Screenroom-0.2.0-macos-arm64.zip" || {
  echo "the tampered-case feed could not be generated" >&2
  exit 1
}
python3 - "$release_root/tampered/Screenroom-0.2.0-macos-arm64.zip" <<'PYTHON'
import sys
path = sys.argv[1]
with open(path, "r+b") as handle:
    data = handle.read()
    position = len(data) // 2
    handle.seek(position)
    handle.write(bytes([data[position] ^ 0xFF]))
PYTHON
ok "tampered archive prepared"

make_feed unsigned "$work/Screenroom-0.2.0-macos-arm64.zip" || {
  echo "the unsigned-case feed could not be generated" >&2
  exit 1
}
python3 - "$release_root/unsigned/appcast.xml" <<'PYTHON'
import re
import sys
path = sys.argv[1]
with open(path, encoding="utf-8") as handle:
    text = handle.read()
stripped = re.sub(r"<!--.*?-->", "", text, flags=re.S)
stripped = re.sub(r'\s+sparkle:edSignature="[^"]*"', "", stripped)
stripped = re.sub(r'\s+sparkle:length="[^"]*"', "", stripped)
with open(path, "w", encoding="utf-8") as handle:
    handle.write(stripped)
PYTHON
ok "unsigned feed prepared"

python3 -m http.server "$port" --bind 127.0.0.1 --directory "$release_root" >"$work/server.log" 2>&1 &
server_pid=$!

for _ in $(seq 1 40); do
  if curl -fsS "$base/good/appcast.xml" -o /dev/null 2>/dev/null; then
    break
  fi
  sleep 0.25
done

if ! curl -fsS "$base/good/appcast.xml" -o /dev/null 2>/dev/null; then
  echo "the local update server did not start" >&2
  cat "$work/server.log" >&2
  exit 1
fi
ok "local feed served from $base"

install_version() {
  local version="$1" destination="$2"
  rm -rf "$destination"
  mkdir -p "$destination"
  if [ "$version" = "0.1.0" ]; then
    ditto "$work/pristine/Screenroom.app" "$destination/Screenroom.app"
    return 0
  fi
  ditto "$work/v2/Screenroom.app" "$destination/Screenroom.app"
}

run_update() {
  local name="$1" feed="$2" install_dir="$3" mode="$4"
  local receipt="$receipt_root/$name.jsonl"
  local library="$data_root/$name/library.db"
  local override=()
  mkdir -p "$data_root/$name" "$receipt_root"
  rm -f "$receipt"
  if [ -n "$feed" ]; then
    override=(SCREENROOM_UPDATE_FEED="$feed")
  fi
  env SCREENROOM_WINDOW=hidden \
    SCREENROOM_LIBRARY="$library" \
    SCREENROOM_UPDATE_CHECK="$receipt" \
    SCREENROOM_UPDATE_INSTALL="$mode" \
    SCREENROOM_UPDATE_TIMEOUT=150 \
    SCREENROOM_UPDATE_DEBUG=1 \
    "${override[@]}" \
    "$install_dir/Screenroom.app/Contents/MacOS/Screenroom" >"$work/run-$name.log" 2>&1 &
  local pid=$!
  local waited=0
  while kill -0 "$pid" 2>/dev/null && [ "$waited" -lt 240 ]; do
    sleep 1
    waited=$((waited + 1))
  done
  if kill -0 "$pid" 2>/dev/null; then
    kill "$pid" 2>/dev/null
    wait "$pid" 2>/dev/null
    fail "$name did not finish its update run within 240s"
    return 1
  fi
  wait "$pid" 2>/dev/null
  return 0
}

installed_version() {
  plutil -extract CFBundleShortVersionString raw "$1/Screenroom.app/Contents/Info.plist" 2>/dev/null || printf 'missing'
}

wait_for_version() {
  local install_dir="$1" expected="$2"
  local waited=0
  while [ "$waited" -lt 90 ]; do
    if [ "$(installed_version "$install_dir")" = "$expected" ]; then
      return 0
    fi
    sleep 1
    waited=$((waited + 1))
  done
  return 1
}

receipt_has() {
  grep -q "$2" "$receipt_root/$1.jsonl" 2>/dev/null
}

echo
echo "==> case: a signed newer version is accepted and installs on quit"
case_dir="$install_root/accepted"
install_version 0.1.0 "$case_dir"
if run_update accepted "" "$case_dir" quit; then
  note "receipt: $(tr '\n' ' ' <"$receipt_root/accepted.jsonl" | tail -c 400)"
  if receipt_has accepted '"status":"ready"'; then
    ok "the update was reported ready before anything installed"
  else
    fail "the update was never reported as ready"
  fi
  if receipt_has accepted quit_ready; then
    ok "the quit-time installation was armed before the application exited"
  else
    fail "the quit-time installation was not armed"
  fi
  if wait_for_version "$case_dir" "$version_two"; then
    ok "the installed bundle is $version_two after the update"
  else
    fail "the installed bundle is still $(installed_version "$case_dir")"
  fi
  library="$data_root/accepted/library.db"
  if [ -f "$library" ]; then
    if [ "$(sqlite3 "$library" 'pragma integrity_check' 2>/dev/null)" = "ok" ]; then
      ok "the library database survived the upgrade intact"
    else
      fail "the library database did not survive the upgrade"
    fi
  else
    fail "the library database is missing after the upgrade"
  fi
  if pgrep -f "$case_dir/Screenroom.app/Contents/MacOS/Screenroom" >/dev/null 2>&1; then
    fail "the updated application relaunched itself"
    pkill -f "$case_dir/Screenroom.app/Contents/MacOS/Screenroom" 2>/dev/null || true
  else
    ok "the update did not relaunch Screenroom"
  fi
fi

echo
echo "==> case: the visible Update now path installs without relaunching"
case_dir="$install_root/inplace"
install_version 0.1.0 "$case_dir"
if run_update inplace "" "$case_dir" now; then
  if wait_for_version "$case_dir" "$version_two"; then
    ok "an approved install replaced the bundle without waiting for a quit"
  else
    fail "the approved install did not finish; the bundle is $(installed_version "$case_dir")"
  fi
  if pgrep -f "$case_dir/Screenroom.app/Contents/MacOS/Screenroom" >/dev/null 2>&1; then
    fail "the approved install relaunched the application"
    pkill -f "$case_dir/Screenroom.app/Contents/MacOS/Screenroom" 2>/dev/null || true
  else
    ok "the approved install did not relaunch the application"
  fi
fi

echo
echo "==> case: an unreachable feed fails without touching the installation"
case_dir="$install_root/offline"
dead_port="$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1]); s.close()')"
install_version 0.1.0 "$case_dir"
if run_update offline "http://127.0.0.1:$dead_port/appcast.xml" "$case_dir" stop; then
  if [ "$(installed_version "$case_dir")" = "0.1.0" ]; then
    ok "an unreachable feed left the installed version alone"
  else
    fail "an unreachable feed changed the installation"
  fi
  if grep -q '"status":"failed"' "$receipt_root/offline.jsonl" 2>/dev/null; then
    ok "the failure was reported rather than swallowed"
  else
    fail "an unreachable feed produced no failure report"
  fi
fi

echo
echo "==> case: a lower version is not offered"
case_dir="$install_root/downgrade"
install_version 0.2.0 "$case_dir"
if run_update downgrade "$base/downgrade/appcast.xml" "$case_dir" stop; then
  if [ "$(installed_version "$case_dir")" = "$version_two" ]; then
    ok "the running newer version was left alone"
  else
    fail "a lower version replaced the newer one"
  fi
  if grep -q '"status":"failed"' "$receipt_root/downgrade.jsonl" 2>/dev/null; then
    fail "a lower version was reported as a failure instead of no update"
  else
    ok "no failure was reported for a feed that only carries an older version"
  fi
fi

echo
echo "==> case: a tampered archive is refused"
case_dir="$install_root/tampered"
install_version 0.1.0 "$case_dir"
if run_update tampered "$base/tampered/appcast.xml" "$case_dir" quit; then
  sleep 3
  if [ "$(installed_version "$case_dir")" = "$version_two" ]; then
    fail "a tampered archive was installed"
  else
    ok "the tampered archive was refused; the installed version stayed $(installed_version "$case_dir")"
  fi
  if grep -q "improperly signed and could not be validated" "$work/run-tampered.log"; then
    ok "the refusal named the signature, not a network or disk fault"
  else
    fail "the tampered archive was refused for an unexpected reason"
  fi
fi

echo
echo "==> case: an unsigned feed is refused"
case_dir="$install_root/unsigned"
install_version 0.1.0 "$case_dir"
if run_update unsigned "$base/unsigned/appcast.xml" "$case_dir" quit; then
  sleep 3
  if [ "$(installed_version "$case_dir")" = "$version_two" ]; then
    fail "an unsigned feed was accepted"
  else
    ok "the unsigned feed was refused; the installed version stayed $(installed_version "$case_dir")"
  fi
  if grep -q "update feed is improperly signed" "$work/run-unsigned.log"; then
    ok "the refusal named the feed signature, and no unsigned fallback was used"
  else
    fail "the unsigned feed was refused for an unexpected reason"
  fi
fi

echo
if [ "$failures" -ne 0 ]; then
  echo "$failures updater check(s) failed."
  exit 1
fi

echo "All updater checks passed."
echo
echo "Not proven here: an update delivered from a real GitHub release, a paid"
echo "Developer ID signature, notarisation, or the owner's own installation."
