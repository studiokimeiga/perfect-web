// The head-to-head's browser instrument (ADR-XXXX, "the head-to-head"): one
// page load of one app on one stack, in one engine, cold or warm, with or
// without network shaping, and the interactions after it.
//
// What it records, each from the page or the browser, never from a stack's
// own report:
//
// - every response the load and each interaction made the browser fetch:
//   its decoded bytes, what Brotli (q11) and gzip (9) make of them, as a
//   static host sends them (`lib.mjs`'s `sizes`), the bytes the engine says
//   it received (Playwright's `request.sizes()`; on Chromium, also the
//   DevTools protocol's `Network.loadingFinished`), its encoding and
//   whether a cache answered it;
// - when the app is first in the document and first painted (the Paint
//   Timing API's first contentful paint, which all three engines report, and
//   the frame after the app's field is in the document), the navigation's
//   own marks, and when the app is **interactive**: the moment its handlers
//   are attached. A client-rendered app attaches them as it renders (React
//   at `createRoot`, Svelte in `mount`), so it is interactive when its field
//   is in the document; a page its server rendered is interactive when its
//   runtime says so: Pleris's `data-pw-ready`, Next.js's App Router state
//   (`history.state.__NA`), SvelteKit's router state
//   (`history.state["sveltekit:history"]`). Each run then checks the claim:
//   its first interaction, made at once, must take effect without loading a
//   new document;
// - each interaction's latency, from the input event's own time stamp to the
//   change in the document and to the frame after it, and the bytes it made
//   the browser fetch.
//
// No page is changed: the observers are an init script, which no policy of
// the page's applies to and which runs no code the page did not ask for
// beyond observing. A page's Content-Security-Policy is never bypassed.

import { gitBlob, minifiedVerdict, sizes } from "./lib.mjs";

/** The observers, as a page's first script. `__APP__` is the selector of the
 * app's first control; `__READY__` the readiness rule. */
const OBSERVERS = String.raw`(() => {
  if (window.top !== window || window.__h2h) return;
  const h = (window.__h2h = { t: {}, lcp: null, cls: 0, waits: [], documents: 1 });
  const now = () => performance.now();
  const frame = (set) => requestAnimationFrame(() => setTimeout(() => set(now()), 0));
  try {
    new PerformanceObserver((l) => {
      for (const e of l.getEntries()) if (e.name === "first-contentful-paint") h.t.fcp = e.startTime;
    }).observe({ type: "paint", buffered: true });
  } catch {}
  try {
    new PerformanceObserver((l) => {
      const es = l.getEntries();
      if (es.length) h.lcp = es[es.length - 1].startTime;
    }).observe({ type: "largest-contentful-paint", buffered: true });
  } catch {}
  try {
    new PerformanceObserver((l) => {
      for (const e of l.getEntries()) if (!e.hadRecentInput) h.cls += e.value;
    }).observe({ type: "layout-shift", buffered: true });
  } catch {}
  const APP = "__APP__";
  const READY = "__READY__";
  const readyNow = () => {
    if (READY === "dom") return !!document.querySelector(APP);
    if (READY === "pw") return document.documentElement.dataset.pwReady === "1";
    if (READY === "next") return !!(history.state && history.state.__NA);
    if (READY === "sveltekit") return !!(history.state && "sveltekit:history" in history.state);
    return false;
  };
  // What an interaction waits for, by name: no code is made from text, so a
  // page's policy against eval is no obstacle and is never bypassed.
  const CONDS = {
    todos: (n) => document.querySelectorAll(".todo-list li").length === n,
    completed: (n) => document.querySelectorAll(".todo-list li.completed").length === n,
    filter: (name) => {
      const a = document.querySelector(".filters a.selected");
      return !!a && a.textContent.trim() === name;
    },
    count: (text) => {
      const c = document.querySelector("#cart-count");
      return !!c && c.textContent.trim() === text;
    },
  };
  const check = () => {
    if (h.t.appInDom == null && document.querySelector(APP)) {
      h.t.appInDom = now();
      frame((t) => (h.t.appFrame = t));
    }
    if (h.t.ready == null && readyNow()) h.t.ready = now();
    for (const w of h.waits) {
      if (w.dom == null && w.start != null && CONDS[w.cond](w.arg)) {
        w.dom = now();
        frame((t) => (w.frame = t));
      }
    }
  };
  new MutationObserver(check).observe(document, {
    subtree: true, childList: true, attributes: true, characterData: true,
  });
  for (const name of ["pushState", "replaceState"]) {
    const original = history[name];
    history[name] = function (...args) {
      const r = original.apply(this, args);
      check();
      return r;
    };
  }
  document.addEventListener("DOMContentLoaded", check);
  addEventListener("load", check);
  // The start of an interaction is its first input event's own time stamp.
  for (const type of ["keydown", "pointerdown", "mousedown", "click"]) {
    addEventListener(type, (e) => {
      const w = h.waits[h.waits.length - 1];
      if (w && w.start == null && w.armed) {
        w.start = e.timeStamp;
        w.event = e.type;
        queueMicrotask(check);
      }
    }, true);
  }
  h.arm = (name, cond, arg) => {
    h.waits.push({ name, cond, arg, armed: true, start: null, dom: null, frame: null });
  };
})();`;

