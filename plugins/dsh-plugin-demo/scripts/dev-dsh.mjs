import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { existsSync, watch } from 'node:fs';
import { readFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { buildPlugin } from './build.mjs';

const root = fileURLToPath(new URL('../', import.meta.url));
const args = process.argv.slice(2);
if (args.length && (args.length !== 2 || args[0] !== '--runtime'))
  throw new Error(
    '用法：pnpm dev:dsh [--runtime <Wingmate 开发 runtime.json>]',
  );
let runtimeFile = args[1] && resolve(args[1]);
for (let directory = root; !runtimeFile; directory = dirname(directory)) {
  const candidate = join(directory, 'work/dev/wingmate/runtime.json');
  if (existsSync(candidate)) runtimeFile = candidate;
  else if (dirname(directory) === directory) break;
}
if (!runtimeFile)
  throw new Error(
    '请先运行 Wingmate 的 pnpm dev；仓库外项目可用 --runtime 指定开发版 runtime.json',
  );
const runtime = JSON.parse(await readFile(runtimeFile, 'utf8'));
const wingmateRoot = resolve(dirname(runtimeFile), '../../..');
const launcher = join(
  wingmateRoot,
  'target/debug',
  process.platform === 'win32' ? 'wingmate.exe' : 'wingmate',
);
if (!existsSync(launcher)) throw new Error('未找到 Wingmate 开发版命令');
const controller = new AbortController();
const watchers = [];
let timer,
  pending = false,
  building,
  cookie,
  origin;
let finish;
const stopped = new Promise((done) => {
  finish = done;
});
const stop = () => {
  controller.abort();
  clearTimeout(timer);
  for (const watcher of watchers) watcher.close();
  finish();
};
process.once('SIGINT', stop);
process.once('SIGTERM', stop);

async function publish() {
  controller.signal.throwIfAborted();
  const response = await fetch(origin + '/api/wingmate-devtools', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json', Cookie: cookie },
    body: JSON.stringify({
      action: 'preview',
      directory: join(root, '.dev/dsh'),
    }),
    signal: controller.signal,
  });
  if (!response.headers.get('content-type')?.includes('application/json'))
    throw new Error(
      '开发接口尚未就绪（HTTP ' + response.status + '），请确认开发后台已启动',
    );
  const result = await response.json();
  if (!response.ok || result.application !== 'applied') {
    throw new Error(
      result.error
        ? JSON.stringify(result.error)
        : '开发插件尚未生效：' + JSON.stringify(result),
    );
  }
  console.log('已更新开发插件；退出命令后停止监听。');
}
async function rebuild() {
  if (building) {
    pending = true;
    return building;
  }
  building = (async () => {
    do {
      pending = false;
      try {
        await buildPlugin(true);
        await publish();
      } catch (error) {
        if (!controller.signal.aborted) console.error(error.message);
      }
    } while (pending && !controller.signal.aborted);
  })();
  try {
    await building;
  } finally {
    building = undefined;
  }
}
function schedule() {
  clearTimeout(timer);
  timer = setTimeout(() => void rebuild(), 150);
}

try {
  // Obtain a dedicated local session through the public CLI; never print its credentials.
  const { stdout } = await promisify(execFile)(launcher, ['share-pair-local'], {
    cwd: wingmateRoot,
    windowsHide: true,
    timeout: 15000,
    env: {
      ...process.env,
      WINGMATE_HOME: dirname(runtimeFile),
      DSH_HOME: runtime.dsh_home,
      DSH_AGENTS_HOME: runtime.agents_home,
    },
    signal: controller.signal,
  });
  const pairing = JSON.parse(stdout).pairing_url;
  if (!pairing || new URL(pairing).hostname !== '127.0.0.1')
    throw new Error('开发后台未返回本机配对地址');
  origin = new URL(pairing).origin;
  const paired = await fetch(origin + '/?auth', {
    method: 'POST',
    body: new URL(pairing).searchParams.get('auth'),
    headers: { Origin: origin },
    signal: controller.signal,
  });
  cookie = paired.headers
    .getSetCookie()
    .map((value) => value.split(';')[0])
    .join('; ');
  if (!cookie) throw new Error('开发后台配对失败，请确认开发版已启动');
  for (let attempt = 0; ; attempt++) {
    const ready = await fetch(origin + '/api/wingmate-devtools', {
      headers: { Cookie: cookie },
      signal: AbortSignal.any([controller.signal, AbortSignal.timeout(1500)]),
    });
    if (
      ready.ok &&
      ready.headers.get('content-type')?.includes('application/json')
    )
      break;
    if (attempt >= 59 || ready.status === 401)
      throw new Error('开发后台未就绪，请确认 Wingmate 开发版已启动');
    await new Promise((done) => setTimeout(done, 500));
  }
  await rebuild();
  for (const directory of ['src', 'locale'])
    watchers.push(watch(join(root, directory), { recursive: true }, schedule));
  watchers.push(
    watch(join(root, 'scripts'), { recursive: true }, () =>
      console.log('构建脚本已变化，请重启 pnpm dev:dsh。'),
    ),
  );
  watchers.push(
    watch(root, (_event, filename) => {
      if (
        [
          'package.json',
          'tsconfig.json',
          'cordis.patch.yml',
          'README.md',
          'icon.svg',
        ].includes(String(filename))
      )
        schedule();
    }),
  );
  console.log('正在监听插件源码：' + root);
  await stopped;
} finally {
  stop();
  await building;
  if (origin && cookie)
    await fetch(origin + '/_wingmate/logout', {
      method: 'POST',
      headers: { Cookie: cookie, Origin: origin },
      signal: AbortSignal.timeout(3000),
    }).catch(() => {});
}
