#!/usr/bin/env bash

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
prefix="$root/vendor/lgpl/prefix"
frameworks="$root/vendor/lgpl/frameworks"
work="$root/vendor/lgpl/work"
downloads="$root/vendor/lgpl/downloads"
jobs="${SCREENROOM_MEDIA_JOBS:-$(sysctl -n hw.ncpu 2>/dev/null || printf '4')}"

ffmpeg_version="7.1.1"
ffmpeg_sha="733984395e0dbbe5c046abda2dc49a5544e7e0e1e2366bba849222ae9e3a03b1"
ffmpeg_url="https://ffmpeg.org/releases/ffmpeg-${ffmpeg_version}.tar.xz"

mpv_version="0.41.0"
mpv_sha="ee21092a5ee427353392360929dc64645c54479aefdb5babc5cfbb5fad626209"
mpv_url="https://github.com/mpv-player/mpv/archive/refs/tags/v${mpv_version}.tar.gz"

libplacebo_version="7.387.0"
libplacebo_sha="f9e62d3774a9c4cfd198ec3abaa88294aaade2e31c04d5ab02bcf4abc98b1df5"
libplacebo_url="https://github.com/haasn/libplacebo/archive/refs/tags/v${libplacebo_version}.tar.gz"

fail() {
  printf '%s\n' "$*" >&2
  exit 1
}

if [ "$(uname -s)" != "Darwin" ]; then
  fail "this builds macOS libraries; $(uname -s) is not macOS"
fi

command -v meson >/dev/null || fail "meson is required: brew install meson"
command -v ninja >/dev/null || fail "ninja is required: brew install ninja"
command -v pkg-config >/dev/null || fail "pkg-config is required: brew install pkg-config"
command -v clang >/dev/null || fail "clang comes with the macOS command line tools"

mkdir -p "$work" "$downloads" "$prefix"
rm -rf "$frameworks"
mkdir -p "$frameworks"

fetch() {
  local url="$1" digest="$2" target="$3"
  if [ ! -f "$target" ]; then
    curl -fsSL --retry 5 --retry-all-errors --retry-delay 5 --proto '=https' --tlsv1.2 -o "$target.part" "$url"
    mv "$target.part" "$target"
  fi
  printf '%s  %s\n' "$digest" "$target" | shasum -a 256 -c - >/dev/null \
    || fail "$target does not match its pinned sha256"
}

fetch "$ffmpeg_url" "$ffmpeg_sha" "$downloads/ffmpeg.tar.xz"
fetch "$mpv_url" "$mpv_sha" "$downloads/mpv.tar.gz"
fetch "$libplacebo_url" "$libplacebo_sha" "$downloads/libplacebo.tar.gz"

rm -rf "$work/ffmpeg-${ffmpeg_version}" "$work/mpv-${mpv_version}" "$work/libplacebo-${libplacebo_version}"
tar -xf "$downloads/ffmpeg.tar.xz" -C "$work"
tar -xf "$downloads/mpv.tar.gz" -C "$work"
tar -xf "$downloads/libplacebo.tar.gz" -C "$work"
[ -d "$work/ffmpeg-${ffmpeg_version}" ] || fail "the ffmpeg source did not unpack"
[ -d "$work/mpv-${mpv_version}" ] || fail "the mpv source did not unpack"

ffmpeg_flags=(
  --prefix="$prefix"
  --enable-shared
  --disable-static
  --enable-pic
  --disable-gpl
  --disable-version3
  --disable-nonfree
  --disable-programs
  --disable-doc
  --disable-debug
  --enable-videotoolbox
  --enable-audiotoolbox
  --enable-avfoundation
  --disable-xlib
  --disable-libxcb
  --disable-sdl2
  --disable-vulkan
  --disable-vaapi
  --disable-vdpau
)

echo "==> building ffmpeg ${ffmpeg_version} (LGPL, shared)"
cd "$work/ffmpeg-${ffmpeg_version}"
./configure "${ffmpeg_flags[@]}" >"$work/ffmpeg-configure.log" 2>&1 \
  || { tail -30 "$work/ffmpeg-configure.log" >&2; fail "ffmpeg configure failed"; }
make -j"$jobs" >"$work/ffmpeg-build.log" 2>&1 \
  || { tail -40 "$work/ffmpeg-build.log" >&2; fail "ffmpeg build failed"; }
make install >"$work/ffmpeg-install.log" 2>&1 \
  || { tail -20 "$work/ffmpeg-install.log" >&2; fail "ffmpeg install failed"; }

export PKG_CONFIG_PATH="$prefix/lib/pkgconfig:${PKG_CONFIG_PATH:-}"

if ! python3 -c "import glad" >/dev/null 2>&1; then
  python3 -m venv "$work/glad-venv" >/dev/null 2>&1 \
    || fail "python3 -m venv is required to provide glad"
  "$work/glad-venv/bin/pip" install --quiet glad2 >/dev/null 2>&1 \
    || fail "glad2 could not be installed"
  export PYTHONPATH="$("$work/glad-venv/bin/python" -c 'import glad, os; print(os.path.dirname(os.path.dirname(glad.__file__)))'):${PYTHONPATH:-}"
