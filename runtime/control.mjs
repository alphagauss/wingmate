// 由启动器加载到 DSH 所在的 Node 进程，负责本机发现和关闭。
import { randomUUID } from 'node:crypto';
import { once } from 'node:events';
import { renameSync, rmSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { createRequire } from 'node:module';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';

let handoff = '';
for await (const chunk of process.stdin) handoff += chunk;
if (handoff !== 'start\n') process.exit(1);

// The preload lives outside the runtime tree. Resolve the helper from the DSH entry.
const requireFromDsh = createRequire(process.argv[1]);
const { resolveAgentsHome, resolveDshHome } = await import(pathToFileURL(requireFromDsh.resolve('@deepseek-ai/dsh-home-paths')));
const dsh_home = resolveDshHome();
const agents_home = resolveAgentsHome();
const home = process.env.WINGMATE_HOME;
const runtime = process.env.WINGMATE_RUNTIME_ID;
const runtime_revision = process.env.WINGMATE_RUNTIME_REVISION || null;
const endpointFile = join(home, 'endpoint.json');
const identity = { product: 'wingmate', protocol: 2, instance: randomUUID(), runtime, pid: process.pid };
const token = randomUUID();
let url = null;
let stopping = false;

function shutdown() {
  if (process.listenerCount('SIGTERM') > 0) process.emit('SIGTERM');
  else process.exit(0);
}

const startupTimer = setTimeout(() => {
  if (stopping) return;
  console.error('等待 DSH 就绪超时');
  stopping = true;
  shutdown();
}, 120_000).unref();

// 启动日志保留有限的诊断信息。官方 Web 的带 token 就绪行只交给已认证的客户端。
let loggedBytes = 0;
for (const stream of [process.stdout, process.stderr]) {
  const write = stream.write.bind(stream);
  stream.write = (chunk, ...args) => {
    loggedBytes += Buffer.byteLength(chunk);
    if (loggedBytes <= 64 * 1024) return write(chunk, ...args);
    const callback = args.find(arg => typeof arg === 'function');
    if (callback) queueMicrotask(callback);
    return true;
  };
}
const log = console.log;
const readyPrefix = 'dsh web: ';
console.log = (...args) => {
  if (args.length === 1 && typeof args[0] === 'string' && args[0].startsWith(readyPrefix + 'http')) {
    const candidate = args[0].slice(readyPrefix.length).split(/\s+/, 1)[0];
    const parsed = URL.parse(candidate);
    if (parsed?.protocol === 'http:' && parsed.hostname === '127.0.0.1' && parsed.port &&
        !parsed.username && !parsed.password && parsed.searchParams.get('token')) {
      url ??= candidate;
      clearTimeout(startupTimer);
    }
    return;
  }
  log(...args);
};

function reply(response, code, body = '') {
  response.writeHead(code, { 'Content-Type': 'application/json', Connection: 'close' });
  response.end(body);
}

const server = createServer({ requestTimeout: 5000, headersTimeout: 5000 }, async (request, response) => {
  if (request.method !== 'POST' || request.url !== '/rpc') return reply(response, 404);
  if (request.headers.authorization !== `Bearer ${token}` ||
      request.headers['x-wingmate-instance'] !== identity.instance) return reply(response, 401);
  const length = Number(request.headers['content-length']);
  if (!Number.isInteger(length) || length < 1 || length > 16) return reply(response, 400);

  let action = '';
  try {
    for await (const chunk of request) action += chunk;
  } catch {
    return response.destroy();
  }
  if (action !== 'status' && action !== 'stop') return reply(response, 400);

  if (action === 'stop' && !stopping) {
    stopping = true;
    clearTimeout(startupTimer);
    response.once('finish', shutdown);
  }
  reply(response, 200, JSON.stringify({
    ...identity,
    runtime_revision,
    dsh_home,
    agents_home,
    engine_pid: process.pid,
    phase: stopping ? 'stopping' : url ? 'running' : 'starting',
    url: stopping ? null : url,
    error: null,
  }));
});

server.listen(0, '127.0.0.1');
await once(server, 'listening');
const temporary = endpointFile + '.tmp';
writeFileSync(temporary, JSON.stringify({ ...identity, port: server.address().port, token }), { mode: 0o600 });
renameSync(temporary, endpointFile);
process.once('exit', () => {
  try { rmSync(endpointFile, { force: true }); } catch {}
});
