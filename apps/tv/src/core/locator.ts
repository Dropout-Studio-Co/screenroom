export type LocatorFault =
  | 'empty'
  | 'absolute'
  | 'parent_directory'
  | 'root'
  | 'nul_byte'
  | 'windows_prefix'

export type LocatorVerdict =
  | { readonly safe: true; readonly segments: readonly string[] }
  | { readonly safe: false; readonly fault: LocatorFault }

function hasWindowsPrefix(locator: string): boolean {
  return /^[a-zA-Z]:/.test(locator) || locator.startsWith('\\\\') || locator.startsWith('//')
}

export function checkLocator(locator: string): LocatorVerdict {
  if (locator.length === 0) {
    return { safe: false, fault: 'empty' }
  }
  if (locator.includes('\0')) {
    return { safe: false, fault: 'nul_byte' }
  }
  if (hasWindowsPrefix(locator)) {
    return { safe: false, fault: 'windows_prefix' }
  }

  if (locator.startsWith('/') || locator.startsWith('\\')) {
    return { safe: false, fault: 'absolute' }
  }

  const segments: string[] = []
  for (const segment of locator.split(/[/\\]/)) {
    if (segment.length === 0 || segment === '.') {
      continue
    }
    if (segment === '..') {
      return { safe: false, fault: 'parent_directory' }
    }
    segments.push(segment)
  }

  if (segments.length === 0) {
    return { safe: false, fault: 'root' }
  }
  return { safe: true, segments }
}

function usableRoot(rootPath: string): string | null {
  const root = rootPath.replace(/[/\\]+$/, '')
  if (root.length === 0 || root.includes('\0')) {
    return null
  }
  if (root.startsWith('/') || /^[a-zA-Z]:/.test(root)) {
    return root
  }

  return /^[A-Za-z0-9_][A-Za-z0-9._-]*$/.test(root) ? root : null
}

export function resolveLocator(rootPath: string, locator: string): string | null {
  const verdict = checkLocator(locator)
  if (!verdict.safe) {
    return null
  }
  const root = usableRoot(rootPath)
  if (root === null) {
    return null
  }
  return `${root}/${verdict.segments.join('/')}`
}

export function subtitleStem(locator: string): string | null {
  const verdict = checkLocator(locator)
  if (!verdict.safe) {
    return null
  }
  const name = verdict.segments[verdict.segments.length - 1]
  if (name === undefined) {
    return null
  }
  const dot = name.lastIndexOf('.')
  return dot > 0 ? name.slice(0, dot) : name
}
