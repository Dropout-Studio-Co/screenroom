#!/usr/bin/env bash

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

app="${1:-$root/dist/Screenroom.app}"
executable_name="Screenroom"

failures=0
fail() {
  echo "  FAIL: $*" >&2
  failures=$((failures + 1))
}
ok() { echo "  ok:   $*"; }

if [ ! -d "$app" ]; then
  echo "no bundle at $app" >&2
  echo "build one with: scripts/package-macos.sh" >&2
  exit 2
fi

echo "==> structure"
for required in "Contents/Info.plist" "Contents/MacOS/$executable_name"; do
  if [ -e "$app/$required" ]; then ok "$required"; else fail "$required is missing"; fi
done
if [ -x "$app/Contents/MacOS/$executable_name" ]; then
  ok "the executable is executable"
else
  fail "Contents/MacOS/$executable_name is not executable"
fi

echo
echo "==> Info.plist"
if plutil -lint "$app/Contents/Info.plist" >/dev/null 2>&1; then
  ok "plist parses"
  declared="$(plutil -extract CFBundleExecutable raw "$app/Contents/Info.plist" 2>/dev/null || true)"
  if [ "$declared" = "$executable_name" ]; then
    ok "CFBundleExecutable names $declared"
  else
    fail "CFBundleExecutable is '${declared:-unset}', but the binary is $executable_name"
  fi
  identifier="$(plutil -extract CFBundleIdentifier raw "$app/Contents/Info.plist" 2>/dev/null || true)"
  if [ -n "$identifier" ]; then ok "CFBundleIdentifier is $identifier"; else fail "no CFBundleIdentifier"; fi
else
  fail "Info.plist does not parse"
fi

echo
echo "==> bundled libmpv"
ls -1 "target/release/screenroom-desktop" >/dev/null 2>&1 || true
libmpv="$app/Contents/Frameworks/libmpv.2.dylib"
if [ ! -f "$libmpv" ]; then
  echo "  note: the bundle carries no libmpv, so playback will report itself unavailable."
  echo "        Install one with: brew install mpv, then re-run scripts/package-macos.sh"