fi

echo "==> building libplacebo ${libplacebo_version} (no Vulkan, OpenGL only)"
cd "$work/libplacebo-${libplacebo_version}"
rm -rf build
meson setup build \
  -Dprefix="$prefix" \
  -Dvulkan=disabled \
  -Dvk-proc-addr=disabled \
  -Dvulkan-registry=disabled \
  -Dopengl=enabled \
  -Dd3d11=disabled \
  -Dglslang=disabled \
  -Dshaderc=disabled \
  -Dlcms=disabled \
  -Ddovi=disabled \
  -Dlibdovi=disabled \
  -Ddemos=false \
  -Dtests=false \
  -Dbench=false \
  -Dfuzz=false \
  >"$work/libplacebo-configure.log" 2>&1 \
  || { tail -40 "$work/libplacebo-configure.log" >&2; fail "libplacebo configure failed"; }
ninja -C build >"$work/libplacebo-build.log" 2>&1 \
  || { tail -40 "$work/libplacebo-build.log" >&2; fail "libplacebo build failed"; }
ninja -C build install >"$work/libplacebo-install.log" 2>&1 \
  || { tail -20 "$work/libplacebo-install.log" >&2; fail "libplacebo install failed"; }

apply_mpv_patch() {
  python3 - "$work/mpv-${mpv_version}/meson.build" <<'PATCH'
import shutil
import sys

path = sys.argv[1]
with open(path, encoding="utf-8") as handle:
    text = handle.read()

if "sources += files('osdep/utils-mac.c')" not in text:
    text = text.replace(
        """if darwin
    path_source = files('osdep/path-darwin.c')
    timer_source = files('osdep/timer-darwin.c')
endif""",
        """if darwin
    path_source = files('osdep/path-darwin.c')
    timer_source = files('osdep/timer-darwin.c')
    sources += files('osdep/utils-mac.c')
endif""",
        1,
    )
    text = text.replace("""    sources += files('osdep/language-mac.c',
                     'osdep/utils-mac.c',
                     'osdep/mac/app_bridge.m',""", """    sources += files('osdep/language-mac.c',
                     'osdep/mac/app_bridge.m',""", 1)
    with open(path, "w", encoding="utf-8") as handle:
        handle.write(text)
PATCH
}

original="$work/meson-build.orig"
cp "$work/mpv-${mpv_version}/meson.build" "$original"
apply_mpv_patch
if ! cmp -s "$original" "$work/mpv-${mpv_version}/meson.build"; then
  ( cd "$work/mpv-${mpv_version}" && diff -u "$original" meson.build | sed "s#$original#a/meson.build#" ) \
    >"$root/vendor/lgpl/mpv-lgpl-build.patch" || true
fi

mpv_flags=(
  -Dprefix="$prefix"
  -Dlibmpv=true
  -Dcocoa=disabled
  -Dcplayer=false
  -Dgpl=false
  -Dbuild-date=false
  -Dlcms2=disabled
  -Dzimg=disabled
  -Dlibavdevice=disabled
  -Dlua=enabled
  -Djavascript=disabled
  -Dlibarchive=disabled
  -Dlibbluray=disabled
  -Ddvdnav=disabled
  -Duchardet=disabled
  -Drubberband=disabled
  -Dvapoursynth=disabled
  -Dpulse=disabled
  -Dpipewire=disabled
  -Djack=disabled
  -Dsdl2-audio=disabled
  -Dsdl2-video=disabled
  -Dsdl2-gamepad=disabled
  -Dcaca=disabled
  -Ddrm=disabled
  -Dvaapi=disabled
  -Dvdpau=disabled
  -Dx11=disabled
  -Dwayland=disabled
  -Dvulkan=disabled
  -Dshaderc=disabled
  -Dmacos-10-15-4-features=disabled
)

echo "==> building mpv ${mpv_version} (LGPL, libmpv)"
cd "$work/mpv-${mpv_version}"
rm -rf build
meson setup build "${mpv_flags[@]}" >"$work/mpv-configure.log" 2>&1 \
  || { tail -60 "$work/mpv-configure.log" >&2; fail "mpv configure failed"; }
ninja -C build >"$work/mpv-build.log" 2>&1 \
  || { tail -40 "$work/mpv-build.log" >&2; fail "mpv build failed"; }
ninja -C build install >"$work/mpv-install.log" 2>&1 \
  || { tail -20 "$work/mpv-install.log" >&2; fail "mpv install failed"; }

[ -f "$prefix/lib/libmpv.2.dylib" ] || fail "libmpv.2.dylib was not installed"

echo "==> collecting the non-system dependencies"
collected="$work/collected.txt"
: >"$collected"

resolve_dependency() {
  local dependency="$1" name="$2"
  case "$dependency" in
    @rpath/*)
      local directory
      for directory in "$prefix/lib" /opt/homebrew/lib /opt/homebrew/opt/*/lib; do
        if [ -f "$directory/$name" ]; then
          printf '%s\n' "$directory/$name"
          return 0
        fi
      done
      ;;
    *)
      if [ -f "$dependency" ]; then
        printf '%s\n' "$dependency"
        return 0
      fi
      ;;
  esac
  return 1
}

