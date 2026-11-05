#!/usr/bin/env bash

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
app="${1:-$root/dist/Screenroom.app}"
profile="${SCREENROOM_NOTARY_PROFILE:-}"

fail() {
  printf '%s\n' "$*" >&2
  exit 1
}

if [ "$(uname -s)" != "Darwin" ]; then
  fail "notarisation exists on macOS only; $(uname -s) is not macOS"
fi

[ -d "$app" ] || fail "no bundle at $app"

identity="$(codesign -dv "$app" 2>&1 | sed -n 's/^Authority=//p' | head -1)"
if [ -z "$identity" ]; then
  fail "the bundle is ad-hoc signed; notarisation needs a Developer ID Application certificate"
fi
case "$identity" in
  "Developer ID Application:"*) ;;
  *) fail "the bundle is signed by '$identity', which is not a Developer ID Application certificate" ;;
esac

if ! codesign --verify --deep --strict --verbose=2 "$app" >/dev/null 2>&1; then
  fail "the bundle's signature does not verify, so it cannot be notarised"
fi

if ! codesign -dv "$app" 2>&1 | grep -q "flags=0x10000(runtime)"; then
  fail "the bundle was signed without the hardened runtime; rebuild with SCREENROOM_HARDENED_RUNTIME=1"
fi

[ -n "$profile" ] || fail "set SCREENROOM_NOTARY_PROFILE to a notarytool keychain profile (xcrun notarytool store-credentials)"

archive="$(mktemp -d "${TMPDIR:-/tmp}/screenroom-notary.XXXXXX")/Screenroom.zip"
cleanup() { rm -rf "$(dirname "$archive")"; }
trap cleanup EXIT

ditto -c -k --sequesterRsrc --keepParent "$app" "$archive"

echo "==> submitting $app"
xcrun notarytool submit "$archive" --keychain-profile "$profile" --wait

echo "==> stapling the ticket"
xcrun stapler staple "$app"
xcrun stapler validate "$app"

echo "==> checking what Gatekeeper will do"
spctl --assess --type execute --verbose=4 "$app"

echo "notarised, stapled and accepted by Gatekeeper"
