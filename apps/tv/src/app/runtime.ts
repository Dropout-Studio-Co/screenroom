import { createFocusMemory, type FocusMemory } from '../core/focus.ts'
import { createNavMemory, type NavMemory } from '../core/nav.ts'
import { LibraryService, nextId, type StoragePort } from '../core/storage.ts'
import type { LibraryItem, Source } from '../core/contract.ts'
import {
  MAX_ISSUES,
  planCatalog,
  reconcileSources,
  replaceSourceItems,
  withSourceAvailability,
  type ScanIssue,
  type ScanReport,
} from '../core/scan.ts'
import type { DeviceSettings } from '../core/settings.ts'
import { detectHost, readDeviceInfo, type DeviceInfo, type HostCapabilities } from '../platform/env.ts'
import { registerRemoteKeys, type KeyRegistration } from '../platform/keys.ts'
import {
  listMediaRoots,
  watchStorageChanges,
  type MediaRoot,
  type RootsResult,
  type StorageWatch,
} from '../platform/media-roots.ts'
import { walkRoot } from '../platform/scan.ts'
import { createDeviceStorage } from '../platform/persist.ts'
import { recordEvidence } from '../platform/evidence.ts'

let service: LibraryService | null = null

const focusMemory: FocusMemory = createFocusMemory()

const navigation: NavMemory = createNavMemory()

export interface SearchSession {
  readonly query: string
  readonly keyIndex: number
  readonly resultIndex: number
  readonly railIndex: number
  readonly zone: 'rail' | 'keys' | 'results'
}

let search: SearchSession = {
  query: '',
  keyIndex: 0,
  resultIndex: 0,
  railIndex: 3,
  zone: 'keys',
}

let walkProgress: { folders: number; files: number } | null = null

let roots: readonly MediaRoot[] = []

let hostCapabilities: HostCapabilities | null = null

let keyRegistration: KeyRegistration | null = null

let deviceInfo: DeviceInfo | null = null

let storageWatch: StorageWatch | null = null

let scanning = false

let scanCancelled = false

let idSequence = 0

export function host(): HostCapabilities {
  hostCapabilities ??= detectHost()
  return hostCapabilities
}

export function keys(): KeyRegistration {
  return keyRegistration ?? { registered: [], refused: [], supported: false }
}

export function device(): DeviceInfo {
  deviceInfo ??= { model: null, firmwareVersion: null, platformVersion: null, productVersion: null }
  return deviceInfo
}

export function library(): LibraryService {
  service ??= new LibraryService(storagePort())
  return service
}

function storagePort(): StoragePort {
  if (portOverride !== null) {
    return portOverride
  }
  return createDeviceStorage()
}

let portOverride: StoragePort | null = null

export function useStoragePort(port: StoragePort | null): void {
  portOverride = port
  service = null
}

export function focus(): FocusMemory {
  return focusMemory
}

export function nav(): NavMemory {
  return navigation
}

export function searchSession(): SearchSession {
  return search
}

export function rememberSearch(next: SearchSession): void {
  search = next
}

export function mediaRoots(): readonly MediaRoot[] {
  return roots
}

export function items(): readonly LibraryItem[] {
  return library().items()
}

export function sources(): readonly Source[] {
  return library().state().sources
}

export function profiles() {
  return library().profiles()
}

export function activeProfileId(): string | null {
  return library().activeProfileId()
}

export function settings(): DeviceSettings {
  return library().settings()
}

export function saveSettings(next: DeviceSettings): boolean {
  return library().saveSettings(next)
}

export function scanReport(): ScanReport {
  return library().lastScan()
}

export function scanInProgress(): boolean {
  return scanning
}

export function scanProgress(): { folders: number; files: number } | null {
  return walkProgress
}

export function prepare(): void {
  hostCapabilities = detectHost()
  deviceInfo = readDeviceInfo()
  keyRegistration = registerRemoteKeys()
  startStorageWatch()
  recordEvidence('boot', bootDetail())
}

function bootDetail(): string {
  const caps = host()
  const info = device()
  const keysAccepted = keys()
  const flag = (value: boolean): string => (value ? '1' : '0')
  return [
    `host=${caps.kind}`,
    `avplay=${flag(caps.avplay)}`,
    `filesystem=${flag(caps.filesystem)}`,
    `inputdevice=${flag(caps.inputdevice)}`,
    `storage=${flag(caps.durableStorage)}`,
    `keys=${keysAccepted.registered.length}/refused=${keysAccepted.refused.length}`,
    `model=${info.model ?? 'unknown'}`,
    `firmware=${info.firmwareVersion ?? 'unknown'}`,
    `platform=${info.platformVersion ?? 'unknown'}`,
    `product=${info.productVersion ?? 'unknown'}`,
    `ua=${caps.userAgent}`,
  ].join(' ')
}

function scannableMounts(current: readonly MediaRoot[]): { label: string; mounted: boolean }[] {
  return current
    .filter((root) => root.scannable)
    .map((root) => ({ label: root.label, mounted: root.mounted }))
}

export async function refreshMediaRoots(): Promise<RootsResult> {
  const result = await listMediaRoots()
  roots = result.ok ? result.roots : []
  return result
}

export function grantRoot(root: MediaRoot, resolvedPath: string): Source | null {
  const existing = library()
    .state()
    .sources.find((source) => source.label === root.label)
  if (existing !== undefined) {
    return existing
  }
  const sources: Source[] = [
    ...library().state().sources,
    {
      id: `source-${root.label}`,
      label: root.label,
      rootPath: resolvedPath,
      state: 'available',
      lastScanMs: 0,
    },
  ]
  library().replaceCatalog(library().items(), sources)
  return sources[sources.length - 1] ?? null
}

