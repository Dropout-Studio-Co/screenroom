# WS-4 — large-library pagination: measured decision

**Date:** 2026-11-21
**Branch:** `docs/macos-pagination-decision`, based on `origin/main` at `7ac4273`.
**Scope:** macOS only (ADR-004, development priority 2026-11-05).
**Companion item:** [`MACOS_REMAINING_WORK.md`](../MACOS_REMAINING_WORK.md) P2-05 (re-check
large-library paging on the grouped query).
**Outcome:** keep `LIMIT ? OFFSET ?` plus the separate count. No query change. No code change
outside the benchmark. Revisit at 25,000 works, at a 250 ms grouped last page, or when the
interface stops showing page numbers.

---

## 1. The question

F11 (search, filter, sort, paging) needs a bounded, honest answer at three library sizes:

- the owner's real library: **410 works** (session brief, 2026-11-21);
- the PRD criterion D size: **10,000 rows**;
- the product stretch: **50,000 rows**.

The open question is whether SQLite's `OFFSET` paging, run over a `ROW_NUMBER()` window that
collapses files into works, is good enough for Screenroom Personal Edition, or whether the
library query has to move to keyset (cursor) pagination before the desktop client can be called
finished.

## 2. What the code does today

`crates/screenroom-core/src/library/catalog.rs` builds the page in two steps.

Ungrouped (`group_by_work = false`):

```sql
SELECT * FROM (<SELECT …> WHERE <filters>) ORDER BY <sort> LIMIT ? OFFSET ?
```

Grouped (`group_by_work = true`, what the Library screen uses):

```sql
SELECT * FROM (
  SELECT ROW_NUMBER() OVER (
           PARTITION BY w.id
           ORDER BY CASE WHEN f.available=1 AND s.state='available' THEN 0 ELSE 1 END,
                    COALESCE(f.season_number,0), COALESCE(f.episode_number,0), f.id) work_rank,
         <SELECT …>
  FROM files f JOIN editions e … JOIN works w … JOIN sources s …
  LEFT JOIN progress pr ON pr.edition_id = e.id AND pr.profile_id = ?
  LEFT JOIN watchlist wl ON wl.work_id = w.id AND wl.profile_id = ?
  WHERE <filters>
) WHERE work_rank = 1
ORDER BY <sort> LIMIT ? OFFSET ?
```

Count is a second, independent statement over the same filtered projection:
`COUNT(DISTINCT work_id)` when grouped, `COUNT(*)` when not.

The four sorts are ([`library/mod.rs`](../../crates/screenroom-core/src/library/mod.rs)):

| Sort | `ORDER BY` |
| --- | --- |
| Title | `title COLLATE NOCASE, COALESCE(season_number, 0), COALESCE(episode_number, 0), id` |
| Added | `added_ms DESC, id` |
| Year | `year DESC, title COLLATE NOCASE, id` |
| Progress | `progress_updated_ms DESC, id` |

The desktop client is page-numbered: `PAGE_ROWS = 6`, `DEFAULT_COLUMNS = 4`, so
`PAGE_SIZE = 24`; `page_count = ceil(total / 24)`; `reload_browse` runs `count`, then `items`
at `offset = page * 24`, then `known_genres` — every page turn. Page numbers are a visible,
keyboard-navigable part of the interface, not an implementation detail.

## 3. Method

