import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readdirSync, readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..', 'src')

const DIRECTORIES = ['screens', 'components']

const FRAMEWORK_MEMBERS = new Set([
  'focus',
  'router',
  'log',
  'setTimeout',
  'clearTimeout',
  'fireAncestors',
  'emit',
  'el',
  'app',
  'props',
  'route',
  'intercept',
])

function componentFiles(): { readonly dir: string; readonly name: string }[] {
  return DIRECTORIES.flatMap((dir) =>
    readdirSync(join(ROOT, dir))
      .filter((name) => name.endsWith('.ts'))
      .sort()
      .map((name) => ({ dir, name })),
  )
}

function sourceOf(dir: string, name: string): string {
  return withoutComments(readFileSync(join(ROOT, dir, name), 'utf8'))
}

function withoutComments(source: string): string {
  return source
    .replace(/\/\*[\s\S]*?\*\//g, '')
    .replace(/(^|[^:'"\\])\/\/[^\n]*/g, '$1')
}

function templateBody(source: string): string {
  const marker = 'template: `'
  const start = source.indexOf(marker)
  assert.notEqual(start, -1, 'the component has no template')
  const from = start + marker.length
  const end = source.indexOf('`', from)
  assert.notEqual(end, -1, 'the template string is not closed')
  return source.slice(from, end)
}

function closingBrace(source: string, open: number): number {
  let depth = 0
  for (let i = open; i < source.length; i += 1) {
    const char = source[i]
    if (char === '{') {
      depth += 1
    } else if (char === '}') {
      depth -= 1
      if (depth === 0) {
        return i
      }
    }
  }
  return -1
}

function blockBody(source: string, signature: string): string | null {
  const at = source.indexOf(signature)
  if (at === -1) {
    return null
  }
  const open = source.indexOf('{', at + signature.length - 1)
  if (open === -1) {
    return null
  }
  const close = closingBrace(source, open)
  return close === -1 ? null : source.slice(open, close + 1)
}

function bracketDelta(line: string): number {
  let delta = 0
  let quote: string | null = null
  for (let i = 0; i < line.length; i += 1) {
    const char = line[i]
    if (quote !== null) {
      if (char === quote) {
        quote = null
      }
      continue
    }
    if (char === "'" || char === '"' || char === '`') {
      quote = char
    } else if (char === '{' || char === '(' || char === '[') {
      delta += 1
    } else if (char === '}' || char === ')' || char === ']') {
      delta -= 1
    }
  }
  return delta
}

function topLevelKeys(block: string): string[] {
  const keys: string[] = []
  let depth = 0
  for (const line of block.split('\n')) {
    const trimmed = line.trim()
    if (depth === 1 && trimmed.length > 0) {
      const match = /^([A-Za-z_$][\w$]*)\s*[:(]/.exec(trimmed)
      if (match !== null && match[1] !== undefined) {
        keys.push(match[1])
      }
    }
    depth += bracketDelta(line)
  }
  return keys
}

function declaredKeys(source: string): Set<string> {
  const keys = new Set<string>()

  const stateAt = source.indexOf('state()')
  if (stateAt !== -1) {
    const returnAt = source.indexOf('return {', stateAt)
    if (returnAt !== -1) {
      const open = source.indexOf('{', returnAt)
      const close = closingBrace(source, open)
      if (close !== -1) {
        for (const key of topLevelKeys(source.slice(open, close + 1))) {
          keys.add(key)
        }
      }
    }
  }
  for (const signature of ['props:', 'computed:']) {
    const body = blockBody(source, signature)
    if (body !== null) {
      for (const key of topLevelKeys(body)) {
        keys.add(key)
      }
    }
  }
  return keys
}

function loopVariables(template: string): Set<string> {
  const names = new Set<string>()
  for (const match of template.matchAll(/:?for="([A-Za-z_$][\w$]*)\s+in\b/g)) {
    if (match[1] !== undefined) {
      names.add(match[1])
    }
  }
  return names
}

function references(template: string): Set<string> {
  const names = new Set<string>()
  for (const match of template.matchAll(/\$([A-Za-z_$][\w$]*)/g)) {
    if (match[1] !== undefined) {
      names.add(match[1])
    }
  }
  return names
}

test('every template reference in every component resolves to something it declares', () => {
  const problems: string[] = []
  for (const { dir, name } of componentFiles()) {
    const source = sourceOf(dir, name)
    const template = templateBody(source)
    const known = new Set([
      ...declaredKeys(source),
      ...loopVariables(template),
      ...FRAMEWORK_MEMBERS,
    ])
    const missing = [...references(template)].filter((name) => !known.has(name)).sort()
    if (missing.length > 0) {
      problems.push(`${name}: ${missing.map((key) => `$${key}`).join(', ')}`)
    }
  }
  assert.deepEqual(
    problems,
    [],
    `a template can only read state, props, computed values and its own loop variables:\n${problems.join('\n')}`,
  )
})

test('no loop variable is named key, which the compiled scope overwrites', () => {
  const problems: string[] = []
  for (const { dir, name } of componentFiles()) {
    const template = templateBody(sourceOf(dir, name))
    for (const match of template.matchAll(/:?for="([A-Za-z_$][\w$]*)\s+in\b/g)) {
      if (match[1] === 'key') {
        problems.push(`${name}: a :for variable is named key`)
      }
    }
  }
  assert.deepEqual(problems, [], problems.join('\n'))
})

test('the checker itself sees the state it is supposed to see', () => {

  const source = sourceOf('screens', 'Home.ts')
  const declared = declaredKeys(source)
  for (const key of ['cards', 'background', 'footY', 'grid']) {
    assert.ok(declared.has(key), `expected ${key} to be parsed out of Home.ts`)
  }

  const named = references(templateBody(source))
  assert.ok(named.has('footY'), 'expected the Home template to name $footY')
  assert.deepEqual(
    [...loopVariables(templateBody(source))],
    ['card'],
    'expected the Home template to declare the card loop variable',
  )
})
