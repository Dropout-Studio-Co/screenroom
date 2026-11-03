#!/bin/bash

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

if [ "$(uname -s)" != "Darwin" ]; then
  echo "rebuild-app: this automation is macOS only; $(uname -s) does nothing" >&2
  exit 0
fi

common="$(git rev-parse --git-common-dir)"
common="$(cd "$common" && pwd)"
lock="$common/screenroom-build.lock"
pending="$common/screenroom-build.pending"
stamp="$common/screenroom-installed.sha"
last="$common/screenroom-build-last.json"
state="$root/.artifacts/builds"
install_dir="${SCREENROOM_INSTALL_DIR:-$HOME/Applications}"
builds="$install_dir/.screenroom-builds"
installed="$install_dir/Screenroom.app"
dist="$root/dist/Screenroom.app"
relevant='^(apps/|crates/|assets/|scripts/|vendor/|Cargo\.toml|Cargo\.lock|rust-toolchain|rust-toolchain\.toml|\.cargo/)'

force=0
unlock=0

holder_pid() {
  cut -d'|' -f1 "$lock" 2>/dev/null || true
}

holder_start() {
  cut -d'|' -f2- "$lock" 2>/dev/null || true
}

live_holder() {
  local pid start current
  pid="$(holder_pid)"
  start="$(holder_start)"
  [ -n "$pid" ] || return 1
  current="$(ps -o lstart= -p "$pid" 2>/dev/null | tr -s ' ' | sed 's/^ //;s/ $//')"
  [ -n "$current" ] || return 1
  [ "$current" = "$start" ]
}

status() {
  local app_state lock_state pending_state
  if [ -d "$installed" ]; then app_state="present"; else app_state="missing"; fi
  if [ -f "$lock" ]; then
    if live_holder; then lock_state="held by $(holder_pid)"; else lock_state="stale (run --unlock)"; fi
  else
    lock_state="free"
  fi
  if [ -f "$pending" ]; then
    pending_state="queued $(sed -n '1p' "$pending" 2>/dev/null)@$(sed -n '2p' "$pending" 2>/dev/null)"
  else
    pending_state="none"
  fi
  printf 'installed: %s\n' "$(cat "$stamp" 2>/dev/null || echo none)"
  printf 'head:      %s\n' "$(git rev-parse HEAD 2>/dev/null || echo none)"
  if [ -L "$installed" ]; then
    printf 'app:       %s at %s -> %s\n' "$app_state" "$installed" "$(readlink "$installed")"
  else
    printf 'app:       %s at %s\n' "$app_state" "$installed"
  fi
  printf 'lock:      %s\n' "$lock_state"
  printf 'pending:   %s\n' "$pending_state"
  [ -f "$last" ] && cat "$last"
  return 0
}

for argument in "$@"; do
  case "$argument" in
    --background) ;;
    --force) force=1 ;;
    --unlock)
      unlock=1
      ;;
    --status)
      status
      exit 0
      ;;
    *)
      echo "rebuild-app: unknown argument $argument" >&2
      exit 2
      ;;
  esac
done

mkdir -p "$state"

if [ "$unlock" -eq 1 ]; then
  rm -f "$lock" "$pending"
  echo "rebuild-app: lock and pending marker cleared"
  exit 0
fi

my_start="$(ps -o lstart= -p $$ 2>/dev/null | tr -s ' ' | sed 's/^ //;s/ $//')"

claim() {
  local staging
  staging="$(mktemp "$common/.screenroom-lock.XXXXXX")" || return 1
  if ! printf '%s|%s\n' "$$" "$my_start" >"$staging"; then
    rm -f "$staging"
    return 1
  fi
  if ln "$staging" "$lock" 2>/dev/null; then
    rm -f "$staging"
    return 0
  fi
  rm -f "$staging"
  return 1
}

acquire() {
  local attempt
  attempt=0
  while [ "$attempt" -lt 3 ]; do
    attempt=$((attempt + 1))
    if claim; then
      return 0
    fi
    if live_holder; then
      return 1
    fi
    sleep 1
  done
  claim
}

