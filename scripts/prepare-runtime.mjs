// 从锁定的 npm 发布包生成运行包；开发与发布使用同一套字节，不修改上游源码。
import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { mkdir, readFile, writeFile, copyFile, stat } from 'node:fs/promises';
import { existsSync, createWriteStream } from 'node:fs';
import { pipeline } from 'node:stream/promises';
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
const stage = join(runtime, 'stage');
await mkdir(out, { recursive: true });
await mkdir(stage, { recursive: true });
const manifest = await readFile(join(runtime, 'package.json'));
const lockPath = join(runtime, 'package-lock.json');
const recipe = await readFile(fileURLToPath(import.meta.url));
const fileRules = await readFile(new URL('./runtime-files.mjs', import.meta.url));
function fingerprint(lock) {
  return createHash('sha256').update(manifest).update(recipe).update(fileRules)
    .update(lock).update(hostTarget).digest('hex');
}
if (existsSync(lockPath) && existsSync(join(out, 'inputs.sha256')) && existsSync(archive)) {
  const expected = fingerprint(await readFile(lockPath));
  if ((await readFile(join(out, 'inputs.sha256'), 'utf8')).trim() === expected) {
    console.log('复用已准备的 DSH 运行包');
    process.exit(0);
  }
}
await copyFile(join(runtime, 'package.json'), join(stage, 'package.json'));
const locked = existsSync(lockPath);
if (locked) await copyFile(lockPath, join(stage, 'package-lock.json'));
const args = [locked ? 'ci' : 'install', '--omit=dev', '--ignore-scripts', '--no-audit', '--no-fund',
  '--cache', join(runtime, '.cache')];
// Windows 直接运行 Node 随带的 npm CLI，避免 cmd.exe 对路径作二次解释。
const npmCli = join(dirname(process.execPath), 'node_modules/npm/bin/npm-cli.js');
const executable = process.platform === 'win32' ? process.execPath : 'npm';
if (process.platform === 'win32' && !existsSync(npmCli)) throw new Error('当前 Node 安装缺少 npm CLI');
const installed = spawnSync(executable, process.platform === 'win32' ? [npmCli, ...args] : args,
  { cwd: stage, stdio: 'inherit', windowsHide: true });
if (installed.status !== 0) throw new Error('DSH 运行依赖安装失败');
if (!locked) await copyFile(join(stage, 'package-lock.json'), lockPath);
const installedDsh = JSON.parse(await readFile(join(stage, 'node_modules/@deepseek-ai/dsh/package.json'), 'utf8'));
const selected = JSON.parse(manifest).dependencies['@deepseek-ai/dsh'];
if (installedDsh.version !== selected) throw new Error('安装的 DSH 版本与声明不一致');
// 保留安装树；只在分发归档时过滤命令 shim、调试信息和异架构预编译文件。
const report = { target: hostTarget, compression: 'zstd-19', keptFiles: 0, keptBytes: 0, excluded: {} };
await pipeline(tar.create({ cwd: stage,
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
}, ['package.json', 'node_modules']),
createZstdCompress({ params: { [constants.ZSTD_c_compressionLevel]: 19 } }), createWriteStream(archive));
report.archiveBytes = (await stat(archive)).size;
await writeFile(join(out, 'build-report.json'), JSON.stringify(report, null, 2) + '\n');
await writeFile(join(out, 'inputs.sha256'), fingerprint(await readFile(lockPath)));
console.log(`DSH ${selected} 运行包已准备 (${hostTarget})`);
