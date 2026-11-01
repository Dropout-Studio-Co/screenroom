# ADR-005 — HDR presentation on macOS: an extended-range swapchain

- **Date:** 2026-11-02
- **Status:** Accepted; implemented; one owner check outstanding
- **Supersedes:** the "blocked, needs a patched renderer or an
  application-owned layer" conclusion in P1-03 of
  [`MACOS_REMAINING_WORK.md`](MACOS_REMAINING_WORK.md)
- **Relates to:** ADR-002 (Rust-first stack), ADR-004 (Personal Edition scope),
  F07 / Q03 in the capability matrix, gate 2 in
  [`FEASIBILITY_GATES.md`](FEASIBILITY_GATES.md)

## The question

P1-03 established, by measurement, that Screenroom already produces a 16-bit
half-float PQ surface (`RGhA`, `GL_RGBA16F`, delivered peak 1.076 in code space)
but that nothing could be *presented* as extended range: `iced_wgpu 0.14.0`
picks the first sRGB surface format and offers no override, and `wgpu-hal`
sets `wantsExtendedDynamicRangeContent` only as a consequence of the surface
format. The question was how to close the last step without damaging the shipped
eight-bit path.

## Decision

Present the picture through an **extended-range swapchain owned by iced's
existing compositor**, by vendoring `iced_wgpu` with one small, documented patch,
and carry the picture in the renderer's own colour convention rather than
converting it twice.

1. `vendor/iced_wgpu` is a copy of `iced_wgpu 0.14.0` with a semantic diff
   confined to `src/window/compositor.rs` (format selection, the layer's colour
   space, a layer-state reader, and unit tests), plus the dependencies and the
   receipt example in its `Cargo.toml`. Every other file is byte-identical to
   the published crate. It is pulled in with `[patch.crates-io]` and excluded
   from the workspace so that its upstream formatting is not rewritten.
2. `select_format` returns `wgpu::TextureFormat::Rgba16Float` when
   `SCREENROOM_HDR=extended` and the surface offers it; otherwise it is the
   upstream expression, unchanged. A request that cannot be honoured falls back
   to the upstream choice and logs why.
3. Because the swapchain is half float, `wgpu-hal` sets
   `wantsExtendedDynamicRangeContent` on the `CAMetalLayer`. CoreAnimation then
   re-tags the layer `kCGColorSpaceExtendedLinearSRGB`, which is the wrong
   transfer function for this renderer (see 4), so the fork overrides the colour
   space to `kCGColorSpaceExtendedSRGB`.
4. The renderer's convention is "the drawable holds sRGB-encoded values", not
   "linear values": `iced`'s default feature set includes `web-colors`, so
   `iced_graphics::color::pack` writes authored sRGB components and the engine
   selects the **non-sRGB** `Bgra8Unorm` surface format. The layer is tagged
   `kCGColorSpaceSRGB`, so those values are displayed as authored. The picture
   therefore has to arrive in the same encoding:
   - the `IOSurface` is imported without an sRGB view (`Bgra8Unorm`/`Rgba8Unorm`),
     so surface codes reach the shader unchanged;
   - for a PQ frame the shader applies the ST 2084 inverse EOTF, scales nits by
     the 203-nit reference white to an extended-linear value, and then applies
     the sRGB transfer function, which can produce values above 1.0 in the
     extended space. Those values are the highlights.
5. libmpv is asked to **tone map and gamut map into the display's real peak**:
   `target-trc=pq`, `target-prim=bt.709`, `target-peak=<203 × measured EDR
   headroom>`, `tone-mapping=bt.2390`, `hdr-compute-peak=no`. The display peak is
   measured once, on the main thread, from
   `NSScreen.maximumPotentialExtendedDynamicRangeColorComponentValue`; with no
   measurement available the peak falls back to the 203-nit reference white and
   the output description says `assumed`.
6. `SCREENROOM_HDR` remains the single opt-in and remains off by default. It is
   read by the player (frame format) and by the vendored compositor (swapchain
   format), so the two cannot disagree.

