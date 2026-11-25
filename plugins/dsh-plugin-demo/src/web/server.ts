import { createServer } from 'node:http';
import { readFile } from 'node:fs/promises';
import { dirname, extname, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
import { handleStatistics } from '../server/statistics-service.ts';
import { statisticsPath } from '../types.ts';

const webRoot = resolve(dirname(fileURLToPath(import.meta.url)), 'web');
const mime: Record<string, string> = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript',
  '.css': 'text/css',
  '.svg': 'image/svg+xml',
};
const server = createServer(async (req, res) => {
  const pathname = new URL(req.url || '/', 'http://localhost').pathname;
  if (pathname === statisticsPath) return handleStatistics(req, res);
  if (req.method !== 'GET' && req.method !== 'HEAD') {
    res.writeHead(405, { Allow: 'GET, HEAD' }).end();
    return;
  }
  try {
    const file = resolve(
      webRoot,
      '.' + decodeURIComponent(pathname === '/' ? '/index.html' : pathname),
    );
    if (!file.startsWith(webRoot + sep)) {
      res.writeHead(404).end();
      return;
    }
    const content = await readFile(file);
    res.writeHead(200, {
      'Content-Type': mime[extname(file)] || 'application/octet-stream',
      'X-Content-Type-Options': 'nosniff',
    });
    res.end(req.method === 'HEAD' ? undefined : content);
  } catch {
    res.writeHead(404).end();
  }
});
server.listen(Number(process.env.PORT || 4173), '127.0.0.1', () => {
  const address = server.address();
  if (address && typeof address === 'object')
    console.log('插件示例：http://127.0.0.1:' + address.port);
});
for (const signal of ['SIGINT', 'SIGTERM'] as const)
  process.once(signal, () => server.close());
