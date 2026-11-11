// Rebuild browser bundles and publish them to the development DSH client watcher.
import { spawn } from 'node:child_process';
import { randomUUID } from 'node:crypto';
import { once } from 'node:events';
import { readdirSync, watch } from 'node:fs';
import { readFile, realpath, rename, rm, writeFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { basename, dirname, isAbsolute, join, relative, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
import { isDeepStrictEqual } from 'node:util';

const root = fileURLToPath(new URL('../', import.meta.url));
const pluginRoot = join(root, 'plugins');
const windows = process.platform === 'win32';

async function clientManifest(directory) {
  try {
    const manifest = JSON.parse(await readFile(join(directory, 'package.json'), 'utf8'));
    if (manifest.dsh?.client && typeof manifest.name === 'string' && manifest.exports?.['./client']) return manifest;
  } catch (error) {
    if (error.code !== 'ENOENT') throw error;
  }
}

async function clientBundle(directory, name) {
  const require = createRequire(join(directory, 'package.json'));
  const bundle = await realpath(require.resolve(name + '/client'));
  const path = relative(await realpath(directory), bundle);
  if (isAbsolute(path) || path === '..' || path.startsWith('..' + sep)) {
    throw new Error('客户端入口必须位于插件目录内');
  }
  return bundle;
}

async function buildClient(directory) {
  const manifest = await clientManifest(directory);
  const script = manifest?.scripts?.['build:client'] ? 'build:client' : 'build';
  if (!manifest?.scripts?.[script]) return;
  const args = ['run', script];
  if (windows) args.unshift(join(dirname(process.execPath), 'node_modules/npm/bin/npm-cli.js'));
  const child = spawn(windows ? process.execPath : 'npm', args, {
    cwd: directory, windowsHide: true, stdio: ['ignore', 'inherit', 'inherit'],
  });
  const [code] = await once(child, 'close');
  if (code !== 0) throw new Error('插件客户端构建失败');
}

async function publishClient(source, installed) {
  const next = await clientManifest(source);
  const current = await clientManifest(installed);
  if (!next || !current) return;
  if (next.name !== current.name || next.version !== current.version
      || next.engines?.dsh !== current.engines?.dsh || !isDeepStrictEqual(next.dsh.client, current.dsh.client)) {
    console.log(`${next.name} 的插件清单已变化，请重新准备开发运行时并更新后台`);
    return;
  }
  const bundle = await clientBundle(source, next.name);
  const destination = await clientBundle(installed, current.name);
  const bytes = await readFile(bundle);
  if (bytes.equals(await readFile(destination))) return;
  const temporary = join(dirname(destination), `.${basename(destination)}-${randomUUID()}.tmp`);
  try {
    await writeFile(temporary, bytes, { flag: 'wx' });
    await rename(temporary, destination);
  } finally {
    await rm(temporary, { force: true });
  }
  console.log(`已发布插件客户端更新 ${next.name}`);
}

const ignored = new Set(['.git', 'node_modules', 'dist', 'lib', 'build', 'coverage', 'test', 'tests']);

function sourceChanged(parts) {
  return !parts.some(part => ignored.has(part) || part.startsWith('.'))
    && /\.(?:[cm]?[jt]sx?|css|html|svg|json|ya?ml)$/i.test(parts.at(-1) ?? '');
}

export function watchPluginClients() {
  const tool = join(root, 'target/debug', windows ? 'wingmate-runtime.exe' : 'wingmate-runtime');
  const pending = new Set();
  let timer;
  let building = false;

  async function rebuild() {
    if (building) return;
    building = true;
    try {
      for (const plugin of pending) {
        pending.delete(plugin);
        try {
          const child = spawn(tool, ['rebuild-dev-client', plugin], { cwd: root, stdio: 'inherit' });
          const [code] = await once(child, 'close');
          if (code !== 0) throw new Error(`退出状态 ${code}`);
        } catch (error) {
          console.error(`无法更新插件客户端 ${plugin}`, error.message);
        }
      }
    } finally {
      building = false;
    }
  }

  function schedule(...plugins) {
    for (const plugin of plugins) pending.add(plugin);
    clearTimeout(timer);
    timer = setTimeout(rebuild, 150);
  }

  function scheduleAll() {
    schedule(...readdirSync(pluginRoot, { withFileTypes: true })
      .filter(entry => entry.isDirectory() && !ignored.has(entry.name) && !entry.name.startsWith('.'))
      .map(entry => entry.name));
  }

  watch(pluginRoot, { recursive: true }, (_event, filename) => {
    if (!filename) return scheduleAll();
    const [plugin, ...parts] = filename.split(/[\\/]/);
    if (!ignored.has(plugin) && !plugin.startsWith('.') && sourceChanged(parts)) schedule(plugin);
  });
  watch(join(root, 'packages'), { recursive: true }, (_event, filename) => {
    if (!filename || sourceChanged(filename.split(/[\\/]/))) scheduleAll();
  });
}

const [action, source, installed] = process.argv.slice(2);
if (action === 'build') await buildClient(source);
if (action === 'publish') await publishClient(source, installed);
