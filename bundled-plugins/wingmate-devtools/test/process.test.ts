import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, mkdir, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { packageCommand, runCommand } from '../src/process.ts';

test('参数和带空格目录不经过 shell 拼接，保留退出错误与日志', async () => {
  const directory = await mkdtemp(join(tmpdir(), 'wingmate process '));
  try {
    const script = join(directory, 'echo.mjs');
    await writeFile(
      script,
      'console.log(process.argv[2]); console.error("diagnostic");',
    );
    let log = '';
    const streams = new Map<string, string>();
    const output = await runCommand(
      { command: process.execPath, args: [script] },
      ['value & remains literal'],
      directory,
      new AbortController().signal,
      (chunk, stream) => {
        log += chunk;
        streams.set(stream!, (streams.get(stream!) || '') + chunk);
      },
    );
    assert.equal(output.trim(), 'value & remains literal');
    assert.match(log, /diagnostic/);
    assert.equal(streams.get('stdout')?.trim(), 'value & remains literal');
    assert.equal(streams.get('stderr')?.trim(), 'diagnostic');
    await assert.rejects(
      runCommand(
        { command: process.execPath, args: ['-e', 'process.exit(7)'] },
        [],
        directory,
        new AbortController().signal,
        () => {},
      ),
      /退出状态 7/,
    );
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});

test('优先使用当前 Node 同目录的 npm 和 pnpm，覆盖 PATH 中的其他安装', async () => {
  const root = await mkdtemp(join(tmpdir(), 'wingmate toolchains '));
  const originalNode = Object.getOwnPropertyDescriptor(process, 'execPath')!;
  const originalPath = process.env.PATH;
  try {
    const inherited = join(root, 'system');
    process.env.PATH = inherited;
    for (const layout of ['windows', 'unix']) {
      const managed = join(root, layout);
      const bin = layout === 'windows' ? managed : join(managed, 'bin');
      const node = join(bin, layout === 'windows' ? 'node.exe' : 'node');
      Object.defineProperty(process, 'execPath', {
        ...originalNode,
        value: node,
      });
      for (const name of ['npm', 'pnpm'] as const) {
        const entry =
          name === 'npm' ? 'npm/bin/npm-cli.js' : 'pnpm/bin/pnpm.mjs';
        const local = join(
          managed,
          layout === 'windows' ? 'node_modules' : 'lib/node_modules',
          entry,
        );
        const external = join(inherited, 'node_modules', entry);
        for (const file of [local, external]) {
          await mkdir(dirname(file), { recursive: true });
          await writeFile(file, '');
        }
        assert.deepEqual(packageCommand(name), {
          command: node,
          args: [local],
        });
      }
    }
  } finally {
    Object.defineProperty(process, 'execPath', originalNode);
    if (originalPath === undefined) delete process.env.PATH;
    else process.env.PATH = originalPath;
    await rm(root, { recursive: true, force: true });
  }
});

test('UTF-8 日志跨数据块时仍保留完整中文', async () => {
  let log = '';
  const code =
    'const bytes=Buffer.from("构建完成");process.stdout.write(bytes.subarray(0,2));setTimeout(()=>process.stdout.write(bytes.subarray(2)),30);';
  const output = await runCommand(
    { command: process.execPath, args: ['-e', code] },
    [],
    tmpdir(),
    new AbortController().signal,
    (chunk) => {
      log += chunk;
    },
  );
  assert.equal(output, '构建完成');
  assert.equal(log, '构建完成');
});

test('取消时结束任务拥有的进程树并等待退出', { timeout: 10000 }, async () => {
  const controller = new AbortController();
  let descendant = 0;
  const code =
    'const {spawn}=require("node:child_process");const child=spawn(process.execPath,["-e","setInterval(()=>{},1000)"],{stdio:"inherit"});console.log("child:"+child.pid);setInterval(()=>{},1000);';
  const operation = runCommand(
    { command: process.execPath, args: ['-e', code] },
    [],
    tmpdir(),
    controller.signal,
    (chunk) => {
      const match = chunk.match(/child:(\d+)/);
      if (match) {
        descendant = Number(match[1]);
        controller.abort(new Error('cancelled'));
      }
    },
  );
  await assert.rejects(operation, /cancelled/);
  assert.ok(descendant > 0);
  assert.throws(() => process.kill(descendant, 0), { code: 'ESRCH' });
});
