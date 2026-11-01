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

## HDR10 on the OpenGL path: an extended-range surface, measured (2026-11-01)

Everything below ran headless. No window was opened, nothing was raised, and no
desktop state was touched. The media was opened read-only and looped.

### What the installed headers define, read rather than recalled

mpv/libmpv 0.41.0 (client API 2.5), `/opt/homebrew/include/mpv/render.h`:
the whole `mpv_render_param_type` enum runs from `MPV_RENDER_PARAM_INVALID = 0`
to `MPV_RENDER_PARAM_SW_POINTER = 20`. **There is no colourspace, HDR-metadata or
target-peak render parameter in it.** The HDR-relevant entries that do exist are:

| Enum | Value | What the header says it is |
|---|---:|---|
| `MPV_RENDER_PARAM_OPENGL_FBO` | 3 | `mpv_opengl_fbo*`, whose `internal_format` is "Underlying texture internal format (e.g. `GL_RGBA8`), or 0 if unknown" |
| `MPV_RENDER_PARAM_DEPTH` | 5 | "Control surface depth… the depth of the surface passed to the render function in bits per channel. If omitted or set to 0, the renderer will assume 8. Typically used to control dithering." |
| `MPV_RENDER_PARAM_ICC_PROFILE` | 6 | ICC blob for `--icc-profile-auto` only; unrelated to HDR output |

`render_gl.h` adds no HDR fields: `mpv_opengl_fbo` is `{int fbo; int w, h; int
internal_format;}`. `mpv master`'s `render.h` fetched during this session also has
no target-colourspace parameter, so this is not a version lag in the Homebrew build.

The target colourspace is therefore **an mpv option, not a render parameter**.
Three further facts decide the design:

- The manual states `--target-colorspace-hint` is "supported on Wayland, D3D11 and
  winvk contexts" and "Requires a supporting driver and `--vo=gpu-next`". macOS/CGL
  is not in that list, so there is no swapchain colours-space hint available from
  libmpv here.
- `video/out/gpu/libmpv_gpu.c` (v0.41.0, lines 182–190) feeds
  `MPV_RENDER_PARAM_DEPTH` into `gl_video_set_fb_depth()`, and
  `video/out/opengl/ra_gl.c` (lines 420–458) wraps the caller's FBO with
  `fbo_dummy_format`, which carries no bit depth. So `internal_format` alone does
  **not** tell mpv the target precision; `MPV_RENDER_PARAM_DEPTH` is the parameter
  that does.
- The libmpv GL render API runs **vo_gpu** (`gl_video_render_frame`), not
  vo_gpu-next. `--target-prim`, `--target-trc`, `--target-peak`, `--tone-mapping`
  and `--hdr-compute-peak` are all live on that path; `--target-colorspace-hint`
  is not (see above).

### What was implemented

- `PixelFormat { Rgba8, Bgra8, Rgba16Float }` with `bytes_per_pixel`,
  `bits_per_channel` and `is_extended_range`, and `Frame::format`, so a delivered
  frame states its own precision instead of the desktop layer assuming BGRA.
- `OutputDescription { format, extended_range_requested, target_peak_nits,
  transfer, tone_mapping }` with `label()`, published as
  `PlayerSnapshot.output` for Settings to show. It says what was requested and
  what the surface carries; it does not claim that a display received HDR.
- `SurfaceFrame::new_with_format`. The eight-bit path is byte-identical to before:
  pixel format `BGRA` (`0x42475241`), 4 bytes per element, row = width × 4. The new
  path uses CoreVideo's `kCVPixelFormatType_64RGBAHalf`, quoted from
  `CVPixelBuffer.h`: `'RGhA'`, "64 bit RGBA IEEE half-precision float, 16-bit
  little-endian samples", i.e. `0x52476841`, 8 bytes per element, row = width × 8
  (3840 → 30720). `IOSurfaceCreate` returning non-null **and** `IOSurfaceGetPixelFormat`
  reading back `0x52476841` is the acceptance, and both are asserted in
  `surface::tests::a_half_float_surface_is_created_with_the_core_video_pixel_format`
  and `…row_stride_holds_eight_bytes_per_pixel` (16×9 and 3840×2160).
