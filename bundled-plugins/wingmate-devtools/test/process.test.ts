import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { runCommand } from '../src/process.ts';

test('参数和带空格目录不经过 shell 拼接，保留退出错误与日志', async () => {
  const directory = await mkdtemp(join(tmpdir(), 'wingmate process '));
  try {
    const script = join(directory, 'echo.mjs');
    await writeFile(
      script,
      'console.log(process.argv[2]); console.error("diagnostic");',
    );
    let log = '';
    const output = await runCommand(
      { command: process.execPath, args: [script] },
      ['value & remains literal'],
      directory,
      new AbortController().signal,
      (chunk) => {
        log += chunk;
      },
    );
    assert.equal(output.trim(), 'value & remains literal');
    assert.match(log, /diagnostic/);
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
