import { link, mkdir, readdir, readlink, symlink } from 'node:fs/promises';
import { join } from 'node:path';

// All generated directories share a filesystem. Never silently copy a whole dependency tree.
// Patches must replace files atomically so they cannot modify the shared original inode.
export async function linkDependencies(source, destination) {
  await mkdir(destination, { recursive: true });
  for (const entry of await readdir(source, { withFileTypes: true })) {
    const from = join(source, entry.name), to = join(destination, entry.name);
    if (entry.isDirectory()) await linkDependencies(from, to);
    else if (entry.isSymbolicLink()) await symlink(await readlink(from), to);
    else if (entry.isFile()) await link(from, to);
  }
}
