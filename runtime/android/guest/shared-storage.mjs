import { createHash } from 'node:crypto';
import { lstat, mkdir, realpath, rename } from 'node:fs/promises';
import { join, resolve } from 'node:path';

export function isSharedDshPath(path) {
  const fullPath = resolve(path);
  return fullPath === '/root/.dsh' || fullPath.startsWith('/root/.dsh/');
}

export async function sessionLockPath(directory, filename) {
  if (!isSharedDshPath(directory)) return join(directory, filename);

  const target = await privateLockTarget('session', await realpath(directory));
  return `${target}.lock`;
}

export async function privateLockTarget(category, key) {
  const locks = `/var/lib/dsh-app/${category}-locks`;
  await mkdir(locks, { recursive: true, mode: 0o700 });
  return join(locks, createHash('sha256').update(key).digest('hex'));
}

export async function publishSessionFile(stage, target, link) {
  if (!isSharedDshPath(target)) return link(stage, target);

  return publishSharedFile(stage, target);
}

// The caller holds the write lock. Android shared storage supports renaming
// within the mount but disallows hard links.
export async function publishSharedFile(stage, target) {
  const existing = await lstat(target).catch((error) => {
    if (error.code !== 'ENOENT') throw error;
  });
  if (existing) {
    throw Object.assign(new Error(`File already exists: ${target}`), {
      code: 'EEXIST',
    });
  }
  await rename(stage, target);
}
