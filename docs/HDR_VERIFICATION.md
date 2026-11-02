# HDR without a display: what was measured, what still needs eyes

- **Date:** 2026-11-02
- **Scope:** F07, Q02, Q03, P1-03 of
  [`MACOS_REMAINING_WORK.md`](MACOS_REMAINING_WORK.md), gate 2 of
  [`FEASIBILITY_GATES.md`](FEASIBILITY_GATES.md)
- **Machine:** the owner's MacBook Air M4, macOS 27.0.1, built-in panel in a
  2940×1912 mode, `maximumPotentialExtendedDynamicRangeColorComponentValue` 2.0
- **Decision:** [`ADR-019_HDR_PRESENTATION.md`](ADR-019_HDR_PRESENTATION.md)

Everything below ran headless or offscreen. No window was raised, no space was
switched, and the frontmost application was unchanged (`lsappinfo front` same
before and after). Nothing here says the panel *looks* right; that is the one
step left, and §6 says exactly what to look at.

## 1. The fixtures

`.artifacts/media` had no HDR file in this worktree, so two were generated from
FFmpeg's own sources — 10-bit HEVC Main10, tagged PQ/bt.2020, rights-free:

```bash
python3 - <<'PY'
import struct
W, H, levels = 4096, 1080, 1024
per = W // levels
row = b''.join(struct.pack('<H', (x // per) << 6) for x in range(W))
open('/tmp/ramp.gray16', 'wb').write(row * H)
PY

ffmpeg -f rawvideo -pix_fmt gray16le -s 4096x1080 -r 24 -i /tmp/ramp.gray16 \
  -frames:v 1 -vf format=yuv420p10le -c:v libx265 -preset ultrafast \
  -x265-params "log-level=error:lossless=1:colorprim=bt2020:transfer=smpte2084:colormatrix=bt2020nc:range=full:master-display=G(8500,39850)B(6550,2300)R(34000,16000)WP(15635,16450)L(10000000,1):max-cll=1000,400" \
  -tag:v hvc1 hdr10-steps-4k.mp4
```

The result is a **1024-step 10-bit wedge**, 4 pixels per step, so a
one-step-per-column pattern stands in for real 10-bit gradation and every
quantisation stage is visible as a lost level. `ffprobe` confirms
`pix_fmt=yuv420p10le`, `color_transfer=smpte2084`,
`color_primaries=bt2020`, `color_range=pc`, `profile=Main 10`.

Controls: the same wedge tagged `bt709` (SDR 10-bit), and a 1920×1080 24 fps
ten-second PQ clip from `testsrc2` for a sustained run.

## 2. Does the path engage? — yes, to the compositor

`cargo run --release -p screenroom-player --example hdr_check -- <file> 2 all`
does three passes over one file in one process: `sdr` (8-bit), `float8` (a
half-float surface **without** any HDR request), and `extended` (half float plus
the tone map). On the 10-bit PQ wedge:

| | `sdr` | `float8` (control) | `extended` |
|---|---:|---:|---:|
| Surface fourcc | `BGRA` `0x42475241` | `RGhA` `0x52476841` | `RGhA` `0x52476841` |
| Bits per channel | 8 | 16 | 16 |
| `GL` internal format | `0x8058` | `0x881a` | `0x881a` |
| Extended range requested | no | no | **yes** |
| Distinct green values | 248 | 469 | **421** |
| Samples off the 8-bit grid | 0 | 70 680 | **90 830 of 90 830** |
| Peak green code | 1.000000 | 1.000000 | 0.653809 |
| Peak luma | — | — | **404.6 nits** |
| Peak vs reference white | — | — | **1.993×** |
| Above reference white | — | — | 35 960 of 90 830 |
| Display target | — | — | 406 nits (2.0 × 203, measured) |

The `float8` control is the reason "off the grid" is not claimed as HDR by
itself: it is off the grid (70 680 samples) while carrying no PQ and never
exceeding 1.0.

The player's own published frame, not just the example's own FBO, changes with
the switch too (`hdr_check … player` starts `Player::start_with_hdr`):

```
extended  → output "16 bits per channel, pq/bt.709, tone mapping bt.2390, target peak 406 nits"
            frame_format RGBA16Float, frame_bytes_per_pixel 8, extended true
sdr       → output "8 bits per channel, srgb, tone mapping mpv default"
            frame_format BGRA8, frame_bytes_per_pixel 4, extended false
```

