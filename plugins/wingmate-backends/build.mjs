import { build } from 'esbuild';
import { copyFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';

const root = new URL('./', import.meta.url);
const clientOnly = process.argv.includes('--client-only');
if (!clientOnly) {
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
}
await build({
  absWorkingDir: fileURLToPath(root),
  entryPoints: ['client.tsx'],
  outfile: 'dist/client.js',
  bundle: true,
  format: 'iife',
  // The client factory supplies React. The type-checking config emits nothing.
  tsconfigRaw: {},
  jsxFactory: 'h',
});
if (!clientOnly) {
  const adapter = new URL(import.meta.resolve('@agentclientprotocol/codex-acp'));
  await copyFile(adapter, new URL('dist/codex-acp.mjs', root));
  await copyFile(new URL('../LICENSE', adapter), new URL('dist/LICENSE.codex-acp', root));
  await copyFile(new URL('node_modules/@agentclientprotocol/sdk/LICENSE', root), new URL('dist/LICENSE.acp-sdk', root));
  await copyFile(new URL('transport.js', root), new URL('dist/transport.js', root));
}
