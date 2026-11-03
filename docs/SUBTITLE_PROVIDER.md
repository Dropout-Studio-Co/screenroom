# Subtitles: where a track comes from

Screenroom uses a subtitle the file already has before it asks anyone for one.
There is exactly one online subtitle provider, **The Subtitle DB** (TSDB), and it
is only reached when the file has neither an embedded track nor a sidecar. This
document records the order, the provider's routes, the authentication, the error
mapping and the bounds. The decision is
[`ADR-005`](ADR-005_SUBTITLE_PROVIDER_AND_FEATURE_REMOVALS.md) (2026-11-03).

**Status (2026-11-03).** The behaviour below is the binding product behaviour.
Imports, storage and offline reuse are implemented and covered by tests. The
provider was replaced by TSDB under this decision, and its own code path — key
discovery, anonymous lookup, keyed search and a cross-host download — was run once
against the live service on this host (§8, with the output). What is still
**PENDING** is the part that needs a visible window and a person: a downloaded
subtitle drawn in the player, and offline reuse after a restart. No screenshot and
no measured latency is claimed for anything that did not run.

## 1. The priority chain

The question is never "search for a subtitle". It is "resolve subtitles for this
file", and it is answered from the file outwards.

| # | Tier | Where it comes from | Network | When it is chosen |
| --- | --- | --- | --- | --- |
| 1 | **Embedded track** | a subtitle stream inside the media file, read from the player's own track list | none | any subtitle track exists in the file |
| 2 | **Sidecar file** | a file beside the media: `.srt`, `.smi`, `.vtt`, `.ass`, `.ssa`, `.sub`, `.ttml`, `.dfxp` — or a copy you imported into the library | none | no usable embedded track, and a sidecar or kept copy exists |
| 3 | **The Subtitle DB** | `https://api.thesubtitledb.org` | yes, and only here | neither of the above exists, and you press *Find subtitles online* |

Rules that hold at every tier:

- **A tier that resolves ends the resolution.** When a track or a sidecar is
  present, the online action returns before it opens a socket, reads the key or
  shows a consent step. Proving the transport is never called in that case is an
  acceptance condition of the change, so the rule is checked rather than assumed.
- **A track list that has not arrived is not an empty track list.** Until the
  player has reported the loaded file's tracks, the action refuses to look online
  and says why, because it cannot yet rule out a subtitle track inside the file.
- **One provider, no chain.** There is no second online service, no mirror, no
  "try the other one" fallback and no silent automatic search. OpenSubtitles was
  removed on 2026-11-03 and appears nowhere as a live provider.
- **Nothing is guessed.** A track with no language tag is listed as Unknown rather
  than assumed English. A downloaded file's language, format and release come
  from the provider's own record, not from the filename.
- **The media file is never touched.** A download is stored in the library's
  `subtitles` table with its provenance, exactly like an import. No sidecar is
  written, renamed, moved or deleted beside the media.
- **An image track is still a track.** PGS and VobSub are drawn by the decoder when
  it can draw them; the chain does not prefer a text download over a track the file
  already carries.

## 2. Routes used

Base: `https://api.thesubtitledb.org`. Lookups are anonymous; only the catalogue
search requires a key, and the download route accepts one to raise its allowance.

| Route | What it answers | Auth |
| --- | --- | --- |
| `GET /v1/by-tmdb/movie/{id}` | a film, by TMDB id | none |
| `GET /v1/by-tmdb/tv/{id}/season/{s}/episode/{e}` | one episode, by the series' TMDB id | none |
| `GET /v1/by-imdb/{id}` | a film, or a series with `/season/{s}/episode/{e}` | none |
| `GET /v1/by-releasename?release=…` | a release name, such as the file's own name | none |
| `GET /v1/by-title?q=…&type=movie\|tv` | one resolved title; not a candidate list | none |
| `GET /v1/by-search?q=…&type=movie\|tv` | catalogue search | `Authorization: Bearer sdb_…` |
| `GET /get/{id}` | the subtitle bytes; answers 302 to `files.thesubtitledb.org` | optional: `Bearer` raises the download allowance |

