import { build } from 'esbuild';
import { mkdir, writeFile, cp, rm } from 'node:fs/promises';
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
  external: ['react', '@deepseek-ai/dsh-client-ui-primitives'],
  loader: { '.css': 'text' },
  write: false,
});
await mkdir('dist', { recursive: true });
await writeFile(
  'dist/client.js',
  `window.__ModuleLoader__.load({id:"dsh-wingmate-devtools",factory(require){var module={exports:{}};var exports=module.exports;\n${client.outputFiles[0].text}\nreturn module.exports;}});\n`,
);
if (!process.argv.includes('--client-only')) {
  const template = new URL('./dist/template/', import.meta.url);
  await rm(template, { recursive: true, force: true });
  await mkdir(template, { recursive: true });
  for (const file of [
    'src',
    'scripts',
    'tests',
    'locale',
    'package.json',
    'pnpm-lock.yaml',
    'pnpm-workspace.yaml',
    'tsconfig.json',
    'vite.config.ts',
    'cordis.patch.yml',
    'icon.svg',
    'README.md',
    'AGENTS.md',
    'LICENSE',
    '.gitignore',
  ]) {
    await cp(
      new URL('../../plugins/dsh-plugin-demo/' + file, import.meta.url),
      new URL(file === '.gitignore' ? 'gitignore' : file, template),
      { recursive: true },
    );
  }
}