export function storageSummary(): string {
  if (roots.length === 0) {
    return 'The TV reported no removable storage.'
  }
  return roots
    .map((root) => {
      const state = root.state.toLowerCase()
      const chosen = root.scannable ? 'scanned' : 'not scanned'
      return `${root.label} (${root.type.toLowerCase()}, ${state}, ${chosen})`
    })
    .join(', ')
}

export function onTv(): boolean {
  return host().kind === 'tizen'
}

function describeRoots(result: RootsResult): string {
  if (!result.ok) {
    return `ok=0 reason=${result.reason}`
  }
  const parts = result.roots.map((root) => {
    const chosen = root.scannable ? 'scanned' : 'skipped'
    return `${root.label || '(no label)'}[${root.type},${root.state},${chosen}]`
  })
  return `ok=1 count=${result.roots.length} ${parts.join(' ') || 'none'}`
}

function describeReport(report: ScanReport): string {
  return [
    `sources=${report.scannedSources}`,
    `imported=${report.imported}`,
    `withdrawn=${report.withdrawn}`,
    `complete=${report.complete ? '1' : '0'}`,
    `bounded=${report.bounded ? '1' : '0'}`,
    `issues=${report.issues.length}`,
    `reason=${report.reason ?? 'none'}`,
  ].join(' ')
}

export function cancelScan(): void {
  scanCancelled = true
}

export async function scanDrives(): Promise<ScanReport> {
  if (scanning) {
    return library().lastScan()
  }
  scanning = true
  scanCancelled = false
  walkProgress = null
  const startedMs = Date.now()
  try {
    const discovered = await refreshMediaRoots()
    recordEvidence('storage', describeRoots(discovered))
    if (!discovered.ok) {
      const report: ScanReport = {
        scannedSources: 0,
        imported: 0,
        withdrawn: 0,
        issues: [],
        complete: false,
        bounded: false,
        finishedMs: startedMs,
        reason: discovered.reason,
      }
      library().recordScan(report)
      recordEvidence('scan', describeReport(report))
      return report
    }

    const reported = discovered.roots
    const sources = reconcileSources(
      library().state().sources,
      scannableMounts(reported),

      (label) => label,
    )

    let catalog: LibraryItem[] = [...library().items()]
    const issues: ScanIssue[] = []
    const walked = new Set<string>()
    let imported = 0
    let withdrawn = 0
    let scannedSources = 0

    let foldersWalked = 0
    let filesWalked = 0
    let complete = true
    let bounded = false
    let reason: string | null = null

    for (const source of sources) {
      if (scanCancelled) {
        complete = false
        break
      }
      const mount = reported.find((root) => root.label === source.label)
      if (source.state !== 'available' || mount === undefined || !mount.mounted) {

        catalog = withSourceAvailability(catalog, source.id, false)
        continue
      }

      const walk = await walkRoot({
        root: source.rootPath,
        isCancelled: () => scanCancelled,
        onProgress: (visited: number, files: number) => {
          walkProgress = { folders: foldersWalked + visited, files: filesWalked + files }
        },
      })
      foldersWalked += walk.visited
      filesWalked += walk.files.length
      if (walk.rootProblem !== null) {

        catalog = withSourceAvailability(catalog, source.id, false)
        complete = false
        reason = walk.rootProblem
        issues.push(...walk.issues)
        continue
      }

      const plan = planCatalog({
        sourceId: source.id,
        found: walk.files,
        existing: catalog,
        complete: walk.complete,
        nowMs: startedMs,
        newId: (prefix) => nextId(prefix, idSequence++, Date.now()),
      })
      catalog = replaceSourceItems(catalog, source.id, plan.items)
      imported += plan.imported
      withdrawn += plan.withdrawn
      issues.push(...plan.issues)
      scannedSources += 1
      walked.add(source.id)
      complete = complete && walk.complete
      bounded = bounded || walk.bounded
      if (scanCancelled) {
        complete = false
        break
      }
    }

    const finishedMs = Date.now()
    const report: ScanReport = {
      scannedSources,
      imported,
      withdrawn,
      issues: issues.slice(0, MAX_ISSUES),
      complete,
      bounded,
      finishedMs,
      reason,
    }
    const stamped: Source[] = sources.map((source) =>
      walked.has(source.id)
        ? { ...source, state: 'available', lastScanMs: finishedMs }
        : source,
    )
    library().applyScan({ items: catalog, sources: stamped, report })
    recordEvidence('scan', describeReport(report))

    recordEvidence(
      'library',
      [
        `items=${catalog.length}`,
        `available=${catalog.filter((item) => item.available).length}`,
        `sources=${stamped.length}`,
        `write=${library().writeFailure() === null ? 'accepted' : 'refused'}`,
      ].join(' '),
    )
    return report
  } finally {
    scanning = false
    scanCancelled = false
  }
}

export function startStorageWatch(): StorageWatch {
  stopStorageWatch()
  storageWatch = watchStorageChanges(() => {
    void onStorageChanged()
  })
  return storageWatch
}

export function stopStorageWatch(): void {
  if (storageWatch !== null && storageWatch.watching) {
    storageWatch.stop()
  }
  storageWatch = null
}

async function onStorageChanged(): Promise<void> {
  const discovered = await refreshMediaRoots()
  if (!discovered.ok) {
    return
  }
  const sources = reconcileSources(
    library().state().sources,
    scannableMounts(discovered.roots),
    (label) => label,
  )
  let catalog: LibraryItem[] = [...library().items()]
  for (const source of sources) {

    if (source.state === 'available') {
      continue
    }
    catalog = withSourceAvailability(catalog, source.id, false)
  }
  library().applyScan({ items: catalog, sources, report: library().lastScan() })
}