cp -fL "$prefix/lib/libmpv.2.dylib" "$frameworks/libmpv.2.dylib"
chmod u+w "$frameworks/libmpv.2.dylib"
printf 'libmpv.2.dylib\t%s\n' "$prefix/lib/libmpv.2.dylib" >>"$collected"

round=0
while [ "$round" -lt 12 ]; do
  added=0
  for library in "$frameworks"/*.dylib; do
    while IFS= read -r dependency; do
      [ -n "$dependency" ] || continue
      case "$dependency" in
        @rpath/*|/opt/homebrew/*|/usr/local/*|"$prefix"/*) ;;
        *) continue ;;
      esac
      name="$(basename "$dependency")"
      if [ -f "$frameworks/$name" ]; then
        continue
      fi
      resolved="$(resolve_dependency "$dependency" "$name")" \
        || fail "cannot resolve $dependency, needed by $library"
      cp -fL "$resolved" "$frameworks/$name"
      chmod u+w "$frameworks/$name"
      printf '%s\t%s\n' "$name" "$resolved" >>"$collected"
      added=1
    done < <(otool -L "$library" | tail -n +2 | awk '{print $1}')
  done
  if [ "$added" -eq 0 ]; then
    break
  fi
  round=$((round + 1))
done
[ "$round" -lt 12 ] || fail "the dependency closure did not settle"

echo "==> rewriting install names to @rpath"
for library in "$frameworks"/*.dylib; do
  name="$(basename "$library")"
  install_name_tool -id "@rpath/$name" "$library" 2>/dev/null || true
  otool -L "$library" | tail -n +2 | awk '{print $1}' | while IFS= read -r dependency; do
    case "$dependency" in
      /opt/homebrew/*|/usr/local/*|"$prefix"/*)
        install_name_tool -change "$dependency" "@rpath/$(basename "$dependency")" "$library" 2>/dev/null || true
        ;;
    esac
  done
  if ! otool -l "$library" | grep -A2 LC_RPATH | grep -q '@loader_path'; then
    install_name_tool -add_rpath "@loader_path" "$library" 2>/dev/null || true
  fi
done

for library in "$frameworks"/*.dylib; do
  codesign --force --sign - "$library" >/dev/null 2>&1 || true
done

echo "==> checking that nothing still points outside the bundle"
leaks=0
for library in "$frameworks"/*.dylib; do
  while IFS= read -r dependency; do
    case "$dependency" in
      /opt/homebrew/*|/usr/local/*) echo "  still absolute: $dependency" >&2; leaks=$((leaks + 1)) ;;
    esac
  done < <(otool -L "$library" | tail -n +2 | awk '{print $1}')
done
[ "$leaks" -eq 0 ] || fail "$leaks absolute dependency path(s) remain"

{
  printf 'Bundled libraries and where this build took them from:\n'
  while IFS=$'\t' read -r name origin; do
    printf '  %-28s %s\n' "$name" "$origin"
  done <"$collected"
} >"$root/vendor/lgpl/COMPONENTS.txt"

licences="$root/vendor/lgpl/licenses"
rm -rf "$licences"
mkdir -p "$licences"
cp "$work/ffmpeg-${ffmpeg_version}/COPYING.LGPLv2.1" "$licences/FFmpeg-LGPL-2.1.txt"
cp "$work/ffmpeg-${ffmpeg_version}/COPYING.LGPLv3" "$licences/FFmpeg-LGPL-3.txt"
cp "$work/ffmpeg-${ffmpeg_version}/COPYING.GPLv2" "$licences/FFmpeg-GPL-2.txt"
cp "$work/mpv-${mpv_version}/Copyright" "$licences/mpv-Copyright.txt"
cp "$root/vendor/lgpl/mpv-lgpl-build.patch" "$licences/mpv-lgpl-build.patch" 2>/dev/null || true
cp "$root/docs/LGPL_SOURCE_OFFER.md" "$licences/LGPL-SOURCE-OFFER.txt"

cat >"$root/vendor/lgpl/BUILD_INFO" <<INFO
product: screenroom-lgpl-media
built_at: $(date -u +%Y-%m-%dT%H:%M:%SZ)
arch: $(uname -m)
macos: $(sw_vers -productVersion)

ffmpeg: ${ffmpeg_version}
ffmpeg_sha256: ${ffmpeg_sha}
ffmpeg_flags: ${ffmpeg_flags[*]}

libplacebo: ${libplacebo_version}
libplacebo_sha256: ${libplacebo_sha}

mpv: ${mpv_version}
mpv_sha256: ${mpv_sha}
mpv_flags: ${mpv_flags[*]}

licences: FFmpeg LGPL-2.1-or-later (GPL and version3 disabled), mpv LGPL-2.1-or-later (gpl=false)
INFO

printf 'built %s\n' "$frameworks/libmpv.2.dylib"
du -sh "$frameworks" | sed 's/^/  size: /'
ls "$frameworks" | sed 's/^/  /'
