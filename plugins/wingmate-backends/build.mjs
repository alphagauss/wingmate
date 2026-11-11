import { build } from 'esbuild';
import { copyFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';

const root = new URL('./', import.meta.url);
await build({
  absWorkingDir: fileURLToPath(root),
  entryPoints: ['host.mjs'],
  outfile: 'dist/host.mjs',
  bundle: true,
  platform: 'node',
  format: 'esm',
  target: 'node24',
  legalComments: 'linked',
});
await copyFile(new URL('node_modules/@agentclientprotocol/sdk/LICENSE', root), new URL('dist/LICENSE.acp-sdk', root));
await copyFile(new URL('transport.js', root), new URL('dist/transport.js', root));
