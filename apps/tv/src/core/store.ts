import type {
  LibraryItem,
  MediaKind,
  Profile,
  ProfileId,
  Source,
  SourceState,
} from './contract.ts'
import { decodeScanReport, NEVER_SCANNED, type ScanReport } from './scan.ts'
import { decodeSettings, DEFAULT_SETTINGS, type DeviceSettings } from './settings.ts'

export const STORE_FORMAT_VERSION = 2

export const MAX_STORE_CHARS = 8 * 1024 * 1024

export const MAX_ROWS = 200_000

export interface StoreSnapshot {
  readonly formatVersion: number
  readonly profiles: readonly Profile[]
  readonly sources: readonly Source[]
  readonly items: readonly LibraryItem[]

  readonly activeProfileId: ProfileId | null

  readonly hiddenWorkIds: readonly string[]

  readonly settings: DeviceSettings

  readonly lastScan: ScanReport
}

export function emptySnapshot(): StoreSnapshot {
  return {
    formatVersion: STORE_FORMAT_VERSION,
    profiles: [],
    sources: [],
    items: [],
    activeProfileId: null,
    hiddenWorkIds: [],
    settings: DEFAULT_SETTINGS,
    lastScan: NEVER_SCANNED,
  }
}

export interface StoreIssue {
  readonly collection: 'profiles' | 'sources' | 'items' | 'document'
  readonly reason: string

  readonly index: number
}

export interface StoreReadResult {
  readonly snapshot: StoreSnapshot

  readonly reset: boolean
  readonly issues: readonly StoreIssue[]
}

const SOURCE_STATES: readonly SourceState[] = [
  'available',
  'disconnected',
  'permissionLost',
  'missing',
  'removed',
]

const KINDS: readonly MediaKind[] = ['movie', 'series', 'other']

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

function wholeOrNull(value: unknown): number | null {
  return typeof value === 'number' && Number.isSafeInteger(value) ? value : null
}

function whole(value: unknown, fallback = 0): number {
  return wholeOrNull(value) ?? fallback
}

function text(value: unknown): string | null {
  return typeof value === 'string' ? value : null
}

function boolean(value: unknown): boolean {
  return value === true
}

function readProfile(row: unknown): Profile | null {
  if (!isRecord(row)) {
    return null
  }
  const id = text(row['id'])
  const name = text(row['name'])
  if (id === null || id.length === 0 || name === null) {
    return null
  }
  return { id, name }
}

function readSource(row: unknown): Source | null {
  if (!isRecord(row)) {
    return null
  }
  const id = text(row['id'])
  const label = text(row['label'])
  const rootPath = text(row['rootPath'])
  const state = text(row['state'])
  if (id === null || id.length === 0 || label === null || rootPath === null || state === null) {
    return null
  }
  if (!SOURCE_STATES.includes(state as SourceState)) {
    return null
  }
  return { id, label, rootPath, state: state as SourceState, lastScanMs: whole(row['lastScanMs']) }
}

function readItem(row: unknown): LibraryItem | null {
  if (!isRecord(row)) {
    return null
  }
  const id = text(row['id'])
  const workId = text(row['workId'])
  const title = text(row['title'])
  const kind = text(row['kind'])
  const relativePath = text(row['relativePath'])
  const sourceId = text(row['sourceId'])
  if (
    id === null ||
    id.length === 0 ||
    workId === null ||
    workId.length === 0 ||
    title === null ||
    relativePath === null ||
    sourceId === null ||
    kind === null ||
    !KINDS.includes(kind as MediaKind)
  ) {
    return null
  }
  return {
    id,
    workId,
    title,
    kind: kind as MediaKind,
    season: wholeOrNull(row['season']),
    episode: wholeOrNull(row['episode']),
    year: wholeOrNull(row['year']),
    relativePath,
    sourceId,
    available: boolean(row['available']),
    favorite: boolean(row['favorite']),
    watched: boolean(row['watched']),
    hidden: boolean(row['hidden']),
    positionMs: Math.max(0, whole(row['positionMs'])),
    durationMs: Math.max(0, whole(row['durationMs'])),
    updatedMs: Math.max(0, whole(row['updatedMs'])),
  }
}

