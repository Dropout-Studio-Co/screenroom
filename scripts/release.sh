#!/usr/bin/env bash

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"

out_dir="$root/dist/release"
install_dir=""
app_path="$root/dist/Screenroom.app"
tag=""
archive=1

usage() {
  cat >&2 <<'TEXT'
usage: scripts/release.sh macos [--out DIR] [--install DIR] [--no-archive]
       scripts/release.sh install [--app PATH] [--install DIR]
       scripts/release.sh manifest [--out DIR]
       scripts/release.sh appcast [--out DIR] [--tag TAG]
       scripts/release.sh version

macos     build dist/Screenroom.app, archive it and write its manifest fragment
install   replace the installed copy with --app, without launching it
manifest  merge manifest-*.json into manifest.json, signing it when a key is set
version   print the version this release carries

The archive lands in DIR (default dist/release). The manifest fragment is
DIR/manifest-macos-<arch>.json and the merged document is DIR/manifest.json.
Signing uses SCREENROOM_SIGNING_KEY_FILE with ssh-keygen in the screenroom
namespace; with no key configured the manifest is published unsigned.

appcast writes DIR/appcast.xml from the archives in DIR and signs it with the
Sparkle Ed25519 key, read from SCREENROOM_SPARKLE_ED_KEY or the file named by
SCREENROOM_SPARKLE_KEY_FILE. It refuses to write an unsigned feed.
TEXT
}

fail() {
  printf '%s\n' "$*" >&2
  exit 1
}

resolve_version() {
  local declared
  declared="$(sed -n 's/^version *= *"\([^"]*\)" *$/\1/p' "$root/apps/desktop/Cargo.toml" | head -1 || true)"
  if [ -n "$declared" ]; then
    printf '%s\n' "$declared"
    return 0
  fi
  sed -n '/^\[workspace\.package\]/,/^\[/s/^version *= *"\([^"]*\)" *$/\1/p' "$root/Cargo.toml" | head -1
}

host_arch() {
  case "$(uname -m)" in
    arm64) printf 'arm64\n' ;;
    x86_64) printf 'x64\n' ;;
    *) uname -m ;;
  esac
}

file_size() {
  case "$(uname -s)" in
    Darwin) stat -f%z "$1" ;;
    *) stat -c%s "$1" ;;
  esac
}

sha256_of() {
  if command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{print $1}'
  else
    sha256sum "$1" | awk '{print $1}'
  fi
}

write_fragment() {
  local target="$1" platform="$2" arch="$3" filename="$4" digest="$5" bytes="$6" version="$7"
  python3 - "$target" "$platform" "$arch" "$filename" "$digest" "$bytes" "$version" <<'PYTHON'
import json
import sys

target, platform, arch, filename, digest, size, version = sys.argv[1:8]
document = {
    "artifacts": [
        {
            "platform": platform,
            "arch": arch,
            "version": version,
            "filename": filename,
            "sha256": digest,
            "size": int(size),
        }
    ]
}
with open(target, "w", encoding="utf-8") as handle:
    json.dump(document, handle, indent=2)
    handle.write("\n")
print(target)
PYTHON
}

merge_manifest() {
  local directory="$1" version="$2"
  python3 - "$directory" "$version" <<'PYTHON'
import datetime
import glob
import json
import os
import sys

directory, version = sys.argv[1], sys.argv[2]
artifacts = []
for path in sorted(glob.glob(os.path.join(directory, "manifest-*.json"))):
    with open(path, encoding="utf-8") as handle:
        fragment = json.load(handle)
    artifacts.extend(fragment.get("artifacts", []))
if not artifacts:
    raise SystemExit("no manifest fragment was found; nothing to publish")
artifacts.sort(key=lambda item: (item.get("platform", ""), item.get("arch", "")))
stamp = datetime.datetime.now(datetime.timezone.utc).replace(microsecond=0)
manifest = {
    "product": "screenroom",
    "version": version,
    "tag": f"v{version}",
    "generated_at": stamp.isoformat().replace("+00:00", "Z"),
    "artifacts": artifacts,
}
target = os.path.join(directory, "manifest.json")
with open(target, "w", encoding="utf-8") as handle:
    json.dump(manifest, handle, indent=2)
    handle.write("\n")
print(target)
PYTHON
}

sign_manifest() {
  local manifest="$1" key="${SCREENROOM_SIGNING_KEY_FILE:-}"
  rm -f "$manifest.sig"
  if [ -z "$key" ]; then
    printf 'manifest published unsigned: SCREENROOM_SIGNING_KEY_FILE is not set\n'
    return 0
  fi
  if [ ! -f "$key" ]; then
    printf 'manifest published unsigned: %s does not exist\n' "$key"
    return 0
  fi
  chmod 600 "$key" 2>/dev/null || true
  ssh-keygen -Y sign -f "$key" -n screenroom "$manifest" >/dev/null
  printf 'manifest signed: %s.sig\n' "$manifest"
}

