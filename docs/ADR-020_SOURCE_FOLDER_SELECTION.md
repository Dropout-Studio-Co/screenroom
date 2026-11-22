# ADR-020 — A source is a location; the library reads only the folders you choose

- **Status:** Accepted. Owner decision, dated 2026-11-23.
- **Date:** 2026-11-23
- **Amends:** F01 in [`MACOS_REMAINING_WORK.md`](MACOS_REMAINING_WORK.md) (now
  folder-level selection, with a gating sub-row), the F01 row of
  [`CAPABILITY_MATRIX.md`](CAPABILITY_MATRIX.md), the schema and scanning sections
  of [`ARCHITECTURE.md`](ARCHITECTURE.md) and the schema progression of
  [`DATA_MODEL.md`](DATA_MODEL.md).
- **Relates to:** ADR-002 (Rust-first stack), ADR-004 (Personal Edition scope),
  F01/F02/F22 in [`PRD.md`](PRD.md).
- **Owner decision:** selection is per folder inside a location. This is not a
  proposal.

## The question

A source was a whole root: the owner granted a location, and everything below it
was read. That is wrong for the drives this application is actually pointed at. One
external SSD can hold the films, a series archive, a backup, installers, samples and
a conversion output folder, and only some of those are the library. A whole-root
source indexes all of it: scan time and row count grow with material nobody
intends to watch, and the library stops describing a collection and starts
describing a disk.

The alternative the product forbids is reorganising the media on disk. Screenroom
never moves, renames or deletes a file, so the application has to express "only
these folders" itself, on top of a location that stays stable when a drive is
unplugged and replugged.

The same change answers the first-run problem. An application with no source, or
with a source that holds nothing playable, opened onto Home and Library screens
that had nothing to show and no way to say why.

## Decision

### 1. A source is a storage location, not a folder of media

A source is one location: a local folder, an external drive or a USB volume. Its
identity is the location and its volume identity, not the set of folders being
read, so relink, rescan and removal keep working as they do today. One source can
carry an arbitrary set of **selected folders**.

### 2. Selection is relative to the source root, and `""` is the root

Each selected folder is stored as a path **relative to the source root**. The empty
string `""` means the root itself. Within one source, sibling folders, nested
folders and individual folders combine, and a selected parent covers its
descendants.

The set is normalised before it is stored: duplicates collapse, a selected child is
absorbed by a selected ancestor, and a root selection (`""`) absorbs everything
else. The normalised set is what the database holds and what the scanner walks, so
two selections that describe the same subtrees are the same row set.

### 3. New sources start unconfigured; existing libraries are unchanged

At the core layer a source created by `add_source` carries `folders_configured = 0`,
which means the legacy behaviour: a scan reads the whole root. That is what keeps
every existing library working without a migration decision. The desktop does not
leave a newly picked location in that state: it writes a configured-but-empty
selection immediately, so no scan — including the automatic reconcile — reads the
location before the owner has chosen folders. A typed path is an explicit
whole-location choice and is saved as a configured root selection (`""`).

Configuring the source writes its selection and sets `folders_configured = 1`. From
then on a scan reads only the selected subtrees. A configured source with an empty
selection indexes nothing; it does not fall back to the whole root, because the
owner asked for nothing and silently reading everything would be the opposite of
what the control says.

### 4. Schema 11

`SCHEMA_VERSION` becomes **11**. The change is additive:

- `sources.folders_configured INTEGER NOT NULL DEFAULT 0`, constrained to `0`/`1`.
- `source_folders(source_id, relative_path)`, one row per selected folder, keyed by
  both columns and referencing `sources(id)`.

Opening a version 10 file migrates it forward in one transaction. No source row is
rewritten, no item is re-indexed and no file is touched; every existing source comes
out unconfigured, which is exactly its previous behaviour. A version 12 or newer
file still fails `future_database` and is left untouched.

### 5. Scanning reads only the selected subtrees, in place

The scanner is given the source and its normalised selection and walks those
subtrees, not the root. Everything else about the walk is unchanged: breadth-first,
the entry and depth bounds, the settle window, symlinks never followed, hidden
directories skipped, one unreadable folder producing an issue rather than ending the
walk, and one scan per source at a time.

The desktop's filesystem watcher follows the same boundary. A source is watched at
its selected folders; a change in a folder that is not selected raises nothing,
because the application is not reading that folder.

### 6. Withdrawal is scoped to the subtree that was scanned

A complete scan may withdraw availability, and a partial, cancelled or bounded scan
may not — the existing rule. Its scope is now the selected subtree: a file becomes
unavailable only when a complete scan of the folder that contains it proves it gone.
One explicit addition: a file outside the saved selection is withdrawn on any scan
that runs to the end, even when that scan is otherwise partial, because the owner has
already removed its folder from the selection and its presence on disk says nothing
about whether it belongs in the library.

A missing selected subfolder is a folder-level condition. It does not mark the
source `Disconnected`, and it does not withdraw files in the folders that are still
present and selected. Source-level states stay what they were: `Disconnected`, a
moved or missing root (`Missing`), `Access lost` for a revoked permission, and
`Removed` for a source taken out of the library. A folder that cannot be listed says
so where it is listed.

A folder that is no longer selected is simply not visited. Its rows are never
deleted, and selecting it again puts it back in the scan.

