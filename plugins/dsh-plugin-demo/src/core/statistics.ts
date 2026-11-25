import type { Statistics } from '../types.ts';

export function countText(text: string): Statistics {
  const characters = Array.from(text);
  return {
    characters: characters.length,
    nonWhitespace: characters.filter((character) => !/\s/u.test(character))
      .length,
    lines: text.length === 0 ? 0 : text.split(/\r\n|\r|\n/).length,
    words: text.match(/\S+/gu)?.length ?? 0,
  };
}
