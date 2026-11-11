import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { runInNewContext } from 'node:vm';
import { test } from 'node:test';

test('external sessions withdraw DSH feedback and fork controls without changing native sessions', async () => {
  let module;
  const window = {
    __WINGMATE_AGENTS__: [{ id: 'codex' }],
    __ModuleLoader__: { load(entry) { module = entry.factory(() => ({})); } },
  };
  runInNewContext(await readFile(new URL('../client.js', import.meta.url), 'utf8'),
    { window, URL, location: { href: 'http://localhost/?wingmate-agent=codex' } });
  const native = { getSnapshot: () => true };
  const source = () => native;
  let dispose;
  const ctx = {
    remoteAccess: { source },
    effect(install) { dispose = install(); },
    slots: { inject() {} },
  };
  module.apply(ctx);
  assert.equal(ctx.remoteAccess.source('session', 'codex:test', 'messageFeedback/put').getSnapshot(), false);
  assert.equal(ctx.remoteAccess.source('session', 'codex:test', 'session/fork').getSnapshot(), false);
  assert.equal(ctx.remoteAccess.source('session', 'dsh-session', 'messageFeedback/put'), native);
  assert.equal(ctx.remoteAccess.source('session', 'codex:test', 'session/prompt'), native);
  dispose();
  assert.equal(ctx.remoteAccess.source, source);
});
