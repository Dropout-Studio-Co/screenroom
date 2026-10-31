import type { TrackChoice } from '../core/tracks.ts'
import { readTracks, toDisplayRect } from './avplay-pure.ts'
import { webapisGlobal, type AvPlay } from './tizen.ts'

export type PlaybackBackend = 'avplay' | 'video'

export interface PlaybackEvents {

  onReady(): void

  onPosition(milliseconds: number): void

  onDuration(milliseconds: number): void

  onEnded(): void

  onError(message: string): void

  onBuffering(active: boolean): void

  onSubtitleFile?(result: { readonly offered: string | null; readonly accepted: boolean }): void

  onSubtitleText?(text: string): void
}

export interface PlaybackSession {
  readonly backend: PlaybackBackend

  play(): void

  pause(): void

  isPaused(): boolean

  seekTo(milliseconds: number): void

  jump(deltaMilliseconds: number): void

  tracks(): readonly TrackChoice[]

  selectTrack(type: string, index: number): boolean

  setSubtitlesSilent(silent: boolean): boolean

  subtitlesSilent(): boolean

  positionMs(): number

  durationMs(): number

  state(): string

  dispose(): void
}

function createSurface(kind: 'object' | 'video'): HTMLElement {
  const surface = document.createElement(kind === 'object' ? 'object' : 'video')
  if (kind === 'object') {
    surface.setAttribute('type', 'application/avplayer')
  }
  surface.style.position = 'absolute'
  surface.style.top = '0'
  surface.style.left = '0'
  surface.style.width = '100%'
  surface.style.height = '100%'
  surface.style.zIndex = '0'
  surface.style.background = 'transparent'
  document.body.insertBefore(surface, document.body.firstChild)

  const canvas = document.querySelector('canvas')
  if (canvas instanceof HTMLElement) {
    canvas.style.position = 'absolute'
    canvas.style.zIndex = '1'
  }
  return surface
}

function currentViewport(): { width: number; height: number } {
  const root = document.documentElement
  return { width: root.clientWidth, height: root.clientHeight }
}

function fullScreenRect(): { x: number; y: number; width: number; height: number } {
  return toDisplayRect(
    { x: 0, y: 0, width: currentViewport().width, height: currentViewport().height },
    currentViewport(),
  )
}

function messageOf(error: unknown): string {
  if (typeof error === 'string') {
    return error
  }
  if (error instanceof Error) {
    return error.message
  }
  if (typeof error === 'object' && error !== null && 'message' in error) {
    const message = (error as { message?: unknown }).message
    if (typeof message === 'string') {
      return message
    }
  }
  return 'The player reported an error without a message.'
}

function createAvPlaySession(
  player: AvPlay,
  uri: string,
  startMs: number,
  events: PlaybackEvents,
  options: StartOptions,
): PlaybackSession {
  const surface = createSurface('object')
  let position = 0
  let duration = 0
  let paused = false
  let disposed = false
  let silent = false
  let sidecarAccepted = false
  let tracks: readonly TrackChoice[] = []

  const state = (): string => {
    try {
      return player.getState()
    } catch {
      return 'UNKNOWN'
    }
  }

  const refreshTracks = (): void => {
    try {
      tracks = readTracks(player.getTotalTrackInfo())
    } catch {

      tracks = []
    }
  }

  player.setListener({
    onbufferingstart: () => events.onBuffering(true),
    onbufferingcomplete: () => {

      try {
        duration = player.getDuration()
      } catch {
        duration = 0
      }
      if (duration > 0) {
        events.onDuration(duration)
      }
      events.onBuffering(false)
    },
    oncurrentplaytime: (milliseconds: number) => {
      if (!Number.isSafeInteger(milliseconds) || milliseconds < 0) {
        return
      }
      position = milliseconds
      events.onPosition(milliseconds)
    },
    onstreamcompleted: () => events.onEnded(),
    onerror: (errorType: string) => events.onError(`AVPlay: ${errorType}`),
    onsubtitlechange: (_duration: number, text: string) => {

      if (sidecarAccepted) {
        events.onSubtitleText?.(typeof text === 'string' ? text : '')
      }
    },
  })

  player.open(uri)
  const rect = fullScreenRect()
  player.setDisplayRect(rect.x, rect.y, rect.width, rect.height)

  player.setDisplayMethod('PLAYER_DISPLAY_MODE_LETTER_BOX')

  if (options.subtitleFile !== null && options.subtitleFile !== undefined) {
    try {
      player.setExternalSubtitlePath(options.subtitleFile)
      sidecarAccepted = true
    } catch {
      sidecarAccepted = false
    }
  }
  events.onSubtitleFile?.({
    offered: options.subtitleFile ?? null,
    accepted: sidecarAccepted,
  })

  if (startMs > 0) {

    try {
      player.seekTo(startMs)
      position = startMs
    } catch {
      position = 0
    }
  }

  player.prepareAsync(
    () => {
      refreshTracks()
      try {
        duration = player.getDuration()
      } catch {
        duration = 0
      }
      events.onReady()
      if (duration > 0) {
        events.onDuration(duration)
      }
      player.play()
    },
    (error: unknown) => events.onError(messageOf(error)),
  )

  return {
    backend: 'avplay',
    play(): void {
      if (disposed) {
        return
      }
      const current = state()
      if (current === 'PLAYING' || current === 'PAUSED' || current === 'READY') {
        player.play()
        paused = false
      }
    },
    pause(): void {
      if (disposed) {
        return
      }
      if (state() === 'PLAYING') {
        player.pause()
        paused = true
      }
    },
    isPaused: () => paused,
    seekTo(milliseconds: number): void {
      if (disposed) {
        return
      }
      const current = state()
      if (current === 'PLAYING' || current === 'PAUSED' || current === 'READY') {
        const bounded = Math.max(0, Math.trunc(milliseconds))
        player.seekTo(bounded)
        position = bounded
        events.onPosition(bounded)
      }
    },
    jump(deltaMilliseconds: number): void {
      if (disposed) {
        return
      }
      const current = state()
      if (current !== 'PLAYING' && current !== 'PAUSED') {
        return
      }

      if (deltaMilliseconds >= 0) {
        player.jumpForward(Math.trunc(deltaMilliseconds))
      } else {
        player.jumpBackward(Math.trunc(-deltaMilliseconds))
      }
    },
    tracks: () => {
      refreshTracks()
      return tracks
    },
    selectTrack(type: string, index: number): boolean {
      if (disposed || (type !== 'AUDIO' && type !== 'TEXT')) {
        return false
      }
      if (!Number.isSafeInteger(index) || index < 0) {
        return false
      }
      try {
        player.setSelectTrack(type, index)
        if (type === 'TEXT') {

          try {
            player.setSilentSubtitle(false)
            silent = false
          } catch {

          }
        }
        return true
      } catch {
        return false
      }
    },
    setSubtitlesSilent(next: boolean): boolean {
      if (disposed) {
        return false
      }
      try {
        player.setSilentSubtitle(next)
        silent = next
        if (next) {

          events.onSubtitleText?.('')
        }
        return true
      } catch {
        return false
      }
    },
    subtitlesSilent: () => silent,
    positionMs: () => position,
    durationMs: () => duration,
    state,
    dispose(): void {
      if (disposed) {
        return
      }
      disposed = true

      try {
        if (state() === 'PLAYING' || state() === 'PAUSED') {
          player.stop()
        }
      } catch {

      }
      try {
        player.close()
      } catch {

      }
      surface.remove()
    },
  }
}

