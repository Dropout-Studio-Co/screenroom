#!/usr/bin/env bash

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
cd "$root"

version=""
bump="patch"
publish=1
push_first=0
only_if_changed=0

usage() {
  cat >&2 <<'TEXT'
usage: scripts/publish-update.sh [--version X | --patch | --minor] [--dry-run]

Builds the current tree, bumps the version, publishes a GitHub release with the
application, its signed appcast and the media source, and reinstalls this Mac's
copy at the new version. The installed application then offers the update itself.

  --version X   publish exactly X
  --patch       bump the last number (default)
  --minor       bump the middle number
  --dry-run     build and sign everything, print the result, publish nothing
  --push        push the current commit first when it is ahead of its upstream
  --if-changed  publish only when the application changed since the last release

The last two are what the project rule uses after every commit, so an agent can run
this unconditionally without creating empty or duplicate releases.
TEXT
}

fail() {
  printf '%s\n' "$*" >&2
  exit 1
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --version)
      [ -n "${2:-}" ] || fail "--version needs a version"
      version="$2"
      shift 2
      ;;
    --patch)
      bump="patch"
      shift
      ;;
    --minor)
      bump="minor"
      shift
      ;;
    --dry-run)
      publish=0
      shift
      ;;
    --push)
      push_first=1
      shift
      ;;
    --if-changed)
      only_if_changed=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      usage
      exit 2
      ;;
  esac
done

if [ "$(uname -s)" != "Darwin" ]; then
  fail "publishing a macOS build happens on macOS"
fi

[ "${SCREENROOM_ALLOW_DIRTY_RELEASE:-0}" = "1" ] || {
  [ -z "$(git status --porcelain)" ] || fail "the tree has uncommitted changes; commit or stash them first"
}

current="$(sed -n '/^\[workspace\.package\]/,/^\[/s/^version *= *"\([^"]*\)" *$/\1/p' Cargo.toml | head -1)"
[ -n "$current" ] || fail "could not read the workspace version"

if [ -z "$version" ]; then
  major="${current%%.*}"
  rest="${current#*.}"
  middle="${rest%%.*}"
  last="${rest##*.}"
  case "$bump" in
    patch) version="$major.$middle.$((last + 1))" ;;
    minor) version="$major.$((middle + 1)).0" ;;
  esac
fi

printf '==> %s -> %s\n' "$current" "$version"

if [ "$push_first" = "1" ] && [ "$publish" = "1" ]; then
  if [ -n "$(git rev-list '@{u}..HEAD' 2>/dev/null)" ]; then
    git push origin HEAD >/dev/null 2>&1 || fail "the current commit could not be pushed"
    printf '==> pushed %s\n' "$(git rev-parse --short HEAD)"
  fi
fi

if [ "$only_if_changed" = "1" ] && [ "$publish" = "1" ]; then
  last_release="$(gh release list --limit 1 --json tagName --jq '.[0].tagName // ""' 2>/dev/null || true)"
  if [ -n "$last_release" ] && git rev-parse -q --verify "$last_release" >/dev/null 2>&1; then
    if git diff --quiet "$last_release" HEAD -- apps crates Cargo.toml Cargo.lock assets; then
      printf 'nothing in the application changed since %s; no release\n' "$last_release"
      exit 0
    fi
  fi
fi

if [ "$publish" = "1" ]; then
  git fetch --quiet origin
  upstream="$(git rev-parse --abbrev-ref --symbolic-full-name '@{u}' 2>/dev/null || true)"
  if [ -n "$upstream" ]; then
    [ "$(git rev-parse HEAD)" = "$(git rev-parse "$upstream")" ] \
      || fail "HEAD is not level with $upstream; push or pull first"
  fi
  gh release view "v$version" >/dev/null 2>&1 \
    && fail "release v$version already exists; never reuse a version"
fi

python3 - "$version" <<'PYTHON'
import re
import sys

version = sys.argv[1]
path = "Cargo.toml"
with open(path, encoding="utf-8") as handle:
    text = handle.read()
