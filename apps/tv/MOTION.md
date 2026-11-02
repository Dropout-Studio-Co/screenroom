# Motion on the television

The TV client keeps the desktop's motion language: one critically damped spring,
one rubber band, one momentum projection, and a hard budget of `transform` and
`opacity` only. Everything lives in [`src/core/motion.ts`](src/core/motion.ts);
the band's row arithmetic lives in [`src/core/band.ts`](src/core/band.ts); the
Blits page transition lives in [`src/app/page-transition.ts`](src/app/page-transition.ts).

## The house constants

| Constant | Value | Job |
| --- | --- | --- |
| `MOTION.spring.response` | 0.34 s | the focus and row spring |
| `MOTION.spring.damping` | 1.0 | critically damped: a lift and a settle, no wobble |
| `MOTION.momentumSpring.damping` | 0.8 | only momentum, so it may overshoot |
| `MOTION.deceleration` | 0.998 | the momentum projection's rate |
| `MOTION.rubberConstant` | 0.55 | how hard the row ends pull back |
| `MOTION.focusLift` | 1.045 | a focused card's scale |
| `MOTION.slidePx` | 72 | the page transition's travel |
| `MOTION.pageMs` | 210 | the page transition's length |
| `MOTION.crossFadeMs` | 130 | reduced motion's cross-fade |
| `MOTION.momentumLimit` | 0.35 | the most a page jump may carry past its row |

The primitive functions are `springStep`, `springEasing`, `springSettled`,
`projectMomentum`, `momentumTravel` and `rubberBand`. `springStep` integrates
`x'' = -ω²(x - target) - 2ζωx'` with ω = 2π / response, in substeps of at most
1/240 s so a 60 Hz frame is the curve that was designed, not a degraded one.
`springEasing` samples that integrator once and hands the renderer an easing
function, so the animation runs on the renderer's own ticker rather than a
JavaScript frame loop.

## Where it is used

**Focus.** Cards on Home, Movies, TV Shows and the search results carry a
`:scale.transition` binding; the rail rows and the focused row on Detail,
Settings and Media Sources carry a small `:x` or `:scale` transition. The
transition's `easing` is the sampled spring, and its `value` is the focus target
from `focusMotion()`. Blits cancels the running transition and captures the
node's current value when a new one starts, so a press never waits for an
animation and a second press mid-flight re-targets from where the card actually
is. Nothing in a press handler awaits.

**Screens.** Every route carries `transition: pageTransition`. The function
compares the previous and next route's depth (`/` and the rail are depth 0,
`/detail` is 1, `/player` is 2) and, within a depth, the rail order. Forward
moves slide 72 px from the right; Back mirrors it with `-72` in and `+36` out.
The incoming and outgoing pages cross-fade at the same time. Only `x` and
`alpha` are animated.

**Rows.** The grid's band is drawn with one row of padding above and below
(`bandRows`, `bandWindow`) and the container's `y` is a spring transition to
`bandOffset`. A one-row move animates; a jump recycles the window instantly and
then carries momentum: `projectMomentum(velocity)` gives the travel, capped at
0.35 of a row pitch, and the band springs back. A press that cannot move the
cursor is not a dead key: it pulls the band with `rubberBand(26 px, bandHeight)`
and releases it with the spring. A horizontal dead end does the same on `x`, so
a row has a visible end. The five-column row already fits the 10-foot band, so
there is no horizontal fling to make; the horizontal axis is the row's end.

## Reduced motion

There is no preferences API on this widget, so the switch is the single constant
`MOTION.reducedMotion` in `src/core/motion.ts`. Set it to `true` and rebuild:

- `focusMotion()` returns a lift of 1 and a duration of 0: the focus styling
  still changes, but nothing slides or grows;
- `pageTransition()` drops the slide and becomes a 130 ms alpha cross-fade;
- the row band's duration becomes 0, so a row change is immediate.

No settings screen is invented for this, and no device preference is read.

## Cost on the panel

Every animated property is `x`, `y`, `scale` or `alpha`, which the Lightning 3
renderer sends as a transform or an opacity. No width, height, padding or text
metric is animated. The card window is at most four rows (20 cards), so the
band never animates 1,200 nodes. The spring itself is sampled once at module
load, not per frame.

## What has not been checked

None of this has run on the television. These are off-device results: the unit
tests in [`test/motion.test.ts`](test/motion.test.ts), `pnpm typecheck`,
`pnpm test` and `pnpm build`. The panel's frame rate under a held D-pad press,
the AVPlay surface's behaviour under a page transition, and whether the sampled
spring reads as intended at couch distance all need the set.

Two lines for the owner, with the app open on the set:

1. Hold **Down** through a screen of cards: the row band should move in one
   motion, stop at the last row with a short pull and a settle, and never leave a
   card half-drawn or a row blank.
2. Press **Left** at the first card of a row, then **Back** from a detail screen:
   each should animate, and Back should mirror the way in rather than repeat it.
