import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { createReadStream } from 'node:fs';
import {
  chmod,
  copyFile,
  mkdir,
  readFile,
  rename,
  rm,
  writeFile,
} from 'node:fs/promises';
import { join } from 'node:path';
import { excludedRuntimeFile } from './runtime-files.mjs';

const windows = process.platform === 'win32';
const nodeLock = new URL('../runtime/locks/node.json', import.meta.url);
const cliToolsLock = new URL(
  '../runtime/locks/cli-tools.json',
  import.meta.url,
);
const jqLicense = new URL('../runtime/licenses/jq-COPYING', import.meta.url);
const pnpmLock = new URL('../runtime/locks/pnpm/', import.meta.url);

export async function toolchainInputs() {
  return Promise.all([
    readFile(nodeLock),
    readFile(cliToolsLock),
    readFile(jqLicense),
    readFile(new URL('package.json', pnpmLock)),
    readFile(new URL('package-lock.json', pnpmLock)),
    readFile(new URL(import.meta.url)),
    readFile(new URL('./runtime-files.mjs', import.meta.url)),
  ]);
}

function run(command, args, cwd) {
  const result = spawnSync(command, args, {
    cwd,
    stdio: ['ignore', 'inherit', 'inherit'],
    windowsHide: true,
  });
  if (result.error) throw result.error;
  return result.status === 0;
}

async function checksum(path) {
  const hash = createHash('sha256');
  try {
    for await (const chunk of createReadStream(path)) hash.update(chunk);
  } catch (error) {
    if (error.code === 'ENOENT') return null;
    throw error;
  }
  return hash.digest('hex');
}

async function downloadVerified(cache, stage, filename, sha256, urls) {
  const archive = join(cache, filename);
  await mkdir(cache, { recursive: true });
  if ((await checksum(archive)) === sha256) return archive;
  const partial = join(stage, filename);
  for (const url of urls) {
    if (
      !run('curl', [
        '-fsSL',
        '--retry',
        '1',
        '--connect-timeout',
        '10',
        '--max-time',
        '120',
        '-o',
        partial,
        url,
      ])
    )
      continue;
    if ((await checksum(partial)) !== sha256)
      throw new Error(`下载校验失败: ${url}`);
    await rename(partial, archive);
    return archive;
  }
  throw new Error(`无法下载 ${filename}`);
}

