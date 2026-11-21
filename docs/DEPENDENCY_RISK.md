# Dependency risk register

**Scope:** Screenroom Personal Edition, macOS-first. This register covers the Rust
dependency graph (`Cargo.toml`, `Cargo.lock`) of the desktop application and the
two workspace crates it links. It does not cover the Tizen client in `apps/tv`,
which has its own `pnpm` lockfile, or the FFmpeg and libmpv sources that
[`RELEASE_PIPELINE.md`](RELEASE_PIPELINE.md) §1 builds and ships separately.

**What a row here means.** An advisory in `Cargo.lock` is not the same as a
defect in the application. Some are reachable code in the shipped macOS binary;
some are only in the graph of a target this product does not build. The register
records which, because that difference decides whether the item is a live risk or
a lockfile entry waiting for an upstream move.

**Last full review:** 2026-11-21. Toolchain: `cargo 1.99.1 (5f94df478 2026-08-27)`,
`rustc 1.99.1 (58cf8f9c3 2026-10-13)`, host `aarch64-apple-darwin`.

## 1. The register

Paths were produced by `cargo tree -i <crate>` on this machine, not copied from
an advisory feed. "Fixed" is the version the advisory names; `none` means the
advisory publishes no fixed version, because the crate is unmaintained rather than
patched.

