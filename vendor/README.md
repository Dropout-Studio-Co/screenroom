# Vendored crates

Third-party source that this repository carries so a behaviour it needs can exist.
Nothing here is first-party code, and nothing here is edited beyond the patch each
section records.

## iced_wgpu 0.14.0

**Upstream:** `iced_wgpu` 0.14.0 from crates.io. **Licence:** MIT, as declared in
its `Cargo.toml`; upstream ships no separate `LICENSE` file, so none is added here.
**Attribution:** the iced authors, <https://github.com/iced-rs/iced>.

**Why it is carried.** iced 0.14 chooses its window swapchain format itself:
`src/window/compositor.rs` filtered the surface's formats for one that `is_srgb()`
and offered no setting to influence it. That makes an eight-bit sRGB swapchain
unavoidable, and an eight-bit swapchain cannot carry extended dynamic range — so
HDR could never reach the panel no matter what the player delivered. wgpu already
does its half: `wgpu-hal`'s Metal surface sets
`wantsExtendedDynamicRangeContent` when the surface format is `Rgba16Float`.
Only the format choice was missing.

**What the patch changes.** Three files:

- `Cargo.toml` — macOS-only dependencies on `objc2`, `objc2-core-foundation`,
  `objc2-core-graphics` and `raw-window-handle`, needed to set the layer's colour
  space. No dependency is added on any other platform.
- `src/window/compositor.rs` — a `SCREENROOM_HDR` gate; a pure
  `select_format(available, extended_range, gamma_correction)` helper that returns
  `Rgba16Float` only when the variable asks for extended range *and* the surface
  offers that format, and otherwise reproduces upstream's selection exactly; a
  macOS-only step that sets the window layer's colour space once the surface
  exists (`wgpu-hal` sets `wantsExtendedDynamicRangeContent` itself, and
  CoreAnimation's own default for an EDR layer is extended *linear* sRGB, which is
  the wrong transfer function for a renderer that packs sRGB-encoded values); and
  the receipt example that reads those facts back from a real `CAMetalLayer`.
- `examples/metal_extended_range.rs` — an offscreen `NSView` and a real `wgpu`
  surface, so the swapchain format, the flag and the colour space can be checked
  headlessly, with an eight-bit control. It needs `objc2-app-kit` as a
  dev-dependency for `NSView` and `NSScreen`.

**What it does not change.** With `SCREENROOM_HDR` unset or set to anything other
than `extended`, `select_format` returns what upstream's code returned, so every
existing path — including the whole SDR player — behaves as before. The fork adds
no rendering code, touches no shader, and changes no other file.

**How to update it.** Take the new upstream release, re-apply the two changes
above, and re-run `cargo test --workspace` plus `SCREENROOM_HDR=extended` on an
HDR10 file. This crate is excluded from the workspace, so its own tests and the
receipt run against its own lockfile:

```
CARGO_TARGET_DIR=$PWD/target cargo test --manifest-path vendor/iced_wgpu/Cargo.toml --lib
CARGO_TARGET_DIR=$PWD/target cargo run  --manifest-path vendor/iced_wgpu/Cargo.toml --example metal_extended_range
```

If a future iced release exposes the swapchain format through
`Settings`, delete this directory, remove the `[patch.crates-io]` entry in the root
`Cargo.toml`, and use the setting instead.

**Excluded from `scripts/no_comments.py`.** That check enforces a first-party
authoring rule. Upstream source carries comments that are not ours to remove, and
rewriting them would make the fork larger and harder to update. `vendor/` is
excluded for the same reason `reference/` is, and the patch itself is written
without added commentary apart from the public documentation of the new constants
and helper, which a crate's public API needs in order to be usable.
