# Metadata enrichment drain — evidence (WS-3, 2026-11-21)

Owner: WS-3 core data engineer. Branch `chore/macos-enrichment-drain`, cut from
`origin/main` at `7ac4273`. Nothing pushed, merged or published by this workstream.

Scope: drain the real macOS library's online-metadata queue once, without
re-enqueueing the tasks that had already completed, and record what the provider
actually produced. Library, credentials and artwork are the owner's real data on
the owner's MacBook Air M4.

Run window: 21 Nov 2026, 15:54–15:59 Dhaka time.

## 1. Preconditions

| Check | Result |
| --- | --- |
| `pgrep -fl Screenroom` before the run | empty (no process, no writes from the app) |
| Library | `~/Library/Application Support/Screenroom/library.db`, `user_version` 10, 7,094,272 bytes |
| Backup | `.artifacts/macos-completion/library-before-drain.db` (Git-ignored), 7,094,272 bytes |
| Backup `PRAGMA integrity_check` | `ok` |

The key stayed inside the example: it was read by `ProviderConfig::from_environment()`
from `~/Library/Application Support/Screenroom/tmdb.key` and was never printed,
copied, logged or written by any command in this session. The example prints fixed
text and provider error codes only. No private title, poster or address appears in
this document.

## 2. Artwork retention decision

`artwork_retention()` treats a missing setting as `DoNotStore`, and the settings
table was empty, so `apply_provider_artwork` was returning `false` before this run
and no provider poster could be kept. The schema is
`settings(key TEXT PRIMARY KEY, value TEXT NOT NULL)`.

Decision: enable provider artwork retention for this verification, and leave it on
afterwards so the posters just stored are the ones a later refresh keeps.

```sql
INSERT OR REPLACE INTO settings(key, value) VALUES('metadataArtworkRetention', 'keep');
```

Result: the settings table holds exactly one row, `metadataArtworkRetention = keep`.
No artwork row was deleted by hand. To revert:

```sql
DELETE FROM settings WHERE key = 'metadataArtworkRetention';
```

## 3. Commands

```sh
DB="$HOME/Library/Application Support/Screenroom/library.db"

sqlite3 "$DB" ".backup '/Users/beyourahi/Desktop/projects/screenroom/.artifacts/macos-completion/library-before-drain.db'"
sqlite3 "/Users/beyourahi/Desktop/projects/screenroom/.artifacts/macos-completion/library-before-drain.db" "PRAGMA integrity_check;"

cargo run --release -p screenroom-core --example enrich_check -- "$DB" --drain-only --rounds 0
cargo run --release -p screenroom-core --example enrich_check -- "$DB" --drain-only 20
```

The first line is a dry run: it prints the capability, the pre-flight probe and the
queue depth with `--rounds 0`, so a rejected key would stop the run before a single
queue row is claimed. The second line is the bounded drain (20 rounds of up to 40
tasks). `--drain-only` skips `enqueue_whole_library`, which would otherwise reset
every completed row to `queued` with `attempts = 0`.

## 4. Before and after

Recorded with read-only connections (`sqlite3 -readonly`).

| Measure | Before | After |
| --- | --- | --- |
| `enrichment_queue` rows | 727 | 727 |
| state `queued` | 241 | 0 |
| state `rateLimited` | 0 | 0 |
| state `waitingNetwork` | 0 | 0 |
| state `failed` | 2 | 93 |
| state `done` | 484 | 634 |
| works | 410 | 410 |
| works with genres | 315 | 315 |
| works with `genres = '[]'` | 95 | 95 |
| artwork rows | 113 | 243 |
| artwork rows from a media frame (`file_id NOT NULL`) | 113 | 93 |
| artwork rows from the provider (`file_id IS NULL`) | 0 | 150 |
| works with any stored artwork | 113 | 243 |
| works without stored artwork | 297 | 167 |
| files | 867 | 867 |
| sources | 2 | 2 |
| external identities | 317 | 317 |
| `metadata_origin = legacy` / `provider` | 93 / 317 | 93 / 317 |
| `PRAGMA integrity_check` | `ok` | `ok` |
| `PRAGMA foreign_key_check` | — | no rows |
| database size | 7,094,272 bytes | 18,558,976 bytes |

