// 启动页与网关共用，连接面板由桌面壳使用。
import { watch } from 'node:fs';
import { mkdir, copyFile } from 'node:fs/promises';
import { basename, dirname } from 'node:path';

const dist = new URL('./dist/', import.meta.url);
const assets = [
  ['../launcher/index.html', 'index.html'],
  ['../launcher/home.js', '_wingmate/home.js'],
  ['./connections.html', 'connections.html'],
  ['./connections.js', 'connections.js'],
  ['../../src-tauri/icons/logo.svg', '_wingmate/logo.svg'],
];

function copyAsset([source, destination]) {
  return copyFile(new URL(source, import.meta.url), new URL(destination, dist));
}

await mkdir(new URL('_wingmate/', dist), { recursive: true });
if (process.argv.includes('--watch')) {
  const pending = new Set();
  let debounce;
  // Watch the containing directories so editor saves that replace a file also reach dist.
  for (const [directory, files] of Map.groupBy(assets, ([source]) => dirname(source))) {
    watch(new URL(directory, import.meta.url), (_event, filename) => {
      const changed = filename ? files.filter(([source]) => basename(source) === filename) : files;
      if (!changed.length) return;
      for (const asset of changed) pending.add(asset);
      clearTimeout(debounce);
      debounce = setTimeout(() => {
        const copies = [...pending].map(copyAsset);
        pending.clear();
        Promise.all(copies).catch(error => console.error('无法同步桌面开发资源', error.message));
      }, 80);
    });
  }
  const { watchPluginClients } = await import('../../scripts/dev-plugin-clients.mjs');
  watchPluginClients();
}
await Promise.all(assets.map(copyAsset));