Parameters: `lang`, `format`, `limit` (at most 100) and `offset` (at most
10 000). The lookup order is the identity the library actually has: a known TMDB
or IMDb id first, then the release name, then the title, and the keyed catalogue
search only if those found nothing. An episode is resolved through its series and
then drilled into the exact season and episode — a season pack is never offered
as an episode's subtitle.

Source for the contract: <https://thesubtitledb.org/llms.txt>.

## 3. The key

The credential is read from, in this order:

1. `SCREENROOM_TSDB_API_KEY`;
2. the key inside the file named by `SCREENROOM_TSDB_CONFIG`;
3. `subtitles.key` in the platform's config directory.

| Platform | Path |
| --- | --- |
| macOS | `~/Library/Application Support/Screenroom/subtitles.key` |
| Windows | `%APPDATA%\Screenroom\subtitles.key` |

`SCREENROOM_CONFIG_DIR` overrides the directory. The third location is what makes
a Finder or Start-menu launch work, because a double-clicked application inherits
none of the shell's environment.

The owner's key already exists at the macOS path. **Its value is never written
down, printed, logged or committed** — not in this document, not in a test, not
in a receipt. The file is read once, held in memory for the request, and its
presence alone is what a capability report shows.

A key is attached to two things only: `/v1/by-search`, which refuses to answer
without one, and `/get/{id}`, where it raises the download allowance from 1 000 an
hour to 100 000 a day. It is never attached to a lookup route, never written into a
URL, never followed across a host change, and it is **not sent at all over a plain
`http` base address** — a keyed configuration pointed at `http` is reported as
not configured rather than shipping the credential in the clear. An invalid or
missing key is reported as such: it is never reinterpreted as anonymous access,
and lookups that need no key keep working without one.

The key file must be readable by your own account only. A file that other accounts
can read is refused with the `chmod 600` that fixes it, rather than being used.

## 4. Published limits

- Lookups and search: **no published rate limit**.
- Downloads: **1 000 per hour anonymous**, **100 000 per day with a key**.
- Overload is answered **503 with `retry-after`**; the app waits at least that
  long before trying again, and gives up after a bounded number of attempts.

The app never retries earlier than the provider says, and reports a refusal
instead of quietly hammering the service. Within one action the transport retries
at most three times, honouring `retry-after` and backing off exponentially, under a
per-minute budget of 120 requests and one overall deadline. The desktop app builds a
provider per action, so that budget and the backoff state start fresh on each press;
because the action is a person pressing a control rather than a loop, the published
allowances are far above what the app can use.

## 5. What each answer means

| Answer | Where | Meaning | What the app does |
| --- | --- | --- | --- |
| **404** | any `/v1/by-*` lookup | no match for that identity | a **miss**, not an error: try the next lookup in the order, then say plainly that no subtitle was found |
| **404** | `GET /get/{id}` | the record exists, the file does not | **unavailable**: say so, offer the other results, never keep a partial file |
| **400** | any route | the request itself is wrong (a malformed id or parameter) | **actionable**: not retried; the message names what was wrong |
| **401**, or `key_required` | `/v1/by-search` | no key, or a key the provider refuses | **actionable**: name the key file and the environment variable, never the value; do not fall back to anonymous and pretend the search ran |
| **429** | any route | too many requests | retried no earlier than `Retry-After`, with a bounded attempt count |
| **503** | any route | provider overload | the same: retried no earlier than `Retry-After`, bounded |
| **5xx, timeout, TLS failure** | any route | provider or network failure | reported as a failure; the library, the progress and the playing file are untouched |

A failed online step never removes a kept subtitle, never changes the selected
track, and never leaves a half-written row.

## 6. Bounds

- **4 MiB per subtitle**, text only. A file with a NUL in its first 8 KiB, an
  unknown extension or an oversized body is refused rather than stored.
