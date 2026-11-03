---
title: "iced trims shader pipelines after every draw, so a video pipeline must survive trim"
author: Beyourahi
date: 2026-11-03
branch: main
id: l-decd8944
scope: repo
status: active
src: agent
touches:
  - apps/desktop/src/video_surface.rs
---

`iced_wgpu` calls `Storage::trim` after every draw. Screenroom's `VideoPipeline::trim` used to drop the imported IOSurface and bind group while leaving `uploaded_generation`/`uploaded_surface`, so the next redraw of the same frame skipped `prepare` (markers matched) and drew nothing — a one-frame blank that only appeared when something forced redraws between video frames, which is exactly what pointer motion does. Measured 117/347 blank frames while sweeping the pointer, 0/321 after.

Keep the live binding across `trim` and make the unchanged-frame shortcut depend on it (`can_reuse`). Do not "fix" it by clearing the markers instead: that re-imports the surface on every redraw.

Also: a `Float`-based reveal on siblings of the picture inflates the stack's layout, which changes the shader's bounds and visibly zooms the video during the animation. Animate alpha, not scale, over a video surface.
