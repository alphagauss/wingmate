import { mkdir, readFile, rename, rm, writeFile } from 'node:fs/promises';
import { dirname } from 'node:path';
import { randomUUID } from 'node:crypto';

export function projectIndex(file) {
  return {
    async read() {
      try {
        return JSON.parse(await readFile(file, 'utf8'));
      } catch (error) {
        if (error.code === 'ENOENT') return [];
        throw error;
      }
    },
    async write(projects) {
      const data = JSON.stringify(projects.map(({ path, title }) => ({ path, title })));
      await mkdir(dirname(file), { recursive: true });
      const temporary = `${file}.${randomUUID()}.tmp`;
      try {
        await writeFile(temporary, data);
        await rename(temporary, file);
      } finally { await rm(temporary, { force: true }); }
    },
  };
}
