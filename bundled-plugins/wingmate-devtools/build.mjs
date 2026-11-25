import { build } from 'esbuild';
await build({
  entryPoints: ['src/projects.ts'],
  outfile: 'dist/projects.js',
  bundle: true,
  platform: 'node',
  format: 'esm',
  target: 'node24',
});
