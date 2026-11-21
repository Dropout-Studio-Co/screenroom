# WS-7 — abrupt-death and full-disk recovery evidence

**Session date:** 2026-11-21 (Dhaka), 10:04–10:11 UTC.
**Branch:** `verify/macos-durability`, from `origin/main` at `7ac4273`.
**Machine:** the owner's MacBook Air M4, macOS, hidden-window mode only.
**Scope:** G9 (durability and interruption) on macOS, item 8 — abrupt death and a
full disk, without risking the real library.

This document is a receipt, not a plan. Every number below comes from a process
that ran on this machine; the raw files are named at the end.

## 1. What this session settled

| Capability | Result | Surface |
| --- | --- | --- |
| Abrupt death during playback (`SIGKILL`) | **PASS** — last bounded checkpoint survived, session closed on reopen, library browsable, resume offered, nothing deleted | Debug desktop binary, real libmpv player, scratch SQLite library, hidden window |
| Abrupt death with a running conversion row | **PASS for the recovery SQL**, with a named limitation: the transcode itself was not in flight (see §4) | Same |
| Full disk during playback, at launch, on backup and on conversion | **PASS** — honest errors, database integrity intact, no partial output presented as complete, nothing deleted, no hang | **Simulated 2 GB APFS volume**, not the boot disk |
| True AC power loss | **NOT RUN** — an agent cannot physically cut power; accepted limitation in §7 | — |

Nothing here closes the whole of G9. Revoked permission and a pulled physical
drive remain the owner's own runs (checklist step 12), and power loss is
accepted as unexercisable by an agent.

## 2. Method and constraints

- Every launch used `SCREENROOM_WINDOW=hidden`, `SCREENROOM_ALLOW_FOREGROUND=0`
  and `SCREENROOM_UPDATE_DISABLE=1`. The frontmost application was sampled before
  and after each run and never changed (`"Terminal"` before and after, both
  scripts), so no window was raised and no Space was switched.
- Every launch used `SCREENROOM_LIBRARY` pointing at a scratch database under
  `.artifacts/macos-completion/durability/` or at
  `/Volumes/ScreenroomDurability`. No run referenced
  `~/Library/Application Support/Screenroom/library.db`.
- Playback was started through the application's own enumerable entry point,
  `SCREENROOM_OPEN_SCREEN=player`, which opens the first catalogue item, starts a
  real `PlaybackTracker` session and writes a checkpoint whenever the position
  moves by at least one second (`apps/desktop/src/playback.rs`,
  `CHECKPOINT_INTERVAL_MS = 1000`).
- Test media was generated with `ffmpeg` (lavfi `testsrc2` + `sine`): a 90 s
  320×180 clip and a 120 s 1280×720 clip. No user media was read or written.
- The full disk was a scratch APFS disk image, never the boot volume. The boot
  volume is `/dev/disk3s1s1`; the simulated volume was `/dev/disk5s1` mounted at
  `/Volumes/ScreenroomDurability`, verified from `mount` and `df` before any byte
  was written, and detached afterwards (`mount` reports no Screenroom volume).

Scratch paths:

```
.artifacts/macos-completion/durability/abrupt/library.db
.artifacts/macos-completion/durability/abrupt/media/Durability Test Clip (2026).mp4
.artifacts/macos-completion/durability/full-disk/screenroom-durability.dmg
/Volumes/ScreenroomDurability/library.db            (inside the image, detached after the run)
```

Reproduction:

```bash
scripts/check-abrupt-death-recovery.sh
cargo run -q -p screenroom-core --example prepare_library -- \
  .artifacts/macos-completion/durability/abrupt/library.db \
  .artifacts/macos-completion/durability/abrupt/media
scripts/check-full-disk-recovery.sh
```

The abrupt-death script prepares nothing itself; it requires the scratch library
and clip above and refuses to run outside `.artifacts/`. The full-disk script
creates, mounts, fills, verifies and detaches its own image, and refuses a mount
point that resolves to the boot volume.

## 3. Abrupt death during playback

Steps actually executed (all times UTC):

