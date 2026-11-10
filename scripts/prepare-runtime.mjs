// 从锁定的 npm 发布包准备开发安装树或发行归档。
import { createHash, randomUUID } from 'node:crypto';
import { spawn, spawnSync } from 'node:child_process';
import { once } from 'node:events';
import { mkdir, mkdtempDisposable, readFile, writeFile, copyFile, stat, rename, rm } from 'node:fs/promises';
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
const target = process.env.TAURI_ENV_TARGET_TRIPLE;
const host = spawnSync('rustc', ['-vV'], { encoding: 'utf8', windowsHide: true });
if (host.status !== 0) throw new Error('无法读取 Rust 构建目标');
const hostTarget = host.stdout.match(/^host: (.+)$/m)?.[1];
if (target && target !== hostTarget) throw new Error('首版只支持在目标平台本机构建运行包');
const out = join(runtime, 'out');
const archive = join(out, 'runtime.tar.zst');
const stamp = join(out, 'inputs.sha256');
const manifest = await readFile(join(runtime, 'package.json'));
const selected = JSON.parse(manifest).dependencies['@deepseek-ai/dsh'];
const lockPath = join(runtime, 'package-lock.json');
const lock = await readFile(lockPath).catch(error => {
  if (error.code === 'ENOENT') return null;
  throw error;
});
const recipe = await readFile(fileURLToPath(import.meta.url));
const fileRules = dev ? '' : await readFile(new URL('./runtime-files.mjs', import.meta.url));
const patchRecipe = await readFile(new URL('./runtime-patches.mjs', import.meta.url));
const pluginChecks = await readFile(new URL('./runtime-plugins.mjs', import.meta.url));
const patchBytes = await readFile(join(runtime, 'patches', `dsh-${selected}.json`));
const patches = JSON.parse(patchBytes);
const registry = process.env.npm_config_registry || process.env.NPM_CONFIG_REGISTRY || 'https://registry.npmmirror.com/';
const npmCli = join(dirname(process.execPath), 'node_modules/npm/bin/npm-cli.js');
const onWindows = process.platform === 'win32';
if (onWindows && !existsSync(npmCli)) throw new Error('当前 Node 安装缺少 npm CLI');

function npm(args, options) {
  // Windows 直接运行 Node 随带的 npm CLI，避免 cmd.exe 对路径作二次解释。
  return spawn(onWindows ? process.execPath : 'npm', onWindows ? [npmCli, ...args] : args,
    { windowsHide: true, ...options });
}

await using pluginBuild = await mkdtempDisposable(join(tmpdir(), 'wingmate-plugin-'));
const plugins = [];
for (const name of ['wingmate-web', 'wingmate-file-viewer']) {
  const directory = `plugins/${name}`;
  const source = join(root, directory);
  const build = npm(['run', 'build', '--if-present'], { cwd: source, stdio: 'inherit' });
  const [buildStatus] = await once(build, 'close');
  if (buildStatus !== 0) throw new Error(`插件编译失败: ${name}`);
  const pack = npm(['pack', '--ignore-scripts', '--pack-destination', pluginBuild.path,
    '--cache', join(runtime, '.cache'), '--json'], { cwd: source, stdio: ['ignore', 'pipe', 'inherit'] });
  let packed = '';
  pack.stdout.setEncoding('utf8');
  pack.stdout.on('data', chunk => { packed += chunk; });
  const [packStatus] = await once(pack, 'close');
  if (packStatus !== 0) throw new Error(`插件打包失败: ${name}`);
  const [{ filename }] = JSON.parse(packed);
  const archive = join(pluginBuild.path, filename);
  await validatePluginArchive(archive, selected);
  console.log(`插件包检查通过: ${name}`);
  plugins.push({ directory, archive, bytes: await readFile(archive) });
}
const pluginPatch = 'plugins/cordis.patch.yml';
const pluginPatchBytes = await readFile(join(root, pluginPatch));

