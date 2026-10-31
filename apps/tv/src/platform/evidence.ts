import {
  appendEvidence,
  decodeEvidence,
  encodeEvidence,
  MAX_ENTRIES,
  type EvidenceEntry,
} from '../core/evidence.ts'

export const EVIDENCE_KEY = 'screenroom:v1:evidence'

export function readEvidence(): EvidenceEntry[] {
  try {
    return decodeEvidence(globalThis.localStorage.getItem(EVIDENCE_KEY))
  } catch {
    return []
  }
}

export function recordEvidence(
  tag: string,
  detail: string,
  now: () => number = () => Date.now(),
): EvidenceEntry[] {
  const existing = readEvidence()
  const next = appendEvidence(existing, { atMs: now(), tag, detail }, MAX_ENTRIES)
  try {
    globalThis.localStorage.setItem(EVIDENCE_KEY, encodeEvidence(next))
  } catch {

  }
  return next
}

export function clearEvidence(): void {
  try {
    globalThis.localStorage.removeItem(EVIDENCE_KEY)
  } catch {

  }
}
