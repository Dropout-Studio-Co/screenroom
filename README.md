# Screenroom

Watch your own movies and TV shows on your computer, even with no internet.

Screenroom is a local media player for one person's own library. It reads the
files where they already are, remembers where you stopped, and never needs an
account, a server or a network connection.

## What you can do

- Add folders of films and episodes. Nothing is moved, renamed or copied.
- Browse posters with search, filters and sorting, and resume what you were
  watching from Home.
- Play with pause, jump back or forward 10 seconds, pick audio and subtitle
  tracks, and auto-play the next episode.
- Keep separate viewer profiles, each with its own progress.
- Subtitles come from the film first: a track inside the file, then a sidecar file
  beside it. Only when neither exists can you ask The Subtitle DB, the one online
  subtitle provider, and a subtitle it gives you is kept for offline reuse.
- Run entirely offline. Playback, browsing, resume and the library open no network
  connections; metadata and subtitle lookups happen only when you press for them.

## Requirements

- **Rust 1.85 or newer.** The workspace uses Rust edition 2024.
- **macOS on Apple Silicon.** This is the only platform built and tested so far.
- **libmpv**, the playback engine: `brew install mpv`.
- **FFmpeg and FFprobe**, for running times, posters and conversions:
  `brew install ffmpeg`.
- **A C compiler** for the bundled SQLite. On macOS this comes with the Xcode
  Command Line Tools.

Windows and the Samsung TV client are planned targets, not working installs.
The Windows build has never compiled, and the TV app has never run on a
television.

## Build

From the repository root:

```bash
cargo build --release -p screenroom-desktop
```

The executable is `target/release/screenroom-desktop`.

To build a normal macOS application instead:

```bash
scripts/package-macos.sh          # creates dist/Screenroom.app
```

## Run

```bash
cargo run -p screenroom-desktop
```

Or launch the built copy directly:

```bash
./target/release/screenroom-desktop
```

## First use

1. Open Screenroom. Home shows an empty library the first time.
2. Press **Choose a folder…** and pick the folder that holds your films or
   episodes, then press **Add and scan**. You can also type a full path.
3. The scan counts files as it goes. Cards appear before it finishes, and the
   row ends with "Scan finished".
4. Click a poster to play. Press Space to pause, the left and right arrows to
   jump 10 seconds, and Escape to go back.

Your library lives in
`~/Library/Application Support/Screenroom/library.db`. Your video files are
never modified.

## Configuration

- `SCREENROOM_LIBRARY=/path/to/library.db` keeps the library database somewhere
  else. Without it, the path above is used.
- Folders, playback preferences and backup or restore are handled in the
  Settings screen inside the app.
- Online subtitle lookups need a The Subtitle DB key. Keep it in
  `~/Library/Application Support/Screenroom/subtitles.key` (or set
  `SCREENROOM_TSDB_API_KEY` for a shell launch); lookups that need no key work
  without one. See [docs/SUBTITLE_PROVIDER.md](docs/SUBTITLE_PROVIDER.md).

## Troubleshooting

1. **"Playback: unavailable"** means libmpv could not be loaded. Install it with
   `brew install mpv`, then reopen the app.
2. **No running times or posters** means FFmpeg is missing. Install it with
   `brew install ffmpeg`, then rescan the folder.
3. **A folder shows nothing** usually means the drive is disconnected or macOS
   has not granted access yet. Reconnect it, allow access when asked, and rescan
   the folder in Settings.

## License

MIT. See [LICENSE](LICENSE). Third-party components keep their own licenses; see
[docs/LICENSING.md](docs/LICENSING.md). The Homebrew `mpv` build is GPL and is
for local development only.
