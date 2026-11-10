import { mountViewer } from '@file-viewer/web';
import { formats } from './formats.js';

const loaders = {
  word: () => import('@file-viewer/renderer-word'),
  presentation: () => import('@file-viewer/renderer-presentation'),
  ebook: () => import('@file-viewer/renderer-epub'),
  email: () => import('@file-viewer/renderer-email'),
  media: () => import('@file-viewer/renderer-media'),
};

let viewer;
window.addEventListener('pagehide', () => viewer?.destroy(), { once: true });
async function openDocument(event) {
  if (event.source !== parent || event.origin !== location.origin || event.data?.type !== 'wingmate-preview-open') return;
  window.removeEventListener('message', openDocument);
  const { filename, buffer, locale, theme } = event.data;
  try {
    const extension = filename.split('.').at(-1).toLowerCase();
    const format = formats.find(format => format.extensions.includes(extension));
    if (!format) throw new Error(`Unsupported file: ${extension}`);
    const renderer = (await loaders[format.id]()).default;
    viewer = mountViewer(document.getElementById('viewer'));
    await viewer.load({
      buffer, filename,
      options: {
        rendererMode: 'replace', autoRenderers: false, renderers: [renderer],
        theme, locale, styleIsolation: 'shadow',
        toolbar: { print: false, exportHtml: false },
        docx: { worker: true, workerUrl: new URL('./docx.worker.js', location.href).href,
          workerJsZipUrl: new URL('./jszip.min.js', location.href).href, workerTimeout: 45000,
          progressive: true, renderPageBatchSize: 4, renderYieldEveryMs: 12,
          externalResourcePolicy: 'block', externalLinkPolicy: 'block' },
        presentation: { workerUrl: new URL('./pptx.worker.js', location.href).href,
          pptVirtualize: true, pptCache: false },
      },
    });
    const state = viewer.getState();
    if (state.error) throw state.error;
    parent.postMessage({ type: 'wingmate-preview-loaded' }, location.origin);
  } catch (error) {
    viewer?.destroy();
    parent.postMessage({ type: 'wingmate-preview-error', message: error.message || String(error) }, location.origin);
  }
}
window.addEventListener('message', openDocument);
