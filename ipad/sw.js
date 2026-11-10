// Offline: the app's own files (and the Claude SDK) from the cache, refreshed
// in the background. GitHub and Claude API calls are never cached.
const CACHE = "jl-app-v3";
const FILES = ["./", "index.html", "app.js", "style.css", "manifest.webmanifest", "icon-256.png", "vendor/jsQR.js", "https://cdn.jsdelivr.net/npm/@anthropic-ai/sdk@1.6.0/+esm"];
self.addEventListener("install", (e) => e.waitUntil(caches.open(CACHE).then((c) => c.addAll(FILES)).then(() => self.skipWaiting())));
self.addEventListener("activate", (e) => e.waitUntil(caches.keys().then((ks) => Promise.all(ks.filter((k) => k !== CACHE).map((k) => caches.delete(k)))).then(() => self.clients.claim())));
self.addEventListener("fetch", (e) => {
  const url = new URL(e.request.url);
  if (e.request.method !== "GET" || url.hostname === "api.github.com" || url.hostname === "api.anthropic.com") return;
  e.respondWith(
    caches.open(CACHE).then(async (c) => {
      const hit = await c.match(e.request);
      const net = fetch(e.request).then((r) => { if (r.ok) c.put(e.request, r.clone()); return r; }).catch(() => hit);
      return hit ?? net;
    }),
  );
});