release() {
  if [ "$(holder_pid)" = "$$" ]; then
    rm -f "$lock"
  fi
}

record() {
  printf '{"sha":"%s","result":"%s","detail":"%s","finished":"%s","head":"%s","dirty":%s}\n' \
    "$1" "$2" "$3" "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$(git rev-parse HEAD)" "${4:-false}" >"$last"
}

if [ "$force" -eq 1 ] && [ -f "$lock" ] && ! live_holder; then
  rm -f "$lock"
fi

if ! acquire; then
  printf '%s\n%s\n' "$root" "$(git rev-parse HEAD)" >"$pending"
  echo "rebuild-app: a build is already running; queued this state behind it"
  waited=0
  while [ "$waited" -lt 60 ]; do
    sleep 1
    waited=$((waited + 1))
    if acquire; then
      rm -f "$pending"
      echo "rebuild-app: the previous build finished; building this checkout"
      break
    fi
    if [ "$waited" -eq 60 ]; then
      exit 0
    fi
  done
fi

trap release EXIT
trap 'release; exit 130' INT TERM

exchange() {
  python3 - "$1" "$2" <<'PY'
import ctypes
import sys

libc = ctypes.CDLL(None, use_errno=True)
swap = libc.renamex_np
swap.argtypes = [ctypes.c_char_p, ctypes.c_char_p, ctypes.c_uint]
swap.restype = ctypes.c_int
RENAME_SWAP = 0x2

if swap(sys.argv[1].encode(), sys.argv[2].encode(), RENAME_SWAP) != 0:
    sys.exit(ctypes.get_errno() or 1)
PY
}

recover() {
  local candidate newest
  for candidate in "$install_dir"/.Screenroom.app.previous.* "$install_dir"/.Screenroom.app.staged.*; do
    [ -e "$candidate" ] || continue
    [ -e "$installed" ] && rm -rf "$candidate"
  done
  if [ -L "$installed" ]; then
    candidate="$(readlink "$installed")"
    rm -f "$installed"
    if [ -d "$candidate" ]; then
      mv "$candidate" "$installed" 2>/dev/null || true
      echo "rebuild-app: replaced a symlinked install with a real bundle"
    fi
  fi
  if [ ! -e "$installed" ]; then
    newest="$(ls -dt "$builds"/*.app 2>/dev/null | head -1 || true)"
    if [ -n "$newest" ] && mv "$newest" "$installed" 2>/dev/null; then
      echo "rebuild-app: restored $installed from the last build"
    fi
  fi
}

recover

tree_is_clean() {
  git diff --quiet HEAD -- apps crates assets scripts vendor Cargo.toml Cargo.lock rust-toolchain rust-toolchain.toml .cargo 2>/dev/null
}

relevant_since() {
  local base="$1" names
  if [ -n "$base" ] && git cat-file -e "$base^{commit}" 2>/dev/null; then
    names="$(git diff --name-only "$base" HEAD)"
  elif [ -n "$base" ]; then
    return 0
  elif git rev-parse HEAD^ >/dev/null 2>&1; then
    names="$(git diff --name-only HEAD^ HEAD)"
  else
    names="$(git ls-tree -r --name-only HEAD)"
  fi
  grep -qE "$relevant" <<<"$names"
}

stamp_state() {
  printf '%s\n' "$1"
}

