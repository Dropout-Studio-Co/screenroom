# Accepted limitations — macOS completion run (2026-11-21)

Every entry below is a check that could not be completed by an agent on this
machine, with the substitute that was attempted and what it observed. None of
these is a pass.

## 1. Developer ID signing and notarisation (item 12)

**Attempted substitute:** look for an existing signing identity and a configured
`notarytool` profile instead of purchasing anything.

```
$ security find-identity -v -p codesigning
     0 valid identities found

$ xcrun notarytool history --keychain-profile screenroom
Error: No Keychain password item found for profile: screenroom
```

**Observation:** no `Developer ID Application` identity exists in any keychain and
no notarisation credential profile is configured. `scripts/notarize.sh` refuses an
ad-hoc bundle by design, so neither the signed build nor the notarised, stapled
bundle can be produced.

**Why an agent cannot close it:** both artifacts need an Apple Developer Program
membership and the certificate that only the account holder can create. Purchasing
a membership is out of scope for this run and no paid service may be added
autonomously.

**What remains as evidence:** the ad-hoc signature the shipped bundle does carry
(`codesign -dv` shows the ad-hoc authority), `scripts/package-macos.sh` reading
`SCREENROOM_CODESIGN_IDENTITY` and `SCREENROOM_HARDENED_RUNTIME`, and
`scripts/notarize.sh` implementing the full submit, staple, validate and
`spctl --assess` path. The gate G10 update half is unaffected: Sparkle verifies its
own Ed25519 feed and archive signatures, and that path has real receipts.

## 2. A true AC power loss (item 8)

**Attempted substitute:** `kill -9` of the running application during playback,
then a relaunch on the same scratch database, plus a simulated full disk on a
scratch APFS image.

**Observation:** the last checkpoint survived (20,042 ms against a 19,042 ms last
sample), `PRAGMA integrity_check` stayed `ok`, the stale session was closed, the
running job was requeued, the interrupted conversion was failed with
`The app closed before this finished.`, and the library stayed browsable. On the
full image the application reported `StorageFull` and
`No space left on device`, wrote no partial output and kept the database intact.

**Why an agent cannot close it:** `kill -9` keeps the page cache, so filesystem
metadata durability under an actual power cut is not exercised. Cutting power to
the machine is a physical act.

**What remains:** the owner-run power-cut step in
`docs/MACOS_ACCEPTANCE_CHECKLIST.md` stays PENDING.

## 3. Human eyes and ears (item 6)

**Attempted substitute:** passive window capture of the application's own window,
image inspection, the application's own `SCREENROOM_PLAY_CHECK` instrumentation,
decoded-audio energy checks, and programmatic message entry points.

**Observation:** see the observation pass recorded in
`docs/MACOS_ACCEPTANCE_EVIDENCE.md`. Every mechanical question — is the picture
present, are there bars, is the subtitle drawn, is the focused row inverted, does
the grid reflow — is answered from a captured PNG or a measured counter.

**Why an agent cannot close it:** whether a film *sounds* right, whether a poster
*looks* right to its owner, and whether playback feels smooth are subjective
judgements about the person's own media. An agent can only report what an
instrument and an image show.

**What remains:** the owner's own acceptance session in
`docs/MACOS_ACCEPTANCE_CHECKLIST.md`.

## 4. The Ultra breakpoint, captured (item 6, P2-06)

**Attempted substitute:** launch the application at 1920×1080 and capture its own
window passively.

**Observation:** the internal display's logical desktop is 1470×956, and the
window is clamped to it. A capture at the requested 1920×1080 returned a
2940×1846 PNG, which is 1470×923 logical points — below the Ultra threshold of
1760. The 1280 and 1460 widths capture exactly as requested.

**Why an agent cannot close it:** showing a 1760-pixel-wide window needs a wider
logical desktop. Changing the display's resolution is a global macOS setting
change, which this project forbids without the owner.

**What remains:** the Ultra column geometry is covered by the five-breakpoint unit
tests (`UI_REBUILD.md`), and the visual check stays PENDING the owner.

## 5. A third-party casting receiver (item 7)

**Attempted substitute:** a first-party DLNA test receiver
(`scripts/dev-receiver/`) advertising SSDP, serving a MediaRenderer description
and AVTransport, fetching the application's tokenised stream with `Range`.

**Observation:** the whole session ran over real LAN sockets: discovery found the
receiver, play/pause/seek/resume/stop each read back, the receiver's fetched bytes
ffprobe as the source media, a mid-session receiver kill produced a clear
unreachable error, and the application's listener was gone afterwards.

**Why an agent cannot close it:** no third-party renderer is present on this
network and none may be assumed. A test double cannot prove a real device's
behaviour, its codec support or its timing.

**What remains:** F19's device gate stays closed and P1-08 stays open until a
renderer the owner owns answers a search.

## 6. A verified source directory (item 10, F23)

**Attempted substitute:** document the shipped bookmark-only behaviour and the
missing precondition.

**Observation:** `source_bookmarks` accepts only `https`/`http`/`ftp`, stores
provenance, and ships no list. No verified nationwide list, operator permission or
measured ISP reachability exists, and the repository's hard rules forbid scraping,
access-control bypass and distribution without a rights check.

**Why an agent cannot close it:** a source directory needs a rights holder's
written permission and a measured reachability result. Neither can be manufactured
from this machine.

**What remains:** ADR-011 stays BLOCKED; the code is unchanged.

## 7. The literal pointer press on "Update now" (item 9)

**Attempted substitute:** the installed application's own programmatic install
path (`SCREENROOM_UPDATE_INSTALL`), which sends the same action the button sends.

**Observation:** recorded in `docs/MACOS_ACCEPTANCE_EVIDENCE.md`.

**Why an agent cannot close it:** the click itself needs the pointer, and pointer
automation of the visible UI is permitted only with
`SCREENROOM_ALLOW_FOREGROUND=1`, which this run never set.

**What remains:** nothing functional; the action path is verified end to end.
