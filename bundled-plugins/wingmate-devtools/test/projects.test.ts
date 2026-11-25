import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, mkdir, writeFile, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createProjects } from '../src/projects.ts';
import type { Application, Manager } from '../src/projects.ts';
import type { Runner } from '../src/process.ts';

async function fixture() {
  const root = await mkdtemp(join(tmpdir(), 'wingmate-devtools-'));
  const directory = join(root, 'source with spaces');
  await mkdir(directory);
  await writeFile(
    join(directory, 'package.json'),
    JSON.stringify({
      name: 'test-demo',
      version: '1.0.0',
      scripts: { build: 'node build.mjs' },
      dsh: { bundle: { patch: './cordis.patch.yml' } },
    }),
  );
  await writeFile(join(directory, 'pnpm-lock.yaml'), 'lock');
  const installs: { path: string; enabled: boolean }[] = [];
  let failBuild = false;
  const manager: Manager = {
    async listBundles() {
      return [{ name: 'test-demo', enabled: false, removable: true }];
    },
    async installBundle(path, options) {
      installs.push({ path, enabled: options.enabled });
      return { application: 'applied' };
    },
    async cancelInstall() {},
    async removeBundle() {
      return { application: 'applied' };
    },
    async setBundleEnabled() {
      return { application: 'applied' };
    },
  };
  const commands: string[][] = [];
  const runner: Runner = async (_invocation, args, cwd, signal, log) => {
    assert.equal(cwd, directory);
    signal.throwIfAborted();
    commands.push(args);
    log('command output\n');
    if (args[0] === 'run' && failBuild) throw new Error('故意制造的构建错误');
    if (args[0] === 'pack') {
      await writeFile(join(args.at(-1)!, 'test-demo.tgz'), 'snapshot');
      return JSON.stringify([
        { name: 'test-demo', version: '1.0.0', filename: 'test-demo.tgz' },
      ]);
    }
    return '';
  };
  const storage = join(root, 'state');
  return {
    root,
    directory,
    storage,
    manager,
    runner,
    installs,
    commands,
    fail() {
      failBuild = true;
    },
    close: () => rm(root, { recursive: true, force: true }),
  };
}

test('关联可恢复，安装使用持久成品副本并保留停用状态', async () => {
  const f = await fixture();
  try {
    const service = createProjects(f);
    await service.register(f.directory);
    const recovered = createProjects(f);
    assert.equal((await recovered.list())[0].directory, f.directory);
    const result = await recovered.build(
      'test-demo',
      new AbortController().signal,
    );
    assert.equal(result.result?.application, 'applied');
    assert.equal(f.installs[0].enabled, false);
    assert.equal(await readFile(f.installs[0].path, 'utf8'), 'snapshot');
    assert.deepEqual(f.commands[0], ['install', '--frozen-lockfile']);
    assert.ok(
      f.commands.some((args) => args[0] === 'run' && args[1] === 'build'),
    );
    assert.equal((await recovered.list())[0].building, false);
    await recovered.forget('test-demo');
    assert.deepEqual(await recovered.list(), []);
  } finally {
    await f.close();
  }
});

test('构建失败和取消均不触碰已安装版本', async () => {
  const f = await fixture();
  try {
    const service = createProjects(f);
    await service.register(f.directory);
    f.fail();
    const result = await service.build(
      'test-demo',
      new AbortController().signal,
    );
    assert.match(result.error!, /构建错误/);
    assert.equal(result.stage, 'build');
    assert.equal(f.installs.length, 0);
    const controller = new AbortController();
    controller.abort(new Error('cancelled'));
    const cancelled = await service.build('test-demo', controller.signal);
    assert.match(cancelled.error!, /cancelled/);
    assert.equal(f.installs.length, 0);
  } finally {
    await f.close();
  }
});

test('显式开发预览通过官方安装器加载开发目录，不进行正式构建', async () => {
  const f = await fixture();
  try {
    const service = createProjects(f);
    const result = await service.preview(
      f.directory,
      new AbortController().signal,
    );
    assert.equal(result.application, 'applied');
    assert.equal(f.installs[0].path, 'link:' + f.directory);
    assert.equal(f.installs[0].enabled, true);
    assert.equal(f.commands.length, 0);
    assert.deepEqual(await service.list(), []);
  } finally {
    await f.close();
  }
});

test('拒绝相对路径、随包插件和构建期间重复操作；原样报告重启要求', async () => {
  const f = await fixture();
  try {
    const service = createProjects(f);
    await assert.rejects(service.register('.'), /绝对路径/);
    const blocked = createProjects({
      ...f,
      manager: {
        ...f.manager,
        async listBundles() {
          return [{ name: 'test-demo', enabled: true, removable: false }];
        },
      },
    });
    await assert.rejects(blocked.register(f.directory), /随包插件/);
    let finish!: (result: Application) => void;
    const updating = createProjects({
      ...f,
      manager: {
        ...f.manager,
        installBundle() {
          return new Promise((resolve) => {
            finish = resolve;
          });
        },
      },
    });
    await updating.register(f.directory);
    const operation = updating.build('test-demo', new AbortController().signal);
    await assert.rejects(
      updating.build('test-demo', new AbortController().signal),
      /正在编译/,
    );
    while (!finish) await new Promise((resolve) => setTimeout(resolve, 5));
    await assert.rejects(updating.forget('test-demo'), /编译期间/);
    finish({ application: 'restart-required', restartReason: 'package-moved' });
    assert.equal((await operation).result?.application, 'restart-required');
  } finally {
    await f.close();
  }
});