/** The init script for an app: its first control, and how it says it is interactive. */
export function observers(appSelector, ready) {
  return OBSERVERS.replace("__APP__", appSelector).replace("__READY__", ready);
}

/** Chromium's network shaping, from Lighthouse's "Slow 4G" (its mobile
 * default): 150 ms round trip, 1.6 Mbit/s down, 750 kbit/s up, and the CPU
 * slowed four times. Firefox and WebKit offer Playwright no shaping. */
export const SLOW_4G = {
  name: "slow-4g",
  latency: 150,
  download: (1.6 * 1024 * 1024) / 8,
  upload: (750 * 1024) / 8,
  cpu: 4,
};

/**
 * Every request a page's context makes, by phase (the load, each
 * interaction): for each response, its bytes decoded, what Brotli (q11) and
 * gzip (9) make of them, whether it is minified, its blob, and the bytes the
 * engine says it received (Playwright's `request.sizes()`). On Chromium also
 * the DevTools protocol's own count for every request, streams and aborted
 * ones included (`Network.dataReceived` and `Network.loadingFinished`), which
 * is what "received" means where it is available.
 *
 * A request held open longer than a second (a long poll or a stream, as
 * Pleris's page keeps one for its updates) does not keep the network from
 * being quiet: it is background, counted when it ends.
 */
