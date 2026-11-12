import assert from 'node:assert/strict'
import { setImmediate } from 'node:timers/promises'
import { test } from 'node:test'

const historyKey = 'wingmate.recent-connections'
const origin = 'http://192.168.1.8:3210'

function mountHome() {
  const history = new Map([[historyKey, JSON.stringify([origin])]])
  const elements = new Map()
  const requests = []
  globalThis.document = {
    documentElement: {},
    querySelector(selector) {
      if (!elements.has(selector)) elements.set(selector, {})
      return elements.get(selector)
    },
    querySelectorAll: () => [],
  }
  globalThis.localStorage = {
    getItem: key => history.get(key),
    removeItem: key => history.delete(key),
  }
  globalThis.window = {
    wingmateConnection: { postMessage: message => requests.push(JSON.parse(message)) },
  }

  function reply(action, payload, args = {}) {
    const request = requests.shift()
    assert.deepEqual(request, { id: request.id, action, args })
    window.receiveConnectionResult(request.id, payload)
  }
  return { elements, history, reply, requests }
}

test('restores the previous home history and opens a scanned link through the native bridge', async () => {
  const { elements, history, reply, requests } = mountHome()
  await import('../android/app/src/main/assets/launch/connection.js')
  reply('restore', { value: { count: 1, language: 'en-US', messages: {} } }, { recent: [origin] })
  await setImmediate()
  assert.equal(history.has(historyKey), false)
  assert.equal(document.documentElement.lang, 'en-US')
  assert.equal(elements.get('#local-agent').hidden, true)
  reply('discover', {}, { historyOnly: true })
  await setImmediate()

  const scanned = elements.get('#scan').onclick()
  const link = `${origin}/?auth=one-time`
  reply('scan', { value: { text: link } })
  await setImmediate()
  assert.equal(elements.get('#scan-progress').hidden, false)
  reply('open', { error: 'Connection failed' }, { link })
  await scanned
  assert.equal(elements.get('#notice').textContent, 'Connection failed')
  assert.equal(elements.get('#scan-progress').hidden, true)
  assert.deepEqual(requests, [])
})

test('shows the local entry before automatically connecting to any remembered host', async () => {
  const { elements, reply, requests } = mountHome()
  await import('../android/app/src/main/assets/launch/connection.js?local')
  reply('restore', { value: { localAgent: true, count: 1, language: 'en-US', messages: {} } }, { recent: [origin] })
  await setImmediate()
  assert.deepEqual(requests, [])
  const entry = elements.get('#local-agent')
  assert.equal(entry.hidden, false)
  const opened = entry.onclick()
  reply('localAgent', {})
  await opened
})
