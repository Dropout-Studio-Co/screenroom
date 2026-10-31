import { tizenGlobal, webapisGlobal } from './tizen.ts'

export type HostKind =

  | 'tizen'

  | 'browser'

export interface HostCapabilities {
  readonly kind: HostKind

  readonly avplay: boolean

  readonly filesystem: boolean

  readonly inputdevice: boolean

  readonly durableStorage: boolean

  readonly userAgent: string

  readonly notes: readonly string[]
}

export interface DeviceInfo {

  readonly model: string | null
  readonly firmwareVersion: string | null

  readonly platformVersion: string | null

  readonly productVersion: string | null
}

function probeLocalStorage(): boolean {
  try {
    const key = '__screenroom_probe__'
    globalThis.localStorage.setItem(key, '1')
    const read = globalThis.localStorage.getItem(key) === '1'
    globalThis.localStorage.removeItem(key)
    return read
  } catch {
    return false
  }
}

export function detectHost(): HostCapabilities {
  const tizen = tizenGlobal()

  const avplay = webapisGlobal()?.avplay !== undefined
  const filesystem = tizen?.filesystem !== undefined
  const inputdevice = tizen?.tvinputdevice !== undefined
  const durableStorage = probeLocalStorage()
  const kind: HostKind = tizen !== null && avplay ? 'tizen' : 'browser'

  const notes: string[] = []
  if (kind === 'browser') {
    notes.push(
      'This is a desktop browser, not the TV. Playback here uses a <video> element and proves nothing about AVPlay.',
    )
  }
  if (!avplay) {
    notes.push('AVPlay is absent, so local file playback cannot be exercised on this host.')
  }
  if (!filesystem) {
    notes.push('The Tizen filesystem API is absent, so USB storage cannot be enumerated here.')
  }
  if (!inputdevice) {
    notes.push('The Tizen input-device API is absent, so media keys will not arrive here.')
  }
  if (!durableStorage) {
    notes.push('Storage refused a probe write. Positions and settings cannot be kept on this host.')
  }

  return {
    kind,
    avplay,
    filesystem,
    inputdevice,
    durableStorage,
    userAgent: globalThis.navigator?.userAgent ?? '',
    notes,
  }
}

export function hostSummary(capabilities: HostCapabilities): string {
  if (capabilities.kind === 'tizen' && capabilities.avplay) {
    return 'Samsung TV: AVPlay is present. Local playback uses the TV decoder.'
  }
  return 'Desktop browser preview: AVPlay is absent, so nothing here proves TV playback.'
}

export function readDeviceInfo(): DeviceInfo {
  const product = webapisGlobal()?.productinfo
  const fromProduct = (name: 'getModel' | 'getFirmware' | 'getVersion'): string | null => {
    const read = product?.[name]
    if (typeof read !== 'function') {
      return null
    }
    try {

      const value = read.call(product)
      return typeof value === 'string' && value.length > 0 ? value : null
    } catch {
      return null
    }
  }

  const systeminfo = tizenGlobal()?.systeminfo
  const read = (key: string): string | null => {
    if (systeminfo === undefined) {
      return null
    }
    try {
      return systeminfo.getPropertyValue(key) ?? null
    } catch {

      return null
    }
  }
  return {
    model: fromProduct('getModel') ?? read('http://tizen.org/system/model_name'),
    firmwareVersion:
      fromProduct('getFirmware') ?? read('http://tizen.org/system/build_version'),
    platformVersion: read('http://tizen.org/system/platform.version'),
    productVersion: fromProduct('getVersion'),
  }
}
