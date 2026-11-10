import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdir, mkdtempDisposable, readFile, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import test from 'node:test';
import { applyRuntimePatches } from './runtime-patches.mjs';

test('补丁校验版本和原文件，拒绝重复或失配应用', async () => {
  await using stage = await mkdtempDisposable(join(tmpdir(), 'wingmate-patch-test-'));
  const directory = join(stage.path, 'node_modules/example');
  await mkdir(directory, { recursive: true });
  await writeFile(join(directory, 'package.json'), JSON.stringify({ version: '1.0.0' }));
  await writeFile(join(directory, 'index.js'), 'keep();\nremove();\n');
  const patch = { id: 'example', package: 'example', version: '1.0.0', file: 'index.js',
    sha256: createHash('sha256').update('keep();\nremove();\n').digest('hex'), before: 'remove();\n', after: 'literal("$& $$ $` $\' $1");\n' };
  await assert.rejects(applyRuntimePatches(stage.path, [{ ...patch, version: '2.0.0' }]), /不匹配/);
  await assert.rejects(applyRuntimePatches(stage.path, [{ ...patch, before: 'absent' }]), /恰好匹配/);
  await applyRuntimePatches(stage.path, [patch]);
  assert.equal(await readFile(join(directory, 'index.js'), 'utf8'), 'keep();\n' + patch.after);
  await assert.rejects(applyRuntimePatches(stage.path, [patch]), /不匹配/);
});
