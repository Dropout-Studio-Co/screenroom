import { FOCUS_SCALE, hex, TOKENS, type Tokens } from './tokens.ts'
import { formatPosition, progressFraction } from '../core/browse.ts'
import type { LibraryItem } from '../core/contract.ts'

export const APP_WIDTH = 1920
export const APP_HEIGHT = 1080

export interface GridGeometry {
  readonly columns: number
  readonly rows: number
  readonly originX: number

  readonly originY: number
  readonly cardWidth: number
  readonly cardHeight: number
  readonly gapX: number
  readonly gapY: number

  readonly artWidth: number
  readonly artHeight: number

  readonly titleY: number

  readonly subtitleY: number
}

export interface GridBounds {

  readonly top: number

  readonly bottom: number

  readonly left?: number
}

export function gridFor(
  columns: number,
  rows: number,
  tokens: Tokens = TOKENS,
  width: number = APP_WIDTH,
  bounds: GridBounds = {
    top: Math.round(TOKENS.spacing.shellPad),
    bottom: APP_HEIGHT - Math.round(TOKENS.spacing.shellPad),
  },
): GridGeometry {

  const safeColumns = countOrOne(columns)
  const safeRows = countOrOne(rows)
  const safeWidth = Number.isFinite(width) && width > 0 ? width : APP_WIDTH
  const originX = Math.round(bounds.left ?? tokens.spacing.shellPad)
  const originY = Number.isFinite(bounds.top) ? Math.round(bounds.top) : 0
  const bottom = Number.isFinite(bounds.bottom) ? Math.round(bounds.bottom) : APP_HEIGHT

  const titleGap = Math.round(tokens.type.caption * 1.2)
  const titleLine = Math.round(tokens.type.bodyLg * 1.5)
  const subtitleLine = Math.round(tokens.type.caption * 1.6)
  const titleStrip = titleGap + titleLine + subtitleLine

  const usable = safeWidth - originX - Math.round(tokens.spacing.shellPad)
  const sectionGap = Math.round(tokens.spacing.section / 2)
  const gapX = Math.max(sectionGap, Math.round(tokens.type.body * 1.5))
  const gapY = gapX

  const byWidth = Math.floor((usable - gapX * (safeColumns - 1)) / safeColumns)
  const usableHeight = Math.max(1, bottom - originY)
  const rowSpace = (usableHeight - gapY * (safeRows - 1)) / safeRows
  const byHeight = Math.floor((rowSpace - titleStrip) * (16 / 9))
  const cardWidth = Math.max(1, Math.min(byWidth, byHeight))

  const artWidth = cardWidth
  const artHeight = Math.round((artWidth * 9) / 16)
  const cardHeight = artHeight + titleStrip

  return {
    columns: safeColumns,
    rows: safeRows,
    originX,
    originY,
    cardWidth,
    cardHeight,
    gapX,
    gapY,
    artWidth,
    artHeight,
    titleY: artHeight + titleGap,
    subtitleY: artHeight + titleGap + titleLine,
  }
}

function countOrOne(value: number): number {
  if (!Number.isFinite(value)) {
    return 1
  }
  const whole = Math.trunc(value)
  return whole >= 1 ? whole : 1
}

export function cellPosition(
  index: number,
  geometry: GridGeometry,
  columns: number = geometry.columns,
): { readonly x: number; readonly y: number } {
  const span = Number.isFinite(columns) && columns >= 1 ? Math.trunc(columns) : geometry.columns
  const safe = Math.max(0, Math.trunc(index))
  const column = safe % span
  const row = Math.floor(safe / span)
  return {
    x: geometry.originX + column * (geometry.cardWidth + geometry.gapX),
    y: geometry.originY + row * (geometry.cardHeight + geometry.gapY),
  }
}

export interface Card {
  readonly id: string
  readonly title: string
  readonly subtitle: string
  readonly x: number
  readonly y: number
  readonly width: number
  readonly height: number
  readonly fill: string
  readonly titleColor: string
  readonly subtitleColor: string

  readonly scale: number

  readonly focused: boolean

  readonly progress: number

  readonly progressWidth: number

  readonly progressVisible: boolean
  readonly progressColor: string

  readonly unavailable: boolean
}

export type CardItem = LibraryItem

export interface CardWindow {

  readonly first: number

  readonly columns: number
}

export function buildCards(
  items: readonly CardItem[],
  focusIndex: number,
  geometry: GridGeometry,
  tokens: Tokens = TOKENS,
  window: CardWindow = { first: 0, columns: geometry.columns },
): Card[] {
  const columns = Number.isFinite(window.columns) && window.columns >= 1 ? window.columns : geometry.columns
  return items.map((item, index) => {
    const absolute = window.first + index
    const position = cellPosition(absolute - window.first, geometry, columns)
    const focused = absolute === focusIndex
    const progress = progressFraction(item.positionMs, item.durationMs)
    return {
      id: item.id,
      title: item.title,
      subtitle: subtitleFor(item),
      x: position.x,
      y: position.y,
      width: geometry.cardWidth,
      height: geometry.cardHeight,
      fill: focused ? hex(tokens.palette.focusSurface) : hex(tokens.palette.surface1),
      titleColor: focused
        ? hex(tokens.palette.focusForeground)
        : hex(tokens.palette.foreground),
      subtitleColor: focused
        ? hex(tokens.palette.focusForeground)
        : hex(tokens.palette.inkMuted),
      scale: focused ? FOCUS_SCALE : 1,
      focused,
      progress,
      progressWidth: Math.round(geometry.artWidth * progress),
      progressVisible: progress > 0 && progress < 1,
      progressColor: focused ? hex(tokens.palette.focusForeground) : hex(tokens.palette.signal),
      unavailable: !item.available,
    }
  })
}

export function subtitleFor(item: CardItem): string {
  if (!item.available) {
    return 'Not readable — reconnect the drive'
  }
  const parts: string[] = []
  if (item.kind === 'series' && item.season !== null && item.episode !== null) {
    parts.push(`S${pad(item.season)}E${pad(item.episode)}`)
  } else if (item.year !== null) {
    parts.push(String(item.year))
  }
  if (item.watched) {
    parts.push('Watched')
  } else if (item.positionMs > 0) {
    const remaining =
      item.durationMs > item.positionMs ? item.durationMs - item.positionMs : 0
    parts.push(
      item.durationMs > 0 ? `${formatPosition(remaining)} left` : formatPosition(item.positionMs),
    )
  }
  return parts.length > 0 ? parts.join(' · ') : 'Not started'
}

function pad(value: number): string {
  return Math.max(0, Math.trunc(value)).toString().padStart(2, '0')
}
