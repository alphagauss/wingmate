// 从锁定的 npm 发布包生成运行包；开发与发布使用同一套字节，不修改上游源码。
import { createHash, randomUUID } from 'node:crypto';
import { spawn, spawnSync } from 'node:child_process';
import { once } from 'node:events';
import { mkdir, readFile, writeFile, copyFile, stat, rename, rm } from 'node:fs/promises';
import { existsSync, createWriteStream } from 'node:fs';
import { createInterface } from 'node:readline';
import { pipeline } from 'node:stream/promises';
import { stripVTControlCharacters } from 'node:util';
import { createZstdCompress, constants } from 'node:zlib';
import { fileURLToPath } from 'node:url';
import { join, dirname } from 'node:path';
import * as tar from 'tar';
import { excludedRuntimeFile } from './runtime-files.mjs';

const root = fileURLToPath(new URL('../', import.meta.url));
const runtime = join(root, 'runtime');
const target = process.env.TAURI_ENV_TARGET_TRIPLE;
const host = spawnSync('rustc', ['-vV'], { encoding: 'utf8', windowsHide: true });
if (host.status !== 0) throw new Error('无法读取 Rust 构建目标');
const hostTarget = host.stdout.match(/^host: (.+)$/m)?.[1];
if (target && target !== hostTarget) throw new Error('首版只支持在目标平台本机构建运行包');
const out = join(runtime, 'out');
const archive = join(out, 'runtime.tar.zst');
const stamp = join(out, 'inputs.sha256');
const stage = join(runtime, 'stage');
await mkdir(out, { recursive: true });
await mkdir(stage, { recursive: true });
const manifest = await readFile(join(runtime, 'package.json'));
const selected = JSON.parse(manifest).dependencies['@deepseek-ai/dsh'];
const lockPath = join(runtime, 'package-lock.json');
const recipe = await readFile(fileURLToPath(import.meta.url));
const fileRules = await readFile(new URL('./runtime-files.mjs', import.meta.url));
function fingerprint(lock) {
  return createHash('sha256').update(manifest).update(recipe).update(fileRules)
    .update(lock).update(hostTarget).digest('hex');
}
console.log(`检查 DSH ${selected} 运行包缓存 (${hostTarget})`);
if (existsSync(lockPath) && existsSync(stamp) && existsSync(archive)) {
  const expected = fingerprint(await readFile(lockPath));
  if ((await readFile(stamp, 'utf8')).trim() === expected) {
    console.log('复用已准备的 DSH 运行包');
    process.exit(0);
  }
}
await copyFile(join(runtime, 'package.json'), join(stage, 'package.json'));
const locked = existsSync(lockPath);
if (locked) await copyFile(lockPath, join(stage, 'package-lock.json'));
const registry = process.env.npm_config_registry || process.env.NPM_CONFIG_REGISTRY || 'https://registry.npmmirror.com/';
const loglevel = process.env.npm_config_loglevel || process.env.NPM_CONFIG_LOGLEVEL;
const command = locked ? 'ci' : 'install';
const args = [command, '--omit=dev', '--ignore-scripts', '--no-audit', '--no-fund',
  '--cache', join(runtime, '.cache'), `--registry=${registry}`, '--replace-registry-host=always',
  '--progress', `--loglevel=${loglevel || 'http'}`];
// Windows 直接运行 Node 随带的 npm CLI，避免 cmd.exe 对路径作二次解释。
const npmCli = join(dirname(process.execPath), 'node_modules/npm/bin/npm-cli.js');
const executable = process.platform === 'win32' ? process.execPath : 'npm';
if (process.platform === 'win32' && !existsSync(npmCli)) throw new Error('当前 Node 安装缺少 npm CLI');
console.log(`安装 DSH ${selected} 运行依赖 (npm ${command}, npm 源 ${new URL(registry).host})`);
const npm = spawn(executable, process.platform === 'win32' ? [npmCli, ...args] : args,
  { cwd: stage, stdio: loglevel ? 'inherit' : ['inherit', 'pipe', 'pipe'], windowsHide: true });
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
  createInterface({ input: npm.stderr }).on('line', line => {
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
  createInterface({ input: npm.stdout }).on('line', line => {
    clearProgress();
    console.log(line);
  });
}
const [status] = await once(npm, 'close');
if (fetched || cacheChecks) showProgress(true);
if (status !== 0) throw new Error('DSH 运行依赖安装失败');
if (!locked) await copyFile(join(stage, 'package-lock.json'), lockPath);
const installedDsh = JSON.parse(await readFile(join(stage, 'node_modules/@deepseek-ai/dsh/package.json'), 'utf8'));
if (installedDsh.version !== selected) throw new Error('安装的 DSH 版本与声明不一致');
// 保留安装树；只在分发归档时过滤命令 shim、调试信息和异架构预编译文件。
const compressionLevel = 9;
console.log(`裁剪并压缩 DSH ${selected} 运行包 (Node 内置 Zstd ${compressionLevel})`);
const report = { target: hostTarget, compression: `zstd-${compressionLevel}`, keptFiles: 0, keptBytes: 0, excluded: {} };
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
}, ['package.json', 'node_modules']);
const partial = `${archive}.${randomUUID()}.part`;
try {
  await pipeline(files, createZstdCompress({ params: { [constants.ZSTD_c_compressionLevel]: compressionLevel } }),
    createWriteStream(partial));
  await writeFile(stamp, '');
  await rename(partial, archive);
} finally {
  await rm(partial, { force: true });
}
report.archiveBytes = (await stat(archive)).size;
await writeFile(join(out, 'build-report.json'), JSON.stringify(report, null, 2) + '\n');
await writeFile(stamp, fingerprint(await readFile(join(stage, 'package-lock.json'))));
console.log(`DSH ${selected} 运行包已准备 (${hostTarget})`);
