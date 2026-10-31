export type RemoteKey =
  | 'left'
  | 'right'
  | 'up'
  | 'down'
  | 'enter'
  | 'back'
  | 'playPause'
  | 'play'
  | 'pause'
  | 'stop'
  | 'rewind'
  | 'fastForward'
  | 'info'
  | 'channelUp'
  | 'channelDown'

export const TIZEN_KEY_NAMES: Readonly<Record<RemoteKey, string | null>> = {

  left: null,
  right: null,
  up: null,
  down: null,
  enter: null,
  back: null,
  playPause: 'MediaPlayPause',
  play: 'MediaPlay',
  pause: 'MediaPause',
  stop: 'MediaStop',
  rewind: 'MediaRewind',
  fastForward: 'MediaFastForward',
  info: 'Info',
  channelUp: 'ChannelUp',
  channelDown: 'ChannelDown',
}

export const REMOTE_KEYMAP: Readonly<Record<number, string>> = {

  37: 'left',
  39: 'right',
  38: 'up',
  40: 'down',
  13: 'enter',
  8: 'back',
  27: 'escape',

  10009: 'back',
  10252: 'playPause',
  415: 'play',
  19: 'pause',
  413: 'stop',
  412: 'rewind',
  417: 'fastForward',
  457: 'info',
  427: 'channelUp',
  428: 'channelDown',
}

export interface KeyRegistration {

  readonly registered: readonly string[]

  readonly refused: readonly { readonly key: string; readonly reason: string }[]

  readonly supported: boolean
}

export function registerRemoteKeys(): KeyRegistration {
  const device = globalThis as { tizen?: { tvinputdevice?: { registerKey(k: string): void } } }
  const inputdevice = device.tizen?.tvinputdevice
  if (inputdevice === undefined) {
    return { registered: [], refused: [], supported: false }
  }

  const registered: string[] = []
  const refused: { key: string; reason: string }[] = []
  for (const name of Object.values(TIZEN_KEY_NAMES)) {
    if (name === null) {
      continue
    }
    try {
      inputdevice.registerKey(name)
      registered.push(name)
    } catch (error) {
      refused.push({ key: name, reason: describe(error) })
    }
  }
  return { registered, refused, supported: true }
}

function describe(error: unknown): string {
  if (error instanceof Error) {
    return error.message
  }
  if (typeof error === 'object' && error !== null && 'message' in error) {
    const message = (error as { message?: unknown }).message
    return typeof message === 'string' ? message : 'no message'
  }
  return String(error)
}

export function isRemoteKey(name: string): name is RemoteKey {
  return name in TIZEN_KEY_NAMES
}
