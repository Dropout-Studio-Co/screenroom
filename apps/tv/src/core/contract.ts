export type WorkId = string

export type EditionId = string

export type ItemId = string

export type SourceId = string

export type ProfileId = string

export type CoreErrorCode =
  | 'duplicate_source'
  | 'invalid_root'
  | 'source_missing'
  | 'source_disconnected'
  | 'source_removed'
  | 'permission_lost'
  | 'unsafe_path'
  | 'still_downloading'
  | 'cancelled'
  | 'database_error'
  | 'io_error'
  | 'invalid_stored_json'

export interface CoreFailure {
  readonly code: CoreErrorCode
  readonly message: string
}

export function isAvailabilityFailure(code: CoreErrorCode): boolean {
  return (
    code === 'source_missing' ||
    code === 'source_disconnected' ||
    code === 'permission_lost' ||
    code === 'io_error'
  )
}

export type SourceState =
  | 'available'
  | 'disconnected'
  | 'permissionLost'
  | 'missing'
  | 'removed'

export interface Source {
  readonly id: SourceId
  readonly label: string

  readonly rootPath: string
  readonly state: SourceState
  readonly lastScanMs: number
}

export type MediaKind = 'movie' | 'series' | 'other'

export interface Profile {
  readonly id: ProfileId
  readonly name: string
}

export interface LibraryItem {
  readonly id: ItemId
  readonly workId: WorkId
  readonly title: string
  readonly kind: MediaKind
  readonly season: number | null
  readonly episode: number | null
  readonly year: number | null

  readonly relativePath: string
  readonly sourceId: SourceId

  readonly available: boolean
  readonly favorite: boolean
  readonly watched: boolean
  readonly hidden: boolean

  readonly positionMs: number

  readonly durationMs: number

  readonly updatedMs: number
}

export interface PlaybackSnapshot {
  readonly sessionId: string
  readonly generation: number
  readonly sequence: number
  readonly positionMs: number
  readonly durationMs: number

  readonly ended: boolean

  readonly paused: boolean
}

export interface ResumeEntry {
  readonly item: LibraryItem
  readonly positionMs: number
  readonly durationMs: number
}
