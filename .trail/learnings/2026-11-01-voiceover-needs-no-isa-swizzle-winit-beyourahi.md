---
title: "VoiceOver needs no isa-swizzle: winit's view takes accessibilityChildren as it is"
author: Beyourahi
date: 2026-11-01
branch: main
cost: { minutes: 90, tokens: 120000 }
id: l-94d5497d
scope: repo
status: active
src: agent
---

What we ruled out, with the evidence, because it decides how any future accessibility work here starts.

iced 0.14 has no accessibility tree at all (grep over iced, iced_core, iced_winit finds only an unrelated Inaccessible(io::Error) image variant; Cargo.lock has no accesskit). So the only route is AppKit.

The obvious design (class_addMethod on WinitView, or an isa-swizzled dynamic subclass) is unnecessary. A plain NSView honours -setAccessibilityChildren:, and a subclass of NSAccessibilityElement registered with objc2::runtime::ClassBuilder is enough for the rest. Verified on the real WinitView, cross-process, with AXUIElement: the content view's custom children appear as the window's AX children with role (AXButton/AXHeading/AXCheckBox), label (kAXDescription), AXFocused and AXPress. Three things that cost time to find:

- NSAccessibilityElement implements the AX *setters* but not every getter: -accessibilityEnabled does not exist on it (sending it panics). Implement the getter yourself, or guard every read with -respondsToSelector:.
- AXFocused needs only an override of -isAccessibilityFocused on the element; it needs no -accessibilityAttributeNames override and no AXSelected. Setting +accessibilityFocused: does not surface.
- objc2's msg_send! cannot take a runtime Sel variable (it sends a literal selector named 'selector'). Guard with -respondsToSelector: and use a match over literal selectors.

Cross-process AX cannot see a hidden window: a window that was never ordered on screen reports zero AXWindows to AXUIElementCreateApplication, even from a trusted client. So a headless check has to read the same NSAccessibility methods AppKit's axserver calls, in-process (SCREENROOM_AX_CHECK), and only a person with VoiceOver can settle the rest. An alpha-0, ignores-mouse-events, never-activated window is visible to AX and can be used to prove the client-side path without taking the desktop.

Per-element frames are still not obtainable: iced exposes no per-widget layout to the application (Operation needs Ids the views do not set, and adding Ids means re-wrapping widgets, which is a layout change). The bridge therefore gives every element the content view's screen frame; the receipt records it (95,73,1280,800 on a 1280x800 window), so the limitation is visible rather than implied.