text = re.sub(
    r'(\[workspace\.package\][\s\S]*?^version = ")[^"]*(")',
    lambda match: match.group(1) + version + match.group(2),
    text,
    count=1,
    flags=re.M,
)
with open(path, "w", encoding="utf-8") as handle:
    handle.write(text)
PYTHON

cargo build --quiet -p screenroom-desktop >/dev/null 2>&1 || fail "the tree does not build at $version"

if ! git diff --quiet -- Cargo.toml Cargo.lock; then
  git add Cargo.toml Cargo.lock
  git -c core.hooksPath=/dev/null commit -q -m "chore(release): $version"
  printf '==> committed %s\n' "$version"
fi

for _ in $(seq 1 180); do
  if scripts/rebuild-app.sh --status 2>/dev/null | grep -q "lock: *free"; then
    break
  fi
  sleep 1
done

if [ ! -f vendor/lgpl/frameworks/libmpv.2.dylib ]; then
  scripts/build-lgpl-media.sh >/dev/null 2>&1 || fail "the LGPL media stack could not be built"
fi

release_dir="dist/release"
rm -rf "$release_dir"
./scripts/release.sh macos --out "$release_dir" >/dev/null 2>&1 || fail "packaging failed"

key="$(mktemp -d "${TMPDIR:-/tmp}/screenroom-ed.XXXXXX")/ed25519.key"
cleanup() { rm -f "$key"; }
trap cleanup EXIT

vendor/sparkle/bin/generate_keys --account "${SCREENROOM_SPARKLE_ACCOUNT:-screenroom}" -x "$key" >/dev/null 2>&1 \
  || fail "the update signing key is not in the keychain account ${SCREENROOM_SPARKLE_ACCOUNT:-screenroom}"

SCREENROOM_SPARKLE_ED_KEY="$(cat "$key")" \
  ./scripts/release.sh appcast --out "$release_dir" --tag "v$version" >/dev/null 2>&1 \
  || fail "the appcast could not be signed"

./scripts/release.sh manifest --out "$release_dir" >/dev/null 2>&1 || true

staging="$(mktemp -d "${TMPDIR:-/tmp}/screenroom-source.XXXXXX")"
mkdir -p "$staging/downloads"
cp vendor/lgpl/BUILD_INFO "$staging/"
cp vendor/lgpl/COMPONENTS.txt "$staging/" 2>/dev/null || true
cp vendor/lgpl/downloads/ffmpeg.tar.xz vendor/lgpl/downloads/mpv.tar.gz vendor/lgpl/downloads/libplacebo.tar.gz "$staging/downloads/"
cp scripts/build-lgpl-media.sh "$staging/"
cp vendor/lgpl/mpv-lgpl-build.patch "$staging/" 2>/dev/null || true
cp docs/LGPL_SOURCE_OFFER.md "$staging/"
tar -cJf "$release_dir/Screenroom-media-source.tar.xz" -C "$staging" .
rm -rf "$staging"

zip="$(ls "$release_dir"/Screenroom-"$version"-macos-*.zip 2>/dev/null | head -1)"
[ -n "$zip" ] || fail "the archive is missing from $release_dir"

if [ "$publish" = "0" ]; then
  printf '==> dry run: nothing was published\n'
  ls -lh "$release_dir"
  exit 0
fi

if [ "$(git rev-parse HEAD)" != "$(git rev-parse "@{u}" 2>/dev/null || git rev-parse HEAD)" ]; then
  git push origin HEAD >/dev/null 2>&1 || fail "the version commit could not be pushed"
fi

notes="Screenroom $version."
gh release create "v$version" \
  "$zip" \
  "$release_dir/appcast.xml" \
  "$release_dir/manifest.json" \
  "$release_dir/Screenroom-media-source.tar.xz" \
  --title "Screenroom $version" \
  --notes "$notes" \
  --target "$(git rev-parse HEAD)" >/dev/null \
  || fail "the release could not be published"

printf '==> published v%s\n' "$version"

scripts/rebuild-app.sh --force >/dev/null 2>&1 || fail "the local app could not be reinstalled"

printf '==> this Mac now runs %s at %s\n' "$version" "$HOME/Applications/Screenroom.app"
printf '    the next launch looks for a newer version by itself\n'
