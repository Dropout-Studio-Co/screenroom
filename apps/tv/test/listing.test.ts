import { test } from 'node:test'
import assert from 'node:assert/strict'

import { readEntriesFrom } from '../src/platform/media-roots.ts'

function platformFile(name: string, isDirectory: boolean): Record<string, unknown> {
  return {
    name,
    isDirectory,
    isFile: !isDirectory,
    path: `removable1/${name}`,
    fullPath: `removable1/${name}`,
    toURI: () => `file:///removable1/${name}`,
  }
}

test('the platform hands back File objects, and their names are what a walk needs', () => {
  const entries = readEntriesFrom([
    platformFile('Movies', true),
    platformFile('Film.1995.mkv', false),
  ])
  assert.deepEqual(entries, [
    { name: 'Movies', isDirectory: true },
    { name: 'Film.1995.mkv', isDirectory: false },
  ])
})

test('a listing of names is accepted, with the type left to be asked for', () => {

  assert.deepEqual(readEntriesFrom(['Movies', 'Film.1995.mkv']), [
    { name: 'Movies', isDirectory: null },
    { name: 'Film.1995.mkv', isDirectory: null },
  ])
})

test('a file flag without a directory flag is still a fact, not a guess', () => {
  assert.deepEqual(readEntriesFrom([{ name: 'Trailer.mp4', isFile: true }]), [
    { name: 'Trailer.mp4', isDirectory: false },
  ])
})

test('an entry that cannot be named is dropped rather than invented', () => {
  const entries = readEntriesFrom([
    null,
    undefined,
    42,
    {},
    { name: '' },
    { name: 7 },
    platformFile('Show', true),
  ])
  assert.deepEqual(entries, [{ name: 'Show', isDirectory: true }])
})

test('something that is not a listing at all produces no entries, not a crash', () => {
  for (const value of [null, undefined, 'Movies', 7, { name: 'Movies' }]) {
    assert.deepEqual(readEntriesFrom(value), [])
  }
})

test('a nested tree keeps every level, so a show folder inside a shows folder survives', () => {

  const root = readEntriesFrom([
    platformFile('Movies', true),
    platformFile('TV Shows', true),
    platformFile('.Trashes', true),
  ])
  const shows = readEntriesFrom([platformFile('Dark', true), platformFile('Severance', true)])
  const seasons = readEntriesFrom([platformFile('Season 01', true)])
  const episodes = readEntriesFrom([platformFile('Dark S01E01.mkv', false)])

  assert.deepEqual(
    [root.length, shows.length, seasons.length, episodes.length],
    [3, 2, 1, 1],
  )
  assert.equal(seasons[0]?.isDirectory, true)
  assert.equal(episodes[0]?.isDirectory, false)
})
