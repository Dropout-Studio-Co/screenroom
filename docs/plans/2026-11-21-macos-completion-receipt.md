# macOS completion receipt — 2026-11-21

One session took the twelve outstanding macOS items to a defensible state.
Base `7ac4273`; final application commit `7b65a4f`, released as **v0.1.8**.

## Gate at the final tree

| Check | Result |
| --- | --- |
| `cargo test --workspace` | **874 passed, 0 failed** |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| `cargo fmt --all --check` | clean |
| `python3 scripts/no_comments.py check` | clean |
| `scripts/package-macos.sh` + `scripts/verify-bundle.sh` | bundle built, ad-hoc signed, all checks passed |
| `scripts/check-quit-routing.sh` | PASS — the menu bar's Quit performs the window's close action |
| `scripts/check-terminate-routing.sh` | PASS — AppKit terminate is routed through the close path |
| `scripts/check-no-network.sh` | PASS — 39 samples, 0 sockets held during playback |
| `scripts/check-updater.sh` | **All updater checks passed** (7 cases, including a signed upgrade, a refusal of a tampered archive and of an unsigned feed) |
| `scripts/check-abrupt-death-recovery.sh` | 21/21 assertions |
| `scripts/check-full-disk-recovery.sh` | 20/20 assertions |
| Installed application | 0.1.8 at `~/Applications/Screenroom.app`, launched outside Cargo |

The full-workspace count rose from 827 at the base commit to 874, of which 47 are
new or rewritten tests for this run.

## Item by item

| # | Item | Final state | Evidence |
| --- | --- | --- | --- |
| 1 | GPU/Metal sync fence (P1-02) | **CLOSED.** Deferred publication behind a GL fence; harness 30 min at 23.996 fps with 43,194 published = consumed = unique and 0 tears/stale/fence events; A/B delivery −0.02 %, CPU −2.86 pp | `plans/2026-11-21-ws1-gpu-sync-evidence.md` |
| 2 | Settings focus | **CLOSED.** Full-row `focus_surface` inversion, 18.34:1 contrast, scrolled into view | `plans/2026-11-21-ws2-ui-focus-and-fit-evidence.md` |
| 3 | Subtitle label truncation | **CLOSED.** Measured with iced's `Paragraph`; suffix and action reserved first | same |
| 4 | Whole-library enrichment (F05) | **CLOSED with two recorded limits.** Queue 241 → 0 without re-queueing 484 done tasks; 150 provider posters stored and decoded; 93 works have no provider entry | `plans/2026-11-21-ws3-metadata-drain-evidence.md` |
| 5 | Large-library paging (P2-05) | **DECIDED, no query change.** 1.0–1.2 ms grouped pages at 410 works; revisit at ~25,000 works | `plans/2026-11-21-ws4-pagination-decision.md` |
| 6 | Observation pass (P1-04/P2-06) | **agent pass COMPLETE**, four widths captured, Ultra not capturable; owner judgement PENDING | `MACOS_ACCEPTANCE_EVIDENCE.md` |
| 7 | Casting (F19) | **session VERIFIED over LAN** against the first-party test receiver; device gate still closed | `plans/2026-11-21-ws6-casting-evidence.md` |
| 8 | Durability (G9) | **two host receipts added** (`SIGKILL`, simulated full disk); true power cut PENDING | `plans/2026-11-21-ws7-durability-evidence.md` |
| 9 | In-app update path | **VERIFIED**, and a defect found and fixed on the way (below) | `MACOS_ACCEPTANCE_EVIDENCE.md` |
| 10 | Source directory (F23) | **accepted limitation**, no code | `plans/2026-11-21-accepted-limitations.md` |
| 11 | Dependency advisories (P2-09) | **DOCUMENTED** with a recheck procedure; a seventh wasm32-only notice found by the live OSV run | `DEPENDENCY_RISK.md` |
| 12 | Developer ID | **accepted limitation** — no identity, no purchase | `plans/2026-11-21-accepted-limitations.md` |

## The defect the update verification found

Running the installed application's own update path against the published feed
exposed a real bug that the repository's own updater gate reproduced on the **base
commit**: Sparkle downloaded a newer release and armed it for installation on quit,
but the application never reported it. The status stayed `idle`, so the banner would
never say an update was available, and the headless check timed out while the
update installed behind it.

- **Cause:** `apps/desktop/native/screenroom_updater.m` announces
  `install_on_quit` (Sparkle's `willInstallUpdateOnQuit`), and
  `Updater::feed` handled that event by setting `quit_install_ready` and nothing
  else — no status, no version.
- **Fix:** the event now carries its version, and `feed` moves the status to
  `Ready { deferred_to_quit: true }` and records the update, so the screen can say
  "Screenroom X installs when you quit, or now if you ask" and the receipt can
  report `ready_on_quit`.
- **Also changed:** the launch-check retry no longer re-issues `CheckNow` every
  second (up to ten times); it backs off to one attempt every four seconds, three
  attempts, so it cannot restart a check Sparkle has already begun.
- **Verified:** `scripts/check-updater.sh` is green on all seven cases; a live run
  of the shipped 0.1.8 build reports `checking` → `up_to_date` against the
  published feed; a live run of 0.1.7 installed 0.1.8 with the library intact and
  no relaunch.

## Release

| | |
| --- | --- |
| Version | **0.1.8** (`v0.1.7` was published earlier in the run, before the updater fix) |
| Assets | application archive, signed appcast, corresponding source |
| Installed app | 0.1.8, ad-hoc signed |
| Library after every step | `PRAGMA integrity_check` ok, 410 works, 867 files |

## Accepted limitations

Seven, each with its substitute and observation, in
`plans/2026-11-21-accepted-limitations.md`: a Developer ID certificate, a true
power loss, human eyes and ears, the Ultra width, a third-party cast receiver, a
verified source directory, and the literal pointer press on *Update now*.

## Measurements worth keeping

| What | Number |
| --- | --- |
| 4K, headless consumer harness, 30 min | 23.996 fps, 0 tears, 0 stale, 0 fence timeouts |
| 4K, hidden app, 40 s | 23.70 fps, 0 uploads, 0 reclaimed, 5.56 % CPU |
| 1080p, hidden app, 40 s | 29.70 fps, 0 uploads, 7.01 % CPU |
| A/B 5 min against the base commit | delivery −0.02 %, CPU −2.86 pp |
| Software fallback, 5 min | 15.65 fps, 180.83 % CPU, every frame uploaded |
| 2:1 film, measured from a capture | aspect 2.0000, full width, letterboxed |
| Provider posters stored | 150, all decoding, 12,873–139,861 bytes |
| Grouped library page, 410 works | 1.0–1.2 ms |
