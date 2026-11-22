> **SCOPE AND STACK NOTICE (2026-10-26):** [`ADR-002_RUST_FIRST_STACK.md`](ADR-002_RUST_FIRST_STACK.md) as amended by [`ADR-003_DESKTOP_AND_TV_SCOPE.md`](ADR-003_DESKTOP_AND_TV_SCOPE.md) is the authoritative architecture: desktop/laptop (Windows, macOS, Linux) and smart TVs (Samsung Tizen, LG webOS, Android TV, other compatible TVs) only, with equal priority; shared Rust core, iced + wgpu + libmpv desktop, LightningJS 3 + Blits + TypeScript TV with platform-native playback, SQLite where supported, FFmpeg/FFprobe, MIT first-party code. iOS/Android phone and tablet apps and the Flutter fallback are out of scope. This document's product requirements, entities and evidence rules still apply to desktop and TV; any mobile, Flutter-only or Svelte/Tauri statement inside it is history, not a requirement. No physical-device gate has passed -- see `IMPLEMENTATION_STATUS.md` for what is actually verified.

# Data model, identity invariants and storage contracts

**Status:** proposed conceptual model; final schema/DDL and storage representations depend on the physical desktop/TV gates. References use application-generated stable UUIDs; **never** use filenames, paths, provider IDs or path+size as permanent identity.

## Entity model

| Entity | Meaning / important fields | Invariant |
|---|---|---|
| `Work` | `id`, type `movie/series`, canonical user title, year, genres, confidence/provenance, manual-override flags | Stable application work ID; third-party IDs are mappings. Unknown playable work allowed. |
| `ExternalIdentity` | `(workId/provider/name/externalId/confidence/sourceDate)` | Unique by provider-ID where justified; never user's primary history ID. |
| `Season` | `id/workId/seasonNumber/displayOrder`, specials/unknown season | Display/absolute order can differ from broadcast. |
| `Episode` | `id/seasonId`, season+episode numbers, optional absoluteOrder, runtime/title, aired date | Missing/gap episodes are first-class; ordering needs confidence. |
| `Edition` | `id/workId`, version/cut/language/runtime, optional release signature | Distinct runtimes/edits get distinct resume and markers. |
| `Source` | `id/displayName/type/rootGrantDescriptor/lastSeen/availability/foldersConfigured` | Stable logical identity; mount letter/path can change; permissions are separately revocable. |
| `SourceFolder` | `sourceId/relativePath` — a selected folder inside the source, relative to its root; `""` is the root itself | Keyed by `(sourceId, relativePath)`; only a configured source carries rows; a selected parent covers its descendants. |
| `MediaFile` | `id/sourceId/relativeLocator/size/mtime/fingerprint/streams/availability/editionId` | Physical file identity can survive conservative relink; not erased on unplug. |
| `FileEpisodeMap` | `mediaFileId/episodeId/inMs/outMs/order/confidence` | Supports combined episodes; unknown boundaries explicit. |
| `ArtworkAsset` | `id/entityId/role/provider/credit/mime/dimensions/hash/localHandle/rights/expiry/refresh` | Actual local permitted bytes, not simply CDN URL; no orphan/rights-blind storage. |
| `Profile` | `id/name/localPrefs/language/accessibility` | Device-local, non-authenticated profiles, no cloud linkage. |
| `PlaybackSession` | `id/profileId/mediaFileId/editionId/generation/status/times` | Unique session; stale events cannot overwrite new session progress. |
| `Progress` | `(profileId,editionId,mediaFileId,lastPositionMs/durationMs/watched/updatedAt/lastSessionId)` | Writes are monotonic by validated session/event sequence, not naïve wall clock only. |
| `WatchlistEntry` | `(profileId,workId/createdAt/favorite)` | Stable across provider title corrections and source disconnect. |
| `MetadataOverride` | `entityId/field/value/who=local/updatedAt` | Never overwritten by automatic enrichment without explicit reset. |
| `SegmentMarker` | `editionId/kind/startMs/endMs/provider/confidence/rights` | Validate range against duration/edition; post-credits matter. |
| `BackgroundJob` | `id/kind/target/status/checkpoint/priority/attempts/nextRetry/error` | Cancel/retry/durable restart, bounded concurrency. |
| `DirectoryEntry` | `id/label/uri/protocol/operator/provenance/access/lastChecked/status/actions/rights` | Not an assertion of accessibility or permission just because listed. |

## Identity and linking rules

- Files may share one `Work` but differ by `Edition`. Episodes may map to multiple files or one file containing multiple episode intervals. Keep work/person-centric display separate from physical playback locator.
- Fingerprint/size/mtime plus relative-path hints help **candidate matching**, never silently merge two uncertain files. Rescan can update locator and availability without resetting watched state.
- Manual wrong-match correction creates an explicit protected override and audit event. Metadata-only title changes must not rekey watchlist, history, or progress.
- If an external drive is missing, mark `Source` and its files `offline`; preserve all reference data, poster and history. If permission revoked, separate from `missing` and `unsupported-codec`.
- Handle multiple remux/conversion outputs with lineage: `conversionJobId/sourceFileId/outputFileId/semanticEditionRelationship`; never simply paste progress onto a runtime-shifted different cut.
- Do not tie episode autoplay to array indices; require specific known next number, within source/edition when necessary. Cross-season continuation is allowed only with verified order and no gap.

## Source folder selection (schema 11)

A source is a storage location, and the folders read from it are a separate,
explicit choice ([`ADR-020`](ADR-020_SOURCE_FOLDER_SELECTION.md)). The physical
model is two pieces:

