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
      if (!backend || peer.principal.kind !== 'owner' || !backend.handles(endpoint)) {
        return dispatchRpc.call(this, endpoint, wingmateAgent === undefined ? payload : native, signal, peer);
      }
      try {
        peer.signal.throwIfAborted();
        signal.throwIfAborted();
        return { ok: true, value: await backend.call(endpoint, native.args ?? native, signal) };
      } catch (error) {
        return { ok: false, error: { code: 'gateway/internal', message: String(error?.message ?? error), details: {} } };
      }
    };
    gateway.openWireStream = function (endpoint, payload, uplink, peer, signal, ...rest) {
      const { wingmateAgent, ...native } = payload ?? {};
      const backend = backends.get(wingmateAgent);
      if (!backend || peer.principal.kind !== 'owner' || !backend.handles(endpoint)) {
        return openWireStream.call(this, endpoint, wingmateAgent === undefined ? payload : native, uplink, peer, signal, ...rest);
      }
      peer.signal.throwIfAborted();
      signal.throwIfAborted();
      return backend.open(endpoint, native.args ?? native, signal);
    };
    return () => {
      gateway.dispatchRpc = dispatchRpc;
      gateway.openWireStream = openWireStream;
    };
  }, 'wingmate-backends: Web routing');
}
