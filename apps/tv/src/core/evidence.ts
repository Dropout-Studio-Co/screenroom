export interface EvidenceEntry {

  readonly atMs: number

  readonly tag: string

  readonly detail: string
}

export const MAX_ENTRIES = 60

export const MAX_DETAIL = 300

export const MAX_TAG = 24

export function sanitise(text: string, max: number = MAX_DETAIL): string {
  const flat = text
    .replace(/[\t\n\r]+/g, ' ')

    .split('')
    .filter((character) => {
      const code = character.charCodeAt(0)
      return code >= 0x20 && code !== 0x7f
    })
    .join('')
    .trim()
  if (flat.length <= max) {
    return flat
  }
  return `${flat.slice(0, Math.max(0, max - 1))}…`
}

export function appendEvidence(
  log: readonly EvidenceEntry[],
  entry: EvidenceEntry,
  limit: number = MAX_ENTRIES,
): EvidenceEntry[] {
  const atMs = Number.isFinite(entry.atMs) ? Math.max(0, Math.trunc(entry.atMs)) : 0
  const tag = sanitise(entry.tag, MAX_TAG)
  if (tag.length === 0) {
    return [...log]
  }
  const next = [...log, { atMs, tag, detail: sanitise(entry.detail) }]
  if (limit <= 0) {
    return []
  }
  return next.length > limit ? next.slice(next.length - limit) : next
}

export function encodeEvidence(log: readonly EvidenceEntry[]): string {
  return log.map((entry) => `${entry.atMs}\t${entry.tag}\t${entry.detail}`).join('\n')
}

export function decodeEvidence(text: string | null): EvidenceEntry[] {
  if (text === null || text.length === 0) {
    return []
  }
  const entries: EvidenceEntry[] = []
  for (const line of text.split('\n')) {
    const firstTab = line.indexOf('\t')
    const secondTab = firstTab < 0 ? -1 : line.indexOf('\t', firstTab + 1)
    if (secondTab < 0) {
      continue
    }
    const stamp = Number(line.slice(0, firstTab))
    const tag = line.slice(firstTab + 1, secondTab)
    if (!Number.isFinite(stamp) || stamp < 0 || tag.length === 0) {
      continue
    }
    entries.push({
      atMs: Math.trunc(stamp),
      tag: sanitise(tag, MAX_TAG),
      detail: sanitise(line.slice(secondTab + 1)),
    })
  }
  return entries
}

export function summariseEvidence(log: readonly EvidenceEntry[]): string {
  if (log.length === 0) {
    return 'Nothing has been recorded on this device yet.'
  }
  const last = log[log.length - 1]
  if (last === undefined) {
    return 'Nothing has been recorded on this device yet.'
  }
  return `${log.length} entries, last ${last.tag}: ${last.detail}`
}
