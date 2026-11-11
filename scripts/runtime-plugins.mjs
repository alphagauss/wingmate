import { spawnSync } from 'node:child_process';
import { mkdir, mkdtempDisposable } from 'node:fs/promises';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import * as tar from 'tar';

const probe = `
import { readFileSync, realpathSync, statSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { createRequire } from 'node:module';
import { join, relative, isAbsolute, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = realpathSync('node_modules/wingmate-check');
const pkg = JSON.parse(readFileSync(join(root, 'package.json'), 'utf8'));
if (pkg.engines?.dsh !== process.argv[1]) throw new Error('插件尚未适配所选 DSH 版本');
const dependencies = ['dependencies', 'optionalDependencies', 'peerDependencies']
  .flatMap(field => Object.keys(pkg[field] ?? {}).map(name => field + ': ' + name));
if (dependencies.length) throw new Error('预装插件尚不支持部署运行依赖，请先内联或实现依赖部署: ' + dependencies.join(', '));

function checkFile(path) {
  const file = realpathSync(path);
  const rel = relative(root, file);
  if (isAbsolute(rel) || rel === '..' || rel.startsWith('..' + sep) || !statSync(file).isFile()) {
    throw new Error('插件入口必须是包内文件: ' + path);
  }
  return file;
}
checkFile(fileURLToPath(import.meta.resolve('wingmate-check')));
if (pkg.dsh?.client) {
  checkFile(createRequire(import.meta.url).resolve('wingmate-check/client'));
}
if (pkg.dsh?.bundle) {
  for (const patch of [pkg.dsh.bundle.patch].flat()) {
    if (typeof patch !== 'string') throw new Error('dsh.bundle.patch 必须声明补丁文件');
    checkFile(join(root, patch));
  }
}
if (pkg.name === 'dsh-wingmate-file-viewer') {
  const { files } = JSON.parse(readFileSync(checkFile(join(root, 'dist/assets.json')), 'utf8'));
  for (const required of ['frame.html', 'viewer.js', 'THIRD-PARTY-NOTICES.txt']) {
    if (!Object.hasOwn(files, required)) throw new Error('文件预览资源清单缺少: ' + required);
  }
  for (const [name, asset] of Object.entries(files)) {
    const file = readFileSync(checkFile(join(root, 'dist/viewer', name)));
    if (file.length !== asset.size || createHash('sha256').update(file).digest('hex') !== asset.hash) {
      throw new Error('文件预览资源指纹不匹配: ' + name);
    }
  }
}
if (pkg.name === 'dsh-wingmate-backends') {
  for (const file of ['dist/transport.js', 'dist/LICENSE.acp-sdk', 'LICENSE.remote']) {
    checkFile(join(root, file));
  }
}
await import('wingmate-check');
// Import verification does not activate the plugin or wait for module-owned timers.
process.exit(0);
`;

export async function validatePluginArchive(archive, dshVersion) {
  // Keep the probe outside the repository so ancestor node_modules cannot hide missing files.
  await using scratch = await mkdtempDisposable(join(tmpdir(), 'wingmate-plugin-check-'));
  const directory = join(scratch.path, 'node_modules/wingmate-check');
  await mkdir(directory, { recursive: true });
  await tar.x({ file: archive, cwd: directory, strip: 1, strict: true });
  const env = { ...process.env, NODE_PATH: '', NODE_OPTIONS: '',
    DSH_HOME: join(scratch.path, 'dsh'), DSH_AGENTS_HOME: join(scratch.path, 'agents') };
  const result = spawnSync(process.execPath,
    ['--no-global-search-paths', '--input-type=module', '--eval', probe, dshVersion],
    { cwd: scratch.path, env, windowsHide: true, encoding: 'utf8', timeout: 10_000 });
  if (result.status !== 0) {
    throw new Error(`插件包检查失败 (${archive}): ${result.error?.message || result.stderr || result.stdout}`);
  }
}
