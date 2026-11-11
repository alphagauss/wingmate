import { spawn } from 'node:child_process';
import type { ChildProcessByStdio } from 'node:child_process';
import { Readable, Writable } from 'node:stream';
import { ClientApp, ndJsonStream } from '@agentclientprotocol/sdk';
import type { RequestPermissionRequest, RequestPermissionResponse, SendRequestOptions, SessionNotification } from '@agentclientprotocol/sdk';
import { once } from 'node:events';
import { setTimeout } from 'node:timers/promises';

type Command = { command: string; args: string[]; env?: NodeJS.ProcessEnv };
type Handlers = {
  update(params: SessionNotification): void;
  permission(params: RequestPermissionRequest, signal: AbortSignal): RequestPermissionResponse | Promise<RequestPermissionResponse>;
  disconnected(): void;
};

export function acpDeadline(milliseconds: number, lifetime?: AbortSignal): SendRequestOptions {
  const timeout = AbortSignal.timeout(milliseconds);
  return { cancellationSignal: lifetime ? AbortSignal.any([lifetime, timeout]) : timeout, stopWaitingOnCancel: true };
}

export function acpConnection(resolveCommand: () => Command, { update, permission, disconnected }: Handlers) {
  let opening: ReturnType<typeof start> | undefined;
  let child: ChildProcessByStdio<Writable, Readable, null> | undefined;
  let connection: ReturnType<ClientApp['connect']> | undefined;
  const lifetime = new AbortController();

  async function start() {
    lifetime.signal.throwIfAborted();
    const { command, args, env } = resolveCommand();
    const processHandle = spawn(command, args, { env, windowsHide: true, stdio: ['pipe', 'pipe', 'ignore'] });
    child = processHandle;
    await new Promise<void>((resolve, reject) => {
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
    // Node's stdout supplies bytes; its BYOB typings differ from the SDK's DOM stream types.
    const stdout = Readable.toWeb(processHandle.stdout) as unknown as Parameters<typeof ndJsonStream>[1];
    const stream = ndJsonStream(Writable.toWeb(processHandle.stdin), stdout);
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
      }, acpDeadline(15_000, lifetime.signal));
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
