import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { test } from 'node:test'
import { runInNewContext } from 'node:vm'

const script = readFileSync(new URL('../android/app/src/main/assets/web/webview-compat.js', import.meta.url), 'utf8')
const window = {}
window.top = window

test('keeps the native interfaces on newer WebView versions', () => {
  const preserved = runInNewContext(`
    const native = [Array.prototype.toSorted, Array.prototype.toReversed, URL.canParse, Promise.withResolvers, AbortSignal.any];
    ${script}
    const current = [Array.prototype.toSorted, Array.prototype.toReversed, URL.canParse, Promise.withResolvers, AbortSignal.any]
    native.every((method, index) => typeof method === 'function' && method === current[index])
  `, { window, URL, AbortSignal })
  assert.equal(preserved, true)
})

test('does not install compatibility interfaces in a child frame', () => {
  const result = runInNewContext(`
    delete Array.prototype.toSorted;
    ${script}
    typeof Array.prototype.toSorted
  `, { window: { top: {} } })
  assert.equal(result, 'undefined')
})

test('restores the interfaces missing on WebView 104', async () => {
  // The test file runs in its own process. Model the WebView's missing native interfaces.
  const removed = [
    [Promise, 'withResolvers'],
    [AbortSignal, 'any'],
  ].map(([owner, name]) => [owner, name, Object.getOwnPropertyDescriptor(owner, name)])
  try {
    for (const [owner, name] of removed) delete owner[name]
    class LegacyURL extends URL { static canParse }
    const browser = { window, Promise, AbortController, AbortSignal, URL: LegacyURL }
    runInNewContext(`
      for (const name of ['toSorted', 'toReversed']) {
        delete Array.prototype[name]
      }
      ${script}
      const rows = [3, 1, 2]
      globalThis.arrayResult = JSON.stringify([
        rows.toSorted((a, b) => a - b), rows.toReversed(), rows,
      ])
    `, browser)
    assert.deepEqual(JSON.parse(browser.arrayResult), [[1, 2, 3], [2, 1, 3], [3, 1, 2]])
    assert.equal(LegacyURL.canParse('child', 'http://127.0.0.1/'), true)
    assert.equal(LegacyURL.canParse('child'), false)

    const resolved = Promise.withResolvers()
    resolved.resolve('ready')
    assert.equal(await resolved.promise, 'ready')
    const rejected = Promise.withResolvers()
    rejected.reject(new Error('failed'))
    await assert.rejects(rejected.promise, /failed/)

    const first = new AbortController()
    const second = new AbortController()
    const combined = AbortSignal.any([first.signal, second.signal])
    assert.equal(combined.reason, undefined)
    combined.throwIfAborted()
    const reason = new Error('cancelled')
    second.abort(reason)
    first.abort()
    assert.equal(combined.reason, reason)
    assert.throws(() => combined.throwIfAborted(), (error) => error === reason)
    assert.equal(AbortSignal.any([first.signal, second.signal]).reason.name, 'AbortError')
  } finally {
    for (const [owner, name, descriptor] of removed) Object.defineProperty(owner, name, descriptor)
  }
})
