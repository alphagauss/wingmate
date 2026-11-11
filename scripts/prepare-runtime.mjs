// 从锁定的 npm 发布包准备开发安装树或发行归档。
import { createHash } from 'node:crypto';
import { spawn, spawnSync } from 'node:child_process';
import { once } from 'node:events';
import { mkdir, mkdtempDisposable, readFile, writeFile, copyFile, rename } from 'node:fs/promises';
import { existsSync, createReadStream, createWriteStream } from 'node:fs';
import { createInterface } from 'node:readline';
import { pipeline } from 'node:stream/promises';
import { stripVTControlCharacters } from 'node:util';
import { createZstdCompress, constants } from 'node:zlib';
import { fileURLToPath } from 'node:url';
import { join, dirname } from 'node:path';
import { tmpdir } from 'node:os';
import * as tar from 'tar';
import { excludedRuntimeFile } from './runtime-files.mjs';
import { applyRuntimePatches } from './runtime-patches.mjs';
import { validatePluginArchive } from './runtime-plugins.mjs';

const root = fileURLToPath(new URL('../', import.meta.url));
const runtime = join(root, 'runtime');
const dev = process.argv.includes('--dev');
if (process.env.WINGMATE_PREPARE_LOCKED !== '1') throw new Error('请使用 pnpm runtime:dev 或 pnpm runtime:prepare，以取得构建锁');
process.stdin.on('end', () => process.exit(1));
process.stdin.resume();
process.stdin.unref();
const target = process.env.TAURI_ENV_TARGET_TRIPLE;
const host = spawnSync('rustc', ['-vV'], { encoding: 'utf8', windowsHide: true });
if (host.status !== 0) throw new Error('无法读取 Rust 构建目标');
const hostTarget = host.stdout.match(/^host: (.+)$/m)?.[1];
if (!hostTarget) throw new Error('无法读取 Rust 主机目标');
if (target && target !== hostTarget) throw new Error(`请在目标平台和架构上本机构建运行包：${target}（当前 ${hostTarget}）`);
const rustArch = hostTarget.split('-')[0]
  .replace(/^x86_64$/, 'x64')
  .replace(/^i[3-6]86$/, 'ia32')
  .replace(/^aarch64$/, 'arm64')
  .replace(/^(?:armv\d.*|thumbv\d.*)$/, 'arm')
  .replace(/^powerpc64(?:le)?$/, 'ppc64')
  .replace(/^riscv64gc$/, 'riscv64');
const rustPlatform = hostTarget.includes('-windows-') ? 'win32'
  : hostTarget.endsWith('-apple-darwin') ? 'darwin'
  : hostTarget.includes('-linux-') ? 'linux' : undefined;
if (rustPlatform !== process.platform || rustArch !== process.arch) {
  throw new Error(`Node ${process.platform}-${process.arch} 与 Rust ${hostTarget} 的平台或架构不匹配`);
}
const nodeTarget = `${process.platform}-${process.arch}/node-${process.versions.modules}`;
const out = join(root, 'work/build/runtime');
const cache = join(root, 'work/cache/npm');
const manifest = await readFile(join(runtime, 'package.json'));
const selected = JSON.parse(manifest).dependencies['@deepseek-ai/dsh'];
const lockPath = join(runtime, 'package-lock.json');
const lock = await readFile(lockPath).catch(error => {
  if (error.code === 'ENOENT') return null;
  throw error;
});
const fileRules = dev ? '' : await readFile(new URL('./runtime-files.mjs', import.meta.url));
const patchRecipe = await readFile(new URL('./runtime-patches.mjs', import.meta.url));
const patchBytes = await readFile(join(runtime, 'patches', `dsh-${selected}.json`));
const patches = JSON.parse(patchBytes);
const registry = process.env.npm_config_registry || process.env.NPM_CONFIG_REGISTRY || 'https://registry.npmmirror.com/';
const npmCli = join(dirname(process.execPath), 'node_modules/npm/bin/npm-cli.js');
const onWindows = process.platform === 'win32';
if (onWindows && !existsSync(npmCli)) throw new Error('当前 Node 安装缺少 npm CLI');

function npm(args, options) {
  // Windows 直接运行 Node 随带的 npm CLI，避免 cmd.exe 对路径作二次解释。
  const output = options.stdio === 'inherit' ? ['inherit', 'inherit'] : options.stdio.slice(1);
  return spawn(onWindows ? process.execPath : 'npm', onWindows ? [npmCli, ...args] : args,
    { windowsHide: true, ...options, stdio: ['ignore', ...output] });
}

