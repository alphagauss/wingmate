import { build as bundle } from 'esbuild';
import { build as buildWeb } from 'vite';
import { mkdir, rename, rm } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../', import.meta.url));
const staging = fileURLToPath(new URL('../.dev/build', import.meta.url));
await rm(staging, { recursive: true, force: true });
await mkdir(staging, { recursive: true });
await bundle({
  absWorkingDir: root,
  entryPoints: { host: 'src/dsh/host.ts', server: 'src/web/server.ts' },
  outdir: staging,
  bundle: true,
  platform: 'node',
  format: 'esm',
  target: 'node24',
});
await buildWeb({
  root: root + '/src/web',
  build: { outDir: staging + '/web', emptyOutDir: true },
});
await rm(new URL('../dist', import.meta.url), { recursive: true, force: true });
await rename(staging, new URL('../dist', import.meta.url));
