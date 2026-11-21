# Online metadata

Screenroom works offline. Online metadata is optional. It is one of only two
features that can talk to the internet: the other is the subtitle lookup, which
runs only when the file has no embedded track and no sidecar, and which is
described in [`SUBTITLE_PROVIDER.md`](SUBTITLE_PROVIDER.md). Nothing else in the
application opens a connection on its own.

## The key

A TMDB credential is read from, in this order:

1. `SCREENROOM_TMDB_READ_TOKEN` (a v4 read token), then
2. `SCREENROOM_TMDB_API_KEY` (a v3 key), then
3. a key file at the platform's config directory.

The third one is what makes a Finder or Start-menu launch work, because a
double-clicked application inherits none of the shell's environment.

| Platform | Path |
| --- | --- |
| macOS | `~/Library/Application Support/Screenroom/tmdb.key` |
| Windows | `%APPDATA%\Screenroom\tmdb.key` |
| Linux | `$XDG_CONFIG_HOME/screenroom/tmdb.key`, otherwise `~/.config/screenroom/tmdb.key` |

`SCREENROOM_CONFIG_DIR` overrides the directory, which is what the tests use.

The file holds one credential and nothing else. A value beginning `eyJ` that
contains dots is treated as a read token and sent as a bearer header; anything
else is treated as a v3 key and sent as a query parameter. Blank files, values
with whitespace, and values over 512 bytes are refused rather than guessed at.

Create it so only your user can read it:

```
mkdir -p "$HOME/Library/Application Support/Screenroom"
printf '%s\n' 'your-key' > "$HOME/Library/Application Support/Screenroom/tmdb.key"
chmod 600 "$HOME/Library/Application Support/Screenroom/tmdb.key"
```

The key is never logged, never printed, and never written to the library. The
credential type prints itself as `ApiKey(redacted)` or `ReadToken(redacted)`.

## Using it

Settings → *Online metadata* → **Enrich the library online now**. Posters and
synopses are stored in the library, so the same film does not need the network
twice. Provider artwork retention is a separate setting: turning it off keeps the
extracted frame instead of the downloaded poster.

**Retention is the gate on provider posters, not enrichment.** The setting
`metadataArtworkRetention` is unset on a fresh library and unset means
`DoNotStore`, so an enrichment pass fills genres and synopses but stores no
provider poster. Only `keep` stores one. On 2026-11-21 the owner's library was
switched to `keep` for verification: 150 provider posters were stored and every
one decoded with `ffprobe` at its declared size (12,873–139,861 bytes against the
2 MiB budget), while the 93 media-extracted frames were left alone. Turning it
back off stops *new* posters being stored; it does not delete the ones already
there.

To run it without the interface:

```
cargo run --release -p screenroom-core --example enrich_check -- <library.db> 14
```

Add `--drain-only` to drain the queue that already exists **without** re-queueing
the whole library. That distinction matters: the default path resets every `done`
row to `queued`, so on a library that has already been enriched it repeats work
and spends provider quota. `--drain-only` leaves completed rows alone and stops on
a deferred round (rate limit or waiting for the network) instead of hammering the
provider.

It prints the capability, a one-shot provider probe, the queue depth, and a line
per round. A round that reports `succeeded=40` with `remaining` falling is
working; a round with failures and a static `remaining` is usually rate limiting,
and running it again later drains the rest. A work the provider has no entry for
fails with `The provider has no matching entry.` and stays failed rather than
being retried forever; on the owner's library 93 of 410 works are in that state,
91 of them without a parsed year.

## When it is not configured

The capability reports itself as not configured with the exact reason, and the
Settings screen shows that reason instead of pretending to start a pass. Nothing
else in the application depends on the network.

The subtitle provider has its own credential and its own precedence —
`SCREENROOM_TSDB_API_KEY`, then `SCREENROOM_TSDB_CONFIG`, then `subtitles.key` —
recorded in [`SUBTITLE_PROVIDER.md`](SUBTITLE_PROVIDER.md) §3. It is not the same
key as the metadata one, and one being absent never affects the other.
