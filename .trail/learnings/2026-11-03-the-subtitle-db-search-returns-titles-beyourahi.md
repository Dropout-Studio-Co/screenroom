---
title: "The Subtitle DB search returns titles, not subtitle files"
author: Beyourahi
date: 2026-11-03
branch: main
id: l-feea9ed4
scope: repo
status: active
src: agent
touches:
  - apps/desktop/src/app.rs
  - crates/screenroom-core/src/library/provider_subtitles.rs
---

`GET /v1/by-search` is the only keyed TSDB route and it does not answer `subtitles.items`. It answers a `results` array of *titles* (name, year, media_type, tmdb_id, imdb, language_counts), so a search is two steps: pick the best title by name/kind/year and by the requested language's count, then fetch that title's files with a normal `/v1/by-tmdb/...` or `/v1/by-imdb/...` lookup. Do not call the `subtitles_url` field the results carry; it names an undocumented route. Fixtures that pretend search returns `subtitles.items` will pass while the live client returns nothing.

Also verified live on 2026-11-03: `/get/{id}` answers 302 to `files.thesubtitledb.org` on a different host, so the TLS transport needs an explicit redirect allowlist plus Authorization stripping; the default same-host rule silently refuses the download.

Checked 2026-11-03 against https://thesubtitledb.org/llms.txt and the live API with the owner's key (value never printed).
