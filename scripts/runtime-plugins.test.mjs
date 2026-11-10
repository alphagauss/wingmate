import assert from 'node:assert/strict';
import { mkdir, mkdtempDisposable, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { fileURLToPath } from 'node:url';
import test from 'node:test';
import * as tar from 'tar';
import { validatePluginArchive } from './runtime-plugins.mjs';

async function archiveFixture(root, { manifest = {}, host = 'export function apply() {}', omit = [] } = {}) {
  const directory = join(root, 'package');
  await mkdir(directory, { recursive: true });
  const pkg = { name: 'fixture', type: 'module', engines: { dsh: 'test-version' },
    exports: { '.': { import: './host.js' }, './client': { default: './client.js' } },
    dsh: { client: { platform: 'web' }, bundle: { patch: ['./first.yml', './second.yml'] } }, ...manifest };
  const files = { 'package.json': JSON.stringify(pkg), 'host.js': host,
    'client.js': 'window.__ModuleLoader__.load({});', 'first.yml': '[]', 'second.yml': '[]' };
  for (const [name, content] of Object.entries(files)) await writeFile(join(directory, name), content);
  const archive = join(root, 'plugin.tgz');
  await tar.create({ cwd: root, file: archive, gzip: true,
    filter: path => !omit.includes(path.replace('package/', '')) }, ['package']);
  return archive;
}

test('独立插件包支持条件导出和多补丁，检查时不执行客户端或 apply', async () => {
  await using scratch = await mkdtempDisposable(join(tmpdir(), 'wingmate-plugin-test-'));
  const archive = await archiveFixture(scratch.path, {
    host: 'export function apply() { throw new Error("must not activate"); }',
    manifest: { devDependencies: { 'build-only': '1.0.0' } },
  });
  await validatePluginArchive(archive, 'test-version');
  await assert.rejects(validatePluginArchive(archive, 'other-version'), /尚未适配/);
});

test('检查实际归档中的宿主、客户端和每个补丁，源目录不能补足漏打包文件', async () => {
  for (const file of ['host.js', 'client.js', 'second.yml']) {
    await using scratch = await mkdtempDisposable(join(tmpdir(), 'wingmate-plugin-test-'));
    const archive = await archiveFixture(scratch.path, { omit: [file] });
    await assert.rejects(validatePluginArchive(archive, 'test-version'), /插件包检查失败/);
  }
});

test('运行依赖未部署时明确失败，包括可选与 peer 依赖', async () => {
  for (const field of ['dependencies', 'optionalDependencies', 'peerDependencies']) {
    await using scratch = await mkdtempDisposable(join(tmpdir(), 'wingmate-plugin-test-'));
    const archive = await archiveFixture(scratch.path, { manifest: { [field]: { missing: '1.0.0' } } });
    await assert.rejects(validatePluginArchive(archive, 'test-version'), /尚不支持部署运行依赖.*missing/);
  }
});

test('宿主加载不能借用开发依赖或 NODE_PATH 掩盖未声明依赖', async () => {
  await using scratch = await mkdtempDisposable(join(tmpdir(), 'wingmate-plugin-test-'));
  const archive = await archiveFixture(scratch.path, {
    host: "import { createRequire } from 'node:module'; createRequire(import.meta.url)('tar');",
  });
  const previous = process.env.NODE_PATH;
  process.env.NODE_PATH = fileURLToPath(new URL('../node_modules', import.meta.url));
  try {
    await assert.rejects(validatePluginArchive(archive, 'test-version'), /Cannot find module 'tar'/);
  } finally {
    if (previous === undefined) delete process.env.NODE_PATH;
    else process.env.NODE_PATH = previous;
  }
});
