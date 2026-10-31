import { filterItems, sortForDisplay } from './browse.ts'
import type { LibraryItem } from './contract.ts'

export const QUERY_MAX = 40

export interface SearchKey {

  readonly id: string

  readonly label: string
  readonly kind: 'char' | 'delete' | 'clear'

  readonly value: string
}

export const SEARCH_KEYS: readonly SearchKey[] = [
  ...'ABCDEF'.split('').map(charKey),
  ...'GHIJKL'.split('').map(charKey),
  ...'MNOPQR'.split('').map(charKey),
  ...'STUVWX'.split('').map(charKey),
  ...'YZ0123'.split('').map(charKey),
  ...'456789'.split('').map(charKey),
  { id: 'space', label: 'Space', kind: 'char', value: ' ' },
  { id: 'delete', label: '⌫', kind: 'delete', value: '' },
  { id: 'clear', label: 'Clear', kind: 'clear', value: '' },
]

function charKey(char: string): SearchKey {
  return { id: `key-${char.toLowerCase()}`, label: char, kind: 'char', value: char }
}

export function withKey(query: string, key: SearchKey): string {
  switch (key.kind) {
    case 'char':
      return query.length >= QUERY_MAX ? query : `${query}${key.value}`
    case 'delete':
      return query.slice(0, -1)
    case 'clear':
      return ''
  }
}

export function searchResults(
  items: readonly LibraryItem[],
  query: string,
): LibraryItem[] {
  const trimmed = query.trim()
  if (trimmed.length === 0) {
    return []
  }
  return sortForDisplay(filterItems(items, { kind: 'all', text: trimmed }))
}

export function describeSearch(query: string, matches: number): string {
  if (query.trim().length === 0) {
    return 'Type a film, a show or a file name with the arrow keys and OK.'
  }
  if (matches === 0) {
    return `Nothing matches “${query.trim()}”. ⌫ removes a letter, Clear starts again.`
  }
  return `${matches} ${matches === 1 ? 'match' : 'matches'} for “${query.trim()}”.`
}
