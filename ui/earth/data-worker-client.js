// A small promise bridge for the WASM renderer. The worker owns network I/O.
export function startDataWorker() {
  if (window.canglingDataFetch) return;
  let worker;
  try { worker = new Worker(new URL('./data-worker.js', import.meta.url), { type: 'module' }); }
  catch { return; } // Embedded hosts can use the browser's direct fetch path.
  const pending = new Map();
  let nextId = 1;
  worker.onmessage = ({ data }) => {
    const request = pending.get(data.id);
    if (!request) return;
    pending.delete(data.id);
    if (data.error) request.reject(new Error(data.error));
    else request.resolve({ bytes: new Uint8Array(data.bytes), hit: data.hit });
  };
  worker.onerror = (event) => {
    for (const request of pending.values()) request.reject(new Error(event.message || 'map data worker failed'));
    pending.clear();
    delete window.canglingDataFetch;
    worker.terminate();
  };
  window.canglingDataFetch = (url, cacheName) => new Promise((resolve, reject) => {
    const id = nextId++;
    pending.set(id, { resolve, reject });
    try { worker.postMessage({ id, url, cacheName }); }
    catch (error) { pending.delete(id); reject(error); }
  });
}
