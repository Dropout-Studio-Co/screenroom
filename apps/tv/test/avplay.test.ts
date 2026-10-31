import { test } from 'node:test'
import assert from 'node:assert/strict'

import {
  AVPLAY_SPACE_HEIGHT,
  AVPLAY_SPACE_WIDTH,
  readTracks,
  toDisplayRect,
} from '../src/platform/avplay-pure.ts'

import { selectableTracks } from '../src/core/tracks.ts'

test('a full-screen app rectangle maps to the full 1920x1080 AVPlay rectangle', () => {

  assert.deepEqual(toDisplayRect({ x: 0, y: 0, width: 1920, height: 1080 }, { width: 1920, height: 1080 }), {
    x: 0,
    y: 0,
    width: 1920,
    height: 1080,
  })
})

test('an app that is not 1920 wide is scaled into AVPlay space', () => {

  const half = toDisplayRect({ x: 0, y: 0, width: 960, height: 540 }, { width: 960, height: 540 })
  assert.deepEqual(half, { x: 0, y: 0, width: 1920, height: 1080 })
})

test('a rectangle in the lower half stays in the lower half', () => {

  const half = toDisplayRect(
    { x: 0, y: 1080, width: 3840, height: 1080 },
    { width: 3840, height: 2160 },
  )
  assert.equal(half.y, 540, 'half-way down stays half-way down')
  assert.equal(half.height, 540)
  assert.equal(half.width, 1920)
})

test('the space constants are the ones the platform uses', () => {
  assert.equal(AVPLAY_SPACE_WIDTH, 1920)
  assert.equal(AVPLAY_SPACE_HEIGHT, 1080)
})

test('a degenerate viewport cannot produce a NaN or infinite rectangle', () => {

  for (const viewport of [
    { width: 0, height: 0 },
    { width: -100, height: -100 },
    { width: Number.NaN, height: Number.NaN },
    { width: Number.POSITIVE_INFINITY, height: Number.POSITIVE_INFINITY },
  ]) {
    const rect = toDisplayRect({ x: 0, y: 0, width: 1920, height: 1080 }, viewport)
    for (const value of [rect.x, rect.y, rect.width, rect.height]) {
      assert.ok(Number.isFinite(value), `${JSON.stringify(viewport)} produced ${value}`)
    }
    assert.equal(rect.width, 1920)
  }
})

test('tracks are read one entry at a time, which is what the published sample gets wrong', () => {

  const tracks = readTracks([
    { type: 'AUDIO', index: 0, extra_info: '{"language":"eng"}' },
    { type: 'TEXT', index: 0, extra_info: '{"language":"eng"}' },
    { type: 'TEXT', index: 1, extra_info: '{"language":"deu"}' },
  ])
  assert.equal(tracks.length, 3)
  assert.deepEqual(
    tracks.map((track) => `${track.type}:${track.index}:${track.language}`),
    ['AUDIO:0:eng', 'TEXT:0:eng', 'TEXT:1:deu'],
  )
  assert.match(tracks[0]?.label ?? '', /AUDIO 0/)
  assert.match(tracks[2]?.label ?? '', /deu/)
})

test('an entry that cannot be used is skipped rather than crashing the menu', () => {
  const tracks = readTracks([
    null,
    'not an object',
    { type: 'AUDIO' },
    { index: 4 },
    { type: 'AUDIO', index: 'zero' },
    { type: 'AUDIO', index: 3 },
  ])
  assert.deepEqual(
    tracks.map((track) => track.index),
    [3],
  )

  assert.deepEqual(readTracks(undefined), [])
  assert.deepEqual(readTracks('nope'), [])
})

test('an unreadable extra_info leaves the language unknown rather than inventing one', () => {
  for (const extra of [undefined, '', 'not json', '[]', '"text"', '{"other":1}']) {
    const tracks = readTracks([{ type: 'AUDIO', index: 0, extra_info: extra }])
    assert.equal(tracks[0]?.language, null, `extra_info=${String(extra)}`)
    assert.equal(tracks[0]?.label, 'AUDIO 0')
  }

  assert.equal(readTracks([{ type: 'AUDIO', index: 0, extra_info: '{"Language":"fra"}' }])[0]?.language, 'fra')
})

test('only the two switchable track types are offered as menus', () => {
  const tracks = readTracks([
    { type: 'VIDEO', index: 0 },
    { type: 'AUDIO', index: 0 },
    { type: 'TEXT', index: 0 },
    { type: 'SUBTITLE', index: 0 },
  ])
  const menus = selectableTracks(tracks)
  assert.deepEqual(
    menus.audio.map((track) => track.type),
    ['AUDIO'],
  )
  assert.deepEqual(
    menus.text.map((track) => track.type),
    ['TEXT'],
  )
  assert.ok(!menus.text.some((track) => track.type === 'SUBTITLE'))
})
