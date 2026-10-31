import { scannableStorage } from '../core/scan.ts'
import { tizenGlobal, type TizenStorage } from './tizen.ts'

export interface MediaRoot {

  readonly label: string

  readonly type: string

  readonly state: string

  readonly mounted: boolean

  readonly removable: boolean

  readonly scannable: boolean
}

export type RootsResult =
  | { readonly ok: true; readonly roots: readonly MediaRoot[] }
  | { readonly ok: false; readonly reason: string }

export interface ReadEntry {

  readonly name: string

  readonly isDirectory: boolean | null
}

export type DirectoryResult =
  | { readonly ok: true; readonly entries: readonly ReadEntry[] }
  | { readonly ok: false; readonly reason: string }

export type UriResult =
  | { readonly ok: true; readonly uri: string; readonly via: 'toURI' | 'file-url' }
  | { readonly ok: false; readonly reason: string }

export async function listMediaRoots(): Promise<RootsResult> {
  const filesystem = tizenGlobal()?.filesystem
  if (filesystem === undefined) {
    return { ok: false, reason: 'This host has no removable-storage API, so no drive can be seen.' }
  }
  return await new Promise<RootsResult>((resolve) => {
    const onError = (error: unknown): void => {
      resolve({ ok: false, reason: `The TV refused to list storage: ${describe(error)}` })
    }
    try {
      filesystem.listStorages((storages: TizenStorage[]) => {
        const roots = (Array.isArray(storages) ? storages : []).map((storage) => {
          const type = typeof storage.type === 'string' ? storage.type : ''
          const state = typeof storage.state === 'string' ? storage.state : ''
          const label = typeof storage.label === 'string' ? storage.label : ''
          return {
            label,
            type,
            state,
            mounted: state.toUpperCase() === 'MOUNTED',
            removable: type.toUpperCase() === 'EXTERNAL',
            scannable: scannableStorage({ label, type, mounted: state.toUpperCase() === 'MOUNTED' }),
          }
        })
        resolve({ ok: true, roots })
      }, onError)
    } catch (error) {
      onError(error)
    }
  })
}

export function readEntriesFrom(list: unknown): ReadEntry[] {
  if (!Array.isArray(list)) {
    return []
  }
  const entries: ReadEntry[] = []
  for (const item of list) {
    if (typeof item === 'string') {
      if (item.length > 0) {
        entries.push({ name: item, isDirectory: null })
      }
      continue
    }
    if (item === null || typeof item !== 'object') {
      continue
    }
    const record = item as { name?: unknown; fullPath?: unknown; isDirectory?: unknown; isFile?: unknown }
    const name = typeof record.name === 'string' && record.name.length > 0 ? record.name : null
    if (name === null) {
      continue
    }
    entries.push({
      name,
      isDirectory:
        typeof record.isDirectory === 'boolean'
          ? record.isDirectory
          : record.isFile === true
            ? false
            : null,
    })
  }
  return entries
}

export async function readDirectory(path: string): Promise<DirectoryResult> {
  const filesystem = tizenGlobal()?.filesystem
  if (filesystem === undefined) {
    return { ok: false, reason: 'This host cannot list a folder.' }
  }
  return await new Promise<DirectoryResult>((resolve) => {
    const onError = (error: unknown): void => {
      resolve({ ok: false, reason: describe(error) })
    }
    const done = (entries: unknown): void => {
      resolve({ ok: true, entries: readEntriesFrom(entries) })
    }

    const legacy = (firstReason?: unknown): void => {
      if (typeof filesystem.listDirectory !== 'function') {
        onError(firstReason ?? new Error('the TV has no way to list a folder'))
        return
      }
      try {
        filesystem.listDirectory(path, done, onError)
      } catch (error) {
        onError(error)
      }
    }

    if (typeof filesystem.resolve !== 'function') {
      legacy()
      return
    }
    try {
      filesystem.resolve(
        path,
        (folder: { listFiles?: (ok: (files: unknown) => void, fail: (error: unknown) => void) => void }) => {
          if (typeof folder?.listFiles !== 'function') {
            legacy()
            return
          }
          try {
            folder.listFiles(done, onError)
          } catch (error) {
            onError(error)
          }
        },

        (error: unknown) => legacy(error),
        'r',
      )
    } catch (error) {
      onError(error)
    }
  })
}

export function isDirectoryPath(path: string): boolean | null {
  const filesystem = tizenGlobal()?.filesystem
  if (filesystem === undefined || typeof filesystem.isDirectory !== 'function') {
    return null
  }
  try {
    return filesystem.isDirectory(path) === true
  } catch {
    return null
  }
}

export function pathExists(path: string): boolean | null {
  const filesystem = tizenGlobal()?.filesystem
  if (filesystem === undefined || typeof filesystem.pathExists !== 'function') {
    return null
  }
  try {
    return filesystem.pathExists(path) === true
  } catch {
    return null
  }
}

export function fileUri(location: string): UriResult {
  if (location.length === 0) {
    return { ok: false, reason: 'The file has no location to open.' }
  }
  const filesystem = tizenGlobal()?.filesystem
  if (filesystem !== undefined && typeof filesystem.toURI === 'function') {
    try {
      const uri = filesystem.toURI(location)
      if (typeof uri === 'string' && uri.length > 0) {
        return { ok: true, uri, via: 'toURI' }
      }
    } catch {

    }
  }

  if (!location.startsWith('/') && !/^[a-zA-Z]:/.test(location)) {
    return {
      ok: false,
      reason:
        'The TV would not convert this drive location into a file address, and this app will not guess one.',
    }
  }
  const encoded = location
    .split('/')
    .map((segment) => encodeURIComponent(segment))
    .join('/')
  return { ok: true, uri: `file://${encoded}`, via: 'file-url' }
}

export type StorageWatch =
  | { readonly watching: true; readonly stop: () => void }
  | { readonly watching: false; readonly reason: string }

export function watchStorageChanges(
  onChange: (storage: { readonly label: string; readonly state: string }) => void,
): StorageWatch {
  const filesystem = tizenGlobal()?.filesystem
  if (filesystem === undefined || typeof filesystem.addStorageStateChangeListener !== 'function') {
    return { watching: false, reason: 'This host does not report storage changes.' }
  }
  try {
    const watchId = filesystem.addStorageStateChangeListener((storage: TizenStorage) => {
      onChange({
        label: typeof storage?.label === 'string' ? storage.label : '',
        state: typeof storage?.state === 'string' ? storage.state : '',
      })
    })
    return {
      watching: true,
      stop: () => {
        try {
          filesystem.removeStorageStateChangeListener?.(watchId)
        } catch {

        }
      },
    }
  } catch (error) {
    return { watching: false, reason: describe(error) }
  }
}

function describe(error: unknown): string {
  if (typeof error === 'string') {
    return error
  }
  if (error instanceof Error) {
    return error.message
  }
  if (typeof error === 'object' && error !== null && 'message' in error) {
    const message = (error as { message?: unknown }).message
    if (typeof message === 'string') {
      return message
    }
  }
  return 'no message'
}
