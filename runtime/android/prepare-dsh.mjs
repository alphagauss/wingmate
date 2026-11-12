import { copyFile, readFile, readdir, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { excludedRuntimeFile } from '../../scripts/runtime-files.mjs';
import { applyRuntimePatches } from '../../scripts/runtime-patches.mjs';

const [prefix, output] = process.argv.slice(2);
const version = JSON.parse(await readFile(join(prefix, 'package.json'), 'utf8')).dependencies['@deepseek-ai/dsh'];
const patches = JSON.parse(await readFile(new URL(`../patches/dsh-${version}.json`, import.meta.url), 'utf8'));
const androidPatches = JSON.parse(await readFile(new URL(`patches/dsh-${version}.json`, import.meta.url), 'utf8'));
await applyRuntimePatches(prefix, [...patches, ...androidPatches]);

const modules = join(prefix, 'node_modules');
await copyFile(new URL('webview-compat.js', import.meta.url),
  join(modules, '@deepseek-ai/dsh-web-frontend/dist/assets/wingmate-webview-compat.js'));
const excluded = {};
for (const entry of await readdir(modules, { recursive: true, withFileTypes: true })) {
  if (!entry.isFile()) continue;
  const file = join(entry.parentPath, entry.name).slice(prefix.length + 1);
  const reason = excludedRuntimeFile(file, 'linux', 'arm64');
  if (reason) excluded[file] = reason;
}
await writeFile(output, JSON.stringify(excluded));
