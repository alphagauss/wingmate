import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { test } from 'node:test'
import { runInNewContext } from 'node:vm'

const script = readFileSync(new URL('../../../runtime/android/webview-compat.js', import.meta.url), 'utf8')

test('keeps the native interfaces on newer WebView versions', () => {
  const native = [Promise.withResolvers, AbortSignal.any, AbortSignal.prototype.throwIfAborted, AbortController.prototype.abort, structuredClone]
  const browser = { Promise, AbortController, AbortSignal, structuredClone, URL }
  runInNewContext(script, browser)
  assert.deepEqual(
    [Promise.withResolvers, AbortSignal.any, AbortSignal.prototype.throwIfAborted, AbortController.prototype.abort, browser.structuredClone],
    native,
  )
})

test('restores connection and workspace configuration APIs on WebView 96', async () => {
  // The test file runs in its own process. Model the WebView's missing native interfaces.
  const removed = [
    [Promise, 'withResolvers'],
    [AbortSignal, 'any'],
    [AbortSignal.prototype, 'throwIfAborted'],
    [AbortSignal.prototype, 'reason'],
    [AbortController.prototype, 'abort'],
  ].map(([owner, name]) => [owner, name, Object.getOwnPropertyDescriptor(owner, name)])
  try {
    for (const [owner, name] of removed.slice(0, -1)) delete owner[name]
    class LegacyURL extends URL { static canParse }
    const browser = {
      Promise, AbortController, AbortSignal, DOMException, FinalizationRegistry, WeakRef, PerformanceMark, MessageChannel,
      URL: LegacyURL,
    }
    runInNewContext(`
      for (const name of ['toSorted', 'toReversed', 'findLast', 'findLastIndex']) {
        delete Array.prototype[name]
      }
      ${script}
      const rows = [3, 1, 2]
      globalThis.arrayResult = JSON.stringify([
        rows.toSorted((a, b) => a - b), rows.toReversed(), rows.findLast(n => n < 3),
        rows.findLastIndex(n => n === 1), rows,
      ])
    `, browser)
    assert.deepEqual(JSON.parse(browser.arrayResult), [[1, 2, 3], [2, 1, 3], 2, 1, [3, 1, 2]])
    assert.equal(LegacyURL.canParse('child', 'http://127.0.0.1/'), true)
    assert.equal(LegacyURL.canParse('child'), false)

    const workspaceOps = [{ kind: 'set', path: ['workspace'], value: { directories: ['/root'], missing: undefined } }]
    const copy = browser.structuredClone(workspaceOps)
    assert.deepEqual(copy, workspaceOps)
    copy[0].value.directories.push('/tmp')
    assert.deepEqual(workspaceOps[0].value.directories, ['/root'])
    const bytes = Uint8Array.of(1, 2, 3)
    assert.deepEqual(browser.structuredClone(bytes, { transfer: [bytes.buffer] }), Uint8Array.of(1, 2, 3))
    assert.equal(bytes.byteLength, 0)

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
