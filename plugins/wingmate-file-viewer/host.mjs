import { readFile, open } from 'node:fs/promises';
import { Readable } from 'node:stream';

export const inject = ['webServer'];
export async function apply(ctx) {
  const manifest = JSON.parse(await readFile(new URL('./dist/assets.json', import.meta.url), 'utf8'));
  const base = `/wingmate-file-viewer/${manifest.revision}`;
  ctx.on('webserver/index-inject', table => {
    table.push({ kind: 'global', name: '__WINGMATE_FILE_VIEWER__', value: { base } });
  });
  ctx.effect(() => ctx.webServer.register({
    kind: 'prefix', path: base,
    async resource(request) {
      const path = new URL(request.url).pathname.slice(base.length + 1);
      // Resolve only exact build-manifest entries, never request-controlled filesystem paths.
      const asset = Object.hasOwn(manifest.files, path) && manifest.files[path];
      if (!asset) return new Response(null, { status: 404 });
      const headers = { 'Content-Type': asset.type, 'Content-Length': String(asset.size),
        'Cache-Control': 'public, max-age=31536000, immutable', ETag: `"${asset.hash}"`,
        'X-Content-Type-Options': 'nosniff' };
      if (path === 'frame.html') headers['Content-Security-Policy'] = "default-src 'none'; script-src 'self' blob:; style-src 'self' 'unsafe-inline' blob:; img-src 'self' data: blob:; font-src 'self' data: blob:; connect-src 'self' data: blob:; worker-src 'self' blob: data:; media-src 'self' data: blob:; frame-src 'self' blob:; frame-ancestors 'self'";
      if (request.headers.get('if-none-match') === headers.ETag) return new Response(null, { status: 304, headers });
      if (request.method === 'HEAD') return new Response(null, { headers });
      const file = await open(new URL(`./dist/viewer/${path}`, import.meta.url));
      return new Response(Readable.toWeb(file.createReadStream()), { headers });
    },
  }));
}
