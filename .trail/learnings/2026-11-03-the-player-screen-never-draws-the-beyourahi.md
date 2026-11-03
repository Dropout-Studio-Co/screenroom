---
title: The player screen never draws the shell status bar
author: Beyourahi
date: 2026-11-03
branch: main
id: l-300b68a6
scope: repo
status: active
src: agent
touches:
  - apps/desktop/src/app.rs
  - apps/desktop/src/ui/shell.rs
---

`ui::shell::frame` returns the bare body for `Screen::Player`, and both the windowed and fullscreen paths bypass it, so anything written to `App::status` or `App::error` while a film plays is never shown. That is why the player's own controls must draw their own message: a refusal that only sets `status` looks like a dead button. The subtitle panel keeps a dedicated `subtitle_note` rather than borrowing the global status, because unrelated writes ("Muted.", "No intro or credits marker…") would otherwise appear under the panel's heading.

The overlay is bottom-anchored with a 330 px scrim and the column is not clamped or scrollable, so a growing panel is clipped off the top of the window rather than scrolled. Any list added there needs a row budget derived from the window height, not a constant.

Checked 2026-11-03 while landing the subtitle controls (ADR-005).
