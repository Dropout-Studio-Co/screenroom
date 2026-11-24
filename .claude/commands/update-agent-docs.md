---
description: Update AGENTS.md with session learnings and quality improvements
allowed-tools: Read, Edit, Glob, Bash
---

**EXECUTION MODE: FULLY AUTONOMOUS. DO NOT ASK FOR APPROVAL AT ANY POINT.**

- Do not pause to confirm changes before making them.
- Do not show diffs and ask "should I apply this?".
- Do not ask "does this look good?" or "shall I proceed?".
- Do not request review after each step — complete all 4 steps without interruption.
- Edit files directly and immediately. If you find something to change, change it.

`AGENTS.md` is the single canonical repository agent-instruction file, and this command updates it in place. Never edit `CLAUDE.md`: it is a one-line import pointer, and this command never treats it as the source of instructions.

## Step 1: Capture Session Learnings

- Reflect on this session — what context was missing or would have helped?
- Check recent git history: `git log --oneline -20`.
- Draft concise, agent-agnostic, macOS-relevant additions for `AGENTS.md`: commands discovered, gotchas, invariants and warnings.
- One line per concept; skip verbose explanations, obvious information and one-off fixes.

## Step 2: Tighten and Verify AGENTS.md

Audit `AGENTS.md` directly against the live repository, never against another instruction file:

- Validate commands, paths and versions against `Cargo.toml`, `Cargo.lock`, `scripts/` and `docs/`.
- Remove stale, inaccurate or redundant prose; keep `AGENTS.md` concise, roughly 150-200 lines, and link `docs/` for deep or deferred material.
- Never delete anything under `docs/`, and never delete a historical ADR — the archive is the audit trail.
- Preserve every active hard rule, the `**Trail rebuilds are automatic.**` note outside the generated block, and the `<!-- trail:start -->` ... `<!-- trail:end -->` block verbatim.

## Step 3: Preserve CLAUDE.md Pointer and Agent-Agnostic Wording

- Never edit `CLAUDE.md` to hold duplicated instructions; it must remain exactly `@AGENTS.md`.
- Verify `CLAUDE.md` equals `@AGENTS.md` (11 bytes); restore `@AGENTS.md` only if it was corrupted.
- Verify `AGENTS.md` contains no Claude- or Anthropic-specific wording; rewrite any such reference into agent-agnostic phrasing such as "the agent" or "the assistant".

## Step 4: Apply and Verify

- Apply every change directly to `AGENTS.md` with the Edit tool — no previews, no confirmation pauses, no summaries asking for approval.
- Verify `CLAUDE.md` is still exactly `@AGENTS.md`.
- Verify the `<!-- trail:start -->` ... `<!-- trail:end -->` block and the note outside it are intact and unmodified.