await using pluginBuild = await mkdtempDisposable(join(tmpdir(), 'wingmate-plugin-'));
const plugins = [];
for (const name of ['wingmate-web', 'wingmate-file-viewer', 'wingmate-backends']) {
  const directory = `plugins/${name}`;
  const source = join(root, directory);
  const build = npm(['run', 'build', '--if-present'], { cwd: source, stdio: 'inherit' });
  const [buildStatus] = await once(build, 'close');
  if (buildStatus !== 0) throw new Error(`插件编译失败: ${name}`);
  const pack = npm(['pack', '--ignore-scripts', '--pack-destination', pluginBuild.path,
    '--cache', cache, '--json'], { cwd: source, stdio: ['ignore', 'pipe', 'inherit'] });
  let packed = '';
  pack.stdout.setEncoding('utf8');
  pack.stdout.on('data', chunk => { packed += chunk; });
  const [packStatus] = await once(pack, 'close');
  if (packStatus !== 0) throw new Error(`插件打包失败: ${name}`);
  const [{ filename }] = JSON.parse(packed);
  const archive = join(pluginBuild.path, filename);
  await validatePluginArchive(archive, selected);
  console.log(`插件包检查通过: ${name}`);
  plugins.push({ name, directory, archive, bytes: await readFile(archive) });
}
const pluginPatch = 'plugins/cordis.patch.yml';
const pluginPatchBytes = await readFile(join(root, pluginPatch));

if (!lock) throw new Error('准备运行包需要 runtime/package-lock.json');
const output = dev ? join(root, 'work/dev/components') : out;
const dependencies = join(root, 'work/cache/runtime-dependencies');
const dependencyKey = createHash('sha256').update(manifest).update(lock)
  .update(`${hostTarget}/${nodeTarget}`).update(install.toString()).digest('hex');
const base = join(dependencies, dependencyKey);
const archiveOptions = { portable: true, mtime: new Date('2026-01-01T00:00:00Z') };
const compressionLevel = dev ? 1 : 9;
const compression = () => createZstdCompress({ params: { [constants.ZSTD_c_compressionLevel]: compressionLevel } });
const archiveRecipe = digest(JSON.stringify(archiveOptions), compression.toString(),
  String(compressionLevel), process.versions.node, archiveComponent.toString());
const patchFiles = patches.map(patch => ({
  target: join('node_modules', patch.package, patch.file).replaceAll('\\', '/'),
  manifest: join('node_modules', patch.package, 'package.json'),
}));
const targets = [...new Set(patchFiles.map(file => file.target)
  .filter(file => dev || !excludedRuntimeFile(file)))].sort();
await mkdir(output, { recursive: true });
await using scratch = await mkdtempDisposable(join(output, '.prepare-'));

const components = {};
components.base = await archiveComponent('base',
  digest(dependencyKey, fileRules, JSON.stringify(targets), baseFilter.toString()),
  ['package.json', 'node_modules'], async () => {
    await prepareDependencies();
    return { cwd: base, filter: baseFilter };
  });

if (targets.length) {
  components.patches = await archiveComponent('patches',
    digest(dependencyKey, patchBytes, patchRecipe, JSON.stringify(targets)),
    targets, async () => {
      await prepareDependencies();
      const stage = join(scratch.path, 'patches');
      const files = new Set(patchFiles.flatMap(file => [file.target, file.manifest]));
      for (const file of files) {
        await mkdir(dirname(join(stage, file)), { recursive: true });
        await copyFile(join(base, file), join(stage, file));
      }
      await applyRuntimePatches(stage, patches);
      return { cwd: stage };
    });
}
for (const { name, directory, archive, bytes } of plugins) {
  components[name] = await archiveComponent(name, digest(bytes), [directory], async () => {
    const stage = join(scratch.path, name);
    const plugin = join(stage, directory);
    await mkdir(plugin, { recursive: true });
    await tar.x({ file: archive, cwd: plugin, strip: 1, strict: true });
    return { cwd: stage };
  });
}
components.config = await archiveComponent('config', digest(pluginPatchBytes), [pluginPatch], async () => {
  await mkdir(join(scratch.path, 'plugins'), { recursive: true });
  await writeFile(join(scratch.path, pluginPatch), pluginPatchBytes);
  return { cwd: scratch.path };
});
await publish('manifest.json', JSON.stringify({
  format: 1, target: hostTarget, node_arch: process.arch, components,
}, null, 2) + '\n');

function digest(...values) {
  const hash = createHash('sha256');
  for (const value of values) {
    const bytes = Buffer.from(value);
    hash.update(String(bytes.length)).update(':').update(bytes);
  }
  return hash.digest('hex');
}

async function prepareDependencies() {
  if (existsSync(join(base, 'node_modules/@deepseek-ai/dsh/lib/bin.js'))) return;
  await mkdir(dependencies, { recursive: true });
  await using installTree = await mkdtempDisposable(join(dependencies, '.install-'));
  await install(installTree.path);
  await rename(installTree.path, base);
}