- `GlRenderer` carries an `OutputDescription`. SDR keeps `GL_RGBA8` (`0x8058`) with
  `GL_BGRA`/`GL_UNSIGNED_INT_8_8_8_8_REV` and passes that as
  `MPV_RENDER_PARAM_OPENGL_FBO.internal_format`. Extended uses `GL_RGBA16F`
  (`0x881a`) with `GL_RGBA`/`GL_HALF_FLOAT` (`0x140b`) and adds
  `MPV_RENDER_PARAM_DEPTH = 16`. On the SDR path the param array now carries
  `MPV_RENDER_PARAM_DEPTH = 0`, which the header documents as equivalent to
  omitting it; the surface format, FBO format and pixel path are unchanged.
- `enable_extended_range()` sets, on the live core: `target-prim=bt.2020`,
  `target-trc=pq`, `target-peak=<the file's own tag, default 1000>`,
  `tone-mapping=clip`, `hdr-compute-peak=no`. `clip` with no peak computation is
  deliberately "do not compress": mpv is told the target accepts the file's own
  peak, so the master's codes are passed through instead of being tone-mapped into
  the SDR range. `reset_extended_range()` returns all five to `auto`.
- The worker observes `video-params/gamma`, `video-params/max-cll` and
  `video-params/max-luma` (no blocking reads were added to the render loop), treats
  **only** `pq` as extended range, switches the surface when the first tag arrives,
  and reverts when an SDR file loads. `HdrMode` is **opt-in**: `Player::start()`
  and `start_with_hwdec()` stay on `HdrMode::Sdr`, and
  `SCREENROOM_HDR=extended` or `Player::start_with_hdr(…, HdrMode::Extended)` turns
  it on. See "what the lead must wire" for why that default must not be flipped yet.

### The headless receipt

`crates/screenroom-player/examples/hdr_check.rs`. Three passes over the same file in
one process, each with its own `Mpv` and `GlRenderer`: `sdr` (8-bit BGRA, mpv's
defaults), `float8` (a 16F surface **without** any HDR request — the control that
separates "float container" from "HDR signal"), and `extended` (16F plus the options
above). It samples 169,641 pixels per pass on a 7-pixel grid, reports the surface
fourcc read back from the `IOSurface`, the FBO internal format, distinct green
values, how many values sit off the 8-bit grid, how many exceed the container's
1.0 ceiling, and — for the PQ pass only — luma in nits from the PQ EOTF.

Command, on the generated 3840×2160 24 fps HEVC Main10 HDR10 fixture
(`bt2020nc`/`smpte2084`, BT.2020, 1000-nit MaxCLL, `ffprobe` above):

```
cargo run -p screenroom-player --example hdr_check -- .artifacts/media/bench-4k-hdr10.mp4 2 all
```

M4 MacBook Air, libmpv 2.5 via `/opt/homebrew/lib/libmpv.2.dylib`, debug build,
`hwdec=auto-copy`, audio and subtitles disabled, `vo=libmpv`, no window:

| Measurement | `sdr` | `float8` (control) | `extended` |
|---|---:|---:|---:|
| Surface pixel format (read back) | `BGRA` `0x42475241` | `RGhA` `0x52476841` | `RGhA` `0x52476841` |
| Bits per channel | 8 | 16 | 16 |
| `GL` internal format | `0x8058` | `0x881a` | `0x881a` |
| Bytes per row at 3840 | 15360 | 30720 | 30720 |
| `MPV_RENDER_PARAM_DEPTH` | 0 | 16 | 16 |
| Extended range requested | no | **no** | **yes** |
| Distinct green values | **245** | **2105** | **4509** |
| Samples off the 8-bit grid | **0** | 113558 | 169116 |
| Samples above code 1.0 | 0 | 0 | **13** |
| Mean green code | 0.512115 | 0.512021 | 0.509543 |
| Peak green code | 1.000000 | 1.000000 | **1.076172** |
| Peak luma | — | — | 9407.0 nits |
| Peak vs SDR white (100 nits) | — | — | **94.1×** |
| Samples above SDR white | — | — | 168970 of 169641 |

Repeat runs move a little, because each pass samples whichever frames arrive during
its own two-second window of a noise clip: across four runs the `extended` pass
reported 4485–4606 distinct green values, a peak green code of 1.026–1.076, 13–14
samples above 1.0 and 8128–9407 peak nits. The `sdr` pass stayed at 245–250 distinct
values with zero off-grid samples and zero values above 1.0 in every run, and
`float8` at 2105 distinct values with zero above 1.0. The numbers in the table are
one complete run, not a best-of.

Three things in that table are the actual evidence:

