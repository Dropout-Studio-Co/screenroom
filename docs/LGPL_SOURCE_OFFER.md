# LGPL source offer for the bundled media libraries

Screenroom's macOS application ships dynamic libraries that are licensed under the
GNU Lesser General Public License, version 2.1 or later. They are separate files in
`Screenroom.app/Contents/Frameworks`, not statically linked into the application, so
they can be inspected and replaced.

This document is the written offer required by LGPL-2.1 §4(d). It is included in
every application bundle as `Contents/Resources/LGPL-SOURCE-OFFER.txt` and published
with every release.

## What is bundled

| Library | Licence | Where the source is |
| --- | --- | --- |
| mpv (libmpv) 0.41.0 | LGPL-2.1-or-later (`-Dgpl=false`) | attached to the release as `Screenroom-media-source.tar.xz` |
| FFmpeg 7.1.1 (libavcodec, libavformat, libavutil, libavfilter, libswscale, libswresample, libavdevice) | LGPL-2.1-or-later (`--disable-gpl --disable-version3 --disable-nonfree`) | attached to the release as `Screenroom-media-source.tar.xz` |
| libplacebo 7.387.0 (`-Dvulkan=disabled`, OpenGL only) | LGPL-2.1-or-later | attached to the release as `Screenroom-media-source.tar.xz` |
| FriBidi 1.0.18 | LGPL-2.1-or-later | on request, and from <https://github.com/fribidi/fribidi> |
| GLib 2.90.1 | LGPL-2.1-or-later | on request, and from <https://gitlab.gnome.org/GNOME/glib> |
| gettext (libintl) | LGPL-2.1-or-later | on request, and from <https://www.gnu.org/software/gettext> |
| Graphite2 1.3.14 | LGPL-2.1-or-later or MPL-2.0 | on request, and from <https://github.com/silnrsi/graphite> |

The remaining bundled libraries — libass (ISC), FreeType (FTL), HarfBuzz (MIT),
libjpeg-turbo (IJG/BSD), Little CMS (MIT), PCRE2 (BSD), libpng (libpng), SDL2 (zlib),
shaderc (Apache-2.0), Vulkan Loader (Apache-2.0), libunibreak (zlib), zimg (WTFPL),
Brotli (MIT), and the X11/XCB libraries (MIT) — are permissive and carry no source
obligation. Their notices are the ones their own distributions ship.

`vendor/lgpl/COMPONENTS.txt` in the build output lists the exact files, versions and
formula paths of the closure that a given build produced.

## The offer

For a period of three years from the date a release is published, the owner of this
repository will supply, to any third party who asks, a complete machine-readable copy
of the Corresponding Source for the LGPL libraries listed above, on the same terms as
the binary was distributed (no charge beyond the cost of the medium and delivery).

Ask by opening an issue at <https://github.com/Dropout-Studio-Co/screenroom/issues>
with the release version and the library you want.

mpv, FFmpeg and libplacebo need no request: their exact source archives, the build
script and `BUILD_INFO` (versions, SHA-256, configure and meson flags) are attached
to the same release as the application.

## Replacement and relinking

Every library is a separate `.dylib` in `Contents/Frameworks` with its install name
rewritten to `@rpath` and an `@loader_path` run path, so a compatible build of any of
them can replace the bundled copy without touching the application binary. No
anti-circumvention measure, signature requirement on those files beyond the
application's own ad-hoc signature, or technical restriction prevents that.

## What was deliberately not built

FFmpeg is configured without GPL code, without `--enable-version3` and without
`--enable-nonfree`. The GPL-only encoders (x264, x265) and the GPL-only filters are
therefore absent. Screenroom decodes with native FFmpeg code; its optional local
conversion calls the `ffmpeg` binary the machine already has, which is not part of
the bundle.
