import { setTimeout } from 'node:timers/promises';
import { installWorkbench } from './workbench.mjs';
import { projections, snapshot, summary, workspace } from './catalog.mjs';

export { inject } from './workbench.mjs';

// Copied into a temporary plugin as host.mjs for browser integration checks.
export async function apply(ctx) {
  const time = Date.now();
  const session = {
    id: 'fixture:conversation', title: 'Wingmate fixture conversation', cwd: process.cwd(),
    createdAt: time, updatedAt: time, running: false, blank: false, records: [],
  };
  const project = workspace('fixture', session.cwd, new Date(time).toISOString());
  project.title = 'Wingmate fixture workspace';
  project.sessionIds.push(session.id);
  const calls = {
    'session/list': () => ({ items: [summary(session)] }),
    'session/create': () => ({ sessionId: session.id }),
    'session/projections': () => ({ kind: 'sequenced', ...projections(session) }),
    'session/events': () => ({ records: [], hasMore: false }),
    'session/page': () => ({ records: [], hasMore: false }),
    'workspace/initializeDefault': () => project,
  };
  const streams = {
    'workspace/follow': { type: 'baseline', value: { items: [project], archivedSessionIds: [], pinnedSessionIds: [] } },
    'session/control': { type: 'baseline', value: { projections: { [session.id]: projections(session) } } },
    'session/follow': snapshot(session),
  };
  await installWorkbench(ctx, new Map([['fixture', {
    label: 'ACP Fixture',
    handles: endpoint => Object.hasOwn(calls, endpoint) || Object.hasOwn(streams, endpoint),
    call: endpoint => calls[endpoint](),
    async *open(endpoint, _args, signal) {
      yield streams[endpoint];
      if (endpoint === 'session/follow') {
        await setTimeout(150, undefined, { signal });
        yield { type: 'event', event: {
          type: 'user/message', seq: 0, time: Date.now(), surfaceOp: 'append',
          data: { content: [{ type: 'text', text: 'Wingmate fixture live update' }], source: { kind: 'user' } },
        } };
      }
      await setTimeout(2_147_483_647, undefined, { signal, ref: false });
    },
  }]]));
}
