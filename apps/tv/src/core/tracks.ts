import type { LibraryItem } from './contract.ts'

export interface TrackChoice {

  readonly type: string

  readonly index: number

  readonly language: string | null

  readonly label: string
}

export const SUBTITLE_SIDECAR_EXTENSIONS: readonly string[] = ['smi', 'srt', 'sub', 'ttxt']

export function sidecarFileNames(stem: string): string[] {
  return SUBTITLE_SIDECAR_EXTENSIONS.map((extension) => `${stem}.${extension}`)
}

export function selectableTracks(tracks: readonly TrackChoice[]): {
  readonly audio: readonly TrackChoice[]
  readonly text: readonly TrackChoice[]
} {
  return {
    audio: tracks.filter((track) => track.type.toUpperCase() === 'AUDIO'),
    text: tracks.filter((track) => track.type.toUpperCase() === 'TEXT'),
  }
}

export interface TrackState {

  readonly audioIndex: number | null

  readonly textIndex: number | null

  readonly silent: boolean
}

export interface TrackMenuRow {

  readonly key: string

  readonly kind: 'audio' | 'text' | 'off'

  readonly track: TrackChoice | null
  readonly label: string
  readonly detail: string
  readonly selected: boolean
}

export function buildTrackMenu(
  tracks: readonly TrackChoice[],
  state: TrackState,
): TrackMenuRow[] {
  const grouped = selectableTracks(tracks)
  const rows: TrackMenuRow[] = []

  for (const track of grouped.audio) {
    const language = track.language?.trim() ?? ''
    rows.push({
      key: `audio:${String(track.index)}`,
      kind: 'audio',
      track,
      label: track.label,
      detail: language === '' ? 'No language declared by the decoder' : `Audio in ${language}`,
      selected: state.audioIndex !== null && state.audioIndex === track.index,
    })
  }

  if (grouped.text.length > 0) {
    rows.push({
      key: 'text:off',
      kind: 'off',
      track: null,
      label: 'Subtitles off',
      detail: 'Hides subtitles without changing the file.',

      selected: state.silent || state.textIndex === null,
    })
    for (const track of grouped.text) {
      const language = track.language?.trim() ?? ''
      rows.push({
        key: `text:${String(track.index)}`,
        kind: 'text',
        track,
        label: track.label,
        detail: language === '' ? 'No language declared by the decoder' : `Subtitles in ${language}`,
        selected: !state.silent && state.textIndex === track.index,
      })
    }
  }

  return rows
}

export interface TrackPreference {

  readonly subtitles: 'off' | 'auto'

  readonly subtitleLanguage: string

  readonly audioLanguage: string
}

function matches(track: TrackChoice, language: string): boolean {
  if (language === '') {
    return true
  }
  const declared = track.language?.trim().toLowerCase() ?? ''
  if (declared === '') {
    return false
  }

  return declared === language || declared.startsWith(language) || language.startsWith(declared)
}

export function chooseTracks(
  tracks: readonly TrackChoice[],
  preference: TrackPreference,
): TrackState & { readonly selectAudio: boolean; readonly selectText: boolean } {
  const grouped = selectableTracks(tracks)

  let audioIndex: number | null = null
  if (preference.audioLanguage !== '') {
    const audio = grouped.audio.find((track) => matches(track, preference.audioLanguage))
    audioIndex = audio?.index ?? null
  }

  if (preference.subtitles === 'off') {
    return { audioIndex, textIndex: null, silent: true, selectAudio: audioIndex !== null, selectText: false }
  }

  const text =
    preference.subtitleLanguage === ''
      ? grouped.text[0]
      : grouped.text.find((track) => matches(track, preference.subtitleLanguage))
  if (text === undefined) {

    return { audioIndex, textIndex: null, silent: true, selectAudio: audioIndex !== null, selectText: false }
  }
  return {
    audioIndex,
    textIndex: text.index,
    silent: false,
    selectAudio: audioIndex !== null,
    selectText: true,
  }
}

export function nextEpisode(
  items: readonly LibraryItem[],
  current: LibraryItem,
): LibraryItem | null {
  if (current.kind !== 'series' || current.season === null || current.episode === null) {
    return null
  }
  const candidates = items.filter(
    (item) =>
      item.id !== current.id &&
      item.workId === current.workId &&
      item.season === current.season &&
      item.episode === current.episode! + 1 &&
      item.available,
  )

  return candidates.length === 1 ? (candidates[0] ?? null) : null
}
