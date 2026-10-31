export interface TizenSystemInfo {
  getPropertyValue(property: string): string | null
}

export interface TizenStorage {

  readonly label: string

  readonly state: string

  readonly type: string
}

export interface TizenFileHandle {
  readonly name?: string
  readonly isDirectory?: boolean
  readonly path?: string

  listFiles?(onSuccess: (files: unknown) => void, onError?: (error: unknown) => void): void
}

export interface TizenFilesystem {
  listStorages(
    onSuccess: (storages: TizenStorage[]) => void,
    onError: (error: unknown) => void,
  ): void

  resolve?(
    location: string,
    onSuccess: (file: TizenFileHandle) => void,
    onError?: (error: unknown) => void,
    mode?: string,
  ): void

  listDirectory?(
    path: string,
    onSuccess: (entries: unknown, path?: string) => void,
    onError?: (error: unknown) => void,
    filter?: unknown,
  ): void

  isDirectory(path: string): boolean

  pathExists(path: string): boolean

  toURI(path: string): string

  addStorageStateChangeListener?(
    onSuccess: (storage: TizenStorage) => void,
    onError?: (error: unknown) => void,
  ): number
  removeStorageStateChangeListener?(watchId: number): void
}

export interface TizenTvInputDevice {
  registerKey(keyName: string): void
  registerKeyBatch(keyNames: string[]): void
  getSupportedKeys(): readonly { readonly name: string; readonly code: string }[]
}

export interface AvPlay {
  open(uri: string): void
  close(): void
  prepareAsync(onSuccess: () => void, onError: (error: unknown) => void): void
  play(): void
  pause(): void
  stop(): void
  seekTo(milliseconds: number, onSuccess?: () => void, onError?: (error: unknown) => void): void
  jumpForward(milliseconds: number): void
  jumpBackward(milliseconds: number): void
  getState(): string
  getCurrentTime(): number
  getDuration(): number
  getTotalTrackInfo(): readonly AvPlayTrack[]
  setSelectTrack(type: 'AUDIO' | 'TEXT', index: number): void

  setSilentSubtitle(silent: boolean): void

  setExternalSubtitlePath(path: string): void
  setDisplayRect(x: number, y: number, width: number, height: number): void
  setDisplayMethod(method: string): void
  setListener(listener: AvPlayListener): void
  suspend(): void
  restore(): void
}

export interface AvPlayTrack {
  readonly type: string
  readonly index: number

  readonly extra_info?: string
}

export interface AvPlayListener {
  onbufferingstart?: () => void
  onbufferingprogress?: (percent: number) => void
  onbufferingcomplete?: () => void
  oncurrentplaytime?: (milliseconds: number) => void
  onstreamcompleted?: () => void
  onerror?: (errorType: string) => void
  onevent?: (eventType: string, eventData: string) => void
  onsubtitlechange?: (duration: number, text: string) => void
}

export interface TizenApplicationManager {
  getCurrentApplication(): { exit(): void }
}

export interface TizenProductInfo {
  getModel?(): string
  getModelCode?(): string
  getFirmware?(): string
  getVersion?(): string
  getDuid?(): string
}

export interface TizenWebApis {
  readonly avplay?: AvPlay
  readonly productinfo?: TizenProductInfo
}

export interface TizenGlobal {
  readonly webapis?: TizenWebApis
  readonly application?: TizenApplicationManager
  readonly filesystem?: TizenFilesystem
  readonly tvinputdevice?: TizenTvInputDevice
  readonly systeminfo?: TizenSystemInfo
}

export function tizenGlobal(): TizenGlobal | null {
  const candidate = (globalThis as { tizen?: unknown }).tizen
  return typeof candidate === 'object' && candidate !== null ? (candidate as TizenGlobal) : null
}

export function webapisGlobal(): TizenWebApis | null {
  const candidate = (globalThis as { webapis?: unknown }).webapis
  if (typeof candidate === 'object' && candidate !== null) {
    return candidate as TizenWebApis
  }
  return tizenGlobal()?.webapis ?? null
}

export function hasAvPlay(): boolean {
  return webapisGlobal()?.avplay !== undefined
}

export function exitApp(): boolean {
  const application = tizenGlobal()?.application
  if (application === undefined) {
    return false
  }
  try {
    application.getCurrentApplication().exit()
    return true
  } catch {
    return false
  }
}
