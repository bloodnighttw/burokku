(() => {
  const nativeSetTimeout = globalThis.setTimeout;
  const nativeClearTimeout = globalThis.clearTimeout;
  const nativeSetInterval = globalThis.setInterval;
  const nativeClearInterval = globalThis.clearInterval;
  const nativeSetImmediate = globalThis.setImmediate;
  const pendingTimeouts = new Set();
  const pendingIntervals = new Set();

  globalThis.setTimeout = (callback, delay = 0, ...args) => {
    let id;
    id = nativeSetTimeout(() => {
      pendingTimeouts.delete(id);
      callback(...args);
    }, delay);
    pendingTimeouts.add(id);
    return id;
  };

  globalThis.clearTimeout = (id) => {
    pendingTimeouts.delete(id);
    nativeClearTimeout(id);
  };

  globalThis.setInterval = (callback, delay = 0, ...args) => {
    const id = nativeSetInterval(() => callback(...args), delay);
    pendingIntervals.add(id);
    return id;
  };

  globalThis.clearInterval = (id) => {
    pendingIntervals.delete(id);
    nativeClearInterval(id);
  };

  globalThis.setImmediate = (callback, ...args) => {
    let id;
    id = nativeSetImmediate(() => {
      pendingTimeouts.delete(id);
      callback(...args);
    });
    pendingTimeouts.add(id);
    return id;
  };

  Object.defineProperty(globalThis, "__burokkuShutdown", {
    configurable: false,
    enumerable: false,
    writable: false,
    value() {
      for (const id of pendingTimeouts) nativeClearTimeout(id);
      for (const id of pendingIntervals) nativeClearInterval(id);
      pendingTimeouts.clear();
      pendingIntervals.clear();
    },
  });
})();
