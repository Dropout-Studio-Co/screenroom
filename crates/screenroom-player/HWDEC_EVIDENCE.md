# macOS software-renderer hardware decode check

Run on the owner's Apple M4 MacBook Air with mpv/libmpv 0.41.0 (client API
2.5). Audio was muted. The three files were opened read-only, one process at a
time. Each playback row covers 20 seconds after the first non-blank frame. Frames
were delivered at native resolution up to a 3840-pixel maximum dimension after the
2026-11-01 change; the rows below were recorded before it, when the smaller 1920-pixel
cap was still in place, and their dimensions are the delivered RGBA sizes of that day.

| Sample | Mode requested | First frame (ms) | Delivered fps | Worst gap (ms) | CPU mean / max (%) | RSS mean / max (MiB) | `hwdec-current` | Non-blank | Mean luma |
|---|---:|---:|---:|---:|---:|---:|---|---|---:|
| 4K HEVC Main10 sample A (3240×2160) | no | 2496 | 13.549 | 249.83 | 343.78 / 454.30 | 737 / 743 | no | yes | 0.1839 |
| 4K HEVC Main10 sample A (3240×2160) | videotoolbox-copy | 2137 | 18.149 | 111.74 | 164.48 / 176.80 | 475 / 536 | videotoolbox-copy | yes | 0.1838 |
| 4K HEVC Main10 sample A (3240×2160) | auto-copy | 2103 | 19.449 | 150.41 | 165.08 / 174.60 | 486 / 498 | videotoolbox-copy | yes | 0.1840 |
| 4K HEVC Main10 sample B (3840×1600) | no | 2138 | 23.949 | 51.68 | 65.09 / 82.00 | 551 / 554 | no | yes | 0.7745 |
| 4K HEVC Main10 sample B (3840×1600) | videotoolbox-copy | 2135 | 23.950 | 57.74 | 51.47 / 69.10 | 386 / 391 | videotoolbox-copy | yes | 0.7744 |
| 4K HEVC Main10 sample B (3840×1600) | auto-copy | 2130 | 23.949 | 57.36 | 56.76 / 73.60 | 387 / 393 | videotoolbox-copy | yes | 0.7744 |
| 1080p H.264 sample C (1804×1080) | no | 2300 | 23.949 | 52.44 | 30.13 / 34.30 | 187 / 217 | no | yes | 0.0173 |
| 1080p H.264 sample C (1804×1080) | videotoolbox-copy | 2394 | 23.949 | 53.63 | 20.38 / 26.40 | 135 / 179 | videotoolbox-copy | yes | 0.0173 |
| 1080p H.264 sample C (1804×1080) | auto-copy | 2408 | 23.950 | 51.92 | 20.75 / 24.40 | 150 / 177 | videotoolbox-copy | yes | 0.0173 |

CPU and RSS are one-second `ps -o %cpu,rss -p <pid>` samples. macOS `ps %cpu`
is a process-level percentage, not a battery or energy measurement. The corrected
serial rerun occurred under mixed host load, including unrelated high-CPU
processes, so these are single-run observations rather than controlled energy
measurements. Luma is the mean Rec. 709 weighted RGB value normalized to 0–1
from the sampled 20-second frame PNG. The PNGs are outside Git in the local
evidence directory alongside the raw per-run logs and `results.csv`.

## Decision

Use `auto-copy` by default on macOS; retain `no` as the default on platforms
without measurements. On sample A, automatic copy-back reduced sampled CPU by
52.0% and mean RSS by 34.0%, with effectively identical luma (0.1839 vs
0.1840). It delivered 19.45 fps against the source's 23.976 fps, so it did not
meet real-time delivery in this run. Sample B remained at 23.95 fps while CPU
fell 12.8% and mean RSS fell 29.7%; sample C remained at 23.95 fps while CPU
fell 31.1% and mean RSS fell 19.9%. All sampled images had expected delivered
dimensions and non-black RGB pixels. `auto-copy` selected
`videotoolbox-copy` on this Mac for all three files. Given materially lower CPU
and memory use on all samples, matching luma, and the built-in software fallback,
the macOS default is still useful, with sample A's dropped-frame result as a
known limitation requiring further measurement under controlled load. A
generated MPEG-2 regression case outside mpv's default hardware codec whitelist
asserts that `auto-copy` reports `hwdec-current=no` and still delivers a frame.