rounds=0
while [ "$rounds" -lt 20 ]; do
  rounds=$((rounds + 1))
  head_sha="$(git rev-parse HEAD)"
  installed_sha="$(cat "$stamp" 2>/dev/null || true)"
  dirty="false"
  if ! tree_is_clean; then dirty="true"; fi

  if [ "$force" -eq 0 ] && [ "$head_sha" = "$installed_sha" ] && [ "$dirty" = "false" ]; then
    echo "rebuild-app: $head_sha is already the installed build"
    break
  fi

  if [ "$force" -eq 0 ] && [ "$dirty" = "false" ] && [ -n "$installed_sha" ] && ! relevant_since "$installed_sha"; then
    echo "rebuild-app: no app-affecting change since $installed_sha; recording $head_sha without a rebuild"
    record "$head_sha" skipped "no app-affecting change" false
    printf '%s\n' "$head_sha" >"$stamp"
    if [ -f "$pending" ] && [ "$(git rev-parse HEAD)" != "$head_sha" ]; then
      rm -f "$pending"
      continue
    fi
    break
  fi

  log="$state/$(date -u +%Y%m%dT%H%M%SZ)-${head_sha:0:12}.log"
  echo "rebuild-app: building $head_sha (log $log)"
  started="$(date +%s)"

  failed=""
  if ! scripts/package-macos.sh >"$log" 2>&1; then
    failed="package failed"
  elif ! scripts/verify-bundle.sh "$dist" >>"$log" 2>&1; then
    failed="verify failed"
  fi

  if [ -z "$failed" ]; then
    mkdir -p "$builds"
    versioned="$builds/Screenroom-${head_sha:0:12}-$(date +%s).app"
    if ! ditto "$dist" "$versioned"; then
      rm -rf "$versioned"
      failed="staging failed"
    fi
  fi

  installed_ok=""
  if [ -z "$failed" ] && [ -d "$installed" ] && [ ! -L "$installed" ]; then
    if exchange "$installed" "$versioned"; then
      mv "$versioned" "$builds/Screenroom-previous-$(date +%s).app" 2>/dev/null || rm -rf "$versioned"
      installed_ok="1"
    fi
  elif [ -z "$failed" ]; then
    rm -f "$installed"
    if mv "$versioned" "$installed" 2>/dev/null; then
      installed_ok="1"
    fi
  fi

  if [ -z "$failed" ] && [ -z "$installed_ok" ]; then
    rm -rf "$versioned"
    failed="install failed; the previous build is still in place"
  fi

  if [ -z "$failed" ]; then
    chmod +x "$installed/Contents/MacOS/Screenroom" 2>/dev/null || true
    xattr -dr com.apple.quarantine "$installed" 2>/dev/null || true
    took="$(( $(date +%s) - started ))"
    record "$head_sha" built "installed in ${took}s" "$dirty"
    printf '%s\n' "$head_sha" >"$stamp"
    echo "rebuild-app: installed $head_sha at $installed in ${took}s"
    ls -dt "$builds"/*.app 2>/dev/null | tail -n +3 | while IFS= read -r old; do
      rm -rf "$old"
    done
  fi

  if [ -n "$failed" ]; then
    record "$(cat "$stamp" 2>/dev/null || true)" failed "$failed" "$dirty"
    echo "rebuild-app: $failed; $installed was left alone. See $log" >&2
    tail -20 "$log" >&2 || true
    if [ -f "$pending" ] && [ "$(git rev-parse HEAD)" != "$head_sha" ]; then
      rm -f "$pending"
      continue
    fi
    exit 1
  fi

  if [ -f "$pending" ] && [ "$(git rev-parse HEAD)" != "$head_sha" ]; then
    rm -f "$pending"
    continue
  fi
  break
done

if [ -f "$pending" ] && [ "$(cat "$stamp" 2>/dev/null || true)" != "$(git rev-parse HEAD)" ]; then
  queued_root="$(sed -n '1p' "$pending" 2>/dev/null || true)"
  rm -f "$pending"
  if [ -n "$queued_root" ] && [ "$queued_root" != "$root" ] && [ -x "$queued_root/scripts/rebuild-app.sh" ]; then
    echo "rebuild-app: handing the queue to $queued_root"
    ( cd "$queued_root" && nohup scripts/rebuild-app.sh --background >>"$state/hooks.log" 2>&1 & )
  else
    echo "rebuild-app: commits are still arriving; continuing in a fresh worker"
    nohup "$0" --background >>"$state/hooks.log" 2>&1 &
  fi
fi

if [ "$dirty" = "true" ]; then
  echo "rebuild-app: note: the working tree has uncommitted app changes, so the installed build includes them"
fi

exit 0
