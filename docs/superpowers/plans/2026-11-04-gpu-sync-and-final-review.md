# GPU synchronization and final review — plan (2026-11-04)

Written by GPT-6.1 Sol High (Codex CLI, high reasoning), read-only, against HEAD `6bff3e5`. Implementation by DeepSeek V4.1 Flash.

**9 findings FIXED; 6 NOT FIXED. The GPU defect is confirmed. Recommend per-draw completion leases using wgpu 27’s existing render-pass callback.**

Reviewed clean HEAD `6bff3e5`. No files modified; no builds, tests, or playback executed.

1. **FIXED** — [provider_subtitles.rs:989](/Users/beyourahi/Desktop/projects/screenroom/crates/screenroom-core/src/library/provider_subtitles.rs:989): provider error text redacts the configured key.
2. **FIXED** — [app.rs:2299](/Users/beyourahi/Desktop/projects/screenroom/apps/desktop/src/app.rs:2299): Find requires the matching load and `tracks_ready`.
3. **FIXED** — [app.rs:2431](/Users/beyourahi/Desktop/projects/screenroom/apps/desktop/src/app.rs:2431): Download rechecks embedded tracks and current sidecars.
4. **FIXED** — [rebuild-app.sh:168](/Users/beyourahi/Desktop/projects/screenroom/scripts/rebuild-app.sh:168), [:275](/Users/beyourahi/Desktop/projects/screenroom/scripts/rebuild-app.sh:275): ordinary bundle replacement uses atomic `RENAME_SWAP`.
5. **FIXED** — [rebuild-app.sh:147](/Users/beyourahi/Desktop/projects/screenroom/scripts/rebuild-app.sh:147): the triggering process retries acquisition across the finishing worker’s final-check gap.
6. **NOT FIXED** — [rebuild-app.sh:231](/Users/beyourahi/Desktop/projects/screenroom/scripts/rebuild-app.sh:231), [:315](/Users/beyourahi/Desktop/projects/screenroom/scripts/rebuild-app.sh:315): seventh-build case works, but the ceiling merely moved to twenty; round twenty can delete the twenty-first request.
7. **NOT FIXED** — [rebuild-app.sh:116](/Users/beyourahi/Desktop/projects/screenroom/scripts/rebuild-app.sh:116): normal acquisition never removes a dead lock, so automatic rebuilding can remain blocked indefinitely.
8. **NOT FIXED** — [rebuild-app.sh:143](/Users/beyourahi/Desktop/projects/screenroom/scripts/rebuild-app.sh:143): concurrent `--force` reclaimers can unlink a newly acquired lock.
9. **NOT FIXED** — [rebuild-app.sh:159](/Users/beyourahi/Desktop/projects/screenroom/scripts/rebuild-app.sh:159), [:322](/Users/beyourahi/Desktop/projects/screenroom/scripts/rebuild-app.sh:322): another worktree’s timed-out request is suppressed when the current worktree’s stamp equals its HEAD.
10. **FIXED** — [app.rs:5323](/Users/beyourahi/Desktop/projects/screenroom/apps/desktop/src/app.rs:5323): `a` imports subtitles.
11. **FIXED** — [app.rs:5310](/Users/beyourahi/Desktop/projects/screenroom/apps/desktop/src/app.rs:5310): Backspace/Delete removes the focused kept subtitle.
12. **NOT FIXED** — [player.rs:232](/Users/beyourahi/Desktop/projects/screenroom/apps/desktop/src/ui/screens/player.rs:232), [app.rs:2284](/Users/beyourahi/Desktop/projects/screenroom/apps/desktop/src/app.rs:2284): the 150 px band still receives at least one row plus status/pagination/padding, exceeding its height.
13. **FIXED** — [app.rs:2205](/Users/beyourahi/Desktop/projects/screenroom/apps/desktop/src/app.rs:2205), [:5304](/Users/beyourahi/Desktop/projects/screenroom/apps/desktop/src/app.rs:5304): keyboard focus advances the visible result window.
14. **FIXED** — [app.rs:4907](/Users/beyourahi/Desktop/projects/screenroom/apps/desktop/src/app.rs:4907), [:4641](/Users/beyourahi/Desktop/projects/screenroom/apps/desktop/src/app.rs:4641): surviving menus retain their chrome hold.
15. **NOT FIXED** — [app.rs:2240](/Users/beyourahi/Desktop/projects/screenroom/apps/desktop/src/app.rs:2240), [:5887](/Users/beyourahi/Desktop/projects/screenroom/apps/desktop/src/app.rs:5887): character-count truncation assumes seven-pixel glyphs; wide labels can still consume Download’s allocation.

The GPU chain is confirmed, with one wording correction: the last sample’s callback is **never registered**, rather than registered and never executed. `release_sampled` performs `self.sampled = None`; `note_sampled` registers completion only for `previous` ([video_surface.rs:267](/Users/beyourahi/Desktop/projects/screenroom/apps/desktop/src/video_surface.rs:267), [:299](/Users/beyourahi/Desktop/projects/screenroom/apps/desktop/src/video_surface.rs:299)). Consequently, `begun > finished` persists, `reads_reported()` remains false, and publication-age reclamation increments the counter ([surface.rs:33](/Users/beyourahi/Desktop/projects/screenroom/crates/screenroom-player/src/surface.rs:33), [gl_render.rs:382](/Users/beyourahi/Desktop/projects/screenroom/crates/screenroom-player/src/gl_render.rs:382), [:609](/Users/beyourahi/Desktop/projects/screenroom/crates/screenroom-player/src/gl_render.rs:609)). “Nearly every frame” needs measurement.