export function networkLog(context, cdp) {
  const entries = [];
  const pending = new Map();
  let lastActivity = Date.now();
  let phase = "load";
  const chromium = new Map();
  if (cdp) {
    // Each byte is counted in the phase it arrived in: a stream opened while
    // the page loaded and answering an interaction counts in the interaction.
    // A response a cache answered received nothing from the network.
    const count = (c, n) => {
      if (c.fromCache) return;
      c.received += n;
      c.byPhase[phase] = (c.byPhase[phase] ?? 0) + n;
    };
    cdp.on("Network.requestWillBeSent", (e) => {
      if (!chromium.has(e.requestId)) {
        chromium.set(e.requestId, { url: e.request.url, phase, received: 0, byPhase: {}, total: null, failed: null, fromCache: false });
      }
    });
    cdp.on("Network.responseReceived", (e) => {
      const c = chromium.get(e.requestId);
      if (!c) return;
      Object.assign(c, {
        status: e.response.status,
        fromCache: c.fromCache || !!e.response.fromDiskCache || !!e.response.fromPrefetchCache || !!e.response.fromServiceWorker,
      });
      count(c, e.response.encodedDataLength ?? 0);
    });
    cdp.on("Network.requestServedFromCache", (e) => {
      const c = chromium.get(e.requestId);
      if (c) c.fromCache = true;
    });
    cdp.on("Network.dataReceived", (e) => {
      const c = chromium.get(e.requestId);
      if (c) count(c, e.encodedDataLength);
    });
    cdp.on("Network.loadingFinished", (e) => {
      const c = chromium.get(e.requestId);
      if (!c) return;
      c.total = e.encodedDataLength;
      // What arrived uncounted by its chunks (Chromium may report a body's
      // bytes only at its end) counts where the request finished.
      if (c.total > c.received) count(c, c.total - c.received);
    });
    cdp.on("Network.loadingFailed", (e) => {
      const c = chromium.get(e.requestId);
      if (c) c.failed = e.errorText || (e.canceled ? "canceled" : "failed");
    });
  }
  const onRequest = (request) => {
    pending.set(request, Date.now());
    lastActivity = Date.now();
  };
  const settle = async (request, failed) => {
    const opened = pending.get(request) ?? Date.now();
    pending.delete(request);
    const longLived = Date.now() - opened > 1000;
    if (!longLived) lastActivity = Date.now();
    const entry = {
      phase,
      url: request.url(),
      method: request.method(),
      kind: request.resourceType(),
      failed: failed ? request.failure()?.errorText ?? "failed" : null,
      heldMs: Date.now() - opened,
    };
    entries.push(entry);
    if (failed) return;
    try {
      const response = await request.response();
      const headers = response ? await response.allHeaders() : {};
      entry.status = response?.status() ?? null;
      entry.type = (headers["content-type"] ?? "").split(";")[0];
      entry.encoding = headers["content-encoding"] ?? null;
      entry.cacheControl = headers["cache-control"] ?? null;
      entry.validator = headers.etag ? "etag" : headers["last-modified"] ? "last-modified" : null;
      let body = Buffer.alloc(0);
      if (entry.status && entry.status !== 304 && !(entry.status >= 300 && entry.status < 400)) {
        try {
          body = await response.body();
        } catch {
          /* no body */
        }
      }
      Object.assign(entry, sizes(body));
      entry.blob = body.length ? gitBlob(body) : null;
      if (/javascript|css/.test(entry.type)) {
        const v = minifiedVerdict(body.toString("utf8"), entry.type);
        entry.minified = v.minified;
        entry.minifiedShare = v.share;
        entry.minifiedBytes = v.minifiedBytes;
      }
      if (entry.kind === "document") entry.text = body.toString("utf8");
      const reported = await request.sizes().catch(() => null);
      // Playwright reports a cached response's sizes as negative: nothing
      // came from the network.
      entry.receivedBody = reported ? Math.max(0, reported.responseBodySize) : null;
      entry.receivedHeaders = reported ? Math.max(0, reported.responseHeadersSize) : null;
    } catch (e) {
      entry.error = String(e).split("\n")[0];
    }
  };
  const settling = [];
  context.on("request", onRequest);
  context.on("requestfinished", (r) => settling.push(settle(r, false)));
  context.on("requestfailed", (r) => settling.push(settle(r, true)));
  return {
    entries,
    /** Every response read: its body and sizes are in its entry. */
    settled: () => Promise.allSettled(settling),
    setPhase(p) {
      phase = p;
    },
    /** The network is quiet: nothing but long-held requests in flight, and
     * nothing else started or ended for `quietMs`, within `capMs`. */
    async quiet(quietMs = 400, capMs = 15_000) {
      const until = Date.now() + capMs;
      while (Date.now() < until) {
        const now = Date.now();
        const busy = [...pending.values()].some((opened) => now - opened <= 1000);
        if (!busy && now - lastActivity >= quietMs) return true;
        await new Promise((r) => setTimeout(r, 25));
      }
      return false;
    },
    /** Chromium's own count of every request's bytes, with the phase it began in. */
    chromiumReceived() {
      return [...chromium.values()].map((c) => ({ ...c, bytes: c.total ?? c.received }));
    },
  };
}

async function shape(page, cdp, network) {
  if (!network) return;
  await cdp.send("Network.enable");
  await cdp.send("Network.emulateNetworkConditions", {
    offline: false,
    latency: network.latency,
    downloadThroughput: network.download,
    uploadThroughput: network.upload,
  });
  await cdp.send("Emulation.setCPUThrottlingRate", { rate: network.cpu });
}

