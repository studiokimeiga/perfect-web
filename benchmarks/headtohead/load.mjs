// The head-to-head's server instrument (ADR-0392, "the head-to-head"): what a
// stack's server spends per request under a fixed load.
//
// The load is open: requests are sent on a fixed schedule (`rate` a second
// for `seconds`), whether or not earlier ones have been answered, and each
// one's latency is counted from when it was due, not from when it was sent,
// so a server that falls behind is not flattered by a client that waits for
// it (coordinated omission). A request a timer sent before it was due is
// counted from when it was sent: a latency is never less than its answer
// took. A fixed number of warm-up requests come first and are not counted.
//
// The server's cost is read from /proc for its whole process tree (`lib.mjs`):
// CPU time (user and system) across the measured requests, divided by the
// requests answered; resident memory before, at its highest (sampled every
// 50 ms) and after. The client runs in this process, not the server's tree,
// so its own work is not counted.

import http from "node:http";
import { PAGE_KIB, TICK_MS, sleep, summary, treeCost } from "./lib.mjs";

/** One request: when it was sent, and its status and when it ended, or its error. */
function request(agent, url, headers) {
  return new Promise((resolve) => {
    const sent = performance.now();
    const req = http.request(url, { agent, headers }, (res) => {
      res.on("data", () => {});
      res.on("end", () => resolve({ status: res.statusCode, sent, end: performance.now() }));
      res.on("error", (e) => resolve({ error: String(e), sent, end: performance.now() }));
    });
    req.on("error", (e) => resolve({ error: String(e), sent, end: performance.now() }));
    req.end();
  });
}

/** A session per virtual visitor: the cookie the stack's first answer sets. */
export async function sessions(url, n) {
  const out = [];
  for (let i = 0; i < n; i++) {
    const res = await fetch(url, { redirect: "manual" });
    await res.arrayBuffer();
    const set = res.headers.getSetCookie?.() ?? [];
    out.push(set.map((c) => c.split(";")[0]).join("; "));
  }
  return out;
}

/** The headers of the i-th request: what a browser sends, and each visitor's cookie, round robin. */
function headerSource(cookies) {
  return (i) => {
    const h = { "accept-encoding": "br, gzip", accept: "text/html,*/*" };
    const c = cookies[i % cookies.length];
    if (c) h.cookie = c;
    return h;
  };
}

/** The server's resident memory at its highest, sampled every 50 ms until stopped. */
function trackPeak(pid, start) {
  let peak = start;
  const timer = setInterval(() => {
    peak = Math.max(peak, treeCost(pid).rssKiB);
  }, 50);
  return { peak: () => peak, stop: () => clearInterval(timer) };
}

/** `total` requests sent on a fixed schedule, one every `interval` ms, whether or not the earlier ones have been answered. */
async function sendOnSchedule(agent, url, headersFor, total, interval) {
  const start = performance.now();
  const pending = [];
  for (let i = 0; i < total; i++) {
    const due = start + i * interval;
    const wait = due - performance.now();
    if (wait > 1) await sleep(wait);
    pending.push(request(agent, url, headersFor(i)).then((r) => ({ ...r, due })));
  }
  return { start, results: await Promise.all(pending) };
}

/** The answers that came with a status under 400. */
const answered = (results) => results.filter((r) => !r.error && r.status >= 200 && r.status < 400);

/** How many answers each status got ("error" for a request that failed). */
function countStatuses(results) {
  const statuses = {};
  for (const r of results) {
    const k = r.error ? "error" : String(r.status);
    statuses[k] = (statuses[k] ?? 0) + 1;
  }
  return statuses;
}

/**
 * **Each answered request's latency, from when it was due**, or from when it
 * was sent where a timer sent it before then. Node's timers keep the event
 * loop's time in whole milliseconds, so a sleep can end up to a millisecond
 * before its due time as `performance.now()` reads it, and a quick answer to
 * a request sent that early came out negative: the static host's median
 * latency was -0.06 ms, and every stack's was short by the early sends, a
 * noise as large as the latencies compared (train-15).
 */
export const latencies = (results) => results.filter((r) => !r.error).map((r) => r.end - Math.min(r.due, r.sent));

/** The 99th percentile of the latencies, or null where there are none. */
function p99(xs) {
  const sorted = xs.slice().sort((a, b) => a - b);
  return sorted.length ? sorted[Math.min(sorted.length - 1, Math.floor(sorted.length * 0.99))] : null;
}

/** A total per request answered, or null where none was. */
const perRequest = (total, n) => (n ? total / n : null);

/**
 * `rate` requests a second for `seconds`, after `warmup`, to `url` with the
 * headers a browser sends (and each visitor's cookie, round robin).
 */
