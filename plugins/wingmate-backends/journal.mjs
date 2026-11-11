import { randomUUID } from 'node:crypto';

// DSH wire events and live assistant frames follow ds-harness-remote's
// virtual-live projection, with ACP updates as the source.
export function record(session, type, data) {
  const event = { type, seq: session.records.length, time: Date.now(), data };
  if (type.startsWith('wingmate/')) event.ignorable = true;
  if (['user/message', 'assistant/message', 'tool/result'].includes(type)) event.surfaceOp = 'append';
  const entry = { type: 'event', event };
  session.records.push(entry);
  session.events.emit('follow', entry);
  return event.seq;
}

function assistantFrame(session, type, data) {
  session.revision++;
  session.events.emit('follow', {
    type: 'assistant-stream',
    frame: { type, attemptId: session.attempt.id, revision: session.revision, ...data },
  });
}

function chunk(session, data) {
  const time = Date.now();
  const index = session.attempt.stream.length;
  session.attempt.stream.push({ type: 'chunk', time, chunk: data });
  assistantFrame(session, 'chunk', { index, time, chunk: data });
}

export function beginTurn(session, content, requestId) {
  session.turn++;
  session.step = 1;
  record(session, 'user/message', {
    id: randomUUID(), content, source: { kind: 'user', ...(requestId ? { rpcId: requestId } : {}) },
  });
  record(session, 'turn/start', { turn: session.turn });
  record(session, 'step/start', { turn: session.turn, step: session.step });
}

export function appendAssistant(session, type, text) {
  if (!session.attempt) {
    const startedAfterSeq = session.records.length - 1;
    session.attempt = { id: randomUUID(), blocks: [], startedAfterSeq, stream: [] };
    assistantFrame(session, 'start', { startedAfterSeq, turn: session.turn, step: session.step });
  }
  let block = session.attempt.blocks.at(-1);
  if (block?.type !== type) {
    if (block) chunk(session, { type: 'block-end', index: block.index, block: { type: block.type, text: block.text } });
    block = { type, index: session.attempt.blocks.length, text: '' };
    session.attempt.blocks.push(block);
    chunk(session, { type: 'block-start', index: block.index, blockType: type });
  }
  block.text += text;
  chunk(session, { type: type === 'text' ? 'text-delta' : 'reasoning-delta', index: block.index, text });
}

function finishAssistant(session, stop) {
  const attempt = session.attempt;
  if (!attempt) return;
  const last = attempt.blocks.at(-1);
  chunk(session, { type: 'block-end', index: last.index, block: { type: last.type, text: last.text } });
  chunk(session, { type: 'finish', reason: { kind: 'stop' } });
  const seq = record(session, 'assistant/message', {
    turn: session.turn, step: session.step, interrupted: stop === 'aborted',
    message: {
      id: attempt.id, role: 'assistant', content: attempt.blocks.map(({ type, text }) => ({ type, text })),
      source: { kind: 'model', provider: session.backend, model: session.model?.currentValue ?? '' },
    },
    stream: attempt.stream,
  });
  assistantFrame(session, 'end', { index: attempt.stream.length,
    outcome: { kind: 'committed', eventType: 'assistant/message', seq } });
  session.attempt = undefined;
}

export function finishTurn(session, reason) {
  finishAssistant(session, reason.kind);
  record(session, 'step/end', { turn: session.turn, step: session.step });
  record(session, 'turn/end', { turn: session.turn, reason });
}

export function assistantBaseline(session) {
  const attempt = session.attempt;
  return {
    revision: session.revision ?? 0,
    ...(attempt && { activeAttempt: {
      attemptId: attempt.id, startedAfterSeq: attempt.startedAfterSeq,
      turn: session.turn, step: session.step, nextIndex: attempt.stream.length, stream: attempt.stream.slice(),
    } }),
  };
}

export function applyUpdate(session, update) {
  switch (update.sessionUpdate) {
    case 'agent_message_chunk':
    case 'agent_thought_chunk':
      if (update.content?.type === 'text') {
        appendAssistant(session, update.sessionUpdate === 'agent_message_chunk' ? 'text' : 'reasoning', update.content.text);
      }
      break;
    case 'tool_call':
      finishAssistant(session, 'completed');
      record(session, 'tool/call', {
        turn: session.turn, step: session.step, callId: update.toolCallId,
        name: update.title, arguments: JSON.stringify(update.rawInput ?? {}),
      });
      break;
    case 'tool_call_update':
      if (update.status === 'completed' || update.status === 'failed') {
        const content = (update.content ?? []).flatMap(part => part.content?.type === 'text' ? [part.content] : []);
        record(session, 'tool/result', {
          turn: session.turn, step: session.step,
          message: {
            id: `${update.toolCallId}:result`, role: 'user',
            content: [{ type: 'tool-result', toolCallId: update.toolCallId, content, isError: update.status === 'failed' }],
            source: { kind: 'tool', callId: update.toolCallId },
          },
        });
        record(session, 'step/end', { turn: session.turn, step: session.step });
        record(session, 'step/start', { turn: session.turn, step: ++session.step });
      }
      break;
  }
}

export function replay(session, updates) {
  let user = [];
  function openTurn() {
    if (!user.length) return;
    if (session.turn) finishTurn(session, { kind: 'completed' });
    beginTurn(session, [{ type: 'text', text: user.join('') }]);
    user = [];
  }
  for (const update of updates) {
    if (update.sessionUpdate === 'user_message_chunk') {
      if (update.content.type === 'text') user.push(update.content.text);
    } else if (['agent_message_chunk', 'agent_thought_chunk', 'tool_call', 'tool_call_update'].includes(update.sessionUpdate)) {
      if (user.length) openTurn();
      else if (!session.turn) {
        session.turn = 1;
        session.step = 1;
        record(session, 'turn/start', { turn: 1 });
        record(session, 'step/start', { turn: 1, step: 1 });
      }
      applyUpdate(session, update);
    }
  }
  openTurn();
  if (session.turn) finishTurn(session, { kind: 'completed' });
}
