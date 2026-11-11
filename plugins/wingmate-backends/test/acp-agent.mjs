import { Readable, Writable } from 'node:stream';
import { AgentApp, ndJsonStream } from '@agentclientprotocol/sdk';

const options = [{
  id: 'model', category: 'model', name: 'Model', type: 'select', currentValue: 'test-model',
  options: [{ value: 'test-model', name: 'Test model' }],
}];
let pending;
const app = new AgentApp()
  .onRequest('initialize', () => ({
    protocolVersion: 1, agentCapabilities: { loadSession: true, sessionCapabilities: { list: {} } },
  }))
  .onRequest('session/list', () => ({ sessions: [{ sessionId: 'existing', cwd: process.cwd(), title: 'Existing history' }] }))
  .onRequest('session/new', () => ({ sessionId: 'new-session', configOptions: options }))
  .onRequest('session/load', async ({ params, client }) => {
    for (const [sessionUpdate, text] of [['user_message_chunk', 'Historical question'], ['agent_message_chunk', 'Historical answer']]) {
      await client.notify('session/update', { sessionId: params.sessionId, update: {
        sessionUpdate, content: { type: 'text', text },
      } });
    }
    return { configOptions: options };
  })
  .onRequest('session/prompt', async ({ params, client }) => {
    if (params.prompt[0]?.text === 'Permission') {
      const decision = await client.request('session/request_permission', {
        sessionId: params.sessionId,
        toolCall: { toolCallId: 'tool-1', title: 'Test approval' },
        options: [{ optionId: 'once', name: 'Allow once', kind: 'allow_once' }],
      });
      await client.notify('session/update', { sessionId: params.sessionId, update: {
        sessionUpdate: 'agent_message_chunk', content: { type: 'text', text: JSON.stringify(decision) },
      } });
      return { stopReason: 'end_turn' };
    }
    await client.notify('session/update', { sessionId: params.sessionId, update: {
      sessionUpdate: 'agent_message_chunk', content: { type: 'text', text: 'Live answer' },
    } });
    return new Promise(resolve => {
      pending = resolve;
      setTimeout(() => { resolve({ stopReason: 'end_turn' }); pending = undefined; }, 200);
    });
  })
  .onNotification('session/cancel', () => pending?.({ stopReason: 'cancelled' }));
app.connect(ndJsonStream(Writable.toWeb(process.stdout), Readable.toWeb(process.stdin)));
