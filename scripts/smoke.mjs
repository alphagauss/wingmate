// 在隔离数据目录中验证真实单文件运行、并发发现、身份拒绝、停止和重启。
import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { copyFile, mkdir, mkdtemp, readFile, writeFile } from 'node:fs/promises';
import { resolve, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const original = resolve(process.argv[2] ?? (process.platform === 'win32' ? 'target/release/wingmate.exe' : 'target/release/wingmate'));
await mkdir('work', { recursive: true });
const work = process.argv[3] ? resolve(process.argv[3]) : await mkdtemp(resolve('work/smoke-'));
const executable = join(work, process.platform === 'win32' ? 'wingmate.exe' : 'wingmate');
await copyFile(original, executable);
const env = { ...process.env, WINGMATE_HOME: join(work, 'wingmate'), DSH_HOME: join(work, 'dsh'), DSH_AGENTS_HOME: join(work, 'agents') };
for (const key of Object.keys(env)) if (/API_KEY|ACCESS_TOKEN|AUTH_TOKEN/.test(key)) delete env[key];
const endpointFile = join(env.WINGMATE_HOME, 'endpoint.json');
const checks = [];
let expectedRevision;
if (!process.argv[2]) {
  // 默认成品应内嵌本次构建准备的归档，且只内嵌一份。
  const archive = await readFile(new URL('../runtime/out/runtime.tar.zst', import.meta.url));
  const binary = await readFile(executable);
  const first = binary.indexOf(archive);
  assert.notEqual(first, -1, '成品缺少本次构建准备的运行包');
  assert.equal(binary.indexOf(archive, first + archive.length), -1, '成品重复内嵌运行包');
  expectedRevision = createHash('sha256').update(archive).digest('hex');
  checks.push('单文件只内嵌一份运行包');
}
function cli(action, success = true) {
  return new Promise((resolve, reject) => {
    const child = spawn(executable, [action], { cwd: work, env, windowsHide: true, stdio: ['ignore', 'pipe', 'pipe'] });
    let output = '', error = '';
    child.stdout.on('data', chunk => output += chunk);
    child.stderr.on('data', chunk => error += chunk);
    const timer = setTimeout(() => { child.kill(); reject(new Error(`${action} 超时`)); }, 180_000);
    child.on('error', reject);
    child.on('close', code => {
      clearTimeout(timer);
      if (success && code !== 0) reject(new Error(`${action}: ${error || output}`));
      else resolve(success ? JSON.parse(output) : { code, error, output });
    });
  });
}
const alive = pid => { try { process.kill(pid, 0); return true; } catch { return false; } };
try {
  assert.equal((await cli('status')).phase, 'stopped');
  const [a, b] = await Promise.all([cli('start'), cli('start')]);
  assert.equal(a.phase, 'running');
  assert.equal(a.instance, b.instance);
  assert.equal(a.pid, a.engine_pid);
  assert.equal(a.engine_pid, b.engine_pid);
  await copyFile(original, executable);
  assert.equal((await cli('status')).instance, a.instance);
  checks.push('只有 DSH 的 Node 进程常驻，覆盖桌面 EXE 后仍能连接');
  const manifest = JSON.parse(await readFile(new URL('../runtime/package.json', import.meta.url), 'utf8'));
  assert.equal(a.runtime, `dsh-${manifest.dependencies['@deepseek-ai/dsh']}`);
  assert.match(a.runtime_revision, /^[0-9a-f]{64}$/);
  if (expectedRevision) assert.equal(a.runtime_revision, expectedRevision);
  const runtimePath = join(env.WINGMATE_HOME, 'runtimes', `${a.runtime}-${a.runtime_revision}`);
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
  checks.push('按运行包指纹释放，重启直接复用相同内容的目录');
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
  await writeFile(join(work, 'result.json'), JSON.stringify({ executable, checks }, null, 2));
  console.log(JSON.stringify({ checks, evidence: work }, null, 2));
} finally {
  try { await cli('stop'); } catch (error) { console.error(`清理未完成：${error.message}`); }
}
