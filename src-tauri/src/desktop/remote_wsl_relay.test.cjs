const assert = require('node:assert/strict');
const { spawn } = require('node:child_process');
const { once } = require('node:events');
const { readFileSync } = require('node:fs');
const { createServer } = require('node:net');
const { join } = require('node:path');
const { test } = require('node:test');

const relay = readFileSync(join(__dirname, 'remote_wsl_relay.cjs'), 'utf8');
function started(pid) {
  const stat = readFileSync('/proc/' + pid + '/stat', 'utf8');
  return stat.slice(stat.lastIndexOf(')') + 2).split(' ')[19];
}

async function dial(port, pid, time) {
  const child = spawn(process.execPath, [
    '--input-type=commonjs',
    '--eval',
    relay,
    '--',
    String(port),
    String(pid),
    time,
  ]);
  const chunks = [];
  child.stdout.on('data', (chunk) => chunks.push(chunk));
  child.stdin.end('pairing-data');
  const [code] = await once(child, 'close');
  return { code, response: Buffer.concat(chunks) };
}

test('verifies the owner of the accepted Linux socket before relaying', {
  skip: process.platform !== 'linux',
  timeout: 8000,
}, async () => {
  const connections = [];
  const server = createServer((socket) => {
    const payload = [];
    connections.push(payload);
    socket.on('data', (data) => {
      payload.push(data);
      socket.end('gateway-response');
    });
  });
  server.listen(0, '127.0.0.1');
  await once(server, 'listening');
  try {
    const { port } = server.address();
    const time = started(process.pid);
    const rejected = await dial(port, process.ppid, started(process.ppid));
    assert.deepEqual(rejected, { code: 1, response: Buffer.of(1) });
    assert.deepEqual(connections.at(-1), []);
    const replaced = await dial(port, process.pid, '0');
    assert.deepEqual(replaced, { code: 1, response: Buffer.of(1) });
    assert.deepEqual(connections.at(-1), []);
    const accepted = await dial(port, process.pid, time);
    assert.deepEqual(accepted, {
      code: 0,
      response: Buffer.concat([Buffer.of(0), Buffer.from('gateway-response')]),
    });
    assert.equal(Buffer.concat(connections.at(-1)).toString(), 'pairing-data');
  } finally {
    server.close();
    await once(server, 'close');
  }
});
