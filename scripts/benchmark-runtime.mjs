// 比较同一 tar 的压缩体积与纯解压耗时；不修改正式运行包。XZ 对比需要系统 7z。
import { createReadStream, createWriteStream } from 'node:fs';
import { mkdir, mkdtemp, readFile, stat, writeFile } from 'node:fs/promises';
import { pipeline } from 'node:stream/promises';
import { Writable } from 'node:stream';
import { createHash } from 'node:crypto';
import { spawn } from 'node:child_process';
import { join, resolve } from 'node:path';
import { createGunzip, createGzip, createZstdCompress, createZstdDecompress, constants } from 'node:zlib';

const prepared = resolve('work/build/runtime');
const input = process.argv[2] ? resolve(process.argv[2]) : join(prepared,
  `base-${JSON.parse(await readFile(join(prepared, 'manifest.json'))).components.base.sha256}.tar.zst`);
await mkdir('work', { recursive: true });
const output = await mkdtemp(resolve('work/compression-'));
const source = join(output, 'runtime.tar');
await pipeline(createReadStream(input), input.endsWith('.zst') ? createZstdDecompress() : createGunzip(), createWriteStream(source));
function digestSink() {
  const hash = createHash('sha256');
  return { stream: new Writable({ write(chunk, encoding, done) { hash.update(chunk); done(); } }), hash };
}
const expected = digestSink();
await pipeline(createReadStream(source), expected.stream);
const digest = expected.hash.digest('hex');
async function sevenZip(args, sink) {
  const child = spawn('7z', args, { windowsHide: true, stdio: ['ignore', sink ? 'pipe' : 'ignore', 'pipe'] });
  const exited = new Promise((resolve, reject) => {
    let error = '';
    child.stderr.on('data', chunk => error += chunk);
    child.on('error', reject);
    child.on('close', code => {
      if (code !== 0) reject(new Error(`7z 退出 ${code}: ${error}`));
      else resolve();
    });
  });
  await Promise.all([exited, sink ? pipeline(child.stdout, sink) : Promise.resolve()]);
}
const results = [];
for (const format of ['gzip-6', 'gzip-9', 'zstd-19', 'xz-6']) {
  const file = join(output, `${format}.bin`);
  let started = performance.now();
  if (format === 'xz-6') {
    await sevenZip(['a', '-txz', '-mx=6', '-md=8m', '-mmt=1', '-bd', file, source]);
  } else {
    const encoder = format.startsWith('gzip') ? createGzip({ level: Number(format.at(-1)) })
      : createZstdCompress({ params: { [constants.ZSTD_c_compressionLevel]: 19 } });
    await pipeline(createReadStream(source), encoder, createWriteStream(file));
  }
  const compressSeconds = (performance.now() - started) / 1000;
  // 三次热缓存解压，读取到哈希而不写盘，避免把磁盘与海量小文件开销混入算法比较。
  const decodeSeconds = [];
  for (let trial = 0; trial < 3; trial++) {
    const sink = digestSink();
    started = performance.now();
    if (format === 'xz-6') await sevenZip(['x', '-txz', '-so', '-bd', file], sink.stream);
    else await pipeline(createReadStream(file), format.startsWith('gzip') ? createGunzip() : createZstdDecompress(), sink.stream);
    decodeSeconds.push((performance.now() - started) / 1000);
    if (sink.hash.digest('hex') !== digest) throw new Error(`${format} 解压哈希不一致`);
  }
  const result = { format, bytes: (await stat(file)).size, compressSeconds,
    decodeSeconds, medianDecodeSeconds: [...decodeSeconds].sort((a, b) => a - b)[1] };
  results.push(result);
  console.log(JSON.stringify(result));
  await writeFile(join(output, 'result.json'), JSON.stringify({ input, tarBytes: (await stat(source)).size,
    tarSha256: digest, node: process.version, results }, null, 2) + '\n');
}
console.log(`对比结果：${output}`);
