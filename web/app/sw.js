// OG Paper service worker: network first, cache as offline fallback, so new
// builds always load when online and the app still opens offline.
const CACHE = 'og-paper-v1';
self.addEventListener('install', e => self.skipWaiting());
self.addEventListener('activate', e => e.waitUntil(self.clients.claim()));
self.addEventListener('fetch', e => {
  const url = new URL(e.request.url);
  // Account data (a hosted site's /paper/api/...) is never kept here.
  if (e.request.method !== 'GET' || url.pathname.includes('/api/')) return;
  // The app's own files are checked with the server every time (not taken
  // from the browser's HTTP cache), so a new og-web.js never runs against
  // an older pkg/og_paper.js. Other sites' files (fonts, pdf.js) as usual.
  const same = url.origin === self.location.origin;
  e.respondWith(
    fetch(e.request, same ? { cache: 'no-cache' } : undefined)
      .then(r => {
        if (r.ok) { const copy = r.clone(); caches.open(CACHE).then(c => c.put(e.request, copy)); }
        return r;
      })
      .catch(() => caches.match(e.request))
  );
});
