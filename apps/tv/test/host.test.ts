import { afterEach, test } from 'node:test'
import assert from 'node:assert/strict'

import { detectHost, hostSummary, readDeviceInfo } from '../src/platform/env.ts'
import { hasAvPlay } from '../src/platform/tizen.ts'

const globals = globalThis as { tizen?: unknown; webapis?: unknown }

function install(shape: { tizen?: unknown; webapis?: unknown }): void {
  globals.tizen = shape.tizen
  globals.webapis = shape.webapis
}

afterEach(() => {
  delete globals.tizen
  delete globals.webapis
})

const tizenDeviceApis = {
  filesystem: { listStorages() {} },
  tvinputdevice: { registerKey() {}, registerKeyBatch() {}, getSupportedKeys: () => [] },
}

const avplayStub = {
  open() {},
  close() {},
  getState: () => 'NONE',
  getCurrentTime: () => 0,
  getDuration: () => 0,
  getTotalTrackInfo: () => [],
  setDisplayRect() {},
  prepareAsync() {},
  play() {},
  pause() {},
  stop() {},
  seekTo() {},
}

test('AVPlay is found on window.webapis, not on window.tizen', () => {
  install({ tizen: tizenDeviceApis, webapis: { avplay: avplayStub } })
  assert.equal(hasAvPlay(), true)
  assert.equal(detectHost().avplay, true)
})

test('a TV with AVPlay is reported as the TV, not as a browser preview', () => {
  install({ tizen: tizenDeviceApis, webapis: { avplay: avplayStub } })
  const host = detectHost()
  assert.equal(host.kind, 'tizen')
  assert.match(hostSummary(host), /AVPlay is present/)

  assert.equal(host.notes.some((note) => note.includes('desktop browser')), false)
})

test('the nested form is still accepted, for hosts that expose it that way', () => {
  install({ tizen: { ...tizenDeviceApis, webapis: { avplay: avplayStub } } })
  assert.equal(hasAvPlay(), true)
})

test('a host with tizen device APIs but no AVPlay is a browser, and says so', () => {

  install({ tizen: tizenDeviceApis })
  const host = detectHost()
  assert.equal(host.avplay, false)
  assert.equal(host.kind, 'browser')
  assert.match(hostSummary(host), /AVPlay is absent/)
})

test('the device identity comes from webapis.productinfo', () => {
  install({
    tizen: tizenDeviceApis,
    webapis: {
      avplay: avplayStub,
      productinfo: {
        getModel: () => 'UA50AU8000',
        getFirmware: () => 'T-KTMAKUC-1490.3',
        getVersion: () => '6.0',
      },
    },
  })
  assert.deepEqual(readDeviceInfo(), {
    model: 'UA50AU8000',
    firmwareVersion: 'T-KTMAKUC-1490.3',

    platformVersion: null,
    productVersion: '6.0',
  })
})

test('a product API that throws degrades to null rather than failing the probe', () => {
  install({
    webapis: {
      avplay: avplayStub,
      productinfo: {
        getModel: () => {
          throw new Error('not supported')
        },
      },
    },
  })
  assert.equal(readDeviceInfo().model, null)
})

test('an empty product answer is treated as no answer', () => {
  install({ webapis: { avplay: avplayStub, productinfo: { getModel: () => '' } } })
  assert.equal(readDeviceInfo().model, null)
})