/**
 * One measured load of `target` in `browser`, and its interactions.
 * `condition` is "cold" (a new context: an empty cache, no storage) or
 * "warm" (the same context after one whole visit with the same interactions:
 * its HTTP cache primed, its page storage cleared, its cookies kept).
 */
export async function measureOnce({ browser, engine, target, condition, network = null }) {
  const context = await browser.newContext({ viewport: { width: 1280, height: 800 } });
  try {
    await context.addInitScript(observers(target.appSelector, target.ready));
    const page = await context.newPage();
    const cdp = engine === "chromium" ? await context.newCDPSession(page) : null;
    if (cdp) await cdp.send("Network.enable");
    if (condition === "warm") {
      await visit(page, target, null);
      await page.evaluate(() => {
        try {
          localStorage.clear();
          sessionStorage.clear();
        } catch {
          /* none */
        }
      });
      await page.goto("about:blank");
    } else if (condition !== "cold") {
      throw new Error(`unknown condition ${condition}`);
    }
    const log = networkLog(context, cdp);
    if (network) await shape(page, cdp, network);
    const result = await visit(page, target, log);
    await log.settled();
    result.received = cdp ? log.chromiumReceived() : null;
    result.requests = log.entries;
    return result;
  } finally {
    await context.close();
  }
}

/** A visit: the load, readiness, then each interaction of the target. */
async function visit(page, target, log) {
  const startedAt = Date.now();
  log?.setPhase("load");
  await page.goto(target.url, { waitUntil: "commit", timeout: 60_000 });
  await page.waitForFunction(() => window.__h2h && window.__h2h.t.ready != null, null, {
    timeout: 60_000,
    polling: 50,
  });
  await page.waitForLoadState("load", { timeout: 60_000 });
  if (log) await log.quiet();
  const timings = await page.evaluate(() => {
    const nav = performance.getEntriesByType("navigation")[0];
    const h = window.__h2h;
    return {
      ttfb: nav ? nav.responseStart : null,
      // Chromium's shaping holds a response's body, not its first byte: the
      // document's last byte is where its round trip shows.
      documentEnd: nav ? nav.responseEnd : null,
      domContentLoaded: nav ? nav.domContentLoadedEventEnd : null,
      load: nav ? nav.loadEventEnd : null,
      fcp: h.t.fcp ?? null,
      lcp: h.lcp,
      cls: h.cls,
      appInDom: h.t.appInDom ?? null,
      appFrame: h.t.appFrame ?? null,
      ready: h.t.ready ?? null,
    };
  });
  const navigations = () => page.evaluate(() => performance.getEntriesByType("navigation").length);
  const docBefore = await page.evaluate(() => performance.timeOrigin);
  const interactions = [];
  for (const step of target.interactions) {
    log?.setPhase(`interaction:${step.name}`);
    const arg = step.before ? await step.before(page) : step.arg;
    await page.evaluate(([n, c, a]) => window.__h2h.arm(n, c, a), [step.name, step.cond, arg]);
    await step.run(page);
    await page.waitForFunction(
      () => {
        const w = window.__h2h.waits[window.__h2h.waits.length - 1];
        return w.frame != null;
      },
      null,
      { timeout: 30_000, polling: 25 },
    );
    if (log) await log.quiet();
    const w = await page.evaluate(() => window.__h2h.waits[window.__h2h.waits.length - 1]);
    interactions.push({
      name: step.name,
      event: w.event,
      toDom: w.dom - w.start,
      toFrame: w.frame - w.start,
    });
  }
  // The claim that the page was interactive: its interactions took effect
  // in the document it loaded, not in a new one.
  const docAfter = await page.evaluate(() => performance.timeOrigin);
  const sameDocument = docBefore === docAfter && (await navigations()) === 1;
  return { timings, interactions, sameDocument, wallMs: Date.now() - startedAt };
}
