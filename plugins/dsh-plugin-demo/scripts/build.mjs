import { build as bundle } from 'esbuild';
import { build as buildWeb } from 'vite';
import {
  mkdir,
  mkdtemp,
  rename,
  rm,
  writeFile,
  readFile,
  cp,
} from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { resolve } from 'node:path';

const root = fileURLToPath(new URL('../', import.meta.url));
export async function buildPlugin(development = false) {
  const manifest = JSON.parse(
    await readFile(new URL('../package.json', import.meta.url), 'utf8'),
  );
  const destination = new URL(
    development ? '../.dev/dsh/dist' : '../dist',
    import.meta.url,
  );
  await mkdir(new URL('../.dev', import.meta.url), { recursive: true });
  const staging = await mkdtemp(
    fileURLToPath(new URL('../.dev/build-', import.meta.url)),
  );
  try {
    await bundle({
      absWorkingDir: root,
      entryPoints: development
        ? { host: 'src/dsh/host.ts' }
        : { host: 'src/dsh/host.ts', server: 'src/web/server.ts' },
      outdir: staging,
      bundle: true,
      platform: 'node',
      format: 'esm',
      target: 'node24',
    });
    if (!development)
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
      external: ['react', '@deepseek-ai/dsh-client-ui-primitives'],
      loader: { '.css': 'text' },
      write: false,
    });
    await writeFile(
      staging + '/client.js',
      `window.__ModuleLoader__.load({id:${JSON.stringify(manifest.name)},factory(require){var module={exports:{}};var exports=module.exports;\n${client.outputFiles[0].text}\nreturn module.exports;}});\n`,
    );
    await mkdir(new URL('./', destination), { recursive: true });
    await rm(destination, { recursive: true, force: true });
    await rename(staging, destination);
    if (development) {
      const { name, version, type, engines, exports, dsh } = manifest;
      await writeFile(
        new URL('../.dev/dsh/package.json', import.meta.url),
        JSON.stringify({ name, version, type, engines, exports, dsh }, null, 2),
      );
      for (const file of [
        'cordis.patch.yml',
        'README.md',
        'icon.svg',
        'locale',
      ])
        await cp(
          new URL('../' + file, import.meta.url),
          new URL('../.dev/dsh/' + file, import.meta.url),
          { recursive: true },
        );
    }
  } finally {
    await rm(staging, { recursive: true, force: true });
  }
}
if (
  process.argv[1] &&
  resolve(process.argv[1]) === fileURLToPath(import.meta.url)
)
  await buildPlugin(process.argv.includes('--development'));