function readCollection<T>(
  value: unknown,
  collection: StoreIssue['collection'],
  read: (row: unknown) => T | null,
  issues: StoreIssue[],
): T[] {
  if (!Array.isArray(value)) {
    if (value !== undefined) {
      issues.push({ collection, reason: 'not a list', index: -1 })
    }
    return []
  }
  if (value.length > MAX_ROWS) {
    issues.push({ collection, reason: `more than ${MAX_ROWS} rows`, index: -1 })
    return []
  }
  const rows: T[] = []
  value.forEach((row, index) => {
    const parsed = read(row)
    if (parsed === null) {
      issues.push({ collection, reason: 'unreadable row', index })
      return
    }
    rows.push(parsed)
  })
  return rows
}

export function decodeSnapshot(raw: string | null): StoreReadResult {
  if (raw === null || raw.length === 0) {
    return { snapshot: emptySnapshot(), reset: false, issues: [] }
  }
  if (raw.length > MAX_STORE_CHARS) {
    return {
      snapshot: emptySnapshot(),
      reset: true,
      issues: [
        { collection: 'document', reason: `larger than ${MAX_STORE_CHARS} characters`, index: -1 },
      ],
    }
  }

  let parsed: unknown
  try {
    parsed = JSON.parse(raw)
  } catch {
    return {
      snapshot: emptySnapshot(),
      reset: true,
      issues: [{ collection: 'document', reason: 'not valid JSON', index: -1 }],
    }
  }

  if (!isRecord(parsed)) {
    return {
      snapshot: emptySnapshot(),
      reset: true,
      issues: [{ collection: 'document', reason: 'not an object', index: -1 }],
    }
  }

  const version = whole(parsed['formatVersion'], 0)
  if (version > STORE_FORMAT_VERSION) {

    return {
      snapshot: emptySnapshot(),
      reset: true,
      issues: [
        {
          collection: 'document',
          reason: `written by a newer version (${version})`,
          index: -1,
        },
      ],
    }
  }

  const issues: StoreIssue[] = []
  const profiles = readCollection(parsed['profiles'], 'profiles', readProfile, issues)
  const sources = readCollection(parsed['sources'], 'sources', readSource, issues)
  const items = readCollection(parsed['items'], 'items', readItem, issues)

  const activeProfileId = text(parsed['activeProfileId'])
  const hiddenWorkIds = Array.isArray(parsed['hiddenWorkIds'])
    ? parsed['hiddenWorkIds'].filter((value): value is string => typeof value === 'string')
    : []

  return {
    snapshot: {
      formatVersion: STORE_FORMAT_VERSION,
      profiles,
      sources,
      items,

      activeProfileId:
        activeProfileId !== null && profiles.some((profile) => profile.id === activeProfileId)
          ? activeProfileId
          : null,
      hiddenWorkIds,
      settings: decodeSettings(parsed['settings']),
      lastScan: decodeScanReport(parsed['lastScan']),
    },
    reset: false,
    issues,
  }
}

export function encodeSnapshot(snapshot: StoreSnapshot): string {
  return JSON.stringify({
    formatVersion: STORE_FORMAT_VERSION,
    profiles: snapshot.profiles,
    sources: snapshot.sources,
    items: snapshot.items,
    activeProfileId: snapshot.activeProfileId,
    hiddenWorkIds: snapshot.hiddenWorkIds,
    settings: snapshot.settings,
    lastScan: snapshot.lastScan,
  })
}

export function withItemProgress(
  snapshot: StoreSnapshot,
  itemId: string,
  positionMs: number,
  durationMs: number,
  watched: boolean,
  updatedMs: number,
): StoreSnapshot {
  let changed = false
  const items = snapshot.items.map((item) => {
    if (item.id !== itemId) {
      return item
    }
    changed = true
    return {
      ...item,
      positionMs: Math.max(0, Math.trunc(positionMs)),
      durationMs: Math.max(0, Math.trunc(durationMs)),
      watched,
      updatedMs,
    }
  })
  return changed ? { ...snapshot, items } : snapshot
}

export function withSettings(snapshot: StoreSnapshot, settings: DeviceSettings): StoreSnapshot {
  return { ...snapshot, settings }
}

export function withScanReport(snapshot: StoreSnapshot, report: ScanReport): StoreSnapshot {
  return { ...snapshot, lastScan: report }
}

export function withHiddenWork(
  snapshot: StoreSnapshot,
  workId: string,
  hidden: boolean,
): StoreSnapshot {
  const has = snapshot.hiddenWorkIds.includes(workId)
  if (hidden === has) {
    return snapshot
  }
  return {
    ...snapshot,
    hiddenWorkIds: hidden
      ? [...snapshot.hiddenWorkIds, workId]
      : snapshot.hiddenWorkIds.filter((id) => id !== workId),
  }
}