1. Baseline snapshot of the scratch library: 1 file, 1 work, 1 source,
   0 sessions, 0 jobs, 0 conversions, no progress row, `integrity_check = ok`,
   `journal_mode = delete`, `user_version = 10`.
2. Launched `target/debug/screenroom-desktop` hidden with the player screen at
   `10:10:06` (pid 97313, epoch 1795255806.87). The session opened and checkpoints
   began landing within a second.
3. Playback was allowed to run past 19 s of media.
4. A `jobs` row in `status = 'running'` and a `conversion_jobs` row in
   `status = 'running'` were committed against the scratch library while the
   application was playing, so the killed process really held interrupted work.
5. Positions were sampled every 500 ms; the last observation was **19042 ms**.
6. `kill -9 97313` at `10:10:40` (epoch 1795255827.97). Recorded signal
   `SIGKILL(9)`, `wait` status **137**.

Database state immediately after the kill:

| Field | After the kill |
| --- | --- |
| `PRAGMA integrity_check` | `ok` (and `quick_check = ok`) |
| sessions total / active | 1 / **1** |
| `jobs` probe row | `running` |
| `conversion_jobs` probe row | `running`, message `encoding` |
| progress position / duration | **20042 ms** / 90000 ms |
| files / works / sources | 1 / 1 / 1 |
| hot journal file | absent (the page cache had already committed) |

One further checkpoint landed between the last sample and the signal (19042 →
20042 ms), which is inside the one-second cadence; the stored value is the last
bounded checkpoint, not a partial write.

Relaunched hidden on the same scratch library at `10:10:30` with no screen
override, which is where the application's own recovery pass runs
(`Library::open` → close every session, requeue every `running` job, fail every
`queued`/`running`/`verifying` conversion):

| Field | After the reopen | Verdict |
| --- | --- | --- |
| `PRAGMA integrity_check` | `ok` | PASS |
| sessions total / active / closed | 1 / **0** / 1 | PASS — the crashed process is no longer authoritative |
| `jobs` probe row | **`queued`** | PASS — retried, not left claiming to run |
| `conversion_jobs` probe row | **`failed`**, message `The app closed before this finished.` | PASS — the documented message |
| progress position / duration | **20042 ms** / 90000 ms | PASS — the checkpoint survived the reopen |
| files / works / sources | 1 / 1 / 1 | PASS — nothing deleted |
| continue-watching rows (`position_ms > 0 AND watched = 0`) | 1 | PASS — the interrupted viewing is offered |
| stored `last_session_id` | the killed session's id | the row still names the session it came from |

Browse and resume were then checked through the application and the core query
path, not only through raw SQL:

- The recovered database was copied and read by the shipped contract example:
  `PASS catalogue has entries: 1 rows`,
  `PASS paging reaches every entry exactly once: 1 unique of 1 after 1 page(s), 0 repeated`.
- The application was launched hidden on the library screen and stayed up for six
  seconds (`alive`), so the recovered catalogue was read by the application's own
  page query.
- The application was launched hidden on the player screen again: it opened a
  **new** session (the stored `last_session_id` changed) and reached
  **3042 ms** of playback, which proves the recovered item was still readable and
  playable.

## 4. Abrupt death during a conversion job — what was and was not exercised

The recovery SQL was exercised on a real `running` conversion row in a real
application process: the row was staged by SQL while the application was playing,
the process was killed with `SIGKILL`, and the next open turned it into
`failed` with exactly `The app closed before this finished.` — the behaviour in
`crates/screenroom-core/src/library/mod.rs` at the `conversion_jobs` update.

What was **not** exercised: an actual `ffmpeg` transcode in flight. The
conversion flow is reachable only from the application's UI (Settings and the
detail screen), and this session was not permitted to take the keyboard or a
visible window, so no transcode could be started by the real application. There
is no environment-gated conversion hook. The full-disk run in §5 does drive the
exact command the application spawns (`apps/desktop/src/conversion.rs`,
`command_for` + `profile_args(Remux)`) but it is the command, not the job
bookkeeping.