## 3. Is ten bits preserved end to end? — yes, and where it would break

The chain and the format at each hop, read from the code and confirmed by the
receipt:

1. libmpv decodes 10-bit HEVC and renders into a `GL_RGBA16F` FBO, told the
   depth with `MPV_RENDER_PARAM_DEPTH = 16`.
2. The surface is a CoreVideo `kCVPixelFormatType_64RGBAHalf` `IOSurface`
   (`'RGhA'`, 8 bytes per pixel, 32 768-byte row at 4096) — read back, not
   assumed.
3. `apps/desktop/src/video_surface.rs` imports that `IOSurface` as
   `MTLPixelFormatRGBA16Float`/`wgpu::TextureFormat::Rgba16Float` **without an
   sRGB view** (`texture_formats` and `texture_from_surface`).
4. The picture shader decodes PQ and encodes extended sRGB in `f32`; the
   swapchain is `Rgba16Float` when the switch is on
   (`vendor/iced_wgpu` `select_format`), `Bgra8Unorm` when it is off.

A ten-bit source therefore has 16 bits of container at every hop. The
measurement that proves it is not an 8-bit pipeline wearing a float container:
the wedge asked for 585 distinct input levels across the sampled columns and
delivered **421 distinct green values, every sample off the 8-bit grid**, against
248 on the 8-bit path and a hard ceiling of 256. The `float8` control separates
"container" from "signal"; the `extended` pass separates "signal" from "8-bit
precision".

**Where a truncation would show:** a lost step becomes a repeated value. The
same test therefore fails if a future change routes the picture through an
8-bit texture, coincides with an 8-bit grid, or drops below 256 distinct values —
`hdr_check` returns non-zero on all three.

**One place that was wrong, now fixed.** The CPU upload path in
`video_surface.rs` always created an `Rgba8Unorm` texture, whatever the frame's
format, and only rebuilt it when the frame *size* changed. A half-float frame
delivered without an `IOSurface` would have been written into an 8-bit texture
(and a format change at the same size would have kept the old texture). The
texture format now follows the frame format
(`upload_format`, `texture_format`), with a unit test. The shipped macOS HDR path
was never affected because it carries an `IOSurface` and returns before the CPU
upload; this closes a latent path rather than a visible one.

## 4. What the tone map actually does — and that the setting is not a label

- Reference white is **203 nits** (`REFERENCE_WHITE_NITS`), the standard SDR
  reference this renderer and the shader share.
- The headroom is read once from
  `NSScreen.maximumPotentialExtendedDynamicRangeColorComponentValue` on the main
  thread (`display::extended_range_target`). On this machine it is **2.0**, so
  the target peak is **406 nits**, reported as `measured`.
- libmpv is given `target-prim=bt.709`, `target-trc=pq`, `target-peak=406`,
  `tone-mapping=bt.2390`, `hdr-compute-peak=no` (`gl_render::extended_range_values`).
- Delivered: **404.6 nits peak, 1.993× reference white** against a 406-nit
  target and a 2.0 ceiling. The tone map is reached, not merely requested.

**The setting changes the pipeline, not a label.** `Preferences::hdr_extended`
is read at startup by `apply_hdr_preference()` and turned into
`SCREENROOM_HDR=extended`, which the player (frame format) and the vendored
compositor (swapchain format) both read. Proven end to end, hidden, with the
persisted preference and **no environment variable**:

| `hdrExtended` | Swapchain, printed by the app | Delivered fps | CPU mean | Uploads |
|---|---:|---:|---:|---:|
| `true` | **`Rgba16Float`** | 22.910 | 34.39 % | 0 |
| `false` | **`Bgra8Unorm`** | 21.816 | 32.46 % | 0 |

Those CPU figures were sampled while other builds were running on the same
machine, so they overstate the application's own cost (an earlier quiet run of
the same pair read 18.87 % and 16.84 %); the frame rate is stable across both.
The frontmost application was unchanged before and after every run
(`lsappinfo front`), and no window was shown.

The toggle's own copy is honest about the one caveat: the window picks its
surface when it opens, so it says "restart to apply", and it really does
require the restart.

