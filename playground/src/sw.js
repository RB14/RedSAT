// The playground's service worker.
//
// 1. Cross-origin isolation: GitHub Pages cannot send the COOP/COEP headers that make
//    SharedArrayBuffer available, so this worker adds them to the playground's own responses.
//    With a SharedArrayBuffer, the page can stop a running program (or the losers of a race)
//    without terminating its Web Worker, which keeps the compiler and solvers loaded.
// 2. Offline use: every response is also kept in a cache (replaced on every deploy; VERSION is
//    stamped by scripts/build.mjs) and served from it when the network is unavailable.
//
// Everything is fetched from the network first and revalidated with the server (`no-cache`): a
// fresh page with cached scripts or styles (GitHub Pages lets browsers keep files for 10 minutes)
// would mix two versions of the playground after a deploy. Unchanged files cost a
// `304 Not Modified`.
//
// The guide under guide/ is a separate site and is left alone.

const VERSION = '__REDSAT_BUILD__';
const CACHE = `redsat-playground-${VERSION}`;
/** No caching while developing, so that rebuilt files show up at once. */
const DEVELOPING = ['localhost', '127.0.0.1'].includes(self.location.hostname);

self.addEventListener('install', () => self.skipWaiting());

self.addEventListener('activate', (event) => {
  event.waitUntil(
    (async () => {
      for (const key of await caches.keys()) {
        if (key.startsWith('redsat-playground-') && key !== CACHE) await caches.delete(key);
      }
      await self.clients.claim();
    })(),
  );
});

self.addEventListener('fetch', (event) => {
  const { request } = event;
  const url = new URL(request.url);
  const scope = new URL(self.registration.scope);
  if (request.method !== 'GET' || url.origin !== scope.origin) return;
  if (!url.pathname.startsWith(scope.pathname) || url.pathname.startsWith(`${scope.pathname}guide/`)) return;
  event.respondWith(respond(request));
});

/** Fetches `request` from the server, revalidating anything the browser has cached. */
function fresh(request) {
  // A navigation request cannot be copied with new options; its URL is enough.
  return fetch(request.mode === 'navigate' ? request.url : request, { cache: 'no-cache' });
}

async function respond(request) {
  if (DEVELOPING) return isolated(await fetch(request));
  const cache = await caches.open(CACHE);
  try {
    const response = await fresh(request);
    if (response.ok) await cache.put(request, response.clone());
    return isolated(response);
  } catch (error) {
    const cached = await cache.match(request);
    if (cached) return isolated(cached);
    throw error;
  }
}

/** The response with the headers that make the page cross-origin isolated. */
function isolated(response) {
  if (response.type === 'opaque' || response.status === 0) return response;
  const headers = new Headers(response.headers);
  headers.set('Cross-Origin-Embedder-Policy', 'require-corp');
  headers.set('Cross-Origin-Opener-Policy', 'same-origin');
  headers.set('Cross-Origin-Resource-Policy', 'same-origin');
  return new Response(response.body, { status: response.status, statusText: response.statusText, headers });
}
