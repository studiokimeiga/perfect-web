// The head-to-head's server instrument (ADR-0392, "the head-to-head"): what a
// stack's server spends per request under a fixed load.
//
// The load is open: requests are sent on a fixed schedule (`rate` a second
// for `seconds`), whether or not earlier ones have been answered, and each
// one's latency is counted from when it was due, not from when it was sent,
// so a server that falls behind is not flattered by a client that waits for
// it (coordinated omission). A fixed number of warm-up requests come first
// and are not counted.
//
// The server's cost is read from /proc for its whole process tree (`lib.mjs`):
// CPU time (user and system) across the measured requests, divided by the
// requests answered; resident memory before, at its highest (sampled every
// 50 ms) and after. The client runs in this process, not the server's tree,
// so its own work is not counted.

import http from "node:http";
import { PAGE_KIB, TICK_MS, summary, treeCost } from "./lib.mjs";

/** One request: its status and when it ended, or its error. */
function request(agent, url, headers) {
  return new Promise((resolve) => {
    const req = http.request(url, { agent, headers }, (res) => {
      res.on("data", () => {});
      res.on("end", () => resolve({ status: res.statusCode, end: performance.now() }));
      res.on("error", (e) => resolve({ error: String(e), end: performance.now() }));
    });
    req.on("error", (e) => resolve({ error: String(e), end: performance.now() }));
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

/**
 * `rate` requests a second for `seconds`, after `warmup`, to `url` with the
 * headers a browser sends (and each visitor's cookie, round robin).
 */
export async function load({ pid, url, rate = 100, seconds = 10, warmup = 50, cookies = [""], maxSockets = 64 }) {
  const agent = new http.Agent({ keepAlive: true, maxSockets });
  const headersFor = (i) => {
    const h = { "accept-encoding": "br, gzip", accept: "text/html,*/*" };
    const c = cookies[i % cookies.length];
    if (c) h.cookie = c;
    return h;
  };
  for (let i = 0; i < warmup; i++) await request(agent, url, headersFor(i));

  const before = treeCost(pid);
  let peak = before.rssKiB;
  const sampler = setInterval(() => {
    peak = Math.max(peak, treeCost(pid).rssKiB);
  }, 50);
  const total = Math.round(rate * seconds);
  const interval = 1000 / rate;
  const start = performance.now();
  const pending = [];
  for (let i = 0; i < total; i++) {
    const due = start + i * interval;
    const wait = due - performance.now();
    if (wait > 1) await new Promise((r) => setTimeout(r, wait));
    pending.push(request(agent, url, headersFor(i)).then((r) => ({ ...r, due })));
  }
  const results = await Promise.all(pending);
  const elapsed = performance.now() - start;
  clearInterval(sampler);
  const after = treeCost(pid);
  peak = Math.max(peak, after.rssKiB);
  agent.destroy();

  const ok = results.filter((r) => !r.error && r.status >= 200 && r.status < 400);
  const statuses = {};
  for (const r of results) {
    const k = r.error ? "error" : String(r.status);
    statuses[k] = (statuses[k] ?? 0) + 1;
  }
  return {
    url,
    rate,
    seconds,
    sent: total,
    answered: ok.length,
    statuses,
    achievedRate: Number(((results.length / elapsed) * 1000).toFixed(1)),
    latencyMs: summary(results.filter((r) => !r.error).map((r) => r.end - r.due)),
    p99Ms: (() => {
      const xs = results.filter((r) => !r.error).map((r) => r.end - r.due).sort((a, b) => a - b);
      return xs.length ? xs[Math.min(xs.length - 1, Math.floor(xs.length * 0.99))] : null;
    })(),
    cpuMsPerRequest: ok.length ? (after.cpuMs - before.cpuMs) / ok.length : null,
    // What this run can tell apart (`report.mjs` judges by it): the CPU
    // clock's tick over the requests answered; Node's clock; a page.
    resolution: { cpuMsPerRequest: ok.length ? TICK_MS / ok.length : null, latencyMs: 0.001, rssMiB: PAGE_KIB / 1024 },
    rssMiB: {
      before: before.rssKiB / 1024,
      peak: peak / 1024,
      after: after.rssKiB / 1024,
    },
    processes: after.pids,
  };
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

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
    for (let i = 0; i < pages; i++) {
      const context = await browser.newContext();
      contexts.push(context);
      const page = await context.newPage();
      await page.goto(url);
      await ready(page);
    }
    // Settled: whatever loading started is over before the window opens.
    await sleep(2000);
    let requests = 0;
    const count = () => requests++;
    for (const c of contexts) c.on("request", count);
    const before = treeCost(pid);
    await sleep(seconds * 1000);
    const after = treeCost(pid);
    for (const c of contexts) c.off("request", count);
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
    for (const c of contexts) await c.close();
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
