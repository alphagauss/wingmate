import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, mkdir, writeFile, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createProjects } from '../src/projects.ts';
import type { Application, BuildResult, Manager } from '../src/projects.ts';
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
  let source: string | undefined;
  let snapshot = 'snapshot';
  const manager: Manager = {
    async listBundles() {
      return [
        {
          name: 'test-demo',
          enabled: false,
          removable: true,
          installed: true,
          availability: 'profile',
          source,
        },
      ];
    },
    async installBundle(path, options) {
      installs.push({ path, enabled: options.enabled });
      const nextSource = 'file:' + path.replaceAll('\\', '/');
      if (source === nextSource)
        return { application: 'failed', error: { code: 'already-installed' } };
      source = nextSource;
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
      await writeFile(join(args.at(-1)!, 'test-demo.tgz'), snapshot);
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
    change() {
      snapshot = 'updated snapshot';
    },
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

test('相同成品跳过重复安装并保留停用状态，同版本内容变化后仍安装更新', async () => {
  const f = await fixture();
  try {
    const service = createProjects(f);
    await service.register(f.directory);
    const first = await service.build(
      'test-demo',
      new AbortController().signal,
    );
    assert.equal(first.result?.application, 'applied');
    const repeated = await createProjects(f).build(
      'test-demo',
      new AbortController().signal,
    );
    assert.equal(repeated.unchanged, true);
    assert.equal(repeated.result, undefined);
    assert.equal(repeated.archive, first.archive);
    assert.match(repeated.log, /内容未变化/);
    assert.equal(f.installs.length, 1);
    assert.equal((await f.manager.listBundles())[0].enabled, false);
    f.change();
    const changed = await service.build(
      'test-demo',
      new AbortController().signal,
    );
    assert.equal(changed.unchanged, undefined);
    assert.equal(changed.result?.application, 'applied');
    assert.notEqual(changed.archive, first.archive);
    assert.equal(f.installs.length, 2);
    assert.equal(f.installs[1].enabled, false);
  } finally {
    await f.close();
  }
});

test('来源不同、安装缺失或成品损坏时不跳过安装，也不将 already-installed 改成成功', async (t) => {
  for (const scenario of [
    'other-source',
    'missing',
    'uninstalled',
    'inventory-error',
    'corrupt-archive',
  ]) {
    await t.test(scenario, async () => {
      const f = await fixture();
      try {
        const service = createProjects(f);
        await service.register(f.directory);
        const first = await service.build(
          'test-demo',
          new AbortController().signal,
        );
        const [installed] = await f.manager.listBundles();
        if (scenario === 'other-source') {
          const other = join(f.root, 'other.tgz');
          await writeFile(other, 'snapshot');
          installed.source = 'file:' + other;
        }
        if (scenario === 'missing') installed.availability = 'missing';
        if (scenario === 'uninstalled') installed.installed = false;
        if (scenario === 'inventory-error')
          installed.error = { code: 'operation-error' };
        if (scenario === 'corrupt-archive')
          await writeFile(first.archive!, 'damaged');
        f.manager.listBundles = async () => [installed];
        const refused = {
          application: 'failed',
          error: { code: 'already-installed' },
        };
        f.manager.installBundle = async (path, options) => {
          f.installs.push({ path, enabled: options.enabled });
          return refused;
        };
        const repeated = await service.build(
          'test-demo',
          new AbortController().signal,
        );
        assert.equal(repeated.unchanged, undefined);
        assert.deepEqual(repeated.result, refused);
        assert.equal(f.installs.length, 2);
      } finally {
        await f.close();
      }
    });
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

test('构建未结束时提供最新一行日志，成功打包只显示摘要并保留警告', async () => {
  const f = await fixture();
  let resume!: () => void;
  let ready!: () => void;
  const entered = new Promise<void>((resolve) => {
    ready = resolve;
  });
  const gate = new Promise<void>((resolve) => {
    resume = resolve;
  });
  let operation: Promise<BuildResult> | undefined;
  try {
    const runner: Runner = async (invocation, args, cwd, signal, log) => {
      if (args[0] === 'run') {
        log('\u001b[32m正在编', 'stdout');
        log('译\u001b[0m\n进度 1/2\r进度 2/2\r', 'stdout');
        ready();
        await gate;
      }
      const output = await f.runner(invocation, args, cwd, signal, log);
      if (args[0] === 'pack') {
        log(output, 'stdout');
        log('打包警告示例\n', 'stderr');
      }
      return output;
    };
    const service = createProjects({ ...f, runner });
    await service.register(f.directory);
    operation = service.build('test-demo', new AbortController().signal);
    await entered;
    const [progress] = await service.list();
    assert.equal(progress.building, true);
    assert.equal(progress.latestLine, '进度 2/2');
    resume();
    const report = await operation;
    assert.equal(report.result?.application, 'applied');
    assert.match(report.log, /正在编译/);
    assert.match(report.log, /打包警告示例/);
    assert.match(report.log, /打包完成：test-demo.tgz/);
    assert.doesNotMatch(report.log, /"filename"|\u001b/);
    assert.equal((await service.list())[0].latestLine, undefined);
  } finally {
    resume();
    await operation;
    await f.close();
  }
});

test('打包失败时保留 stdout 中的错误详情', async () => {
  const f = await fixture();
  try {
    const runner: Runner = async (invocation, args, cwd, signal, log) => {
      if (args[0] === 'pack') {
        log(
          '{"error":{"code":"ENOENT","summary":"missing package file"}}',
          'stdout',
        );
        throw new Error('命令失败，退出状态 1');
      }
      return f.runner(invocation, args, cwd, signal, log);
    };
    const service = createProjects({ ...f, runner });
    await service.register(f.directory);
    const report = await service.build(
      'test-demo',
      new AbortController().signal,
    );
    assert.equal(report.stage, 'pack');
    assert.match(report.error!, /退出状态 1/);
    assert.match(report.log, /ENOENT.*missing package file/);
    assert.equal(f.installs.length, 0);
    assert.equal((await service.list())[0].building, false);
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