build_appcast() {
  local directory="$1" tag="$2"
  local generator="$root/vendor/sparkle/bin/generate_appcast"
  [ -x "$generator" ] || fail "no Sparkle tools; run scripts/fetch-sparkle.sh first"
  local base="${SCREENROOM_UPDATE_BASE_URL:-}"
  if [ -z "$base" ]; then
    [ -n "$tag" ] || fail "an appcast needs the release tag it will be served under"
    base="https://github.com/Dropout-Studio-Co/screenroom/releases/download/$tag"
  fi
  base="${base%/}/"
  local key_file="${SCREENROOM_SPARKLE_KEY_FILE:-}"
  local archives="$directory"
  find "$archives" -maxdepth 1 -name '*.zip' -type f | grep -q . \
    || fail "no update archive is in $archives"

  if [ -n "${SCREENROOM_SPARKLE_ED_KEY:-}" ]; then
    printf '%s\n' "$SCREENROOM_SPARKLE_ED_KEY" | "$generator" \
      --ed-key-file - \
      --maximum-deltas 0 \
      --maximum-versions 1 \
      --download-url-prefix "$base" \
      --link "https://github.com/Dropout-Studio-Co/screenroom" \
      -o "$archives/appcast.xml" \
      "$archives"
  elif [ -n "$key_file" ] && [ -s "$key_file" ]; then
    "$generator" \
      --ed-key-file "$key_file" \
      --maximum-deltas 0 \
      --maximum-versions 1 \
      --download-url-prefix "$base" \
      --link "https://github.com/Dropout-Studio-Co/screenroom" \
      -o "$archives/appcast.xml" \
      "$archives"
  else
    fail "no update signing key; refusing to write an unsigned appcast"
  fi

  grep -q 'sparkle-signatures:' "$archives/appcast.xml" \
    || fail "the generated appcast carries no feed signature"
  printf 'appcast %s\n' "$archives/appcast.xml"
}

install_bundle() {
  local source_app="$1" destination="$2"
  [ -d "$source_app/Contents/MacOS" ] || fail "no application bundle at $source_app"
  mkdir -p "$destination"
  local incoming="$destination/.Screenroom.app.incoming"
  local previous="$destination/.Screenroom.app.previous"
  rm -rf "$incoming" "$previous"
  ditto "$source_app" "$incoming"
  if [ -e "$destination/Screenroom.app" ]; then
    mv "$destination/Screenroom.app" "$previous"
  fi
  mv "$incoming" "$destination/Screenroom.app"
  rm -rf "$previous"
  printf 'installed %s\n' "$destination/Screenroom.app"
}

build_macos() {
  [ "$(uname -s)" = "Darwin" ] || fail "a macOS bundle can only be assembled on macOS"
  if [ ! -d "$root/vendor/sparkle/Sparkle.framework" ]; then
    "$root/scripts/fetch-sparkle.sh" || fail "the pinned update framework could not be fetched"
  fi
  local version arch filename zip digest bytes fragment
  version="$(resolve_version)"
  [ -n "$version" ] || fail "could not read the version from apps/desktop/Cargo.toml or Cargo.toml"
  arch="$(host_arch)"
  "$root/scripts/package-macos.sh"
  if [ -n "$install_dir" ]; then
    install_bundle "$app_path" "$install_dir"
  fi
  if [ "$archive" = "0" ]; then
    return 0
  fi
  mkdir -p "$out_dir"
  filename="Screenroom-$version-macos-$arch.zip"
  zip="$out_dir/$filename"
  rm -f "$zip"
  ditto -c -k --sequesterRsrc --keepParent "$app_path" "$zip"
  digest="$(sha256_of "$zip")"
  bytes="$(file_size "$zip")"
  fragment="$(write_fragment "$out_dir/manifest-macos-$arch.json" macos "$arch" "$filename" "$digest" "$bytes" "$version")"
  printf 'artefact %s\n' "$zip"
  printf 'sha256 %s\n' "$digest"
  printf 'size %s\n' "$bytes"
  printf 'fragment %s\n' "$fragment"
}

command_name="${1:-}"
if [ "$#" -gt 0 ]; then
  shift
fi

while [ "$#" -gt 0 ]; do
  case "$1" in
    --out)
      [ -n "${2:-}" ] || fail "--out needs a directory"
      out_dir="$2"
      shift 2
      ;;
    --install)
      [ -n "${2:-}" ] || fail "--install needs a directory"
      install_dir="$2"
      shift 2
      ;;
    --app)
      [ -n "${2:-}" ] || fail "--app needs a path"
      app_path="$2"
      shift 2
      ;;
    --tag)
      [ -n "${2:-}" ] || fail "--tag needs the release tag"
      tag="$2"
      shift 2
      ;;
    --no-archive)
      archive=0
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      fail "unknown argument: $1"
      ;;
  esac
done

case "$command_name" in
  macos)
    build_macos
    ;;
  install)
    install_dir="${install_dir:-$HOME/Applications}"
    install_bundle "$app_path" "$install_dir"
    ;;
  manifest)
    version="$(resolve_version)"
    [ -n "$version" ] || fail "could not read the version"
    mkdir -p "$out_dir"
    target="$(merge_manifest "$out_dir" "$version")"
    printf 'manifest %s\n' "$target"
    sign_manifest "$target"
    ;;
  appcast)
    mkdir -p "$out_dir"
    build_appcast "$out_dir" "$tag"
    ;;
  version)
    resolve_version
    ;;
  ''|help)
    usage
    exit 2
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
