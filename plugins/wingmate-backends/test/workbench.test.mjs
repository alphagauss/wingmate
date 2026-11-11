import assert from 'node:assert/strict';
import { test } from 'node:test';
import { installWorkbench } from '../workbench.mjs';

test('the authenticated carrier unwraps ACP approval results and preserves native calls', async () => {
  let answer;
  let calls = 0;
  const gateway = {
    async dispatchRpc(...args) { calls++; return { ok: true, value: args[1] }; },
    async *openWireStream() {},
  };
  const original = gateway.dispatchRpc;
  let dispose;
  const ctx = { typertGateway: gateway, on() {}, effect(install) { dispose = install(); } };
  await installWorkbench(ctx, new Map([['fixture', {
    label: 'Fixture', handles: endpoint => endpoint === 'session/prompt',
    call: () => 'projected', answer: args => { answer = args; return true; },
  }]]));
  const signal = new AbortController().signal;
  const peer = { principal: { kind: 'owner' }, signal };
  const reply = { eventId: 'wingmate-acp:test', outcome: { kind: 'result', value: 'allowed-once' } };
  await gateway.dispatchRpc('$events/result', { args: reply, wingmateAgent: 'fixture' }, signal, peer);
  assert.deepEqual(answer, reply);
  assert.equal(calls, 0);
  assert.equal((await gateway.dispatchRpc('session/prompt', { args: {}, wingmateAgent: 'fixture' }, signal, peer)).value, 'projected');
  const result = await gateway.dispatchRpc('settings/describe', { args: {}, wingmateAgent: 'fixture' }, signal, peer);
  assert.deepEqual(result.value, { args: {} });
  await gateway.dispatchRpc('session/prompt', { args: {}, wingmateAgent: 'fixture' }, signal,
    { ...peer, principal: { kind: 'viewer' } });
  assert.equal(calls, 2);
  dispose();
  assert.equal(gateway.dispatchRpc, original);
});
