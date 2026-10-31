import { clampIndex, moveFocus, type Direction } from './focus.ts'

export interface RailEntry {

  readonly key: string

  readonly label: string

  readonly route: string
}

export const RAIL_ENTRIES: readonly RailEntry[] = [
  { key: 'home', label: 'Home', route: '/' },
  { key: 'movies', label: 'Movies', route: '/movies' },
  { key: 'series', label: 'TV Shows', route: '/series' },
  { key: 'search', label: 'Search', route: '/search' },
  { key: 'sources', label: 'Media Sources', route: '/sources' },
  { key: 'settings', label: 'Settings', route: '/settings' },
]

export function railIndexFor(route: string): number {

  const trimmed = route.split('?')[0]?.replace(/\/+$/, '') ?? ''
  const path = trimmed.length === 0 ? '/' : trimmed
  return RAIL_ENTRIES.findIndex((entry) => entry.route === path)
}

export function railEntryFor(route: string): RailEntry | null {
  const index = railIndexFor(route)
  return index < 0 ? null : (RAIL_ENTRIES[index] ?? null)
}

export type NavZone = 'rail' | 'content'

export interface NavPosition {
  readonly zone: NavZone
  readonly rail: number
  readonly content: number
}

export interface NavBounds {
  readonly railCount: number
  readonly contentCount: number

  readonly columns: number
}

export function initialNav(zone: NavZone = 'content'): NavPosition {
  return { zone, rail: 0, content: 0 }
}

function countOf(value: number): number {
  return Number.isFinite(value) && value > 0 ? Math.trunc(value) : 0
}

function columnsOf(value: number): number {
  return Number.isFinite(value) && value >= 1 ? Math.trunc(value) : 1
}

export function moveNav(
  position: NavPosition,
  direction: Direction,
  bounds: NavBounds,
): NavPosition {
  const railCount = Math.max(1, countOf(bounds.railCount) || RAIL_ENTRIES.length)
  const contentCount = countOf(bounds.contentCount)
  const columns = columnsOf(bounds.columns)

  const rail = Math.max(0, clampIndex(position.rail, railCount))
  const content = contentCount === 0 ? -1 : clampIndex(position.content, contentCount)

  const zone: NavZone = contentCount === 0 ? 'rail' : position.zone

  if (zone === 'rail') {
    switch (direction) {
      case 'up':
        return { zone, rail: Math.max(0, rail - 1), content }
      case 'down':
        return { zone, rail: Math.min(railCount - 1, rail + 1), content }
      case 'right':

        return contentCount === 0
          ? { zone, rail, content }
          : { zone: 'content', rail, content: Math.max(0, content) }
      case 'left':
        return { zone, rail, content }
    }
  }

  if (direction === 'left' && content % columns === 0) {
    return { zone: 'rail', rail, content }
  }
  return { zone: 'content', rail, content: moveFocus(content, contentCount, columns, direction) }
}

export interface GridWindow {

  readonly first: number

  readonly count: number

  readonly rows: number

  readonly scrollRows: number

  readonly totalRows: number
}

export function gridWindow(
  total: number,
  columns: number,
  visibleRows: number,
  focusIndex: number,
): GridWindow {
  const safeTotal = countOf(total)
  const cols = columnsOf(columns)
  const rowsVisible = Math.max(1, countOf(visibleRows) || 1)
  if (safeTotal === 0) {
    return { first: 0, count: 0, rows: 0, scrollRows: 0, totalRows: 0 }
  }
  const totalRows = Math.ceil(safeTotal / cols)
  const focus = clampIndex(focusIndex, safeTotal)
  const focusRow = Math.min(totalRows - 1, Math.max(0, Math.floor(focus / cols)))
  const lastStart = Math.max(0, totalRows - rowsVisible)
  const scrollRows = Math.min(lastStart, Math.max(0, focusRow - rowsVisible + 1))
  const first = scrollRows * cols
  const count = Math.min(safeTotal - first, rowsVisible * cols)
  return {
    first,
    count,
    rows: Math.min(rowsVisible, count === 0 ? 0 : Math.ceil(count / cols)),
    scrollRows,
    totalRows,
  }
}

export interface NavMemory {
  get(key: string): NavPosition | undefined

  set(key: string, position: NavPosition, bounds: NavBounds): void
  clear(key: string): void
}

export function createNavMemory(): NavMemory {
  const remembered = new Map<string, NavPosition>()
  return {
    get(key: string): NavPosition | undefined {
      return remembered.get(key)
    },
    set(key: string, position: NavPosition, bounds: NavBounds): void {
      const railCount = Math.max(1, countOf(bounds.railCount) || RAIL_ENTRIES.length)
      const contentCount = countOf(bounds.contentCount)
      remembered.set(key, {
        zone: contentCount === 0 ? 'rail' : position.zone,
        rail: Math.max(0, clampIndex(position.rail, railCount)),
        content: contentCount === 0 ? -1 : clampIndex(position.content, contentCount),
      })
    },
    clear(key: string): void {
      remembered.delete(key)
    },
  }
}

export function restoreNav(
  memory: NavMemory,
  key: string,
  bounds: NavBounds,
  fallback: NavZone,
): NavPosition {
  const remembered = memory.get(key)
  if (remembered === undefined) {
    const base = initialNav(fallback)
    return contentOf(bounds) === 0 ? { ...base, zone: 'rail', content: -1 } : base
  }
  const contentCount = contentOf(bounds)
  const railCount = Math.max(1, countOf(bounds.railCount) || RAIL_ENTRIES.length)
  if (contentCount === 0) {
    return {
      zone: 'rail',
      rail: Math.max(0, clampIndex(remembered.rail, railCount)),
      content: -1,
    }
  }
  return {
    zone: remembered.zone,
    rail: Math.max(0, clampIndex(remembered.rail, railCount)),
    content: clampIndex(remembered.content, contentCount),
  }
}

function contentOf(bounds: NavBounds): number {
  return countOf(bounds.contentCount)
}
