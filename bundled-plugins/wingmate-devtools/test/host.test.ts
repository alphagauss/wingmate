import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { apply } from '../src/host.ts';

test('界面与工具共享项目操作，工具先经过权限检查', async () => {
  const directory = await mkdtemp(join(tmpdir(), 'wingmate-devtools-host-'));
  let route: any, tool: any, dispose: any;
  let mode = 'workspace-write',
    outcome = 'rejected',
    approvals = 0;
  try {
    apply({
      profileContext: { dir: directory },
      pluginManager: {
        async listBundles() {
          return [];
        },
        async installBundle() {
          throw new Error('unexpected install');
        },
        async cancelInstall() {},
      },
      effect(install) {
        dispose = install();
      },
      get() {
        return {
          async request() {
            approvals++;
            return outcome;
          },
        };
      },
      sandboxPolicy: {
        resolve() {
          return { mode };
        },
      },
      connection: {
        fetch: {
          register(value) {
            route = value;
            return async () => {};
          },
        },
      },
      tools: {
        register(value) {
          tool = value;
          return () => {};
        },
      },
    });
    assert.equal(route.path, '/api/wingmate-devtools');
    const response = await route.fetch(
      new Request('http://localhost/api/wingmate-devtools'),
    );
    assert.deepEqual(await response.json(), []);
    const exec = {
      agent: { session: {} },
      callId: 'test',
      signal: new AbortController().signal,
    };
    await assert.rejects(tool.execute({ action: 'list' }, exec), /未获批准/);
    outcome = 'allowed-once';
    assert.equal(await tool.execute({ action: 'list' }, exec), '[]');
    assert.equal(approvals, 2);
    mode = 'danger-full-access';
    assert.equal(await tool.execute({ action: 'list' }, exec), '[]');
    assert.equal(approvals, 2);
    assert.equal(
      (
        await route.fetch(
          new Request('http://localhost/api/wingmate-devtools', {
            method: 'POST',
            body: '{}',
          }),
        )
      ).status,
      415,
    );
    const invalid = await route.fetch(
      new Request('http://localhost/api/wingmate-devtools', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ action: 'register', directory: '.' }),
      }),
    );
    assert.equal(invalid.status, 400);
    await assert.rejects(
      tool.execute({ action: 'register', directory: '.' }, exec),
      /绝对路径/,
    );
  } finally {
    await dispose?.();
    await rm(directory, { recursive: true, force: true });
  }
});