Nothing in this ADR changes what withdrawal is: a row is marked unavailable. Media is
never modified, moved, renamed or deleted, and a disconnected drive or a deselected
folder keeps its works, progress, lists and posters.

### 7. The Sources screen manages one source at a time

The screen is a source rail beside a folder browser. The rail lists each source with
its label, its state and its location, plus **Add a location**. The browser shows
the current folder's breadcrumb, an **Everything in this folder** row for the
current folder itself, and each subfolder with its indexed media count and the
inclusion state it holds: selected, included by a selected parent, or excluded. It
carries **Scan selected folders**, **Clear selection**, **Rescan**, and the
per-source **Remove** and **Relink** actions, with add available from the rail.

Direct media counts are the files directly in that folder, so a parent's count does
not stand in for its children. That is deliberate: a folder holding 2 films and 40
subfolders reads as 2, and the subfolders are opened to see their own counts.

### 8. Three application states

1. **No sources** — the welcome state: what Screenroom does, **Add a media
   location**, and a typed absolute path as the alternative.
2. **Sources, zero indexed media** — a purpose-built empty state that says the
   locations are ready but hold nothing playable yet, lists them with their state,
   and offers **Review folders**, **Add another location** and **Scan all
   locations**.
3. **At least one indexed item** — the main interface.

Home and Library are not offered while there is no media to show. The navigation
keeps Settings and the Sources screen reachable, so the way out of either empty
state is the same screen that caused it.

### 9. The add flow reviews before it reads

**Add a media location** opens the native folder picker, adds the location, and
lands on its folder browser so the selection can be reviewed before anything is
scanned; the scan runs from **Scan selected folders**.

The typed-path alternative adds the location with the root selected and scans it,
so a path typed by hand behaves like the whole-root source it names.

## Consequences

- **F01 becomes a folder-level capability.** "Scan this location" and "scan these
  folders of this location" are different promises, and the second is the one the
  screen makes.
- **The library reflects the choice.** A configured source contributes only its
  selected folders; the deselected remainder is not visited by later scans or
  watches, and its rows are kept, not deleted.
- **One more state to keep honest.** Source state and folder state are separate, so
  a missing subfolder cannot be reported as a broken drive, and a broken drive
  cannot be hidden behind a present subfolder.
- **Migration stays boring.** The flag defaults to `0`, so the whole-root behaviour
  survives the upgrade and no library loses a row.
- **Cost follows the selection.** Scan and watch work are proportional to the
  selected subtrees rather than to the size of the location.
- **The screen is denser than the rest of the app.** A rail and a browser with
  breadcrumbs and per-row inclusion need a deterministic keyboard order and a
  focus model; they are part of the change, not a follow-up.

## Alternatives rejected

- **Reorganise the media on disk** — moving films into one folder per library is the
  obvious simplification and it is forbidden: the application never modifies,
  moves, renames or deletes user media.
- **Exclusion patterns instead of folder selection** — a pattern list cannot answer
  "what will be indexed" before the scan, it hides the skipped material, and it
  invites patterns that silently do nothing. A folder selection is enumerable,
  visible in the browser and countable.
- **One source per selected folder** — a drive would appear as several sources with
  the same volume identity, relink and removal would multiply, and the drive's own
  state could no longer be reported once.
- **Keep the whole-root scan and filter in queries** — the noise and the scan cost
  stay, and the application would keep statting files it will never show.
- **One selection shared by every source** — drives hold different material, and a
  selection belongs to the location it was made in.
- **Recall the last folder and read it silently** — a scan the owner did not ask
  for is indistinguishable from reading a folder they excluded.

## Evidence status

- **Verified by automated tests, not by a person.** The change is covered by the
  core suite — schema 11 migration, selection round-trip and normalisation, the
  subtree walk, and withdrawal that stays inside the scanned subtree — and by the
  desktop suite — the folder browser's inclusion states, the three application
  states, and the navigation gating. `cargo test --workspace` is the check that
  runs them.
- **PENDING: owner visual acceptance.** The Sources screen, the welcome state, the
  empty state and the hidden navigation have not been looked at by the owner, and
  no window capture of them has been recorded. Tests can prove a row is present and
  a message is reachable; they cannot say whether the screen reads clearly.
- **PENDING: a new capture.** No screenshot pass of this change has been recorded,
  so the interface claims here rest on the code and the tests, not on a picture.
- **No device result is claimed.** This is a macOS desktop change on the owner's
  host. The Windows desktop and the Samsung TV keep their own gates, and neither has
  been run for this change.

## What this does not change

- Media stays read-only: scan and read in place, report failures, never clean up
  destructively.
- Identity, progress, watchlists, favourites, markers and artwork are untouched, and
  a deselected or disconnected folder keeps all of them.
- Volume identity and relink, the bounds and cancellation rules of the scan, and the
  rule that only a complete scan withdraws availability.
- Offline-first behaviour, the installed-application contract, the three-device
  scope and every hardware gate.
- The fail-closed database rules: an unreadable, corrupt or future schema is
  reported and left alone.

## Exit criteria

Reopen this ADR only on a new owner decision. A per-file selection model, observing
folders the owner excluded, or reading a location without a scan would each need
their own decision, because each contradicts a rule stated here.
