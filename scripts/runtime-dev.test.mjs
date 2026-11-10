import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdir, mkdtempDisposable, readFile, stat, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import test from 'node:test';
import { linkDependencies } from './runtime-dev.mjs';
import { applyRuntimePatches } from './runtime-patches.mjs';

test('开发视图共享依赖，重做或撤销补丁不会改动原包与旧视图', async () => {
  await using scratch = await mkdtempDisposable(join(tmpdir(), 'wingmate-dev-test-'));
  const base = join(scratch.path, 'base'), old = join(scratch.path, 'old'), next = join(scratch.path, 'next');
  const packageDir = 'node_modules/example';
  await mkdir(join(base, packageDir), { recursive: true });
  await writeFile(join(base, packageDir, 'package.json'), '{"version":"1"}');
  await writeFile(join(base, packageDir, 'index.js'), 'original');
  const patch = { id: 'fixture', package: 'example', version: '1', file: 'index.js',
    sha256: createHash('sha256').update('original').digest('hex'), before: 'original', after: 'first' };
  await linkDependencies(base, old);
  assert.equal((await stat(join(base, packageDir, 'index.js'))).nlink, 2);
  const originalMode = (await stat(join(base, packageDir, 'index.js'))).mode;
  const previousUmask = process.umask(0o077);
  try { await applyRuntimePatches(old, [patch]); }
  finally { process.umask(previousUmask); }
  assert.equal((await stat(join(old, packageDir, 'index.js'))).mode, originalMode);
  await linkDependencies(base, next);
  await applyRuntimePatches(next, [{ ...patch, after: 'second' }]);
  assert.equal(await readFile(join(base, packageDir, 'index.js'), 'utf8'), 'original');
  assert.equal(await readFile(join(old, packageDir, 'index.js'), 'utf8'), 'first');
  assert.equal(await readFile(join(next, packageDir, 'index.js'), 'utf8'), 'second');
  const unpatched = join(scratch.path, 'unpatched');
  await linkDependencies(base, unpatched);
  await applyRuntimePatches(unpatched, []);
  assert.equal(await readFile(join(unpatched, packageDir, 'index.js'), 'utf8'), 'original');
  assert.equal((await stat(join(base, packageDir, 'package.json'))).nlink, 4);
});
