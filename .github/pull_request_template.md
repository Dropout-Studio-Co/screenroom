## What changed

<!-- One or two sentences. What the tree does now that it did not before. -->

## Why

<!-- The requirement, bug or gate this closes. Link the doc or issue. -->

## Verification

Every row must be a command that actually ran, with its real result. A box that
was not run stays unticked and says why.

| Check | Command | Result |
| --- | --- | --- |
| Format | `cargo fmt --all --check` | |
| Lints | `cargo clippy --workspace --all-targets -- -D warnings` | |
| Rust tests | `cargo test --workspace --all-targets` | |
| TV tests (`apps/tv`) | `pnpm typecheck && pnpm test` | |

## Platform limits

<!-- What could not be run here (hardware, OS, window, network) and why. Never
claim a device or rendering result that was not observed on that device. -->

## Risks and rollback

<!-- Blast radius, and how to undo this if it is wrong. -->
