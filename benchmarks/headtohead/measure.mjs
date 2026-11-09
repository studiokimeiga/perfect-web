// The head-to-head's browser instrument (ADR-0392, "the head-to-head"): one
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
//   runtime says so: Pleris's `data-pw-ready`; Next.js's App Router state
//   (`history.state.__NA`, written by an effect of the hydrated tree);
//   SvelteKit's router started (`_start_router` sets
//   `history.scrollRestoration` to "manual" after `_hydrate`; its router
//   state, `history.state["sveltekit:history"]`, is written BEFORE it
//   hydrates, so it is not the signal). Each run then checks the claim: its
//   first interaction, made at once, must take effect without loading a new
//   document;
// - each interaction's latency, from the input event's own time stamp to the
//   change in the document and to the frame after it, and the bytes it made
//   the browser fetch.
//
// No page is changed: the observers are an init script, which no policy of
// the page's applies to and which runs no code the page did not ask for
// beyond observing. A page's Content-Security-Policy is never bypassed.

import { gitBlob, minifiedVerdict, sizes, sleep } from "./lib.mjs";

/** The observers, as a page's first script. `__APP__` is the selector of the
 * app's first control; `__READY__` the readiness rule. */
const OBSERVERS = String.raw`(() => {
  if (window.top !== window || window.__h2h) return;
  // A measurement an engine does not report stays null, never zero: layout
  // shift starts at 0 only where the engine reports layout shifts.
  const h = (window.__h2h = { t: {}, lcp: null, cls: null, waits: [], documents: 1 });
  const now = () => performance.now();
  const frame = (set) => requestAnimationFrame(() => setTimeout(() => set(now()), 0));
  const reports = (type) => (PerformanceObserver.supportedEntryTypes ?? []).includes(type);
  try {
    new PerformanceObserver((l) => {
      for (const e of l.getEntries()) if (e.name === "first-contentful-paint") h.t.fcp = e.startTime;
    }).observe({ type: "paint", buffered: true });
  } catch {}
  try {
    if (reports("largest-contentful-paint")) {
      new PerformanceObserver((l) => {
        const es = l.getEntries();
        if (es.length) h.lcp = es[es.length - 1].startTime;
      }).observe({ type: "largest-contentful-paint", buffered: true });
    }
  } catch {}
  try {
    if (reports("layout-shift")) {
      h.cls = 0;
      new PerformanceObserver((l) => {
        for (const e of l.getEntries()) if (!e.hadRecentInput) h.cls += e.value;
      }).observe({ type: "layout-shift", buffered: true });
    }
  } catch {}
  const APP = "__APP__";
  const READY = "__READY__";
  const readyNow = () => {
    if (READY === "dom") return !!document.querySelector(APP);
    if (READY === "pw") return document.documentElement.dataset.pwReady === "1";
    if (READY === "next") return !!(history.state && history.state.__NA);
    // SvelteKit's client writes its router state BEFORE it hydrates, and
    // starts its router after: _start_router sets scrollRestoration first.
    if (READY === "sveltekit") return history.scrollRestoration === "manual";
    return false;
  };
  try {
    const d = Object.getOwnPropertyDescriptor(History.prototype, "scrollRestoration");
    Object.defineProperty(History.prototype, "scrollRestoration", {
      configurable: true,
      enumerable: d.enumerable,
      get() {
        return d.get.call(this);
      },
      set(v) {
        d.set.call(this, v);
        check();
      },
    });
  } catch {}
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
    rows: (n) => document.getElementById("tbody")?.children.length === n,
    firstRow: (id) => document.querySelector("#tbody tr .id")?.textContent === id,
    secondRow: (id) => document.getElementById("tbody")?.children[1]?.querySelector(".id")?.textContent === id,
    updated: () => !!document.querySelector("#tbody tr .select")?.textContent.endsWith("!!!"),
    selected: (at) => document.getElementById("tbody")?.children[at]?.className === "danger",
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
function observers(appSelector, ready) {
  return OBSERVERS.replace("__APP__", appSelector).replace("__READY__", ready);
}

/**
 * Chromium's network shaping: Lighthouse's "Slow 4G" (its mobile default) as
 * Lighthouse itself applies it through the DevTools protocol. Its 150 ms
 * round trip and 1.6 Mbit/s are a packet-level network's, for its simulator;
 * `Network.emulateNetworkConditions` delays each request instead, so for it
 * Lighthouse sends `requestLatencyMs` (150 x 3.75 = 562.5 ms) and the
 * throughputs x 0.9 (Lantern's `mobileSlow4G` and its DevTools adjustment
 * factors, `front_end/models/trace/lantern/simulation/Constants.ts`; their
 * use and the kbit/s-to-bytes conversion, `core/lib/emulation.js`'s
 * `enableNetworkThrottling`; both read 2026-11-02). Until then this profile
 * sent the packet-level 150 ms as a request's latency: each request cost a
 * quarter of the round trip it names, which flattered most the stack that
 * makes the most requests. The CPU is slowed four times. Firefox and WebKit
 * offer Playwright no shaping.
 */
export const SLOW_4G = {
  name: "slow-4g",
  latency: 150 * 3.75,
  download: Math.floor((1.6 * 1024 * 0.9 * 1024) / 8),
  upload: Math.floor((750 * 0.9 * 1024) / 8),
  cpu: 4,
  describe: "Lighthouse's Slow 4G as it applies it through DevTools: 562.5 ms a request, 1.6 Mbit/s x 0.9 down, 750 kbit/s x 0.9 up, CPU x4",
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
  const log = { entries: [], pending: new Map(), lastActivity: Date.now(), phase: "load", chromium: new Map() };
  if (cdp) trackChromium(log, cdp);
  const settling = [];
  context.on("request", (request) => onRequest(log, request));
  context.on("requestfinished", (r) => settling.push(settle(log, r, false)));
  context.on("requestfailed", (r) => settling.push(settle(log, r, true)));
  return {
    entries: log.entries,
    /** Every response read: its body and sizes are in its entry. */
    settled: () => Promise.allSettled(settling),
    setPhase(p) {
      log.phase = p;
    },
    /** The network is quiet: nothing but long-held requests in flight, and
     * nothing else started or ended for `quietMs`, within `capMs`. */
    quiet: (quietMs, capMs) => quiet(log, quietMs, capMs),
    /** Chromium's own count of every request's bytes, with the phase it began in. */
    chromiumReceived: () => [...log.chromium.values()].map((c) => ({ ...c, bytes: c.total ?? c.received })),
  };
}

// --- Chromium's count, from the DevTools protocol ---------------------------------

/** Each byte is counted in the phase it arrived in: a stream opened while
 * the page loaded and answering an interaction counts in the interaction.
 * A response a cache answered received nothing from the network. */
function count(log, c, n) {
  if (c.fromCache) return;
  c.received += n;
  c.byPhase[log.phase] = (c.byPhase[log.phase] ?? 0) + n;
}

function onRequestWillBeSent(log, e) {
  if (!log.chromium.has(e.requestId)) {
    log.chromium.set(e.requestId, { url: e.request.url, phase: log.phase, received: 0, byPhase: {}, total: null, failed: null, fromCache: false });
  }
}

const servedFromCache = (response) => !!response.fromDiskCache || !!response.fromPrefetchCache || !!response.fromServiceWorker;

function onResponseReceived(log, e) {
  const c = log.chromium.get(e.requestId);
  if (!c) return;
  Object.assign(c, { status: e.response.status, fromCache: c.fromCache || servedFromCache(e.response) });
  count(log, c, e.response.encodedDataLength ?? 0);
}

function onServedFromCache(log, e) {
  const c = log.chromium.get(e.requestId);
  if (c) c.fromCache = true;
}

function onDataReceived(log, e) {
  const c = log.chromium.get(e.requestId);
  if (c) count(log, c, e.encodedDataLength);
}

function onLoadingFinished(log, e) {
  const c = log.chromium.get(e.requestId);
  if (!c) return;
  c.total = e.encodedDataLength;
  // What arrived uncounted by its chunks (Chromium may report a body's
  // bytes only at its end) counts where the request finished.
  if (c.total > c.received) count(log, c, c.total - c.received);
}

function onLoadingFailed(log, e) {
  const c = log.chromium.get(e.requestId);
  if (c) c.failed = e.errorText || (e.canceled ? "canceled" : "failed");
}

const CDP_EVENTS = [
  ["Network.requestWillBeSent", onRequestWillBeSent],
  ["Network.responseReceived", onResponseReceived],
  ["Network.requestServedFromCache", onServedFromCache],
  ["Network.dataReceived", onDataReceived],
  ["Network.loadingFinished", onLoadingFinished],
  ["Network.loadingFailed", onLoadingFailed],
];

function trackChromium(log, cdp) {
  CDP_EVENTS.forEach(([event, handler]) => cdp.on(event, (e) => handler(log, e)));
}

// --- Playwright's count, request by request ---------------------------------------

function onRequest(log, request) {
  log.pending.set(request, Date.now());
  log.lastActivity = Date.now();
}

/** The request is no longer in flight; unless it was held open longer than
 * a second (background), the network was active just now. Returns when it began. */
function endPending(log, request) {
  const opened = log.pending.get(request) ?? Date.now();
  log.pending.delete(request);
  const longLived = Date.now() - opened > 1000;
  if (!longLived) log.lastActivity = Date.now();
  return opened;
}

const failureText = (request) => request.failure()?.errorText ?? "failed";

function newEntry(log, request, failed, opened) {
  return {
    phase: log.phase,
    url: request.url(),
    method: request.method(),
    kind: request.resourceType(),
    failed: failed ? failureText(request) : null,
    heldMs: Date.now() - opened,
  };
}

const validatorOf = (headers) => (headers.etag ? "etag" : headers["last-modified"] ? "last-modified" : null);

/** The headers an entry records, by the field and the header they come from. */
const HEADER_FIELDS = [
  ["encoding", "content-encoding"],
  ["cacheControl", "cache-control"],
];

function readHeaders(entry, response, headers) {
  entry.status = response?.status() ?? null;
  entry.type = (headers["content-type"] ?? "").split(";")[0];
  HEADER_FIELDS.forEach(([field, header]) => (entry[field] = headers[header] ?? null));
  entry.validator = validatorOf(headers);
}

/** A status that comes with a body: not a 304, nor any other redirect. */
const hasBody = (status) => !!status && status !== 304 && !(status >= 300 && status < 400);

/** The response's body: empty where its status says it has none, or reading it fails. */
async function readBody(response, status) {
  if (!hasBody(status)) return Buffer.alloc(0);
  try {
    return await response.body();
  } catch {
    return Buffer.alloc(0);
  }
}

function addMinified(entry, body) {
  const v = minifiedVerdict(body.toString("utf8"), entry.type);
  entry.minified = v.minified;
  entry.minifiedShare = v.share;
  entry.minifiedBytes = v.minifiedBytes;
}

/** The body's sizes, its blob, whether a script or style is minified, and a document's text. */
function readSizes(entry, body) {
  Object.assign(entry, sizes(body));
  entry.blob = body.length ? gitBlob(body) : null;
  if (/javascript|css/.test(entry.type)) addMinified(entry, body);
  if (entry.kind === "document") entry.text = body.toString("utf8");
}

/** What the engine says it received. Playwright reports a cached response's
 * sizes as negative: nothing came from the network. */
async function readReceived(entry, request) {
  const reported = await request.sizes().catch(() => null);
  entry.receivedBody = reported ? Math.max(0, reported.responseBodySize) : null;
  entry.receivedHeaders = reported ? Math.max(0, reported.responseHeadersSize) : null;
}

async function readResponse(entry, request) {
  const response = await request.response();
  const headers = response ? await response.allHeaders() : {};
  readHeaders(entry, response, headers);
  readSizes(entry, await readBody(response, entry.status));
  await readReceived(entry, request);
}

/** A request that ended: its entry, and for a request that did not fail, its response read into it. */
async function settle(log, request, failed) {
  const opened = endPending(log, request);
  const entry = newEntry(log, request, failed, opened);
  log.entries.push(entry);
  if (failed) return;
  try {
    await readResponse(entry, request);
  } catch (e) {
    entry.error = String(e).split("\n")[0];
  }
}

/** Whether a request in flight began a second ago or less. */
const busyAt = (log, now) => [...log.pending.values()].some((opened) => now - opened <= 1000);

async function quiet(log, quietMs = 400, capMs = 15_000) {
  const until = Date.now() + capMs;
  while (Date.now() < until) {
    const now = Date.now();
    if (!busyAt(log, now) && now - log.lastActivity >= quietMs) return true;
    await sleep(25);
  }
  return false;
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

/** The page's storage emptied: a warm visit keeps the HTTP cache and the cookies only. */
const clearStorage = () => {
  try {
    localStorage.clear();
    sessionStorage.clear();
  } catch {
    /* none */
  }
};

/** A first visit that primes the HTTP cache, with the page's storage cleared and the page left. */
async function prime(page, target) {
  await visit(page, target, null);
  await page.evaluate(clearStorage);
  await page.goto("about:blank");
}

async function prepare(page, target, condition) {
  if (condition === "warm") await prime(page, target);
  else if (condition !== "cold") throw new Error(`unknown condition ${condition}`);
}

/** A DevTools session with the network enabled, for Chromium alone. */
async function devtools(context, page, engine) {
  if (engine !== "chromium") return null;
  const cdp = await context.newCDPSession(page);
  await cdp.send("Network.enable");
  return cdp;
}

/** The measured visit, its network logged and shaped if the run is. */
async function measured(context, page, cdp, target, network) {
  const log = networkLog(context, cdp);
  if (network) await shape(page, cdp, network);
  const result = await visit(page, target, log);
  await log.settled();
  result.received = cdp ? log.chromiumReceived() : null;
  result.requests = log.entries;
  return result;
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
    const cdp = await devtools(context, page, engine);
    await prepare(page, target, condition);
    return await measured(context, page, cdp, target, network);
  } finally {
    await context.close();
  }
}

/** The navigation's own marks and the app's, read in the page. */
const pageTimings = () => {
  const nav = performance.getEntriesByType("navigation")[0];
  const h = window.__h2h;
  const of = (key) => (nav ? nav[key] : null);
  const seen = (key) => h.t[key] ?? null;
  return {
    ttfb: of("responseStart"),
    // Chromium's shaping holds a response's body, not its first byte: the
    // document's last byte is where its round trip shows.
    documentEnd: of("responseEnd"),
    domContentLoaded: of("domContentLoadedEventEnd"),
    load: of("loadEventEnd"),
    fcp: seen("fcp"),
    lcp: h.lcp,
    cls: h.cls,
    appInDom: seen("appInDom"),
    appFrame: seen("appFrame"),
    ready: seen("ready"),
  };
};

/** The load: committed, interactive by the stack's own signal, loaded, and the network quiet. */
async function navigate(page, target, log) {
  log?.setPhase("load");
  await page.goto(target.url, { waitUntil: "commit", timeout: 60_000 });
  await page.waitForFunction(() => window.__h2h && window.__h2h.t.ready != null, null, {
    timeout: 60_000,
    polling: 50,
  });
  await page.waitForLoadState("load", { timeout: 60_000 });
  if (log) await log.quiet();
}

/** One interaction of the target, armed, made, and timed from its input event to the change and the frame after it. */
async function interact(page, step, log) {
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
  return { name: step.name, event: w.event, toDom: w.dom - w.start, toFrame: w.frame - w.start };
}

const timeOrigin = (page) => page.evaluate(() => performance.timeOrigin);
const navigations = (page) => page.evaluate(() => performance.getEntriesByType("navigation").length);

/** A visit: the load, readiness, then each interaction of the target. */
async function visit(page, target, log) {
  const startedAt = Date.now();
  await navigate(page, target, log);
  const timings = await page.evaluate(pageTimings);
  const docBefore = await timeOrigin(page);
  const interactions = [];
  for (const step of target.interactions) interactions.push(await interact(page, step, log));
  // The claim that the page was interactive: its interactions took effect
  // in the document it loaded, not in a new one.
  const docAfter = await timeOrigin(page);
  const sameDocument = docBefore === docAfter && (await navigations(page)) === 1;
  return { timings, interactions, sameDocument, wallMs: Date.now() - startedAt };
}
