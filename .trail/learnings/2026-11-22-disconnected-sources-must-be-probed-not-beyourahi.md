---
title: "Disconnected sources must be probed, not inferred from the last scan"
author: Beyourahi
date: 2026-11-22
branch: main
id: l-9229290b
scope: repo
status: active
src: agent
touches:
  - "apps/desktop/src/app.rs#probe_sources"
  - "crates/screenroom-core/src/library/sources.rs#probe_source"
  - "crates/screenroom-core/src/library/catalog.rs#SELECT"
---

The desktop reconciled watchers by skipping any source root that was not a directory. The scanner is the only code that reclassifies an unreachable root, so an unplugged SSD stayed 'available' with every files.available flag still 1, and Home kept showing its titles. Reconcile every source, including a missing root: probe the filesystem at launch, on a stopped watcher and at each reconcile, mark a non-available source and zero its file flags while keeping every row, and derive LibraryItem.available from both the file flag and the source state. Continue watching, the Home hero, genre chips, available counts and folder counts must require an available source; the Library should open on available media with an opt-in path to inspect missing rows. Only a scan may restore availability, so a reconnect probe that finds a readable root queues the scan instead of trusting old flags.
