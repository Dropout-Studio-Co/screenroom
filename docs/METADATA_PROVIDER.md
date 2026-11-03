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

To run it without the interface:

```
cargo run --release -p screenroom-core --example enrich_check -- <library.db> 14
```

It prints the capability, the queue depth, and a line per round. A round that
reports `succeeded=40` with `remaining` falling is working; a round with failures
and a static `remaining` is usually rate limiting, and running it again later
drains the rest.

## When it is not configured

The capability reports itself as not configured with the exact reason, and the
Settings screen shows that reason instead of pretending to start a pass. Nothing
else in the application depends on the network.

The subtitle provider has its own credential and its own precedence —
`SCREENROOM_TSDB_API_KEY`, then `SCREENROOM_TSDB_CONFIG`, then `subtitles.key` —
recorded in [`SUBTITLE_PROVIDER.md`](SUBTITLE_PROVIDER.md) §3. It is not the same
key as the metadata one, and one being absent never affects the other.