The partial-output contract is covered by the core suite, not by this session:
`a_result_is_checked_against_the_original_before_it_counts` in
`crates/screenroom-core/src/library/conversion.rs`.

## 5. Full disk (simulated APFS volume, not the boot disk)

Volume: 2 GB APFS image, mounted at `/Volumes/ScreenroomDurability` as
`/dev/disk5s1`; boot volume `/dev/disk3s1s1`; `mount` confirmed
`/dev/disk5s1 on /Volumes/ScreenroomDurability (apfs, local, …)`. Free space
before the fill: 2,025,664 KB. A library (2 files) and the generated clips were
written onto the image.

Steps:

1. Launched the application hidden on the image's library with the player screen.
   Playback reached **5000 ms** while the volume still had room.
2. Filled the image with `dd` at decreasing block sizes (8 MB down to 1 KB) until
   every size was refused:

```
dd: /Volumes/ScreenroomDurability/ws7-fill.bin-8m.bin: No space left on device
dd: /Volumes/ScreenroomDurability/ws7-fill.bin-1k.bin: No space left on device
2,040,528,896 bytes written by the fill passes
```

`df` still reported 32,868 KB nominally available at 99 % capacity, yet a
**one-kilobyte** write was refused — the APFS container was exhausted, which is
what "full disk" means for the application. This is a simulated volume, not the
boot disk, and the fill files were removed before the image was detached.

Observations on the full volume:

| Attempt | Result |
| --- | --- |
| Playback continuing | Application **alive**, CPU 19.6 % in one `ps` sample, no hang. Checkpoints stopped: 7042 ms at the halfway sample and 7042 ms at the end |
| Database after the stalled playback | `integrity_check = ok`, 2 files, 2 media files on disk, 1 session still active (closed on the next open) |
| Launching the application again on the full volume | Application **alive** after six seconds; it does not crash |
| Launch error, from the same `Library::open` call the launch makes | `Sqlite(SqliteFailure(Error { code: CannotOpen, extended_code: 14 }, Some("unable to open database file")))`; the banner form is `database_error: unable to open database file`, from `CoreError::code()` (`"database_error"`) plus rusqlite 0.41.1's `Display`, which prints the stored message verbatim |
| Backup onto the full volume (`Library::backup_to`, driven by the shipped `library_check` example with its temp destination on the image) | `Error: Custom { kind: StorageFull, error: PathError { path: "/Volumes/ScreenroomDurability/ws7-tmp/.tmpMuyHEC", err: Os { code: 28, kind: StorageFull, message: "No space left on device" } } }` — **0** files left in the destination folder, no partial backup |
| Conversion, the application's exact command, output on the volume | `[out#0/mp4] Error opening output /Volumes/ScreenroomDurability/ws7-converted.mp4: No space left on device` / `Error opening output files: No space left on device` — output **absent**, 0 of 61,099,880 source bytes, nothing recorded as complete |
| Files deleted | None: 2 file rows and 2 media files before, 2 and 2 after, and 2 and 2 again after the reopen |

After the fill data was removed and the volume had room again, the library was
reopened hidden: `integrity_check = ok`, 2 files, the last checkpoint **7042 ms**
and the stored duration 90000 ms survived, and the process stayed up. The image
was then detached; `mount` shows no remaining Screenroom volume.

The one message worth naming rather than smoothing over: on a completely full
volume the launch path says `database_error: unable to open database file`
(SQLite `SQLITE_CANTOPEN`, because no journal or WAL file can be created). It is
accurate and it does not damage anything, but it does not tell a person that the
disk is full. A checkpoint that fails while playing surfaces SQLite's own
`database or disk is full` text through the same banner path; the launch case
cannot, because SQLite refuses to open before it can classify the failure.

## 6. The real library was not touched by this session

`~/Library/Application Support/Screenroom/library.db`, read-only captures:

| | Before (09:55 UTC) | After (10:13 UTC) |
| --- | --- | --- |
| files | 867 | 867 |
| works | 410 | 410 |
| profiles | 1 | 1 |
| sources | 2 | 2 |
| sessions | 25 | 25 |
| progress | 7 | 7 |
| `PRAGMA integrity_check` | ok | ok |
| inode | 507950173 | 507950173 |

