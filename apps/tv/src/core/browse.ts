import type { LibraryItem, MediaKind, ResumeEntry } from './contract.ts'

export const COLUMNS = 5

export const ROWS = 2

export type KindFilter = 'all' | 'movies' | 'series'

export function kindOf(filter: KindFilter): MediaKind | null {
  switch (filter) {
    case 'all':
      return null
    case 'movies':
      return 'movie'
    case 'series':
      return 'series'
  }
}

export interface BrowseFilter {
  readonly kind: KindFilter
  readonly text: string
}

export const NO_FILTER: BrowseFilter = { kind: 'all', text: '' }

function normalise(text: string): string {
  return text.trim().toLowerCase()
}

export function sortForDisplay(items: readonly LibraryItem[]): LibraryItem[] {
  const key = (item: LibraryItem): string => item.title.toLowerCase()
  return [...items].sort((left, right) => {
    const byTitle = key(left) < key(right) ? -1 : key(left) > key(right) ? 1 : 0
    if (byTitle !== 0) {
      return byTitle
    }
    const bySeason = (left.season ?? 0) - (right.season ?? 0)
    if (bySeason !== 0) {
      return bySeason
    }
    const byEpisode = (left.episode ?? 0) - (right.episode ?? 0)
    if (byEpisode !== 0) {
      return byEpisode
    }
    return left.relativePath < right.relativePath ? -1 : left.relativePath > right.relativePath ? 1 : 0
  })
}

export function itemsFor(
  items: readonly LibraryItem[],
  kind: KindFilter,
): LibraryItem[] {
  switch (kind) {
    case 'all':
      return sortForDisplay(items)
    case 'movies':
      return sortForDisplay(items.filter((item) => item.kind !== 'series'))
    case 'series':
      return sortForDisplay(items.filter((item) => item.kind === 'series'))
  }
}

export function filterItems(
  items: readonly LibraryItem[],
  filter: BrowseFilter,
): readonly LibraryItem[] {
  const wanted = kindOf(filter.kind)
  const needle = normalise(filter.text)
  return items.filter((item) => {
    if (wanted !== null && item.kind !== wanted) {
      return false
    }
    if (needle.length === 0) {
      return true
    }
    return (
      normalise(item.title).includes(needle) || normalise(item.relativePath).includes(needle)
    )
  })
}

export type EmptySituation =
  | 'first_run'
  | 'scanning'
  | 'source_unreadable'
  | 'filtered'
  | 'empty_library'

export interface EmptyState {
  readonly situation: EmptySituation
  readonly title: string
  readonly detail: string
}

export interface EmptyInput {

  readonly sourceCount: number

  readonly catalogCount: number

  readonly scanning: boolean

  readonly unavailableSources: number

  readonly filterText: string
}

export function emptyState(input: EmptyInput): EmptyState | null {
  if (input.unavailableSources > 0) {
    return {
      situation: 'source_unreadable',
      title: 'A drive is not readable right now.',
      detail: 'Plug it back in, then Scan for media in Settings. Nothing in your library was lost.',
    }
  }
  if (input.scanning) {
    return {
      situation: 'scanning',
      title: 'Looking for media…',
      detail: 'Files appear as they are found. You can keep browsing while this runs.',
    }
  }
  if (input.filterText.trim().length > 0) {
    return {
      situation: 'filtered',
      title: 'Nothing matches that search.',
      detail: 'Clear the search, or switch the filter back to All.',
    }
  }
  if (input.sourceCount === 0) {
    return {
      situation: 'first_run',
      title: 'No media yet.',

      detail: 'Plug a drive with films or episodes into the TV, then choose Media Sources in the menu and run Scan for media.',
    }
  }
  if (input.catalogCount === 0) {
    return {
      situation: 'empty_library',
      title: 'This drive has no media in it.',
      detail: 'Put films or episodes on the drive, then scan again from Media Sources.',
    }
  }
  return null
}

export function continueWatching(
  items: readonly LibraryItem[],
  limit: number,
): readonly ResumeEntry[] {
  const bounded = Math.max(1, Math.trunc(limit))
  return items
    .filter(
      (item) =>
        item.positionMs > 0 &&
        !item.watched &&
        !item.hidden &&
        item.available &&
        item.sourceId !== '',
    )
    .sort((left, right) => {
      if (right.updatedMs !== left.updatedMs) {
        return right.updatedMs - left.updatedMs
      }
      return left.id < right.id ? -1 : left.id > right.id ? 1 : 0
    })
    .slice(0, bounded)
    .map((item) => ({ item, positionMs: item.positionMs, durationMs: item.durationMs }))
}

export interface HomeShelf {
  readonly heading: string

  readonly note: string
  readonly items: readonly LibraryItem[]

  readonly fromResume: boolean
}

export function homeShelf(items: readonly LibraryItem[]): HomeShelf {
  const resume = continueWatching(items, Number.MAX_SAFE_INTEGER)
  const total = `${items.length} ${items.length === 1 ? 'file' : 'files'} in the library`
  if (resume.length > 0) {
    return {
      heading: 'Continue watching',
      note: `${resume.length} in progress · ${total}`,
      items: resume.map((entry) => entry.item),
      fromResume: true,
    }
  }
  return { heading: 'Your library', note: total, items: sortForDisplay(items), fromResume: false }
}

export function formatPosition(positionMs: number): string {
  const total = Math.max(0, Math.trunc(positionMs / 1000))
  const hours = Math.trunc(total / 3600)
  const minutes = Math.trunc((total % 3600) / 60)
  const seconds = total % 60
  const two = (value: number): string => value.toString().padStart(2, '0')
  return hours > 0 ? `${hours}:${two(minutes)}:${two(seconds)}` : `${minutes}:${two(seconds)}`
}

export function progressFraction(positionMs: number, durationMs: number): number {
  if (durationMs <= 0) {
    return 0
  }
  const fraction = positionMs / durationMs
  if (!Number.isFinite(fraction)) {
    return 0
  }
  return Math.min(1, Math.max(0, fraction))
}
