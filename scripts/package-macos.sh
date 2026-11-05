#!/usr/bin/env bash

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

install_into="${1:-}"
app="$root/dist/Screenroom.app"
bundle_id="com.screenroom.desktop"
process="Screenroom"
sparkle="$root/vendor/sparkle/Sparkle.framework"
public_key_file="$root/apps/desktop/updater/ed25519_public_key.txt"
feed_url="${SCREENROOM_UPDATE_FEED_URL:-https://github.com/Dropout-Studio-Co/screenroom/releases/latest/download/appcast.xml}"
sign_identity="${SCREENROOM_CODESIGN_IDENTITY:--}"
sign_options=()
if [ "$sign_identity" != "-" ] && [ "${SCREENROOM_HARDENED_RUNTIME:-0}" = "1" ]; then
  sign_options=(--options runtime)
fi
minimum_macos="12.0"

if [ -n "${SCREENROOM_UPDATE_PUBLIC_KEY:-}" ]; then
  if [ "${SCREENROOM_ALLOW_TEST_FEED:-0}" != "1" ]; then
    echo "!! SCREENROOM_UPDATE_PUBLIC_KEY would replace the committed trust anchor; only an isolated test build may do that" >&2
    exit 2
  fi
  public_key="$SCREENROOM_UPDATE_PUBLIC_KEY"
elif [ -f "$public_key_file" ]; then
  public_key="$(tr -d '\n\r ' <"$public_key_file")"
else
  public_key=""
fi

case "$feed_url" in
  https://*) ;;
  file://* | http://127.0.0.1:* | http://localhost:*)
    if [ "${SCREENROOM_ALLOW_TEST_FEED:-0}" != "1" ]; then
      echo "!! $feed_url is not https; set SCREENROOM_ALLOW_TEST_FEED=1 only for an isolated test build" >&2
      exit 2
    fi
    ;;
  *)
    echo "!! the update feed must be https, not $feed_url" >&2
    exit 2
    ;;
esac

if [ -z "$public_key" ]; then
  echo "!! no update public key; refusing to build a bundle whose updates cannot be verified" >&2
  echo "   apps/desktop/updater/ed25519_public_key.txt is the committed trust anchor" >&2
  exit 2
fi

if [ ! -d "$sparkle" ]; then
  echo "!! Sparkle.framework is absent; run scripts/fetch-sparkle.sh first" >&2
  exit 2
fi

if [ "$(uname -s)" != "Darwin" ]; then
  echo "this script packages a macOS app; $(uname -s) is not macOS" >&2
  exit 2
fi

echo "==> building the release binary"
cargo build --release -p screenroom-desktop

binary="$root/target/release/screenroom-desktop"
[ -x "$binary" ] || { echo "missing $binary" >&2; exit 1; }

echo "==> assembling $app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources" "$app/Contents/Frameworks"
cp "$binary" "$app/Contents/MacOS/$process"
chmod +x "$app/Contents/MacOS/$process"

if [ -f "$root/assets/Screenroom.icns" ]; then
  cp "$root/assets/Screenroom.icns" "$app/Contents/Resources/Screenroom.icns"
fi

if [ -d "$root/vendor/lgpl/licenses" ]; then
  mkdir -p "$app/Contents/Resources/licenses"
  cp "$root/vendor/lgpl/licenses/"*.txt "$app/Contents/Resources/licenses/"
  cp "$root/vendor/lgpl/COMPONENTS.txt" "$app/Contents/Resources/licenses/COMPONENTS.txt" 2>/dev/null || true
fi

lgpl="$root/vendor/lgpl/frameworks"
libmpv=""
self_contained=0

if [ -n "${SCREENROOM_LIBMPV_DIR:-}" ] && [ -f "$SCREENROOM_LIBMPV_DIR/libmpv.2.dylib" ]; then
  libmpv="$SCREENROOM_LIBMPV_DIR/libmpv.2.dylib"
  self_contained=1
elif [ -f "$lgpl/libmpv.2.dylib" ]; then
  libmpv="$lgpl/libmpv.2.dylib"
  self_contained=1
