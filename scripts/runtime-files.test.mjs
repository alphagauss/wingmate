import assert from 'node:assert/strict';
import { mkdtemp, mkdir, writeFile, rm, readdir } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
import * as tar from 'tar';
import { excludedRuntimeFile } from './runtime-files.mjs';

test('裁剪归档保留运行入口、类型、许可证和目标平台二进制', async () => {
  const root = await mkdtemp(join(tmpdir(), 'wingmate-archive-'));
  const kept = ['lib/index.js', 'lib/index.d.ts', 'lib/index.d.cts', 'src/index.ts',
    'LICENSE', 'data/model.onnx', 'prebuilds/win32-x64/addon.node',
    'prebuilds/win32-arm64/LICENSE', 'prebuilds/unknown/addon.node'];
  const removed = ['lib/index.js.map', 'lib/style.css.map', 'prebuilds/win32-x64/addon.pdb',
    'prebuilds/win32-arm64/addon.node', 'prebuilds/linux-x64/addon.node'];
  try {
    for (const path of [...kept, ...removed]) {
      const full = join(root, 'source/node_modules/sample', path);
      await mkdir(join(full, '..'), { recursive: true });
      await writeFile(full, path);
    }
    const file = join(root, 'runtime.tar.gz');
    await tar.create({ cwd: join(root, 'source'), file, gzip: true,
      filter: (path, entry) => !entry.isFile() || !excludedRuntimeFile(path, 'win32', 'x64'),
    }, ['node_modules']);
    const included = [];
    await tar.list({ file, onReadEntry: entry => {
      if (entry.type === 'File') included.push(entry.path.replace('node_modules/sample/', ''));
    } });
    assert.deepEqual(included.sort(), kept.sort());
    assert.ok((await readdir(join(root, 'source/node_modules/sample/lib'))).includes('index.js.map'));
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

test('平台裁剪对 Windows 路径和其他目标架构一致', () => {
  assert.equal(excludedRuntimeFile('node_modules/p/prebuilds/win32-x64/a.node', 'linux', 'x64'), 'foreign-prebuild');
  assert.equal(excludedRuntimeFile('node_modules\\p\\prebuilds\\win32-arm64\\a.node', 'win32', 'arm64'), undefined);
  assert.equal(excludedRuntimeFile('node_modules/p/prebuilds/linux-x64/node.napi.musl.node', 'linux', 'x64'), undefined);
});
