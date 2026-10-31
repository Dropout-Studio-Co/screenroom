import { buildTrackMenu, type TrackChoice, type TrackState } from './tracks.ts'

export const MENU_SEEK_MS = 10_000

export type PlayerMenuKind =
  | 'playpause'
  | 'seekBack'
  | 'seekForward'
  | 'next'
  | 'audio'
  | 'text'
  | 'off'
  | 'exit'

export interface PlayerMenuRow {
  readonly key: string
  readonly kind: PlayerMenuKind

  readonly track: TrackChoice | null
  readonly label: string
  readonly detail: string

  readonly selected: boolean
}

export interface PlayerMenuInput {
  readonly tracks: readonly TrackChoice[]
  readonly state: TrackState
  readonly paused: boolean

  readonly running?: boolean

  readonly finished: boolean

  readonly nextLabel: string
}

export function buildPlayerMenu(input: PlayerMenuInput): PlayerMenuRow[] {
  const rows: PlayerMenuRow[] = []
  if (input.running !== false) {
    rows.push(
      {
        key: 'control:playpause',
        kind: 'playpause',
        track: null,
        label: input.paused ? 'Play' : 'Pause',
        detail: input.paused ? 'Continues from where the picture stopped.' : 'Holds the picture where it is.',
        selected: false,
      },
      {
        key: 'control:seekBack',
        kind: 'seekBack',
        track: null,
        label: `Back ${MENU_SEEK_MS / 1000} seconds`,
        detail: 'Steps back through the file.',
        selected: false,
      },
      {
        key: 'control:seekForward',
        kind: 'seekForward',
        track: null,
        label: `Forward ${MENU_SEEK_MS / 1000} seconds`,
        detail: 'Steps forward through the file.',
        selected: false,
      },
    )
  }

  if (input.finished && input.nextLabel !== '') {
    rows.push({
      key: 'control:next',
      kind: 'next',
      track: null,
      label: `Play ${input.nextLabel}`,
      detail: 'The next episode in this series.',
      selected: false,
    })
  }

  for (const row of buildTrackMenu(input.tracks, input.state)) {
    rows.push({
      key: row.key,
      kind: row.kind === 'off' ? 'off' : row.kind,
      track: row.track,
      label: row.label,
      detail: row.detail,
      selected: row.selected,
    })
  }

  rows.push({
    key: 'control:exit',
    kind: 'exit',
    track: null,
    label: 'Back to the library',
    detail: 'Stops playback. Your position is kept.',
    selected: false,
  })
  return rows
}
