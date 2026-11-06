// 对 EXE 实际释放的依赖做功能验证，避免只验证 Web 页面而漏掉被裁剪的原生组件。
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { readFile, writeFile } from 'node:fs/promises';
import { join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

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

  const { zipSync, strToU8 } = require('fflate');
  const inputPath = join(work, 'sample.docx'), outputPath = join(work, 'sample.pdf');
  await writeFile(inputPath, zipSync({
    '[Content_Types].xml': strToU8('<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>'),
    '_rels/.rels': strToU8('<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>'),
    'word/document.xml': strToU8('<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>Wingmate runtime smoke test</w:t></w:r></w:p></w:body></w:document>'),
  }));
  const { createConverter } = await import(pathToFileURL(require.resolve('@deepseek-ai/libreoffice-kit')).href);
  const converter = await createConverter({ timeoutMs: 60_000 });
  try {
    await converter.render({ inputPath, outputPath });
    assert.equal((await readFile(outputPath)).subarray(0, 5).toString(), '%PDF-');
  } finally {
    await converter.dispose();
  }
  checks.push('保留的 Office 引擎完成 DOCX → PDF 转换');
  return checks;
}

console.log(JSON.stringify(await smokeRuntime(resolve(process.argv[2]), resolve(process.argv[3]), process.env)));
// 原生库可能保留线程池；所有操作已完成并 dispose，结束这个专用测试进程。
process.exit(0);
