# macOS human-acceptance checklist

Everything here ends in something only a **visible window, a person's eyes or
ears, or a drive in hand** can settle, so no step can be *closed* under the
background-first testing policy. Every other macOS behaviour is covered
headlessly; see `IMPLEMENTATION_STATUS.md` ("macOS audit and completion"). Until a
step below is ticked by the owner, its status is **PENDING**, not passed.

**2026-11-01:** the mechanism under each step is now executed by
`apps/desktop/src/app/acceptance.rs` against the real application, a real player
and real generated media — 16 tests, part of `cargo test --workspace`. The
`Automated` column says what that run settled and what it did not; the evidence,
the two defects it found and the reduced manual session are in
[`MACOS_ACCEPTANCE_EVIDENCE.md`](MACOS_ACCEPTANCE_EVIDENCE.md). An automated
`PASS` never covers the human half named beside it.

Use the installed bundle, with Wi-Fi off for the whole session:

```bash
scripts/package-macos.sh && scripts/verify-bundle.sh   # must end "All checks passed."
open dist/Screenroom.app
```

Expect two first-launch prompts: Gatekeeper ("unidentified developer": right-click
› Open once), and removable-volume access when the SSD folder is first read. That
access prompt returns after each rebuild, because an ad-hoc signature changes with
every build.

| # | Step | Pass when | Automated (2026-11-01) |
| --- | --- | --- | --- |
| 1 | First run: Settings › Add folder › choose the SSD's library folder | The scan row counts up, cards appear before it finishes, and it ends with "Scan finished". | **PASS** — 1,200 files: row counts up, cards on screen mid-walk, ends "Scan finished". OS permission prompt is the owner's. |
| 2 | Library grid after about two minutes | Cards show posters, cover art where a file has one, and runtimes. A series is **one** card. | **PARTIAL** — one card per series, years, runtimes and stored posters all asserted. Whether a poster *looks* right is yours. |
| 3 | Open a series, use ↑/↓ to pick episode 3, press Enter | Episode 3 plays, not the episode the screen opened on. | **PASS** — and it failed when first run: ↓ could not reach episode 3. Fixed. |
| 4 | Watch about a minute, press Space, then ←/→ | Pauses and resumes; arrows move ±10 s. **Listen**: speech is in sync with lips. | **PASS** for pause/resume/±10 s (measured). **Listen** is yours. |
| 5 | Audio and subtitle pickers on a file with several tracks | The spoken language and the subtitle text both change. | **PARTIAL** — both selections reach the player and are read back. The audible/visible change is yours. |
| 6 | Press f, then the green button (or ⌃⌘F) to leave fullscreen | The picture fills the screen. The header returns after leaving, and one Escape then exits the player. | **PARTIAL** — mode, platform exit, Escape order and header return asserted. The fill is yours. |
| 7 | Play a 2:1 film (`Green Book - 2018.mkv`, `Smile - 2022.mp4`, `Smile 2 - 2024.mkv` are all 1920×960) | The picture is wide with no extra bars inside it, and nothing is stretched. | **PARTIAL** — verified on your own media: `Green Book` is HEVC 10-bit 1920×960 with a 2:1 display aspect, and the app renders it at 1920×960 for 22.96 fps at 11.39 % CPU. Its poster extracts at 1440×720, the correct 2:1 shape. Looking at it is still yours. |
| 8 | Play the longest film you own for five minutes on battery | Smooth motion; note the battery percentage before and after. | **HUMAN REQUIRED**, and the premise changed: the library holds **no 4K file**, so this is now a 1080p battery observation. The mechanism it used to test is closed — 4K through the GPU path measured 24.39 fps at 13.95 % CPU, and real 1080p HEVC 10-bit measures 22.58 fps at 13.11 % on the same path against 41.91 % on the old one. |
| 9 | Let an episode end | "Up next … in 5 s" appears. Cancel stays on the finished episode, and Play now (or waiting) starts the next. | **PASS** — offer, cancel, replay-then-offer and Play now all asserted. |
| 10 | Mid-film, press ⌘Q; reopen the app | Continue Watching shows the film within a second of where it was. | **PASS** — quit path stores, reopen resumes within 1 s. ⌘Q routing checked headlessly by `scripts/check-quit-routing.sh`. |
| 11 | Mid-film, click Home in the header | Sound stops at once, and the film is in Continue Watching. | **PASS** — session closed, player idle, audio clock stopped, place kept, in Continue Watching. |
| 12 | Unplug the SSD mid-film, then plug it back in and Rescan | A clear failure, not a hang; cards show "unavailable" and keep their history; after Rescan they play again. | **PARTIAL** — folder-away keeps cards and history and a rescan recovers playback. **Pulling the cable is yours.** |
| 13 | Settings › Back up, change something, Restore that backup (press twice) | The change is undone, and the status names the `…before-restore` safety copy. | **PASS** — change undone, safety copy named and present. |
| 14 | Add a second profile and switch to it; quit and reopen | It reopens on that profile. Remove asks twice. | **PASS** — reopens on the profile; removal arms then removes. |
| 15 | Keyboard only: Home → Library → open → play → Escape → Escape | Every step is reachable, and the focused card is visible and readable. | **PARTIAL** — the whole journey runs on key events alone, and since 2026-11-01 ⌘3 → Settings → Up/Down → Enter also runs Settings actions. Readability is yours. |

