import assert from 'node:assert/strict';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { diagnosticsPath, installDiagnostics } from '../diagnostics.ts';
import { createWorkbench } from '../projection.mjs';

test('passive status does not connect, while a failed check can be retried', async t => {
  let command;
  const backend = createWorkbench('fixture', 'Fixture', () => {
    if (!command) throw new Error('Adapter not installed');
    return { command: process.execPath, args: command };
  });
  t.after(() => backend.dispose());
  let invoke;
  installDiagnostics({ connection: { fetch: { register(route) {
    assert.equal(route.path, diagnosticsPath);
    invoke = route.fetch;
  } } } }, new Map([['fixture', backend]]));
  const url = new URL(diagnosticsPath, 'http://localhost');
  const check = id => invoke(new Request(url, {
    method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ id }),
  }));
  const state = async () => (await (await invoke(new Request(url))).json())[0];

  assert.equal((await state()).adapterAvailable, false);
  command = ['-e', 'process.exit(1)'];
  assert.equal((await state()).adapterAvailable, true);
  assert.equal((await check('unknown')).status, 400);
  assert.equal((await state()).connection, 'unchecked');
  assert.equal((await (await check('fixture')).json())[0].connection, 'failed');

  command = [fileURLToPath(new URL('./acp-agent.mjs', import.meta.url))];
  const checked = await Promise.all([check('fixture'), check('fixture')].map(async result => (await result).json()));
  assert.ok(checked.every(([result]) => result.connection === 'connected'));
  assert.equal((await state()).detail, undefined);
});
