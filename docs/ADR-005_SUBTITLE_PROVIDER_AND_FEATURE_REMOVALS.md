# ADR-005 — The Subtitle DB, the subtitle priority, and three removals

- **Status:** Accepted. Four owner decisions, dated 2026-11-03.
- **Date:** 2026-11-03
- **Supersedes:** ADR-010 (OpenSubtitles as the optional subtitle provider; the
  seed row in [`DECISIONS_AND_RESEARCH.md`](DECISIONS_AND_RESEARCH.md) is marked
  superseded rather than deleted), ADR-017 (the `NSAccessibility` bridge and its
  VoiceOver pass), and ADR-019 (HDR presentation, with
  [`ADR-019_HDR_PRESENTATION.md`](ADR-019_HDR_PRESENTATION.md) retired in place).
- **Amends:** F13 and F15 in [`PRD.md`](PRD.md), the F07 and F15 rows of
  [`CAPABILITY_MATRIX.md`](CAPABILITY_MATRIX.md), gate G4 (`Desktop playback
  quality`) in [`FEASIBILITY_GATES.md`](FEASIBILITY_GATES.md), and the macOS
  backlog rows for the same features.
- **Owner decision:** the owner removed three pieces of functionality and fixed
  the subtitle order. None of this is a proposal.
- **Detail:** [`SUBTITLE_PROVIDER.md`](SUBTITLE_PROVIDER.md),
  [`VOICEOVER_PASS.md`](VOICEOVER_PASS.md) (retired pass).

## Decision

### 1. Online subtitles: The Subtitle DB, and nothing else

**The Subtitle DB** (`https://api.thesubtitledb.org`, "TSDB") is the single online
subtitle provider. **OpenSubtitles is removed entirely** — no client, no key
handling, no copy, no fallback, no "other service" row in a capability table.

Lookups by TMDB id, IMDb id, release name and title need no key. The
catalogue search (`/v1/by-search`) needs `Authorization: Bearer sdb_…`. Downloads
answer `302` to `files.thesubtitledb.org`. Lookups and search have no published
rate limit; downloads allow 1 000 per hour anonymously and 100 000 per day with a
key; overload is `503` with `retry-after`.

*Why.* One provider is one contract, one error map and one key to protect, and
TSDB answers the identities this library actually holds (TMDB ids from
enrichment, the release name from the filename) without an account for a lookup.
A second service would double the failure modes for no capability the first one
lacks.

### 2. Subtitle source priority: file, sidecar, then network

Binding behaviour, in order:

1. **Embedded subtitle tracks in the media file.**
2. **Local sidecar files beside the media** — `.srt`, `.smi`, `.vtt`, `.ass`,
   `.ssa`, `.sub`, `.ttml`, `.dfxp` — or a copy you imported into the library.
3. **The Subtitle DB**, and only when neither of the first two exists.

The control is **Find subtitles online**, and it returns before any network call
when a track or a sidecar is present: no socket, no key read, no "searching…"
state. There is no second online service and no fallback chain.

*Why.* A subtitle shipped with the film is the one the release was timed for, and
a sidecar that sits beside it was chosen by the person who has the file. Asking a
catalogue first would overwrite a better answer with a worse one, and it would
make an offline machine look broken when it is not.

### 3. VoiceOver-specific functionality is removed

Gone from the desktop app: the macOS `NSAccessibility` tree bridge
(`apps/desktop/src/accessibility.rs`), its snapshot publication, its
`NSAccessibilityAnnouncementRequestedNotification` announcements and their
receipt plumbing (`SCREENROOM_AX_CHECK`, `SCREENROOM_AX_CHECK_SCREEN`). No
VoiceOver pass is run, and no further VoiceOver receipt is produced. The
[`VOICEOVER_PASS.md`](VOICEOVER_PASS.md) document is retired in place.

**Everything unrelated to VoiceOver stays**, and stays a requirement: keyboard
navigation and a deterministic focus order, focus restoration per screen and
after a modal, the reduced-motion setting, subtitle size and position
preferences, contrast tokens and the 44×44 coarse-target rule.

*Why.* The bridge served one assistive technology through a workaround that
carried the window frame instead of per-widget geometry, so the cursor
highlighted the window rather than a card. It was never accepted by a real
VoiceOver pass. Keeping a half-working tree would cost every future layout change
and would misstate what is supported.

### 4. HDR is removed completely

No HDR mode, no extended-range output, no HDR preference or setting, no HDR
receipt. **SDR is the only desktop output.** `ADR-019_HDR_PRESENTATION.md` and
`HDR_VERIFICATION.md` are retired in place, with their measurements kept as
history; they describe why the path existed and what it delivered, not current
behaviour.

*Why.* The measurement that mattered never happened — a person looking at an HDR
film on the panel — and the machinery behind it was the most expensive thing in
the render path to carry: a vendored `iced_wgpu`, a 16-bit float swapchain, an
extended-range layer, a second colour convention and its own regression suite. The
desktop plays SDR, and that cost bought nothing that was being used.

## Consequences

- **F15 changes shape.** It is no longer "search a subtitle site". It is
  "resolve subtitles in file-first order, and consult TSDB only when the file
  offers nothing", with the resolution order itself as an acceptance condition.
- **One key, in one place.** `SCREENROOM_TSDB_API_KEY`, then
  `SCREENROOM_TSDB_CONFIG`, then `subtitles.key` in the platform config directory,
  honouring `SCREENROOM_CONFIG_DIR`. The owner's key exists at the macOS path; its
  value is never documented, printed or committed.
- **The store does not change.** Imported and downloaded subtitles live in the
  same `subtitles` table, bounded at 4 MiB, text-only, eight formats, with
  provenance. A media file is still never modified, moved or renamed, and no
  sidecar is written beside it.
- **Historical receipts are kept, not rewritten.** The HDR measurements, the
  VoiceOver tree counts and the OpenSubtitles `403` transport check were real
  events. They are marked superseded in place, so the audit trail shows what was
  measured and when it stopped being current.
- **Rows become "removed", not "complete".** F07's HDR half and Q07's VoiceOver
  half are marked removed with this date. A removed feature is not a passing one,
  and the counts in the backlog say so.
- **The vendored `iced_wgpu` had one consumer, and it is gone.** Its only purpose
  was selecting a 16-bit float swapchain for HDR. The directory, the root
  `[patch.crates-io]` entry and its `SCREENROOM_HDR` gate were deleted in this
  change, and `iced_wgpu` resolves from crates.io 0.14.0 again with the ordinary
  MIT obligation ([`LICENSING.md`](LICENSING.md)).
- **No receipt is invented for any of this.** Removing a feature needs no device
  run. The provider's own path has one recorded live run
  ([`SUBTITLE_PROVIDER.md`](SUBTITLE_PROVIDER.md) §8); the two steps that need a
  visible window and a person stay PENDING until someone runs them.

## What this does not change

- The three-device scope, the installed-application contract, offline-first
  behaviour, per-device state, the no-paid-service rule, the media-must-not-be-
  modified rule, and every hardware gate.
- Subtitle *playback*: embedded tracks, sidecars, the track picker, subtitle size
  and position, and the Bengali rendering receipt are untouched.
- Accessibility that is not VoiceOver-specific, as listed in decision 3.

## Exit criteria

Reopen any of the four only on a new owner decision. Reinstating an online
subtitle service, a screen-reader bridge or HDR output each needs its own ADR,
because each brings back a dependency, a failure mode and an unverifiable claim
that this ADR deliberately dropped.
