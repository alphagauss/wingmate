import assert from 'node:assert/strict'
import { test } from 'node:test'
import { parsePairingLink } from '../android/app/src/main/assets/launch/pairing-link.js'

test('opens LAN and public Wingmate pairing links with the original code', () => {
  const lan = 'http://192.168.1.8:3210/?auth=lan-code'
  const publicLink = 'https://wingmate-test.trycloudflare.com/?auth=public-code'
  assert.equal(parsePairingLink(JSON.stringify({ url: lan })).href, lan)
  assert.equal(parsePairingLink('192.168.1.8:3210/?auth=lan-code').href, lan)
  assert.equal(parsePairingLink(` ${publicLink}\n`, true).href, publicLink)
  assert.equal(parsePairingLink(new URL(publicLink).host, true).href, new URL(publicLink).origin + '/')
})

test('rejects unsupported targets and ambiguous pairing links before opening the WebView', () => {
  for (const link of [
    'javascript:alert(1)',
    'file:///sdcard/a.html',
    'http://example.com/?auth=abc',
    'https://192.168.1.8/?auth=abc',
    'http://192.168.1.8/elsewhere?auth=abc',
    'https://example.com/?token=abc',
    'https://example.com/?auth=',
    'https://example.com/?auth=abc&auth=xyz',
    'https://example.com/?auth=abc#other',
    'https://user:pass@example.com/',
    '{"url":null}',
  ]) {
    assert.equal(parsePairingLink(link, true), null, link)
  }
})