Row counts are identical. The file's size, mtime and checksum did change during
the session, and the cause is visible rather than assumed: another workstream's
process, `/tmp/screenroom-ws1-baseline-target/release/screenroom-desktop`
(pid 95012 when the baseline was taken, pid 99906 when the closing capture was
taken, still holding the file open), is writing to that database for its own
baseline capture, and an `enrich_check` run on the same path was doing the same
at the start. Every process this session launched used `SCREENROOM_LIBRARY`
pointing at `.artifacts/…` or `/Volumes/ScreenroomDurability`, and the binary in
both receipts is this worktree's `target/debug/screenroom-desktop` — never the
`/tmp/screenroom-ws1-baseline-target` build and never the installed application.

## 7. Accepted limitation: true AC power loss

An agent cannot physically cut power to the owner's laptop, and pulling the
battery or the charger is the owner's own action on their own machine. The
substitute used here is `kill -9` plus a restart, and what it covers is exactly
the process-death half: the operating system keeps the page cache, so the
filesystem's own durability (ordered journal commits, `fsync` on checkpoint
writes) is **not** tested by it. A true power cut could lose the last committed
transaction that the kernel had not yet flushed, where `SIGKILL` cannot.

What the substitute did establish: SQLite left no hot journal, the database
opened afterwards with `integrity_check = ok`, the last bounded checkpoint was
present, sessions and jobs were reconciled by the next open, and no file row was
lost. What it cannot establish is that the same is true after an abrupt loss of
power to the drive.

Closed by: an owner-run power cut during playback (unplug, or hold the power
button) followed by a relaunch, recorded in the same four snapshots this
document uses. Until then this half stays **PENDING**, not passed.

## 8. Verdict

- **Completed:** abrupt death during playback, abrupt death with a running
  conversion row (recovery SQL), full disk during playback, at launch, on backup
  and on conversion — all on scratch data, all hidden-window, all on this
  MacBook.
- **Verified:** the checkpoints, integrity results, error texts and row counts
  above; no foreground change in either script; no Screenroom volume left
  mounted.
- **Blocked / PENDING:** true AC power loss (agent cannot cut power); a real
  transcode in flight at the moment of death (conversion is UI-only and this
  session had no UI); revoked permission and a pulled physical drive (checklist
  step 12, the owner's run).
- **Gate:** G9 stays **PARTIAL** on the host. It is stronger than "no full disk
  has ever been run" — the full disk now has a receipt on a simulated volume —
  but power loss and the physical-device runs are still open.

## 9. Files produced

```text
scripts/check-abrupt-death-recovery.sh        the abrupt-death run, 21 assertions
scripts/check-full-disk-recovery.sh           the simulated-volume run, 20 assertions
.artifacts/macos-completion/durability/abrupt/recovery-receipt.txt
.artifacts/macos-completion/durability/abrupt/state-0-before.txt
.artifacts/macos-completion/durability/abrupt/state-1-before-kill.txt
.artifacts/macos-completion/durability/abrupt/state-2-after-kill.txt
.artifacts/macos-completion/durability/abrupt/state-3-after-reopen.txt
.artifacts/macos-completion/durability/abrupt/state-4-browse.txt
.artifacts/macos-completion/durability/abrupt/state-5-replay.txt
.artifacts/macos-completion/durability/abrupt/catalog-receipt.txt
.artifacts/macos-completion/durability/full-disk/full-disk-receipt.txt
.artifacts/macos-completion/durability/full-disk/fill-error.txt
.artifacts/macos-completion/durability/full-disk/launch-error.txt
.artifacts/macos-completion/durability/full-disk/backup-error.txt
.artifacts/macos-completion/durability/full-disk/conversion-error.txt
.artifacts/macos-completion/durability/full-disk/df-after-fill.txt
.artifacts/macos-completion/durability/real-library-before.txt
.artifacts/macos-completion/durability/real-library-after.txt
```

`.artifacts/` is Git-ignored, so the receipts live on this machine; this document
carries the numbers and the commands that regenerate them.
