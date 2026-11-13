// Offline: the app's own files (and the Claude SDK) from the cache. GitHub and Claude API calls are never cached.
const CACHE = "jl-app-3696ee5";
const FILES = ["./", "index.html", "app.js", "style.css", "manifest.webmanifest", "icon-256.png", "vendor/jsQR.js", "https://cdn.jsdelivr.net/npm/@anthropic-ai/sdk@1.6.0/+esm"];
self.addEventListener("install", (e) => e.waitUntil(caches.open(CACHE).then((c) => c.addAll(FILES)).then(() => self.skipWaiting())));
self.addEventListener("activate", (e) => e.waitUntil(caches.keys().then((ks) => Promise.all(ks.filter((k) => k.startsWith("jl-app-") && k !== CACHE).map((k) => caches.delete(k)))).then(() => self.clients.claim())));
self.addEventListener("fetch", (e) => {
  const url = new URL(e.request.url);
  // GitHub, Claude and the lesson pictures (kept by the page itself, shrunk) are never cached here.
  if (e.request.method !== "GET" || url.hostname === "api.github.com" || url.hostname === "api.anthropic.com" || url.hostname === "upload.wikimedia.org") return;
  // The newest version first (a new publish shows on the next open); the kept
  // copy when offline or when the network does not answer within 4 s.
  e.respondWith(
    caches.open(CACHE).then(async (c) => {
      const net = fetch(e.request, { cache: "no-cache" }).then((r) => { if (r.ok) c.put(e.request, r.clone()); return r; });
      const slow = new Promise((resolve) => setTimeout(resolve, 4000));
      const first = await Promise.race([net.catch(() => null), slow]);
      if (first) return first;
      const hit = await c.match(e.request);
      return hit ?? net;
    }),
  );
});
