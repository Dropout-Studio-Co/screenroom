# Licensing and redistribution policy

Checked 2026-11-02 against the resolved dependency graph on this host, and updated
2026-11-03 for the subtitle provider change and the HDR retirement
([ADR-005](ADR-005_SUBTITLE_PROVIDER_AND_FEATURE_REMOVALS.md)). This records
evidence and release requirements. It grants no new rights in third-party code,
brands, codecs, media or private design material.

## First-party source

Screenroom's original first-party code is **MIT**, Copyright (c) 2026 Dropout
Studio, per the ROOT `LICENSE` file. Third-party files keep their own notices.

## Resolved Rust dependency graph

`cargo metadata --format-version 1` reports **550 packages** in the desktop
workspace. License expression distribution (top rows):

| License expression | Packages |
| --- | --- |
| MIT OR Apache-2.0 | 230 |
| MIT | 140 |
| Apache-2.0 OR MIT | 42 |
| MIT/Apache-2.0 | 22 |
| Unicode-3.0 | 18 |
| Zlib OR Apache-2.0 OR MIT | 15 |
| Apache-2.0 | 14 |
| MIT OR Apache-2.0 OR Zlib | 9 |
| BSD-3-Clause | 7 |
| ISC | 4 |
| BSL-1.0 | 2 |
| ISC | 2 |

Principal direct components:

| Component | Version | License (declared) |
| --- | --- | --- |
| iced / iced_wgpu | 0.14.0 | MIT |
| wgpu / naga | 27.0.1 / 27.0.3 | MIT OR Apache-2.0 |
| rusqlite / libsqlite3-sys | 0.41.0 / 0.39.1 | MIT |
| serde, serde_json, sha2, thiserror, regex | current workspace pins | MIT OR Apache-2.0 |
| uuid | 1.29.0 | Apache-2.0 OR MIT |
| libloading | 0.9.0 | ISC |
| notify / notify-types | 8.2.1 / 2.2.0 | CC0-1.0 |

Obligations that follow: preserve MIT/BSD/ISC copyright notices, carry the Apache
2.0 notice and patent grant where used, and keep the zlib and BSL-1.0 notices that
apply to two compiled-in components. Apache-2.0 is compatible with an MIT-licensed
product as long as notices are retained; it does not relicense first-party code.

`uniffi` (MPL-2.0) is no longer planned: its only consumers were the iOS/Android
apps removed by [ADR-003](ADR-003_DESKTOP_AND_TV_SCOPE.md).

## Dependencies added for the online providers, and the retired HDR fork

Two changes in the 2026-11-02 session brought new third-party code in, and both are
permissive and compatible with an MIT-licensed product.

**TLS for the metadata and subtitle providers.** `rustls 0.23` with the `ring`
provider, plus `rustls-pki-types`, `rustls-webpki`, `ring`, `subtle`, `zeroize`,
`untrusted`, `getrandom` and `webpki-roots`, and `rcgen` as a test-only dependency.
Licences: Apache-2.0 OR ISC OR MIT (`rustls`), MIT OR Apache-2.0 (most of the rest),
ISC (`rustls-webpki`), Apache-2.0 AND ISC (`ring`), BSD-3-Clause (`subtle`), and
**CDLA-Permissive-2.0** for `webpki-roots`, which is a data licence covering a
certificate list rather than code. That is the first CDLA-licensed component in this
graph; it is permissive, imposes no source obligation, and is recorded here because a
release notice should name it.

**A vendored `iced_wgpu` for the HDR swapchain — removed.** `vendor/iced_wgpu` was
upstream `iced_wgpu` 0.14.0, MIT as declared in its own `Cargo.toml` (upstream ships
no separate LICENSE file), carried under a root `[patch.crates-io]` entry. Only
`Cargo.toml` and `src/window/compositor.rs` differed. Its **only** purpose was
selecting a 16-bit float swapchain for HDR. HDR was removed on 2026-11-03
([ADR-005](ADR-005_SUBTITLE_PROVIDER_AND_FEATURE_REMOVALS.md)), so the fork lost its
only consumer: the directory, the `[patch.crates-io]` entry and its
`SCREENROOM_HDR` gate were deleted in the same change, and `iced_wgpu` now resolves
from crates.io 0.14.0 with the ordinary MIT obligation recorded in the table above.
Nothing is vendored for it any more, so no patched source needs to ship with a
binary release.

