// Adapted from deepseek-harness-desktop apps/native/src/utils/bridge-protocol.ts.
export function parsePairingLink(raw, manual = false) {
  if (raw.length > 8192)
    return null
  let text = raw.trim()
  if (text.startsWith('{')) {
    try {
      const data = JSON.parse(text)
      if (typeof data?.url !== 'string')
        return null
      text = data.url.trim()
    }
    catch {
      return null
    }
  }
  if (/^[\w.-]+:\d+(?:[/?].*)?$/.test(text))
    text = `http://${text}`
  else if (manual && !text.includes('://'))
    text = `https://${text}`
  if (!/^https?:\/\/[^/]/i.test(text) || /[\s\\\x00-\x1f\x7f#]/.test(text))
    return null
  try {
    const url = new URL(text)
    if (url.username || url.password || url.pathname !== '/')
      return null
    const ipv4 = /^(?:\d{1,3}\.){3}\d{1,3}$/.test(url.hostname)
    if (url.hostname.startsWith('[') || (url.protocol === 'http:') !== ipv4)
      return null
    if (url.search && !/^\?auth=[^&]+$/.test(url.search))
      return null
    return url
  }
  catch {
    return null
  }
}
