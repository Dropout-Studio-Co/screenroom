import type { PlaybackSnapshot } from '../core/contract.ts'
import { library } from './runtime.ts'
import { startPlayback, type PlaybackEvents, type PlaybackSession } from '../platform/playback.ts'

export const CHECKPOINT_MS = 5_000

const GENERATION = 1

let session: PlaybackSession | null = null
let sessionId = ''
let itemId = ''
let workId = ''
let sequence = 0
let lastCheckpointMs = 0
let positionMs = 0
let durationMs = 0

export interface PlaybackView {
  readonly positionMs: number
  readonly durationMs: number
  readonly paused: boolean
  readonly running: boolean
}

export interface BeginOptions {

  readonly subtitleFile?: string | null
}

export function begin(
  item: { readonly id: string; readonly workId: string },
  uri: string,
  startMs: number,
  events: PlaybackEvents,
  options: BeginOptions = {},
): boolean {
  const restarted = session !== null
  if (restarted) {
    end()
  }
  sessionId = `play-${item.id}-${String(Date.now())}`
  itemId = item.id
  workId = item.workId
  sequence = 0
  positionMs = startMs
  durationMs = 0
  lastCheckpointMs = startMs

  library().beginSession(sessionId, item.id)
  session = startPlayback(uri, startMs, {
    onReady: events.onReady,
    onPosition: (milliseconds: number) => {
      positionMs = milliseconds
      events.onPosition(milliseconds)
    },
    onDuration: (milliseconds: number) => {
      durationMs = milliseconds
      events.onDuration(milliseconds)
    },
    onEnded: events.onEnded,
    onError: events.onError,
    onBuffering: events.onBuffering,

    ...(events.onSubtitleFile === undefined ? {} : { onSubtitleFile: events.onSubtitleFile }),
    ...(events.onSubtitleText === undefined ? {} : { onSubtitleText: events.onSubtitleText }),
  }, options)
  return !restarted
}

export function running(): boolean {
  return session !== null
}

export function view(): PlaybackView {
  return {
    positionMs,
    durationMs,
    paused: session?.isPaused() ?? false,
    running: session !== null,
  }
}

export function pause(): void {
  if (session === null) {
    return
  }
  if (!session.isPaused()) {
    session.pause()
    checkpoint(false)
  }
}

export function resume(): void {
  session?.play()
}

export function seekTo(milliseconds: number): void {
  session?.seekTo(milliseconds)
}

export function jump(deltaMilliseconds: number): void {
  session?.jump(deltaMilliseconds)
}

export function tracks() {
  return session?.tracks() ?? []
}

export function selectTrack(type: string, index: number): boolean {
  return session?.selectTrack(type, index) ?? false
}

export function setSubtitlesSilent(silent: boolean): boolean {
  return session?.setSubtitlesSilent(silent) ?? false
}

export function subtitlesSilent(): boolean {
  return session?.subtitlesSilent() ?? false
}

export function decoderDurationMs(): number {
  return session?.durationMs() ?? 0
}

export function checkpointIfDue(): void {
  if (Math.abs(positionMs - lastCheckpointMs) < CHECKPOINT_MS) {
    return
  }
  checkpoint(false)
}

export function checkpoint(ended: boolean): void {
  if (sessionId === '' || itemId === '') {
    return
  }
  sequence += 1
  const snapshot: PlaybackSnapshot = {
    sessionId,
    generation: GENERATION,
    sequence,
    positionMs,
    durationMs,
    ended,
    paused: session?.isPaused() ?? false,
  }
  library().recordProgress({ sessionId, generation: GENERATION }, snapshot, workId)
  lastCheckpointMs = positionMs
}

export function end(): void {
  if (session === null) {
    return
  }
  checkpoint(false)
  library().endSession(sessionId)
  session.dispose()
  session = null
  sessionId = ''
  itemId = ''
  workId = ''
  sequence = 0
  lastCheckpointMs = 0
  positionMs = 0
  durationMs = 0
}
