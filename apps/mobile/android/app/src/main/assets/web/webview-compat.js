// Runs before DSH's browser boot scripts to fill APIs missing in WebView 104.
(() => {
  if (window.top !== window) return;

  function install(object, name, value) {
    if (typeof object[name] !== 'function') {
      Object.defineProperty(object, name, { configurable: true, writable: true, value });
    }
  }

  install(Array.prototype, 'toSorted', function toSorted(compare) {
    return Array.from(this).sort(compare);
  });
  install(Array.prototype, 'toReversed', function toReversed() {
    return Array.from(this).reverse();
  });
  install(URL, 'canParse', function canParse(value, base) {
    try {
      new URL(value, base);
      return true;
    } catch {
      return false;
    }
  });

  install(Promise, 'withResolvers', function withResolvers() {
    let resolve, reject;
    const promise = new this((resolvePromise, rejectPromise) => {
      resolve = resolvePromise;
      reject = rejectPromise;
    });
    return { promise, resolve, reject };
  });

  if (typeof AbortSignal.any !== 'function') {
    const controllers = new WeakMap();
    const listeners = new FinalizationRegistry((cleanup) => cleanup.abort());

    function follow(source, target, cleanup) {
      source.addEventListener('abort', () => {
        if (source.aborted) target.deref()?.abort(source.reason);
      }, { signal: cleanup.signal });
    }

    install(AbortSignal, 'any', function any(signals) {
      const sources = Array.from(signals);
      const controller = new AbortController();
      const { signal } = controller;
      const aborted = sources.find((source) => source.aborted);
      if (aborted) {
        controller.abort(aborted.reason);
        return signal;
      }
      // DSH creates composite signals per request. Release listeners even when no source aborts.
      const cleanup = new AbortController();
      const target = new WeakRef(controller);
      controllers.set(signal, controller);
      listeners.register(signal, cleanup, cleanup);
      signal.addEventListener('abort', () => {
        cleanup.abort();
        listeners.unregister(cleanup);
      }, { once: true });
      for (const source of sources) follow(source, target, cleanup);
      return signal;
    });
  }
})();
