#!/usr/bin/env bash

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
config_file="${SCREENROOM_DEVICES_FILE:-$HOME/.config/screenroom/devices.env}"
work_dir="$(mktemp -d "${TMPDIR:-/tmp}/screenroom-ship.XXXXXX")"
launched_names=()
relaunch=1
requested=""
only_given=0

cleanup() {
  rm -rf "$work_dir"
}

trap cleanup EXIT

usage() {
  cat >&2 <<'TEXT'
usage: scripts/ship.sh [--only macos|windows|tv] [--no-relaunch]

macos     build the app, replace the installed copy, relaunch it
windows   run the release build on the configured Windows host over SSH
tv        build, sign and install the widget on the reachable Samsung TV

The three lanes run at once and the table at the end reports each one. A powered
off or absent device is skipped, not failed. The command exits non-zero when the
macOS lane failed, because that is the device that is always on.

Configuration, read from the environment or from ~/.config/screenroom/devices.env
(SCREENROOM_DEVICES_FILE overrides the path):

  SCREENROOM_MACOS_INSTALL_DIR   where Screenroom.app is installed
  SCREENROOM_WINDOWS_HOST        user@host of the Windows machine
  SCREENROOM_WINDOWS_REPO        repository path on that machine
  SCREENROOM_TV_ADDRESS          address the TV was last seen on, port 26101
  SCREENROOM_TIZEN_PROFILE       signing profile, default screenroom
  SCREENROOM_TIZEN_SDK           Tizen SDK root
  SCREENROOM_SDB                 sdb binary

SCREENROOM_SHIP_STRICT=1 additionally fails on a lane that ran and failed; the
CI job uses it so a broken device deployment is not reported as green.
TEXT
}

load_devices() {
  [ -f "$config_file" ] || return 0
  local line key value
  while IFS= read -r line || [ -n "$line" ]; do
    case "$line" in
      ''|'#'*) continue ;;
    esac
    key="${line%%=*}"
    value="${line#*=}"
    [ "$key" = "$line" ] && continue
    case "$key" in
      SCREENROOM_*) ;;
      *) continue ;;
    esac
    export "$key=$value"
  done <"$config_file"
}

resolve_binary() {
  local override="$1" fallback="$2"
  if [ -n "$override" ] && [ -x "$override" ]; then
    printf '%s\n' "$override"
    return 0
  fi
  if command -v "$(basename "$fallback")" >/dev/null 2>&1; then
    command -v "$(basename "$fallback")"
    return 0
  fi
  if [ -x "$fallback" ]; then
    printf '%s\n' "$fallback"
    return 0
  fi
  return 1
}

tizen_sdk_root() {
  printf '%s\n' "${SCREENROOM_TIZEN_SDK:-$HOME/.local/share/screenroom-toolchains/tizen}"
}

resolve_sdb() {
  resolve_binary "${SCREENROOM_SDB:-}" "$(tizen_sdk_root)/tools/sdb"
}

resolve_tizen() {
  resolve_binary "${SCREENROOM_TIZEN_CLI:-}" "$(tizen_sdk_root)/tools/ide/bin/tizen"
}

stop_running_app() {
  pgrep -x Screenroom >/dev/null 2>&1 || return 0
  osascript -e 'quit app "Screenroom"' >/dev/null 2>&1 || true
  local waited=0
  while pgrep -x Screenroom >/dev/null 2>&1 && [ "$waited" -lt 20 ]; do
    sleep 0.5
    waited=$((waited + 1))
  done
  return 0
}

lane_macos() {
  local destination="${SCREENROOM_MACOS_INSTALL_DIR:-$HOME/Applications}"
  local app="$destination/Screenroom.app"
  stop_running_app
  "$root/scripts/release.sh" macos --install "$destination" --no-archive
  if [ "${relaunch}" = "1" ]; then
    open "$app"
    printf 'installed %s and relaunched it\n' "$app"
  else
    printf 'installed %s without relaunching it\n' "$app"
  fi
}