else
  for candidate in \
    /opt/homebrew/lib/libmpv.2.dylib \
    /usr/local/lib/libmpv.2.dylib \
    /opt/homebrew/lib/libmpv.dylib \
    /usr/local/lib/libmpv.dylib
  do
    if [ -f "$candidate" ]; then
      libmpv="$candidate"
      break
    fi
  done
fi

if [ -z "$libmpv" ]; then
  echo "!! no libmpv found, so the app will report playback as unavailable." >&2
  echo "   Build the redistributable one with: scripts/build-lgpl-media.sh" >&2
else
  directory="$(dirname "$libmpv")"
  if [ "$self_contained" = "1" ]; then
    for library in "$directory"/*.dylib; do
      cp -f "$library" "$app/Contents/Frameworks/$(basename "$library")"
    done
    echo "==> bundled the LGPL media stack from $directory"
    chmod u+w "$app/Contents/Frameworks/"*.dylib
    leaks=0
    for library in "$app/Contents/Frameworks/"*.dylib; do
      while IFS= read -r dependency; do
        case "$dependency" in
          /opt/homebrew/*|/usr/local/*)
            echo "      absolute dependency: $dependency" >&2
            leaks=$((leaks + 1))
            ;;
        esac
      done < <(otool -L "$library" | tail -n +2 | awk '{print $1}')
    done
    if [ "$leaks" -ne 0 ]; then
      echo "!! the media stack is not self-contained; the bundle would need Homebrew on the target" >&2
      exit 1
    fi
    echo "    the stack is self-contained; nothing resolves through Homebrew"
  else
    cp -L "$libmpv" "$app/Contents/Frameworks/libmpv.2.dylib"
    echo "!! bundled $libmpv, which is a Homebrew build" >&2
    echo "!! it is NOT redistributable; run scripts/build-lgpl-media.sh before publishing" >&2
    resolved="$(cd "$(dirname "$libmpv")" && pwd -P)/$(basename "$libmpv")"
    missing=0
    while IFS= read -r dependency; do
      [ -e "$dependency" ] || { echo "      MISSING: $dependency" >&2; missing=1; }
    done < <(otool -L "$resolved" 2>/dev/null | tail -n +2 | awk '{print $1}' | grep -E '^/(opt|usr/local)/')
    if [ "$missing" -ne 0 ]; then
      echo "!! some libmpv dependencies are absent; playback will fail on this machine." >&2
      exit 1
    fi
  fi
fi

version="$(grep -m1 '^version' "$root/Cargo.toml" | sed -E 's/.*"(.*)".*/\1/')"
version="${version:-0.1.0}"

if [ -n "${SCREENROOM_VERSION:-}" ] \
  && { [ "${SCREENROOM_ALLOW_TEST_FEED:-0}" = "1" ] || [ "${SCREENROOM_ALLOW_VERSION_OVERRIDE:-0}" = "1" ]; }; then
  version="$SCREENROOM_VERSION"
fi

build_number="${SCREENROOM_BUILD_NUMBER:-$version}"
build_number="$(printf '%s' "$build_number" | sed -E 's/[^0-9]+/./g; s/^\.//; s/\.$//')"
if [ -z "$build_number" ]; then
  echo "!! $version cannot become a CFBundleVersion" >&2
  exit 1
fi

test_transport=""
if [ "${SCREENROOM_ALLOW_TEST_FEED:-0}" = "1" ]; then
  test_transport="  <key>NSAppTransportSecurity</key>
  <dict><key>NSAllowsLocalNetworking</key><true/></dict>"
fi

echo "==> embedding $sparkle"
ditto "$sparkle" "$app/Contents/Frameworks/Sparkle.framework"

cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Screenroom</string>
  <key>CFBundleDisplayName</key><string>Screenroom</string>
  <key>CFBundleIdentifier</key><string>$bundle_id</string>
  <key>CFBundleExecutable</key><string>$process</string>
  <key>CFBundleVersion</key><string>$build_number</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>LSMinimumSystemVersion</key><string>$minimum_macos</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>LSApplicationCategoryType</key><string>public.app-category.video</string>
  <key>NSSupportsAutomaticGraphicsSwitching</key><true/>
  <key>CFBundleIconFile</key><string>Screenroom</string>
  <key>SUFeedURL</key><string>$feed_url</string>
  <key>SUPublicEDKey</key><string>$public_key</string>
  <key>SUEnableAutomaticChecks</key><true/>
  <key>SUScheduledCheckInterval</key><integer>3600</integer>
  <key>SUAutomaticallyUpdate</key><true/>
  <key>SUAllowsAutomaticUpdates</key><true/>
  <key>SUVerifyUpdateBeforeExtraction</key><true/>
  <key>SURequireSignedFeed</key><true/>
  <key>SUSignedFeedFailureExpirationInterval</key><integer>0</integer>
  <key>SUEnableSystemProfiling</key><false/>
