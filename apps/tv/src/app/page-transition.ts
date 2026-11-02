import { MOTION } from '../core/motion.ts'
import { railIndexFor } from '../core/nav.ts'

export type PageDirection = -1 | 0 | 1

export interface TransitionStep {
  prop: string
  value: number
  duration?: number
  easing?: string
}

export interface PageTransition {
  before: TransitionStep[]
  in: TransitionStep[]
  out: TransitionStep[]
}

const DEPTH: Readonly<Record<string, number>> = {
  '/': 0,
  '/movies': 0,
  '/series': 0,
  '/search': 0,
  '/sources': 0,
  '/settings': 0,
  '/detail': 1,
  '/player': 2,
}

export function pagePath(path: string | undefined): string {
  const trimmed = (path ?? '').split('?')[0] ?? ''
  const clean = trimmed.replace(/\/+$/, '')
  return clean.length === 0 ? '/' : clean
}

export function pageDepth(path: string | undefined): number {
  return DEPTH[pagePath(path)] ?? 0
}

export function pageDirection(previousPath?: string, nextPath?: string): PageDirection {
  const previous = pagePath(previousPath)
  const next = pagePath(nextPath)
  if (previous === next) {
    return 0
  }
  const previousDepth = pageDepth(previous)
  const nextDepth = pageDepth(next)
  if (nextDepth > previousDepth) {
    return 1
  }
  if (nextDepth < previousDepth) {
    return -1
  }
  const previousRail = railIndexFor(previous)
  const nextRail = railIndexFor(next)
  if (previousRail >= 0 && nextRail >= 0 && previousRail !== nextRail) {
    return nextRail > previousRail ? 1 : -1
  }
  return 0
}

export function pageTransition(
  previous?: { readonly path?: string },
  next?: { readonly path?: string },
  reduced: boolean = MOTION.reducedMotion,
): PageTransition {
  const direction = pageDirection(previous?.path, next?.path)
  const slide = reduced ? 0 : MOTION.slidePx * direction
  const duration = reduced ? MOTION.crossFadeMs : MOTION.pageMs
  const fadeIn = reduced ? duration : Math.round(duration * 0.75)
  const fadeOut = reduced ? duration : Math.round(duration * 0.65)

  if (slide === 0) {
    return {
      before: [{ prop: 'alpha', value: 0 }],
      in: [{ prop: 'alpha', value: 1, duration, easing: 'ease-out' }],
      out: [{ prop: 'alpha', value: 0, duration, easing: 'ease-in' }],
    }
  }

  return {
    before: [
      { prop: 'x', value: slide },
      { prop: 'alpha', value: 0 },
    ],
    in: [
      { prop: 'x', value: 0, duration, easing: 'ease-out' },
      { prop: 'alpha', value: 1, duration: fadeIn, easing: 'ease-out' },
    ],
    out: [
      { prop: 'x', value: -slide * 0.5, duration, easing: 'ease-in' },
      { prop: 'alpha', value: 0, duration: fadeOut, easing: 'ease-in' },
    ],
  }
}
