import Blits from '@lightningjs/blits'

import App from './App.ts'
import { recordEvidence } from './platform/evidence.ts'
import { REMOTE_KEYMAP } from './platform/keys.ts'

function recordFailures(): void {
  const target = globalThis as {
    addEventListener?: (type: string, listener: (event: unknown) => void) => void
  }
  if (typeof target.addEventListener !== 'function') {
    return
  }
  target.addEventListener('error', (event) => {
    const detail = event as { message?: string; filename?: string; lineno?: number; error?: unknown }
    const message = detail.message ?? (detail.error instanceof Error ? detail.error.message : 'error')
    const where = detail.filename ? ` at ${detail.filename.split('/').pop()}:${detail.lineno ?? 0}` : ''
    recordEvidence('error', `${message}${where}`)
  })
  target.addEventListener('unhandledrejection', (event) => {
    const reason = (event as { reason?: unknown }).reason
    const message = reason instanceof Error ? reason.message : String(reason)
    recordEvidence('rejection', message)
  })
}

function recordRender(): void {
  globalThis.setTimeout(() => {
    try {
      const canvases = Array.from(document.querySelectorAll('canvas'))
      const first = canvases[0]
      const painted = document.getElementById('app')?.childElementCount ?? 0
      const ratio = globalThis.devicePixelRatio ?? 1

      const viewport = `${globalThis.innerWidth ?? 0}x${globalThis.innerHeight ?? 0}`
      recordEvidence(
        'render',
        first === undefined
          ? `canvases=0 mounted=${painted} viewport=${viewport} dpr=${ratio}`
          : [
              `canvases=${canvases.length}`,
              `size=${first.width}x${first.height}`,
              `css=${first.style.width || 'unset'}/${first.style.height || 'unset'}`,
              `client=${first.clientWidth}x${first.clientHeight}`,
              `viewport=${viewport}`,
              `dpr=${ratio}`,
              `mounted=${painted}`,
            ].join(' '),
      )
    } catch (error) {
      recordEvidence('render', `probe failed: ${error instanceof Error ? error.message : 'unknown'}`)
    }
  }, 700)
}

recordFailures()

Blits.Launch(App, 'app', {
  w: 1920,
  h: 1080,

  debugLevel: 0,
  keymap: REMOTE_KEYMAP,
  inputThrottle: 80,
  holdTimeout: 320,
  screenResolution: '1080p',
  renderQuality: 'high',

  canvasColor: 'transparent',
  defaultFont: 'sans-serif',
  enableMouse: false,
  multithreaded: false,
  gpuMemory: {
    max: 90,
    target: 0.7,
    cleanupInterval: 3000,
    baseline: 20,
  },
})

recordRender()
