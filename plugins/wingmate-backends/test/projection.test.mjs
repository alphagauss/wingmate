import assert from 'node:assert/strict';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { createWorkbench } from '../projection.mjs';
import { snapshot } from '../catalog.mjs';

test('ACP history, live streams, duplicate prompts and cancellation stay on one host session', async t => {
  const backend = createWorkbench('fixture', 'Fixture', () =>
    ({ command: process.execPath, args: [fileURLToPath(new URL('./acp-agent.mjs', import.meta.url))] }));
  t.after(() => backend.dispose());
  const listing = await backend.call('session/list', {});
  assert.equal(listing.items[0].sessionId, 'fixture:existing');
  const controller = new AbortController();
  const stream = backend.open('session/follow', { request: { address: { sessionId: 'fixture:existing' } } }, controller.signal);
  const initial = (await stream.next()).value;
  assert.equal(initial.cursor, initial.records.length - 1);
  assert.equal(initial.records.find(row => row.event.type === 'assistant/message').event.data.message.content[0].text, 'Historical answer');
  const prompt = { sessionId: 'fixture:existing', requestId: 'same-prompt', content: [{ type: 'text', text: 'Question' }] };
  await Promise.all([backend.call('session/prompt', { request: prompt }), backend.call('session/prompt', { request: prompt })]);
  await assert.rejects(backend.call('session/prompt', { ...prompt, requestId: 'different-prompt' }), /already running/);
  controller.abort();
  await stream.return();
  assert.equal((await backend.call('session/list', {})).items[0].running, true);
  const reconnect = backend.open('session/follow', { address: { sessionId: 'fixture:existing' } }, new AbortController().signal);
  const active = (await reconnect.next()).value;
  assert.equal(active.records.filter(row => row.event.type === 'user/message').length, 2);
  await backend.call('session/cancel', { sessionId: 'fixture:existing' });
  await new Promise(resolve => setTimeout(resolve, 30));
  const history = await backend.call('session/events', { address: { sessionId: 'fixture:existing' } });
  assert.equal(history.records.at(-1).event.data.reason.kind, 'aborted');
  assert.deepEqual(history.records.at(-1).event.data.reason.reason, { kind: 'user' });
  await reconnect.return();
  await assert.rejects(backend.call('session/prompt', { ...prompt, sessionId: 'dsh:existing' }), /does not belong/);
});

test('an empty session snapshot uses the native empty cursor and assistant baseline', () => {
  const frame = snapshot({
    id: 'fixture:new', cwd: process.cwd(), title: 'Empty', records: [], createdAt: 1, blank: true,
  });
  assert.equal(frame.cursor, -1);
  assert.equal(frame.projections.asOfSeq, -1);
  assert.deepEqual(frame.assistantStream, { revision: 0 });
});

test('an approval keeps its native option id and ignores stale answers from another view', async t => {
  const backend = createWorkbench('fixture', 'Fixture', () =>
    ({ command: process.execPath, args: [fileURLToPath(new URL('./acp-agent.mjs', import.meta.url))] }));
  t.after(() => backend.dispose());
  await backend.call('session/list', {});
  const controller = new AbortController();
  const remote = backend.remote(controller.signal);
  const frame = remote.next();
  await backend.call('session/prompt', {
    sessionId: 'fixture:existing', requestId: 'approval-test', content: [{ type: 'text', text: 'Permission' }],
  });
  let approval = (await frame).value;
  while (approval.type !== 'waterfall') approval = (await remote.next()).value;
  assert.equal(backend.answer({ eventId: approval.eventId, outcome: { kind: 'next' } }), true);
  assert.equal(backend.answer({ eventId: approval.eventId, outcome: { kind: 'result', value: 'allowed-once' } }), true);
  assert.equal(backend.answer({ eventId: approval.eventId, outcome: { kind: 'result', value: 'rejected' } }), true);
  await new Promise(resolve => setTimeout(resolve, 40));
  const history = await backend.call('session/events', { sessionId: 'fixture:existing' });
  assert.match(JSON.stringify(history.records), /\\"optionId\\":\\"once\\"/);
  controller.abort();
  await remote.return();
});
