import { build } from 'vite';
import { createHash } from 'node:crypto';
import { readFile, readdir, writeFile, copyFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';
import { join, dirname } from 'node:path';

const root = fileURLToPath(new URL('./', import.meta.url));
const clientOnly = process.argv.includes('--client-only');
await build({ configFile: false, root, logLevel: 'warn', build: {
  outDir: 'dist', emptyOutDir: !clientOnly, minify: false,
  lib: { entry: join(root, 'src/client.js'), formats: ['cjs'], fileName: () => 'client.js' },
  rollupOptions: { output: {
    banner: 'window.__ModuleLoader__.load({ id: "dsh-wingmate-file-viewer", factory(require) { const exports = {};',
    footer: 'return exports; } });',
  } },
} });
if (clientOnly) process.exit(0);
const packages = new Set();
await build({ configFile: false, root, base: './', logLevel: 'warn', plugins: [{
  name: 'bundled-license-notices',
  moduleParsed({ id }) {
    if (id.startsWith('\0')) return;
    const match = id.replaceAll('\\', '/').match(/^(.*\/node_modules\/(?:@[^/]+\/)?[^/]+)\//);
    if (match) packages.add(match[1]);
  },
}], worker: { format: 'es' }, build: {
  outDir: 'dist/viewer', emptyOutDir: true, target: 'es2022',
  lib: { entry: join(root, 'src/viewer.js'), formats: ['es'], fileName: () => 'viewer.js' },
} });
const wordRequire = createRequire(import.meta.resolve('@file-viewer/renderer-word'));
const docxDist = dirname(wordRequire.resolve('@file-viewer/docx'));
const presentationRequire = createRequire(import.meta.resolve('@file-viewer/renderer-presentation'));
const pptxRequire = createRequire(presentationRequire.resolve('@file-viewer/renderer-pptx'));
const pptxDist = dirname(pptxRequire.resolve('@file-viewer/pptx'));
for (const [source, destination] of [
  [join(docxDist, 'docx-preview.worker.js'), 'docx.worker.js'],
  [join(docxDist, 'jszip.min.js'), 'jszip.min.js'],
  [join(pptxDist, 'worker/pptx.worker.js'), 'pptx.worker.js'],
]) await copyFile(source, join(root, 'dist/viewer', destination));
await writeFile(join(root, 'dist/viewer/frame.html'), `<!doctype html>
<html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<style>html,body,#viewer{height:100%;margin:0;overflow:hidden}</style></head>
<body><div id="viewer"></div><script type="module" src="./viewer.js"></script></body></html>`);
const notices = [];
for (const directory of [...packages].sort()) {
  const pkg = JSON.parse(await readFile(join(directory, 'package.json'), 'utf8'));
  notices.push(`${pkg.name} ${pkg.version} — ${pkg.license || 'See notices'}`);
  const files = (await readdir(directory, { recursive: true, withFileTypes: true }))
    .filter(entry => entry.isFile() && /(^|[-_.])(licen[cs]es?|copying|notices?)([-_.]|$)/i.test(entry.name)
      && !/\.(?:[cm]?js|ts|map)$/i.test(entry.name))
    .map(entry => join(entry.parentPath, entry.name)).sort();
  for (const file of files) {
    notices.push(`${file.slice(directory.length + 1).replaceAll('\\', '/')}\n${await readFile(file, 'utf8')}`);
  }
}
await writeFile(join(root, 'dist/viewer/THIRD-PARTY-NOTICES.txt'), notices.join('\n\n'));
const types = { '.html': 'text/html; charset=utf-8', '.js': 'text/javascript; charset=utf-8', '.mjs': 'text/javascript; charset=utf-8',
  '.css': 'text/css', '.json': 'application/json', '.wasm': 'application/wasm',
  '.woff2': 'font/woff2', '.woff': 'font/woff', '.ttf': 'font/ttf', '.txt': 'text/plain; charset=utf-8' };
const files = {};
for (const path of (await readdir(join(root, 'dist/viewer'), { recursive: true, withFileTypes: true }))
  .filter(entry => entry.isFile()).map(entry => join(entry.parentPath, entry.name)).sort()) {
  const bytes = await readFile(path);
  const name = path.slice(join(root, 'dist/viewer').length + 1).replaceAll('\\', '/');
  files[name] = { size: bytes.length, hash: createHash('sha256').update(bytes).digest('hex'),
    type: types[name.slice(name.lastIndexOf('.'))] || 'application/octet-stream' };
}
const revision = createHash('sha256').update(JSON.stringify(files))
  .update(await readFile(join(root, 'host.mjs'))).digest('hex').slice(0, 16);
await writeFile(join(root, 'dist/assets.json'), JSON.stringify({ revision, files }, null, 2) + '\n');
console.log(`File Viewer built — ${Object.keys(files).length} assets, ${revision}`);
