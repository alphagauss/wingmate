import assert from 'node:assert/strict';
import test from 'node:test';
import { readFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { apply } from './host.mjs';
import { mountFrame } from './src/frame-host.js';

test('静态路由只提供构建清单内容，支持 HEAD 和内容指纹缓存', async () => {
  let route;
  const ctx = { webServer: { register(value) { route = value; return () => {}; } }, on() {}, effect: fn => fn() };
  await apply(ctx);
  const manifest = JSON.parse(await readFile(new URL('./dist/assets.json', import.meta.url), 'utf8'));
  // Full resource integrity is checked against the released EXE by test:smoke.
  for (const path of ['frame.html', 'viewer.js', 'docx.worker.js']) {
    const asset = manifest.files[path];
    const request = new Request(`http://localhost${route.path}/${path}`);
    const response = await route.resource(request);
    assert.equal(response.status, 200);
    assert.equal(createHash('sha256').update(Buffer.from(await response.arrayBuffer())).digest('hex'), asset.hash);
    const head = await route.resource(new Request(request, { method: 'HEAD' }));
    assert.equal((await head.arrayBuffer()).byteLength, 0);
    assert.equal(head.headers.get('content-length'), String(asset.size));
    assert.equal((await route.resource(new Request(request, { headers: { 'If-None-Match': head.headers.get('etag') } }))).status, 304);
  }
  for (const path of ['missing.js', '%2e%2e/package.json', '__proto__', 'constructor']) {
    assert.equal((await route.resource(new Request(`http://localhost${route.path}/${path}`))).status, 404);
  }
});

test('解析并发有界，取消排队和运行中的预览，消息只能完成对应 iframe', async () => {
  const window = new EventTarget();
  const frames = new Set();
  const posted = [];
  class Frame extends EventTarget {
    style = {};
    contentWindow = { postMessage(message, origin, transfer) {
      posted.push(structuredClone(message, { transfer }));
    } };
    setAttribute() {}
    remove() { frames.delete(this); }
  }
  const container = { append(frame) { frames.add(frame); frame.dispatchEvent(new Event('load')); } };
  const saved = { window: globalThis.window, document: globalThis.document, location: globalThis.location };
  Object.assign(globalThis, { window, location: { origin: 'http://localhost' },
    document: { createElement: () => new Frame(), documentElement: { lang: 'en', classList: { contains: () => false } } } });
  const controllers = Array.from({ length: 4 }, () => new AbortController());
  const bytes = new Uint8Array([1, 2, 3]);
  const start = index => mountFrame(container, '/assets', bytes, 'test.docx', controllers[index].signal);
  const done = source => window.dispatchEvent(Object.assign(new Event('message'), {
    source, origin: 'http://localhost', data: { type: 'wingmate-preview-loaded' },
  }));
  try {
    const first = start(0), second = start(1), queued = start(2);
    const cancelled = assert.rejects(queued, { name: 'AbortError' });
    await new Promise(resolve => setImmediate(resolve));
    assert.equal(frames.size, 2);
    controllers[2].abort();
    await cancelled;
    assert.equal(bytes.byteLength, 3, 'shared DSH buffer must not be detached');
    assert.deepEqual([...new Uint8Array(posted[0].buffer)], [1, 2, 3]);
    done({});
    const [one, two] = [...frames];
    done(one.contentWindow);
    await first;
    const next = start(3);
    const abortNext = assert.rejects(next, { name: 'AbortError' });
    await new Promise(resolve => setImmediate(resolve));
    assert.equal(frames.size, 3, 'completed views do not occupy a parser slot');
    controllers[3].abort();
    await abortNext;
    done(two.contentWindow);
    await second;
    controllers[0].abort();
    controllers[1].abort();
    assert.equal(frames.size, 0, 'closing finished views releases their frames');
  } finally {
    for (const controller of controllers) controller.abort();
    for (const [key, value] of Object.entries(saved)) {
      if (value === undefined) delete globalThis[key];
      else globalThis[key] = value;
    }
  }
});
