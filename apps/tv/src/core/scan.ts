import type { LibraryItem, Source, SourceState } from './contract.ts'
import { checkLocator } from './locator.ts'
import { extensionOf, folderHintOf, identityKeyOf, parseFilename } from './name.ts'

export const MEDIA_EXTENSIONS: readonly string[] = [
  'mkv',
  'mp4',
  'm4v',
  'avi',
  'mov',
  'webm',
  'mpeg',
  'mpg',
  'ts',
  'm2ts',
  'wmv',
  'flv',
  'ogv',
  '3gp',
]

export const DOWNLOAD_SUFFIXES: readonly string[] = [
  '.part',
  '.crdownload',
  '.download',
  '.aria2',
  '.!qb',
]

export const MAX_ENTRIES = 25_000

export const MAX_DEPTH = 16

export const MAX_ISSUES = 200

export const MAX_LOCATOR_CHARS = 1024

export interface ScannedFile {
  readonly relativePath: string
}

export type EntryVerdict =

  | 'directory'

  | 'hidden_directory'

  | 'media'

  | 'ignored'

  | 'downloading'

export function isDownloadingName(name: string): boolean {
  const lower = name.toLowerCase()
  return DOWNLOAD_SUFFIXES.some((suffix) => lower.endsWith(suffix))
}

export function isMediaName(name: string): boolean {
  const extension = extensionOf(name)
  return extension !== null && MEDIA_EXTENSIONS.includes(extension)
}

export function classifyEntry(name: string, isDirectory: boolean): EntryVerdict {
  if (isDirectory) {
    return name.startsWith('.') ? 'hidden_directory' : 'directory'
  }
  if (isDownloadingName(name)) {
    return 'downloading'
  }
  return isMediaName(name) ? 'media' : 'ignored'
}

export interface ReportedStorage {
  readonly label: string
  readonly type: string
  readonly mounted: boolean
}

export function scannableStorage(storage: ReportedStorage): boolean {
  if (!storage.mounted) {
    return false
  }
  const type = storage.type.toUpperCase()
  return type === 'EXTERNAL' || storage.label.toLowerCase().startsWith('removable')
}

export function mayDescend(depth: number): boolean {
  return depth < MAX_DEPTH
}

export function withinEntryLimit(visited: number): boolean {
  return visited <= MAX_ENTRIES
}

export function joinEntry(directoryPath: string, entry: string): string {
  const trimmed = entry.replace(/^[/\\]+/, '')
  if (trimmed.length === 0) {
    return directoryPath
  }
  const prefix = directoryPath.length === 0 ? '' : `${directoryPath.replace(/[/\\]+$/, '')}/`
  if (prefix.length > 0 && trimmed.startsWith(prefix)) {
    return trimmed
  }
  return `${prefix}${trimmed}`
}

export function baseName(path: string): string {
  const separator = Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\'))
  return separator < 0 ? path : path.slice(separator + 1)
}

export interface ScanIssue {

  readonly relativePath: string

  readonly code: string
}

export class Issues {
  private readonly entries: ScanIssue[] = []

  push(relativePath: string, code: string): void {
    if (this.entries.length < MAX_ISSUES) {
      this.entries.push({ relativePath, code })
      return
    }
    if (this.entries.length === MAX_ISSUES) {
      this.entries.push({ relativePath: '', code: 'additional_errors_omitted' })
    }
  }

  get length(): number {
    return this.entries.length
  }

  snapshot(): ScanIssue[] {
    return [...this.entries]
  }
}

export interface CatalogPlan {

  readonly items: readonly LibraryItem[]

  readonly imported: number

  readonly withdrawn: number

  readonly issues: readonly ScanIssue[]
}

export interface CatalogPlanInput {
  readonly sourceId: string

  readonly found: readonly ScannedFile[]

  readonly existing: readonly LibraryItem[]

  readonly complete: boolean
  readonly nowMs: number

  readonly newId: (prefix: string) => string
}

function comparePaths(left: string, right: string): number {
  return left < right ? -1 : left > right ? 1 : 0
}

export function planCatalog(input: CatalogPlanInput): CatalogPlan {
  const issues = new Issues()
  const existingByPath = new Map<string, LibraryItem>()
  for (const item of input.existing) {
    if (item.sourceId === input.sourceId) {
      existingByPath.set(item.relativePath, item)
    }
  }

  const workByKey = new Map<string, string>()
  for (const item of input.existing) {
    if (item.sourceId !== input.sourceId) {
      continue
    }
    const parsed = parseFilename(baseName(item.relativePath), folderHintOf(item.relativePath))

    workByKey.set(identityKeyOf(parsed, item.sourceId, item.id), item.workId)
  }

  const seen = new Set<string>()
  const items: LibraryItem[] = []
  let imported = 0

  for (const file of [...input.found].sort((left, right) =>
    comparePaths(left.relativePath, right.relativePath),
  )) {
    const relativePath = file.relativePath
    if (seen.has(relativePath)) {

      continue
    }
    seen.add(relativePath)

    const locator = checkLocator(relativePath)
    if (!locator.safe) {
      issues.push(relativePath, 'unsafe_path')
      continue
    }
    if (relativePath.length > MAX_LOCATOR_CHARS) {
      issues.push(relativePath, 'invalid_name')
      continue
    }
    const name = baseName(relativePath)
    if (isDownloadingName(name)) {
      issues.push(relativePath, 'still_downloading')
      continue
    }
    if (!isMediaName(name)) {
      issues.push(relativePath, 'unsupported_container')
      continue
    }

    const parsed = parseFilename(name, folderHintOf(relativePath))
    const before = existingByPath.get(relativePath)
    if (before !== undefined) {
      imported += 1
      items.push({
        ...before,
        title: parsed.title,
        kind: parsed.kind,
        season: parsed.season,
        episode: parsed.episode,
        year: parsed.year,
        available: true,
      })
      continue
    }

    const itemId = input.newId('item')
    const key = identityKeyOf(parsed, input.sourceId, itemId)
    const workId = workByKey.get(key) ?? input.newId('work')
    workByKey.set(key, workId)
    imported += 1
    items.push({
      id: itemId,
      workId,
      title: parsed.title,
      kind: parsed.kind,
      season: parsed.season,
      episode: parsed.episode,
      year: parsed.year,
      relativePath,
      sourceId: input.sourceId,
      available: true,
      favorite: false,
      watched: false,
      hidden: false,
      positionMs: 0,
      durationMs: 0,
      updatedMs: 0,
    })
  }

  let withdrawn = 0
  if (input.complete) {

    for (const item of input.existing) {
      if (item.sourceId !== input.sourceId || seen.has(item.relativePath)) {
        continue
      }
      if (item.available) {
        withdrawn += 1
      }
      items.push({ ...item, available: false })
    }
    items.sort((left, right) => comparePaths(left.relativePath, right.relativePath))
  } else {
    for (const item of input.existing) {
      if (item.sourceId !== input.sourceId || seen.has(item.relativePath)) {
        continue
      }
      items.push(item)
    }
    items.sort((left, right) => comparePaths(left.relativePath, right.relativePath))
  }

  return { items, imported, withdrawn, issues: issues.snapshot() }
}

