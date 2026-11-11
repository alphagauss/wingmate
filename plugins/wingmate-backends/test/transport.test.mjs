import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { runInNewContext } from 'node:vm';
import { test } from 'node:test';

test('each page restores its backend selection and marks both official carriers', async () => {
  class Storage {
    values = new Map();
    getItem(key) { return this.values.get(key) ?? null; }
    setItem(key, value) { this.values.set(key, value); }
  }
  const sent = [];
  const storage = new Storage();
  const globals = {
    URL, Storage, localStorage: storage, sessionStorage: new Storage(),
    location: { href: 'http://localhost/?wingmate-agent=codex' },
    __WINGMATE_AGENTS__: [{ id: 'codex' }],
    fetch(input, options) { sent.push(JSON.parse(options.body)); return Promise.resolve({}); },
    WebSocket: class { send(data) { sent.push(JSON.parse(data)); } },
  };
  await globals.localStorage.setItem('ordinary-setting', 'original');
  runInNewContext(await readFile(new URL('../transport.js', import.meta.url), 'utf8'), globals);
  globals.localStorage.setItem('dsh.sessions.current', 'external-session');
  assert.equal(globals.localStorage.getItem('dsh.sessions.current'), 'external-session');
  assert.equal(globals.sessionStorage.getItem('wingmate.current.codex'), 'external-session');
  assert.equal(globals.localStorage.getItem('ordinary-setting'), 'original');
  globals.__DSH_TRANSPORT__.fetch('api/session/prompt', { body: JSON.stringify({ payload: { args: {} } }) });
  const socket = globals.__DSH_TRANSPORT__.createWebSocket('ws://localhost/api/remote.mux');
  socket.send(JSON.stringify({ type: 'open', endpoint: 'session/follow', payload: { args: {} } }));
  assert.deepEqual(sent.map(message => message.payload.wingmateAgent), ['codex', 'codex']);
  socket.send(JSON.stringify({ type: 'cancel', streamId: 'own-stream' }));
  assert.equal(sent.at(-1).wingmateAgent, undefined);
});