- **Eight formats**: `.srt`, `.smi`, `.vtt`, `.ass`, `.ssa`, `.sub`, `.ttml`,
  `.dfxp`. Anything else is refused by the same rule for an import and for a
  download.
- **Pages of at most 100 results**, `offset` at most 10 000, and no unbounded crawl
  of the catalogue.
- **Redirects**: at most three, HTTPS only, and only to the TSDB API host and
  `files.thesubtitledb.org`. `Authorization` is stripped whenever the host
  changes, and a redirect to any other host — loopback included — is refused.
- **One deadline** spans connect, handshake, write, read and every redirect hop,
  and a JSON body is bounded before it is parsed.

## 7. Offline

Nothing in the chain requires the network:

- an embedded track is read from the file by the decoder;
- a sidecar is read from the same folder;
- a kept subtitle is stored as bytes in the library, so it survives the file
  moving, the drive being unplugged and the machine being offline.

Turning the network off costs you only the third tier. The player still lists and
selects every track it has, the kept-subtitle panel still works, and nothing
hangs waiting for a request. `scripts/check-no-network.sh` is the measurement that
playback holds no socket at all; the Wi-Fi-off run was performed by the owner on
2026-11-03 and is recorded in [`MACOS_REMAINING_WORK.md`](MACOS_REMAINING_WORK.md)
(P1-12).

## 8. Verification, and what is not verified

Implemented and tested off-device (these run against fixtures and the store, with
no network):

```bash
cargo test --workspace                      # store, import bounds, round-trip, provider fixtures
cargo test -p screenroom-desktop --lib acceptance
```

**Settled on the owner's MacBook, 2026-11-03.** The provider was exercised through
its own code path — key discovery, anonymous lookup, keyed search, cross-host
download redirect and text validation — not with `curl` alone:

```bash
SCREENROOM_TSDB_LIVE=1 cargo run -q -p screenroom-core --example tsdb_live
```

Observed, on the owner's key at `~/Library/Application Support/Screenroom/subtitles.key`
(the value was never printed, logged or committed):

```text
provider: thesubtitledb
configured: true
detail: Online subtitle lookups and keyed title search are configured for The Subtitle DB.
keyed search: true
lookup matches: 10          (Parasite, TMDB 496243, Bengali)
first match: id=12557726 language=bn format=srt cues=1660
keyed search matches: 65
downloaded: 196269 bytes, format srt, cues observed 1660, first cue on line 1
episode matches: 4          (Game of Thrones S04E02, IMDb tt0944947, Bengali)
tsdb_live: ok
```

The same session also confirmed, against the live service, that a malformed
language code answers `400`, a wrong media kind and a missing id answer `404`, and
`/get/{id}` answers `302` to `files.thesubtitledb.org` before the bytes. Those are
provider facts observed with `curl`; the run above is the receipt for this
application's own transport, authentication and parsing.

Two checks stay PENDING, because they need a visible window and a person:

```bash
# A film with no track and no sidecar, in the installed application:
#   player → t (or the overlay's Find online) → pick a result → the text
#   appears → quit,
#   turn the network off, reopen, play: the kept subtitle still works and the
#   media file's bytes and modification time are unchanged.
scripts/package-macos.sh && open dist/Screenroom.app
```

**Also not verified:** the Windows path (no build on that machine); a downloaded
subtitle drawn inside the player window; the player's own subtitle panel as drawn
on screen (its messages are covered by tests, its layout has not been looked at by
a person); the offline-reuse step above; and the TV
client, which has no online subtitle provider at all — it reads embedded tracks
and the four sidecar documents its decoder accepts (`.smi`, `.srt`, `.sub`,
`.ttxt`). Nothing here is claimed to have happened beyond the recorded run.

## 9. Rights and retention

TSDB grants API use; it does not grant a right to redistribute a subtitle that
someone else wrote. Screenroom stores a downloaded file for the owner's own
personal playback, keeps its provenance with it, and never uploads, re-shares or
redistributes it. That is the same rule as imported files and retained artwork,
and it is recorded in [`LICENSING.md`](LICENSING.md).
