import { readFile } from 'node:fs/promises';

export const inject = ['webServer', 'typertGateway'];

export async function installWorkbench(ctx, backends) {
  const script = await readFile(new URL('./transport.js', import.meta.url), 'utf8');
  ctx.on('webserver/index-inject', (table, peer) => {
    if (peer?.principal.kind !== 'owner') return;
    table.push({
      kind: 'global', name: '__WINGMATE_AGENTS__',
      value: [...backends].map(([id, backend]) => ({ id, label: backend.label })),
    });
    table.push({ kind: 'script', placement: 'head', text: script });
  });

  // The 0.2.2-rc.1 gateway has separate unary and stream carriers.
  // Adapted from ds-harness-remote's TypertGatewaySwitch (see LICENSE.remote).
  const gateway = ctx.typertGateway;
  const { dispatchRpc, openWireStream } = gateway;
  if (typeof dispatchRpc !== 'function' || typeof openWireStream !== 'function') {
    throw new Error('Unsupported DSH workbench gateway');
  }
  ctx.effect(() => {
    gateway.dispatchRpc = async function (endpoint, payload, signal, peer) {
      const { wingmateAgent, ...native } = payload ?? {};
      const backend = backends.get(wingmateAgent);
      if (!backend || peer.principal.kind !== 'owner') {
        return dispatchRpc.call(this, endpoint, wingmateAgent === undefined ? payload : native, signal, peer);
      }
      try {
        peer.signal.throwIfAborted();
        signal.throwIfAborted();
        if (endpoint === '$events/result' && backend.answer?.(native.args ?? native)) return { ok: true };
        if (endpoint === 'permissionPresets/catalog' && backend.permissionOptions) {
          const result = await dispatchRpc.call(this, endpoint, native, signal, peer);
          return result.ok ? { ...result, value: { ...result.value, options: backend.permissionOptions() } } : result;
        }
        if (!backend.handles(endpoint)) return dispatchRpc.call(this, endpoint, native, signal, peer);
        return { ok: true, value: await backend.call(endpoint, native.args ?? native, signal) };
      } catch (error) {
        return { ok: false, error: { code: 'gateway/internal', message: String(error?.message ?? error), details: {} } };
      }
    };
    gateway.openWireStream = async function (endpoint, payload, uplink, peer, signal, ...rest) {
      const { wingmateAgent, ...native } = payload ?? {};
      const backend = backends.get(wingmateAgent);
      if (!backend || peer.principal.kind !== 'owner') {
        return openWireStream.call(this, endpoint, wingmateAgent === undefined ? payload : native, uplink, peer, signal, ...rest);
      }
      peer.signal.throwIfAborted();
      signal.throwIfAborted();
      const lifetime = AbortSignal.any([signal, peer.signal]);
      if (endpoint === '$events' && backend.remote) {
        const source = await openWireStream.call(this, endpoint, native, uplink, peer, signal, ...rest);
        return mergeEvents(source, backend.remote(lifetime));
      }
      if (!backend.handles(endpoint)) return openWireStream.call(this, endpoint, native, uplink, peer, signal, ...rest);
      return backend.open(endpoint, native.args ?? native, lifetime);
    };
    return () => {
      gateway.dispatchRpc = dispatchRpc;
      gateway.openWireStream = openWireStream;
    };
  }, 'wingmate-backends: Web routing');
}

async function *mergeEvents(native, projected) {
  const sources = [native[Symbol.asyncIterator](), projected[Symbol.asyncIterator]()];
  const read = index => sources[index].next().then(
    result => ({ index, result }), error => ({ index, error }));
  const pending = sources.map((_, index) => read(index));
  try {
    while (true) {
      const { index, result, error } = await Promise.race(pending);
      if (error) throw error;
      if (result.done) return;
      pending[index] = read(index);
      // Each workbench has its own session catalog. Host-wide settings events
      // still travel through the native Remote Event generation.
      if (index === 0 && (result.value.event?.startsWith('api-session/')
        || result.value.agentId)) continue;
      yield result.value;
    }
  } finally {
    for (const source of sources) void source.return?.().catch(() => {});
    await Promise.allSettled(pending);
  }
}
