export type SubtitleMode = 'off' | 'auto'

export interface DeviceSettings {

  readonly subtitles: SubtitleMode

  readonly subtitleLanguage: string

  readonly audioLanguage: string
}

export const DEFAULT_SETTINGS: DeviceSettings = {
  subtitles: 'off',
  subtitleLanguage: '',
  audioLanguage: '',
}

export function normaliseLanguage(value: unknown): string {
  if (typeof value !== 'string') {
    return ''
  }
  const trimmed = value.trim().toLowerCase()
  return trimmed.length <= 24 ? trimmed : ''
}

export function decodeSettings(raw: unknown): DeviceSettings {
  if (typeof raw !== 'object' || raw === null || Array.isArray(raw)) {
    return DEFAULT_SETTINGS
  }
  const record = raw as Record<string, unknown>
  const mode = record['subtitles'] === 'auto' ? 'auto' : 'off'
  return {
    subtitles: mode,
    subtitleLanguage: normaliseLanguage(record['subtitleLanguage']),
    audioLanguage: normaliseLanguage(record['audioLanguage']),
  }
}

export function rememberChoice(
  settings: DeviceSettings,
  choice:
    | { readonly kind: 'audio'; readonly language: string | null }
    | { readonly kind: 'text'; readonly language: string | null }
    | { readonly kind: 'off' },
): DeviceSettings {
  switch (choice.kind) {
    case 'audio': {
      const language = normaliseLanguage(choice.language)
      return { ...settings, audioLanguage: language }
    }
    case 'text': {
      const language = normaliseLanguage(choice.language)
      return {
        ...settings,

        subtitles: 'auto',
        subtitleLanguage: language,
      }
    }
    case 'off':
      return { ...settings, subtitles: 'off' }
  }
}

export function forgetChoices(): DeviceSettings {
  return DEFAULT_SETTINGS
}

export function describeSubtitles(settings: DeviceSettings): string {
  if (settings.subtitles === 'off') {
    return 'Off — you can turn them on per file'
  }
  return settings.subtitleLanguage === ''
    ? 'On, in whichever language the file defaults to'
    : `On, preferring ${settings.subtitleLanguage}`
}

export function describeAudio(settings: DeviceSettings): string {
  return settings.audioLanguage === ''
    ? 'Whatever the file defaults to'
    : `Preferring ${settings.audioLanguage}`
}
