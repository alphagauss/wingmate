import type { IncomingMessage, ServerResponse } from 'node:http';
import { countText } from '../core/statistics.ts';

export function analyze(input: unknown) {
  if (
    !input ||
    typeof input !== 'object' ||
    !('text' in input) ||
    typeof input.text !== 'string'
  ) {
    throw new Error('请输入文本');
  }
  if (input.text.length > 100_000)
    throw new Error('文本不能超过 100000 个字符');
  return countText(input.text);
}

export async function handleStatistics(
  req: IncomingMessage,
  res: ServerResponse,
) {
  res.setHeader('Content-Type', 'application/json; charset=utf-8');
  res.setHeader('Cache-Control', 'no-store');
  if (req.method !== 'POST') {
    res
      .writeHead(405, { Allow: 'POST' })
      .end(JSON.stringify({ error: '请使用 POST' }));
    return;
  }
  try {
    const chunks: Buffer[] = [];
    let size = 0;
    for await (const chunk of req) {
      size += chunk.length;
      if (size > 500_000) {
        res.writeHead(413).end(JSON.stringify({ error: '请求过大' }));
        return;
      }
      chunks.push(Buffer.from(chunk));
    }
    const result = analyze(JSON.parse(Buffer.concat(chunks).toString('utf8')));
    res.end(JSON.stringify(result));
  } catch (error) {
    res.writeHead(400);
    res.end(
      JSON.stringify({
        error: error instanceof Error ? error.message : '请求无效',
      }),
    );
  }
}