function baseFilter(path, entry) {
  const normalized = path.replaceAll('\\', '/');
  if ((!dev && normalized === 'node_modules/.bin') || targets.includes(normalized)) return false;
  return dev || !entry.isFile() || !excludedRuntimeFile(normalized);
}

async function archiveComponent(name, input, paths, prepare) {
  const key = digest(archiveRecipe, input, JSON.stringify(paths), prepare.toString());
  const stamp = `${name}.sha256`;
  const cached = await readOptional(join(output, stamp));
  if (cached) {
    const [oldKey, sha256] = cached.trim().split('\n');
    const path = join(output, `${name}-${sha256}.tar.zst`);
    if (oldKey === key && /^[0-9a-f]{64}$/.test(sha256) && existsSync(path)
        && sha256 === await archiveDigest(path)) {
      console.log(`复用运行组件: ${name}`);
      return { sha256, paths };
    }
  }
  const options = await prepare();
  const partial = join(scratch.path, `${name}.tar.zst`);
  await pipeline(tar.create({ ...archiveOptions, ...options }, paths), compression(), createWriteStream(partial));
  const sha256 = await archiveDigest(partial);
  await rename(partial, join(output, `${name}-${sha256}.tar.zst`));
  await publish(stamp, `${key}\n${sha256}\n`);
  console.log(`已打包运行组件: ${name}`);
  return { sha256, paths };
}

async function readOptional(path) {
  return readFile(path, 'utf8').catch(error => {
    if (error.code === 'ENOENT') return null;
    throw error;
  });
}

async function publish(name, content) {
  const destination = join(output, name);
  if (await readOptional(destination) === content) return;
  const partial = join(scratch.path, name);
  await writeFile(partial, content);
  await rename(partial, destination);
}

async function archiveDigest(path) {
  const hash = createHash('sha256');
  for await (const chunk of createReadStream(path)) hash.update(chunk);
  return hash.digest('hex');
}

async function install(stage) {
  await writeFile(join(stage, 'package.json'), manifest);
  if (lock) await writeFile(join(stage, 'package-lock.json'), lock);
  const loglevel = process.env.npm_config_loglevel || process.env.NPM_CONFIG_LOGLEVEL;
  const command = lock ? 'ci' : 'install';
  const args = [command, '--omit=dev', '--ignore-scripts', '--no-audit', '--no-fund',
    '--cache', cache, `--registry=${registry}`, '--replace-registry-host=always',
    '--progress', `--loglevel=${loglevel || 'http'}`];
  console.log(`安装 DSH ${selected} 运行依赖 (npm ${command}, npm 源 ${new URL(registry).host})`);
  const installProcess = npm(args, { cwd: stage, stdio: loglevel ? 'inherit' : ['inherit', 'pipe', 'pipe'] });
  const terminal = process.stderr.isTTY;
  let fetched = 0;
  let cacheChecks = 0;
  let lastUpdate = 0;
  let showingProgress = false;
  function clearProgress() {
    if (!showingProgress) return;
    process.stderr.cursorTo(0);
    process.stderr.clearLine(0);
    showingProgress = false;
  }
  function showProgress(final = false) {
    const now = Date.now();
    if (!final && now - lastUpdate < (terminal ? 100 : 5000)) return;
    clearProgress();
    showingProgress = terminal && !final;
    process.stderr.write(`依赖获取 · 下载请求 ${fetched} 次 · 缓存检查 ${cacheChecks} 次${showingProgress ? '' : '\n'}`);
    lastUpdate = now;
  }
  if (!loglevel) {
    createInterface({ input: installProcess.stderr }).on('line', line => {
      const message = stripVTControlCharacters(line);
      if (/^npm http fetch GET 2\d\d /.test(message)) fetched++;
      else if (message.startsWith('npm http cache ')) cacheChecks++;
      else {
        clearProgress();
        console.error(line);
        return;
      }
      showProgress();
    });
    createInterface({ input: installProcess.stdout }).on('line', line => {
      clearProgress();
      console.log(line);
    });
  }
  const [status] = await once(installProcess, 'close');
  if (fetched || cacheChecks) showProgress(true);
  if (status !== 0) throw new Error('DSH 运行依赖安装失败');
  if (!lock) await copyFile(join(stage, 'package-lock.json'), lockPath);
  const installedDsh = JSON.parse(await readFile(join(stage, 'node_modules/@deepseek-ai/dsh/package.json'), 'utf8'));
  if (installedDsh.version !== selected) throw new Error('安装的 DSH 版本与声明不一致');
}
