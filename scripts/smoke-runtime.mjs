// 对 EXE 实际释放的依赖做功能验证，避免只验证 Web 页面而漏掉被裁剪的原生组件。
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { createRequire } from 'node:module';
import { readFile, readdir } from 'node:fs/promises';
import { join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

async function smokeRuntime(runtime, work, env) {
  const require = createRequire(join(runtime, 'package.json'));
  const checks = [];
  const backend = join(runtime, 'plugins/wingmate-backends');
  const manifest = JSON.parse(
    await readFile(join(backend, 'package.json'), 'utf8'),
  );
  const adapter = spawnSync(
    process.execPath,
    [join(backend, 'dist/codex-acp.mjs'), '--version'],
    {
      cwd: work,
      env: { ...env, NODE_PATH: '', NODE_OPTIONS: '', PATH: '' },
      windowsHide: true,
      encoding: 'utf8',
      timeout: 10_000,
    },
  );
  assert.equal(adapter.status, 0, adapter.error?.message || adapter.stderr);
  assert.equal(
    adapter.stdout.trim(),
    '@agentclientprotocol/codex-acp ' +
      manifest.devDependencies['@agentclientprotocol/codex-acp'],
  );
  checks.push('内置 ACP 适配器可脱离开发依赖和全局安装启动');
  const { z } = require('zod');
  assert.equal(
    z.object({ value: z.string() }).parse({ value: 'ok' }).value,
    'ok',
  );
  await readFile(join(runtime, 'node_modules/zod/index.d.ts'));
  const png = await require('sharp')({
    create: { width: 2, height: 2, channels: 3, background: '#2459c7' },
  })
    .png()
    .toBuffer();
  assert.equal((await require('sharp')(png).metadata()).width, 2);
  checks.push('释放后的 Zod、类型声明和 Sharp 原生图片处理可用');

  const speech = require('sherpa-onnx-node');
  assert.ok(speech.version);
  assert.equal(typeof speech.OfflineRecognizer, 'function');
  checks.push('语音原生模块及 ONNX 依赖可加载（不下载模型）');

  await new Promise((resolve, reject) => {
    const terminal = require('node-pty').spawn(
      process.execPath,
      ['-e', 'process.stdout.write("wingmate-pty-ok")'],
      { cwd: work, env, cols: 80, rows: 24 },
    );
    let output = '';
    const timer = setTimeout(() => {
      terminal.kill();
      reject(new Error('终端测试超时'));
    }, 15_000);
    terminal.onData((data) => (output += data));
    terminal.onExit(({ exitCode }) => {
      clearTimeout(timer);
      if (exitCode !== 0 || !output.includes('wingmate-pty-ok'))
        reject(new Error(`终端运行失败: ${output}`));
      else resolve();
    });
  });
  checks.push('node-pty 目标架构终端可运行子进程');

  assert.throws(() => require.resolve('@deepseek-ai/libreoffice-kit'));
  const kitFiles = (
    await readdir(join(runtime, 'node_modules'), {
      recursive: true,
      withFileTypes: true,
    })
  ).filter(
    (entry) =>
      entry.isFile() &&
      /[\\/]@deepseek-ai[\\/]libreoffice-kit(?:-[^\\/]+)?[\\/]/.test(
        `${entry.parentPath}/`,
      ),
  );
  assert.ok(
    kitFiles.every((entry) =>
      /^(licen[cs]e|copying|copyright|notice)([.-]|$)/i.test(entry.name),
    ),
  );
  const preview = await readFile(
    join(
      runtime,
      'node_modules/@deepseek-ai/dsh-client-ui-sidebar-documentpreview/lib/client.js',
    ),
    'utf8',
  );
  assert.doesNotMatch(preview, /\bapply\$\d+\(ctx,\s*config\.office\)/);
  const webConfig = spawnSync(
    process.execPath,
    [
      join(runtime, 'node_modules/@deepseek-ai/dsh/lib/bin.js'),
      'web',
      '--patch',
      join(runtime, 'plugins/cordis.patch.yml'),
      '--dump-config',
    ],
    { cwd: work, env, windowsHide: true, encoding: 'utf8', timeout: 25_000 },
  );
  assert.equal(
    webConfig.status,
    0,
    webConfig.error?.message || webConfig.stderr,
  );
  const plugins = require('yaml').parse(webConfig.stdout, {
    logLevel: 'silent',
  });
  for (const id of ['office-kit', 'skill-office']) {
    assert.equal(plugins.find((row) => row.id === id)?.disabled, true, id);
  }
  for (const preset of ['standard', 'ptc']) {
    const rows = plugins.find((row) => row.id === `preset-${preset}`).config
      .plugins;
    assert.ok(
      !rows.some((row) => row.name === '@deepseek-ai/dsh-tool-office'),
      preset,
    );
  }
  checks.push(
    'LibreOffice 运行内容已裁剪，内置 Office 预览、技能和默认预设工具已禁用',
  );

  assert.throws(() =>
    require.resolve('@deepseek-ai/dsh-experimental-inspector'),
  );
  const inspectorProfile = require('yaml').parse(
    await readFile(
      join(
        runtime,
        'node_modules/@deepseek-ai/dsh-experimental-inspector-profile/cordis.patch.yml',
      ),
      'utf8',
    ),
    { logLevel: 'silent' },
  );
  const inspectorRows = inspectorProfile.flatMap((row) => row.insert ?? []);
  assert.ok(
    !inspectorRows.some(
      (row) => row.name === '@deepseek-ai/dsh-experimental-inspector',
    ),
  );
  assert.ok(
    inspectorRows.some(
      (row) => row.name === '@deepseek-ai/dsh-experimental-session-inspector',
    ),
  );
  assert.ok(
    inspectorRows.some(
      (row) => row.name === '@deepseek-ai/dsh-experimental-client-ui-stats',
    ),
  );
  checks.push(
    '实验性内嵌 DevTools 已裁剪，开发者工具保留会话数据诊断与统计页签',
  );

  assert.throws(() => require.resolve('@vscode/ripgrep'));
  const { resolveRgPath } = await import(
    pathToFileURL(
      join(runtime, 'node_modules/@deepseek-ai/dsh-util-ripgrep/lib/index.js'),
    ).href
  );
  const resolvedRg = await resolveRgPath();
  const expectedRg = join(
    env.WINGMATE_HOME,
    'toolchains/fallback',
    process.platform === 'win32' ? 'rg.exe' : 'rg',
  );
  assert.equal(resolvedRg.toLowerCase(), expectedRg.toLowerCase());
  const rgVersion = spawnSync(resolvedRg, ['--version'], {
    cwd: work,
    env,
    windowsHide: true,
    encoding: 'utf8',
    timeout: 10_000,
  });
  assert.equal(
    rgVersion.status,
    0,
    rgVersion.error?.message || rgVersion.stderr,
  );
  assert.match(rgVersion.stdout.trim(), /^ripgrep \d+\.\d+\.\d+/);
  checks.push('重复的 @vscode/ripgrep 已裁剪，dsh-util-ripgrep 复用随包 rg');

  await readFile(
    join(runtime, 'node_modules/@deepseek-ai/dsh/src/bin.ts'),
    'utf8',
  );
  await assert.rejects(
    readFile(
      join(
        runtime,
        'node_modules/@deepseek-ai/dsh-client-ui-settings-account/src/client/assets/onboarding-welcome.webp',
      ),
    ),
    { code: 'ENOENT' },
  );
  checks.push('保留 DSH TypeScript 源码与文档，排除源码目录下的重复静态资源');
  return checks;
}

console.log(
  JSON.stringify(
    await smokeRuntime(
      resolve(process.argv[2]),
      resolve(process.argv[3]),
      process.env,
    ),
  ),
);
// 原生库可能保留线程池；所有操作已完成并 dispose，结束这个专用测试进程。
process.exit(0);