export async function load({ pid, url, rate = 100, seconds = 10, warmup = 50, cookies = [""], maxSockets = 64 }) {
  const agent = new http.Agent({ keepAlive: true, maxSockets });
  const headersFor = headerSource(cookies);
  for (let i = 0; i < warmup; i++) await request(agent, url, headersFor(i));

  const before = treeCost(pid);
  const memory = trackPeak(pid, before.rssKiB);
  const total = Math.round(rate * seconds);
  const { start, results } = await sendOnSchedule(agent, url, headersFor, total, 1000 / rate);
  const elapsed = performance.now() - start;
  memory.stop();
  const after = treeCost(pid);
  const peak = Math.max(memory.peak(), after.rssKiB);
  agent.destroy();

  const ok = answered(results);
  return {
    url,
    rate,
    seconds,
    sent: total,
    answered: ok.length,
    statuses: countStatuses(results),
    achievedRate: Number(((results.length / elapsed) * 1000).toFixed(1)),
    latencyMs: summary(latencies(results)),
    p99Ms: p99(latencies(results)),
    cpuMsPerRequest: perRequest(after.cpuMs - before.cpuMs, ok.length),
    // What this run can tell apart (`report.mjs` judges by it): the CPU
    // clock's tick over the requests answered; Node's clock; a page.
    resolution: { cpuMsPerRequest: perRequest(TICK_MS, ok.length), latencyMs: 0.001, rssMiB: PAGE_KIB / 1024 },
    rssMiB: {
      before: before.rssKiB / 1024,
      peak: peak / 1024,
      after: after.rssKiB / 1024,
    },
    processes: after.pids,
  };
}

/** Opens `pages` pages of the app, each in a browser context of its own, ready; the contexts go into `contexts` as they are made, so that a failure leaves them to be closed. */
async function openPages(browser, url, pages, ready, contexts) {
  for (let i = 0; i < pages; i++) {
    const context = await browser.newContext();
    contexts.push(context);
    const page = await context.newPage();
    await page.goto(url);
    await ready(page);
  }
}

/** The requests the contexts make, and the server's cost, over `seconds` of nobody touching the pages. */
async function watch(contexts, pid, seconds) {
  let requests = 0;
  const count = () => requests++;
  contexts.forEach((c) => c.on("request", count));
  const before = treeCost(pid);
  await sleep(seconds * 1000);
  const after = treeCost(pid);
  contexts.forEach((c) => c.off("request", count));
  return { requests, before, after };
}

const closeAll = async (contexts) => {
  for (const c of contexts) await c.close();
};

/**
 * What open pages cost a server while nobody touches them: `pages` visitors,
 * each page in a browser context of its own, loaded and left alone (a person
 * reading) for `seconds`. The server's CPU over that window, and the
 * requests the pages made in it (a page that keeps a live connection asks
 * again whenever its server closes one). A stack whose pages ask nothing
 * while idle costs its server nothing.
 *
 * A context of its own for each, as separate people's browsers are: one
 * context's pages share its six connections to a host over HTTP/1.1, so
 * twenty tabs of one browser can hold no more than six requests open, and
 * what they cost their server is that cap's, not the pages' (until
 * 2026-11-02 this measured one context: 29 requests in 10 s, the six
 * connections' ceiling for streams held two seconds each). One person's
 * tabs sharing their connections is `pressWithTabsOpen`'s question.
 */
export async function idleCost({ browser, url, pid, pages = 20, seconds = 10, ready }) {
  const contexts = [];
  try {
    await openPages(browser, url, pages, ready, contexts);
    // Settled: whatever loading started is over before the window opens.
    await sleep(2000);
    const { requests, before, after } = await watch(contexts, pid, seconds);
    return {
      pages,
      contexts: contexts.length,
      seconds,
      requests,
      requestsPerPagePerMinute: (requests / pages) * (60 / seconds),
      cpuMsPerSecond: (after.cpuMs - before.cpuMs) / seconds,
      rssMiB: after.rssKiB / 1024,
      // One request over the window, a CPU tick over it, a page.
      resolution: { requestsPerPagePerMinute: (1 / pages) * (60 / seconds), cpuMsPerSecond: TICK_MS / seconds, rssMiB: PAGE_KIB / 1024 },
    };
  } finally {
    await closeAll(contexts);
  }
}

/**
 * A press with other tabs of the app open in one person's browser: `tabs`
 * pages in ONE browser context, loaded and left for `settleSeconds`; then a
 * press in the last, timed by `press` to its change. Over HTTP/1.1 a browser
 * opens at most six connections to one host, and a connection a tab holds is
 * one the press waits for.
 */
export async function pressWithTabsOpen({ browser, url, tabs = 20, settleSeconds = 4, ready, press, resolutionMs }) {
  const context = await browser.newContext();
  try {
    const opened = [];
    for (let i = 0; i < tabs; i++) {
      const page = await context.newPage();
      await page.goto(url);
      await ready(page);
      opened.push(page);
    }
    await sleep(settleSeconds * 1000);
    return { tabs, settleSeconds, pressMs: await press(opened[opened.length - 1]), resolution: { pressMs: resolutionMs } };
  } finally {
    await context.close();
  }
}
