#!/usr/bin/env bash

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
version="2.10.1"
digest="1a856d2a9edb8e628de88121d7be04cf0e2167f8641650b78fca00b14244cf27"
url="https://github.com/sparkle-project/Sparkle/releases/download/${version}/Sparkle-${version}.tar.xz"
destination="${SCREENROOM_SPARKLE_DIR:-$root/vendor/sparkle}"
force="${1:-}"

fail() {
  printf '%s\n' "$*" >&2
  exit 1
}

if [ "$(uname -s)" != "Darwin" ]; then
  fail "Sparkle is only fetched on macOS; $(uname -s) has no use for it"
fi

if [ -d "$destination/Sparkle.framework" ] && [ "$force" != "--force" ]; then
  recorded="$(cat "$destination/VERSION" 2>/dev/null || true)"
  if [ "$recorded" = "$version" ]; then
    printf 'sparkle %s already present at %s\n' "$version" "$destination"
    exit 0
  fi
fi

work="$(mktemp -d "${TMPDIR:-/tmp}/screenroom-sparkle.XXXXXX")"
cleanup() { rm -rf "$work"; }
trap cleanup EXIT

printf 'fetching sparkle %s\n' "$version"
curl -fsSL --retry 5 --retry-all-errors --retry-delay 5 --proto '=https' --tlsv1.2 -o "$work/Sparkle.tar.xz" "$url"
printf '%s  %s\n' "$digest" "$work/Sparkle.tar.xz" | shasum -a 256 -c - >/dev/null \
  || fail "the Sparkle archive does not match its pinned sha256; nothing was installed"

mkdir -p "$work/extracted"
tar -xf "$work/Sparkle.tar.xz" -C "$work/extracted"
[ -d "$work/extracted/Sparkle.framework" ] || fail "the archive has no Sparkle.framework"
for tool in sign_update generate_appcast generate_keys; do
  [ -x "$work/extracted/bin/$tool" ] || fail "the archive has no bin/$tool"
done

rm -rf "$destination"
mkdir -p "$destination"
ditto "$work/extracted/Sparkle.framework" "$destination/Sparkle.framework"
ditto "$work/extracted/bin" "$destination/bin"
printf '%s\n' "$version" >"$destination/VERSION"
cp "$work/extracted/LICENSE" "$destination/LICENSE"

printf 'sparkle %s installed at %s\n' "$version" "$destination"
printf 'public tools: sign_update, generate_appcast, generate_keys\n'