$test_transport
  <key>NSRemovableVolumesUsageDescription</key><string>Screenroom reads the films and shows on drives you add to its library. It never changes them.</string>
  <key>NSDocumentsFolderUsageDescription</key><string>Screenroom reads media folders and writes backups only where you choose.</string>
  <key>NSLocalNetworkUsageDescription</key><string>Screenroom looks for cast receivers on your network only when you press the button, and then serves the one file you chose from a temporary endpoint that closes itself.</string>
</dict>
</plist>
PLIST

xattr -cr "$app" 2>/dev/null || true

for tool in ffmpeg ffprobe; do
  found=""
  for directory in /opt/homebrew/bin /usr/local/bin; do
    [ -x "$directory/$tool" ] && { found="$directory/$tool"; break; }
  done
  if [ -n "$found" ]; then
    echo "==> $tool: $found"
  else
    echo "!! no $tool found; posters and runtimes will be missing. Install with: brew install ffmpeg" >&2
  fi
done

for library in "$app/Contents/Frameworks/"*.dylib; do
  [ -f "$library" ] || continue
  codesign --force "${sign_options[@]}" --sign "$sign_identity" "$library"
done

echo "==> signing Sparkle's own helpers, then the framework"
sparkle_root="$app/Contents/Frameworks/Sparkle.framework/Versions/B"
for helper in "$sparkle_root/Autoupdate" "$sparkle_root/XPCServices/Downloader.xpc" "$sparkle_root/XPCServices/Installer.xpc" "$sparkle_root/Updater.app"; do
  [ -e "$helper" ] || continue
  codesign --force --deep "${sign_options[@]}" --sign "$sign_identity" "$helper"
done
codesign --force "${sign_options[@]}" --sign "$sign_identity" "$app/Contents/Frameworks/Sparkle.framework"
codesign --force "${sign_options[@]}" --sign "$sign_identity" "$app"
codesign --verify --deep --strict "$app"
echo "==> signed with '$sign_identity' and verified"

echo "==> built $app"
plutil -lint "$app/Contents/Info.plist" >/dev/null && echo "    Info.plist is valid"
du -sh "$app" | sed 's/^/    size: /'

if [ -n "$install_into" ]; then
  mkdir -p "$install_into"
  staged="$install_into/.Screenroom.app.staged.$$"
  previous="$install_into/.Screenroom.app.previous.$$"
  rm -rf "$staged" "$previous"
  if ! ditto "$app" "$staged"; then
    rm -rf "$staged"
    echo "!! could not stage the bundle into $install_into; nothing was replaced" >&2
    exit 1
  fi
  if [ -d "$install_into/Screenroom.app" ] && ! mv "$install_into/Screenroom.app" "$previous"; then
    rm -rf "$staged"
    echo "!! could not move the installed app aside; nothing was replaced" >&2
    exit 1
  fi
  if mv "$staged" "$install_into/Screenroom.app"; then
    rm -rf "$previous"
    echo "==> installed $install_into/Screenroom.app"
  else
    if [ -d "$previous" ]; then
      mv "$previous" "$install_into/Screenroom.app" || true
    fi
    rm -rf "$staged"
    echo "!! could not install; the previous app was restored if it existed" >&2
    exit 1
  fi
fi

cat <<'NOTE'

Run it with:
  open dist/Screenroom.app

The first launch of a locally built app may show Gatekeeper's "unidentified
developer" prompt. Right-click the app and choose Open once, or open
System Settings > Privacy & Security and press "Open Anyway".

Settings (viewer profiles, folders, backup) live in the app's own data
directory, which is the same one the development build uses, so an installed
app opens the library you have already been building.
NOTE