## Alternatives, and why they were not chosen

- **Patch iced to a 16F swapchain and treat the layer as extended linear.**
  Rejected as the final form: with `web-colors` packing, the whole UI would be
  interpreted as linear and would visibly change. Adopted *and corrected*: the
  fork sets `kCGColorSpaceExtendedSRGB` instead.
- **An application-owned `CAMetalLayer`/`NSView` for the picture, composited
  with iced's content.** Rejected. The player screen stacks its transport
  chrome *over* the picture (`stack![surface, overlay]`), so a native layer above
  the iced layer would hide the controls, and a native layer below it would
  require a transparent window and an alpha hole punched through iced's clear —
  changing the shipped window's compositing, the most invasive option for the
  least verifiable gain. It also needs a second GPU device and its own swapchain
  timing, and none of it can be verified headlessly.
- **A separate `NSWindow` for the picture.** Rejected: it breaks fullscreen,
  mini-player, focus and the in-app layout, and duplicates window management.
- **Stay on SDR and tone map honestly.** Rejected as the end state: it discards
  every highlight above SDR white on a panel that reports 2.0× of them, and it
  left Q03 permanently unmet. It remains the default until the switch is flipped,
  and it is the control the receipts compare against.

## Consequences

- **What works now:** with `SCREENROOM_HDR=extended`, a PQ file is tone mapped to
  the measured display peak, gamut mapped to BT.709, delivered as
  `RGBA16Float`, presented through a `Rgba16Float` swapchain on a layer that
  asks for extended range and is tagged extended sRGB. Measured headlessly:
  peak 380.6 nits, 1.875× reference white, 85 050 of 169 641 sampled pixels
  above reference white, on a display whose potential headroom is 2.0.
- **Gamut ceiling.** One drawable has one colour space, and the UI owns it, so
  the picture is gamut mapped to BT.709 rather than shown in the panel's full
  P3 gamut. HDR luminance and 10-bit precision are preserved; wide-gamut
  saturation is not. Lifting that ceiling requires the colour space of the
  picture's own layer (the rejected option), so it stays a named limitation.
- **A gamma defect was found and fixed on the way.** The picture widget imported
  its surface through an sRGB view and wrote the decoded linear value into a
  non-sRGB target, so a mid-grey code of 128 arrived at the display as 55. The
  UI, which is packed as sRGB codes, was unaffected; the picture was wrong by
  design of the import mapping. `apps/desktop/src/video_surface.rs` now imports
  without the sRGB view, and a test asserts 128 → 128 through the real
  `texture_from_surface` path. This is a visible correction to the shipped SDR
  picture and is recorded as such.
- **Cost of the fork.** iced upgrades re-apply a ~330-line diff in one file
  plus a manifest block. Nothing else in the tree knows about the fork, and the
  eight-bit path is the upstream expression when the switch is off.
- **Verification limits.** `maximumExtendedDynamicRangeColorComponentValue` only
  rises above 1.0 while EDR content is actually on screen, so the *delivered*
  headroom cannot be read headlessly; and the fixture is generated noise, not a
  graded master. What an eye sees on the panel is the owner's step.

## Evidence

- [`../crates/screenroom-player/HWDEC_EVIDENCE.md`](../crates/screenroom-player/HWDEC_EVIDENCE.md),
  "HDR10 on screen: the extended-range layer, measured (2026-11-02)".
- `crates/screenroom-player/examples/hdr_check.rs` — delivered range in code
  space, nits, display target, and the SDR control.
- `vendor/iced_wgpu/examples/metal_extended_range.rs` — the swapchain format,
  `wantsExtendedDynamicRangeContent` and the colour space read back from a real
  `CAMetalLayer`, with the eight-bit control.
- `apps/desktop/src/video_surface.rs` — shader compilation for both targets, the
  generated PQ constants, and the sRGB pass-through measurement.
