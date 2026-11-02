import { MOTION } from '../core/motion.ts'

export type Color = number

function rgb(red: number, green: number, blue: number, alpha = 1): Color {
  const byte = (value: number): number => Math.round(Math.min(1, Math.max(0, value)) * 255)
  return (
    ((byte(red) << 24) | (byte(green) << 16) | (byte(blue) << 8) | byte(alpha)) >>> 0
  )
}

const ROOT_PX = 16

export const TYPE_SCALE = 1.75

export const FOCUS_SCALE = MOTION.focusLift

export interface Palette {

  readonly background: Color

  readonly surface1: Color

  readonly surface2: Color

  readonly surface3: Color

  readonly foreground: Color

  readonly inkMuted: Color

  readonly signal: Color

  readonly signalForeground: Color

  readonly focusSurface: Color

  readonly focusForeground: Color

  readonly destructive: Color

  readonly destructiveForeground: Color

  readonly hair: Color

  readonly disabledForeground: Color
}

export interface TypeScale {
  readonly micro: number
  readonly caption: number
  readonly label: number
  readonly body: number
  readonly bodyLg: number
  readonly lead: number
  readonly subtitle: number
  readonly titleSm: number
  readonly title: number
  readonly titleLg: number
}

export interface RadiusScale {
  readonly small: number
  readonly control: number
  readonly surface: number
  readonly overlay: number
  readonly full: number
}

export interface ControlScale {
  readonly sm: number
  readonly md: number
  readonly lg: number
}

export interface Spacing {

  readonly shellPad: number

  readonly contentPad: number

  readonly section: number

  readonly fieldX: number

  readonly fieldY: number
}

export interface Tokens {
  readonly palette: Palette
  readonly type: TypeScale
  readonly radius: RadiusScale
  readonly control: ControlScale
  readonly spacing: Spacing

  readonly provenance: string
}

export const TOKENS: Tokens = {
  palette: {
    background: rgb(0.07, 0.07, 0.08),
    surface1: rgb(0.11, 0.11, 0.13),
    surface2: rgb(0.15, 0.15, 0.17),
    surface3: rgb(0.19, 0.19, 0.22),
    foreground: rgb(0.96, 0.96, 0.97),
    inkMuted: rgb(0.66, 0.66, 0.7),
    signal: rgb(0.99, 0.99, 1.0),
    signalForeground: rgb(0.08, 0.08, 0.09),
    focusSurface: rgb(0.98, 0.98, 1.0),
    focusForeground: rgb(0.06, 0.06, 0.07),
    destructive: rgb(0.85, 0.35, 0.35),
    destructiveForeground: rgb(0.06, 0.06, 0.07),
    hair: rgb(1.0, 1.0, 1.0, 0.1),
    disabledForeground: rgb(0.45, 0.45, 0.49),
  },
  type: {
    micro: 0.625 * ROOT_PX * TYPE_SCALE,
    caption: 0.75 * ROOT_PX * TYPE_SCALE,
    label: 0.875 * ROOT_PX * TYPE_SCALE,
    body: ROOT_PX * TYPE_SCALE,
    bodyLg: 1.0625 * ROOT_PX * TYPE_SCALE,
    lead: 1.125 * ROOT_PX * TYPE_SCALE,
    subtitle: 1.5 * ROOT_PX * TYPE_SCALE,
    titleSm: 1.875 * ROOT_PX * TYPE_SCALE,
    title: 2.25 * ROOT_PX * TYPE_SCALE,
    titleLg: 3.0 * ROOT_PX * TYPE_SCALE,
  },
  radius: {
    small: 0.5 * ROOT_PX,
    control: 0.75 * ROOT_PX,
    surface: 0.875 * ROOT_PX,
    overlay: ROOT_PX,
    full: 9999,
  },
  control: {

    sm: 2.75 * ROOT_PX,
    md: 3.25 * ROOT_PX,
    lg: 3.75 * ROOT_PX,
  },
  spacing: {
    shellPad: 1.125 * ROOT_PX * TYPE_SCALE,
    contentPad: ROOT_PX * TYPE_SCALE,
    section: 4.0 * ROOT_PX * TYPE_SCALE,
    fieldX: 0.9375 * ROOT_PX * TYPE_SCALE,
    fieldY: 0.8125 * ROOT_PX * TYPE_SCALE,
  },
  provenance: 'public fallback (no design-system pin in this build)',
}

export const MIN_LEGIBLE_PX = 24

export function hex(color: Color): string {
  const value = color >>> 0
  const part = (shift: number): string =>
    (((value >>> shift) & 0xff) as number).toString(16).padStart(2, '0')
  return `#${part(24)}${part(16)}${part(8)}`
}
