# OpenGL → IOSurface → Metal probe

A standalone measurement of a zero-copy GPU path for `screenroom-player`, kept
because the numbers in `crates/screenroom-player/HWDEC_EVIDENCE.md` come from it.
It is not part of the Rust build: it is a C/Objective-C program, built by hand.

What it establishes, on a real GPU and with real decoded frames:

1. libmpv's `MPV_RENDER_API_TYPE_OPENGL` render context can be created against a
   headless `CGL` context on Apple Silicon (`GL 4.1`, renderer `Apple M4`).
2. mpv can render a decoded frame into an FBO whose colour attachment is an
   `IOSurface` (`CGLTexImageIOSurface2D`), so no frame is ever read back to the CPU.
3. Metal can wrap that same `IOSurface`
   (`-[MTLDevice newTextureWithDescriptor:iosurface:plane:]`) and reads exactly the
   bytes OpenGL wrote.
4. That is fast enough and cheap enough to be worth integrating: see the evidence
   document for delivered frames, CPU and RSS against the software renderer.

It does not touch the application, open a window, take focus or play audio.

## Build

```bash
cd crates/screenroom-player/examples/gl-path-probe
clang -O2 -DGL_SILENCE_DEPRECATION -o probe probe.m \
  -I/opt/homebrew/include -L/opt/homebrew/lib -lmpv \
  -framework OpenGL -framework IOSurface -framework CoreFoundation -framework Metal
```

## Run

```bash
./probe /path/to/film.mkv 8          # paced, glFinish per frame
./probe /path/to/film.mkv 6 untimed  # decode and render as fast as possible
/usr/bin/time -l ./probe …           # for CPU and peak RSS
```

The exit status is 0 only when every check passed. The program loops the file, so a
short fixture can be measured for longer than it plays, and it disables audio.

## What it does not prove

Presentation through iced and wgpu, GL-to-Metal synchronisation without a
`glFinish` per frame, HDR/EDR output, or anything about a real film on the owner's
drive. The integration plan and the open questions are recorded in
`HWDEC_EVIDENCE.md`.
