import { test } from 'node:test'
import assert from 'node:assert/strict'

import {
  FOCUS_EASE,
  FOCUS_SPRING,
  MOTION,
  MOMENTUM_SPRING,
  focusMotion,
  momentumTravel,
  projectMomentum,
  rubberBand,
  springEasing,
  springSettled,
  springStep,
  type SpringState,
} from '../src/core/motion.ts'
import {
  bandMove,
  bandOffset,
  bandRows,
  bandScreenOffset,
  bandSettle,
  bandStart,
  bandWindow,
} from '../src/core/band.ts'
import { pageDirection, pageTransition } from '../src/app/page-transition.ts'

test('the house spring is critically damped with a 0.30 to 0.40 second response', () => {
  assert.equal(MOTION.spring.damping, 1)
  assert.ok(MOTION.spring.response >= 0.3 && MOTION.spring.response <= 0.4)
  assert.ok(MOTION.momentumSpring.damping >= 0.78 && MOTION.momentumSpring.damping <= 0.82)
  assert.ok(MOTION.deceleration >= 0.996 && MOTION.deceleration <= 0.999)
  assert.ok(MOTION.focusLift >= 1.03 && MOTION.focusLift <= 1.06)
  assert.equal(MOTION.reducedMotion, false, 'the TV ships with motion on until the flag is flipped')
})

test('a critically damped spring reaches its target without ringing', () => {
  let state: SpringState = { value: 0, velocity: 0 }
  let previous = 0
  let frames = 0
  while (frames < 240 && !springSettled(state, 1)) {
    state = springStep(state, 1, 1 / 60)
    assert.ok(state.value <= 1.0001, `overshoot at frame ${frames}: ${state.value}`)
    assert.ok(state.value >= previous - 1e-9, `the value went backwards at frame ${frames}`)
    previous = state.value
    frames += 1
  }
  assert.ok(springSettled(state, 1), 'the spring stopped')
  assert.ok(frames < 120, `it took ${frames} frames to settle`)
})

test('momentum is underdamped, so it overshoots and comes back', () => {
  let state: SpringState = { value: 0, velocity: 0 }
  let peak = 0
  for (let frame = 0; frame < 60; frame += 1) {
    state = springStep(state, 1, 1 / 60, MOMENTUM_SPRING)
    peak = Math.max(peak, state.value)
  }
  assert.ok(peak > 1.004, `momentum never overshot (peak ${peak})`)
  assert.ok(peak < 1.05, `momentum overshot too far (peak ${peak})`)
  assert.ok(springSettled(state, 1, 0.002), 'momentum settled within a second')
})

test('a bigger step takes proportionally longer, and a bad delta is clamped', () => {
  const oneStep = springStep({ value: 0, velocity: 0 }, 100, 1 / 60)
  const zeroStep = springStep({ value: 0, velocity: 0 }, 0, 0)
  assert.ok(oneStep.value > 0)
  assert.equal(zeroStep.value, 0)
  const nanStep = springStep({ value: 0, velocity: 0 }, 100, Number.NaN)
  assert.ok(Number.isFinite(nanStep.value))
})

test('the sampled easing runs from zero to one', () => {
  const ease = springEasing(FOCUS_SPRING)
  assert.equal(ease(0), 0)
  assert.equal(ease(1), 1)
  let previous = 0
  for (let step = 0; step <= 100; step += 1) {
    const value = ease(step / 100)
    assert.ok(value >= previous - 1e-9, `the curve went backwards at ${step}`)
    assert.ok(value <= 1.0001)
    previous = value
  }
  assert.equal(FOCUS_EASE(1), 1)
})

test('the momentum projection is the documented formula', () => {
  assert.ok(Math.abs(projectMomentum(1000, 0.998) - 499) < 1e-6)
  assert.ok(Math.abs(projectMomentum(0, 0.998)) < 1e-9)
  assert.ok(Math.abs(projectMomentum(-2000, 0.998) + 998) < 1e-6)
  const projected = projectMomentum(4000)
  const capped = momentumTravel(4000, 200)
  assert.ok(Math.abs(projected) > 200, 'the raw projection is larger than the cap')
  assert.equal(capped, 200)
  assert.equal(momentumTravel(-4000, 200), -200)
  assert.equal(momentumTravel(0, 200), 0)
})

test('a bad deceleration is kept in a usable range instead of exploding', () => {
  assert.ok(Number.isFinite(projectMomentum(1000, 1)))
  assert.ok(Number.isFinite(projectMomentum(1000, 0)))
  assert.ok(projectMomentum(1000, 0.99999) > 0)
})

test('rubber banding resists more the further it is pulled', () => {
  assert.equal(rubberBand(0, 900), 0)
  const small = rubberBand(40, 900)
  const large = rubberBand(400, 900)
  assert.ok(small > 0 && small < 40, `a small pull was ${small}`)
  assert.ok(large > small && large < 400, `a large pull was ${large}`)
  assert.ok(rubberBand(400, 900) < rubberBand(400, 1800), 'a longer band gives way more')
  assert.ok(rubberBand(100, 0) > 0, 'a degenerate dimension still resists')
})

