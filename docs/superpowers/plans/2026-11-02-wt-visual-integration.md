# wt/visual → main: integration plan

**Verdict:** wt/visual does not delete anything. The three errors happen because
`rail.rs` and `home.rs` were resolved with the branch's side, which predates main's
motion and a11y work. Keep main's API and port the branch's two helpers onto it.

## Root cause
- Branch base is `f353bb9`. After that, main added `rail::{RAIL_WINDOW, window, limit,
  extent}`, changed `scroll_task(label, offset)`, made `home::hero_item` a free `pub fn`
  and made `in_progress` `pub`.
- On the branch, `rail.rs` still has the old `scroll_task(label, focus, card_width)` and
  no `extent`. `home.rs` has no free `hero_item` and a private `in_progress`.
- **Hidden drift that would compile but be wrong:** the branch's `shelf` drops the
  leading `Space` spacer and passes `focus.column - start` to `rail()`. Main's motion
  engine scrolls to the absolute `focus_offset(column)`, and that position only lines up
  when the spacer is there.

## Changes
**`ui/components/rail.rs`** (start from main's version)
1. Add the branch's `pub fn cards_per_view` and `pub fn window_start` unchanged, with
   their two tests.
2. Change `window(column, focused, count)` to `window(column, focused, count, per_view)`.
   Delete `RAIL_WINDOW`.
3. `limit` and `extent` keep their signatures and compute `per_view` internally.
4. Keep main's `scroll_task(label, offset)`.

**`ui/screens/home.rs`**
1. Keep `pub fn hero_item` and `pub fn in_progress`.
2. Add the branch's `rail_span(width)`, `pub(crate)`.
3. In `shelf`, compute `per_view` and call the new `rail::window`, keeping the leading
   spacer and the absolute `focus.column`.

**`app.rs` `rail_viewport`** — replace its body with `home::rail_span(width)` so the
motion extent and the rendered window agree.

**Take the branch side cleanly** for `shell.rs`, `theme.rs`, `library.rs`, `settings.rs`.

## Flaky test
`cast_stream::tests::dropping_the_server_closes_it_even_without_an_explicit_stop` does
not involve this branch. The risk is the final `connect_timeout(address).is_err()` probe
on a port that came from binding 0; replace it with `server.is_running() == false` plus
the existing peer-read proof.

## Acceptance
`cargo fmt --all --check`, `cargo clippy --workspace --all-targets -D warnings`,
`cargo test --workspace`, `python3 scripts/no_comments.py check` pass, and
`git grep RAIL_WINDOW` returns nothing.
