export function routeString(value: unknown, fallback = ''): string {
  return typeof value === 'string' ? value : fallback
}

export function routeBoolean(value: unknown, fallback: boolean): boolean {
  return typeof value === 'boolean' ? value : fallback
}
