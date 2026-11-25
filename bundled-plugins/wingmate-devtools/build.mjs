import { build } from 'esbuild';
import { mkdir, writeFile } from 'node:fs/promises';
if (!process.argv.includes('--client-only'))
  await build({
    entryPoints: { projects: 'src/projects.ts', host: 'src/host.ts' },
    outdir: 'dist',
    bundle: true,
    platform: 'node',
    format: 'esm',
    target: 'node24',
  });
const client = await build({
  entryPoints: ['src/client.tsx'],
  bundle: true,
  format: 'cjs',
  platform: 'browser',
  external: ['react'],
  loader: { '.css': 'text' },
  write: false,
});
await mkdir('dist', { recursive: true });
await writeFile(
  'dist/client.js',
  `window.__ModuleLoader__.load({id:"dsh-wingmate-devtools",factory(require){var module={exports:{}};var exports=module.exports;\n${client.outputFiles[0].text}\nreturn module.exports;}});\n`,
);
