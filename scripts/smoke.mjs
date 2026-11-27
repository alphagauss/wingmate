// 在隔离数据目录中验证真实单文件运行、并发发现、身份拒绝、停止和重启。
import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { createHash, randomUUID } from 'node:crypto';
import { on, once } from 'node:events';
import { existsSync } from 'node:fs';
import {
  copyFile,
  mkdir,
  mkdtemp,
  readFile,
  readdir,
  writeFile,
  access,
  rm,
  stat,
} from 'node:fs/promises';
import { createRequire } from 'node:module';
import { resolve, join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const args = process.argv.slice(2).filter((arg) => arg !== '--keep-work');
const keepWork = process.argv.includes('--keep-work');
assert.ok(
  args.length <= 2 && args.every((arg) => !arg.startsWith('--')),
  '用法: smoke.mjs [EXE] [新测试目录] [--keep-work]',
);
const original = resolve(
  args[0] ??
    (process.platform === 'win32'
      ? 'target/release/wingmate.exe'
      : 'target/release/wingmate'),
);
await mkdir('work/tests', { recursive: true });
const work = args[1]
  ? resolve(args[1])
  : await mkdtemp(resolve('work/tests/smoke-'));
if (args[1]) {
  await mkdir(dirname(work), { recursive: true });
  // Own a fresh directory before allowing automatic deletion; never adopt existing user data.
  await mkdir(work);
}
const executable = join(
  work,
  process.platform === 'win32' ? 'wingmate.exe' : 'wingmate',
);
const env = {
  ...process.env,
  WINGMATE_HOME: join(work, 'wingmate-home'),
  DSH_HOME: join(work, 'dsh'),
  DSH_AGENTS_HOME: join(work, 'agents'),
};
delete env.WINGMATE_CODEX_ACP;
for (const key of Object.keys(env))
  if (/API_KEY|ACCESS_TOKEN|AUTH_TOKEN/.test(key)) delete env[key];
const endpointFile = join(env.WINGMATE_HOME, 'endpoint.json');
const checks = [];
let expectedRevision;
let mayHaveStarted = false;
let previousService;
let failure, cleanupError;
function cli(action, success = true, ...parameters) {
  if (action === 'start' || action === 'restart') mayHaveStarted = true;
  return new Promise((resolve, reject) => {
    const child = spawn(executable, [action, ...parameters], {
      cwd: work,
      env,
      windowsHide: true,
      stdio: ['ignore', 'pipe', 'pipe'],
    });
    let output = '',
      error = '';
    child.stdout.on('data', (chunk) => (output += chunk));
    child.stderr.on('data', (chunk) => (error += chunk));
    let timedOut = false;
    const timer = setTimeout(() => {
      timedOut = true;
      child.kill();
    }, 180_000);
    child.on('error', (error) => {
      clearTimeout(timer);
      reject(error);
    });
    child.on('close', (code) => {
      clearTimeout(timer);
      try {
        if (timedOut) throw new Error(`${action} 超时`);
        if (success && code !== 0)
          throw new Error(`${action}: ${error || output}`);
        resolve(success ? JSON.parse(output) : { code, error, output });
      } catch (error) {
        reject(error);
      }
    });
  });
}
const alive = (pid) => {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
};
try {
  await copyFile(original, executable);
  if (!args[0]) {
    const directory = fileURLToPath(
      new URL('../work/build/runtime/', import.meta.url),
    );
    const manifest = await readFile(join(directory, 'manifest.json'));
    const binary = await readFile(executable);
    for (const [name, { sha256 }] of Object.entries(
      JSON.parse(manifest).components,
    )) {
      const archive = await readFile(
        join(directory, `${name}-${sha256}.tar.zst`),
      );
      const first = binary.indexOf(archive);
      assert.notEqual(first, -1, `成品缺少运行组件 ${name}`);
      assert.equal(
        binary.indexOf(archive, first + archive.length),
        -1,
        `成品重复内嵌运行组件 ${name}`,
      );
    }
    expectedRevision = createHash('sha256').update(manifest).digest('hex');
    checks.push('单文件只内嵌每个运行组件各一份');
  }
  assert.equal((await cli('status')).phase, 'stopped');
  assert.ok(
    existsSync(env.WINGMATE_HOME),
    '被测 EXE 未使用隔离管理目录，请提供发行模式 EXE',
  );
  const oldRevision = 'a'.repeat(64);
  const oldRuntime = join(
    env.WINGMATE_HOME,
    'runtimes',
    `dsh-0.0.0-${oldRevision.slice(0, 6)}`,
  );
  const unmanaged = join(env.WINGMATE_HOME, 'runtimes', 'dsh-0.0.0-abcdef');
  await mkdir(oldRuntime, { recursive: true });
  await mkdir(unmanaged, { recursive: true });
  await writeFile(join(oldRuntime, '.wingmate-base'), oldRevision);
  await writeFile(join(unmanaged, 'keep.txt'), 'keep');
  // Wait for both launchers even if one fails, before attempting shutdown and deletion.
  const starts = await Promise.allSettled([cli('start'), cli('start')]);
  for (const result of starts)
    if (result.status === 'rejected') throw result.reason;
  const [a, b] = starts.map((result) => result.value);
  assert.equal(a.phase, 'running');
  assert.equal(a.dsh_home, env.DSH_HOME);
  assert.equal(a.instance, b.instance);
  assert.equal(a.pid, a.engine_pid);
  assert.equal(a.engine_pid, b.engine_pid);
  const tools = join(env.WINGMATE_HOME, 'toolchains');
  const toolsBin = process.platform === 'win32' ? tools : join(tools, 'bin');
  const bundledNode = join(
    toolsBin,
    process.platform === 'win32' ? 'node.exe' : 'node',
  );
  const diagnose = (await cli('doctor')).node;
  assert.equal(diagnose.ok, true);
  assert.equal(diagnose.source, 'bundled');
  assert.equal(diagnose.info.path.toLowerCase(), bundledNode.toLowerCase());
  const toolEnv = Object.fromEntries(
    Object.entries(env).filter(([key]) => key.toUpperCase() !== 'PATH'),
  );
  toolEnv.PATH = toolsBin;
  const runTool = (tool, args, customEnv = toolEnv) =>
    spawnSync(
      process.platform === 'win32'
        ? join(process.env.SystemRoot, 'System32/cmd.exe')
        : tool,
      process.platform === 'win32' ? ['/d', '/c', tool, ...args] : args,
      { cwd: work, env: customEnv, encoding: 'utf8', windowsHide: true },
    );
  const toolVersions = {};
  for (const tool of ['node', 'npm', 'npx', 'pnpm']) {
    const versionArgs =
      tool === 'pnpm' ? ['with', 'current', '--version'] : ['--version'];
    const result = runTool(tool, versionArgs);
    assert.equal(result.status, 0, result.error ?? result.stderr);
    toolVersions[tool] = result.stdout.trim();
    assert.match(toolVersions[tool], /^v?\d+\.\d+\.\d+$/);
  }
  const pnpxCheck = runTool('pnpx', ['--help']);
  assert.equal(pnpxCheck.status, 0, pnpxCheck.error ?? pnpxCheck.stderr);
  assert.match(pnpxCheck.stdout, /Usage:\s+pnpm dlx\b/);
  const modulesDir =
    process.platform === 'win32'
      ? join(tools, 'node_modules')
      : join(tools, 'lib/node_modules');
  await assert.rejects(access(join(modulesDir, 'corepack')), {
    code: 'ENOENT',
  });
  await assert.rejects(
    access(
      join(
        toolsBin,
        process.platform === 'win32' ? 'corepack.cmd' : 'corepack',
      ),
    ),
    { code: 'ENOENT' },
  );
  checks.push(
    '发行版使用固定目录中的随包 Node，npm、npx、pnpm 与 pnpx 在无系统工具的 PATH 下可直接执行，且不含 corepack',
  );
  await assert.rejects(access(oldRuntime), { code: 'ENOENT' });
  assert.equal(await readFile(join(unmanaged, 'keep.txt'), 'utf8'), 'keep');
  checks.push('新版就绪后清理旧运行目录，保留非托管内容');
  await copyFile(original, executable);
  assert.equal((await cli('status')).instance, a.instance);
  const host = await cli('service-status');
  assert.notEqual(host.pid, a.engine_pid);
  assert.equal(host.business.instance, a.instance);
  assert.ok(alive(host.pid));
  const serviceName = `wingmate-service${process.platform === 'win32' ? '.exe' : ''}`;
  const serviceDirectory = join(env.WINGMATE_HOME, 'services');
  const servicePath = join(serviceDirectory, serviceName);
  const serviceBinary = await readFile(servicePath);
  const serviceModifiedAt = (await stat(servicePath)).mtimeMs;
  assert.deepEqual(await readdir(serviceDirectory), [serviceName]);
  assert.equal(
    createHash('sha256').update(serviceBinary).digest('hex'),
    host.revision,
  );
  assert.equal(
    (await readFile(executable)).indexOf(serviceBinary),
    -1,
    '成品仍内嵌未压缩的管理服务',
  );
  const componentsDirectory = fileURLToPath(
    new URL('../work/build/runtime/', import.meta.url),
  );
  const componentsManifest = JSON.parse(
    await readFile(join(componentsDirectory, 'manifest.json'), 'utf8'),
  );
  for (const [name, { sha256 }] of Object.entries(
    componentsManifest.components,
  )) {
    const archive = await readFile(
      join(componentsDirectory, `${name}-${sha256}.tar.zst`),
    );
    assert.equal(
      serviceBinary.indexOf(archive),
      -1,
      `管理服务错误内嵌运行资源 ${name}`,
    );
  }
  checks.push('管理服务与业务进程独立，服务不携带运行资源，发行入口可替换');
  assert.notEqual((await cli('service-stop', false)).code, 0);
  assert.equal((await cli('status')).instance, a.instance);
  const manifest = JSON.parse(
    await readFile(new URL('../runtime/package.json', import.meta.url), 'utf8'),
  );
  assert.equal(a.runtime, `dsh-${manifest.dependencies['@deepseek-ai/dsh']}`);
  assert.match(a.runtime_revision, /^[0-9a-f]{64}$/);
  if (expectedRevision) assert.equal(a.runtime_revision, expectedRevision);
  const runtimes = join(env.WINGMATE_HOME, 'runtimes');
  let runtimePath;
  for (const name of await readdir(runtimes)) {
    if (!name.startsWith(`${a.runtime}-`)) continue;
    const path = join(runtimes, name);
    const installed = await readFile(join(path, 'installed.json')).catch(
      (error) => {
        if (error.code !== 'ENOENT') throw error;
      },
    );
    if (
      installed &&
      createHash('sha256').update(installed).digest('hex') ===
        a.runtime_revision
    ) {
      assert.equal(
        name,
        `${a.runtime}-${JSON.parse(installed).components.base.sha256.slice(0, 6)}`,
      );
      runtimePath = path;
      break;
    }
  }
  assert.ok(runtimePath, '找不到与当前后台一致的安装清单');
  const reuseProbe = join(runtimePath, 'reuse-probe.txt');
  await writeFile(reuseProbe, 'keep');
  await writeFile(
    join(runtimePath, 'control.mjs'),
    'throw new Error("old control must not run");\n',
  );
  checks.push('并发启动复用同一个后台和 DSH');
  console.log(checks.at(-1));

  const endpoint = JSON.parse(await readFile(endpointFile, 'utf8'));
  const rpc = `http://127.0.0.1:${endpoint.port}/rpc`;
  const denied = await fetch(rpc, {
    method: 'POST',
    body: 'stop',
    headers: { Authorization: 'Bearer invalid' },
  });
  assert.equal(denied.status, 401);
  assert.equal((await cli('status')).phase, 'running');
  checks.push('错误凭据无法停止后台');

  await writeFile(
    endpointFile,
    JSON.stringify({ ...endpoint, instance: 'incorrect-instance' }),
  );
  try {
    assert.notEqual((await cli('stop', false)).code, 0);
  } finally {
    await writeFile(endpointFile, JSON.stringify(endpoint));
  }
  assert.equal((await cli('status')).instance, a.instance);
  checks.push('错误服务身份被拒绝，原服务继续运行');

  const response = await fetch(rpc, {
    method: 'POST',
    body: 'status',
    headers: {
      Authorization: `Bearer ${endpoint.token}`,
      'X-Wingmate-Instance': endpoint.instance,
    },
  });
  const privateState = await response.json();
  const login = await fetch(privateState.url, { redirect: 'manual' });
  assert.equal(login.status, 303);
  const cookie = login.headers
    .getSetCookie()
    .map((s) => s.split(';')[0])
    .join('; ');
  const page = await fetch(a.url, { headers: { Cookie: cookie } });
  assert.equal(page.status, 200);
  const html = await page.text();
  const bootPattern =
    /(?:globalThis|window)\["__DSH_BOOT__"\]\s*=\s*(\{[^\n]+?\})\s*;?\s*<\/script>/;
  const boot = html.match(bootPattern);
  assert.ok(boot, 'DSH Web 缺少客户端启动清单');
  const plugin = JSON.parse(boot[1]).entries.find(
    (row) => row.id === 'dsh-wingmate-web',
  );
  assert.ok(plugin, '释放的运行包未加载后台服务插件');
  const client = await fetch(new URL(plugin.url, a.url), {
    headers: { Cookie: cookie },
  });
  assert.equal(client.status, 200);
  const webClient = await client.text();
  assert.match(webClient, /wingmate-backend/);
  assert.match(webClient, /plugins\.item/);
  for (const id of [
    'dsh-wingmate-web',
    'dsh-wingmate-file-viewer',
    'dsh-wingmate-backends',
  ])
    assert.ok(webClient.includes(id));
  checks.push('释放的 DSH Web 完成认证并提供后台服务设置和内置插件卡片');
  const devtools = JSON.parse(boot[1]).entries.find(
    (row) => row.id === 'dsh-wingmate-devtools',
  );
  assert.ok(devtools, '释放的运行包未加载插件开发入口');
  const devtoolsClient = await fetch(new URL(devtools.url, a.url), {
    headers: { Cookie: cookie },
  });
  assert.equal(devtoolsClient.status, 200);
  assert.match(await devtoolsClient.text(), /wingmate-devtools/);
  const devtoolsRoot = join(runtimePath, 'plugins/wingmate-devtools');
  for (const file of [
    'skills/wingmate-plugin-development/SKILL.md',
    'dist/template/AGENTS.md',
    'dist/template/README.md',
    'dist/template/gitignore',
    'dist/template/pnpm-lock.yaml',
    'dist/template/src/dsh/client.tsx',
    'dist/template/src/web/server.ts',
    'dist/template/scripts/dev-dsh.mjs',
  ])
    assert.ok((await readFile(join(devtoolsRoot, file))).length, file);
  const developmentUrl = new URL('/api/wingmate-devtools', a.url);
  assert.equal((await fetch(developmentUrl)).status, 401);
  const developmentList = await fetch(developmentUrl, {
    headers: { Cookie: cookie },
  });
  assert.equal(developmentList.status, 200);
  assert.deepEqual(await developmentList.json(), []);
  // Exercise the released Node/npm/pnpm and official installer without downloading dependencies.
  const project = join(work, 'independent plugin');
  await mkdir(project);
  const smokeName = 'dsh-wingmate-independent-smoke';
  await writeFile(
    join(project, 'package.json'),
    JSON.stringify({
      name: smokeName,
      version: '1.0.0',
      private: true,
      type: 'module',
      exports: { '.': './dist/host.js' },
      files: ['dist', 'cordis.patch.yml'],
      engines: {
        dsh: JSON.parse(
          await readFile(
            join(runtimePath, 'node_modules/@deepseek-ai/dsh/package.json'),
            'utf8',
          ),
        ).version,
      },
      scripts: { build: 'node build.mjs' },
      dsh: { bundle: { patch: './cordis.patch.yml' } },
    }),
  );
  await writeFile(join(project, 'pnpm-workspace.yaml'), 'packages:\n  - .\n');
  await writeFile(
    join(project, 'cordis.patch.yml'),
    `- insert:\n    - id: ${smokeName}\n      name: ${smokeName}\n`,
  );
  await writeFile(
    join(project, 'build.mjs'),
    `import { mkdir, copyFile } from 'node:fs/promises';\nawait mkdir('dist', {recursive:true});\nawait copyFile('source.mjs','dist/host.js');\n`,
  );
  const sourceFor = (revision) =>
    `export const inject=['connection'];\nexport function apply(ctx){ctx.connection.fetch.register({path:'/api/wingmate-independent-smoke',methods:['GET'],requestBody:'buffered',async fetch(){return Response.json({revision:${revision}});}});}\n`;
  await writeFile(join(project, 'source.mjs'), sourceFor(1));
  const developmentCall = async (body) => {
    const response = await fetch(developmentUrl, {
      method: 'POST',
      headers: { Cookie: cookie, 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
      signal: AbortSignal.timeout(120_000),
    });
    assert.equal(response.status, 200);
    return response.json();
  };
  await developmentCall({ action: 'register', directory: project });
  const firstBuild = await developmentCall({
    action: 'build',
    name: smokeName,
  });
  assert.equal(
    firstBuild.result?.application,
    'applied',
    JSON.stringify(firstBuild),
  );
  const readRevision = async () => {
    const response = await fetch(
      new URL('/api/wingmate-independent-smoke', a.url),
      { headers: { Cookie: cookie } },
    );
    assert.equal(response.status, 200);
    return (await response.json()).revision;
  };
  assert.equal(await readRevision(), 1);
  await writeFile(join(project, 'source.mjs'), sourceFor(2));
  assert.equal(await readRevision(), 1, '源码变化不应直接改变正在使用的版本');
  const updatedBuild = await developmentCall({
    action: 'build',
    name: smokeName,
  });
  assert.equal(
    updatedBuild.result?.application,
    'applied',
    JSON.stringify(updatedBuild),
  );
  assert.equal(await readRevision(), 2);
  await writeFile(
    join(project, 'build.mjs'),
    'throw new Error("smoke build failure");\n',
  );
  const failedBuild = await developmentCall({
    action: 'build',
    name: smokeName,
  });
  assert.ok(failedBuild.error);
  assert.equal(await readRevision(), 2, '构建失败必须保留已安装成品');
  checks.push(
    '单文件提供开发入口、技能和完整模板，随包工具可构建更新独立插件，失败保留旧版',
  );
  const backends = JSON.parse(boot[1]).entries.find(
    (row) => row.id === 'dsh-wingmate-backends',
  );
  assert.ok(backends, '释放的运行包未加载执行后端插件');
  const backendClient = await fetch(new URL(backends.url, a.url), {
    headers: { Cookie: cookie },
  });
  assert.equal(backendClient.status, 200);
  assert.match(await backendClient.text(), /wingmate-agent-picker/);
  assert.match(html, /__WINGMATE_AGENTS__/);
  assert.match(html, /wingmateAgent/);
  const diagnosticsUrl = new URL('/api/wingmate-backends', a.url);
  assert.equal((await fetch(diagnosticsUrl)).status, 401);
  const diagnostics = await fetch(diagnosticsUrl, {
    headers: { Cookie: cookie },
  });
  assert.equal(diagnostics.status, 200);
  const codex = (await diagnostics.json()).find((row) => row.id === 'codex');
  assert.equal(codex?.connection, 'unchecked');
  assert.equal(codex.adapterAvailable, true);
  checks.push(
    '执行后端插件、ACP 适配器与页面 transport 随单文件释放，检查状态不启动 Agent',
  );
  const fileViewer = JSON.parse(boot[1]).entries.find(
    (row) => row.id === 'dsh-wingmate-file-viewer',
  );
  assert.ok(fileViewer, '释放的运行包未加载文件预览插件');
  const viewerClient = await fetch(new URL(fileViewer.url, a.url), {
    headers: { Cookie: cookie },
  });
  assert.match(await viewerClient.text(), /wingmate-preview-open/);
  const assets = JSON.parse(
    await readFile(
      join(runtimePath, 'plugins/wingmate-file-viewer/dist/assets.json'),
      'utf8',
    ),
  );
  const base = new URL(`/wingmate-file-viewer/${assets.revision}/`, a.url);
  for (const [path, asset] of Object.entries(assets.files)) {
    const resource = await fetch(new URL(path, base));
    assert.equal(resource.status, 200, path);
    assert.equal(
      createHash('sha256')
        .update(Buffer.from(await resource.arrayBuffer()))
        .digest('hex'),
      asset.hash,
      path,
    );
  }
  assert.equal((await fetch(new URL('missing.js', base))).status, 404);
  assert.equal(
    (await fetch(new URL('viewer.js', base), { method: 'POST' })).status,
    405,
  );
  checks.push('文件预览插件及全部离线资源在释放目录中完整可访问');
  const native = spawnSync(
    process.execPath,
    [
      fileURLToPath(new URL('./smoke-runtime.mjs', import.meta.url)),
      runtimePath,
      work,
    ],
    { env, windowsHide: true, encoding: 'utf8', timeout: 90_000 },
  );
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
  assert.equal((await cli('service-status')).instance, host.instance);
  assert.equal((await cli('share-status')).address, null);
  const { local_address } = await cli('share-status');
  assert.ok(local_address);
  const gateway = 'http://' + local_address;
  assert.equal((await fetch(gateway)).status, 401);
  const { pairing_url } = await cli('share-pair-local');
  const link = new URL(pairing_url);
  assert.equal(link.pathname, '/');
  assert.equal(link.hash, '');
  const code = link.searchParams.get('auth');
  assert.ok(code);
  const landing = await fetch(link);
  assert.equal(landing.status, 200);
  assert.equal(landing.headers.has('set-cookie'), false);
  const pair = new URL('/?auth', gateway);
  const paired = await fetch(pair, {
    method: 'POST',
    headers: { Origin: gateway },
    body: code,
  });
  assert.equal(paired.status, 204);
  const shareCookie = paired.headers.getSetCookie()[0].split(';')[0];
  assert.equal(
    (
      await fetch(pair, {
        method: 'POST',
        headers: { Origin: gateway },
        body: code,
      })
    ).status,
    401,
  );
  const remote = (path, { headers, ...options } = {}) =>
    fetch(new URL(path, gateway), {
      ...options,
      headers: { Cookie: shareCookie, ...headers },
      signal: AbortSignal.timeout(160_000),
    });
  const manage = (action, expected_instance = null) =>
    remote('/_wingmate/manage', {
      method: 'POST',
      headers: { Origin: gateway, 'Content-Type': 'application/json' },
      body: JSON.stringify({ action, expected_instance }),
    });
  const recovery = await remote('/', { headers: { Accept: 'text/html' } });
  assert.equal(recovery.status, 200);
  assert.match(await recovery.text(), /_wingmate\/home\.js/);
  assert.equal((await remote('/_wingmate/home.js')).status, 200);
  const startResponse = await manage('start');
  assert.equal(startResponse.status, 200);
  const remoteStarted = await startResponse.json();
  assert.equal(remoteStarted.phase, 'running');
  assert.equal((await cli('status')).runtime_revision, a.runtime_revision);
  assert.equal((await manage('stop', restarted.instance)).status, 409);
  const sharedPage = await remote('/');
  assert.equal(sharedPage.status, 200);
  assert.equal(sharedPage.headers.has('set-cookie'), false);
  const sharedBoot = (await sharedPage.text()).match(bootPattern);
  assert.ok(sharedBoot, '分享工作台缺少客户端启动清单');
  const sharedModules = JSON.parse(sharedBoot[1]).entries.map((row) => row.id);
  assert.ok(
    sharedModules.includes(
      '@deepseek-ai/dsh-client-ui-directory-picker-browse',
    ),
  );
  assert.ok(
    !sharedModules.includes(
      '@deepseek-ai/dsh-client-ui-directory-picker-native',
    ),
  );
  assert.equal((await remote(plugin.url)).status, 200);
  assert.equal((await remote(backends.url)).status, 200);
  assert.equal((await remote(fileViewer.url)).status, 200);

  const call = (method, args) =>
    remote('/api/' + method, {
      method: 'POST',
      headers: { Origin: gateway, 'Content-Type': 'application/json' },
      body: JSON.stringify({
        type: 'client-request',
        rpcId: randomUUID(),
        method,
        payload: { args },
      }),
    });
  const listing = await call('directoryPicker/list', { path: work });
  assert.equal(listing.status, 200);
  const { result: directories } = await listing.json();
  assert.equal(directories.ok, true, JSON.stringify(directories));
  assert.equal(directories.value.path, work);
  assert.ok(
    directories.value.entries.some((entry) => entry.path === env.WINGMATE_HOME),
  );
  checks.push('分享工作台加载网页目录选择器，并可浏览主机目录');

  const created = await call('session/create', { request: { cwd: work } });
  assert.equal(created.status, 200);
  const { result: session } = await created.json();
  assert.equal(session.ok, true, JSON.stringify(session));
  const bytes = Buffer.from('Wingmate LAN file transfer\n');
  const uploadQuery = new URLSearchParams({
    sessionId: session.value.sessionId,
    name: 'gateway.txt',
  });
  const uploaded = await remote(
    '/api/session/uploadFileBinary?' + uploadQuery,
    {
      method: 'POST',
      headers: { Origin: gateway, 'Content-Type': 'application/octet-stream' },
      body: bytes,
    },
  );
  assert.equal(uploaded.status, 200);
  const uploadResult = await uploaded.json();
  assert.equal(uploadResult.ok, true, JSON.stringify(uploadResult));
  const sample = join(env.DSH_HOME, 'gateway.txt');
  await writeFile(sample, bytes);
  const downloaded = await remote(
    '/api/file?' + new URLSearchParams({ path: sample }),
  );
  assert.equal(downloaded.status, 200);
  assert.deepEqual(Buffer.from(await downloaded.arrayBuffer()), bytes);

  const WebSocket = createRequire(
    join(runtimePath, 'node_modules/@deepseek-ai/dsh/package.json'),
  )('ws');
  const socket = new WebSocket(
    gateway.replace('http:', 'ws:') + '/api/remote.mux',
    {
      headers: { Cookie: shareCookie, Origin: gateway },
      handshakeTimeout: 5000,
    },
  );
  try {
    await once(socket, 'open');
    if (process.platform === 'win32') {
      const agentId = session.value.sessionId;
      const shells = await call('terminal/shells', { agentId });
      assert.equal(shells.status, 200);
      const { result: profiles } = await shells.json();
      assert.equal(profiles.ok, true, JSON.stringify(profiles));
      const cmd = profiles.value.find((shell) =>
        /^cmd(?:\.exe)?$/i.test(shell.name),
      );
      assert.ok(cmd, '终端菜单缺少 CMD');
      const id = randomUUID();
      const created = await call('terminal/create', {
        agentId,
        request: { id, shellPath: cmd.path, cols: 80, rows: 24 },
      });
      assert.equal(created.status, 200);
      const { result: terminal } = await created.json();
      assert.equal(terminal.ok, true, JSON.stringify(terminal));

      const messages = on(socket, 'message', {
        signal: AbortSignal.timeout(15_000),
      });
      socket.send(
        JSON.stringify({
          type: 'open',
          streamId: id,
          endpoint: 'terminal/attach',
          payload: { args: { agentId, id } },
        }),
      );
      let output = '';
      for await (const [message] of messages) {
        const frame = JSON.parse(message);
        if (frame.streamId !== id) continue;
        assert.equal(frame.type, 'item', JSON.stringify(frame));
        const value = frame.value;
        if (value.type === 'snapshot') {
          assert.equal(value.info.state, 'running', JSON.stringify(value.info));
          assert.equal(value.writable, true);
          socket.send(
            JSON.stringify({
              type: 'item',
              streamId: id,
              value: {
                type: 'write',
                sequence: 1,
                data: "set WINGMATE_PTY_CHECK=wingmate-pty\recho %WINGMATE_PTY_CHECK%-ok\rnode -e \"console.log(['managed', require('node:path').basename(require('node:path').dirname(process.execPath))].join('-'))\"\rnpm --version\rpnpm with current --version\rexit\r",
              },
            }),
          );
        }
        if (value.type === 'output') output += value.data;
        if (value.type === 'state' && value.info.state !== 'running') {
          assert.equal(value.info.exitCode, 0, JSON.stringify(value.info));
          break;
        }
      }
      assert.match(output, /wingmate-pty-ok/);
      assert.match(output, /managed-toolchains/);
      assert.ok(output.includes(toolVersions.npm), output);
      assert.ok(output.includes(toolVersions.pnpm), output);
      socket.send(JSON.stringify({ type: 'cancel', streamId: id }));
      checks.push('实际 DSH 侧栏终端能启动 CMD、调用随包 Node、npm 和 pnpm');
    }
    const {
      devices: [device],
    } = await cli('share-status');
    await Promise.all([
      once(socket, 'close', { signal: AbortSignal.timeout(8000) }),
      cli('share-revoke', true, device.id),
    ]);
  } finally {
    socket.terminate();
  }
  assert.equal((await manage('stop', remoteStarted.instance)).status, 401);
  await cli('stop');
  checks.push(
    '本机网关复用真实 DSH 工作台、插件、文件传输和长连接，停机后可从原地址启动并撤销设备',
  );
  await cli('service-restart');
  const replacementHost = await cli('service-status');
  assert.notEqual(replacementHost.instance, host.instance);
  assert.equal(alive(host.pid), false);
  assert.equal((await stat(servicePath)).mtimeMs, serviceModifiedAt);
  assert.equal(replacementHost.revision, host.revision);
  assert.equal((await cli('status')).phase, 'stopped');
  checks.push('显式重启服务切换管理实例并复用同指纹文件，业务仍为停止状态');
  if (process.platform === 'win32') {
    const crash = await cli('start');
    process.kill(crash.pid, 'SIGKILL');
    const deadline = Date.now() + 10_000;
    while (alive(crash.engine_pid) && Date.now() < deadline)
      await new Promise((r) => setTimeout(r, 100));
    assert.equal(alive(crash.engine_pid), false);
    assert.equal((await cli('status')).phase, 'stopped');
    assert.equal((await cli('start')).phase, 'running');
    await cli('stop');
    checks.push('DSH 异常退出后，陈旧发现文件不阻止重新启动');
    const business = await cli('start');
    const beforeCrash = await cli('service-status');
    process.kill(beforeCrash.pid, 'SIGKILL');
    const hostDeadline = Date.now() + 10_000;
    while (alive(beforeCrash.pid) && Date.now() < hostDeadline)
      await new Promise((r) => setTimeout(r, 100));
    assert.ok(alive(business.engine_pid));
    assert.notEqual((await cli('status', false)).code, 0);
    assert.equal((await cli('start')).instance, business.instance);
    assert.notEqual(
      (await cli('service-status')).instance,
      beforeCrash.instance,
    );
    await cli('stop');
    checks.push('管理服务异常退出不终止业务，重新引导后恢复同一业务实例');
    // 改动已释放服务的 DOS 提示文字，模拟一个已有的旧版服务。
    await cli('service-stop');
    const oldBinary = Buffer.from(serviceBinary);
    const textOffset = oldBinary.indexOf(
      Buffer.from('This program cannot be run in DOS mode'),
    );
    assert.notEqual(textOffset, -1);
    oldBinary[textOffset] = 't'.charCodeAt(0);
    const oldServiceHash = createHash('sha256').update(oldBinary).digest('hex');
    await writeFile(servicePath, oldBinary);
    previousService = spawn(servicePath, ['--home', env.WINGMATE_HOME], {
      cwd: work,
      env,
      windowsHide: true,
      stdio: 'ignore',
    });
    await once(previousService, 'spawn');
    const readyDeadline = Date.now() + 10_000;
    let oldHost;
    do {
      oldHost = await cli('service-status');
      if (oldHost.revision === oldServiceHash) break;
      assert.ok(
        previousService.exitCode === null && Date.now() < readyDeadline,
        '旧管理服务未就绪',
      );
      await new Promise((resolve) => setTimeout(resolve, 100));
    } while (true);
    assert.equal(oldHost.pid, previousService.pid);
    const oldBusiness = await cli('start');
    assert.equal((await cli('service-status')).instance, oldHost.instance);
    assert.notEqual((await cli('service-restart', false)).code, 0);
    assert.equal((await cli('status')).instance, oldBusiness.instance);
    assert.deepEqual(await readFile(servicePath), oldBinary);
    assert.deepEqual(await readdir(serviceDirectory), [serviceName]);
    const newBusiness = await cli('restart');
    const newHost = await cli('service-status');
    assert.equal(newHost.revision, host.revision);
    assert.notEqual(newHost.instance, oldHost.instance);
    assert.notEqual(newBusiness.instance, oldBusiness.instance);
    for (const target of [
      { service: oldHost.instance, business: newBusiness.instance },
      { service: newHost.instance, business: oldBusiness.instance },
    ]) {
      assert.notEqual(
        (await cli('internal-restart', false, JSON.stringify(target))).code,
        0,
      );
    }
    assert.equal((await cli('status')).instance, newBusiness.instance);
    assert.equal(alive(oldHost.pid), false);
    assert.deepEqual(await readdir(serviceDirectory), [serviceName]);
    assert.deepEqual(await readFile(servicePath), serviceBinary);
    assert.equal(newBusiness.runtime_revision, oldBusiness.runtime_revision);
    assert.equal(await readFile(reuseProbe, 'utf8'), 'keep');
    assert.equal((await cli('start')).instance, newBusiness.instance);
    assert.equal((await cli('service-status')).revision, newHost.revision);
    await cli('stop');
    checks.push(
      '显式重启等待旧服务退出并替换固定路径，拒绝指向已更换实例的确认',
    );
    const revisionFile = join(tools, '.wingmate-toolchains');
    const revision = await readFile(revisionFile, 'utf8');
    await writeFile(revisionFile, oldRevision);
    const processes = [bundledNode, process.execPath].map((node) =>
      spawn(node, ['-e', 'setInterval(() => {}, 1000)'], {
        env,
        windowsHide: true,
        stdio: 'ignore',
      }),
    );
    const [managed, system] = processes;
    try {
      await Promise.all(processes.map((child) => once(child, 'spawn')));
      const blocked = await cli('restart', false);
      assert.notEqual(blocked.code, 0);
      assert.match(blocked.error, new RegExp(`\\b${managed.pid}\\b`));
      assert.ok(alive(managed.pid) && alive(system.pid));
      assert.equal(await readFile(revisionFile, 'utf8'), oldRevision);
      const ended = once(managed, 'exit', {
        signal: AbortSignal.timeout(90_000),
      });
      const dshHome = env.DSH_HOME;
      env.DSH_HOME = join(env.WINGMATE_HOME, 'unusable-dsh-home');
      await writeFile(env.DSH_HOME, 'this file cannot hold DSH data');
      try {
        const [failedStart] = await Promise.all([
          cli('restart', false, '--force-toolchain'),
          ended,
        ]);
        assert.notEqual(failedStart.code, 0);
        assert.match(failedStart.error, /DSH/);
        assert.match(
          await readFile(join(env.WINGMATE_HOME, 'service.log'), 'utf8'),
          /ENOTDIR.*unusable-dsh-home/,
        );
      } finally {
        env.DSH_HOME = dshHome;
      }
      assert.ok(alive(system.pid));
      assert.equal(await readFile(revisionFile, 'utf8'), revision);
      const config = JSON.parse(
        await readFile(join(env.WINGMATE_HOME, 'runtime.json'), 'utf8'),
      );
      const installed = JSON.parse(
        await readFile(join(config.directory, 'installed.json'), 'utf8'),
      );
      assert.equal(installed.components.toolchains.sha256, revision);
      assert.equal(
        await readFile(
          join(env.WINGMATE_HOME, '.toolchains-previous/.wingmate-toolchains'),
          'utf8',
        ),
        oldRevision,
      );
      assert.equal((await cli('start')).phase, 'running');
      await assert.rejects(
        access(join(env.WINGMATE_HOME, '.toolchains-previous')),
        { code: 'ENOENT' },
      );
      checks.push(
        'Windows 强制更新只结束随包 Node，业务启动失败后已发布的清单和工具仍然一致',
      );
    } finally {
      for (const child of processes) {
        if (child.pid && child.exitCode === null && child.signalCode === null) {
          const exit = once(child, 'exit', {
            signal: AbortSignal.timeout(10_000),
          });
          child.kill();
          await exit;
        }
      }
    }
  }
} catch (error) {
  failure = error;
} finally {
  try {
    if (mayHaveStarted) {
      await cli('stop');
      assert.equal(
        (await cli('status')).phase,
        'stopped',
        '后台未退出，保留测试目录',
      );
      await cli('service-stop');
      assert.equal(
        (await cli('service-status')).phase,
        'stopped',
        '管理服务未退出，保留测试目录',
      );
    }
  } catch (error) {
    cleanupError = error;
  }
  try {
    if (
      previousService?.pid &&
      previousService.exitCode === null &&
      previousService.signalCode === null
    ) {
      const exit = once(previousService, 'exit', {
        signal: AbortSignal.timeout(10_000),
      });
      previousService.kill();
      await exit;
    }
  } catch (error) {
    cleanupError ??= error;
  }
  // Save diagnostics before deletion too: cleanup itself can fail partway through.
  for (const name of ['service.log', 'host.log']) {
    try {
      await copyFile(join(env.WINGMATE_HOME, name), join(work, name));
    } catch (error) {
      if (error.code !== 'ENOENT') cleanupError ??= error;
    }
  }
  if (!keepWork && !cleanupError) {
    try {
      for (const path of [
        env.WINGMATE_HOME,
        env.DSH_HOME,
        env.DSH_AGENTS_HOME,
        executable,
      ]) {
        await rm(path, {
          recursive: true,
          force: true,
          maxRetries: 3,
          retryDelay: 200,
        });
      }
      if (!failure)
        for (const name of ['service.log', 'host.log'])
          await rm(join(work, name), { force: true });
    } catch (error) {
      cleanupError = error;
    }
  }
}
const report = {
  executable,
  checks,
  status: failure || cleanupError ? 'failed' : 'passed',
  error: failure?.stack,
  cleanupError: cleanupError?.stack,
  retained: keepWork || !!cleanupError,
};
await writeFile(
  join(work, 'result.json'),
  JSON.stringify(report, null, 2) + '\n',
);
console.log(JSON.stringify({ ...report, evidence: work }, null, 2));
if (failure || cleanupError) process.exitCode = 1;
