import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'

function inputBlock(file: string): string {
  const source = readFileSync(new URL(`../src/screens/${file}`, import.meta.url), 'utf8')
  const start = source.indexOf('\n  input: {')
  assert.notEqual(start, -1, `${file} declares no input block, so no remote key reaches it`)
  const end = source.indexOf('\n  },\n', start)
  assert.notEqual(end, -1, `${file} has an unterminated input block`)
  return source.slice(start, end)
}

const REQUIRED: Readonly<Record<string, readonly string[]>> = {

  'Home.ts': ['left', 'right', 'up', 'down', 'enter'],
  'Browse.ts': ['left', 'right', 'up', 'down', 'enter'],
  'Search.ts': ['left', 'right', 'up', 'down', 'enter'],
  'Sources.ts': ['left', 'right', 'up', 'down', 'enter'],
  'Detail.ts': ['up', 'down', 'enter'],
  'Settings.ts': ['left', 'right', 'up', 'down', 'enter'],

  'Player.ts': [
    'enter',
    'space',
    'play',
    'pause',
    'playPause',
    'left',
    'right',
    'up',
    'down',
    'rewind',
    'fastForward',
    'info',
    'back',
  ],
}

for (const [file, keys] of Object.entries(REQUIRED)) {
  test(`${file} declares every remote key it implements`, () => {
    const block = inputBlock(file)
    for (const key of keys) {
      assert.ok(
        block.includes(`${key}() {`),
        `${file} does not declare the ${key} key, so pressing it would do nothing`,
      )
    }
  })
}

test('no screen tells the viewer to press a key the remote may not have', () => {
  const screens = ['Home.ts', 'Browse.ts', 'Search.ts', 'Sources.ts', 'Detail.ts', 'Settings.ts']
  for (const file of screens) {
    const source = readFileSync(new URL(`../src/screens/${file}`, import.meta.url), 'utf8')
    assert.equal(
      /press Info|Info opens|Info key/i.test(source),
      false,
      `${file} still depends on the Info key`,
    )
  }
})

test('the player claims Back so the menu can close before the file does', () => {
  const block = inputBlock('Player.ts')
  assert.ok(block.includes('back() {'), 'the player must handle Back itself')
  assert.ok(block.includes('this.$router.back()'), 'and still leave the file afterwards')
})
