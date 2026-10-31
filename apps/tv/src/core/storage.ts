import type { LibraryItem, PlaybackSnapshot, Profile, ProfileId, Source } from './contract.ts'
import type { ScanReport } from './scan.ts'
import type { DeviceSettings } from './settings.ts'
import { acceptProgress, isFinished, isWorthSaving, type LiveSession } from './progress.ts'
import {
  decodeSnapshot,
  emptySnapshot,
  encodeSnapshot,
  withHiddenWork,
  withItemProgress,
  withScanReport,
  withSettings,
  type StoreIssue,
  type StoreSnapshot,
} from './store.ts'

export interface StoragePort {

  read(): string | null

  write(document: string): boolean

  clear(): void

  describe(): string
}

export function createMemoryStorage(initial: string | null = null): StoragePort & {
  readonly document: () => string | null
} {
  let stored = initial
  return {
    read: () => stored,
    write: (document: string) => {
      stored = document
      return true
    },
    clear: () => {
      stored = null
    },
    describe: () => 'this tab only',
    document: () => stored,
  }
}

export interface LoadReport {

  readonly lostStoredState: boolean
  readonly issues: readonly StoreIssue[]

  readonly location: string
}

export interface WriteFailure {
  readonly reason: string
}

export class LibraryService {
  private snapshot: StoreSnapshot
  private readonly port: StoragePort
  private readonly now: () => number
  private lastLoad: LoadReport
  private writeFailed: WriteFailure | null = null
  private lastWritten: string | null = null

  constructor(port: StoragePort, now: () => number = () => Date.now()) {
    this.port = port
    this.now = now
    const result = decodeSnapshot(port.read())
    this.snapshot = result.snapshot
    this.lastLoad = {
      lostStoredState: result.reset,
      issues: result.issues,
      location: port.describe(),
    }
    this.lastWritten = result.reset ? null : encodeSnapshot(this.snapshot)
  }

  loadReport(): LoadReport {
    return this.lastLoad
  }

  writeFailure(): WriteFailure | null {
    return this.writeFailed
  }

  state(): StoreSnapshot {
    return this.snapshot
  }

  items(): readonly LibraryItem[] {
    return this.snapshot.items
  }

  profiles(): readonly Profile[] {
    return this.snapshot.profiles
  }

  activeProfileId(): ProfileId | null {
    return this.snapshot.activeProfileId
  }

  source(id: string) {
    return this.snapshot.sources.find((source) => source.id === id) ?? null
  }

  isHidden(workId: string): boolean {
    return this.snapshot.hiddenWorkIds.includes(workId)
  }

  setActiveProfile(profileId: ProfileId | null): boolean {
    if (profileId !== null && !this.snapshot.profiles.some((p) => p.id === profileId)) {
      return false
    }
    this.snapshot = { ...this.snapshot, activeProfileId: profileId }
    return this.persist()
  }

  addProfile(name: string): ProfileId | null {
    const trimmed = name.trim()
    if (trimmed.length === 0) {
      return null
    }
    const id = nextId('profile', this.snapshot.profiles.length, this.now())
    const profile: Profile = { id, name: trimmed }
    this.snapshot = {
      ...this.snapshot,
      profiles: [...this.snapshot.profiles, profile],

      activeProfileId: this.snapshot.activeProfileId ?? id,
    }
    this.persist()
    return id
  }

  recordProgress(
    live: LiveSession,
    incoming: PlaybackSnapshot,
    workId: string | null,
  ): boolean {
    const itemId = this.itemIdForSnapshot(incoming)
    if (itemId === null) {
      return false
    }
    const verdict = acceptProgress(live, this.acceptedFor(itemId), incoming)
    if (!verdict.accept) {
      return false
    }
    this.accepted.set(itemId, verdict.snapshot)

    const finished = isFinished(incoming.positionMs, incoming.durationMs, incoming.ended)
    const position = finished ? 0 : incoming.positionMs
    if (!finished && !isWorthSaving(position, incoming.durationMs)) {

      return false
    }
    this.snapshot = withItemProgress(
      this.snapshot,
      itemId,
      position,
      incoming.durationMs,
      finished,
      this.now(),
    )
    if (finished && workId !== null) {

      this.snapshot = withHiddenWork(this.snapshot, workId, false)
    }
    return this.persist()
  }

