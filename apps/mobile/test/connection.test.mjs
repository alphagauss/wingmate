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
      if (!elements.has(selector)) elements.set(selector, { dataset: {} })
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
  reply('restore', { value: { autoConnect: true, language: 'en-US', messages: {} } }, { recent: [origin] })
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
  assert.equal(elements.get('#scan-progress').className, 'connecting')
  reply('open', { error: 'Connection failed' }, { link })
  await scanned
  assert.equal(elements.get('#scan-progress').className, '')
  assert.equal(elements.get('#notice').textContent, 'Connection failed')
  assert.equal(elements.get('#scan-progress').hidden, true)
  assert.deepEqual(requests, [])
})

test('switches the home entry from engine settings to the local workbench when ready', async () => {
  const { elements, reply, requests } = mountHome()
  await import('../android/app/src/main/assets/launch/connection.js?local')
  reply('restore', { value: {
    localEntry: 'localAgent.title', autoConnect: false, language: 'en-US',
    messages: { 'localAgent.title': 'Local engine', 'localAgent.workbench': 'Local workbench' },
  } },
    { recent: [origin] })
  await setImmediate()
  assert.deepEqual(requests, [])
  const entry = elements.get('#local-agent')
  assert.equal(entry.hidden, false)
  assert.equal(entry.textContent, 'Local engine')
  window.updateLocalEntry('localAgent.workbench')
  assert.equal(entry.dataset.i18n, 'localAgent.workbench')
  assert.equal(entry.textContent, 'Local workbench')
  const opened = entry.onclick()
  reply('localAgent', {})
  await opened
})

test('honors the startup preference independently of the APK and updates home labels without restoring again', async () => {
  const { reply, requests } = mountHome()
  await import('../android/app/src/main/assets/launch/connection.js?manual')
  reply('restore', { value: { autoConnect: false, language: 'en-US', messages: {} } }, { recent: [origin] })
  await setImmediate()
  window.applyWingmateLabels('zh-CN', {})
  assert.equal(document.documentElement.lang, 'zh-CN')
  assert.deepEqual(requests, [])
})

test('skips the startup connection when the user has opened host selection', async () => {
  const { elements, reply, requests } = mountHome()
  await import('../android/app/src/main/assets/launch/connection.js?host-selection')
  reply('restore', { value: { autoConnect: true, language: 'en-US', messages: {} } }, { recent: [origin] })
  const opened = elements.get('#history').onclick()
  reply('drawer', {})
  await opened
  assert.deepEqual(requests, [])
})