Artwork provenance, compared against the backup:

- 20 media-extracted frames were replaced by a provider poster for the same work
  (the retention path owns that row, by design).
- 130 works that had no artwork row gained a provider poster.
- 93 media-extracted frames are untouched, on works the provider did not match.
- 150 posters added 12,077,229 bytes of image data; the database grew by
  11,464,704 bytes.

## 5. Drain round log

```
provider: configured=true language=en-US detail=Online metadata is configured.
probe: ok
enqueue: skipped
queue: queued=241 running=0 rateLimited=0 waitingNetwork=0 failed=2 done=484
round 1: attempted=40 succeeded=40 failed=0 rateLimited=0 waitingNetwork=0 remaining=201
round 2: attempted=40 succeeded=40 failed=0 rateLimited=0 waitingNetwork=0 remaining=161
round 3: attempted=40 succeeded=40 failed=0 rateLimited=0 waitingNetwork=0 remaining=121
round 4: attempted=40 succeeded=30 failed=10 rateLimited=0 waitingNetwork=0 remaining=81
round 5: attempted=40 succeeded=0 failed=40 rateLimited=0 waitingNetwork=0 remaining=67
round 6: attempted=40 succeeded=0 failed=40 rateLimited=0 waitingNetwork=0 remaining=67
round 7: attempted=40 succeeded=0 failed=40 rateLimited=0 waitingNetwork=0 remaining=28
round 8: attempted=28 succeeded=0 failed=28 rateLimited=0 waitingNetwork=0 remaining=0
round 9: attempted=0 succeeded=0 failed=0 rateLimited=0 waitingNetwork=0 remaining=0
done: succeeded=150 failed=158 remaining=0
```

No 401, 403 or 429 answer occurred; no round reported `rateLimited` or
`waitingNetwork`, so the run never had to stop for the provider. `failed=158` counts
attempts of the same 93 queue rows, not 93 distinct final states: a row that is not
yet at its third attempt returns to `queued` and is retried in a later round.

The drain stopped on its own: round 9 found nothing due, so `remaining` reached 0
after 8 productive rounds. `--drain-only` means the 484 rows that were already
`done` were never touched — verified below.

## 6. Poster verification

Every provider poster (150 rows, all `image/jpeg`) was read from the database,
written to a temporary directory, and decoded:

```sh
ffprobe -v error -select_streams v:0 -show_entries stream=codec_name,width,height -of json <file>
```

| Check | Result |
| --- | --- |
| rows checked | 150 |
| decoded by ffprobe | 150 |
| decode failures | 0 |
| stored width/height vs decoded width/height | 0 mismatches |
| JPEG signature (`FF D8 FF`) | 150 |
| smallest poster | 12,873 bytes |
| largest poster | 139,861 bytes |
| budget (`MAX_ARTWORK_BYTES`) | 2,097,152 bytes |
| rows over budget or empty | 0 |
| total poster bytes | 12,077,229 |
| dimensions | 500×750 (148), 444×666 (1), 500×751 (1) |

The temporary directory was removed after the check.

## 7. The unmatched works

93 queue rows are `failed` with `attempts = 3`, `next_attempt_ms = 0`, all with the
target kind `work` and the single message `The provider has no matching entry.`
(`ProviderError::NotFound`). None of them is being retried again by this drain.

Generic shape of the set, without titles:

- all 93 are `kind = movie`;
- 90 carry no parsed year, 3 do, so the search was mostly asked for a title alone;
- none has a recorded provider identity, so none was matched and then lost;
- none is a manual match;
- all 93 still own their files, and 25 of them keep a media-extracted frame;
- 91 of them had already failed once or twice before this run, which is why the
  previous depth showed 2 terminal failures and 241 queued rows.

The two rows that were already terminal before the run
(`27668ff8-…` and `8b3d896a-…`) are still `failed` at `attempts = 3` with the same
message and were not requeued.

