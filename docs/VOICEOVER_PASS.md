# The VoiceOver pass — retired

**Retired 2026-11-03 by [`ADR-005`](ADR-005_SUBTITLE_PROVIDER_AND_FEATURE_REMOVALS.md).**
This file is kept as the record of what the pass was. It is no longer a procedure
and there is nothing to run.

## What was removed

VoiceOver-specific functionality is gone from the desktop application:

- the macOS `NSAccessibility` tree bridge (`apps/desktop/src/accessibility.rs`)
  and the snapshot publication behind it;
- its `NSAccessibilityAnnouncementRequestedNotification` announcements;
- its receipt plumbing — `SCREENROOM_AX_CHECK` and
  `SCREENROOM_AX_CHECK_SCREEN` — and the headless tree comparisons that reported
  `labels_match`, `values_match`, `enabled_match`, `press_actions_match`,
  `adjust_actions_match` and `focus_match`;
- the ten-minute listening pass this document described.

No further VoiceOver receipt is produced, and none is owed. The numbers that were
measured — 0 controls before the bridge attached; 19, 23, 30, 33 and 16 after, on
Home, Library, Detail, Settings and the Player — remain in
[`IMPLEMENTATION_STATUS.md`](IMPLEMENTATION_STATUS.md) and in Git history as dated
history. They describe a tree that no longer exists.

## What remains for accessibility

The removal is confined to the screen-reader bridge. These are unchanged and
still requirements (PRD Q07):

- **Keyboard navigation.** Every destination, the library grid, the episode list,
  the player transport and the Settings action strip are reachable with arrows,
  Enter and Escape, with no pointer. Settings carries an ordered action list that
  drives what is drawn, where the cursor is and what Enter runs.
- **Focus handling and restoration.** One deterministic focus order per screen,
  focus restored when returning from Detail or the player, and no control focused
  that the screen does not draw.
- **Reduced motion.** The setting is honoured by previews and by transitions that
  would otherwise move.
- **Subtitle size and position.** 50–300 % and 0–100 %, applied as libmpv's own
  `sub-scale` and `sub-pos` and read back rather than echoed, with the Bengali
  rendering receipt unchanged.
- **Contrast, target size and focus visibility.** Dropout Design System tokens,
  WCAG 2.2 AA contrast, 44×44 coarse targets and a focus state that is visible at
  couch distance.

What a person using VoiceOver gets today is what AppKit infers from a window that
publishes no accessibility tree: nothing the application controls. That is stated
plainly in the capability matrix rather than implied to work.
