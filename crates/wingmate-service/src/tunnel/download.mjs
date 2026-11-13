// Adapted from deepseek-harness-desktop fa55bd68883e.
// See LICENSE.desktop and the additional upstream terms in ../../README.md.
import { Buffer } from 'node:buffer'
import { execFile } from 'node:child_process'
import { createHash } from 'node:crypto'
import { chmod, mkdir, mkdtemp, rename, rm, writeFile } from 'node:fs/promises'
import { join } from 'node:path'
import { promisify } from 'node:util'

const execFileAsync = promisify(execFile)

const RELEASE_BASE = 'https://github.com/cloudflare/cloudflared/releases/download/'

const RELEASE_API = 'https://api.github.com/repos/cloudflare/cloudflared/releases/latest'

const RELEASE_TIMEOUT_MS = 20_000

// API 限流时使用固定版本和官方资产摘要。
const FALLBACK_VERSION = '2026.10.0'

const MIRROR_PREFIX = 'https://ghfast.top/'

const DOWNLOAD_TIMEOUT_MS = 600_000
const VERSION_TIMEOUT_MS = 5_000
const EXTRACT_TIMEOUT_MS = 60_000

const RE_VERSION = /^cloudflared version \d+\.\d+\.\d+/m

const ASSETS = {
  'darwin-arm64': { asset: 'cloudflared-darwin-arm64.tgz', executable: 'cloudflared', archive: true, digest: 'sha256:a2f79ff7b9420aa537d74af239f376da170bbabeb529aec416002adac6a72e70' },
  'darwin-x64': { asset: 'cloudflared-darwin-amd64.tgz', executable: 'cloudflared', archive: true, digest: 'sha256:903845b81828c8cb3c5d13d816a2de71c06a3da5785469df8eb0e1b736d92f9f' },
  'linux-x64': { asset: 'cloudflared-linux-amd64', executable: 'cloudflared', archive: false, digest: 'sha256:d33ff2d14475178d2012c2c56beba87389ac5ded27649519f198a7d3134a99db' },
  'linux-arm64': { asset: 'cloudflared-linux-arm64', executable: 'cloudflared', archive: false, digest: 'sha256:e6422b9d4f72d3194bc5a38676f13667c06666523217b842a877d72a80b5ac08' },
  'win32-x64': { asset: 'cloudflared-windows-amd64.exe', executable: 'cloudflared.exe', archive: false, digest: 'sha256:86aee4017b26625cee8484c113558f48effa4cd47f7aa05fcf425604e5d2b23c' },
}

export function cloudflaredAsset(platform = process.platform, arch = process.arch) {
  return ASSETS[`${platform}-${arch}`]
}

/** 候选下载地址：官方 tag 直链优先，镜像只做传输兜底；两者都用同一份官方摘要校验。 */
export function cloudflaredDownloadUrls(asset, tag) {
  const primary = `${RELEASE_BASE}${tag}/${asset}`
  return [primary, `${MIRROR_PREFIX}${primary}`]
}

export async function fetchCloudflaredRelease(asset, version) {
  const url = version === undefined || version === '' ? RELEASE_API : `https://api.github.com/repos/cloudflare/cloudflared/releases/tags/${version}`
  const response = await fetch(url, { signal: AbortSignal.timeout(RELEASE_TIMEOUT_MS), headers: { accept: 'application/vnd.github+json' } })
  if (!response.ok) {
    const fallback = Object.values(ASSETS).find(entry => entry.asset === asset)
    if ((response.status === 403 || response.status === 429) && (version === undefined || version === '' || version === FALLBACK_VERSION) && fallback !== undefined)
      return { tag: FALLBACK_VERSION, digest: fallback.digest }
    throw new Error(`发布元数据不可用（HTTP ${response.status}）`)
  }
  const body = await response.json()
  const tag = typeof body.tag_name === 'string' ? body.tag_name : ''
  const entries = Array.isArray(body.assets) ? body.assets : []
  const found = entries.find(entry => entry?.name === asset)
  const digest = typeof found?.digest === 'string' ? found.digest : ''
  if (tag === '' || !/^sha256:[0-9a-f]{64}$/.test(digest))
    throw new Error(`官方未提供 ${asset} 的 sha256 摘要，拒绝执行未校验的二进制`)
  return { tag, digest, ...typeof found?.size === 'number' ? { size: found.size } : {} }
}

export async function downloadCloudflared(directory, onProgress, version = process.env.CLOUDFLARED_VERSION) {
  const asset = cloudflaredAsset()
  if (asset === undefined)
    throw new Error(`不支持自动下载的平台：${process.platform}-${process.arch}；请安装 cloudflared 或设置 CLOUDFLARED_PATH`)
  const release = await fetchCloudflaredRelease(asset.asset, version)
  const failures = []
  for (const url of cloudflaredDownloadUrls(asset.asset, release.tag)) {
    try {
      onProgress?.({ stage: 'download', received: 0, total: release.size })
      return await downloadInto(url, asset, directory, release.digest, onProgress)
    }
    catch (error) {
      failures.push(`${url} → ${error.message}`)
    }
  }
  throw new Error(`cloudflared 下载失败（${failures.join('；')}）`)
}

export async function downloadInto(url, asset, directory, digest, onProgress) {
  const response = await fetch(url, { signal: AbortSignal.timeout(DOWNLOAD_TIMEOUT_MS) })
  if (!response.ok)
    throw new Error(`HTTP ${response.status}`)
  const total = Number(response.headers.get('content-length')) || undefined
  const chunks = []
  let received = 0
  let reported = 0
  for await (const chunk of response.body ?? []) {
    chunks.push(chunk)
    received += chunk.length
    if (received - reported >= 256 * 1024) {
      onProgress?.({ stage: 'download', received, total })
      reported = received
    }
  }
  if (received === 0)
    throw new Error('响应为空')
  onProgress?.({ stage: 'download', received, total })
  const body = Buffer.concat(chunks, received)
  if (`sha256:${createHash('sha256').update(body).digest('hex')}` !== digest)
    throw new Error('SHA-256 校验失败')

  await mkdir(directory, { recursive: true, mode: 0o700 })
  const staging = await mkdtemp(join(directory, '.download-'))
  try {
    const executable = join(staging, asset.executable)
    if (asset.archive) {
      onProgress?.({ stage: 'extract' })
      const archive = join(staging, asset.asset)
      await writeFile(archive, body)
      await execFileAsync('tar', ['-xzf', archive, '-C', staging, asset.executable], { timeout: EXTRACT_TIMEOUT_MS })
    }
    else {
      await writeFile(executable, body)
    }
    await chmod(executable, 0o755)
    onProgress?.({ stage: 'verify' })
    const { stdout, stderr } = await execFileAsync(executable, ['--version'], { timeout: VERSION_TIMEOUT_MS, windowsHide: true })
    if (!RE_VERSION.test(`${stdout}\n${stderr}`))
      throw new Error('cloudflared --version 自检失败')
    const target = join(directory, asset.executable)
    await rename(executable, target)
    return target
  }
  finally {
    await rm(staging, { recursive: true, force: true })
  }
}

if (process.argv[1] === '-') {
  try {
    const report = event => console.log(JSON.stringify(event))
    const path = await downloadCloudflared(process.argv[2], report)
    report({ stage: 'installed', path })
  }
  catch (error) {
    console.error(error.message)
    process.exitCode = 1
  }
}