test('reduced motion collapses every spring to a short cross-fade', () => {
  const full = focusMotion(false)
  assert.equal(full.lift, MOTION.focusLift)
  assert.ok(full.duration > 0)
  assert.equal(full.ease(1), 1)

  const reduced = focusMotion(true)
  assert.equal(reduced.lift, 1)
  assert.equal(reduced.duration, 0)
  assert.equal(reduced.ease(0.5), 0.5)

  const calm = pageTransition({ path: '/movies' }, { path: '/detail' }, true)
  assert.ok(calm.before.every((step) => step.prop !== 'x'))
  assert.ok(calm.in.every((step) => step.prop !== 'x'))
  assert.ok(calm.out.every((step) => step.prop !== 'x'))
  assert.equal(calm.in[0]?.duration, MOTION.crossFadeMs)
})

test('a deeper screen slides in from the right and Back mirrors it', () => {
  assert.equal(pageDirection('/', '/movies'), 1)
  assert.equal(pageDirection('/movies', '/settings'), 1)
  assert.equal(pageDirection('/settings', '/movies'), -1)
  assert.equal(pageDirection('/movies', '/detail'), 1)
  assert.equal(pageDirection('/detail', '/movies'), -1)
  assert.equal(pageDirection('/detail', '/player'), 1)
  assert.equal(pageDirection('/player', '/detail'), -1)
  assert.equal(pageDirection('/detail', '/detail'), 0)

  const forward = pageTransition({ path: '/movies' }, { path: '/detail' })
  const back = pageTransition({ path: '/detail' }, { path: '/movies' })
  const forwardX = forward.before.find((step) => step.prop === 'x')
  const backX = back.before.find((step) => step.prop === 'x')
  assert.equal(forwardX?.value, MOTION.slidePx)
  assert.equal(backX?.value, -MOTION.slidePx)
  const forwardOut = forward.out.find((step) => step.prop === 'x')
  const backOut = back.out.find((step) => step.prop === 'x')
  assert.equal(forwardOut?.value, -MOTION.slidePx * 0.5)
  assert.equal(backOut?.value, MOTION.slidePx * 0.5)
  for (const step of [...forward.in, ...forward.out]) {
    assert.ok((step.duration ?? 0) <= MOTION.pageMs, 'a page move stays short')
  }
})

test('a browser preview with no previous page still cross-fades', () => {
  const first = pageTransition(undefined, { path: '/' })
  assert.equal(first.before.length, 1)
  assert.equal(first.before[0]?.prop, 'alpha')
})

test('the band draws the visible rows plus a row of padding on each side', () => {
  assert.equal(bandRows(2), 4)
  assert.equal(bandRows(3), 5)
  assert.equal(bandRows(2, 0), 2)
  const window = bandWindow(1200, 5, 0, 2)
  assert.equal(window.first, 0)
  assert.equal(window.count, 20)
  const later = bandWindow(1200, 5, 3, 2)
  assert.equal(later.first, 15)
  assert.equal(later.count, 20)
  const end = bandWindow(12, 5, 2, 2)
  assert.equal(end.first, 10)
  assert.equal(end.count, 2)
})

test('a one row move animates and a jump recycles the window', () => {
  const start = bandStart()
  assert.equal(bandOffset(0, 0, 300), 0)

  const first = bandMove(start, 1, 300, 2)
  assert.equal(first.animate, true)
  assert.equal(first.state.padRow, 0)
  assert.equal(first.state.offset, -300)

  const settled = bandSettle(first.state, 300)
  assert.equal(settled.padRow, 0)
  assert.equal(settled.offset, -300)

  const second = bandMove(settled, 2, 300, 2)
  assert.equal(second.animate, true)
  assert.equal(second.state.offset, -600)

  const recycled = bandSettle(second.state, 300)
  assert.equal(recycled.padRow, 1)
  assert.equal(recycled.offset, -300)

  const jump = bandMove(start, 12, 300, 2)
  assert.equal(jump.animate, false)
  assert.equal(jump.state.padRow, 11)
  assert.equal(jump.state.offset, -300)
})

test('the same card is drawn in the same place before and after a recycle', () => {
  const pitch = 300
  const state = bandMove(bandStart(), 4, pitch, 2).state
  const settled = bandSettle(state, pitch)
  for (let row = 0; row < 30; row += 1) {
    assert.ok(
      Math.abs(bandScreenOffset(row, state, pitch) - (row - 4) * pitch) < 1e-9,
      `row ${row} moved when the window was recycled`,
    )
    assert.equal(
      bandScreenOffset(row, state, pitch),
      bandScreenOffset(row, settled, pitch),
      `row ${row} is not where it was`,
    )
  }
})

test('a press that cannot move the cursor leaves the band alone', () => {
  const state = bandStart()
  const same = bandMove(state, 0, 300, 2)
  assert.equal(same.animate, false)
  assert.deepEqual(same.state, state)
})