## Native media components: what ships, and under what licence

This is the sharpest release constraint in the project, and it now has a build
that resolves it.

| Component | Where it comes from | Distribution status |
| --- | --- | --- |
| `libmpv.2.dylib` + the FFmpeg and libplacebo libraries beside it | `scripts/build-lgpl-media.sh`: **mpv 0.41.0** built with `-Dgpl=false`, **FFmpeg 7.1.1** configured with `--disable-gpl --disable-version3 --disable-nonfree`, and **libplacebo 7.387.0** built OpenGL-only with Vulkan, shaderc and lcms disabled | **Redistributable.** LGPL-2.1-or-later, shipped as separate dynamic libraries so they can be replaced |
| Homebrew `mpv 0.41.0` / `libmpv.2.dylib` | `brew install mpv` | **Development only.** GPL-2.0-or-later build. `scripts/package-macos.sh` still accepts it for a local bundle and prints that it is not redistributable |
| Homebrew `ffmpeg 9.0.2` | `brew install ffmpeg` | **Not shipped at all.** The app invokes whatever `ffmpeg`/`ffprobe` the machine already has, for posters and conversions; nothing is copied into the bundle |
| libass, FreeType, HarfBuzz, FriBidi, Fontconfig and their codecs | Homebrew, linked by mpv's subtitle renderer | Permissive or LGPL. Copied into `Contents/Frameworks` with their install names rewritten to `@rpath`, so the set is self-contained |
| SQLite engine | public domain ([sqlite.org/copyright](https://www.sqlite.org/copyright.html)) | Bundled through `rusqlite`'s `bundled` feature; wrapper notices still apply |
| Sparkle 2.10.1 | pinned release archive, MIT | See the next section |

### The LGPL build

`scripts/build-lgpl-media.sh` fetches both sources at pinned SHA-256, builds them
into `vendor/lgpl/`, copies the non-system dependency closure into
`vendor/lgpl/frameworks/`, rewrites every install name to `@rpath`, and fails if
anything still points at `/opt/homebrew`. `vendor/lgpl/BUILD_INFO` records the
versions, flags and hashes of the exact build. `scripts/package-macos.sh` bundles
that directory when it exists and refuses to ship a stack that still resolves
through Homebrew.

Flags that matter for the licence: mpv `-Dgpl=false`; FFmpeg `--disable-gpl
--disable-version3 --disable-nonfree`. Encoders that would force GPL (x264, x265)
are not built; decoding, including HEVC, H.264, AV1, VP9, AAC and DTS, is native
FFmpeg code and unaffected. Conversion still uses the machine's own `ffmpeg`.

### Obligations that follow, and where they are met

- **Notice.** `Contents/Resources/licenses/` carries the FFmpeg LGPL text, the mpv
  copyright notice, the libplacebo and FFmpeg build patches where one was applied, and
  `LGPL-SOURCE-OFFER.txt`, the written offer described in
  [`LGPL_SOURCE_OFFER.md`](LGPL_SOURCE_OFFER.md). `COMPONENTS.txt` records the exact
  files and versions a given build bundled.
- **Corresponding source.** Every release attaches
  `Screenroom-media-source.tar.xz`: the FFmpeg, mpv and libplacebo archives at the
  pinned versions, `BUILD_INFO`, the build script and the mpv build patch, from the
  same download page as the binary. The remaining LGPL libraries (FriBidi, GLib,
  libintl, Graphite2) are covered by the written offer in
  [`LGPL_SOURCE_OFFER.md`](LGPL_SOURCE_OFFER.md), which ships inside the bundle.
- **Relinking and replacement.** mpv and FFmpeg are separate `.dylib` files in
  `Contents/Frameworks`, not statically linked, so the owner can replace them with
  a compatible build. No anti-circumvention measure is applied.
- **No GPL leakage.** `brew install mpv` must never be what a published bundle
  carries; the packaging script says so, and the release workflow builds the LGPL
  stack and asserts the bundle is self-contained before it archives anything.

For every shipped native binary the record is: version, SHA-256 of the source,
complete configure/meson flags, the dependency closure, and the source itself. Do
not ship a GPL-linked binary behind MIT-only notices.

Codec patents and platform store terms are separate from open-source copyright.

## The update framework

**Sparkle 2.10.1** (MIT) is embedded as `Contents/Frameworks/Sparkle.framework`,
fetched by `scripts/fetch-sparkle.sh` at a pinned SHA-256 and never committed. The
framework carries its own `LICENSE`, which ships inside the bundle; Sparkle's MIT
notice must stay with it. It brings no GPL obligation and no service dependency.

Sparkle's tools (`generate_appcast`, `sign_update`, `generate_keys`) run only on the
build machine. They are not shipped in the application.

The update *contents* are Screenroom itself, so the libmpv obligation below applies
to everything Sparkle delivers exactly as it applies to the first download. An
updater does not launder a licence.

The GPL obstacle is gone: `v0.1.0` is published with the LGPL stack described
above, together with its corresponding source. A released bundle must keep using
`scripts/build-lgpl-media.sh`; `scripts/package-macos.sh` refuses to archive a stack
that still resolves through Homebrew, and the workflow asserts the same before
signing.

## TV clients

The Lightning 3 renderer and Blits are MIT-licensed JavaScript packages
(`@lightningjs/core`, `@lightningjs/blits`, declared on npm). The Bundled JavaScript
still carries its dependency notices, and the TV platform SDK/runtime components
(e.g. Samsung AVPlay, webOS media pipeline) have their own vendor terms. An npm
dependency's permissive license is not permission to redistribute a platform SDK.

## Private Dropout Design System

The private DS remains its owner's proprietary material. The approved private
foundations pin is `41406ab436ef2b9c1edcf69e9d4ada4b94d13eec`.

- No MIT grant is inferred for its source, tokens, generated values, assets or any
  derived adapter.
- It stays ignored locally (`/vendor/dropout-design-system/`, `/src/lib/ds/`,
  `**/design/dropout_tokens.g.dart`) and never appears in a public package, issue,
  screenshot bundle or planning archive.
- Each platform maps the same semantic roles through its own presentation layer.
  A public build must not require the private source: the mapping sits behind an
  interface with a neutral fallback, and that interface is an M3 deliverable.
- Publishing private derived values without rights is prohibited even when the
  values are "just colours".

## Assets, providers and user content

- TMDB branding (`tmdb-logo.svg` in the archive) remains TMDB branding, governed by
  TMDB's logo/attribution rules; it is not Screenroom artwork. If TMDB data is used,
  the required attribution sentence stays visible.
- Provider APIs (TMDB, The Subtitle DB) grant API use, not rights to redistribute
  other people's images or subtitle works. Indefinite offline retention of provider
  artwork is unresolved and is treated as a release blocker for automatic remote
  caching; user-permitted local image bytes with provenance are the lawful path.
  A subtitle downloaded from The Subtitle DB is kept for the owner's own playback,
  with its provenance, and is never re-shared or redistributed; see
  [`SUBTITLE_PROVIDER.md`](SUBTITLE_PROVIDER.md) §9. The provider was OpenSubtitles
  before 2026-11-03 (ADR-010, superseded); the same retention rule applied to it.
- User videos, databases, posters and screenshots never become project assets
  because the app can open them. Test media must be generated or explicitly
  rights-cleared; the libmpv smoke test generates its own clip with FFmpeg.

## Release checklist (per package)

1. Dependency notices regenerated from the resolved lockfile of that build.
2. Native binaries pinned by hash with a recorded build configuration.
3. LGPL/GPL posture decided and documented; MPL files unchanged or published.
4. Private DS material confirmed absent from the artifact.
5. No credentials, certificates, DUIDs, personal paths or user media in the bundle.
6. Attributions visible where the provider's terms require them.

Review this document on every SDK, dependency, asset or native-build change.
