// 对 EXE 实际释放的依赖做功能验证，避免只验证 Web 页面而漏掉被裁剪的原生组件。
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { readFile, readdir } from 'node:fs/promises';
import { join, resolve } from 'node:path';

async function smokeRuntime(runtime, work, env) {
  const require = createRequire(join(runtime, 'package.json'));
  const checks = [];
  const { z } = require('zod');
  assert.equal(z.object({ value: z.string() }).parse({ value: 'ok' }).value, 'ok');
  await readFile(join(runtime, 'node_modules/zod/index.d.ts'));
  const png = await require('sharp')({ create: { width: 2, height: 2, channels: 3, background: '#2459c7' } }).png().toBuffer();
  assert.equal((await require('sharp')(png).metadata()).width, 2);
  checks.push('释放后的 Zod、类型声明和 Sharp 原生图片处理可用');

  const speech = require('sherpa-onnx-node');
  assert.ok(speech.version);
  assert.equal(typeof speech.OfflineRecognizer, 'function');
  checks.push('语音原生模块及 ONNX 依赖可加载（不下载模型）');

  await new Promise((resolve, reject) => {
    const terminal = require('node-pty').spawn(process.execPath,
      ['-e', 'process.stdout.write("wingmate-pty-ok")'], { cwd: work, env, cols: 80, rows: 24 });
    let output = '';
    const timer = setTimeout(() => { terminal.kill(); reject(new Error('终端测试超时')); }, 15_000);
    terminal.onData(data => output += data);
    terminal.onExit(({ exitCode }) => {
      clearTimeout(timer);
      if (exitCode !== 0 || !output.includes('wingmate-pty-ok')) reject(new Error(`终端运行失败: ${output}`));
      else resolve();
    });
  });
  checks.push('node-pty 目标架构终端可运行子进程');

  assert.throws(() => require.resolve('@deepseek-ai/libreoffice-kit'));
  const kitFiles = (await readdir(join(runtime, 'node_modules'), { recursive: true, withFileTypes: true }))
    .filter(entry => entry.isFile() && /[\\/]@deepseek-ai[\\/]libreoffice-kit(?:-[^\\/]+)?[\\/]/.test(`${entry.parentPath}/`));
  assert.ok(kitFiles.every(entry => /^(licen[cs]e|copying|copyright|notice)([.-]|$)/i.test(entry.name)));
  const preview = await readFile(join(runtime, 'node_modules/@deepseek-ai/dsh-client-ui-sidebar-documentpreview/lib/client.js'), 'utf8');
  assert.ok(!preview.includes('apply$2(ctx, config.office);'));
  checks.push('LibreOffice 运行内容已裁剪，旧 Office 预览入口已移除');
  return checks;
}

console.log(JSON.stringify(await smokeRuntime(resolve(process.argv[2]), resolve(process.argv[3]), process.env)));
// 原生库可能保留线程池；所有操作已完成并 dispose，结束这个专用测试进程。
process.exit(0);
