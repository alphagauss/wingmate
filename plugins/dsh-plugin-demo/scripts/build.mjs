import { build as bundle } from 'esbuild';
import { build as buildWeb } from 'vite';
import { mkdir, rename, rm, writeFile } from 'node:fs/promises';
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
const client = await bundle({
  absWorkingDir: root,
  entryPoints: ['src/dsh/client.tsx'],
  bundle: true,
  format: 'cjs',
  platform: 'browser',
  external: ['react'],
  loader: { '.css': 'text' },
  write: false,
});
await writeFile(
  staging + '/client.js',
  `window.__ModuleLoader__.load({id:"dsh-plugin-demo",factory(require){var module={exports:{}};var exports=module.exports;\n${client.outputFiles[0].text}\nreturn module.exports;}});\n`,
);
await rm(new URL('../dist', import.meta.url), { recursive: true, force: true });
await rename(staging, new URL('../dist', import.meta.url));
