import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdir, mkdtempDisposable, writeFile } from 'node:fs/promises';
import { dirname, join } from 'node:path';
import { tmpdir } from 'node:os';
import { fileURLToPath } from 'node:url';
import test from 'node:test';
import * as tar from 'tar';
import { validatePluginArchive } from './runtime-plugins.mjs';

async function archiveFixture(root, { manifest = {}, host = 'export function apply() {}', omit = [], extraFiles = {} } = {}) {
  const directory = join(root, 'package');
  await mkdir(directory, { recursive: true });
  const pkg = { name: 'fixture', type: 'module', engines: { dsh: 'test-version' },
    exports: { '.': { import: './host.js' }, './client': { default: './client.js' } },
    dsh: { client: { platform: 'web' }, bundle: { patch: ['./first.yml', './second.yml'] } }, ...manifest };
  const files = { 'package.json': JSON.stringify(pkg), 'host.js': host,
    'client.js': 'window.__ModuleLoader__.load({});', 'first.yml': '[]', 'second.yml': '[]', ...extraFiles };
  for (const [name, content] of Object.entries(files)) {
    const path = join(directory, name);
    await mkdir(dirname(path), { recursive: true });
    await writeFile(path, content);
  }
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

test('文件预览归档须包含资源清单记录的完整文件', async () => {
  await using scratch = await mkdtempDisposable(join(tmpdir(), 'wingmate-plugin-test-'));
  const content = 'fixture';
  const asset = { size: content.length, hash: createHash('sha256').update(content).digest('hex') };
  const names = ['frame.html', 'viewer.js', 'THIRD-PARTY-NOTICES.txt'];
  const options = {
    manifest: { name: 'dsh-wingmate-file-viewer' },
    extraFiles: {
      'dist/assets.json': JSON.stringify({ files: Object.fromEntries(names.map(name => [name, asset])) }),
      ...Object.fromEntries(names.map(name => ['dist/viewer/' + name, content])),
    },
  };
  await validatePluginArchive(await archiveFixture(scratch.path, options), 'test-version');
  const incomplete = await archiveFixture(scratch.path, { ...options, omit: ['dist/viewer/THIRD-PARTY-NOTICES.txt'] });
  await assert.rejects(validatePluginArchive(incomplete, 'test-version'), /THIRD-PARTY-NOTICES\.txt/);
});

test('检查实际归档中的宿主、客户端和每个补丁，源目录不能补足漏打包文件', async () => {
  for (const file of ['host.js', 'client.js', 'second.yml']) {
    await using scratch = await mkdtempDisposable(join(tmpdir(), 'wingmate-plugin-test-'));
    const archive = await archiveFixture(scratch.path, { omit: [file] });
    await assert.rejects(validatePluginArchive(archive, 'test-version'), /插件包检查失败/);
  }
});

test('执行后端归档携带页面 transport 和复用代码的许可证', async () => {
  const files = ['dist/transport.js', 'dist/LICENSE.acp-sdk', 'LICENSE.remote'];
  for (const missing of [undefined, ...files]) {
    await using scratch = await mkdtempDisposable(join(tmpdir(), 'wingmate-backends-package-'));
    const archive = await archiveFixture(scratch.path, {
      manifest: { name: 'dsh-wingmate-backends' },
      extraFiles: Object.fromEntries(files.map(file => [file, 'fixture'])),
      omit: missing ? [missing] : [],
    });
    if (missing) await assert.rejects(validatePluginArchive(archive, 'test-version'), /插件包检查失败/);
    else await validatePluginArchive(archive, 'test-version');
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
