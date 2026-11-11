import { build } from 'esbuild';

await build({
  entryPoints: ['client.js'],
  outfile: 'dist/client.js',
  bundle: true,
  format: 'iife',
  // The client factory supplies React. The type-checking config emits nothing.
  tsconfigRaw: {},
  jsxFactory: 'h',
});
