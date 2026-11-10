import { formats } from './formats.js';
import { mountFrame } from './frame-host.js';

const { createElement: h, useEffect, useRef, useState } = require('react');
const id = 'dsh-wingmate-file-viewer';

function Preview({ content, resourceAddress, useTabInfo }) {
  const element = useRef(null);
  const { tab } = useTabInfo();
  const [error, setError] = useState('');
  const filename = decodeURIComponent(resourceAddress.split(/[?#]/)[0].split('/').at(-1));
  const bytes = content.kind === 'bytes' ? content.data : undefined;
  useEffect(() => {
    if (!tab.visible || !bytes) return;
    const controller = new AbortController();
    setError('');
    const start = async () => {
      const { base } = window.__WINGMATE_FILE_VIEWER__;
      await mountFrame(element.current, base, bytes, filename, controller.signal);
    };
    start().catch(error => {
      if (!controller.signal.aborted) setError(error.message || String(error));
    });
    return () => controller.abort();
  }, [bytes, filename, tab.visible]);
  return h('div', { style: { height: '100%', minHeight: 0, position: 'relative' } },
    error && h('div', { role: 'alert', style: { padding: 16 } }, `文件预览失败 / Preview failed — ${error}`),
    h('div', { ref: element, 'data-wingmate-preview': filename, style: { height: '100%', minHeight: 0 } }),
  );
}

export const inject = ['documentPreviews', 'slots'];
export function apply(ctx) {
  ctx.effect(() => ctx.documentPreviews.register({
    id,
    extensions: formats.flatMap(format => format.extensions),
    binaryExtensions: formats.flatMap(format => format.binary),
    priority: 'extension', title: () => 'File Viewer', loading: 'bytes-complete',
  }));
  ctx.effect(() => ctx.slots.inject('sidebar.right.tab.document', () => ctx.slots.register({
    name: 'sidebar.right.tab.document', key: id,
  }, Preview)));
}