1. **More than eight bits survived.** 4509 distinct green values against 245 on the
   SDR surface, and 169,116 of 169,641 samples are not on the 8-bit grid. The
   `float8` control (2105 distinct, also off-grid) proves that "off-grid" alone is
   not HDR — it is what a float target does to any signal — which is why the two
   controls and the distinct-value count are reported together.
2. **The surface carries code values the SDR container cannot hold.** 13 samples
   exceed 1.0 (peak code 1.076172); the eight-bit path is capped at exactly
   1.000000 by construction.
3. **The HDR target options are live, not ignored.** Re-running the same pass with
   the peak forced to 100 moves the peak from code 1.076172 to **0.580078** and the
   peak luma from 9407 nits to **176.4 nits** while the surface format is unchanged:

   ```
   cargo run -p screenroom-player --example hdr_check -- .artifacts/media/bench-4k-hdr10.mp4 2 extended 100
   → [extended] output 16 bits per channel, pq/bt.2020, tone mapping none, target peak 100 nits
   → [extended] peak_green_code 0.580078
   → [extended] distinct_green 4016
   → [extended] peak_nits 176.4
   ```

   So the delivered range follows mpv's target configuration rather than being a
   fixed SDR envelope.

The SDR path is unchanged on the SDR fixture. Command:

```
cargo run -p screenroom-player --example hdr_check -- .artifacts/media/bench-4k-hevc-10bit.mp4 2 sdr
→ [sdr] surface_fourcc BGRA number 0x42475241
→ [sdr] surface_bits_per_channel 8
→ [sdr] surface_bytes_per_row 15360
→ [sdr] fbo_internal_format 0x8058
→ [sdr] samples 169641
→ [sdr] mean_green_code 0.509241
→ [sdr] peak_green_code 1.000000
→ [sdr] distinct_green 255
→ [sdr] off_grid 0
→ [sdr] above_one 0
```

The same fixture also reaches BGRA8 when the file itself is SDR even with HDR
enabled, which is the "SDR path is unchanged when the file is SDR" half:

```
… -- .artifacts/media/bench-4k-hevc-10bit.mp4 0.1 player
→ [player] output 8 bits per channel, srgb, tone mapping mpv default
→ [player] frame_format BGRA8
```

And the real `Player`, headless, on the HDR10 fixture:

```
… -- .artifacts/media/bench-4k-hdr10.mp4 0.1 player
→ [player] output 16 bits per channel, pq/bt.2020, tone mapping none, target peak 1000 nits
→ [player] extended_range_requested true
→ [player] frame_format RGBA16Float
→ [player] frame_bytes_per_pixel 8

… -- .artifacts/media/bench-4k-hdr10.mp4 0.1 player-sdr
→ [player] output 8 bits per channel, srgb, tone mapping mpv default
→ [player] frame_format BGRA8
→ [player] frame_bytes_per_pixel 4
```

The second run is the shipped default: the same HDR file through `HdrMode::Sdr`
still delivers BGRA8, so a caller that has not opted in cannot get a 16F frame it
cannot display.

### What is not proven, and the one thing that blocks HDR on screen

- **Nothing has been presented. No window was opened, so no EDR layer exists and no
  panel received anything.** The 16F `IOSurface` is a prerequisite, not HDR on
  screen.
- **`wantsExtendedDynamicRangeContent` cannot be reached through iced 0.14 as
  shipped.** Checked in the installed crates, not recalled:
  - `wgpu-hal 27.0.4`, `src/metal/surface.rs:168–173`, already does the right thing:
    `let wants_edr = config.format == wgt::TextureFormat::Rgba16Float;` and sets the
    layer property accordingly, and `src/metal/adapter.rs:362–366` lists
    `Rgba16Float` among the macOS surface formats. So wgpu 27 supports an EDR
    swapchain.
  - `iced_wgpu 0.14.0`, `src/window/compositor.rs:107–121`, picks the first format
    with `is_srgb()` (`Bgra8UnormSrgb`) and offers no override;
    `iced_wgpu::Settings` has no format field. So the swapchain iced creates is
    always 8-bit sRGB, and `Rgba16Float` — non-sRGB — can never be selected.
  - Consequence: an EDR layer requires either a patched/vendored `iced_wgpu` format
    choice, or an application-owned `CAMetalLayer` / view whose
    `wantsExtendedDynamicRangeContent` and `colorspace` (`kCGColorSpaceITUR_2100_PQ`
    for a PQ surface) the app sets itself. Neither exists, and neither can be added
    from this crate.