- `sources.folders_configured INTEGER NOT NULL DEFAULT 0`, constrained to `0`/`1`:
  whether this source has an owner-made selection.
- `source_folders(source_id, relative_path)`: one row per selected folder, keyed by
  `(source_id, relative_path)` and referencing `sources(id)`.

Invariants:

1. **Relative only.** A stored path is relative to the source root: no absolute
   path, no `..`, no NUL, and no leading or trailing separator. It is a plain
   locator, validated the same way as `MediaFile.relativeLocator`.
2. **`""` is the root sentinel.** The empty string selects the root itself. A source
   can hold `""` or named folders, never both: the root absorbs every other row.
3. **A parent covers its descendants.** If `Films` is selected, `Films/2024` is
   included whether or not it has its own row. Storing both is redundant, so the
   set is normalised before it is written: children of a selected folder and
   duplicates are dropped, and `""` replaces the set.
4. **The flag decides the scan.** `folders_configured = 0` means the legacy
   whole-root scan and no selection is authoritative. `folders_configured = 1`
   means only `source_folders` rows are read; an empty selected set indexes nothing
   and never falls back to the root.
5. **One owner, one copy.** The selected set lives in these rows. A UI list is a
   view of them, never a second source of truth, and a failed write leaves the
   previous selection intact.

Migration, 10 → 11: additive in one transaction. `source_folders` is created,
`sources.folders_configured` is added with its default, and every existing source
becomes unconfigured — which is its previous whole-root behaviour. No source row is
rewritten, no item is re-indexed, no file is read. A version 12 or newer file still
fails `future_database` and is left untouched.

Withdrawal rules:

- Only a complete scan may withdraw availability; a cancelled, bounded or partial
  scan may not.
- Withdrawal is scoped to the subtree that was scanned. A file becomes unavailable
  only when a complete scan of the selected folder containing it proves it gone.
- A missing selected subfolder is a folder-level condition. It does not set the
  source to `Disconnected` and does not withdraw files in other selected folders.
- Deselecting a folder stops future scans and watches from visiting it. Its rows are
  not deleted; selecting it again returns it to the scan.
- Removal of a source, a disconnected drive or a revoked permission marks rows
  unavailable and preserves works, editions, progress, lists, markers and artwork.
  Nothing is deleted, and no user media is modified, moved or renamed.

## Playback progress invariants

1. Public contract times are integers in **milliseconds**, positive and bounded by validated duration where available. Convert to/from native player units only at adapter boundary.
2. Progress is updated by currently authoritative `(sessionId,generation)` after successful open; ignore prepare zero-duration/zero-position events, outdated callbacks and canceled seeks.
3. Checkpoint at a bounded interval while playing and on pause/seek/stop/background/suspend, using transactional/journaled durability where platform supports it.
4. Explicit `watched`, `unwatched`, `rewatch` remain user-owned decisions. Default approximate 95% position heuristic is merely proposed; don't clobber post-credit or manual states.
5. Profile change closes/currently checkpoints old session before using a new profile; no cross-profile contamination.
6. On crash/power loss, last durable checkpoint is recoverable without overwriting data with an older callback.

## Persistence and schema progression

- Assign schema version, migrations, checksums, conflict rules, backup metadata and transactional staging. **Fail closed** on invalid imports/unknown future schema: report error and leave current library intact.
- Desktop SQLite: indexes for `(workId, type, title)`, `(sourceId, availability)`, `(profileId, updatedAt)`, `(workId, seasonOrder, episodeOrder)`; bounded cursor pagination and FTS/search index where warranted. Do not bake SQL into Svelte components.
- Tizen M76: test app-private file store vs supported IndexedDB; write images to dedicated files, record handles and checksum. Samsung documents `localStorage` 5 MB: not suitable for tens of thousands of works/posters. Journal/checkpoint, atomic file swap if supported, robust corruption recovery, and legitimate private app space quotas must be verified physically.
- Export/import: versioned JSON or archive with **no video bytes**, optional licensed asset cache, manifest/checksums and provenance, profile/history, manual edits and source descriptions; credentials/absolute private paths omitted or explicitly warned; mapping/reselection on new device.
- Background enrichment persisted with conservative rate limiting; don't make API/provider token an unprotected code constant. Invalid provider/image data cannot inject markup/unsafe URLs.

## Example capability and error contracts (illustrative, not source code mandate)

```text
SourceState = AVAILABLE | DISCONNECTED | PERMISSION_LOST | UNMOUNTABLE | MISSING | UNKNOWN
PlaybackOutcome = READY | FAILED_UNSUPPORTED_STREAM | FAILED_MISSING_FILE |
                  FAILED_PERMISSION | FAILED_DECODER | FAILED_UNKNOWN
ProviderJob = QUEUED | WAITING_NETWORK | RUNNING | RATE_LIMITED | NEEDS_CONFIRMATION |
              COMPLETED | FAILED | CANCELLED
Capability = { supported: boolean, reason?: string, verifiedOnDevice?: boolean, ...limits }
```

No fabricated flags: `verifiedOnDevice` can only be true with test evidence. Actual wire schemas will use strict validation, explicit version and safe finite lengths/counts.

## Mandatory invariant tests

Renamed/moved file, Windows changed drive letter, duplicate files at different sources, alternate cuts, unknown/no TMDB, remapped episode, missing season gap, stale player event, seek and profile race, two concurrent scans, corrupt backup, TV dev reinstall and low storage. Progress/watchlist must be stable except where a documented explicit user action changes it.
