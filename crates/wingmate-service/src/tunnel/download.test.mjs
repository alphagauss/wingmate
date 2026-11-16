import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { releaseAsset, stageCloudflared } from './download.mjs';

const name = 'cloudflared-windows-amd64.exe';
const officialUrl = `https://github.com/cloudflare/cloudflared/releases/download/2026.10.0/${name}`;

test('API 限流时仅允许固定版本的官方摘要', async (t) => {
  t.mock.method(
    globalThis,
    'fetch',
    async () => new Response(null, { status: 429 }),
  );
  assert.deepEqual(await releaseAsset(name), {
    url: officialUrl,
    sha256: '86aee4017b26625cee8484c113558f48effa4cd47f7aa05fcf425604e5d2b23c',
    size: 55_365_048,
  });
  await assert.rejects(releaseAsset(name, 'other'), /429/);
});

test('镜像内容也必须通过官方 SHA-256 校验后才可交给服务', async (t) => {
  const directory = await mkdtemp(join(tmpdir(), 'wingmate-cloudflared-'));
  const target = join(directory, 'cloudflared.exe');
  const content = 'asset contents';
  const digest = createHash('sha256').update(content).digest('hex');
  let mirror = 'asset corrupt!';
  const fetch = t.mock.method(globalThis, 'fetch', async (url) => {
    if (url.startsWith('https://api.github.com/')) {
      return Response.json({
        assets: [
          {
            name,
            browser_download_url: officialUrl,
            digest: `sha256:${digest}`,
            size: content.length,
          },
        ],
      });
    }
    if (url === officialUrl) return new Response(null, { status: 503 });
    assert.equal(url, `https://ghfast.top/${officialUrl}`);
    return new Response(mirror);
  });
  try {
    await assert.rejects(
      stageCloudflared(directory, name, undefined, () => {}),
      /SHA-256/,
    );
    await assert.rejects(readFile(target), { code: 'ENOENT' });

    mirror = content + 'extra';
    await assert.rejects(
      stageCloudflared(directory, name, undefined, () => {}),
      /大小超过/,
    );
    assert.equal((await readFile(join(directory, name))).length, 0);
    await assert.rejects(readFile(target), { code: 'ENOENT' });

    mirror = content;
    const progress = [];
    const requestIndex = fetch.mock.callCount();
    await stageCloudflared(directory, name, '2026.10.0', (event) =>
      progress.push(event),
    );
    assert.match(
      fetch.mock.calls[requestIndex].arguments[0],
      /\/tags\/2026\.10\.0$/,
    );
    assert.equal(await readFile(target, 'utf8'), content);
    assert.deepEqual(progress.at(-1), {
      stage: 'download',
      received: content.length,
      total: content.length,
    });
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});

test('缺少发布摘要时拒绝下载', async (t) => {
  const fetch = t.mock.method(globalThis, 'fetch', async () =>
    Response.json({ assets: [{ name, browser_download_url: officialUrl }] }),
  );
  await assert.rejects(
    stageCloudflared('unused', name, undefined, () => {}),
    /SHA-256/,
  );
  assert.equal(fetch.mock.callCount(), 1);
});
