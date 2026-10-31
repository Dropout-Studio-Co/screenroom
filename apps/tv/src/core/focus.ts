export type Direction = 'left' | 'right' | 'up' | 'down'

export function clampIndex(index: number, count: number): number {
  if (count <= 0) {
    return -1
  }
  if (!Number.isFinite(index)) {
    return 0
  }
  const whole = Math.trunc(index)
  if (whole < 0) {
    return 0
  }
  return whole > count - 1 ? count - 1 : whole
}

export function moveFocus(
  index: number,
  count: number,
  columns: number,
  direction: Direction,
): number {
  if (count <= 0) {
    return -1
  }
  const current = clampIndex(index, count)
  const stride = Number.isFinite(columns) && columns >= 1 ? Math.trunc(columns) : 1

  switch (direction) {
    case 'left':
      return current > 0 ? current - 1 : current
    case 'right':
      return current + 1 < count ? current + 1 : current
    case 'up':
      return current - stride >= 0 ? current - stride : current
    case 'down':
      return current + stride < count ? current + stride : current
  }
}

export interface FocusMemory {

  get(key: string): number | undefined

  set(key: string, index: number, count: number): void

  clear(key: string): void
}

export function createFocusMemory(): FocusMemory {
  const remembered = new Map<string, number>()
  return {
    get(key: string): number | undefined {
      return remembered.get(key)
    },
    set(key: string, index: number, count: number): void {
      remembered.set(key, clampIndex(index, count))
    },
    clear(key: string): void {
      remembered.delete(key)
    },
  }
}

export function restoreFocus(memory: FocusMemory, key: string, count: number): number {
  if (count <= 0) {
    return -1
  }
  const remembered = memory.get(key)
  if (remembered === undefined) {
    return 0
  }
  return clampIndex(remembered, count)
}
