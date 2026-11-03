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
stamp="$common/screenroom-installed.sha"
state="$root/.artifacts/builds"
last="$state/last.json"
install_dir="${SCREENROOM_INSTALL_DIR:-$HOME/Applications}"
installed="$install_dir/Screenroom.app"
dist="$root/dist/Screenroom.app"

force=0
for argument in "$@"; do
  case "$argument" in
    --background) ;;
    --force) force=1 ;;
    --status)
      printf 'installed: %s\n' "$(cat "$stamp" 2>/dev/null || echo none)"
      printf 'head:      %s\n' "$(git rev-parse HEAD 2>/dev/null || echo none)"
      [ -f "$last" ] && cat "$last"
      exit 0
      ;;
    *)
      echo "rebuild-app: unknown argument $argument" >&2
      exit 2
      ;;
  esac
done

mkdir -p "$state"

acquire() {
  if mkdir "$lock" 2>/dev/null; then
    echo $$ >"$lock/pid"
    return 0
  fi
  holder="$(cat "$lock/pid" 2>/dev/null || echo "")"
  if [ -n "$holder" ] && kill -0 "$holder" 2>/dev/null; then
    return 1
  fi
  rm -rf "$lock"
  if mkdir "$lock" 2>/dev/null; then
    echo $$ >"$lock/pid"
    return 0
  fi
  return 1
}

release() {
  rm -rf "$lock"
}

if ! acquire; then
  : >"$lock/pending"
  echo "rebuild-app: a build is already running; queued this state behind it"
  exit 0
fi

trap release EXIT
trap 'release; exit 130' INT TERM

head_sha="$(git rev-parse HEAD)"
installed_sha="$(cat "$stamp" 2>/dev/null || true)"

relevant_since() {
  local base="$1" names
  if [ -n "$base" ] && git cat-file -e "$base^{commit}" 2>/dev/null; then
    names="$(git diff --name-only "$base" HEAD)"
  elif git rev-parse HEAD^ >/dev/null 2>&1; then
    names="$(git diff --name-only HEAD^ HEAD)"
  else
    names="$(git ls-tree -r --name-only HEAD)"
  fi
  printf '%s\n' "$names" | grep -qE '^(apps/|crates/|assets/|scripts/|vendor/|Cargo\.toml|Cargo\.lock|rust-toolchain|\.cargo/)' || return 1
}

record() {
  local sha="$1" result="$2" detail="$3"
  printf '{"sha":"%s","result":"%s","detail":"%s","finished":"%s","head":"%s"}\n' \
    "$sha" "$result" "$detail" "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$(git rev-parse HEAD)" >"$last"
}

mark() {
  record "$1" "$2" "$3"
  printf '%s\n' "$1" >"$stamp"
}

attempts=0
while [ "$attempts" -lt 3 ]; do
  attempts=$((attempts + 1))
  head_sha="$(git rev-parse HEAD)"

  if [ "$force" -eq 0 ] && [ "$head_sha" = "$installed_sha" ]; then
    echo "rebuild-app: $head_sha is already the installed build"
    break
  fi

  if [ "$force" -eq 0 ] && [ -n "$installed_sha" ] && ! relevant_since "$installed_sha"; then
    echo "rebuild-app: no app-affecting change since $installed_sha; recording $head_sha without a rebuild"
    mark "$head_sha" skipped "no app-affecting change"
    break
  fi

  log="$state/$(date -u +%Y%m%dT%H%M%SZ)-${head_sha:0:12}.log"
  echo "rebuild-app: building $head_sha (log $log)"
  started="$(date +%s)"

  if ! scripts/package-macos.sh >"$log" 2>&1; then
    record "$installed_sha" failed "package failed; the installed app was left alone"
    echo "rebuild-app: the build failed; $installed was not touched. See $log" >&2
    tail -20 "$log" >&2
    exit 1
  fi

  if ! scripts/verify-bundle.sh "$dist" >>"$log" 2>&1; then
    record "$installed_sha" failed "verify failed; the installed app was left alone"
    echo "rebuild-app: the bundle did not verify; $installed was not touched. See $log" >&2
    tail -20 "$log" >&2
    exit 1
  fi

  mkdir -p "$install_dir"
  staged="$install_dir/.Screenroom.app.staged.$$"
  previous="$install_dir/.Screenroom.app.previous.$$"
  rm -rf "$staged" "$previous"

  if ! ditto "$dist" "$staged"; then
    rm -rf "$staged"
    record "$installed_sha" failed "staging failed; the installed app was left alone"
    echo "rebuild-app: could not stage the new build; $installed was not touched" >&2
    exit 1
  fi

  if [ -d "$installed" ] && ! mv "$installed" "$previous"; then
    rm -rf "$staged"
    record "$installed_sha" failed "could not move the previous build aside"
    echo "rebuild-app: could not move the previous build aside; $installed was not touched" >&2
    exit 1
  fi

  if ! mv "$staged" "$installed"; then
    if [ -d "$previous" ]; then
      mv "$previous" "$installed" || true
    fi
    rm -rf "$staged"
    record "$installed_sha" failed "install failed and was rolled back"
    echo "rebuild-app: install failed; the previous build was restored" >&2
    exit 1
  fi

  rm -rf "$previous"
  chmod +x "$installed/Contents/MacOS/Screenroom" 2>/dev/null || true
  xattr -dr com.apple.quarantine "$installed" 2>/dev/null || true

  took="$(( $(date +%s) - started ))"
  mark "$head_sha" built "$(printf 'installed in %ss' "$took")"
  echo "rebuild-app: installed $head_sha at $installed in ${took}s"

  if [ -f "$lock/pending" ]; then
    rm -f "$lock/pending"
    if [ "$(git rev-parse HEAD)" != "$head_sha" ]; then
      echo "rebuild-app: a newer commit arrived while building; building it too"
      continue
    fi
  fi
  break
done

if [ -n "$(git status --porcelain)" ]; then
  echo "rebuild-app: note: working tree has uncommitted changes; the build reflects committed HEAD"
fi

exit 0
