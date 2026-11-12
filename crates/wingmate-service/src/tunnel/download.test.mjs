import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { mkdtemp, readdir, readFile, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { test } from 'node:test'
import { cloudflaredAsset, downloadInto, fetchCloudflaredRelease } from './download.mjs'

test('API 限流时仅允许固定版本的官方摘要', async t => {
  t.mock.method(globalThis, 'fetch', async () => new Response(null, { status: 429 }))
  const asset = cloudflaredAsset('win32', 'x64')
  const release = await fetchCloudflaredRelease(asset.asset)
  assert.equal(release.digest, asset.digest)
  await assert.rejects(fetchCloudflaredRelease(asset.asset, 'other'), /429/)
})

test('下载或自检失败时保留现有程序并清理临时文件', async () => {
  const directory = await mkdtemp(join(tmpdir(), 'wingmate-cloudflared-'))
  try {
    const asset = { executable: 'cloudflared.exe' }
    const existing = join(directory, asset.executable)
    await writeFile(existing, 'existing')
    const url = 'data:application/octet-stream,invalid'
    await assert.rejects(downloadInto(url, asset, directory, 'sha256:wrong'), /SHA-256/)
    const digest = createHash('sha256').update('invalid').digest('hex')
    await assert.rejects(downloadInto(url, asset, directory, 'sha256:' + digest))
    assert.equal(await readFile(existing, 'utf8'), 'existing')
    assert.deepEqual(await readdir(directory), [asset.executable])
  }
  finally {
    await rm(directory, { recursive: true, force: true })
  }
})
