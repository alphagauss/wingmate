// Loaded before DSH's browser boot scripts on older Android System WebView.
(() => {
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
  install(Array.prototype, 'findLastIndex', function findLastIndex(predicate, thisArg) {
    for (let index = this.length - 1; index >= 0; index--) {
      if (predicate.call(thisArg, this[index], index, this)) return index;
    }
    return -1;
  });
  install(Array.prototype, 'findLast', function findLast(predicate, thisArg) {
    for (let index = this.length - 1; index >= 0; index--) {
      const item = this[index];
      if (predicate.call(thisArg, item, index, this)) return item;
    }
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

  install(globalThis, 'structuredClone', function structuredClone(value, options) {
    // PerformanceMark.detail uses the browser's structured serialization algorithm.
    const copy = value === undefined ? undefined : new PerformanceMark('structuredClone', { detail: value }).detail;
    if (options?.transfer !== undefined) {
      const { port1, port2 } = new MessageChannel();
      try {
        port1.postMessage(null, options.transfer);
      } finally {
        port1.close();
        port2.close();
      }
    }
    return copy;
  });

  if (!('reason' in AbortSignal.prototype)) {
    const reasons = new WeakMap();
    const abort = AbortController.prototype.abort;
    const defaultReason = () => new DOMException('The operation was aborted.', 'AbortError');
    Object.defineProperty(AbortSignal.prototype, 'reason', {
      configurable: true,
      get() {
        if (!this.aborted) return undefined;
        if (!reasons.has(this)) reasons.set(this, defaultReason());
        return reasons.get(this);
      },
    });
    AbortController.prototype.abort = function abortWithReason(reason = defaultReason()) {
      if (!this.signal.aborted) reasons.set(this.signal, reason);
      abort.call(this);
    };
  }

  install(AbortSignal.prototype, 'throwIfAborted', function throwIfAborted() {
    if (this.aborted) throw this.reason;
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
