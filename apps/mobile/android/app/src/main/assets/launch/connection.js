import { parsePairingLink } from './pairing-link.js'

const notice = document.querySelector('#notice')
const progress = document.querySelector('#scan-progress')
const actions = document.querySelector('.actions')
const historyKey = 'wingmate.recent-connections'
const pending = new Map()
let requestId = 0
let generation = 0
let scanning = false
let messages = {}

function label(key, fallback) {
  return messages[key] ?? fallback
}

function message(text = '') {
  notice.textContent = text
  notice.hidden = !text
}

function action(action, args = {}) {
  const id = ++requestId
  return new Promise((resolve, reject) => {
    pending.set(id, { resolve, reject })
    window.wingmateConnection.postMessage(JSON.stringify({ id, action, args }))
  })
}

window.receiveConnectionResult = (id, { value, error }) => {
  const request = pending.get(id)
  if (!request) return
  pending.delete(id)
  if (error != null) request.reject(error)
  else request.resolve(value)
}

function setScanning(active, text = '', cancellable = true) {
  scanning = active
  actions.hidden = active
  progress.hidden = !active
  progress.className = active && !cancellable ? 'connecting' : ''
  document.querySelector('#scan-message').textContent = text
  document.querySelector('#cancel-scan').hidden = !cancellable
}

async function discover({ historyOnly = false } = {}) {
  const request = ++generation
  message()
  setScanning(true, historyOnly
    ? label('connection.checkingHistory', '寻找可用连接')
    : label('connection.scanning', '扫描局域网端口中...'))
  try {
    await action('discover', { historyOnly })
  }
  catch (error) {
    if (request === generation)
      message(String(error))
  }
  finally {
    if (request === generation)
      setScanning(false)
  }
}

async function cancelScan() {
  ++generation
  setScanning(false)
  await action('cancel')
}

async function scan() {
  const request = ++generation
  message()
  try {
    const result = await action('scan')
    if (result?.text) {
      const target = parsePairingLink(result.text)
      if (!target) throw label('scan.invalidAddress', '请输入有效的 HTTP/HTTPS 连接地址。')
      setScanning(true, label('connection.loading', '连接中…'), false)
      await action('open', { link: target.href })
    }
  }
  catch (error) {
    if (request === generation)
      message(String(error))
  }
  finally {
    if (request === generation)
      setScanning(false)
  }
}

document.querySelector('#discover').onclick = () => discover()
document.querySelector('#cancel-scan').onclick = cancelScan
document.querySelector('#scan').onclick = scan
for (const [selector, command] of [['#history', 'drawer'], ['#settings', 'settings'], ['#local-agent', 'localAgent']]) {
  document.querySelector(selector).onclick = () => {
    ++generation
    return action(command)
  }
}

window.updateLocalEntry = key => {
  const entry = document.querySelector('#local-agent')
  entry.hidden = !key
  if (key) {
    entry.dataset.i18n = key
    entry.textContent = label(key, key)
  }
}

window.applyWingmateLabels = (language, nextMessages) => {
  messages = nextMessages
  document.documentElement.lang = language
  for (const element of document.querySelectorAll('[data-i18n]'))
    element.textContent = label(element.dataset.i18n, element.textContent)
}

async function restore() {
  const startingGeneration = generation
  try {
    const recent = JSON.parse(localStorage.getItem(historyKey) ?? '[]')
    const settings = await action('restore', { recent: Array.isArray(recent) ? recent : [] })
    window.applyWingmateLabels(settings.language, settings.messages)
    localStorage.removeItem(historyKey)
    window.updateLocalEntry(settings.localEntry)
    if (settings.autoConnect && startingGeneration === generation && !scanning)
      void discover({ historyOnly: true })
  }
  catch {
    message(label('connection.storageFailed', '无法读取或保存连接记录，请检查设备存储权限和空间。'))
  }
}
void restore()
