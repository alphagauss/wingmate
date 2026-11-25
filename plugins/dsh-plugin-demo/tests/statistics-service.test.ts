import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { analyze, handleStatistics } from '../src/server/statistics-service.ts';

test('拒绝缺失、错误类型和过长文本', () => {
  for (const input of [null, {}, { text: 1 }, { text: 'x'.repeat(100_001) }])
    assert.throws(() => analyze(input));
});
test('真实 HTTP 请求返回统计与输入错误，限制请求大小和方法', async () => {
  const server = createServer(handleStatistics);
  await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve));
  try {
    const address = server.address();
    assert.ok(address && typeof address === 'object');
    const url = 'http://127.0.0.1:' + address.port;
    const response = await fetch(url, {
      method: 'POST',
      body: JSON.stringify({ text: 'ab\n中' }),
    });
    assert.equal(response.status, 200);
    assert.deepEqual(await response.json(), {
      characters: 4,
      nonWhitespace: 3,
      lines: 2,
      words: 2,
    });
    assert.equal((await fetch(url)).status, 405);
    assert.equal((await fetch(url, { method: 'POST', body: '{' })).status, 400);
    assert.equal(
      (await fetch(url, { method: 'POST', body: 'x'.repeat(500_001) })).status,
      413,
    );
  } finally {
    await new Promise<void>((resolve, reject) =>
      server.close((error) => (error ? reject(error) : resolve())),
    );
  }
});