Queue completion is appropriate **after the sampling submission**. Crucially, iced trims before submitting: [iced_wgpu/lib.rs:154](/Users/beyourahi/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/iced_wgpu-0.14.0/src/lib.rs:154), [:175](/Users/beyourahi/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/iced_wgpu-0.14.0/src/lib.rs:175). Registering a queue callback in `trim` covers the previous submission ([wgpu queue.rs:277](https://github.com/gfx-rs/wgpu/blob/v27.0.1/wgpu/src/api/queue.rs#L277)).

Metal’s `addCompletedHandler` directly signals command-buffer completion ([Apple](https://developer.apple.com/library/archive/documentation/Miscellaneous/Conceptual/MetalProgrammingGuide/Cmd-Submiss/Cmd-Submiss.html)). Public unsafe access exists through `CommandEncoder::as_hal_mut` and Metal `raw_command_buffer`, but `Primitive::draw` receives only a render pass ([command_encoder.rs:270](/Users/beyourahi/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/wgpu-27.0.1/src/api/command_encoder.rs:270), [metal/command.rs:36](/Users/beyourahi/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/wgpu-hal-27.0.4/src/metal/command.rs:36)). No native bridge is necessary: wgpu 27 exposes `RenderPass::on_submitted_work_done`, registered against the actual submission ([render_pass.rs:286](/Users/beyourahi/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/wgpu-27.0.1/src/api/render_pass.rs:286)).

The producer needs actual reader completion before overwriting. Today, `strong_count == 1` protects Rust frame ownership, `!write.pending` protects unfinished OpenGL writes, and `reads_settled` protects consumer reads only until grace expires ([gl_render.rs:378](/Users/beyourahi/Desktop/projects/screenroom/crates/screenroom-player/src/gl_render.rs:378)). Imported textures and `Arc<SurfaceReads>` do not hold that Rust surface ownership ([video_surface.rs:245](/Users/beyourahi/Desktop/projects/screenroom/apps/desktop/src/video_surface.rs:245)).

| Design | Assessment |
|---|---|
| (a) Queue callback from `trim` | Incorrect timing; potentially early release. |
| (b) Ring drained in next `prepare` | Can cover previous submissions; needs a separate final-frame drain and bounded backpressure. |
| (c) Direct release on pipeline Drop | Unsafe while submitted sampling remains active. |
| (d) Grace fallback | Acceptable only behind real ownership protection; elapsed time cannot establish completion. |

Implement the following:

- In [video_surface.rs:212](/Users/beyourahi/Desktop/projects/screenroom/apps/desktop/src/video_surface.rs:212), retain `draw(&self, pipeline: &Self::Pipeline, render_pass: &mut wgpu::RenderPass<'_>) -> bool`. Validate binding/frame correspondence; register completion for **every successful draw**, including unchanged-generation redraws.
- Replace `sampled`, `SampledSurface`, `note_sampled`, and `release_sampled` with proposed `PendingSurfaceRead { surface: Arc<SurfaceFrame>, generation: u64, pump: Arc<CompletionPump> }`. Capture this guard in the completion closure; its Drop acknowledges and releases ownership. Unsubmitted encoder disposal cancels its lease safely. Preserve bindings across trim ([video_surface.rs:224](/Users/beyourahi/Desktop/projects/screenroom/apps/desktop/src/video_surface.rs:224)).
- Add `CompletionPump { device: wgpu::Device, pending: AtomicUsize }` and `pub fn poll_completions() -> Result<(), String>`. Register weak pump references; poll outside registry locks from existing ticks ([app.rs:5092](/Users/beyourahi/Desktop/projects/screenroom/apps/desktop/src/app.rs:5092), [:5458](/Users/beyourahi/Desktop/projects/screenroom/apps/desktop/src/app.rs:5458)). Callback-owned surface Arcs protect reuse and pool shrink even when generation counters already indicate completion ([gl_render.rs:710](/Users/beyourahi/Desktop/projects/screenroom/crates/screenroom-player/src/gl_render.rs:710)).

Headless tests should use [device():772](/Users/beyourahi/Desktop/projects/screenroom/apps/desktop/src/video_surface.rs:772), render an imported IOSurface offscreen, trim **before submit**, submit, then explicitly poll with a bounded timeout. Prove final-frame release without another draw, overlapping same-generation leases, pipeline Drop, discarded encoders, binding preservation, and refusal to recycle after 250 ms while a lease remains. Cover resize, 4K, shrink, film changes, and device-loss cancellation. Missing Metal hardware must report unavailable, not silently pass.

Run workspace tests, clippy, formatting, and no-comments checks. Compare release-build `SCREENROOM_PLAY_CHECK` runs using `SCREENROOM_PLAY_CHECK_MEDIA`, `SCREENROOM_PLAY_CHECK_SECONDS=60`, and hidden-window mode; also run `hwdec_measure auto-copy gpu-sync "$MEDIA" 60` ([play_check.rs:5](/Users/beyourahi/Desktop/projects/screenroom/apps/desktop/src/play_check.rs:5), [hwdec_measure.rs:13](/Users/beyourahi/Desktop/projects/screenroom/crates/screenroom-player/examples/hwdec_measure.rs:13)). Hidden playback cannot certify GPU presentation ([play_check.rs:503](/Users/beyourahi/Desktop/projects/screenroom/apps/desktop/src/play_check.rs:503)).

Acceptance: no overwrite before completion, eventual tail release, zero normal-path reclaims, stable bindings and no pacing/memory regression. Risks: missing polling stalls the bounded pool; premature acknowledgement corrupts frames; device loss requires quarantining/recreating old slots, never timeout-based permission to overwrite.

Independent source review corroborated the GPU recommendation.  
📝 Started from Beyourahi’s trim-lifecycle learning. 🌱 Trail saved ~135k tokens in primary retrieval.

▶ Next: implement completion leases and address the six residual findings.