lane_windows() {
  local host="${SCREENROOM_WINDOWS_HOST:-}"
  local repository="${SCREENROOM_WINDOWS_REPO:-screenroom}"
  if [ -z "$host" ]; then
    printf 'no Windows host configured, skipped\n'
    return 0
  fi
  if ! ssh -o BatchMode=yes -o ConnectTimeout=5 -o StrictHostKeyChecking=accept-new "$host" 'exit 0' >/dev/null 2>&1; then
    printf '%s is unreachable, skipped\n' "$host"
    return 0
  fi
  local quoted
  quoted="$(printf '%q' "${repository//\\//}")"
  ssh -o BatchMode=yes -o ConnectTimeout=10 -o StrictHostKeyChecking=accept-new "$host" \
    "powershell -NoProfile -ExecutionPolicy Bypass -Command \"cd $quoted; cargo build --release -p screenroom-desktop\""
  printf 'rebuilt the release binary on %s\n' "$host"
}

tv_target() {
  local sdb="$1"
  local address="${SCREENROOM_TV_ADDRESS:-}"
  if [ -n "$address" ]; then
    "$sdb" connect "$address" >/dev/null 2>&1 || true
  fi
  "$sdb" devices 2>/dev/null | awk 'NR > 1 && $2 == "device" { print $1; exit }'
}

lane_tv() {
  local sdb tizen profile target package_id remote install_output wgt
  if ! sdb="$(resolve_sdb)"; then
    printf 'sdb was not found, TV skipped\n'
    return 0
  fi
  target="$(tv_target "$sdb")"
  if [ -z "$target" ]; then
    printf 'no TV answered on sdb, skipped\n'
    return 0
  fi
  if ! tizen="$(resolve_tizen)"; then
    printf 'the Tizen CLI was not found, TV skipped\n'
    return 0
  fi
  profile="${SCREENROOM_TIZEN_PROFILE:-screenroom}"
  (
    cd "$root/apps/tv"
    pnpm install --frozen-lockfile
    pnpm build
  )
  rm -f "$root/apps/tv/build/"*.wgt
  "$tizen" package -t wgt -s "$profile" -- "$root/apps/tv/build/widget" -o "$root/apps/tv/build"
  wgt="$(find "$root/apps/tv/build" -maxdepth 1 -name '*.wgt' -type f 2>/dev/null | head -1 || true)"
  if [ -z "$wgt" ]; then
    printf 'the Tizen CLI produced no widget package\n'
    return 1
  fi
  package_id="$(sed -n 's/.*<tizen:application id="\([^"]*\)".*/\1/p' "$root/apps/tv/config.xml" | head -1)"
  if [ -z "$package_id" ]; then
    printf 'config.xml does not declare an application id\n'
    return 1
  fi
  remote="/home/owner/share/tmp/sdk_tools/$(basename "$wgt")"
  "$sdb" -s "$target" push "$wgt" "$remote" >/dev/null
  install_output="$("$sdb" -s "$target" shell 0 vd_appinstall "$package_id" "$remote" 2>&1 || true)"
  case "$install_output" in
    *"install completed"*) ;;
    *)
      printf 'the TV refused the install: %s\n' "$install_output"
      return 1
      ;;
  esac
  "$sdb" -s "$target" shell 0 was_execute "$package_id" >/dev/null 2>&1 || true
  printf 'installed %s on %s and launched it\n' "$(basename "$wgt")" "$target"
}

launch_lane() {
  local name="$1"
  shift
  launched_names+=("$name")
  ( "$@" >"$work_dir/$name.log" 2>&1; printf '%s\n' "$?" >"$work_dir/$name.status" ) &
  printf '%s\n' "$!" >"$work_dir/$name.pid"
}

wait_lane() {
  local name="$1" pid
  pid="$(cat "$work_dir/$name.pid" 2>/dev/null || true)"
  [ -n "$pid" ] || return 0
  wait "$pid" 2>/dev/null || true
}

lane_result() {
  local name="$1" status
  status="$(cat "$work_dir/$name.status" 2>/dev/null || printf '1')"
  if [ "$status" = "0" ]; then
    printf 'ok'
  else
    printf 'failed'
  fi
}

lane_note() {
  local note
  note="$(grep -v '^[[:space:]]*$' "$work_dir/$name.log" 2>/dev/null | tail -1 || true)"
  if [ -z "$note" ]; then
    note="no output"
  fi
  printf '%s' "$note"
}

print_table() {
  printf '\n%-8s  %-7s  %s\n' "LANE" "RESULT" "NOTE"
  printf '%-8s  %-7s  %s\n' "------" "------" "----"
  local name
  for name in macos windows tv; do
    case " ${launched_names[*]} " in
      *" $name "*) printf '%-8s  %-7s  %s\n' "$name" "$(lane_result "$name")" "$(lane_note "$name")" ;;
      *) printf '%-8s  %-7s  %s\n' "$name" "not run" "" ;;
    esac
  done
}

print_failure_logs() {
  local name
  for name in "${launched_names[@]}"; do
    if [ "$(lane_result "$name")" = "failed" ]; then
      printf '\n%s log\n' "$name"
      tail -40 "$work_dir/$name.log" 2>/dev/null || true
    fi
  done
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --only)
      if [ "$#" -lt 2 ] || [ -z "$2" ]; then
        printf '%s\n' "--only needs a lane: macos, windows or tv" >&2
        exit 2
      fi
      requested="$2"
      only_given=1
      shift 2
      ;;
    --only=*)
      requested="${1#*=}"
      only_given=1
      shift
      ;;
    --no-relaunch)
      relaunch=0
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      printf 'unknown argument: %s\n' "$1" >&2
      usage
      exit 2
      ;;
  esac
done

load_devices

if [ "$only_given" = "1" ] && [ -z "$requested" ]; then
  printf '%s\n' "--only needs a lane: macos, windows or tv" >&2
  exit 2
fi

include_macos=0
include_windows=0
include_tv=0

case "$requested" in
  ''|all)
    include_macos=1
    include_windows=1
    include_tv=1
    ;;
  macos)
    include_macos=1
    ;;
  windows)
    include_windows=1
    ;;
  tv)
    include_tv=1
    ;;
  *)
    printf 'unknown lane: %s\n' "$requested" >&2
    exit 2
    ;;
esac

if [ "$include_macos" = "1" ]; then
  launch_lane macos lane_macos
fi
if [ "$include_windows" = "1" ]; then
  launch_lane windows lane_windows
fi
if [ "$include_tv" = "1" ]; then
  launch_lane tv lane_tv
fi

for lapped in "${launched_names[@]}"; do
  wait_lane "$lapped"
done

print_table
print_failure_logs

if [ "$include_macos" = "1" ] && [ "$(lane_result macos)" = "failed" ]; then
  exit 1
fi

if [ "${SCREENROOM_SHIP_STRICT:-0}" = "1" ]; then
  for lapped in "${launched_names[@]}"; do
    if [ "$(lane_result "$lapped")" = "failed" ]; then
      exit 1
    fi
  done
fi

exit 0
