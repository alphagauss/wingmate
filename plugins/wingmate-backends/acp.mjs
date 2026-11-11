import { spawn } from 'node:child_process';
import { Readable, Writable } from 'node:stream';
import { ClientApp, ndJsonStream } from '@agentclientprotocol/sdk';
import { once } from 'node:events';
import { setTimeout } from 'node:timers/promises';

export function acpConnection(resolveCommand, { update, permission, disconnected }) {
  let opening;
  let child;
  let connection;
  const lifetime = new AbortController();

  async function start() {
    lifetime.signal.throwIfAborted();
    const { command, args, env } = resolveCommand();
    const processHandle = spawn(command, args, { env, windowsHide: true, stdio: ['pipe', 'pipe', 'ignore'] });
    child = processHandle;
    await new Promise((resolve, reject) => {
      processHandle.once('spawn', resolve);
      processHandle.once('error', reject);
    });
    if (lifetime.signal.aborted) {
      processHandle.kill();
      lifetime.signal.throwIfAborted();
    }
    const app = new ClientApp()
      .onNotification('session/update', ({ params }) => update(params))
      .onRequest('session/request_permission', ({ params, signal }) => permission(params, signal));
    const stream = ndJsonStream(Writable.toWeb(processHandle.stdin), Readable.toWeb(processHandle.stdout));
    const client = app.connect(stream);
    connection = client;
    void client.closed.then(() => {
      if (connection !== client) return;
      connection = undefined;
      opening = undefined;
      processHandle.stdin.end();
      disconnected();
    });
    try {
      const { protocolVersion, agentCapabilities = {} } = await client.agent.request('initialize', {
        protocolVersion: 1, clientCapabilities: {}, clientInfo: { name: 'wingmate', version: '0.1.0' },
      }, { signal: AbortSignal.any([lifetime.signal, AbortSignal.timeout(15_000)]) });
      if (protocolVersion !== 1) throw new Error('Agent does not support ACP v1');
      return { agent: client.agent, capabilities: agentCapabilities };
    } catch (error) {
      client.close();
      processHandle.kill();
      throw error;
    }
  }

  return {
    async connect() {
      const pending = opening ??= start();
      try {
        return await pending;
      } catch (error) {
        if (opening === pending) opening = undefined;
        throw error;
      }
    },
    async dispose() {
      lifetime.abort();
      connection?.close();
      child?.stdin.end();
      if (child && child.exitCode === null && child.signalCode === null) {
        await Promise.race([once(child, 'close'), setTimeout(2000, undefined, { ref: false })]);
        if (child.exitCode === null && child.signalCode === null) child.kill();
      }
    },
  };
}
