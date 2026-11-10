import { createHash } from 'node:crypto';
import { readFile, writeFile } from 'node:fs/promises';
import { join } from 'node:path';

// Apply ordered, version-bound edits to a fresh npm installation. A changed upstream fails closed.
export async function applyRuntimePatches(stage, patches) {
  for (const patch of patches) {
    const directory = join(stage, 'node_modules', patch.package);
    const manifest = JSON.parse(await readFile(join(directory, 'package.json'), 'utf8'));
    const file = join(directory, patch.file);
    const bytes = await readFile(file);
    if (manifest.version !== patch.version || createHash('sha256').update(bytes).digest('hex') !== patch.sha256) {
      throw new Error(`上游文件与补丁不匹配: ${patch.id} (${patch.package})`);
    }
    const source = bytes.toString('utf8');
    if (!patch.before || source.split(patch.before).length !== 2) {
      throw new Error(`补丁必须恰好匹配一处: ${patch.id}`);
    }
    await writeFile(file, source.replace(patch.before, () => patch.after));
  }
}