- **Absolute luminance is not calibrated.** The nits above are the PQ EOTF applied
  to the codes that arrived; they are not a display measurement and not a reference
  render. A libplacebo/reference comparison was attempted and is unavailable
  headlessly on this machine: `mpv --vo=image -o …` fails with "Error
  opening/initializing the selected video_out (--vo) device" and every other capture
  VO needs a window. The fixture is generated noise tagged PQ, not a graded master,
  so "9407 nits peak" is what its own codes mean, not a highlight anyone graded.
  For the same reason the receipt's reconstructed luma code (0.8489 mean) does not
  reconcile with the file's own `signalstats` luma (YAVG 504.5/1023 = 0.503
  normalized, YMAX 1009/1023 = 1.079) — the source is synthetic and the YCbCr→RGB
  round trip is not luma-preserving on noise. Only the container, precision and
  target-behaviour facts above are claimed.
- **HLG is not treated as HDR.** Only `gamma = pq` selects the extended surface; an
  HLG file stays on the SDR path and is tone-mapped there. That is a decision, not
  an oversight, and it is labelled by the output description.
- **No real HDR10 film and no HDR display were involved.** The owner's media was not
  mounted and nothing was shown.

### What the lead had to wire in `apps/desktop` (2026-11-01; since done)

Recorded as written on 2026-11-01, and resolved on 2026-11-02 — see "HDR10 on
screen" below for what actually shipped, which differs from item 2.

1. `apps/desktop/src/video_surface.rs` hardcoded the imported format:
   `metal::MTLPixelFormat::BGRA8Unorm_sRGB` and `wgpu::TextureFormat::Bgra8UnormSrgb`.
   It now maps `surface.format()`, and on 2026-11-02 the eight-bit mapping was
   changed to the non-sRGB format as well, because the renderer's convention is
   sRGB-encoded values (see the 2026-11-02 section).
2. The app's own `CAMetalLayer` (or a patched `iced_wgpu`) must set
   `wantsExtendedDynamicRangeContent = YES` and a colour space for the picture.
   It was done with a vendored `iced_wgpu`; the colour space is
   `kCGColorSpaceExtendedSRGB`, not `kCGColorSpaceITUR_2100_PQ`, because one
   drawable carries the UI as well as the picture.
3. Settings prints `PlayerSnapshot.output.label()`, and the label now names the
   real target: `16 bits per channel, pq/bt.709, tone mapping bt.2390, target
   peak 406 nits`.
4. `SCREENROOM_HDR` is still the opt-in and still off by default.

### One concurrent-edit note

`crates/screenroom-player/tests/worker.rs::default_decode_policy_still_delivers_generated_frames`
failed partway through this session (the OpenGL path's `decode_policy` returns
`"auto"`, which this libmpv answers with `hwdec-current = videotoolbox`, while the
assertion then accepted only `no` or `*-copy`). The file belongs to another agent,
it was fixed in place at 23:24 while this work continued, and with that fix
`cargo test -p screenroom-player` passes 22/22. Recorded so the failure seen in the
transcript is not mistaken for a regression in this change.

`crates/screenroom-player/src/gl_render.rs` and `src/lib.rs` were also edited
concurrently (surface pool to 16, `previous_surface`, a `cfg(macos)` on
`report_swap`). This change is additive on top of that work and keeps it; the
extended-range format is a separate axis from the surface pool.

## HDR10 on screen: the extended-range layer, measured (2026-11-02)

Decision and rationale: [`docs/ADR-019_HDR_PRESENTATION.md`](../../docs/ADR-019_HDR_PRESENTATION.md).
This section supersedes the "one thing that blocks HDR on screen" conclusion of
the 2026-11-01 section: the swapchain can be half float, and it now is.

### The hardware, read rather than assumed

`sw_vers` reports **macOS 27.0.1 (26A434)**, `Darwin 27.0.0`, `arm64`. A Swift
probe using `NSScreen`, `MTLCreateSystemDefaultDevice` and `CGDisplayCopyDisplayMode`
(no window, nothing raised) reports:

