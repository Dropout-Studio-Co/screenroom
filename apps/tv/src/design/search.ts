import { SEARCH_KEYS } from '../core/search.ts'
import { hex, TOKENS, type Tokens } from './tokens.ts'

export const SEARCH_COLUMNS = 6

export const SEARCH_RESULT_ROWS = 8

export interface KeyCell {
  readonly id: string
  readonly label: string
  readonly x: number
  readonly y: number
  readonly width: number
  readonly height: number
  readonly fill: string
  readonly labelColor: string
  readonly focused: boolean
}

export interface KeyGeometry {
  readonly columns: number
  readonly rows: number
  readonly width: number
  readonly height: number
  readonly gap: number
  readonly panelWidth: number
  readonly panelHeight: number
}

export function keyGeometry(tokens: Tokens = TOKENS): KeyGeometry {
  const columns = SEARCH_COLUMNS
  const rows = Math.ceil(SEARCH_KEYS.length / columns)
  const width = Math.round(tokens.type.body * 3.7)
  const height = Math.round(tokens.control.lg * 1.2)
  const gap = Math.round(tokens.type.caption * 0.5)
  return {
    columns,
    rows,
    width,
    height,
    gap,
    panelWidth: columns * width + (columns - 1) * gap,
    panelHeight: rows * height + (rows - 1) * gap,
  }
}

export function resultRowHeight(tokens: Tokens = TOKENS): number {
  return Math.round(tokens.type.body * 2.5)
}

export function keyCells(
  focusIndex: number,
  originX: number,
  originY: number,
  tokens: Tokens = TOKENS,
): KeyCell[] {
  const geometry = keyGeometry(tokens)
  return SEARCH_KEYS.map((key, index) => {
    const column = index % geometry.columns
    const row = Math.floor(index / geometry.columns)
    const focused = index === focusIndex
    return {
      id: key.id,
      label: key.label,
      x: originX + column * (geometry.width + geometry.gap),
      y: originY + row * (geometry.height + geometry.gap),
      width: geometry.width,
      height: geometry.height,
      fill: focused ? hex(tokens.palette.focusSurface) : hex(tokens.palette.surface1),
      labelColor: focused
        ? hex(tokens.palette.focusForeground)
        : hex(tokens.palette.foreground),
      focused,
    }
  })
}
