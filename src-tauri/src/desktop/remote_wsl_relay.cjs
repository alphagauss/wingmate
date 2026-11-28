// 使用目标 WSL 中 Wingmate 已有的 Node.js 转发经核对的连接。
const { once } = require('node:events');
const { readFileSync, readdirSync, readlinkSync } = require('node:fs');
const { createConnection } = require('node:net');
const { setTimeout: delay } = require('node:timers/promises');

const [port, pid, started] = process.argv.slice(1);
const address = (value) =>
  '0100007F:' + Number(value).toString(16).padStart(4, '0').toUpperCase();

async function verify(socket) {
  const processPath = '/proc/' + pid;
  const local = address(socket.localPort);
  const remote = address(port);
  // 连接建立后，服务端可能尚未接受套接字。等待 inode 就绪后再发送数据。
  for (let attempt = 0; attempt < 30 && !socket.destroyed; attempt++) {
    const stat = readFileSync(processPath + '/stat', 'utf8');
    if (stat.slice(stat.lastIndexOf(')') + 2).split(' ')[19] !== started) break;
    const peer = readFileSync('/proc/net/tcp', 'utf8')
      .split('\n')
      .map((row) => row.trim().split(/\s+/))
      .find((row) => row[1] === remote && row[2] === local && row[3] === '01');
    const inode = peer?.[9];
    if (inode && inode !== '0') {
      const directory = processPath + '/fd';
      // 对照所选发行版内管理进程的描述符，防止接入共用回环上的其他进程。
      return readdirSync(directory).some((fd) => {
        try {
          return (
            readlinkSync(directory + '/' + fd) === 'socket:[' + inode + ']'
          );
        } catch (error) {
          if (error.code === 'ENOENT') return false;
          throw error;
        }
      });
    }
    await delay(25);
  }
  return false;
}

async function main() {
  if (
    !/^[1-9][0-9]*$/.test(pid) ||
    !/^[0-9]+$/.test(started) ||
    !/^[1-9][0-9]{0,4}$/.test(port) ||
    Number(port) > 65535
  ) {
    throw new Error('Invalid Wingmate gateway identity');
  }
  const socket = createConnection({ host: '127.0.0.1', port: Number(port) });
  socket.setTimeout(3000, () =>
    socket.destroy(new Error('Gateway TCP timeout')),
  );
  socket.on('error', () => socket.destroy());
  try {
    await once(socket, 'connect');
    if (!(await verify(socket)))
      throw new Error('Unexpected Wingmate gateway process');
  } catch (error) {
    socket.destroy();
    throw error;
  }
  socket.setTimeout(0);
  socket.on('close', () => process.stdin.pause());
  process.stdout.on('error', () => socket.destroy());
  process.stdout.write(Buffer.of(0));
  socket.pipe(process.stdout, { end: false });
  process.stdin.pipe(socket);
}

main().catch(() => {
  process.stdout.write(Buffer.of(1));
  process.exitCode = 1;
});
