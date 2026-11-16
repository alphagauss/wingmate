import { execFile } from 'node:child_process';
import { createHash } from 'node:crypto';
import { createWriteStream } from 'node:fs';
import { chmod, rename } from 'node:fs/promises';
import { join } from 'node:path';
import { pipeline } from 'node:stream/promises';
import { promisify } from 'node:util';

const API = 'https://api.github.com/repos/cloudflare/cloudflared/releases';
const DOWNLOAD = 'https://github.com/cloudflare/cloudflared/releases/download/';
const FALLBACK_TAG = '2026.10.0';
// Asset sizes and SHA-256 values from Cloudflare's 2026.10.0 GitHub release.
const FALLBACK_ASSETS = {
  'cloudflared-darwin-amd64.tgz': {
    size: 21_741_581,
    sha256: '903845b81828c8cb3c5d13d816a2de71c06a3da5785469df8eb0e1b736d92f9f',
  },
  'cloudflared-darwin-arm64.tgz': {
    size: 19_809_074,
    sha256: 'a2f79ff7b9420aa537d74af239f376da170bbabeb529aec416002adac6a72e70',
  },
  'cloudflared-linux-amd64': {
    size: 40_129_756,
    sha256: 'd33ff2d14475178d2012c2c56beba87389ac5ded27649519f198a7d3134a99db',
  },
  'cloudflared-linux-arm64': {
    size: 37_687_584,
    sha256: 'e6422b9d4f72d3194bc5a38676f13667c06666523217b842a877d72a80b5ac08',
  },
  'cloudflared-windows-amd64.exe': {
    size: 55_365_048,
    sha256: '86aee4017b26625cee8484c113558f48effa4cd47f7aa05fcf425604e5d2b23c',
  },
};

const ASSETS = {
  'darwin-x64': 'cloudflared-darwin-amd64.tgz',
  'darwin-arm64': 'cloudflared-darwin-arm64.tgz',
  'linux-x64': 'cloudflared-linux-amd64',
  'linux-arm64': 'cloudflared-linux-arm64',
  'win32-x64': 'cloudflared-windows-amd64.exe',
};

export async function releaseAsset(name, version) {
  const route = version ? `tags/${encodeURIComponent(version)}` : 'latest';
  const response = await fetch(`${API}/${route}`, {
    headers: { accept: 'application/vnd.github+json' },
    signal: AbortSignal.timeout(20_000),
  });
  if (!response.ok) {
    if (
      [403, 429].includes(response.status) &&
      (!version || version === FALLBACK_TAG) &&
      Object.hasOwn(FALLBACK_ASSETS, name)
    ) {
      return {
        url: `${DOWNLOAD}${FALLBACK_TAG}/${name}`,
        ...FALLBACK_ASSETS[name],
      };
    }
    throw new Error(`无法读取 Cloudflare 发布信息（HTTP ${response.status}）`);
  }

  const { assets } = await response.json();
  const asset = assets?.find((entry) => entry.name === name);
  if (!/^sha256:[a-f0-9]{64}$/.test(asset?.digest ?? '')) {
    throw new Error(`Cloudflare 发布信息缺少 ${name} 的 SHA-256 摘要`);
  }
  if (!asset.browser_download_url?.startsWith(DOWNLOAD)) {
    throw new Error(`Cloudflare 发布信息缺少 ${name} 的官方地址`);
  }
  if (!Number.isSafeInteger(asset.size) || asset.size <= 0) {
    throw new Error(`Cloudflare 发布信息缺少 ${name} 的有效大小`);
  }
  return {
    url: asset.browser_download_url,
    sha256: asset.digest.slice('sha256:'.length),
    size: asset.size,
  };
}

async function fetchPackage(url, asset, file, report) {
  const total = asset.size;
  report({ stage: 'download', received: 0, total });
  const response = await fetch(url, { signal: AbortSignal.timeout(600_000) });
  if (!response.ok || !response.body)
    throw new Error(`HTTP ${response.status}`);

  const hash = createHash('sha256');
  let received = 0;
  let lastReport = 0;
  await pipeline(
    response.body,
    async function* (chunks) {
      for await (const chunk of chunks) {
        received += chunk.byteLength;
        if (received > total) throw new Error('下载大小超过官方发布信息');
        hash.update(chunk);
        if (received - lastReport >= 256 * 1024) {
          report({ stage: 'download', received, total });
          lastReport = received;
        }
        yield chunk;
      }
    },
    createWriteStream(file),
  );
  if (received !== total) throw new Error('下载大小与官方发布信息不符');
  if (hash.digest('hex') !== asset.sha256) {
    throw new Error('SHA-256 校验失败');
  }
  report({ stage: 'download', received, total });
}

// The service owns the staging directory and publishes the executable after checking its version.
export async function stageCloudflared(directory, name, version, report) {
  const asset = await releaseAsset(name, version);
  const file = join(directory, name);
  try {
    await fetchPackage(asset.url, asset, file, report);
  } catch (directError) {
    try {
      await fetchPackage(
        `https://ghfast.top/${asset.url}`,
        asset,
        file,
        report,
      );
    } catch (mirrorError) {
      throw new Error(
        `cloudflared 下载失败（官方 ${directError.message}，镜像 ${mirrorError.message}）`,
      );
    }
  }

  const executable = name.endsWith('.exe') ? 'cloudflared.exe' : 'cloudflared';
  const target = join(directory, executable);
  if (name.endsWith('.tgz')) {
    report({ stage: 'extract' });
    await promisify(execFile)(
      'tar',
      ['-xzf', file, '-C', directory, executable],
      { timeout: 60_000 },
    );
  } else {
    await rename(file, target);
  }
  await chmod(target, 0o755);
}

if (process.argv[1] === '-') {
  try {
    const platform = `${process.platform}-${process.arch}`;
    const name = ASSETS[platform];
    if (!name)
      throw new Error(
        `当前平台 ${platform} 请手动安装 cloudflared 或设置 CLOUDFLARED_PATH`,
      );
    await stageCloudflared(
      process.argv[2],
      name,
      process.env.CLOUDFLARED_VERSION,
      (progress) => {
        console.log(JSON.stringify(progress));
      },
    );
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
