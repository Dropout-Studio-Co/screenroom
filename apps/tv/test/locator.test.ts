import { test } from 'node:test'
import assert from 'node:assert/strict'

import { checkLocator, resolveLocator, subtitleStem } from '../src/core/locator.ts'

test('a plain relative locator is accepted and split into segments', () => {
  const verdict = checkLocator('Shows/Season 1/Show.S01E01.mkv')
  assert.equal(verdict.safe, true)
  assert.deepEqual(verdict.safe ? verdict.segments : [], [
    'Shows',
    'Season 1',
    'Show.S01E01.mkv',
  ])
})

test('an absolute locator is refused on both separator conventions', () => {
  for (const locator of ['/etc/passwd', '\\Windows\\System32', '//server/share']) {
    const verdict = checkLocator(locator)
    assert.equal(verdict.safe, false, `${locator} must be refused`)
  }
})

test('a parent-directory segment is refused however it is spelled', () => {
  for (const locator of ['../outside.mkv', 'Shows/../../outside.mkv', 'Shows/..', '..\\ou.mkv']) {
    const verdict = checkLocator(locator)
    assert.equal(verdict.safe, false, `${locator} must be refused`)
    assert.equal(verdict.safe === false ? verdict.fault : '', 'parent_directory')
  }
})

test('a Windows drive prefix is refused even though this device is not Windows', () => {

  for (const locator of ['C:/Media/Film.mkv', 'D:Film.mkv', 'c:\\Film.mkv']) {
    const verdict = checkLocator(locator)
    assert.equal(verdict.safe, false, `${locator} must be refused`)
    assert.equal(verdict.safe === false ? verdict.fault : '', 'windows_prefix')
  }
})

test('a NUL byte and an empty locator are refused', () => {
  assert.equal(checkLocator('Fil\0m.mkv').safe, false)
  assert.equal(checkLocator('').safe, false)
  assert.equal(checkLocator('./').safe, false)
})

test('a doubled separator is a spelling difference, not an escape', () => {
  const verdict = checkLocator('Shows//Film.mkv')
  assert.equal(verdict.safe, true)
  assert.deepEqual(verdict.safe ? verdict.segments : [], ['Shows', 'Film.mkv'])
})

test('joining keeps the file inside the granted root', () => {
  assert.equal(resolveLocator('/USB/Media', 'Shows/Film.mkv'), '/USB/Media/Shows/Film.mkv')

  assert.equal(resolveLocator('/USB/Media/', 'Film.mkv'), '/USB/Media/Film.mkv')

  assert.equal(resolveLocator('/USB/Media', '../Film.mkv'), null)
  assert.equal(resolveLocator('/USB/Media', '/etc/passwd'), null)

  assert.equal(resolveLocator('USB/Media', 'Film.mkv'), null)
})

test('a drive the TV reports by label is a usable root', () => {

  assert.equal(resolveLocator('removable1', 'Movies/Film.mkv'), 'removable1/Movies/Film.mkv')
  assert.equal(resolveLocator('removable_sda1/', 'Film.mkv'), 'removable_sda1/Film.mkv')
  assert.equal(resolveLocator('videos', 'Film.mkv'), 'videos/Film.mkv')

  assert.equal(resolveLocator('removable1', '../Film.mkv'), null)
  assert.equal(resolveLocator('removable1', 'Movies/../../Film.mkv'), null)
})

test('a root that is neither a device path nor a location name is refused', () => {
  for (const root of ['', '/', 'Movies/Films', 'has space', 'removable1\0']) {
    assert.equal(
      resolveLocator(root, 'Film.mkv'),
      null,
      `${JSON.stringify(root)} must not be treated as a media root`,
    )
  }
})

test('the subtitle stem is the filename without its container extension', () => {
  assert.equal(subtitleStem('Show/Show.S01E01.mkv'), 'Show.S01E01')
  assert.equal(subtitleStem('Film'), 'Film')
  assert.equal(subtitleStem('../Film.mkv'), null)
})
