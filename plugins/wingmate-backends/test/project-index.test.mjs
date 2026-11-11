import assert from 'node:assert/strict';
import { test } from 'node:test';
import { mkdir, mkdtempDisposable, readFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createWorkbench } from '../projection.mjs';
import { projectIndex } from '../project-index.mjs';

test('concurrent workspace writes preserve directory aliases across host restarts', async () => {
  await using scratch = await mkdtempDisposable(join(tmpdir(), 'wingmate-backends-index-'));
  const paths = ['one', 'two'].map(name => join(scratch.path, name));
  await Promise.all(paths.map(path => mkdir(path)));
  const file = join(scratch.path, 'index.json');
  const resolve = () => { throw new Error('workspace indexing must not start an Agent'); };
  const backend = createWorkbench('fixture', 'Fixture', resolve, projectIndex(file));
  const created = await Promise.all(paths.map(path => backend.call('workspace/create', { path })));
  await backend.call('workspace/rename', { workspaceId: created[0].workspace.workspaceId, title: 'Alias' });
  const stored = JSON.parse(await readFile(file, 'utf8'));
  assert.equal(stored.length, 2);
  assert.deepEqual(stored[0], { path: paths[0], title: 'Alias' });
  await backend.dispose();
  const restored = createWorkbench('fixture', 'Fixture', resolve, projectIndex(file));
  const reopened = await restored.call('workspace/create', { path: paths[0] });
  assert.equal(reopened.created, false);
  assert.equal(reopened.workspace.title, 'Alias');
  assert.equal(reopened.workspace.path, paths[0]);
  await restored.dispose();
});
