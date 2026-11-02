import { hex, TOKENS, type Tokens } from './tokens.ts'
import { focusMotion } from '../core/motion.ts'
import { RAIL_ENTRIES } from '../core/nav.ts'

export const RAIL_WIDTH = 320

export interface RailRow {
  readonly key: string
  readonly label: string
  readonly route: string
  readonly x: number
  readonly y: number
  readonly width: number
  readonly height: number

  readonly labelX: number
  readonly labelY: number
  readonly fill: string
  readonly labelColor: string

  readonly markerColor: string

  readonly active: boolean

  readonly focused: boolean

  readonly scale: number
}

export interface RailLayout {
  readonly pad: number
  readonly rowX: number
  readonly rowWidth: number
  readonly rowHeight: number
  readonly rowGap: number

  readonly top: number
  readonly labelX: number
  readonly labelY: number
  readonly titleSize: number
  readonly labelSize: number
  readonly markerWidth: number
}

export function railLayout(tokens: Tokens = TOKENS): RailLayout {
  const pad = Math.round(tokens.spacing.shellPad)
  const rowHeight = Math.round(tokens.control.lg * 1.2)
  const gap = Math.round(tokens.type.caption * 0.9)
  const labelSize = Math.round(tokens.type.body)
  const titleSize = Math.round(tokens.type.bodyLg)
  const markerWidth = 6
  const rowX = Math.round(pad * 0.4)
  return {
    pad,
    rowX,
    rowWidth: RAIL_WIDTH - rowX * 2,
    rowHeight,
    rowGap: rowHeight + gap,

    top: Math.round(pad + tokens.type.title * 2.3),
    labelX: markerWidth + Math.round(tokens.type.caption * 0.7),
    labelY: Math.round((rowHeight - labelSize * 1.25) / 2),
    titleSize,
    labelSize,
    markerWidth,
  }
}

export function railRows(
  activeIndex: number,
  focusedIndex: number,
  focused: boolean,
  tokens: Tokens = TOKENS,
): RailRow[] {
  const layout = railLayout(tokens)
  const lift = focusMotion().lift
  return RAIL_ENTRIES.map((entry, index) => {
    const isFocused = focused && index === focusedIndex
    const active = index === activeIndex
    return {
      key: entry.key,
      label: entry.label,
      route: entry.route,
      x: layout.rowX,
      y: layout.top + index * layout.rowGap,
      width: layout.rowWidth,
      height: layout.rowHeight,
      labelX: layout.labelX,
      labelY: layout.labelY,
      fill: isFocused
        ? hex(tokens.palette.focusSurface)
        : active
          ? hex(tokens.palette.surface2)
          : hex(tokens.palette.surface1),
      labelColor: isFocused
        ? hex(tokens.palette.focusForeground)
        : active
          ? hex(tokens.palette.foreground)
          : hex(tokens.palette.inkMuted),
      markerColor: hex(tokens.palette.signal),
      active,
      focused: isFocused,
      scale: isFocused ? lift : 1,
    }
  })
}

