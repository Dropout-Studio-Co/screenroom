import { test } from 'node:test'
import assert from 'node:assert/strict'

import { FOCUS_SCALE, hex, MIN_LEGIBLE_PX, TOKENS, TYPE_SCALE } from '../src/design/tokens.ts'

test('every body-size role is legible from a sofa', () => {

  const readable = ['body', 'bodyLg', 'lead', 'subtitle', 'titleSm', 'title', 'titleLg'] as const
  for (const role of readable) {
    assert.ok(
      TOKENS.type[role] >= MIN_LEGIBLE_PX,
      `${role} is ${TOKENS.type[role]}px, below the ${MIN_LEGIBLE_PX}px floor`,
    )
  }
  assert.ok(TOKENS.type.caption >= 18, 'even the smallest metadata has to be readable')
})

test('the type scale keeps the desktop roles in ascending order', () => {
  const order = [
    'micro',
    'caption',
    'label',
    'body',
    'bodyLg',
    'lead',
    'subtitle',
    'titleSm',
    'title',
    'titleLg',
  ] as const
  let previous = 0
  for (const role of order) {
    assert.ok(TOKENS.type[role] > previous, `${role} must be larger than the one below it`)
    previous = TOKENS.type[role]
  }
})

test('the scale is one viewing-distance step up from the desktop, not a fork', () => {

  assert.ok(TYPE_SCALE >= 1.5 && TYPE_SCALE <= 2.0, `TYPE_SCALE is ${TYPE_SCALE}`)
  assert.equal(TOKENS.type.body, 16 * TYPE_SCALE)
  assert.ok(TOKENS.control.md > TOKENS.control.sm)
})

test('focus grows a card by enough to be seen from the sofa', () => {
  assert.ok(FOCUS_SCALE > 1.0 && FOCUS_SCALE <= 1.15, `FOCUS_SCALE is ${FOCUS_SCALE}`)
})

test('every colour role is opaque, except the hairline that is meant not to be', () => {
  const roles = Object.entries(TOKENS.palette)
  for (const [name, value] of roles) {
    assert.ok(Number.isInteger(value) && value >= 0, `${name} is not a colour`)
    const alpha = value & 0xff
    if (name === 'hair') {
      assert.ok(alpha < 255, 'a hairline has to be translucent to read as a separator')
      continue
    }
    assert.equal(alpha, 255, `${name} must be fully opaque`)
  }
})

test('the provenance is stated, and says whether the private pin was used', () => {
  assert.ok(TOKENS.provenance.length > 0)

  assert.match(TOKENS.provenance, /fallback|design system/)
})

test('hex conversion is byte-correct', () => {
  assert.equal(hex(TOKENS.palette.background), '#121214')
  assert.equal(hex(TOKENS.palette.foreground), '#f5f5f7')
})
