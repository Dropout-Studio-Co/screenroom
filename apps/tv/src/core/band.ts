export interface BandState {
  readonly scrollRows: number

  readonly padRow: number

  readonly offset: number
}

export interface BandStep {
  readonly state: BandState
  readonly animate: boolean
}

export const BAND_PADDING = 1

export function bandRows(visibleRows: number, padding: number = BAND_PADDING): number {
  const rows = Number.isFinite(visibleRows) && visibleRows >= 1 ? Math.trunc(visibleRows) : 1
  const pad = Number.isFinite(padding) && padding >= 0 ? Math.trunc(padding) : 0
  return rows + pad * 2
}

export function bandStart(): BandState {
  return { scrollRows: 0, padRow: 0, offset: 0 }
}

export function bandOffset(scrollRows: number, padRow: number, pitch: number): number {
  const rows = Number.isFinite(scrollRows) ? Math.trunc(scrollRows) : 0
  const pad = Number.isFinite(padRow) ? Math.trunc(padRow) : 0
  const step = Number.isFinite(pitch) ? pitch : 0
  const offset = -(rows - pad) * step
  return offset === 0 ? 0 : offset
}

export function bandWindow(
  total: number,
  columns: number,
  padRow: number,
  visibleRows: number,
  padding: number = BAND_PADDING,
): { readonly first: number; readonly count: number } {
  const cols = Number.isFinite(columns) && columns >= 1 ? Math.trunc(columns) : 1
  const safeTotal = Number.isFinite(total) && total > 0 ? Math.trunc(total) : 0
  const first = Math.max(0, Math.trunc(padRow)) * cols
  const wanted = bandRows(visibleRows, padding) * cols
  const count = Math.max(0, Math.min(safeTotal - first, wanted))
  return { first, count }
}

export function bandMove(
  state: BandState,
  scrollRows: number,
  pitch: number,
  visibleRows: number,
  padding: number = BAND_PADDING,
): BandStep {
  const target = Math.max(0, Number.isFinite(scrollRows) ? Math.trunc(scrollRows) : 0)
  if (target === state.scrollRows) {
    return { state, animate: false }
  }
  const pad = Number.isFinite(padding) && padding >= 0 ? Math.trunc(padding) : 0
  const rows = Number.isFinite(visibleRows) && visibleRows >= 1 ? Math.trunc(visibleRows) : 1
  const reachable = target >= state.padRow && target <= state.padRow + pad + rows - 1
  if (reachable) {
    return {
      state: {
        scrollRows: target,
        padRow: state.padRow,
        offset: bandOffset(target, state.padRow, pitch),
      },
      animate: true,
    }
  }
  const padRow = Math.max(0, target - pad)
  return {
    state: {
      scrollRows: target,
      padRow,
      offset: bandOffset(target, padRow, pitch),
    },
    animate: false,
  }
}

export function bandSettle(
  state: BandState,
  pitch: number,
  padding: number = BAND_PADDING,
): BandState {
  const pad = Number.isFinite(padding) && padding >= 0 ? Math.trunc(padding) : 0
  const padRow = Math.max(0, state.scrollRows - pad)
  return {
    scrollRows: state.scrollRows,
    padRow,
    offset: bandOffset(state.scrollRows, padRow, pitch),
  }
}

export function bandScreenOffset(row: number, state: BandState, pitch: number): number {
  return (row - state.padRow) * pitch + state.offset
}

export function bandAtRest(state: BandState, pitch: number): boolean {
  return Math.abs(state.offset - bandOffset(state.scrollRows, state.padRow, pitch)) < 0.5
}
