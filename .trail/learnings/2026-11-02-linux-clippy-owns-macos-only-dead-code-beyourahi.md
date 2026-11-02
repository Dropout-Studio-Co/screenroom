---
title: linux clippy is the only gate that sees macos-only dead code
author: Beyourahi
date: 2026-11-02
branch: fix/linux-clippy
cost: { minutes: 25 }
id: l-7b41c2aa
scope: repo
status: active
src: agent
touches:
  - apps/desktop/src/accessibility.rs
  - .github/workflows/ci-linux.yml
---

## Decided

`accessibility::announcement` and its `SCAN_PROGRESS` string are used only by
`mod appkit`, which is `#[cfg(target_os = "macos")]`. macOS therefore compiles them
and never reports them; Linux compiles `accessibility.rs` without `appkit`, so with
`-D warnings` both become `dead_code` errors and the lint job fails the whole
`Release` run before any macOS package step can start.

They are now gated `#[cfg(any(target_os = "macos", test))]`: present where
`appkit` and the tests call them, absent where they are dead. `#[allow(dead_code)]`
would have hidden the next such item too, and the tree takes no comments, so the
predicate has to carry the meaning on its own.

## Watch out

macOS clippy is blind to this whole class. Any item outside a `target_os`-gated
module that only that module (or `#[cfg(test)]`) reaches must be gated, and a local
green `cargo clippy --workspace --all-targets -- -D warnings` proves nothing about
it. The `CI (Linux)` lint job is the only local-or-CI gate that sees it, and it
fails the reusable workflow the `Release` lane calls, so the red lands on `main`
rather than on the pull request that introduced it.
