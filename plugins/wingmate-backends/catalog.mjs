// Adapted from ds-harness-remote virtual-catalog.ts and virtual-harness.ts.
import { assistantBaseline } from './journal.mjs';

export function workspace(backend, path, time) {
  return {
    workspaceId: `${backend}:cwd:${encodeURIComponent(path)}`,
    path,
    title: path.split(/[\\/]/).filter(Boolean).at(-1) || path,
    sessionIds: [],
    createdAt: time,
    updatedAt: time,
  };
}

export function summary(session) {
  return {
    sessionId: session.id,
    agentAvailable: true,
    running: session.running,
    blank: session.blank,
    cwd: session.cwd,
    updatedAt: session.updatedAt,
    projections: { kind: 'sequenced', ...projections(session) },
  };
}

export function projections(session) {
  return {
    asOfSeq: session.records.at(-1)?.event.seq ?? -1,
    values: {
      title: session.title,
      sessionListMetadata: { blank: session.blank, lastPromptAt: session.blank ? null : session.updatedAt },
      ...(session.model && { modelSelection: {
        lastUsed: null, next: { provider: session.backend, model: session.model.currentValue },
      } }),
      ...(session.mode && { permissions: {
        currentValue: session.mode.currentValue,
      } }),
    },
  };
}

export function snapshot(session) {
  return {
    type: 'snapshot',
    header: { version: 3, id: session.id, createdAt: session.createdAt, cwd: session.cwd, isSeeded: false },
    cursor: session.records.at(-1)?.event.seq ?? -1,
    records: session.records.slice(),
    hasMore: false,
    projections: projections(session),
    assistantStream: assistantBaseline(session),
  };
}