| Fact | Value |
|---|---|
| GPU | `Apple M4`, registry 4294968222, recommended working set 12.7 GB |
| Metal families | apple1 … apple9, `metal3`, `metal4` |
| `rgba16Float` | **renderable** (a 64×64 render-target texture is created) |
| `bgra8Unorm_srgb`, `rgba8Unorm_srgb`, `rgba32Float`, `bgr10a2Unorm`, `rgba16Unorm` | renderable |
| Display | built-in, display id 1, scale 2.0, mode 1470×956 pt = **2940×1912 px** at 60 Hz |
| Display colour space | ICC-based `Color LCD` (no reference/HDR preset engaged) |
| `maximumPotentialExtendedDynamicRangeColorComponentValue` | **2.0** |
| `maximumExtendedDynamicRangeColorComponentValue` | **1.0** |
| `maximumReferenceExtendedDynamicRangeColorComponentValue` | 0.0 |

`2.0` is the panel advertising that an EDR window on it may drive up to twice SDR
white; `1.0` is what it currently offers because no EDR content is on screen.
Both are the OS's answer, not a brightness measurement — this session did not
measure panel nits and does not claim to.

Lockfile versions used for every statement below: `iced 0.14.0`,
`iced_wgpu 0.14.0`, `iced_winit 0.14.1`, `wgpu 27.0.1`, `wgpu-core 27.0.3`,
`wgpu-hal 27.0.4`, `libmpv` 0.41.0 (client API 2.5).

### The constraint, re-checked in the installed sources

- `wgpu-hal-27.0.4/src/metal/surface.rs:170–173` still sets
  `wantsExtendedDynamicRangeContent` exactly when the surface format is
  `Rgba16Float`, and `src/metal/adapter.rs:362–366` still offers
  `[Bgra8Unorm, Bgra8UnormSrgb, Rgba16Float, Rgb10a2Unorm?]` as surface formats.
- `iced_wgpu-0.14.0/src/window/compositor.rs` still chooses the format itself and
  `src/settings.rs` still has no format field. The escape hatches were checked
  again and are not usable as-is:
  - `iced_graphics::compositor::Compositor` is a trait, and `iced_winit` selects
    it through `<P::Renderer as compositor::Default>::Compositor`, so a custom
    compositor requires a custom `Renderer` type. `iced::Element` is fixed to
    `iced_renderer::Renderer`, which means that route rewrites every signature in
    `apps/desktop`.
  - `iced::application(...)` exposes no renderer settings beyond
    `graphics::Settings` (vsync, font, text size, antialiasing).
  - `iced_graphics::color::GAMMA_CORRECTION` and its `web-colors` feature are
    compile-time; enabling or disabling one changes the whole UI's colour
    packing, not the surface format.

**One new fact, and it decides the colour handling:** `iced`'s default features
include `web-colors`, so `color::GAMMA_CORRECTION` is **false** and
`color::pack` writes authored sRGB components. The compositor consequently
selects the *non-sRGB* `Bgra8Unorm`, and the layer carries
`kCGColorSpaceSRGB`. Measured in a hidden-window run, not inferred:

```
compositor: selected Bgra8Unorm alpha PostMultiplied gamma_correction false
```

So the renderer's convention is "the drawable holds sRGB-encoded values". The
picture has to arrive in that same encoding, and the extended layer has to use an
extended space with the **same transfer function**, which is
`kCGColorSpaceExtendedSRGB` — not `kCGColorSpaceITUR_2100_PQ` (a PQ layer would
be right only if the picture had the drawable to itself) and not
`kCGColorSpaceExtendedLinearSRGB` (which is what CoreAnimation picks by default
and what would make the UI, packed as sRGB codes, display wrong).

### What was implemented

- `vendor/iced_wgpu`: the published `iced_wgpu 0.14.0` with a semantic diff
  confined to `src/window/compositor.rs` — `select_format` (upstream's
  expression, plus `Rgba16Float` when `SCREENROOM_HDR=extended` and the surface
  offers it), the layer colour-space override, a layer-state reader, and five
  unit tests — plus the dependencies and the receipt example in `Cargo.toml`.
  It is wired in with `[patch.crates-io]` and excluded from the workspace so its
  upstream formatting is not rewritten; its own tests and example run with
  `--manifest-path`.
- `crates/screenroom-player/src/display.rs`: the presentation contract. The
  ST 2084 inverse EOTF and its inverse, the 203-nit reference white, and
  `DisplayTarget` (headroom, peak nits, and whether the peak was measured or
  assumed). The headroom is read once, on the main thread, from
  `NSScreen.mainScreen`.
