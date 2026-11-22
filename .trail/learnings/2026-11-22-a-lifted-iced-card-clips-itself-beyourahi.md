---
title: "A lifted iced card clips itself by 2.75% of its width"
author: Beyourahi
date: 2026-11-22
branch: main
id: l-1e465bf0
scope: repo
status: active
src: agent
touches:
  - apps/desktop/src/ui/components/poster.rs
  - apps/desktop/src/ui/screens/library.rs
---

A card drawn inside iced's Float is scaled by MOTION.lift (0.055) but the layer is clipped to the card's UNSCALED bounds, so the outer 2.75% on each side is cut. On a 214.5 px Library column that is 5.9 px — a whole leading digit, which reads as "the text is wrong" rather than "the card is clipped". Only the card the pointer is over loses it, so it looks like a first-column bug until you notice the second row is fine.

Fix: derive the text inset from MOTION.lift and the card's own width (poster::text_inset), never a fixed pixel value — the required inset grows with the column, so a constant that works at 1460 is too small at 860 (2 columns, ~382 px cards).

Also: iced has no ellipsis, so a one-line card title needs ui::text_fit::fit_measured plus Wrapping::None; a character-count cut still wraps and makes rows uneven.
