# Baseline evidence — macOS completion run (2026-11-21)

Base revision: `7ac4273` on `main`, clean, equal to `origin/main`.
Host: MacBook Air `Mac16,12`, Apple M4, 16 GB RAM, macOS 27.2.1 (26B131).
Power during the run: AC attached. Free disk at start: 54 GB.

## Toolchain

| Component | Version | Source |
| --- | --- | --- |
| rustc | 1.99.1, edition 2024 | `rustc --version` |
| iced / iced_wgpu | 0.14.0 / 0.14.0 | `Cargo.lock` |
| wgpu | 27.0.1 | `Cargo.lock` |
| libmpv | 0.42.0, `/opt/homebrew/lib/libmpv.2.dylib` | `mpv --version` |
| FFmpeg | 9.0.2 | `ffmpeg -version` |

## Installed application at start

`~/Applications/Screenroom.app`, `CFBundleShortVersionString` 0.1.6, ad-hoc signed.
Preserved at `.artifacts/macos-completion/Screenroom-0.1.6-previous.app` for the
update-path verification. Latest published release: `v0.1.6`.

## Library

`~/Library/Application Support/Screenroom/library.db`, `PRAGMA user_version` 10,
410 works, `PRAGMA integrity_check` = ok.

Backed up before any work to `.artifacts/macos-completion/library-baseline.db`
(6.9 MB, integrity ok, 410 works). The enrichment workstream keeps its own
pre-drain copy at `.artifacts/macos-completion/library-before-drain.db`.

## Gate at base

| Command | Result |
| --- | --- |
| `cargo test --workspace` | 827 passed, 0 failed, exit 0 |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| `cargo fmt --all --check` | clean |
| `python3 scripts/no_comments.py check` | clean |

The full test log is `.artifacts/macos-completion/baseline-test.log`.

## Signing identity

`security find-identity -v -p codesigning` reports `0 valid identities found`.
`xcrun notarytool history --keychain-profile screenroom` reports no keychain item.
No Apple Developer Program membership is active, so a signed and notarised build
is an accepted limitation for this run and no purchase is attempted.

## Workstream branches

Each workstream runs in its own worktree under `.worktrees/`, branched from
`origin/main` at `7ac4273`, with a pre-commit hook that keeps the no-comments gate
and a no-op post-commit hook so a worker commit can never replace the installed
application:

- `fix/macos-gpu-sync-fence` — item 1
- `fix/macos-ui-focus-and-fit` — items 2 and 3
- `chore/macos-enrichment-drain` — item 4
- `docs/macos-pagination-decision` — item 5
- `docs/macos-dependency-risk` — item 11
- `verify/macos-casting` — item 7
- `verify/macos-durability` — item 8