else

  if [ -L "$libmpv" ]; then
    fail "Contents/Frameworks/libmpv.2.dylib is a symlink, not a real file"
  else
    ok "libmpv.2.dylib is a real file ($(du -h "$libmpv" | cut -f1 | tr -d ' '))"
  fi
  if file "$libmpv" | grep -q 'Mach-O'; then
    ok "libmpv.2.dylib is a Mach-O library ($(lipo -archs "$libmpv" 2>/dev/null || echo unknown))"
  else
    fail "libmpv.2.dylib is not a Mach-O library"
  fi

  echo
  echo "==> the media stack beside it"
  stack=0
  absolute=0
  for library in "$app/Contents/Frameworks/"*.dylib; do
    [ -f "$library" ] || continue
    stack=$((stack + 1))
    while IFS= read -r dependency; do
      case "$dependency" in
        /opt/homebrew/*|/usr/local/*) absolute=$((absolute + 1)) ;;
      esac
    done < <(otool -L "$library" | tail -n +2 | awk '{print $1}')
  done
  ok "$stack media libraries are bundled"
  if [ "$absolute" -eq 0 ]; then
    ok "every media dependency resolves inside the bundle"
  elif [ "${SCREENROOM_REQUIRE_SELF_CONTAINED:-0}" = "1" ]; then
    fail "$absolute dependency path(s) still point at Homebrew, so this bundle is not redistributable"
  else
    echo "  note: $absolute dependency path(s) point at Homebrew; this bundle is for this machine only."
    echo "        Run scripts/build-lgpl-media.sh before publishing."
  fi

  if [ -f "$app/Contents/Resources/licenses/LGPL-SOURCE-OFFER.txt" ]; then
    ok "the LGPL source offer ships inside the bundle"
  elif [ "$absolute" -eq 0 ]; then
    fail "the bundle carries LGPL libraries but no source offer"
  fi

  echo
  echo "==> the bundle's own libmpv loads (no window, no focus taken)"
  if output="$(cargo run --quiet -p screenroom-player --example capability -- "$libmpv" 2>&1)"; then
    echo "$output" | sed 's/^/  /'
    ok "the copy inside the bundle created a real libmpv core"
  else
    echo "$output" | sed 's/^/  /' >&2
    fail "the bundled libmpv did not load"
  fi
fi

echo
echo "==> the update framework"
framework="$app/Contents/Frameworks/Sparkle.framework"
if [ -d "$framework" ]; then
  ok "Sparkle.framework is embedded"
  if [ -L "$framework/Versions/Current" ]; then
    ok "the framework's symlinks survived archiving"
  else
    fail "Versions/Current is not a symlink, so the framework was flattened"
  fi
  if file "$framework/Versions/B/Sparkle" | grep -q 'Mach-O'; then
    ok "the framework binary is Mach-O ($(lipo -archs "$framework/Versions/B/Sparkle" 2>/dev/null || echo unknown))"
  else
    fail "the framework binary is not Mach-O"
  fi
  for helper in Autoupdate Updater.app/Contents/MacOS/Updater XPCServices/Installer.xpc/Contents/MacOS/Installer; do
    if [ -e "$framework/Versions/B/$helper" ]; then
      ok "the helper $helper is present"
    else
      fail "the helper $helper is missing from the framework"
    fi
  done
else
  fail "Contents/Frameworks/Sparkle.framework is missing, so updates cannot run"
fi

echo
echo "==> update configuration"
feed="$(plutil -extract SUFeedURL raw "$app/Contents/Info.plist" 2>/dev/null || true)"
key="$(plutil -extract SUPublicEDKey raw "$app/Contents/Info.plist" 2>/dev/null || true)"
case "$feed" in
  https://*) ok "the feed is https ($feed)" ;;
  http://127.0.0.1:*|http://localhost:*|file://*)
    if [ "${SCREENROOM_ALLOW_TEST_FEED:-0}" = "1" ]; then
      echo "  note: the feed is a local test address ($feed)"
    else
      fail "the feed is not https: ${feed:-unset}"
    fi
    ;;
  *) fail "the feed is not https: ${feed:-unset}" ;;
esac
if [ -n "$key" ]; then
  ok "an Ed25519 public key is embedded"
else
  fail "no SUPublicEDKey, so no update could ever be verified"
fi
for requirement in SURequireSignedFeed SUVerifyUpdateBeforeExtraction SUEnableAutomaticChecks; do
  value="$(plutil -extract "$requirement" raw "$app/Contents/Info.plist" 2>/dev/null || true)"
  if [ "$value" = "true" ]; then
    ok "$requirement is on"
  else
    fail "$requirement is '${value:-unset}'"
  fi
done
expiry="$(plutil -extract SUSignedFeedFailureExpirationInterval raw "$app/Contents/Info.plist" 2>/dev/null || true)"
if [ "$expiry" = "0" ]; then
  ok "a failed feed signature never expires into an unsigned fallback"
else
  fail "SUSignedFeedFailureExpirationInterval is '${expiry:-unset}', not 0"
fi
minimum="$(plutil -extract LSMinimumSystemVersion raw "$app/Contents/Info.plist" 2>/dev/null || true)"
case "$minimum" in
  1[2-9].*|2[0-9].*) ok "LSMinimumSystemVersion is $minimum, which the framework needs" ;;
  *) fail "LSMinimumSystemVersion is '${minimum:-unset}'; Sparkle 2.10 needs 12.0" ;;
esac
if otool -L "$app/Contents/MacOS/$executable_name" | grep -q '@rpath/Sparkle.framework'; then
  ok "the executable links the framework through @rpath"
else
  fail "the executable does not link Sparkle"
fi
if otool -l "$app/Contents/MacOS/$executable_name" | grep -A2 LC_RPATH | grep -q '@executable_path/../Frameworks'; then
  ok "the runtime search path finds the bundled framework"
else
  fail "no @executable_path/../Frameworks runtime search path"
fi

echo
echo "==> signature and permissions"
if codesign --verify --deep --strict "$app" 2>/dev/null; then
  ok "the bundle's ad-hoc signature verifies, libmpv included"
else
  fail "codesign --verify --deep --strict rejects the bundle"
fi
for key in NSRemovableVolumesUsageDescription NSDocumentsFolderUsageDescription NSLocalNetworkUsageDescription; do
  if plutil -extract "$key" raw "$app/Contents/Info.plist" >/dev/null 2>&1; then
    ok "$key explains the access prompt"
  else
    fail "no $key, so macOS asks for drive access without saying why"
  fi
done

echo
echo "==> media tools the installed app runs (posters, runtimes)"
for tool in ffmpeg ffprobe; do
  if [ -x "/opt/homebrew/bin/$tool" ] || [ -x "/usr/local/bin/$tool" ]; then
    ok "$tool is installed where a Dock launch can find it"
  else
    echo "  note: no $tool in /opt/homebrew/bin or /usr/local/bin; cards will have no posters or runtimes."
  fi
done

echo
if [ "$failures" -ne 0 ]; then
  echo "$failures check(s) failed." >&2
  exit 1
fi
cat <<'NOTE'
All checks passed.

Not proven here, by design: that a frame reaches the screen, or that the window
opens at all. Both need a visible window, which is an owner-approved session —
see docs/TEST_PLAN.md.
NOTE
