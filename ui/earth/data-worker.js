// Fetch and read cached map resources away from the rendering thread.
async function fetchBytes(url, cacheName) {
  // Agri URLs are mutable: CacheStorage has no automatic HTTP freshness checks.
  // Revalidate through the browser HTTP cache instead of serving saved bytes forever.
  const mutableImagery = cacheName?.startsWith('cangling-imagery-agri-');
  const request = new Request(url, mutableImagery ? {cache: 'no-cache'} : {});
  let cache;
  if (!mutableImagery && cacheName && self.caches) {
    try { cache = await caches.open(cacheName); } catch { /* CacheStorage is optional. */ }
  }
  if (cache) {
    try {
      const cached = await cache.match(request);
      if (cached) return { bytes: await cached.arrayBuffer(), hit: true };
    } catch { /* A cache read failure falls through to the network. */ }
  }
  const response = await fetch(request);
  if (!response.ok) throw new Error(`tile HTTP status ${response.status} for ${url}`);
  if (cache) {
    try { await cache.put(request, response.clone()); } catch { /* Quota must not block rendering. */ }
  }
  return { bytes: await response.arrayBuffer(), hit: false };
}

self.onmessage = async ({ data }) => {
  const { id, url, cacheName } = data;
  try {
    const { bytes, hit } = await fetchBytes(url, cacheName);
    self.postMessage({ id, bytes, hit }, [bytes]);
  } catch (error) {
    self.postMessage({ id, error: String(error) });
  }
};
