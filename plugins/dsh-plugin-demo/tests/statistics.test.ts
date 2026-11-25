import { test } from 'node:test';
import assert from 'node:assert/strict';
import { countText } from '../src/core/statistics.ts';

test('空文本与空白分别计数', () => {
  assert.deepEqual(countText(''), {
    characters: 0,
    nonWhitespace: 0,
    lines: 0,
    words: 0,
  });
  assert.deepEqual(countText(' \n\t'), {
    characters: 3,
    nonWhitespace: 0,
    lines: 2,
    words: 0,
  });
});
test('Unicode 码点、混合换行和以空白分隔的词组', () => {
  assert.deepEqual(countText('你好 😀\r\nhello world'), {
    characters: 17,
    nonWhitespace: 13,
    lines: 2,
    words: 4,
  });
  assert.equal(countText('a\rb\nc\r\nd').lines, 4);
});