function fingerprint(lock) {
  const hash = createHash('sha256').update(manifest).update(recipe).update(fileRules)
    .update(patchRecipe).update(patchBytes).update(pluginChecks).update(lock).update(hostTarget).update(pluginPatchBytes);
  for (const plugin of plugins) hash.update(plugin.directory).update(plugin.bytes);
  return hash.digest('hex');
}
if (dev) {
  if (!lock) throw new Error('开发安装树需要 runtime/package-lock.json');
  const stages = join(runtime, 'stage');
  const current = join(stages, `current-${selected}`);
  const key = fingerprint(lock);
  const stage = join(stages, key);
  const entry = 'node_modules/@deepseek-ai/dsh/lib/bin.js';
  const ready = () => existsSync(join(stage, entry)) && existsSync(join(stage, pluginPatch));
  if (ready()) {
    console.log(`复用 DSH ${selected} 开发安装树`);
  } else {
    await mkdir(stages, { recursive: true });
    await using scratch = await mkdtempDisposable(join(stages, '.install-'));
    await install(scratch.path);
    if (!existsSync(join(scratch.path, entry))) throw new Error('开发安装树缺少 DSH 入口');
    try {
      await rename(scratch.path, stage);
    } catch (error) {
      // 同一份依赖可以由另一个并发的开发启动先准备完成。
      if (!ready()) throw error;
    }
  }
  if (!existsSync(current) || (await readFile(current, 'utf8')).trim() !== key) {
    const partial = `${current}.${randomUUID()}.part`;
    try {
      await writeFile(partial, `${key}\n`);
      await rename(partial, current);
    } finally {
      await rm(partial, { force: true });
    }
  }
} else {
  await prepareArchive();
}

async function prepareArchive() {
  console.log(`检查 DSH ${selected} 运行包缓存 (${hostTarget})`);
  if (lock && existsSync(stamp) && existsSync(archive)) {
    const [inputs, digest] = (await readFile(stamp, 'utf8')).trim().split('\n');
    if (inputs === fingerprint(lock) && digest === await archiveDigest(archive)) {
      console.log('复用已准备的 DSH 运行包');
      return;
    }
  }
  await mkdir(out, { recursive: true });
  await using scratch = await mkdtempDisposable(join(out, '.install-'));
  const stage = scratch.path;
  await install(stage);
  // 发行裁剪在补丁与插件集成后执行，开发安装树保留完整依赖。
  const compressionLevel = 9;
  console.log(`裁剪并压缩 DSH ${selected} 运行包 (Node 内置 Zstd ${compressionLevel})`);
  const report = { target: hostTarget, compression: `zstd-${compressionLevel}`,
    patches: patches.map(patch => patch.id), plugins: plugins.map(plugin => plugin.directory),
    keptFiles: 0, keptBytes: 0, excluded: {} };
  const files = tar.create({ cwd: stage,
    portable: true, mtime: new Date('2026-01-01T00:00:00Z'),
    filter(path, entry) {
      if (path.replaceAll('\\', '/') === 'node_modules/.bin') return false;
      if (!entry.isFile()) return true;
      const reason = excludedRuntimeFile(path);
      if (reason) {
        const group = report.excluded[reason] ??= { files: 0, bytes: 0 };
        group.files++;
        group.bytes += entry.size;
        return false;
      }
      report.keptFiles++;
      report.keptBytes += entry.size;
      return true;
    },
  }, ['package.json', 'node_modules', 'plugins']);
  const partial = `${archive}.${randomUUID()}.part`;
  const stampFile = join(stage, 'inputs.sha256');
  const hash = createHash('sha256');
  try {
    await pipeline(files, createZstdCompress({ params: { [constants.ZSTD_c_compressionLevel]: compressionLevel } }),
      async function* (chunks) {
        for await (const chunk of chunks) {
          hash.update(chunk);
          yield chunk;
        }
      }, createWriteStream(partial));
    report.archiveBytes = (await stat(partial)).size;
    const inputs = fingerprint(await readFile(join(stage, 'package-lock.json')));
    await writeFile(stampFile, `${inputs}\n${hash.digest('hex')}\n`);
    await rename(partial, archive);
    await rename(stampFile, stamp);
  } finally {
    await rm(partial, { force: true });
  }
  await writeFile(join(out, 'build-report.json'), JSON.stringify(report, null, 2) + '\n');
  console.log(`DSH ${selected} 运行包已准备 (${hostTarget})`);
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
    '--cache', join(runtime, '.cache'), `--registry=${registry}`, '--replace-registry-host=always',
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
  await applyRuntimePatches(stage, patches);
  for (const item of plugins) {
    const plugin = join(stage, item.directory);
    await mkdir(plugin, { recursive: true });
    await tar.x({ file: item.archive, cwd: plugin, strip: 1, strict: true });
  }
  await writeFile(join(stage, pluginPatch), pluginPatchBytes);
}