  setResumeHidden(workId: string, hidden: boolean): boolean {
    this.snapshot = withHiddenWork(this.snapshot, workId, hidden)
    return this.persist()
  }

  setWatched(itemId: string, watched: boolean): boolean {
    const item = this.snapshot.items.find((candidate) => candidate.id === itemId)
    if (item === undefined) {
      return false
    }
    this.snapshot = withItemProgress(
      this.snapshot,
      itemId,
      watched ? 0 : item.positionMs,
      item.durationMs,
      watched,
      this.now(),
    )
    return this.persist()
  }

  replaceCatalog(
    items: readonly LibraryItem[],
    sources: StoreSnapshot['sources'],
  ): boolean {
    const previous = new Map(this.snapshot.items.map((item) => [item.id, item]))
    const merged = items.map((item) => {
      const before = previous.get(item.id)
      if (before === undefined) {
        return item
      }
      return {
        ...item,

        positionMs: before.positionMs,
        durationMs: item.durationMs > 0 ? item.durationMs : before.durationMs,
        watched: before.watched,
        updatedMs: before.updatedMs,
      }
    })
    this.snapshot = { ...this.snapshot, items: merged, sources }
    return this.persist()
  }

  applyScan(input: {
    readonly items: readonly LibraryItem[]
    readonly sources: readonly Source[]
    readonly report: ScanReport
  }): boolean {
    const previous = new Map(this.snapshot.items.map((item) => [item.id, item]))
    const merged = input.items.map((item) => {
      const before = previous.get(item.id)
      if (before === undefined) {
        return item
      }
      return {
        ...item,
        positionMs: before.positionMs,
        durationMs: item.durationMs > 0 ? item.durationMs : before.durationMs,
        watched: before.watched,
        updatedMs: before.updatedMs,
      }
    })
    this.snapshot = {
      ...this.snapshot,
      items: merged,
      sources: input.sources,
      lastScan: input.report,
    }
    return this.persist()
  }

  settings(): DeviceSettings {
    return this.snapshot.settings
  }

  saveSettings(settings: DeviceSettings): boolean {
    this.snapshot = withSettings(this.snapshot, settings)
    return this.persist()
  }

  lastScan(): ScanReport {
    return this.snapshot.lastScan
  }

  recordScan(report: ScanReport): boolean {
    this.snapshot = withScanReport(this.snapshot, report)
    return this.persist()
  }

  private persist(): boolean {
    const document = encodeSnapshot(this.snapshot)
    if (document === this.lastWritten) {
      return false
    }
    const accepted = this.port.write(document)
    if (accepted) {
      this.lastWritten = document
      this.writeFailed = null
      return true
    }
    this.writeFailed = {
      reason: 'The device refused the write. Storage may be full or the browser blocked it.',
    }
    return false
  }

  private readonly accepted = new Map<string, PlaybackSnapshot>()

  private acceptedFor(itemId: string): PlaybackSnapshot | null {
    return this.accepted.get(itemId) ?? null
  }

  private itemIdForSnapshot(snapshot: PlaybackSnapshot): string | null {
    return this.sessionItems.get(snapshot.sessionId) ?? null
  }

  private readonly sessionItems = new Map<string, string>()

  beginSession(sessionId: string, itemId: string): void {
    this.sessionItems.set(sessionId, itemId)
  }

  endSession(sessionId: string): void {
    this.sessionItems.delete(sessionId)
  }
}

export function nextId(prefix: string, sequence: number, timestampMs: number): string {
  const time = Math.max(0, Math.trunc(timestampMs)).toString(36)
  return `${prefix}-${time}-${sequence.toString(36)}`
}

export { emptySnapshot }
export type { StoreSnapshot }