- `gl_render.rs`: the extended options became a tone map into the display's real
  peak — `target-prim=bt.709`, `target-trc=pq`, `target-peak=<peak>`,
  `tone-mapping=bt.2390`, `hdr-compute-peak=no`. The previous pass-through
  (`tone-mapping=clip`, `target-peak` = the file's own tag) is only correct for a
  PQ-coloured layer, which this is not; with it, 168 970 of 169 641 sampled
  pixels were above SDR white and everything above the panel's ceiling would have
  clipped.
- `apps/desktop/src/video_surface.rs`: the picture shader now converts a PQ code
  to nits, scales by the reference white, and applies the sRGB transfer function
  — so the value 1.0 is reference white and values above it are the extended
  range, in the same encoding as the UI. The PQ constants are generated into the
  WGSL from the crate's constants, so the receipt and the shader cannot drift.
  The `IOSurface` is imported **without** an sRGB view.
- `apps/desktop/src/video_surface.rs`, unrelated to HDR but found by the
  measurement below: the sRGB view was decoding the surface and the decoded value
  was written into a non-sRGB target, so a mid-grey code of 128 reached the
  buffer as **55**. That is a visible gamma error in the shipped eight-bit path;
  it is fixed, and a test asserts `128 → 128` through the real
  `texture_from_surface` path.

### The layer, read back from a real `CAMetalLayer`

`vendor/iced_wgpu/examples/metal_extended_range.rs`, unmodified crate, offscreen
32×18 `NSView`, `wgpu` 27, no window:

```
[hardware] metal_device Apple M4
[capability] potential_edr_headroom 2
[sdr] swapchain_format Bgra8UnormSrgb
[sdr] layer_pixel_format 81
[sdr] layer_wants_extended_dynamic_range false
[sdr] layer_colorspace_wgpu kCGColorSpaceSRGB
[sdr] layer_colorspace_fork kCGColorSpaceSRGB
[extended] swapchain_format Rgba16Float
[extended] layer_pixel_format 115
[extended] layer_wants_extended_dynamic_range true
[extended] layer_colorspace_wgpu kCGColorSpaceExtendedLinearSRGB
[extended] layer_colorspace_fork kCGColorSpaceExtendedSRGB
[receipt] ok
```

`81` is `MTLPixelFormatBGRA8Unorm_sRGB` and `115` is `MTLPixelFormatRGBA16Float`.
Two facts here are worth stating plainly: wgpu flips the EDR flag for the half
float swapchain and nothing else, and CoreAnimation's own default colour space
for an EDR layer is extended **linear** sRGB — which is why the fork overrides it.
The eight-bit control is left exactly as it was.

### The delivered range, headless

```
cargo run -p screenroom-player --example hdr_check -- .artifacts/media/bench-4k-hdr10.mp4 2 all
```

| Measurement | `sdr` | `float8` (control) | `extended` |
|---|---:|---:|---:|
| Surface fourcc | `BGRA` `0x42475241` | `RGhA` `0x52476841` | `RGhA` `0x52476841` |
| Bits per channel | 8 | 16 | 16 |
| `GL` internal format | `0x8058` | `0x881a` | `0x881a` |
| `MPV_RENDER_PARAM_DEPTH` | 0 | 16 | 16 |
| Extended range requested | no | no | **yes** |
| Distinct green values | 246 | 2 105 | **1 261** |
| Samples off the 8-bit grid | 0 | 113 558 | **138 175** |
| Peak green code | 1.000000 | 1.000000 | 0.653809 |
| Mean luma | — | — | 182.2 nits |
| Peak luma | — | — | **380.6 nits** |
| Peak vs reference white | — | — | **1.875×** |
| Samples above reference white | — | — | **84 875 of 169 641** |
| Delivered peak, extended linear | — | — | **1.875** (green channel 1.993) |
| Display ceiling, extended linear | — | — | 2.000 |

The extended pass now reports its own target in the output description:

```
[extended] output 16 bits per channel, pq/bt.709, tone mapping bt.2390, target peak 406 nits
[extended] display 406 nits peak (2 x 203 nits reference white, measured)
[extended] display_assumption measured
```

Three things in that table are the evidence, and one is a correction:

1. **The panel's headroom is what bounds the picture, and it is reached.**
   `peak_luma_code 0.647256` is 380.6 nits, `1.875×` the 203-nit reference white,
   against a ceiling of `2.0`. Half the sampled pixels (84 875 of 169 641) are
   above reference white. On the eight-bit path the ceiling is 1.000000 by
   construction.
2. **More than eight bits survived** — 1 261 distinct green values against 246,
   with 138 175 samples off the 8-bit grid. The `float8` control (2 105 distinct,
   also off-grid) is why "off the grid" alone is not claimed as HDR.
3. **Nothing exceeds the display.** The tone map is asked for 406 nits and
   delivers 380.6; `above_one` is 0 by design, because the whole point of
   tone mapping into a bounded target is that the master's 9 407-nit codes become
   displayable ones instead of being clipped by the compositor.
4. **The previous pass-through is no longer claimed.** The 2026-11-01 receipt's
   "0.653 vs 1.076 peak code, 9 407 nits" measured what a *layer* would clip; it
   described the surface, not a display. This one is bounded by the display and
   says so.

The SDR control is unchanged on the SDR fixture:

```
cargo run -p screenroom-player --example hdr_check -- .artifacts/media/bench-4k-hevc-10bit.mp4 2 sdr
→ surface_fourcc BGRA number 0x42475241, 8 bits, 15360 bytes per row
→ fbo_internal_format 0x8058, distinct_green 256, off_grid 0, above_one 0
```

And per file, with the switch on: an SDR file still reaches an eight-bit surface,
and the shipped default still reaches one for an HDR file.

```
… bench-4k-hdr10.mp4 0.2 player      → 16 bits, pq/bt.709, bt.2390, 406 nits, RGBA16Float, 8 B/px
… bench-4k-hevc-10bit.mp4 0.2 player → 8 bits, srgb, mpv default, BGRA8, 4 B/px
… bench-4k-hdr10.mp4 0.2 player-sdr  → 8 bits, srgb, mpv default, BGRA8, 4 B/px
```

### The whole application, end to end, hidden

`SCREENROOM_WINDOW=hidden` with the play check (never raised, never activated),
release build, 10-second sample, four-corner 3840×2160 HEVC Main10 HDR10 fixture:

| Run | Swapchain | Delivered fps | Drawn fps | Uploaded fps | CPU mean / max (%) | RSS mean (KiB) | Startup (ms) |
|---|---|---:|---:|---:|---:|---:|---:|
| `SCREENROOM_HDR=extended` | `Rgba16Float` | 24.506 | 24.090 | 0.000 | 14.27 / 34.00 | 167 389 | 742.6 |
| default (SDR) | `Bgra8Unorm` | 24.626 | 24.198 | 0.000 | 14.17 / 29.60 | 213 002 | 638.2 |

The swapchain format is printed by the application itself, so the line below is
the application's own compositor, not a stand-in:

```
video surface: target Rgba16Float, extended range true
video surface: target Bgra8Unorm, extended range false
```

Against the recorded baseline of 24.4 fps at 13.95 % mean CPU, the extended path
delivers 24.51 fps at 14.27 % — the same picture rate with no upload, and a CPU
difference inside the run-to-run spread of one sample. An earlier run of the same
build measured 24.79 fps at 14.62 % and a 128 ms worst gap, so both the rate and
the gap move between runs. RSS is
*lower* with the half-float swapchain in this run, which is allocator behaviour,
not a claim.

### What is still not verified, and what only the owner can see

- **No eye has seen the extended range on the panel.** `maximumExtendedDynamic
  RangeColorComponentValue` rises above 1.0 only while EDR content is on screen,
  so the headroom actually granted to the window cannot be read headlessly, and
  a screenshot cannot carry it. The delivery is proven up to the swapchain and
  the layer flags; the last step needs a look at an HDR10 film on the built-in
  panel.
- **The fixture is generated noise tagged PQ**, not a graded master, and the nits
  are the ST 2084 EOTF applied to delivered codes, not a display measurement.
- **Gamut is BT.709.** The drawable is shared with the UI and carries one colour
  space, so the master's BT.2020 is gamut mapped into the sRGB primaries. HDR
  luminance and 10-bit precision reach the panel; wide-gamut saturation does
  not. [`docs/ADR-019_HDR_PRESENTATION.md`](../../docs/ADR-019_HDR_PRESENTATION.md)
  records why and what would lift it.
- **The 16-bit swapchain costs GPU memory**: 2940×1912 at 8 bytes per pixel across
  the swapchain's drawables, against 4 bytes on the eight-bit path. Not measured
  as a GPU allocation here.
- **`SCREENROOM_HDR` stays off by default** until the owner has looked.