function createVideoSession(uri: string, startMs: number, events: PlaybackEvents): PlaybackSession {
  const element = createSurface('video') as HTMLVideoElement
  let paused = false
  let disposed = false
  let position = startMs
  let duration = 0

  const report = (): void => {
    const reported = Math.round(element.currentTime * 1000)
    if (Number.isSafeInteger(reported) && reported >= 0) {
      position = reported
      events.onPosition(reported)
    }
  }

  element.addEventListener('loadedmetadata', () => {
    duration = Number.isFinite(element.duration) ? Math.round(element.duration * 1000) : 0
    events.onDuration(duration)
    events.onReady()
  })
  element.addEventListener('timeupdate', report)
  element.addEventListener('ended', () => events.onEnded())
  element.addEventListener('waiting', () => events.onBuffering(true))
  element.addEventListener('playing', () => events.onBuffering(false))
  element.addEventListener('error', () =>
    events.onError(`The browser could not play this file (${String(element.error?.code ?? '?')}).`),
  )

  element.src = uri
  if (startMs > 0) {

    element.addEventListener(
      'loadedmetadata',
      () => {
        element.currentTime = startMs / 1000
      },
      { once: true },
    )
  }
  void element.play().catch((error: unknown) => events.onError(messageOf(error)))

  return {
    backend: 'video',
    play(): void {
      if (disposed) {
        return
      }
      void element.play()
      paused = false
    },
    pause(): void {
      if (disposed) {
        return
      }
      element.pause()
      paused = true
    },
    isPaused: () => paused,
    seekTo(milliseconds: number): void {
      if (disposed) {
        return
      }
      const bounded = Math.max(0, Math.trunc(milliseconds))
      element.currentTime = bounded / 1000
      position = bounded
      events.onPosition(bounded)
    },
    jump(deltaMilliseconds: number): void {
      if (disposed) {
        return
      }
      const target = Math.min(
        duration > 0 ? duration : Number.MAX_SAFE_INTEGER,
        Math.max(0, position + Math.trunc(deltaMilliseconds)),
      )
      element.currentTime = target / 1000
    },

    tracks: () => [],
    selectTrack: () => false,

    setSubtitlesSilent: () => false,
    subtitlesSilent: () => false,
    positionMs: () => position,
    durationMs: () => duration,
    state: () => (element.readyState >= 3 ? (paused ? 'PAUSED' : 'PLAYING') : 'IDLE'),
    dispose(): void {
      if (disposed) {
        return
      }
      disposed = true
      element.pause()
      element.removeAttribute('src')
      element.load()
      element.remove()
    },
  }
}

export interface StartOptions {

  readonly subtitleFile?: string | null
}

export function startPlayback(
  uri: string,
  startMs: number,
  events: PlaybackEvents,
  options: StartOptions = {},
): PlaybackSession {

  const player = webapisGlobal()?.avplay
  if (player !== undefined) {
    return createAvPlaySession(player, uri, startMs, events, options)
  }
  events.onSubtitleFile?.({
    offered: options.subtitleFile ?? null,
    accepted: false,
  })
  return createVideoSession(uri, startMs, events)
}