## 5. Can the picture be checked without a display? — yes, offscreen

Three layers, none of which needs a monitor:

1. **The delivered range** — `hdr_check`, §2. Surface format, internal format,
   distinct levels, off-grid samples, nits from the ST 2084 EOTF.
2. **The shader itself** — new offscreen tests in `video_surface.rs` render a
   half-float `IOSurface` through the real `VideoPipeline` into a 64×64
   `Rgba16Float` target and read the pixels back. For known PQ codes:

   | PQ code | Light | Expected code | Rendered |
   |---|---:|---:|---:|
   | `0x3800` | 92.2 nits | 0.7045 | **0.7041** |
   | `0x38a5` | 202.8 nits | 0.9995 | **0.9990** |
   | `0x38fa` | 300.6 nits | 1.1875 | **1.1865** |
   | `0x393c` | 406.5 nits | 1.3539 | **1.3535** |

   The tests assert reference white lands at 1.0, the measured 2× peak lands at
   about 1.354 (above 1.0 — that *is* the extended range), and two PQ codes one
   ten-bit step apart stay distinct. A grey ramp that decodes to the wrong
   transfer function, a truncation to 8 bits, or a shader that clips at 1.0 all
   fail these tests.
3. **The whole application** — the hidden-window play check, §4, which prints
   the swapchain format the compositor actually selected.

## 6. What only the owner can settle

The delivered headroom cannot be read back while EDR content is on screen
headlessly, and no agent may say a film looks right. On the built-in panel:

1. Play an HDR10 film (a real one, not the synthetic wedge).
2. Settings → Playback → "Show HDR films as HDR (restart to apply)" on, quit,
   relaunch.
3. Judge, in this order:
   - highlights (a lamp, sky, fire) brighter than SDR white without turning
     into flat white;
   - shadows still black, not lifted grey;
   - skin tones natural, not washed out or oversaturated — note that gamut is
     intentionally BT.709, so very saturated colours will look *less* wide than
     a TV in Filmic/Vivid mode;
   - the interface itself unchanged (same greys and accents as SDR) — if the UI
     shifts, the layer colour space is wrong, not the film;
   - the same scene with the switch off: highlights dimmer, mid-tones the same.
4. Record the result in `MACOS_ACCEPTANCE_EVIDENCE.md` under "Owner session".

## 7. What is still not claimed

- **BT.709 gamut.** One drawable carries one colour space and the UI owns it, so
  the picture is gamut mapped rather than shown in the panel's full gamut.
- **The nits are arithmetic, not a display measurement.** They are the ST 2084
  EOTF applied to delivered codes; only the panel's own EDR report and the
  owner's eyes can confirm what the panel emits.
- **The fixture is synthetic.** A PQ wedge proves the pipe and the precision; it
  is not a graded master.
- **HLG stays on the SDR path**, deliberately (`gamma_is_extended_range` accepts
  only `pq`).
- **Windows** cannot be checked from macOS; gate 4 stays BLOCKED.
- **One UI wording gap, reported not edited.** The Playback settings card states
  "16 bits per channel · pq / bt.709 · tone mapping bt.2390" in the present
  tense even while the switch is off and the picture is 8-bit sRGB. The About
  page's "Picture" row is the live readout (`snapshot.output.label()`), and it
  is honest. The card lives in `apps/desktop/src/ui/screens/settings.rs`, which
  another change owns, so this document records the mismatch rather than
  editing it.

## Reproduce

```bash
export CARGO_TARGET_DIR=$HOME/.cache/sr-wt/hdr/target
cargo run --release -p screenroom-player --example hdr_check -- \
  .artifacts/media/hdr10-steps-4k.mp4 2 all
cargo test -p screenroom-desktop video_surface
```

The application-side check needs a library containing the fixture with
`hdrExtended` set in its `settings` row, then:

```bash
SCREENROOM_LIBRARY=<library.db> \
SCREENROOM_PLAY_CHECK=<report.csv> \
SCREENROOM_PLAY_CHECK_MEDIA=<absolute fixture path> \
SCREENROOM_PLAY_CHECK_SECONDS=8 \
SCREENROOM_WINDOW=hidden \
./target/release/screenroom-desktop
```

`picture_surface` in the report is the swapchain format the compositor selected.
