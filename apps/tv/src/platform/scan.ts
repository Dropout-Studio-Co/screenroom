import {
  classifyEntry,
  Issues,
  joinEntry,
  MAX_ENTRIES,
  mayDescend,
  type ScanIssue,
  type ScannedFile,
} from '../core/scan.ts'
import { isDirectoryPath, readDirectory, type ReadEntry } from './media-roots.ts'

export interface WalkOutcome {

  readonly files: readonly ScannedFile[]

  readonly issues: readonly ScanIssue[]

  readonly visited: number

  readonly complete: boolean

  readonly bounded: boolean

  readonly cancelled: boolean

  readonly rootProblem: string | null
}

export interface WalkOptions {

  readonly root: string

  readonly isCancelled?: () => boolean

  readonly onProgress?: (visited: number, files: number) => void
}

interface Pending {
  readonly path: string
  readonly depth: number
}

const YIELD_EVERY = 20

function yieldToHost(): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, 0))
}

export async function walkRoot(options: WalkOptions): Promise<WalkOutcome> {
  const issues = new Issues()
  const files: ScannedFile[] = []
  let visited = 0
  let bounded = false
  let cancelled = false

  const first = await readDirectory(options.root)
  if (!first.ok) {
    return {
      files,
      issues: issues.snapshot(),
      visited,
      complete: false,
      bounded: false,
      cancelled: false,
      rootProblem: first.reason,
    }
  }

  const queue: Pending[] = [{ path: options.root, depth: 0 }]

  let rootEntries: readonly ReadEntry[] | null = first.entries
  let foldersRead = 0

  const stop = (): WalkOutcome => ({
    files,
    issues: issues.snapshot(),
    visited,
    complete: false,
    bounded,
    cancelled,
    rootProblem: null,
  })

  while (queue.length > 0) {
    if (options.isCancelled?.() === true) {
      cancelled = true
      return stop()
    }

    const current = queue.shift()
    if (current === undefined) {
      break
    }
    const entries = rootEntries ?? (await readEntries(current.path, issues))
    rootEntries = null

    let reachedBound = false
    for (const entry of entries) {
      if (options.isCancelled?.() === true) {
        cancelled = true
        return stop()
      }
      visited += 1
      if (visited > MAX_ENTRIES) {
        bounded = true
        issues.push('', 'entry_limit')
        reachedBound = true
        break
      }

      const path = joinEntry(current.path, entry.name)
      if (path === current.path) {
        continue
      }
      const name = entry.name.split(/[/\\]/).pop() ?? entry.name

      const directory = entry.isDirectory ?? isDirectoryPath(path)
      if (directory === null) {

        issues.push(relativeTo(options.root, path), 'unreadable_entry')
        continue
      }
      switch (classifyEntry(name, directory)) {
        case 'media':
          files.push({ relativePath: relativeTo(options.root, path) })
          break
        case 'directory':
          if (mayDescend(current.depth)) {
            queue.push({ path, depth: current.depth + 1 })
          } else {
            issues.push(relativeTo(options.root, path), 'depth_limit')
          }
          break
        case 'downloading':
          issues.push(relativeTo(options.root, path), 'still_downloading')
          break
        case 'hidden_directory':
        case 'ignored':
          break
      }
    }
    if (reachedBound) {
      break
    }

    options.onProgress?.(visited, files.length)
    foldersRead += 1
    if (foldersRead % YIELD_EVERY === 0) {
      await yieldToHost()
    }
  }

  return {
    files,
    issues: issues.snapshot(),
    visited,

    complete: !bounded && issues.length === 0,
    bounded,
    cancelled: false,
    rootProblem: null,
  }
}

async function readEntries(path: string, issues: Issues): Promise<readonly ReadEntry[]> {
  const result = await readDirectory(path)
  if (result.ok) {
    return result.entries
  }
  issues.push('', looksLikePermission(result.reason) ? 'permission_lost' : 'directory_unavailable')
  return []
}

function looksLikePermission(reason: string): boolean {
  const lower = reason.toLowerCase()
  return lower.includes('permission') || lower.includes('denied') || lower.includes('not allowed')
}

export function relativeTo(root: string, path: string): string {
  const trimmedRoot = root.replace(/[/\\]+$/, '')
  if (trimmedRoot.length === 0) {
    return path.replace(/^[/\\]+/, '')
  }
  const prefix = `${trimmedRoot}/`
  return path.startsWith(prefix) ? path.slice(prefix.length) : path
}
