let active = 0;
const waiting = new Set();

// Waiting previews hold the shared DSH bytes. Copy only when a parsing slot is available.
async function acquire(signal) {
  while (active >= 2) {
    await new Promise((resolve, reject) => {
      const cleanup = () => { waiting.delete(ready); signal.removeEventListener('abort', abort); };
      const ready = () => { cleanup(); resolve(); };
      const abort = () => { cleanup(); reject(signal.reason); };
      signal.addEventListener('abort', abort, { once: true });
      waiting.add(ready);
      if (signal.aborted) abort();
    });
  }
  signal.throwIfAborted();
  active++;
  return () => {
    active--;
    for (const ready of [...waiting]) ready();
  };
}

export async function mountFrame(container, base, bytes, filename, signal) {
  const release = await acquire(signal);
  try {
    signal.throwIfAborted();
    await new Promise((resolve, reject) => {
      const frame = document.createElement('iframe');
      frame.title = `File Viewer — ${filename}`;
      frame.src = `${base}/frame.html`;
      frame.style.cssText = 'width:100%;height:100%;border:0;display:block';
      frame.setAttribute('sandbox', 'allow-scripts allow-same-origin allow-downloads allow-modals');
      const dispose = () => {
        clearTimeout(timer);
        window.removeEventListener('message', message);
        signal.removeEventListener('abort', abort);
        frame.remove();
      };
      const abort = () => { dispose(); reject(signal.reason); };
      const fail = error => { dispose(); reject(error); };
      const message = event => {
        if (event.source !== frame.contentWindow || event.origin !== location.origin) return;
        if (event.data?.type === 'wingmate-preview-error') fail(new Error(event.data.message));
        if (event.data?.type === 'wingmate-preview-loaded') {
          clearTimeout(timer);
          window.removeEventListener('message', message);
          resolve();
        }
      };
      const timer = setTimeout(() => fail(new Error('文件预览超时 / Preview timed out')), 60000);
      window.addEventListener('message', message);
      signal.addEventListener('abort', abort, { once: true });
      frame.addEventListener('load', () => {
        if (signal.aborted) return;
        // Transfer our copy, never detach DSH's retained buffer.
        const buffer = bytes.slice().buffer;
        frame.contentWindow.postMessage({ type: 'wingmate-preview-open', filename, buffer,
          locale: document.documentElement.lang || 'zh-CN',
          theme: document.documentElement.classList.contains('dark') ? 'dark' : 'system',
        }, location.origin, [buffer]);
      }, { once: true });
      container.append(frame);
      if (signal.aborted) abort();
    });
  } finally {
    release();
  }
}
