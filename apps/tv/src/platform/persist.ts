import type { StoragePort } from '../core/storage.ts'

export const STORE_KEY = 'screenroom:v1:library'

export function createDeviceStorage(): StoragePort {
  return {
    read(): string | null {
      try {
        return globalThis.localStorage.getItem(STORE_KEY)
      } catch {

        return null
      }
    },
    write(document: string): boolean {
      try {
        globalThis.localStorage.setItem(STORE_KEY, document)
        return true
      } catch {
        return false
      }
    },
    clear(): void {
      try {
        globalThis.localStorage.removeItem(STORE_KEY)
      } catch {

      }
    },
    describe(): string {
      return 'the TV\'s own storage for this app'
    },
  }
}