Record the result of each step (pass, fail plus what was seen) under the
"Owner session" heading in `IMPLEMENTATION_STATUS.md`. A failed step is a defect to
fix, never a row to delete.

## Two steps worth doing in the same sitting

**HDR.** The extended-range path is built and measured (16-bit PQ surface, float
swapchain, EDR layer, target peak read from the panel at 406 nits), and it is
**opt-in** so nothing changes unless you ask. To see it: quit the app, then

```bash
SCREENROOM_HDR=extended cargo run --release -p screenroom-desktop
```

and play an HDR10 film. If it looks right, say so and it becomes the default for HDR
files instead of a switch. Do it without the variable first with the same film: the
two should look different in the highlights, and that difference is the whole point.

**VoiceOver.** ⌘F5, then VO+Right through the header, the heading and the cards, and
VO+Space on a card. The tree is built and verified headlessly (0 controls before,
13 after, focus and press working); what only you can answer is whether it *reads*
sensibly and in a useful order.

## One step is deferred on purpose

`Q01` / P1-12 asks for a run with the network off. **You asked that Wi-Fi stay up
while other work is running, so this was deliberately not performed.** What was
done instead is in `scripts/check-no-network.sh`: the app plays a file for eight
seconds while its sockets are sampled five times a second. It held **zero** sockets
in 37 samples. That is not the same as being offline, so the step stays open.

Do it whenever it suits you: turn Wi-Fi off, open the app, browse, play something,
quit, reopen, resume. Nothing should hang or show an error.

## The remaining manual session, in the order that lets you stop early

Steps 9–11, 13 and 14 need nothing further. What is left is a single pass:

1. Package, verify and open the app; answer Gatekeeper's one-time prompt.
2. Add the SSD folder — watch the grid fill while the row counts up (1) and
   whether posters match their titles (2).
3. Open a series, ↓ ↓ Enter, and listen for a few seconds (4).
4. In that film: change audio and subtitle language (5); press `f`, check the
   picture fills the screen, leave it, confirm the header returns and one Escape
   exits (6).
5. Play the 2:1 anamorphic film and confirm it looks wide and unstretched (7).
6. Play the longest film you own for five minutes on battery and note the
   percentage before and after (8).
7. During that film, unplug the drive: a clear failure, "unavailable" cards, and
   playback again after Rescan (12).
8. The keyboard-only journey, judging whether the focused card is readable (15).
