// 在隔离数据目录中验证真实单文件运行、并发发现、身份拒绝、停止和重启。
import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync } from 'node:fs';
import { copyFile, mkdir, mkdtemp, readFile, readdir, writeFile, access, rm } from 'node:fs/promises';
import { resolve, join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const args = process.argv.slice(2).filter(arg => arg !== '--keep-work');
const keepWork = process.argv.includes('--keep-work');
assert.ok(args.length <= 2 && args.every(arg => !arg.startsWith('--')), '用法: smoke.mjs [EXE] [新测试目录] [--keep-work]');
const original = resolve(args[0] ?? (process.platform === 'win32' ? 'target/release/wingmate.exe' : 'target/release/wingmate'));
await mkdir('work/tests', { recursive: true });
const work = args[1] ? resolve(args[1]) : await mkdtemp(resolve('work/tests/smoke-'));
if (args[1]) {
  await mkdir(dirname(work), { recursive: true });
  // Own a fresh directory before allowing automatic deletion; never adopt existing user data.
  await mkdir(work);
}
const executable = join(work, process.platform === 'win32' ? 'wingmate.exe' : 'wingmate');
const env = { ...process.env, WINGMATE_HOME: join(work, 'wingmate'), DSH_HOME: join(work, 'dsh'), DSH_AGENTS_HOME: join(work, 'agents') };
for (const key of Object.keys(env)) if (/API_KEY|ACCESS_TOKEN|AUTH_TOKEN/.test(key)) delete env[key];
const endpointFile = join(env.WINGMATE_HOME, 'endpoint.json');
const checks = [];
let expectedRevision;
let mayHaveStarted = false;
let failure, cleanupError;
function cli(action, success = true) {
  if (action === 'start' || action === 'restart') mayHaveStarted = true;
  return new Promise((resolve, reject) => {
    const child = spawn(executable, [action], { cwd: work, env, windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'] });
    let output = '', error = '';
    child.stdout.on('data', chunk => output += chunk);
    child.stderr.on('data', chunk => error += chunk);
    let timedOut = false;
    const timer = setTimeout(() => { timedOut = true; child.kill(); }, 180_000);
    child.on('error', error => { clearTimeout(timer); reject(error); });
    child.on('close', code => {
      clearTimeout(timer);
      try {
        if (timedOut) throw new Error(`${action} 超时`);
        if (success && code !== 0) throw new Error(`${action}: ${error || output}`);
        resolve(success ? JSON.parse(output) : { code, error, output });
      } catch (error) { reject(error); }
    });
  });
}
const alive = pid => { try { process.kill(pid, 0); return true; } catch { return false; } };
try {
  await copyFile(original, executable);
  if (!args[0]) {
    const directory = fileURLToPath(new URL('../work/build/runtime/', import.meta.url));
    const manifest = await readFile(join(directory, 'manifest.json'));
    const binary = await readFile(executable);
    for (const [name, { sha256 }] of Object.entries(JSON.parse(manifest).components)) {
      const archive = await readFile(join(directory, `${name}-${sha256}.tar.zst`));
      const first = binary.indexOf(archive);
      assert.notEqual(first, -1, `成品缺少运行组件 ${name}`);
      assert.equal(binary.indexOf(archive, first + archive.length), -1, `成品重复内嵌运行组件 ${name}`);
    }
    expectedRevision = createHash('sha256').update(manifest).digest('hex');
    checks.push('单文件只内嵌每个运行组件各一份');
  }
  assert.equal((await cli('status')).phase, 'stopped');
  assert.ok(existsSync(env.WINGMATE_HOME), '被测 EXE 未使用隔离管理目录，请提供发行模式 EXE');
  const oldRevision = 'a'.repeat(64);
  const oldRuntime = join(env.WINGMATE_HOME, 'runtimes', `dsh-0.0.0-${oldRevision.slice(0, 6)}`);
  const unmanaged = join(env.WINGMATE_HOME, 'runtimes', 'dsh-0.0.0-abcdef');
  await mkdir(oldRuntime, { recursive: true });
  await mkdir(unmanaged, { recursive: true });
  await writeFile(join(oldRuntime, '.wingmate-base'), oldRevision);
  await writeFile(join(unmanaged, 'keep.txt'), 'keep');
  // Wait for both launchers even if one fails, before attempting shutdown and deletion.
  const starts = await Promise.allSettled([cli('start'), cli('start')]);
  for (const result of starts) if (result.status === 'rejected') throw result.reason;
  const [a, b] = starts.map(result => result.value);
  assert.equal(a.phase, 'running');
  assert.equal(a.dsh_home, env.DSH_HOME);
  assert.equal(a.instance, b.instance);
  assert.equal(a.pid, a.engine_pid);
  assert.equal(a.engine_pid, b.engine_pid);
  await assert.rejects(access(oldRuntime), { code: 'ENOENT' });
  assert.equal(await readFile(join(unmanaged, 'keep.txt'), 'utf8'), 'keep');
  checks.push('新版就绪后清理旧运行目录，保留非托管内容');
  await copyFile(original, executable);
  assert.equal((await cli('status')).instance, a.instance);
  checks.push('只有 DSH 的 Node 进程常驻，覆盖桌面 EXE 后仍能连接');
  const manifest = JSON.parse(await readFile(new URL('../runtime/package.json', import.meta.url), 'utf8'));
  assert.equal(a.runtime, `dsh-${manifest.dependencies['@deepseek-ai/dsh']}`);
  assert.match(a.runtime_revision, /^[0-9a-f]{64}$/);
  if (expectedRevision) assert.equal(a.runtime_revision, expectedRevision);
  const runtimes = join(env.WINGMATE_HOME, 'runtimes');
  let runtimePath;
  for (const name of await readdir(runtimes)) {
    if (!name.startsWith(`${a.runtime}-`)) continue;
    const path = join(runtimes, name);
    const installed = await readFile(join(path, 'installed.json')).catch(error => {
      if (error.code !== 'ENOENT') throw error;
    });
    if (installed && createHash('sha256').update(installed).digest('hex') === a.runtime_revision) {
      assert.equal(name, `${a.runtime}-${JSON.parse(installed).components.base.sha256.slice(0, 6)}`);
      runtimePath = path;
      break;
    }
  }
  assert.ok(runtimePath, '找不到与当前后台一致的安装清单');
  const reuseProbe = join(runtimePath, 'reuse-probe.txt');
  await writeFile(reuseProbe, 'keep');
  await writeFile(join(runtimePath, 'control.mjs'), 'throw new Error("old control must not run");\n');
  checks.push('并发启动复用同一个后台和 DSH');
  console.log(checks.at(-1));

  const endpoint = JSON.parse(await readFile(endpointFile, 'utf8'));
  const rpc = `http://127.0.0.1:${endpoint.port}/rpc`;
  const denied = await fetch(rpc, { method: 'POST', body: 'stop', headers: { Authorization: 'Bearer invalid' } });
  assert.equal(denied.status, 401);
  assert.equal((await cli('status')).phase, 'running');
  checks.push('错误凭据无法停止后台');

  await writeFile(endpointFile, JSON.stringify({ ...endpoint, instance: 'incorrect-instance' }));
  try { assert.notEqual((await cli('stop', false)).code, 0); }
  finally { await writeFile(endpointFile, JSON.stringify(endpoint)); }
  assert.equal((await cli('status')).instance, a.instance);
  checks.push('错误服务身份被拒绝，原服务继续运行');

  const response = await fetch(rpc, { method: 'POST', body: 'status', headers: {
    Authorization: `Bearer ${endpoint.token}`, 'X-Wingmate-Instance': endpoint.instance,
  } });
  const privateState = await response.json();
  const login = await fetch(privateState.url, { redirect: 'manual' });
  assert.equal(login.status, 303);
  const cookie = login.headers.getSetCookie().map(s => s.split(';')[0]).join('; ');
  const page = await fetch(a.url, { headers: { Cookie: cookie } });
  assert.equal(page.status, 200);
  const html = await page.text();
  const boot = html.match(/(?:globalThis|window)\["__DSH_BOOT__"\]\s*=\s*(\{[^\n]+?\})\s*;?\s*<\/script>/);
  assert.ok(boot, 'DSH Web 缺少客户端启动清单');
  const plugin = JSON.parse(boot[1]).entries.find(row => row.id === 'dsh-wingmate-web');
  assert.ok(plugin, '释放的运行包未加载后台服务插件');
  const client = await fetch(new URL(plugin.url, a.url), { headers: { Cookie: cookie } });
  assert.equal(client.status, 200);
  assert.match(await client.text(), /wingmate-backend/);
  checks.push('释放的 DSH Web 完成认证并提供后台服务插件');
  const fileViewer = JSON.parse(boot[1]).entries.find(row => row.id === 'dsh-wingmate-file-viewer');
  assert.ok(fileViewer, '释放的运行包未加载文件预览插件');
  const viewerClient = await fetch(new URL(fileViewer.url, a.url), { headers: { Cookie: cookie } });
  assert.match(await viewerClient.text(), /wingmate-preview-open/);
  const assets = JSON.parse(await readFile(join(runtimePath, 'plugins/wingmate-file-viewer/dist/assets.json'), 'utf8'));
  const base = new URL(`/wingmate-file-viewer/${assets.revision}/`, a.url);
  for (const [path, asset] of Object.entries(assets.files)) {
    const resource = await fetch(new URL(path, base));
    assert.equal(resource.status, 200, path);
    assert.equal(createHash('sha256').update(Buffer.from(await resource.arrayBuffer())).digest('hex'), asset.hash, path);
  }
  assert.equal((await fetch(new URL('missing.js', base))).status, 404);
  assert.equal((await fetch(new URL('viewer.js', base), { method: 'POST' })).status, 405);
  checks.push('文件预览插件及全部离线资源在释放目录中完整可访问');
  const native = spawnSync(process.execPath, [fileURLToPath(new URL('./smoke-runtime.mjs', import.meta.url)),
    runtimePath, work], { env, windowsHide: true, encoding: 'utf8', timeout: 90_000 });
  assert.equal(native.status, 0, native.error?.message || native.stderr);
  checks.push(...JSON.parse(native.stdout));

  const restarted = await cli('restart');
  assert.equal(restarted.phase, 'running');
  assert.notEqual(restarted.instance, a.instance);
  assert.notEqual(restarted.engine_pid, a.engine_pid);
  assert.equal(alive(a.engine_pid), false);
  assert.equal(restarted.runtime, a.runtime);
  assert.equal(restarted.runtime_revision, a.runtime_revision);
  assert.equal(await readFile(reuseProbe, 'utf8'), 'keep');
  checks.push('按六位短指纹命名，重启按完整指纹复用释放目录');
  checks.push('重启使用 EXE 内嵌的控制入口，旧 DSH 已退出');
  await cli('stop');
  assert.equal((await cli('status')).phase, 'stopped');
  assert.equal(alive(restarted.engine_pid), false);
  checks.push('停止等待运行锁释放及 DSH 退出');
  if (process.platform === 'win32') {
    const crash = await cli('start');
    process.kill(crash.pid, 'SIGKILL');
    const deadline = Date.now() + 10_000;
    while (alive(crash.engine_pid) && Date.now() < deadline) await new Promise(r => setTimeout(r, 100));
    assert.equal(alive(crash.engine_pid), false);
    assert.equal((await cli('status')).phase, 'stopped');
    assert.equal((await cli('start')).phase, 'running');
    await cli('stop');
    checks.push('DSH 异常退出后，陈旧发现文件不阻止重新启动');
  }
} catch (error) {
  failure = error;
} finally {
  try {
    if (mayHaveStarted) {
      await cli('stop');
      assert.equal((await cli('status')).phase, 'stopped', '后台未退出，保留测试目录');
    }
  } catch (error) { cleanupError = error; }
  // Save diagnostics before deletion too: cleanup itself can fail partway through.
  try { await copyFile(join(env.WINGMATE_HOME, 'service.log'), join(work, 'service.log')); }
  catch (error) { if (error.code !== 'ENOENT') cleanupError ??= error; }
  if (!keepWork && !cleanupError) {
    try {
      for (const path of [env.WINGMATE_HOME, env.DSH_HOME, env.DSH_AGENTS_HOME, executable]) {
        await rm(path, { recursive: true, force: true, maxRetries: 3, retryDelay: 200 });
      }
      if (!failure) await rm(join(work, 'service.log'), { force: true });
    } catch (error) { cleanupError = error; }
  }
}
const report = { executable, checks, status: failure || cleanupError ? 'failed' : 'passed',
  error: failure?.stack, cleanupError: cleanupError?.stack, retained: keepWork || !!cleanupError };
await writeFile(join(work, 'result.json'), JSON.stringify(report, null, 2) + '\n');
console.log(JSON.stringify({ ...report, evidence: work }, null, 2));
if (failure || cleanupError) process.exitCode = 1;
