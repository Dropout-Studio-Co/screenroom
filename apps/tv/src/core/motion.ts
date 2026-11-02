export interface SpringConfig {
  readonly response: number

  readonly damping: number
}

export interface SpringState {
  readonly value: number
  readonly velocity: number
}

export const FOCUS_SPRING: SpringConfig = { response: 0.34, damping: 1 }

export const MOMENTUM_SPRING: SpringConfig = { response: 0.34, damping: 0.8 }

export const MOTION = {
  spring: FOCUS_SPRING,
  momentumSpring: MOMENTUM_SPRING,
  deceleration: 0.998,
  velocityCeiling: 12000,
  rubberConstant: 0.55,
  focusLift: 1.045,
  crossFadeMs: 130,
  pageMs: 210,
  slidePx: 72,
  nudgePx: 26,
  nudgeMs: 90,
  momentumLimit: 0.35,
  reducedMotion: false,
} as const

const FRAME_SECONDS = 1 / 60

const MAX_STEP_SECONDS = 1 / 30

const SUBSTEP_SECONDS = 1 / 240

const LINEAR: (progress: number) => number = (progress) => progress

function finite(value: number, fallback: number): number {
  return Number.isFinite(value) ? value : fallback
}

function stepSeconds(dt: number): number {
  if (!Number.isFinite(dt) || dt <= 0) {
    return FRAME_SECONDS
  }
  return Math.min(dt, MAX_STEP_SECONDS)
}

function clampUnit(value: number): number {
  return Math.min(1, Math.max(0, value))
}

export function springStep(
  state: SpringState,
  target: number,
  dt: number,
  config: SpringConfig = FOCUS_SPRING,
): SpringState {
  const seconds = stepSeconds(dt)
  const response = Math.max(0.01, finite(config.response, FOCUS_SPRING.response))
  const damping = Math.max(0, finite(config.damping, FOCUS_SPRING.damping))
  const omega = (2 * Math.PI) / response
  const substeps = Math.max(1, Math.ceil(seconds / SUBSTEP_SECONDS))
  const h = seconds / substeps
  let value = state.value
  let velocity = state.velocity
  for (let step = 0; step < substeps; step += 1) {
    const displacement = value - target
    const acceleration = -omega * omega * displacement - 2 * damping * omega * velocity
    velocity += acceleration * h
    value += velocity * h
  }
  return { value, velocity }
}

export function springSettled(
  state: SpringState,
  target: number,
  epsilon = 0.0005,
): boolean {
  return Math.abs(state.value - target) <= epsilon && Math.abs(state.velocity) <= epsilon * 60
}

export function springEasing(
  config: SpringConfig = FOCUS_SPRING,
  samples = 48,
): (progress: number) => number {
  const steps = Math.max(4, Math.trunc(finite(samples, 48)))
  const response = Math.max(0.01, finite(config.response, FOCUS_SPRING.response))
  const dt = response / steps
  const table: number[] = [0]
  let state: SpringState = { value: 0, velocity: 0 }
  for (let index = 0; index < steps; index += 1) {
    state = springStep(state, 1, dt, config)
    table.push(state.value)
  }
  const end = table[steps] ?? 1
  const scale = end > 0 ? 1 / end : 1

  return (progress: number): number => {
    const at = clampUnit(finite(progress, 0))
    if (at <= 0) {
      return 0
    }
    if (at >= 1) {
      return 1
    }
    const position = at * steps
    const index = Math.floor(position)
    const fraction = position - index
    const from = (table[index] ?? 0) * scale
    const to = (table[index + 1] ?? end) * scale
    return from + (to - from) * fraction
  }
}

export const FOCUS_EASE = springEasing(FOCUS_SPRING)

export const MOMENTUM_EASE = springEasing(MOMENTUM_SPRING)

export function projectMomentum(
  velocityPerSecond: number,
  deceleration: number = MOTION.deceleration,
): number {
  const velocity = finite(velocityPerSecond, 0)
  const rate = Math.min(0.99999, Math.max(0.9, finite(deceleration, MOTION.deceleration)))
  return (velocity / 1000) * (rate / (1 - rate))
}

export function boundVelocity(velocityPerSecond: number): number {
  const ceiling = MOTION.velocityCeiling
  return Math.min(ceiling, Math.max(-ceiling, finite(velocityPerSecond, 0)))
}

export function momentumTravel(
  velocityPerSecond: number,
  limit: number = Number.POSITIVE_INFINITY,
): number {
  const velocity = boundVelocity(velocityPerSecond)
  const projected = projectMomentum(velocity)
  const room = Math.abs(finite(limit, Number.POSITIVE_INFINITY))
  const bounded = Math.min(Math.abs(projected), room)
  return velocity < 0 ? -bounded : bounded
}

export function rubberBand(
  distance: number,
  dimension: number,
  constant: number = MOTION.rubberConstant,
): number {
  const travel = Math.max(0, finite(distance, 0))
  const size = Math.max(1, finite(dimension, 1))
  const stiffness = Math.max(0.01, finite(constant, MOTION.rubberConstant))
  return (1 - 1 / ((travel * stiffness) / size + 1)) * (size / stiffness)
}

export interface FocusMotion {
  readonly lift: number
  readonly duration: number
  readonly ease: (progress: number) => number
}

export function focusMotion(reduced: boolean = MOTION.reducedMotion): FocusMotion {
  if (reduced) {
    return { lift: 1, duration: 0, ease: LINEAR }
  }
  return {
    lift: MOTION.focusLift,
    duration: Math.round(MOTION.spring.response * 1000),
    ease: FOCUS_EASE,
  }
}