| Advisory | Crate | Version | Dependency path | In the macOS binary | Fixed | Why there is no in-range fix |
| --- | --- | --- | --- | --- | --- | --- |
| RUSTSEC-2026-0253 | `lru` | 0.16.4 | `cryoglyph 0.1.0` → `iced_wgpu 0.14.0` → `iced_renderer 0.14.0` → `iced 0.14.0` → `screenroom-desktop` | yes | 0.18.2 | `cryoglyph 0.1.0` requires `lru ^0.16`; `0.16.4` is the last `0.16` release; the fix landed in `0.18.2`, outside that range |
| RUSTSEC-2024-0436 | `paste` | 1.0.15 | `metal 0.32.0` (the app's own `cfg(target_os = "macos")` dependency, and also reached through `wgpu-hal 27.0.4` → `wgpu 27.0.1`), plus `pulp 0.22.3` → `exr` → `image` | yes | none | unmaintained; `1.0.15` is the newest `paste` that exists, and both `metal 0.32` and `metal 0.33` require `paste ^1` |
| RUSTSEC-2026-0192 | `ttf-parser` | 0.25.1 | `fontdb 0.23.0` → `cosmic-text 0.15.0` → `iced_graphics 0.14.0`, `cryoglyph 0.1.0`, `iced_tiny_skia 0.14.1` | yes | none | unmaintained; `0.25.1` is the newest release of the crate, and `fontdb 0.23.0` requires `ttf-parser ^0.25` |
| RUSTSEC-2026-0371 | `zbus_names` | 4.3.5 | `zbus 5.19.0` → `mundy 0.2.3` → `iced_winit 0.14.1`, where `mundy` is `cfg(target_os = "linux")` | no | none | unmaintained; the whole `zbus`/`zvariant` group is below a Linux-only gate |
| RUSTSEC-2026-0370 | `zvariant` | 5.15.1 | the same `mundy` path, and `zbus_macros 5.19.0`, `zbus_names 4.3.5` | no | none | unmaintained; Linux-only gate |
| RUSTSEC-2026-0372 | `zvariant_derive` | 5.15.1 | `zvariant 5.15.1` | no | none | unmaintained; Linux-only gate |
| RUSTSEC-2026-0441 | `pin-utils` | 0.1.1 | `wasmtimer 0.4.3` → `iced_futures 0.14.0` → `iced 0.14.0` | no | none | unmaintained; `wasmtimer` is the wasm32 timer, so this is absent from every native build |

The last row is not one of the six this register was opened for. It was found by
the live OSV query in §5 on 2026-11-21 and is recorded here rather than left out:
the register states what the lockfile actually contains. `cargo update --dry-run`
removes `pin-utils` entirely, so an ordinary lockfile refresh clears it.

## 2. Three that ship, four that do not

**Reachable in the macOS application** — `lru`, `paste`, `ttf-parser`. These are
inside `iced` and the app's own Metal binding, which is the presentation stack the
desktop client cannot work without.

**In `Cargo.lock` only** — `zbus_names`, `zvariant`, `zvariant_derive`,
`pin-utils`. The `zbus` group arrives through `mundy`, which `iced_winit` pulls
only for `cfg(target_os = "linux")`; `pin-utils` arrives through `wasmtimer`,
which exists for wasm32 targets. On this host:

```bash
cargo tree -i zbus_names     # warning: nothing to print
cargo tree -i zvariant       # warning: nothing to print
cargo tree -i pin-utils      # warning: nothing to print
cargo tree -i zbus_names --target all   # shows the Linux path
```

A bare invocation that prints `nothing to print` is the evidence that the crate is
outside the macOS graph, and `--target all` is how to see the path it belongs to.
Neither result is a build failure; the exit status is 0 in both cases.

Because Linux is out of scope ([`ADR-004`](ADR-004_PERSONAL_EDITION_SCOPE.md)), the
four lockfile-only rows cannot be retired by this project. They leave the lockfile
when `iced` stops depending on `mundy` for Linux or when `wasmtimer` stops
depending on `pin-utils`.

## 3. The iced stack, and why nothing moves in range

Every macOS-linked advisory above sits inside `iced` 0.14.0 or the Metal binding the
application owns. Checked against crates.io and `cargo tree` on 2026-11-21:

| Fact | Value | Where it was checked |
| --- | --- | --- |
| Latest `iced` release | 0.14.0, published 2025-12-07 | crates.io API, 2026-11-21 |
| `iced 0.14.0` → `iced_wgpu` | 0.14.0, which requires `wgpu ^27.0` | crates.io dependencies API |
| Latest `wgpu` release | 30.0.2, published 2026-11-12 | crates.io API, 2026-11-21 |
| `wgpu` may move alone | no — `AGENTS.md` requires wgpu to match iced | repository rule |
| Latest `cryoglyph` | 0.1.0, published 2025-12-05 | crates.io versions API |
| `cryoglyph 0.1.0` → `lru` | `^0.16` | crates.io dependencies API |
| Last `lru 0.16` release | 0.16.4, 2026-04-13 | crates.io versions API |
| `lru` fix | 0.18.2, 2026-08-03 | OSV `RUSTSEC-2026-0253` |
| Latest `paste` release | 1.0.15, 2024-05-07 | crates.io API, 2026-11-21 |
| `metal 0.32.0` and `0.33.0` → `paste` | `^1` in both | crates.io dependencies API |
| Latest `ttf-parser` release | 0.25.1, 2024-11-29 | crates.io API, 2026-11-21 |
| `fontdb 0.23.0` → `ttf-parser` | `^0.25` | crates.io dependencies API |

So the situation is not a missed update. `iced` 0.14.0 is the newest iced, and it
pins the renderer that pins `wgpu 27`, the `lru 0.16` range, and the `ttf-parser
0.25` range. The only release that would help, `lru 0.18.2`, is two minor versions
past what `cryoglyph` asks for, and `cryoglyph` has published exactly one version.
`paste` and `ttf-parser` have no fixed release at all — the advisory is a
maintenance notice, not a patch. `cargo update` cannot cross any of these
boundaries, which is why `cargo update --dry-run` on 2026-11-21 locks 41 packages
to newer compatible versions and leaves `lru`, `paste`, `ttf-parser` and the `zbus`
group exactly where they are. The only members of §1 that it touches are removals:
it drops `pin-utils` and `pulp` from the graph, and it cannot drop `paste` while
`metal 0.32` is a direct dependency.

`wgpu 30.0.2` does drop the `metal` crate in favour of `objc2-metal`, which removes
the `wgpu-hal` route to `paste`. The app's own `metal = "0.32"` dependency would
still carry it, and `iced 0.14` cannot use `wgpu 30` anyway. That is the shape of
the eventual exit, not an available one.

### What RUSTSEC-2026-0253 actually requires

That advisory is the only memory-safety item of the set: `LruCache::pop()` was not
panic-safe, leaving a dangling node in the internal list. Its own precondition is
narrow — it needs unwinding panics and `std::panic::catch_unwind` around a key type
whose `Drop` panics. The shipped bundle is built with `cargo build --release`
(`scripts/package-macos.sh`) and the workspace `[profile.release]` sets
`panic = "abort"`, so the release binary does not unwind. That lowers the exposure
of the shipped application; it does not retire the advisory, and it does not apply
to `cargo test`, which unwinds.

The other two macOS-linked notices are maintenance advisories. Neither asserts a
memory-safety defect, and neither publishes a fix.

## 4. Acceptance rule

No unpatched **memory-safety** advisory may stay outside the documented iced stack.
Everything inside that stack, and every maintenance-only advisory anywhere, is an
accepted risk that must name its reason here and be re-read at each review. A new
memory-safety advisory in a crate this project controls is a defect to fix, not a
row to add.

`RUSTSEC-2026-0253` is currently the one memory-safety advisory in the accepted
set. It is accepted by the owner's macOS-first scope decision
([`ADR-004`](ADR-004_PERSONAL_EDITION_SCOPE.md)) together with the `panic = "abort"`
release profile above, on the condition that §6 is checked at every `iced` release.

## 5. Recheck commands

Run from the repository root, on macOS, with the workspace resolved. All of these
are read-only.

**1. Which target actually links a crate.** This is the check that separates §2's
two groups:

```bash
cargo tree -i lru
cargo tree -i paste
cargo tree -i ttf-parser
cargo tree -i zbus_names --target all
cargo tree -i zvariant --target all
cargo tree -i zvariant_derive --target all
cargo tree -i pin-utils --target all
```

`nothing to print` means the crate is not in the default (macOS) graph.

**2. Whether anything can move without a source change:**

```bash
cargo update --dry-run
```

A crate that does not appear in the output has no compatible update. This is also
the cheapest confirmation that an advisory's fix version is out of range.

**3. The advisory set itself**, over the real lockfile, through the OSV batch API:

```bash
python3 - <<'PY' > "${TMPDIR:-/tmp}/screenroom-osv.json"
import json, re

packages = []
name = None
for line in open("Cargo.lock"):
    match = re.match(r'^name = "(.*)"$', line.strip())
    if match:
        name = match.group(1)
        continue
    match = re.match(r'^version = "(.*)"$', line.strip())
    if match and name:
        packages.append({"package": {"name": name, "ecosystem": "crates.io"}, "version": match.group(1)})
        name = None
print(json.dumps({"queries": packages}))
PY

curl -s -X POST -H 'Content-Type: application/json' \
  --data @"${TMPDIR:-/tmp}/screenroom-osv.json" https://api.osv.dev/v1/querybatch \
  | python3 -c 'import json,sys
q = json.load(open(sys.argv[1]))["queries"]
for p, r in zip(q, json.load(sys.stdin)["results"]):
    ids = [v["id"] for v in r.get("vulns", [])]
    if ids:
        print(p["package"]["name"], p["version"], ", ".join(ids))' "${TMPDIR:-/tmp}/screenroom-osv.json"
```

On 2026-11-21 this returned exactly the seven rows in §1 out of 536 locked
packages. The endpoint caps a batch at 1000 queries, which this lockfile is well
inside. The query needs the network; a run with no network is PENDING, not clean.

`cargo audit` performs the same check against the RustSec database and is the
preferred command **when it is installed**. It is not installed on this machine
(`error: no such command: 'audit'`), neither is `cargo-deny`
(`error: no such command: 'deny'`), so the OSV query above is the command this
project actually runs.

**4. A single advisory's own metadata**, when a row needs checking:

```bash
curl -s https://api.osv.dev/v1/vulns/RUSTSEC-2026-0253
```

The `affected[].ranges[].events` array carries the `introduced` and `fixed`
versions the table in §1 reports.

## 6. Cadence, and when to revisit

Run §5 on every dependency review pass and once before every release, and record
the date and the result below. A review that could not reach the network or could
not run the toolchain is recorded as PENDING with the reason.

| Date | Toolchain | Result |
| --- | --- | --- |
| 2026-11-21 | cargo 1.99.1, rustc 1.99.1, aarch64-apple-darwin | 7 advisories: 3 in the macOS binary (`lru`, `paste`, `ttf-parser`), 4 lockfile-only (`zbus_names`, `zvariant`, `zvariant_derive`, `pin-utils`). No in-range fix for any of them. `cargo update --dry-run` locked 41 packages to newer compatible versions, removed `pin-utils` and `pulp` from the graph, and moved no other member of the table. |

Revisit this register when any of the following happens:

- **`iced` publishes past 0.14.** Re-run §5 first, then check whether the new
  `iced_wgpu` still pins `wgpu 27`, whether `cryoglyph` still requires `lru ^0.16`,
  and whether `mundy` still gates the `zbus` group to Linux. This is the single
  release that can clear most of the table.
- **`cryoglyph` publishes a version requiring `lru >= 0.18.2`.** That alone clears
  RUSTSEC-2026-0253 without waiting for iced, if the pinned `iced_wgpu` accepts it.
- **`metal` publishes a release that stops requiring `paste`.** `0.33.0` still
  requires it, so this is not a minor bump to watch for but a rewrite.
- **The `wgpu` pin becomes negotiable** — that is, when `AGENTS.md`'s wgpu-must-match-iced
  rule and iced's own requirement stop disagreeing with `wgpu 30`, which drops the
  `metal` crate for `objc2-metal`.
- **`fontdb` or `cosmic-text` moves off `ttf-parser`** — a maintenance-only item,
  so this follows upstream without urgency.
- **A new advisory names a crate this project owns directly**, or upgrades one of
  these from maintenance to memory-safety. That is a defect under §4, not a row.

## 7. What this register does not claim

- It is not a claim that the three macOS-linked advisories are harmless. They are
  accepted risks with the reasons above, and the exit paths in §6.
- It does not cover the FFmpeg and libmpv binaries the bundle ships. Those are
  built from pinned sources recorded in [`LICENSING.md`](LICENSING.md) and
  [`RELEASE_PIPELINE.md`](RELEASE_PIPELINE.md) §1, and their advisories are a
  separate question from the Cargo graph.
- It does not cover `apps/tv`, whose dependencies live in `apps/tv/package.json`
  and `pnpm-lock.yaml` and belong to the deferred television target.
- The 2026-11-21 entry is a macOS row only. Nothing here was evaluated against a
  Windows build or a Tizen bundle, and neither platform's gate is moved by it.
