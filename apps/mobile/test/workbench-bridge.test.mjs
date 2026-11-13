import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { test } from 'node:test'
import { runInNewContext } from 'node:vm'

const origin = 'http://127.0.0.1:3088'
const script = readFileSync(new URL('../android/app/src/main/assets/web/workbench-bridge.js', import.meta.url), 'utf8')
const selector = '[data-row-key^="session:"]'

function openPage({ pageOrigin = origin, framed = false } = {}) {
  const messages = []
  const rows = []
  const observers = []
  let settings
  let sidebarOpens = 0
  class Socket extends EventTarget {
    constructor(url) { super(); this.url = url }
    frame(value) {
      this.dispatchEvent(new MessageEvent('message', { data: JSON.stringify({ type: 'item', value }) }))
    }
    status(id, running) { this.frame({ type: 'emit', event: 'api-session/status', args: [id, running] }) }
  }
  const document = {
    documentElement: { dataset: { dsThemeSource: 'system' } },
    body: { hasAttribute: () => false },
    head: { append() {}, querySelector: () => settings },
    readyState: 'complete',
    createElement: () => ({}),
    querySelectorAll(query) { assert.equal(query, selector); return rows },
    querySelector: () => ({ click() { sidebarOpens++ } }),
  }
  const window = { WingmateWorkbench: { postMessage: message => messages.push(JSON.parse(message)) }, WebSocket: Socket }
  window.top = framed ? {} : window
  runInNewContext(script.replace('__WINGMATE_ORIGIN__', JSON.stringify(origin)), {
    window, document, location: new URL(pageOrigin), URL,
    matchMedia: () => ({ matches: false }),
    MutationObserver: class { constructor(callback) { observers.push(callback) } observe() {} },
    clearTimeout, setTimeout,
  })
  return { messages, rows, observers, window, document, get sidebarOpens() { return sidebarOpens },
    addSettings(style) { settings = style; observers[0]() } }
}

test('observes task completion with the sidebar closed and skips duplicate or pending turns', () => {
  const page = openPage()
  const socket = new page.window.WebSocket('ws://127.0.0.1:3088/api/remote.mux')
  const unrelated = new page.window.WebSocket('ws://127.0.0.1:3088/other')
  unrelated.status('ignored', true)
  unrelated.status('ignored', false)
  socket.status('idle', false)
  socket.status('pending', true)
  socket.frame({ type: 'waterfall', eventId: 'question', agentId: 'pending' })
  socket.status('pending', false)
  socket.status('task', true)
  socket.status('task', false)
  socket.status('task', false)
  assert.deepEqual(page.messages, [
    { event: 'theme', theme: 'system' },
    { event: 'completed', sessionId: 'task' },
  ])
})

test('opens the corresponding session and syncs theme and legacy settings layout', () => {
  const page = openPage()
  let opens = 0
  page.rows.push({ dataset: { rowKey: 'session:task' }, click() { opens++ } })
  page.window.wingmateOpenSession('task')
  assert.equal(opens, 1)
  assert.equal(page.sidebarOpens, 0)
  page.document.documentElement.dataset.dsThemeSource = 'dark'
  page.observers[1]()
  assert.deepEqual(page.messages.at(-1), { event: 'theme', theme: 'dark' })
  const style = { textContent: '@media(width <= 767px){.settings{display:block}}' }
  page.addSettings(style)
  assert.equal(style.textContent, '@media (max-width: 767px){.settings{display:block}}')
})

test('only attaches to the connected top-level workbench', () => {
  for (const page of [openPage({ pageOrigin: 'https://other.example' }), openPage({ framed: true })]) {
    assert.deepEqual(page.messages, [])
    assert.equal(page.window.wingmateOpenSession, undefined)
  }
})