## 8. Non-regression checks

- Queue rows: 727 before, 727 after, and no `done` row changed
  (`state`, `attempts`, `updated_ms`, `last_error` all identical to the backup) —
  the 484 completed tasks were not re-enqueued.
- No `queued` row disappeared; every one of the 241 moved to `done` or `failed`.
- `works`, `files` and `sources` counts unchanged; every work id present before is
  present after.
- `PRAGMA integrity_check` `ok` and `PRAGMA foreign_key_check` clean after the run.
- `user_version` still 10.

## 9. Code change

`crates/screenroom-core/examples/enrich_check.rs`:

- `--drain-only` skips `enqueue_whole_library` and prints `enqueue: skipped`;
  the default (enqueue plus drain) is unchanged.
- `--rounds N` and bare `ROUNDS` both set the round count when given; eight rounds
  is still the default. `--rounds=` and `--rounds N` are both accepted.
- options are parsed by the pure `parse_options` function, whose errors are
  `MissingPath`, `MissingRounds`, `UnknownFlag`, `UnexpectedArgument` and
  `InvalidRounds`; every case exits 2 with the usage line.
- a pre-flight search probe runs before anything is enqueued or claimed, so a
  rejected key stops the run with exit 4 and a `probe rejected: <code>` line
  instead of marking queue rows failed one by one.
- a round that reports `rateLimited` or `waitingNetwork` also exits 4 and prints
  `stopped`, so the rest waits for a later run.
- each round line now also reports `rateLimited` and `waitingNetwork`, and the
  queue depth is printed once before the rounds.

`crates/screenroom-core/Cargo.toml` gains:

```toml
[[example]]
name = "enrich_check"
test = true
```

Without it `cargo test -p screenroom-core` compiles examples but does not run their
`#[cfg(test)]` tests. Six parser tests now run in the default package test run:
default shape, flag before/between/after the positional values, unknown flag,
missing value for `--rounds`, named and joined rounds plus an unusable value,
missing path and a surplus argument.

## 10. What the integrator must decide

1. **`failed` is 93, not 2.** The brief's acceptance expected at most two terminal
   failures. The truth is 93 works with no provider entry: every queued row that
   had a match succeeded, and every row that failed had already failed before this
   run. Nothing is stuck; `queued`, `rateLimited` and `waitingNetwork` are 0.
   Option: keep as is, or make `ProviderError::NotFound` terminal on the first
   attempt instead of spending three on a deterministic negative.
2. **Genres did not increase (315 before and after).** Every queued row with a
   provider match was already enriched by an earlier pass, so the 150 successes
   produced no genre delta. The remaining 95 empty-genre works are the 93 unmatched
   ones plus 2 that matched and received provider metadata but whose provider entry
   supplied no genres. The backlog note about 297 works without genres was the
   artwork count, not a genre count.
3. **A rejected key is not a systemic stop in `drain_enrichment_queue`.** An
   `Unauthorized` answer is handled as a per-task failure, so a bad key would walk
   the whole queue marking rows `failed` and bumping `attempts`. This tool now
   guards itself with the pre-flight probe, but the library behaviour is a real
   defect worth a small fix in `provider_queue.rs` (break and defer like
   `RateLimited`, instead of failing each task).
4. **Artwork retention is now on in the owner's library** (`metadataArtworkRetention
   = keep`). The 150 stored posters are real provider poster bytes at 500×750.
   Revert with the `DELETE` in section 2 if the product decision is to keep
   media-extracted frames instead.
5. `enrich_check` still drains with a 120-request-per-minute pacer, so a full
   library pass takes minutes; that is the provider budget, not a fault.

## 11. Re-running

```sh
cargo run --release -p screenroom-core --example enrich_check -- \
  "$HOME/Library/Application Support/Screenroom/library.db" --drain-only 20
```

`--drain-only` is safe to repeat: it never re-enqueues a completed row. A plain run
without the flag re-arms the 93 `failed` rows (attempts reset to 0 by
`enqueue_whole_library`), which is the only way to retry them.