export async function prepareToolchains(
  cache,
  stage,
  npmCache,
  registry,
  cliCache = cache,
) {
  const lock = JSON.parse(await readFile(nodeLock));
  const platform = windows ? 'win' : process.platform;
  const extension = windows
    ? 'zip'
    : platform === 'linux'
      ? 'tar.xz'
      : 'tar.gz';
  const target = `${platform}-${process.arch}`;
  const asset = `${target}.${extension}`;
  const sha256 = lock.archives[asset];
  if (!sha256) throw new Error(`没有适用于 ${asset} 的 Node 分发锁`);
  const prefix = `node-v${lock.version}-${target}`;
  const filename = `${prefix}.${extension}`;
  const archive = await downloadVerified(
    cache,
    stage,
    filename,
    sha256,
    lock.origins.map((origin) => `${origin}/v${lock.version}/${filename}`),
  );
  const directory = join(stage, 'toolchains');
  await mkdir(directory);
  const entries = windows ? lock.windowsEntries : lock.unixEntries;
  if (
    !run(
      'tar',
      [
        '-xf',
        archive,
        '--strip-components=1',
        ...entries.map((entry) => `${prefix}/${entry}`),
      ],
      directory,
    )
  )
    throw new Error(`无法解包 ${filename}`);

  const bin = windows ? directory : join(directory, 'bin');
  const modules = windows
    ? join(directory, 'node_modules')
    : join(directory, 'lib/node_modules');
  await rm(join(modules, 'corepack'), { recursive: true, force: true });
  if (!windows) await rm(join(bin, 'corepack'), { force: true });
  const node = join(bin, windows ? 'node.exe' : 'node');
  const install = join(stage, 'pnpm-install');
  await mkdir(install);
  for (const filename of ['package.json', 'package-lock.json'])
    await copyFile(new URL(filename, pnpmLock), join(install, filename));
  const npm = join(modules, 'npm/bin/npm-cli.js');
  if (
    !run(
      node,
      [
        npm,
        'ci',
        '--omit=dev',
        '--ignore-scripts',
        '--no-audit',
        '--no-fund',
        '--cache',
        npmCache,
        `--registry=${registry}`,
        '--replace-registry-host=always',
      ],
      install,
    )
  )
    throw new Error('无法准备随包 pnpm');
  await rename(join(install, 'node_modules/pnpm'), join(modules, 'pnpm'));
  for (const [name, entry] of [
    ['npm', 'npm/bin/npm-cli.js'],
    ['npx', 'npm/bin/npx-cli.js'],
    ['pnpm', 'pnpm/bin/pnpm.mjs'],
    ['pnpx', 'pnpm/bin/pnpx.mjs'],
  ]) {
    const path = join(bin, windows ? `${name}.cmd` : name);
    await rm(path, { force: true });
    await writeFile(
      path,
      windows
        ? `@setlocal DisableDelayedExpansion\r\n@set "PATH=%~dp0;%PATH%"\r\n@"%~dp0node.exe" "%~dp0node_modules/${entry}" %*\r\n`
        : `#!/bin/sh\nbin=${'${0%/*}'}\nexport PATH="$bin${'${PATH:+:$PATH}'}"\nexec "$bin/node" "$bin/../lib/node_modules/${entry}" "$@"\n`,
      { mode: 0o755 },
    );
  }
  await prepareFallbackTools(cliCache, stage, directory, target);
  return {
    cwd: stage,
    filter: (path, entry) => !entry.isFile() || !excludedRuntimeFile(path),
  };
}

async function prepareFallbackTools(cache, stage, directory, target) {
  const tools = JSON.parse(await readFile(cliToolsLock));
  const fallback = join(directory, 'fallback');
  const licenses = join(directory, 'licenses');
  await mkdir(fallback, { recursive: true });

  const rg = tools.rg.assets[target];
  if (!rg) throw new Error(`没有适用于 ${target} 的 rg 分发锁`);
  const rgArchive = await downloadVerified(
    cache,
    stage,
    rg.file,
    rg.sha256,
    tools.rg.origins.map((origin) => `${origin}/${rg.file}`),
  );
  const rgStage = join(stage, 'rg-extract');
  await mkdir(rgStage);
  const rgPrefix = rg.file.replace(/\.(?:zip|tar\.gz)$/, '');
  const rgBin = windows ? 'rg.exe' : 'rg';
  if (
    !run(
      'tar',
      [
        '-xf',
        rgArchive,
        '--strip-components=1',
        `${rgPrefix}/${rgBin}`,
        ...tools.rg.licenses.map((name) => `${rgPrefix}/${name}`),
      ],
      rgStage,
    )
  )
    throw new Error(`无法解包 ${rg.file}`);
  const rgTarget = join(fallback, rgBin);
  await rename(join(rgStage, rgBin), rgTarget);
  if (!windows) await chmod(rgTarget, 0o755);
  const rgLicenses = join(licenses, 'ripgrep');
  await mkdir(rgLicenses, { recursive: true });
  for (const name of tools.rg.licenses)
    await rename(join(rgStage, name), join(rgLicenses, name));

  const jq = tools.jq.assets[target];
  if (!jq) throw new Error(`没有适用于 ${target} 的 jq 分发锁`);
  const jqFile = await downloadVerified(
    cache,
    stage,
    jq.file,
    jq.sha256,
    tools.jq.origins.map((origin) => `${origin}/${jq.file}`),
  );
  const jqTarget = join(fallback, windows ? 'jq.exe' : 'jq');
  await copyFile(jqFile, jqTarget);
  if (!windows) await chmod(jqTarget, 0o755);
  const jqLicenses = join(licenses, 'jq');
  await mkdir(jqLicenses, { recursive: true });
  await copyFile(jqLicense, join(jqLicenses, 'COPYING'));
}