Callers can use `Player::start_with_hwdec(HwdecMode::No)` to force software
decoding. `SCREENROOM_HWDEC=no` provides the same override to existing desktop
launches. The existing desktop `Player::start()` call needs no wiring change;
on macOS it now selects the measured policy.

## Idle worker

| Worker | CPU mean / max (%) | RSS mean / max (MiB) | Worker wakeups in 30 s |
|---|---:|---:|---:|
| Before: 20 ms timeout poll | 0.00 / 0.00 | 43.5 / 54.8 | 1,500 timeout wakes (50/s, from the loop interval) |
| After: callbacks + condition variable | 0.00 / 0.00 | 43.4 / 54.1 | 0 notifications during the sample |

Both CPU readings are below `ps`'s observed resolution, so this run does not
establish an idle CPU reduction. It does establish that the updated loop had no
command/event/render notifications during the 30-second idle sample, instead of
its old 1,500 timeout wakeups. The idle example prints the notification count
and samples process CPU/RSS once per second.

## API references

- mpv 0.41 manual, **Video → `--hwdec`**, including `auto-copy`, safe fallback,
  and the `videotoolbox-copy` system-memory description.
- mpv 0.41 manual, **Video → `--hwdec-codecs`** and **Properties →
  `hwdec-current`**. `no` indicates software decoding; the property is
  unavailable if no decoder is loaded.
- `/opt/homebrew/include/mpv/client.h` 0.41: `mpv_event_property` defines
  `MPV_FORMAT_STRING` event payload as pointer-to-pointer; wakeup callback
  section documents draining `mpv_wait_event()` until `MPV_EVENT_NONE`.
- `/opt/homebrew/include/mpv/render.h` 0.41: **Software renderer** describes
  `MPV_RENDER_API_TYPE_SW`; the render update callback is a notification, and
  render API functions must not be called from inside it.

