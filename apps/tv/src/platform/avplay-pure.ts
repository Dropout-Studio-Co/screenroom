import type { TrackChoice } from '../core/tracks.ts'

export interface Rect {
  readonly x: number
  readonly y: number
  readonly width: number
  readonly height: number
}

export const AVPLAY_SPACE_WIDTH = 1920
export const AVPLAY_SPACE_HEIGHT = 1080

export interface Viewport {
  readonly width: number
  readonly height: number
}

export function toDisplayRect(rect: Rect, viewport: Viewport): Rect {
  const usableWidth =
    Number.isFinite(viewport.width) && viewport.width > 0 ? viewport.width : AVPLAY_SPACE_WIDTH
  const usableHeight =
    Number.isFinite(viewport.height) && viewport.height > 0 ? viewport.height : AVPLAY_SPACE_HEIGHT
  const scaleX = AVPLAY_SPACE_WIDTH / usableWidth
  const scaleY = AVPLAY_SPACE_HEIGHT / usableHeight
  const round = (value: number, scale: number): number => {
    const scaled = value * scale
    return Number.isFinite(scaled) ? Math.round(scaled) : 0
  }
  return {
    x: round(rect.x, scaleX),
    y: round(rect.y, scaleY),
    width: Math.max(0, round(rect.width, scaleX)),
    height: Math.max(0, round(rect.height, scaleY)),
  }
}

export type { TrackChoice } from '../core/tracks.ts'

export const SELECTABLE_TRACK_TYPES = ['AUDIO', 'TEXT'] as const

export function readTracks(raw: unknown): TrackChoice[] {
  if (!Array.isArray(raw)) {
    return []
  }
  const tracks: TrackChoice[] = []
  for (const entry of raw) {
    if (typeof entry !== 'object' || entry === null) {
      continue
    }
    const candidate = entry as { type?: unknown; index?: unknown; extra_info?: unknown }
    if (typeof candidate.type !== 'string' || !Number.isSafeInteger(candidate.index)) {
      continue
    }
    const index = candidate.index as number
    const language = readLanguage(candidate.extra_info)
    const type = candidate.type.toUpperCase()
    tracks.push({
      type,
      index,
      language,
      label: language === null ? `${type} ${index}` : `${type} ${index} · ${language}`,
    })
  }
  return tracks
}

function readLanguage(extraInfo: unknown): string | null {
  if (typeof extraInfo !== 'string' || extraInfo.length === 0) {
    return null
  }
  try {
    const parsed: unknown = JSON.parse(extraInfo)
    if (typeof parsed !== 'object' || parsed === null) {
      return null
    }
    const language = (parsed as { language?: unknown; Language?: unknown }).language
    if (typeof language === 'string' && language.length > 0) {
      return language
    }
    const capitalised = (parsed as { Language?: unknown }).Language
    return typeof capitalised === 'string' && capitalised.length > 0 ? capitalised : null
  } catch {
    return null
  }
}

