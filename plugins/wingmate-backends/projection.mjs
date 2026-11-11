import { EventEmitter, on } from 'node:events';
import { randomUUID } from 'node:crypto';
import { realpath, stat } from 'node:fs/promises';
import { homedir } from 'node:os';
import { isAbsolute, normalize } from 'node:path';
import { acpConnection } from './acp.mjs';
import { projections, snapshot, summary, workspace } from './catalog.mjs';
import { applyUpdate, beginTurn, finishTurn, record, replay } from './journal.mjs';

export function createWorkbench(id, label, resolveCommand, index) {
  const sessions = new Map();
  const projects = new Map();
  const events = new EventEmitter();
  events.setMaxListeners(0);
  const approvals = new Map();
  const loading = new Map();
  let refreshing;
  let catalog;
  let permissionOptions = [];
  let indexed;
  let editing = Promise.resolve();

  function loadIndex() {
    return indexed ??= Promise.resolve(index?.read() ?? []).then(items => {
      for (const item of items) ensureProject(item.path).title = item.title;
    });
  }

  const connection = acpConnection(resolveCommand, {
    update({ sessionId, update }) {
      const session = sessions.get(`${id}:${sessionId}`);
      if (!session) return;
      const history = loading.get(session.id);
      if (history) history.push(update);
      else {
        applyUpdate(session, update);
        if (update.sessionUpdate === 'config_option_update') {
          configure(session, update);
          publish(session);
        }
        if (update.sessionUpdate === 'session_info_update' && update.title && session.title !== update.title) {
          session.title = update.title;
          record(session, 'wingmate/metadata', { title: session.title });
          publish(session);
        }
      }
    },
    permission(params, signal) {
      const session = sessions.get(`${id}:${params.sessionId}`);
      if (!session) return { outcome: { outcome: 'cancelled' } };
      const eventId = `wingmate-acp:${randomUUID()}`;
      return new Promise(resolve => {
        const pending = {
          session, options: params.options,
          frame: {
            type: 'waterfall', event: 'approval/request', eventId, agentId: session.id,
            request: { toolName: params.toolCall.title, callId: params.toolCall.toolCallId,
              reason: params.toolCall.title },
          },
          settle(outcome) {
            if (!approvals.delete(eventId)) return;
            signal.removeEventListener('abort', cancel);
            events.emit('remote', { type: 'cancel', eventId });
            resolve({ outcome });
          },
        };
        function cancel() { pending.settle({ outcome: 'cancelled' }); }
        approvals.set(eventId, pending);
        signal.addEventListener('abort', cancel, { once: true });
        if (signal.aborted) cancel();
        else events.emit('remote', pending.frame);
      });
    },
    disconnected() {
      for (const session of sessions.values()) {
        session.loaded = false;
      }
    },
  });

  function register(info) {
    const key = `${id}:${info.sessionId}`;
    let session = sessions.get(key);
    if (!session) {
      session = {
        id: key, nativeId: info.sessionId, backend: id, cwd: normalize(info.cwd),
        title: info.title ?? label, createdAt: Date.now(), updatedAt: Date.parse(info.updatedAt) || Date.now(),
        blank: false, running: false, loaded: false, records: [], turn: 0, step: 1, revision: 0,
        events: new EventEmitter(), requests: new Set(),
      };
      session.events.setMaxListeners(0);
      sessions.set(key, session);
    } else if (!session.running) {
      session.title = info.title ?? session.title;
      session.updatedAt = Math.max(session.updatedAt, Date.parse(info.updatedAt) || 0);
    }
    const project = ensureProject(info.cwd);
    if (!project.sessionIds.includes(key)) project.sessionIds.push(key);
    return session;
  }

  function ensureProject(path) {
    const item = workspace(id, path, new Date().toISOString());
    if (!projects.has(item.workspaceId)) projects.set(item.workspaceId, item);
    return projects.get(item.workspaceId);
  }

  async function current(args) {
    const key = args.sessionId ?? args.agentId ?? args.address?.sessionId;
    if (typeof key !== 'string' || !key.startsWith(`${id}:`)) {
      throw new Error('The session does not belong to the selected Agent');
    }
    if (!sessions.has(key)) await refresh();
    const session = sessions.get(key);
    if (!session) throw new Error('The session does not belong to the selected Agent');
    return session;
  }

  async function refresh() {
    if (refreshing) return refreshing;
    refreshing = (async () => {
      const { agent, capabilities } = await connection.connect();
      await loadIndex();
      if (capabilities.sessionCapabilities?.list) {
        let cursor;
        do {
          const response = await agent.request('session/list', cursor ? { cursor } : {}, { signal: AbortSignal.timeout(15_000) });
          for (const item of response.sessions) register(item);
          cursor = response.nextCursor;
        } while (cursor);
      }
    })();
    try { await refreshing; } finally { refreshing = undefined; }
  }

  function configure(session, response) {
    const previous = JSON.stringify([session.model, session.mode]);
    if (response.configOptions) session.options = response.configOptions;
    session.model = session.options?.find(option => option.category === 'model');
    session.mode = session.options?.find(option => option.category === 'mode');
    if (session.mode) {
      permissionOptions = session.mode.options.map(option => ({
        value: option.value, name: option.name, description: option.description,
      }));
      events.emit('remote', { type: 'emit', event: 'permission-presets/catalog-changed', args: [] });
    }
    if (session.model) {
      catalog = {
        default: { provider: id, model: session.model.currentValue },
        routableProviders: [id], failures: [],
        groups: [{ id, name: label, models: session.model.options.map(option => ({ id: option.value, name: option.name })) }],
      };
      events.emit('remote', { type: 'emit', event: 'llm/adapters-updated', args: [] });
    }
    if (JSON.stringify([session.model, session.mode]) !== previous) record(session, 'wingmate/metadata', {});
  }

  function workspaceBaseline() {
    return { items: [...projects.values()], archivedSessionIds: [], pinnedSessionIds: [] };
  }

  function publish(session) {
    events.emit('control', { type: 'projection', sessionId: session.id,
      seq: session.records.at(-1)?.event.seq ?? -1,
      changes: Object.entries(projections(session).values).map(([key, val]) => ({ key, val })) });
    events.emit('remote', { type: 'emit', event: 'api-session/status', args: [session.id, session.running] });
  }

  async function load(session) {
    if (session.loaded) return;
    if (session.loading) return session.loading;
    session.loading = (async () => {
      const { agent, capabilities } = await connection.connect();
      if (!capabilities.loadSession) throw new Error('This Agent cannot replay session history');
      const updates = [];
      loading.set(session.id, updates);
      try {
        const response = await agent.request('session/load', {
          sessionId: session.nativeId, cwd: session.cwd, mcpServers: [],
        }, { signal: AbortSignal.timeout(30_000) });
        session.records = [];
        session.turn = 0;
        session.attempt = undefined;
        replay(session, updates);
        configure(session, response ?? {});
        session.blank = session.records.length === 0;
        session.loaded = true;
      } finally { loading.delete(session.id); }
    })();
    try { await session.loading; } finally { session.loading = undefined; }
  }

  async function create(args) {
    const project = projects.get(args.workspaceId);
    const requested = args.cwd ?? project?.path;
    if (!requested || !isAbsolute(requested)) throw new Error('Select an existing host directory');
    const cwd = await realpath(requested);
    if (!(await stat(cwd)).isDirectory()) throw new Error('Select an existing host directory');
    const existing = sessions.get(args.sessionId);
    if (existing?.cwd === cwd && existing.blank && !existing.running) return { sessionId: existing.id };
    const { agent } = await connection.connect();
    const response = await agent.request('session/new', { cwd, mcpServers: [] }, { signal: AbortSignal.timeout(30_000) });
    const session = register({ sessionId: response.sessionId, cwd });
    session.blank = true;
    session.loaded = true;
    configure(session, response);
    events.emit('workspace', { type: 'replace', value: workspaceBaseline() });
    events.emit('remote', { type: 'emit', event: 'api-session/added', args: [summary(session)] });
    return { sessionId: session.id };
  }

  async function prompt(args) {
    const session = await current(args);
    await load(session);
    const { agent } = await connection.connect();
    if (session.requests.has(args.requestId)) return { accepted: true };
    if (session.running) throw new Error('This Agent session is already running');
    if (typeof args.requestId !== 'string' || !args.requestId || !args.content?.length
      || args.content.some(part => part.type !== 'text') || !args.content.some(part => part.text.trim())) {
      throw new Error('This workbench currently accepts text prompts');
    }
    session.running = true;
    session.blank = false;
    session.updatedAt = Date.now();
    session.requests.add(args.requestId);
    beginTurn(session, args.content, args.requestId);
    publish(session);
    // The host owns the turn. A page closing only cancels its subscriptions.
    void agent.request('session/prompt', { sessionId: session.nativeId, prompt: args.content }).then(
      result => finishTurn(session, result.stopReason === 'cancelled'
        ? { kind: 'aborted', reason: { kind: 'user' } } : { kind: 'completed' }),
      error => finishTurn(session, { kind: 'error', error: { code: 'ACP_PROMPT_FAILED', message: String(error?.message ?? error) } }),
    ).finally(() => {
      session.running = false;
      for (const pending of approvals.values()) {
        if (pending.session === session) pending.settle({ outcome: 'cancelled' });
      }
      publish(session);
    });
    return { accepted: true };
  }

  const calls = {
    async 'session/list'() { await refresh(); return { items: [...sessions.values()].map(summary) }; },
    async 'workspace/initializeDefault'() {
      await refresh();
      return { workspace: projects.values().next().value ?? ensureProject(homedir()) };
    },
    async 'workspace/create'({ path }) {
      if (!isAbsolute(path) || !(await stat(path)).isDirectory()) throw new Error('Select an existing host directory');
      path = await realpath(path);
      const candidate = workspace(id, path, new Date().toISOString());
      const created = !projects.has(candidate.workspaceId);
      const project = projects.get(candidate.workspaceId) ?? candidate;
      if (created) await index?.write([...projects.values(), project]);
      projects.set(project.workspaceId, project);
      events.emit('workspace', { type: 'replace', value: workspaceBaseline() });
      return { workspace: project, created };
    },
    async 'workspace/rename'({ workspaceId, title }) {
      const project = projects.get(workspaceId);
      if (!project || typeof title !== 'string' || !title.trim()) throw new Error('A workspace and a non-empty title are required');
      const renamed = { ...project, title: title.trim() };
      await index?.write([...projects.values()].map(item => item === project ? renamed : item));
      projects.set(workspaceId, renamed);
      events.emit('workspace', { type: 'replace', value: workspaceBaseline() });
      return { workspace: renamed };
    },
    'session/create': create,
    'session/prompt': prompt,
    async 'session/cancel'(args) {
      const session = await current(args);
      const { agent } = await connection.connect();
      await agent.notify('session/cancel', { sessionId: session.nativeId });
      return { accepted: true };
    },
    async 'session/projections'(args) {
      const session = await current(args);
      await load(session);
      return { kind: 'sequenced', ...projections(session) };
    },
    async 'session/events'(args) {
      const session = await current(args);
      await load(session);
      const records = session.records.filter(({ event }) =>
        event.seq <= (args.throughSeq ?? Infinity) && event.seq < (args.beforeSeq ?? Infinity));
      return { records, hasMore: false, cursor: records.at(-1)?.event.seq ?? -1 };
    },
    'session/page'(args) { return calls['session/events'](args); },
    'session/modelCatalog'() {
      return catalog ?? { default: { provider: id, model: '' }, routableProviders: [], groups: [], failures: [] };
    },
    async 'session/selectModel'(args) {
      const session = await current(args);
      await load(session);
      if (args.provider !== id || !session.model) throw new Error('This Agent does not expose model selection');
      const { agent } = await connection.connect();
      configure(session, await agent.request('session/set_config_option', {
        sessionId: session.nativeId, configId: session.model.id, value: args.model,
      }));
      publish(session);
      return { selected: { provider: id, model: session.model.currentValue } };
    },
    async 'session/selectPermission'(args) {
      const session = await current(args);
      await load(session);
      if (!session.mode) throw new Error('This Agent does not expose permission modes');
      const { agent } = await connection.connect();
      configure(session, await agent.request('session/set_config_option', {
        sessionId: session.nativeId, configId: session.mode.id, value: args.value,
      }));
      publish(session);
      return projections(session).values.permissions;
    },
    'session/search'() { throw new Error('This ACP workbench does not support full-text session search'); },
    async 'session/rename'(args) {
      const session = await current(args);
      const { agent, capabilities } = await connection.connect();
      if (!capabilities.sessionCapabilities?.title) throw new Error('This Agent does not support session titles');
      const result = await agent.request('session/set_title', { sessionId: session.nativeId, title: args.title });
      session.title = result?.title ?? args.title;
      const seq = record(session, 'wingmate/metadata', { title: session.title });
      publish(session);
      return { title: session.title, seq };
    },
    'session/initializeDefaultModel'() {},
    'commands/list'() {
      return [{ name: 'permission', description: 'Select the Agent permission mode', input: { hint: '<mode>' } }];
    },
    async 'commands/execute'(args) {
      const match = /^\/permission\s+(\S+)\s*$/.exec(args.line);
      if (!match) throw new Error('This Agent command is unavailable');
      await calls['session/selectPermission']({ ...args, value: match[1] });
      return { commandId: randomUUID(), result: { kind: 'success', text: match[1] } };
    },
    'skills/list'() { return { items: [] }; },
  };

  async function *subscribe(emitter, name, signal, baseline) {
    const iterator = on(emitter, name, { signal });
    try {
      if (baseline) yield baseline;
      for await (const [item] of iterator) yield item;
    } finally { await iterator.return(); }
  }

  return {
    label,
    permissionOptions: () => permissionOptions,
    handles(endpoint) {
      return /^(workspace|session|commands|skills|messageFeedback)\//.test(endpoint)
        && !['session/openWorkspacePath', 'session/canOpenWorkspacePath', 'session/workspacePathApplications'].includes(endpoint);
    },
    async call(endpoint, args) {
      const method = calls[endpoint];
      if (!method) throw new Error(`${label} does not support ${endpoint}`);
      if (endpoint === 'workspace/create' || endpoint === 'workspace/rename') {
        const operation = editing.then(async () => { await loadIndex(); return method(args.request ?? args); });
        editing = operation.catch(() => {});
        return operation;
      }
      return method(args.request ?? args);
    },
    async *open(endpoint, args, signal) {
      if (endpoint === 'workspace/follow') {
        await refresh();
        yield *subscribe(events, 'workspace', signal, { type: 'baseline', value: workspaceBaseline() });
      } else if (endpoint === 'session/control') {
        yield *subscribe(events, 'control', signal, { type: 'baseline',
          value: { projections: Object.fromEntries([...sessions.values()].map(session => [session.id, projections(session)])) } });
      } else if (endpoint === 'session/follow') {
        const session = await current(args.request ?? args);
        await load(session);
        const stream = subscribe(session.events, 'follow', signal, snapshot(session));
        for (const pending of approvals.values()) {
          if (pending.session === session) queueMicrotask(() => events.emit('remote', pending.frame));
        }
        yield *stream;
      } else throw new Error(`${label} does not support ${endpoint}`);
    },
    remote(signal) {
      return subscribe(events, 'remote', signal);
    },
    answer(args) {
      const pending = approvals.get(args.eventId);
      if (!pending) return typeof args.eventId === 'string' && args.eventId.startsWith('wingmate-acp:');
      const value = args.outcome?.kind === 'result' ? args.outcome.value : undefined;
      if (value === undefined) return true;
      const kind = value === 'allowed-once' ? 'allow_once' : 'reject_once';
      const option = pending.options.find(item => item.kind === kind);
      pending.settle(option ? { outcome: 'selected', optionId: option.optionId } : { outcome: 'cancelled' });
      return true;
    },
    async dispose() {
      for (const pending of approvals.values()) pending.settle({ outcome: 'cancelled' });
      await connection.dispose();
    },
  };
}