Online manual sections: [Video options (`--hwdec`, `--hwdec-codecs`)](https://mpv.io/manual/stable/#video)
and [Properties (`hwdec-current`)](https://mpv.io/manual/stable/#properties).

## Where the 4K frame time actually goes (2026-11-01)

Offscreen, `render_bench 3240x2160` (H.264 fixture, `SCREENROOM_PLAYER_TRACE=1`,
Apple M4, 1920×1280 delivered, stride 7680):

| Stage | Per frame | Share |
|---|---:|---:|
| `mpv_render_context_render` (swscale YUV→RGB and the 3240→1920 downscale) | **39.5–45.9 ms** | ~98% |
| `SoftwareRenderer::pack` (row copy into the pooled buffer) | 0.27–1.12 ms | ~1–2% |
| `queue.write_texture` upload in the iced/wgpu surface | not measured offscreen | — |

Delivery was 162 frames in 7.00 s (23.1 fps) for the whole run, so the offscreen
ceiling at this size is already below the 24 fps target before any on-screen
presentation cost is added.

`--sws-scaler` is **not** the lever. The same run repeated with `bilinear`
measured 39.5 ms mean per frame over 177 frames (24.0 fps delivered) and with
`bicublin` 39.6 ms over 171 frames (23.1 fps), against 39.5–45.9 ms for the
default. The cost is the format conversion of a 3240×2160 10-bit YUV frame, not
the resampling filter, so no filter choice recovers real time. The option is
also unsafe to expose as an env knob: an empty or unknown value fails
`mpv_set_option_string` and aborts player startup, so the trial plumbing was
reverted rather than shipped.

Consequences for the next attempt:

- The Rust side is not the bottleneck; `pack` and the pooled buffers are already
  ~1 ms per frame and cannot yield a 24% improvement.
- The software render API offers only RGB output formats (`rgb0`, `bgra`, …), so
  the CPU conversion cannot be skipped inside it.
- Reaching 24 fps at 1920×1280 therefore needs a zero-copy GPU path
  (`MPV_RENDER_API_TYPE_OPENGL`, or a Metal/IOSurface import) so that mpv
  converts on the GPU. That is an architecture change and was deliberately not
  attempted here.
- The measured trade-off if real time is preferred over resolution: cost tracks
  output pixels, so a smaller delivered size buys frames directly. It was not
  adopted because it lowers picture quality silently.

## Correction (2026-11-01): the 39 ms was frame pacing, not colour conversion

The section above is **superseded**. It timed `mpv_render_context_render` during
paced 24 fps playback, and that call blocks until the next frame is due, so the
"40 ms per frame" was mostly *waiting* rather than pixel work: the same traces
contain individual renders of 2.8 ms. Re-measured with `untimed=yes` (mpv decodes
and renders as fast as it can, so nothing waits), same Apple M4, same libmpv
0.41.0, generated 3840×2160 24 fps fixtures, `render_bench` with
`SCREENROOM_PLAYER_TRACE=1`:

| Target delivered | Render p50 | Render mean | Source | Delivered when paced |
|---|---:|---:|---|---:|
| 1920×1080 (the old cap) | 3.17 ms | 20.52 ms | 4K H.264 8-bit | 24.0 fps |
| 3840×2160 (native) | 11.79 ms | 22.19 ms | 4K H.264 8-bit | 24.2 fps |
| 1280×720 | 3.34 ms | 20.71 ms | 4K H.264 8-bit | 24.0 fps |
| 1920×1080 (the old cap) | 2.74 ms | 18.25 ms | 4K HEVC Main10 | 23.8 fps |
| 3840×2160 (native) | 5.97 ms | 19.78 ms | 4K HEVC Main10 | 23.7 fps |

`mistimed-frame-count` was 0 in every paced run. `--sws-scaler`,
`--zimg-scaler`, `--zimg-dither` and `--sws-allow-zimg` changed the paced figure
by less than 1 ms (39.0–39.9 ms), which is what identified a wait rather than a
filter: a real scaler cost would have moved. Real conversion and scaling is
**3 ms at 1080p out and 6–12 ms at native 4K**, against a 41.7 ms budget at 24 fps
One hold of ~280–300 ms appears once shortly after a load in most runs, at every
output size and with `hwdec=no`. It is **not** periodic and not decode or demux
scheduling; its cause is established in the section at the end of this file.

Consequences, implemented in this change:

- `render::MAX_DIMENSION` is **3840**, so a 4K source is delivered at its own
  size (3240×2160 stays 3240×2160). That costs about 8 ms more per frame and loses
  no frames, where the old 1920 cap silently discarded 4K detail for nothing.
- Frame rows are **256-byte aligned** (`row_stride`), so `queue.write_texture`
  receives the frame directly: the upload is legal at any width (3240 needs 13056
  bytes per row, which 64-byte alignment did not guarantee) and `pack` is a single
  memcpy instead of a per-row copy.
- Browse previews ask for at most 960 px, so a 4K card preview does not allocate
  4K frame buffers. `SCREENROOM_RENDER_MAX` overrides the delivered size for A/B
  measurement.

Still not proven: **on-screen** 4K presentation (no window was opened), HDR tone
mapping, and the owner's 3240×2160 Main10 sample, because `/Volumes/bleh` was not
mounted and the run used generated fixtures instead.

The colour conversion itself cannot be removed here. `render.h` (0.41) says the
software backend takes formats "internally marked as RGB, has exactly 1 plane", and
this libmpv exports only `MPV_RENDER_API_TYPE_SW` and `..._OPENGL`. A GPU path
means an OpenGL render context plus an IOSurface handed to Metal, and wgpu exposes
no Metal device at the point iced draws, so true zero copy stays blocked; HDR also
stays untone-mapped on the software path, as `render.h` warns.

## The one-off hold after a load, and what it is not (2026-11-01)

The new `frame_stall` example reports every inter-frame gap with the playhead,
`avsync` and `mistimed-frame-count`, so a hold is placed in media time and not
only in wall time. Apple M4, libmpv 0.41.0, `auto-copy`, generated 3840×2160
24 fps fixtures, 8–10 s sampled per run:

| Fixture | Runs | Delivered | The hold |
|---|---:|---:|---|
| 4K H.264, no audio track | 2 | 23.98 fps | none above 48 ms |
| 4K H.264, mono AAC | 6 | 23.25–23.40 fps | 277–302 ms, playhead 0.333 s |
| 4K H.264, stereo AAC | 3 | 23.40 / 24.00 fps | 290 ms in one run, none in two |

`mistimed-frame-count` stayed 0 throughout and `avsync` at the hold was −0.0108 s,
the same value every time, so this is a scheduling hold and not an audio sync
error.

Eliminated as causes, each by measurement rather than reasoning:

| Suspect | Test | Result |
|---|---|---|
| `audio-buffer` priming | 0.05 / 0.1 / 0.2 / 0.5 s | 283.6 / none / 302.5 / 289.9 ms — no scaling |
| CoreAudio device open | `ao=null` | 279.6 ms — unchanged |
| Mono channel layout | `audio-channels=stereo` | 283.0 ms — unchanged |
| Colour conversion and scaler | output 1080p vs native 4K | appears at both sizes |
| Hardware decode | `hwdec=no` | appears too |
| mpv waiting for frames | mpv `--log-file` | mpv logs `[v][vo/libmpv] mpv_render_context_render() not being called or stuck.` twice during the hold |

The mpv log settles the direction: mpv is ready and *we* are not rendering.
`mpv_wait_event` returns in microseconds, so the wait is not libmpv's event
queue. Phase timing added to the play loop (behind `SCREENROOM_PLAYER_TRACE`)
attributes the block to event handling — 393 ms and 231 ms inside
`handle_event` — and the per-event timings name the offenders:
`FILE_LOADED` (6–378 ms), `PROPERTY_CHANGE` (136–326 ms),
`VIDEO_RECONFIG` (200–204 ms) and `PLAYBACK_RESTART` (230–238 ms).

**Cause.** Those handlers issue synchronous libmpv property reads — `duration`,
`hwdec-current`, the fifteen `track-list/*` reads, `dwidth`/`dheight`, and
`time-pos` — on the one thread that must also call
`mpv_render_context_render`. mpv holds its core lock while it builds the 4K video
chain and opens VideoToolbox, so the read waits, the due frame waits with it, and
about six frames (0.25 s at 24 fps) are lost once per load. That is the whole
difference between the 23.25 fps and 24.12 fps runs.

Servicing the render request before draining events did not help: the hold still
measured 302 ms, because the block begins before a frame is due. Loop order is
not the fix.

**Next action.** Remove the blocking reads from the render loop: observe
`dwidth`, `dheight`, `duration` and `hwdec-current` and take their values from
the events, take `time-pos` for `PLAYBACK_RESTART` from the last observed value,
and relax the `apply_property` gates so those events are not dropped before
`FILE_LOADED` arrives. `position_ready` must keep waiting for a real `time-pos`
value, or a fresh load can write position 0 over a resume point.

## HDR10 on the software path (2026-11-01)

Fixture: generated 3840×2160 24 fps HEVC Main10 with `bt2020nc`/`smpte2084`,
BT.2020 primaries and mastering metadata (1000-nit MaxCLL). Played with
`auto-copy`: **23.948 fps at native 3840×2160**, `hwdec-current=videotoolbox-copy`,
CPU mean 269.7 % / max 344.4 %, RSS mean 528 MiB / max 573 MiB.

One delivered frame compared against the same source rendered by `mpv -o`
(libplacebo tone mapping), both measured with `signalstats`. The mpv PNG is
16-bit and is divided by 257 to compare:

| Frame | YMIN | YLOW | YAVG | YHIGH | YMAX |
|---|---:|---:|---:|---:|---:|
| Screenroom delivered | 8 | 29 | 129.2 | 226 | 255 |
| mpv/libplacebo tone-mapped | 8.7 | 29.2 | 128.7 | 226.5 | 245.8 |

So the software path **does** receive an already tone-mapped SDR image — mpv tone
maps in its video chain, because the sw renderer targets 8-bit RGB — and it matches
the libplacebo reference to within 0.4 % of mean luma and 0.2 % at YHIGH. Only the
top of the range clips slightly (255 against 245.8).

What remains a limitation, stated plainly:

- The frame is 8-bit SDR RGB with no colour metadata, so HDR10 is never passed
  through: highlight luminance is compressed to the SDR range instead of being
  presented at its mastered level, and the window never asks macOS for an
  HDR/EDR surface.
- The tone-mapping curve is mpv's default. This app neither chooses nor labels it.
- There is no path to `target-colorspace-hint` or EDR from here. `render.h` (0.41)
  gives the sw backend single-plane RGB only, and this libmpv exports only `sw`
  and `opengl` render APIs.

Not proven: a real HDR10 film, an HDR-capable display, and how this looks to a
person. One frame of one generated fixture is the whole sample, and the owner's
media was not mounted.

## The post-load hold is fixed (2026-11-01)

The blocking reads named above are gone from the render loop:

- `dwidth` and `dheight` are observed (user data 13 and 14) and `update_target`
  renders from the cached pair instead of reading them.
- `FILE_LOADED` marks metadata dirty instead of reading `duration`,
  `hwdec-current`, the track list and the dimensions.
- `PLAYBACK_RESTART` takes the playhead from the last observed `time-pos`.
  `position_ready` becomes true only once a real value has arrived (via
  `awaiting_position`), so a fresh load still cannot publish 0 over a resume point.
- A `track-list` change marks metadata dirty; the refresh (tracks, duration, and
  `hwdec-current` as a fallback) runs in an iteration where no frame is pending.
- The gates in `apply_property` now let the load-time properties through before
  `FILE_LOADED`, so their event values are already cached when the file is loaded.

| Fixture, 8–20 s sampled | Before | After |
|---|---:|---:|
| 4K H.264, mono AAC | 23.25–23.40 fps, 277–302 ms hold | **24.00–24.12 fps, no gap ≥ 150 ms** |
| 4K H.264, stereo AAC | 23.40 fps with a 290 ms hold | **24.00 fps, no hold** |
| 4K H.264, no audio track | 23.98 fps | 24.12 fps |
| 4K HEVC Main10 HDR10 | 23.948 fps, worst gap 162 ms | **23.999 fps, worst gap 53 ms** |
| 1080p H.264 (`render_bench`) | 24.0 fps | 24.0 fps |

The worst `handle_event` left is the deferred refresh at 12–28 ms, under one frame
at 24 fps. Whole-workspace tests pass, clippy and rustfmt clean. Re-run with
`frame_stall <mode> <file> <seconds> 150` to confirm the absence of the hold, and
`SCREENROOM_PLAYER_TRACE=1` to see any slow iteration broken down by phase.

## The zero-copy GPU path, measured (2026-11-01)

The software renderer cannot go faster here: `render.h` gives it single-plane RGB
only, so mpv converts YUV to RGB on the CPU, and the application then uploads every
frame to the GPU. This section records the first isolated experiment of the
alternative — libmpv rendering through OpenGL into an `IOSurface` that Metal reads
directly — and what it actually delivered.

Probe: `examples/gl-path-probe/probe.m`, built by hand (its README carries the
command). It opens a headless `CGL` context, creates a 3840×2160 `IOSurface`,
binds it to a GL texture with `CGLTexImageIOSurface2D`, makes that texture an FBO
attachment, creates `mpv_render_context` with `MPV_RENDER_API_TYPE_OPENGL`, loads
the file, renders every frame mpv requests, and then wraps the same `IOSurface` in
a Metal texture to compare bytes. Nothing is shown, nothing is heard, and the
media is opened read-only.

**Resource sharing is real, not assumed.** `-[MTLDevice newTextureWithDescriptor:
iosurface:plane:]` returned a texture and `getBytes:` over a 16-pixel run at the
frame centre returned exactly the bytes OpenGL had written
(`metal read B=255 G=12 R=0 A=255, surface B=255 G=12 R=0 A=255, identical=1`),
so the picture crosses to Metal without a CPU copy.

Apple M4, libmpv 0.41.0, generated 3840×2160 24 fps fixtures from `.artifacts/media`,
`glFinish` after every frame, file looped, audio disabled:

| Fixture | Delivered, paced | CPU (user+sys / real) | Peak RSS |
|---|---:|---:|---:|
| 4K H.264 8-bit | 22.91 fps | 26.7 % | 261 MiB |
| 4K HEVC Main10 | 23.74 fps | 32.1 % | 498 MiB |
| 4K HEVC Main10 HDR10 | 23.62 fps | 30.5 % | 441 MiB |

The same files untimed (mpv decodes and renders as fast as it can, so nothing
waits) render at 65.1 and 71.0 fps, 14.7 and 13.7 ms per frame including the
`glFinish`.

Against the shipped software path, on the same machine and the same kind of
fixture (`SCREENROOM_PLAY_CHECK`, release build, 2026-11-01): 18.7–22.0 fps
delivered, CPU 116–186 %, RSS 380–561 MiB. So the GPU path delivers the source rate
instead of four frames in five, and uses roughly a quarter to a fifth of the CPU —
which is the battery and fan cost of a 4K film, not a cosmetic difference.

What is **not** proven, and therefore not claimed:

- **Nothing has been presented through iced and wgpu.** The probe writes into an
  `IOSurface` and reads it back once to prove the bytes; it never draws that
  surface onto the application's swap chain.
- **GL-to-Metal synchronisation is unproven without `glFinish`.** The numbers above
  pay a full serialisation per frame. A production loop needs a shared event or
  fence so the Metal sampling waits on the GL writes, and must keep the surface
  alive until the GPU is done with it.
- **HDR is unchanged.** The surface is 8-bit BGRA; an EDR path needs a 16-bit
  float target and an EDR-enabled window, neither of which exists here.
- **The owner's 3240×2160 Main10 sample was not measured**: `/Volumes` held only
  `mothership` and `Recovery`, so all media here is generated.

### Why the connection to the real UI is considered viable

Checked against the installed crates rather than assumed:

- `iced_wgpu`'s custom `Primitive::prepare` receives the live `&wgpu::Device`, and
  `Primitive::draw` receives the render pass, so a texture created from the
  `IOSurface` can be sampled inside the application's own pipeline.
- `wgpu` 27 exports `hal` and `Device::as_hal::<wgpu::hal::api::Metal>()`, which
  yields the `wgpu-hal` Metal device; `wgpu-hal` 27 exposes
  `metal::Device::texture_from_raw`, and `wgpu::Device::create_texture_from_hal`
  turns it into a `wgpu::Texture` that iced can bind.
- The Metal device the surface must be created on is reachable from `wgpu-hal`
  (`raw_device()`), so the surface and the texture come from one device.

That path is unexercised end to end. It is the next task, and the software renderer
stays the shipped default until it is measured in the application itself.

### Subtitles and OSD survive the GPU path

One question had to be settled before that path could replace anything: does the
OpenGL renderer still draw the OSD and the subtitles, or would a GPU renderer lose
them? It draws both. Against a black fixture with an imported Bengali cue
(`sub-add … select`, which is what the application calls), the rendered surface
contains the cue at the bottom of the frame; the same run with an
`osd-playing-msg` shows the message too. The application's own software path renders
the same cue — `a_bengali_subtitle_is_drawn_into_the_picture` in
`apps/desktop/src/app/acceptance.rs` lights 173 pixels in the subtitle band against
none without it, and the frame shows real Bengali glyphs rather than boxes.

Two traps for the next measurement: libmpv auto-loads a sidecar named after the
video (`<name>.bn.srt`), so a control run needs a differently named file, and
`--sub-file` as a startup option did not take effect in these probes where
`sub-add … select` did.