export function replaceSourceItems(
  all: readonly LibraryItem[],
  sourceId: string,
  planned: readonly LibraryItem[],
): LibraryItem[] {
  const others = all.filter((item) => item.sourceId !== sourceId)
  return [...others, ...planned].sort((left, right) =>
    comparePaths(`${left.sourceId}/${left.relativePath}`, `${right.sourceId}/${right.relativePath}`),
  )
}

export function withSourceAvailability(
  all: readonly LibraryItem[],
  sourceId: string,
  available: boolean,
): LibraryItem[] {
  let changed = false
  const items = all.map((item) => {
    if (item.sourceId !== sourceId || item.available === available) {
      return item
    }
    changed = true
    return { ...item, available }
  })
  return changed ? items : [...all]
}

export function sourceStateOf(mounted: boolean, seen: boolean): SourceState {
  if (!seen) {
    return 'missing'
  }
  return mounted ? 'available' : 'disconnected'
}

export function reconcileSources(
  sources: readonly Source[],
  reported: readonly { readonly label: string; readonly mounted: boolean }[],
  resolvedPathOf: (label: string) => string,
): Source[] {
  const byLabel = new Map(reported.map((storage) => [storage.label, storage]))
  const reconciled = sources.map((source) => {
    const storage = byLabel.get(source.label)
    if (storage === undefined) {
      return { ...source, state: sourceStateOf(false, false) }
    }
    return { ...source, state: sourceStateOf(storage.mounted, true) }
  })
  const known = new Set(sources.map((source) => source.label))
  for (const storage of reported) {
    if (known.has(storage.label) || !storage.mounted) {
      continue
    }
    reconciled.push({
      id: `source-${storage.label}`,
      label: storage.label,
      rootPath: resolvedPathOf(storage.label),
      state: 'available',
      lastScanMs: 0,
    })
  }
  return reconciled
}

export interface ScanReport {

  readonly scannedSources: number

  readonly imported: number

  readonly withdrawn: number

  readonly issues: readonly ScanIssue[]

  readonly complete: boolean

  readonly bounded: boolean

  readonly finishedMs: number

  readonly reason: string | null
}

export const NEVER_SCANNED: ScanReport = {
  scannedSources: 0,
  imported: 0,
  withdrawn: 0,
  issues: [],
  complete: false,
  bounded: false,
  finishedMs: 0,
  reason: 'No scan has run yet.',
}

export function decodeScanReport(raw: unknown): ScanReport {
  if (typeof raw !== 'object' || raw === null || Array.isArray(raw)) {
    return NEVER_SCANNED
  }
  const record = raw as Record<string, unknown>
  const whole = (value: unknown): number =>
    typeof value === 'number' && Number.isSafeInteger(value) && value >= 0 ? value : 0
  const issues: ScanIssue[] = []
  if (Array.isArray(record['issues'])) {
    for (const entry of record['issues'].slice(0, MAX_ISSUES + 1)) {
      if (typeof entry !== 'object' || entry === null) {
        continue
      }
      const row = entry as Record<string, unknown>
      const relativePath = row['relativePath']
      const code = row['code']
      if (typeof relativePath !== 'string' || typeof code !== 'string') {
        continue
      }
      issues.push({ relativePath, code })
    }
  }
  const reason = record['reason']
  return {
    scannedSources: whole(record['scannedSources']),
    imported: whole(record['imported']),
    withdrawn: whole(record['withdrawn']),
    issues,
    complete: record['complete'] === true,
    bounded: record['bounded'] === true,
    finishedMs: whole(record['finishedMs']),
    reason: typeof reason === 'string' ? reason : null,
  }
}

export function describeScan(report: ScanReport): string {
  if (report.reason !== null) {
    return report.reason
  }
  const files = `${report.imported} ${report.imported === 1 ? 'file' : 'files'}`
  const drives = `${report.scannedSources} ${report.scannedSources === 1 ? 'drive' : 'drives'}`
  if (!report.complete) {
    return `${files} found on ${drives}; the walk did not finish, so nothing was marked unreadable.`
  }
  if (report.issues.length > 0) {
    return `${files} found on ${drives}; ${report.issues.length} ${
      report.issues.length === 1 ? 'entry' : 'entries'
    } skipped.`
  }
  return `${files} found on ${drives}, with nothing skipped.`
}