| Item | Value |
| --- | --- |
| Host | MacBook Air, Apple M4, macOS 27.2.1 (build 26B131) |
| Build | `--release`, `rustc 1.99.1` |
| SQLite | 3.54.1 (rusqlite 0.41.1, `bundled`) |
| Date | 2026-11-21 |
| Bench | `cargo run --release -p screenroom-core --example library_bench -- <N> --runs=3` |
| Repetition | 3 independent invocations per size, 3 samples per query per invocation (9 samples per cell) |
| Storage | in-memory database (the bench's default and the basis of the earlier planning numbers); a file-backed run at 50k and 100k is reported separately in §4.2 |
| Ingest | recorded separately and excluded from every query median |

The benchmark grew three flags, nothing else:

- `--runs=K` — take the median of K samples per query instead of one, and print the samples.
- `--database=PATH` — build the library at a real file so the same database can be inspected, and so the file-backed cost can be measured.
- `--explain=DIR` — write the five plan queries (grouped first, grouped last, ungrouped first, count ungrouped, count grouped) as `.sql` files, and print the first-page ids from the real `Library::items` call.

The plan queries are a copy of the catalog SQL at `7ac4273`, because `catalog::SELECT` and
`where_clause` are private and `Library::connection` is not public. The copy is verified, not
assumed: for each of the three page queries the copied SQL returns exactly the 24 ids the real
`Library::items` call returns, and both count copies return the same total (10,000 at 10k).
Anything that changes `catalog.rs` must be re-verified the same way.

## 4. Measurements

### 4.1 In-memory medians (ms)

Ingest, median of three: 13 ms at 410, 313 ms at 10k, 793 ms at 25k, 1,696 ms at 50k,
3,731 ms at 100k. Ingest is a scan-time cost and is excluded from everything below.

| Query | 410 page / count | 10k page / count | 25k page / count | 50k page / count | 100k page / count |
| --- | --- | --- | --- | --- | --- |
| first page (Title) | 0.36 / 0.16 | 6.1 / 5.3 | 15.6 / 13.7 | 35.1 / 31.0 | 76.7 / 69.9 |
| last page (Title) | 0.58 / 0.16 | 14.7 / 5.4 | 41.2 / 14.4 | 88.0 / 31.5 | 188.2 / 69.5 |
| Added ordering | 0.58 / 0.16 | 12.7 / 5.3 | 32.8 / 13.8 | 69.5 / 31.3 | 145.6 / 69.2 |
| Year ordering | 0.39 / 0.15 | 5.9 / 5.3 | 15.5 / 14.1 | 32.8 / 30.0 | 74.8 / 68.1 |
| Progress ordering | 0.37 / 0.16 | 5.8 / 5.2 | 15.2 / 13.9 | 33.3 / 31.6 | 71.3 / 67.5 |
| literal search | 0.27 / 0.22 | 5.9 / 5.9 | 15.6 / 15.6 | 33.7 / 34.1 | 73.1 / 74.2 |
| search, no match | 0.27 / 0.20 | 5.8 / 5.8 | 15.4 / 15.6 | 34.1 / 34.3 | 73.4 / 73.1 |
| **grouped first page** | **1.03 / 0.26** | **26.3 / 9.3** | **67.2 / 26.2** | **137.5 / 59.7** | **302.7 / 177.4** |
| **grouped last page** | **1.22 / 0.25** | **39.3 / 9.5** | **131.9 / 26.3** | **311.7 / 59.1** | **733.8 / 174.1** |
| known genres | 0.27 | 8.2 | 23.6 | 49.1 | 109.3 |

The grouped rows are the ones the Library screen actually issues. The spread across the three
invocations was tight enough to trust the medians:

| Cell | Run 1 | Run 2 | Run 3 |
| --- | --- | --- | --- |
| grouped first page, 10k | 26.3 | 26.6 | 26.2 |
| grouped first page, 50k | 137.5 | 140.6 | 137.2 |
| grouped first page, 100k | 302.7 | 298.0 | 314.5 |
| grouped last page, 10k | 39.4 | 39.3 | 39.2 |
| grouped last page, 25k | 131.9 | 130.3 | 133.3 |
| grouped last page, 50k | 321.8 | 311.7 | 311.7 |
| grouped last page, 100k | 739.9 | 732.2 | 733.8 |

A page turn is `grouped count` + `grouped items` + `known_genres`, because that is what
`reload_browse` does:

| Library | Page turn, first page | Page turn, last page |
| --- | --- | --- |
| 410 | 1.6 ms | 1.7 ms |
| 10k | 44 ms | 57 ms |
| 25k | 117 ms | 181 ms |
| 50k | 246 ms | 420 ms |
| 100k | 589 ms | 1,017 ms |

### 4.2 File-backed (secondary)

The app opens a real database file; the table above is in-memory and is therefore a lower bound.
At 50k (34 MB database) the file-backed medians were stable and about 10% higher than in-memory:

| 50k, file-backed | page | count |
| --- | --- | --- |
| first page | 65.3 | 60.5 |
| last page | 122.4 | 60.4 |
| grouped first page | 166.5 | 89.8 |
| grouped last page | 343.2 | 89.6 |
| known genres | 79.3 | — |

At 100k (97 MB database) the same queries were dominated by page-cache flushes from the
ingest that had just written the file: grouped last page measured 802–929 ms, and the
ungrouped first page varied 158–478 ms on the same query. Those runs are recorded here as
evidence of variance, not used for the decision; the in-memory 100k column is the cleaner
measure of the query itself.

### 4.3 Corroboration of the earlier single samples

The planning numbers this work started from were single samples: 10k grouped first 27.5 ms,
grouped last 42.6 ms, 50k 140.8 ms and 378.8 ms. The medians above are 10k 26.3 / 39.3 and
50k 137.5 / 311.7. The 50k grouped-last single sample was 21% pessimistic. Use the medians;
treat a single sample as indicative only.

## 5. Query plans

Captured with the same bundled SQLite (3.54.1) against the 10k database the bench built.
Node ids trimmed; every line below is verbatim.

**Ungrouped first page** — full scan, two temp sorts, no index for the requested order:

```
SCAN f
SEARCH s USING INDEX sqlite_autoindex_sources_1 (id=?)
SEARCH e USING INDEX sqlite_autoindex_editions_1 (id=?)
SEARCH w USING INDEX sqlite_autoindex_works_1 (id=?)
SEARCH pr USING INDEX sqlite_autoindex_progress_1 (profile_id=? AND edition_id=?) LEFT-JOIN
SEARCH wl USING INDEX sqlite_autoindex_watchlist_1 (profile_id=? AND work_id=?) LEFT-JOIN
CORRELATED SCALAR SUBQUERY 1
  SEARCH fe USING INDEX sqlite_autoindex_file_episodes_1 (file_id=?)
  USE TEMP B-TREE FOR ORDER BY
CORRELATED SCALAR SUBQUERY 2
  SEARCH fe USING INDEX sqlite_autoindex_file_episodes_1 (file_id=?)
  USE TEMP B-TREE FOR ORDER BY
USE TEMP B-TREE FOR ORDER BY
```

**Grouped first page and grouped last page** — same plan for both; only the offset differs:

```
CO-ROUTINE (subquery-3)
  CO-ROUTINE (subquery-5)
    SCAN f
    SEARCH s / e / w / pr / wl (as above, all by index)
    USE TEMP B-TREE FOR ORDER BY        <- the window's PARTITION BY / ORDER BY
  SCAN (subquery-5)
  CORRELATED SCALAR SUBQUERY 1 …
  CORRELATED SCALAR SUBQUERY 2 …
SCAN (subquery-3)
USE TEMP B-TREE FOR ORDER BY            <- the requested sort, over every grouped row
```

**Count, ungrouped:**

```
SCAN f
SEARCH e USING INDEX sqlite_autoindex_editions_1 (id=?)
SEARCH w USING COVERING INDEX sqlite_autoindex_works_1 (id=?)
SEARCH s USING INDEX sqlite_autoindex_sources_1 (id=?)
SEARCH pr USING COVERING INDEX sqlite_autoindex_progress_1 (…) LEFT-JOIN
SEARCH wl USING COVERING INDEX sqlite_autoindex_watchlist_1 (…) LEFT-JOIN
```

**Count, grouped** — the plain projection plus a distinct pass; no window:

```
USE TEMP B-TREE FOR count(DISTINCT)
SCAN f
SEARCH s / e / w / pr / wl (w and wl covering)
```

What the plans say, in the terms the question was asked:

1. **The window function is not index-supported and is not free.** SQLite implements the
   `ROW_NUMBER()` projection as `CO-ROUTINE (subquery-5)` and sorts its input in a temp B-tree.
   There is no `MATERIALIZE` node in this plan, but the co-routine is fully consumed by the
   outer query before the outer sort, so every candidate file row passes through the window
   before `work_rank = 1` can be decided. At 100k the window plus its sort costs roughly
   225 ms over the 77 ms ungrouped page.
2. **No sort is index-supported.** Every one of the four sorts ends in
   `USE TEMP B-TREE FOR ORDER BY` over the whole filtered set. `title COLLATE NOCASE`,
   `COALESCE(season_number,0)`, `COALESCE(pr.updated_ms,0)` and the partitioned window are not
   the kind of expressions a plain index satisfies.
3. **A grouped page pays two full sorts.** One inside the window, one for the requested order —
   which is why the grouped page is 3.9x the ungrouped page at 100k, and why the *last* page is
   another 2.4x on top: `OFFSET` walks and discards rows from the already sorted set.
4. **`LIMIT 24` buys nothing.** The cost is Θ(N log N) per page regardless of page size, so
   page 1,287 costs nearly the same as page 2 plus the offset walk.
5. **The count is a second full pass with a different tail.** Ungrouped count is the same scan
   with covering-index joins and no sort; grouped count adds `count(DISTINCT)`'s temp B-tree,
   which is why the grouped count is 2.5x the plain count at 100k (177 ms against 70 ms).
6. **The genre list is not free either.** `known_genres` is 109 ms at 100k, and it rides on
   every page turn.

## 6. Keyset pagination: what it would require, and why it does not settle this

Keyset (seek) paging replaces `OFFSET n` with a predicate on the last row's sort key. For
Screenroom it would require all of the following.

1. **A strict total order per sort.** Today's sorts do append `id` as a tiebreaker, so this part
   is satisfied. Every comparison would still have to be spelled with the same collation
   (`title COLLATE NOCASE`) and the same expression, including `COALESCE(season_number,0)`,
   `COALESCE(pr.updated_ms,0)` and SQLite's null ordering under `DESC` for `year`.
2. **The cursor to carry every key column.** For Title that is four values, for Year three, and
   for Progress a value that only exists after the `progress` join — a keyset predicate cannot
   be pushed into an index when the key is a `COALESCE` over a left-joined table.
3. **The grouping to be rewritten first.** `work_rank = 1` is only known after `ROW_NUMBER()`
   has seen every candidate row. A cursor predicate appended to the current outer query would
   therefore leave the window's temp B-tree exactly where it is. Keyset only pays off once the
   "one representative file per work" decision becomes an index-driven join (a per-work
   representative column or index, or a correlated `LIMIT 1`), which is a schema and index
   change plus a query rewrite, not a pagination swap.
4. **Indexes that do not exist.** No index covers `w.title`, `w.year`, `f.added_ms` or the
   coalesced progress key in the order the interface asks for, and a covering index for the
   grouped projection would have to span three tables.

And keyset does not remove the costs this decision actually weighs:

- **The count survives.** Page numbers need `total`, and `total` needs `COUNT(DISTINCT work_id)`
  over the same filtered set — 177 ms at 100k, measured. Keyset does not touch it.
- **Page numbers conflict with keyset.** Jumping to page 37 has no cursor; either the interface
  keeps `OFFSET` for jumps (paying both), or page numbers go away (infinite scroll or
  next-page-only), which is a product decision this workstream cannot take by itself.
- **A page turn would still refresh the count and the genre list.**

So keyset plus a page-numbered interface is the worst of both: the window and the count stay,
and `OFFSET` stays for jumps.

## 7. Decision

**Accept `LIMIT ? OFFSET ?` and the separate count for Screenroom Personal Edition. Change no
query, add no index, add no cursor.** The measured reasons:

- **The real library is three orders of magnitude below the cost.** At 410 works the grouped
  page median is 1.03 ms (first) and 1.22 ms (last), and a complete page turn is 1.6–1.7 ms; the
  noisiest single sample seen on this host was 2.6 ms. The bench at 410 is one file per work, so
  a series-heavy real library has more rows per work; even a 3x factor stays under 5 ms, because
  at that size the fixed per-query overhead dominates.
- **10,000 rows, the PRD size, stays interactive.** Grouped page 26–39 ms, page turn 44–57 ms,
  count 9 ms. This is the size MACOS_REMAINING_WORK P2-05 asks to be under 100 ms for the first
  page; the measured first page is 26 ms.
- **50,000 rows, the stretch, is slow but usable.** Grouped page 138 ms first / 312 ms last,
  count 60 ms, page turn 246–420 ms; 343 ms for the grouped last page on a warm file-backed
  database. Nothing here blocks the interface or risks data; it is a stretch scenario the
  product is not expected to reach.
- **100,000 rows is the measured worst case and the reason for the revisit line.**
  Grouped last page 734 ms, page turn 1,017 ms; 802–929 ms for the same page on a file-backed
  database. It is recorded, not defended.
- **A rewrite now would buy nothing measurable at 410 and would cost a schema decision, four
  cursor contracts and the page-number model** — for a Personal Edition library that is smaller
  than the padding on the 10k benchmark.

### Revisit threshold

Reopen this decision — and then do the keyset workstream described in §8 rather than tuning
`OFFSET` — if any one of these becomes true:

1. **A real library exceeds roughly 25,000 works.** At 25,000 the grouped last page measures
   132 ms with a 26 ms count: 1.9x headroom under the 250 ms line, which is the last size at
   which the interface is comfortably interactive.
2. **A grouped last page exceeds 250 ms on the owner's MacBook.** The measurements cross 250 ms
   somewhere between 25,000 (132 ms) and 50,000 (312 ms); that crossing has not been measured
   precisely and does not need to be, because the first condition fires earlier.
3. **The interface stops showing page numbers.** Without page numbers the count's main
   justification disappears and a cursor becomes viable; that is the point at which keyset stops
   fighting the product.

Until one of those is true, the pagination code stays as it is and `library_bench` is the
regression guard.

## 8. Re-running this measurement

Timings — three invocations per size, medians of three samples each, ingest excluded by
construction:

```bash
cargo run --release -p screenroom-core --example library_bench -- 10000 --runs=3
cargo run --release -p screenroom-core --example library_bench -- 25000 --runs=3
cargo run --release -p screenroom-core --example library_bench -- 50000 --runs=3
cargo run --release -p screenroom-core --example library_bench -- 100000 --runs=3
cargo run --release -p screenroom-core --example library_bench -- 410 --runs=3
```

Query plans, on the same bundled SQLite, without touching the owner's library:

```bash
cargo run --release -p screenroom-core --example library_bench -- \
  10000 --runs=1 --database=/tmp/screenroom-bench.db --explain=/tmp/screenroom-plans
for sql in /tmp/screenroom-plans/*.sql; do
  sqlite3 /tmp/screenroom-bench.db "EXPLAIN QUERY PLAN $(cat "$sql")"
done
```

Numbers to watch, in this order:

| Number | Budget | 2026-11-21 |
| --- | --- | --- |
| grouped first page, 10k | under 100 ms (P2-05) | 26.3 ms |
| grouped last page, 25k | under 250 ms | 131.9 ms |
| grouped last page, 50k | recorded, not budgeted | 311.7 ms |
| grouped count, 50k | recorded | 59.7 ms |
| grouped page, 410 | under 10 ms | 1.03 / 1.22 ms |

A change to `catalog.rs`, the four sorts, `PAGE_SIZE`, or the schema invalidates this table.
Re-run all five sizes and update it. If the plan output gains a `MATERIALIZE` node or loses a
`USE TEMP B-TREE FOR ORDER BY` line, the cost model in §5 changed and the decision has to be
re-read, not just the numbers.

## 9. What would have to be true to switch to keyset

All four, together:

1. **The interface is cursor-driven.** Next/previous or infinite scroll, no page numbers, so the
   total becomes optional (lazy, cached, or dropped) and no jump needs an offset.
2. **The grouping is index-driven.** A representative file per work is selected by an index
   (stored column, or a covering index on `(work_id, available, season, episode, file_id)` with a
   correlated `LIMIT 1`), so no `ROW_NUMBER()` pass over every file remains.
3. **The sort keys are indexable.** Either the keys are materialised where an index can order
   them, or the collations and `COALESCE` expressions are replaced by stored values.
4. **A real library is past the revisit line**, or the owner accepts page turns over ~250 ms —
   and the rewrite is then justified by measurement at that size, not by anticipation of it.

Meeting 1–3 alone would be a schema and index change with no user-visible gain at 410 works.
Meeting 4 alone leaves the count and the offset where they are.

## 10. What this workstream did not change

- `crates/screenroom-core/src/library/catalog.rs` — untouched. No query, filter, sort or index
  change. The 410-work measurement shows no regression to justify one.
- `apps/desktop` — untouched. `PAGE_SIZE = 24` and the page-number model stand as built.
- Schema and migrations — untouched, so no migration is owed.
- The only code change is `crates/screenroom-core/examples/library_bench.rs`: median mode,
  file-backed mode, and plan capture with the id-equivalence check described in §3.
