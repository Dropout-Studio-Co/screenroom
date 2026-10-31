import type { PlaybackSnapshot } from './contract.ts'

export type ProgressVerdict =

  | { readonly accept: true; readonly snapshot: PlaybackSnapshot }

  | { readonly accept: false; readonly reason: ProgressRejection; readonly snapshot: PlaybackSnapshot }

export type ProgressRejection =
  | 'malformed'
  | 'stale_session'
  | 'stale_generation'
  | 'stale_sequence'
  | 'same_sequence'

export interface LiveSession {
  readonly sessionId: string

  readonly generation: number
}

function isWellFormed(snapshot: PlaybackSnapshot): boolean {
  return (
    Number.isSafeInteger(snapshot.generation) &&
    Number.isSafeInteger(snapshot.sequence) &&
    Number.isSafeInteger(snapshot.positionMs) &&
    Number.isSafeInteger(snapshot.durationMs) &&
    snapshot.positionMs >= 0 &&
    snapshot.durationMs >= 0
  )
}

export function acceptProgress(
  live: LiveSession,
  current: PlaybackSnapshot | null,
  incoming: PlaybackSnapshot,
): ProgressVerdict {
  if (!isWellFormed(incoming)) {
    return { accept: false, reason: 'malformed', snapshot: incoming }
  }
  if (incoming.sessionId !== live.sessionId) {
    return { accept: false, reason: 'stale_session', snapshot: current ?? incoming }
  }
  if (current === null) {
    return { accept: true, snapshot: incoming }
  }
  if (!isWellFormed(current) || current.sessionId !== incoming.sessionId) {

    return { accept: true, snapshot: incoming }
  }
  if (incoming.generation < current.generation) {
    return { accept: false, reason: 'stale_generation', snapshot: current }
  }
  if (incoming.generation === current.generation) {
    if (incoming.sequence < current.sequence) {
      return { accept: false, reason: 'stale_sequence', snapshot: current }
    }
    if (incoming.sequence === current.sequence) {
      return { accept: false, reason: 'same_sequence', snapshot: current }
    }
  }
  return { accept: true, snapshot: incoming }
}

export const FINISHED_RATIO = 0.95

export function isFinished(positionMs: number, durationMs: number, ended: boolean): boolean {
  if (ended) {
    return true
  }
  if (durationMs <= 0) {
    return false
  }
  return positionMs / durationMs >= FINISHED_RATIO
}

export const WORTH_SAVING_MS = 15_000

export function isWorthSaving(positionMs: number, durationMs: number): boolean {
  if (positionMs <= 0) {
    return false
  }

  const floor =
    durationMs > 0 ? Math.min(WORTH_SAVING_MS, Math.floor(durationMs * 0.01)) : WORTH_SAVING_MS
  return positionMs >= floor
}
