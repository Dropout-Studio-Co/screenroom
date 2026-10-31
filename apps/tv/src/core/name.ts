import type { MediaKind } from './contract.ts'

const EPISODIC_SE = /\bS(\d{1,3})[ ._-]*E(\d{1,4})(?:[ ._-]*(?:E|-E?)(\d{1,4}))?/i

const EPISODIC_AX = /\b(\d{1,3})x(\d{1,4})(?:-(\d{1,4}))?/i

const YEAR = /\b(19\d{2}|20\d{2})\b/

const QUALITY =
  /\b(?:2160p|1080p|720p|480p|bluray|brrip|webrip|web-dl|hdtv|dvdrip|x26[45]|h26[45]|hevc|av1)\b/i

const BRACKETS = /[[\]()]+/

const SEPARATORS = /[._]/g

const SEASON_FOLDER = /^season\s*\d+$/i

const NUMERIC_TITLE = /^\d+$/

export interface ParsedName {

  readonly title: string

  readonly kind: MediaKind

  readonly year: number | null

  readonly season: number | null

  readonly episode: number | null

  readonly episodeEnd: number | null

  readonly confidence: number
}

interface Episodic {

  readonly start: number
  readonly season: number
  readonly episode: number
  readonly end: number | null
}

export function fileStem(filename: string): string {
  const lastSeparator = Math.max(filename.lastIndexOf('/'), filename.lastIndexOf('\\'))
  const base = filename.slice(lastSeparator + 1)
  const dot = base.lastIndexOf('.')
  if (dot <= 0) {
    return base
  }
  return base.slice(0, dot)
}

function lastFolderComponent(folder: string): string {
  const parts = folder.split(/[/\\]/).filter((part) => part.length > 0)
  return parts[parts.length - 1] ?? ''
}

export function extensionOf(name: string): string | null {
  const lastSeparator = Math.max(name.lastIndexOf('/'), name.lastIndexOf('\\'))
  const base = name.slice(lastSeparator + 1)
  const dot = base.lastIndexOf('.')
  if (dot <= 0 || dot === base.length - 1) {
    return null
  }
  return base.slice(dot + 1).toLowerCase()
}

function numberOrNull(value: string | undefined): number | null {
  if (value === undefined) {
    return null
  }
  const parsed = Number.parseInt(value, 10)
  return Number.isSafeInteger(parsed) ? parsed : null
}

function clean(value: string): string {
  return value
    .replace(SEPARATORS, ' ')
    .replace(BRACKETS, ' ')
    .split(/\s+/)
    .filter((part) => part.length > 0)
    .join(' ')
    .replace(/[\s-]+$/, '')
}

function findEpisodic(name: string): Episodic | null {
  const se = EPISODIC_SE.exec(name)
  const ax = se === null ? EPISODIC_AX.exec(name) : null
  const match = se ?? ax
  if (match === null || match.index === undefined) {
    return null
  }
  const season = numberOrNull(match[1])
  const episode = numberOrNull(match[2])
  if (season === null || episode === null) {
    return null
  }
  return { start: match.index, season, episode, end: numberOrNull(match[3]) }
}

export function parseFilename(filename: string, folder?: string | null): ParsedName {
  const stem = fileStem(filename)
  let name = stem.replace(/_/g, '.')

  const episodic = findEpisodic(name)
  const yearMatch = YEAR.exec(name)
  const usableYear =
    yearMatch === null
      ? null
      : episodic !== null && yearMatch.index >= episodic.start
        ? null
        : yearMatch
  const yearValue = usableYear === null ? null : numberOrNull(usableYear[0])
  const year =
    yearValue !== null && yearValue >= 1800 && yearValue <= 2200 ? yearValue : null

  const cut =
    usableYear?.index ??
    episodic?.start ??
    (QUALITY.exec(name)?.index ?? null)
  if (cut !== null) {
    name = name.slice(0, cut)
  }

  let cleaned = clean(name)
  if (cleaned.length === 0 && folder !== undefined && folder !== null) {
    const label = clean(lastFolderComponent(folder))

    cleaned = SEASON_FOLDER.test(label) ? '' : label
  }

  const unknown = cleaned.length === 0 || NUMERIC_TITLE.test(cleaned)
  const season = episodic?.season ?? null
  const episode = episodic?.episode ?? null
  const episodeEnd =
    episodic?.end !== null &&
    episodic?.end !== undefined &&
    episode !== null &&
    episodic.end >= episode &&
    episodic.end - episode < 100
      ? episodic.end
      : episode

  const kind: MediaKind = episodic !== null ? 'series' : unknown ? 'other' : 'movie'
  const confidence = unknown ? 0.1 : episodic !== null ? 0.85 : year !== null ? 0.75 : 0.4

  return {
    title: unknown ? stem : cleaned,
    kind,
    year,
    season,
    episode,
    episodeEnd,
    confidence,
  }
}

export function identityKeyOf(
  parsed: ParsedName,
  sourceId: string,
  unmatchedSuffix: string,
): string {
  const confidentSeries =
    parsed.kind === 'series' &&
    parsed.confidence >= 0.8 &&
    parsed.season !== null &&
    parsed.episode !== null
  if (confidentSeries) {
    return `series:${sourceId}:${parsed.title.toLowerCase()}:${parsed.year ?? ''}`
  }
  const confidentMovie =
    parsed.kind === 'movie' && parsed.year !== null && parsed.confidence >= 0.75
  if (confidentMovie) {
    return `movie:${parsed.title.toLowerCase()}:${parsed.year}`
  }
  return `unmatched:${unmatchedSuffix}`
}

export function folderHintOf(relativePath: string): string {
  const separator = Math.max(relativePath.lastIndexOf('/'), relativePath.lastIndexOf('\\'))
  if (separator < 0) {
    return ''
  }
  const folder = relativePath.slice(0, separator)
  const last = lastFolderComponent(folder)
  if (!SEASON_FOLDER.test(last)) {
    return folder
  }
  const parentSeparator = Math.max(folder.lastIndexOf('/'), folder.lastIndexOf('\\'))
  return parentSeparator < 0 ? '' : folder.slice(0, parentSeparator)
}
