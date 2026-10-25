// The head-to-head's server instrument (ADR-XXXX, "the head-to-head"): what a
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
import { summary, treeCost } from "./lib.mjs";

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
    rssMiB: {
      before: before.rssKiB / 1024,
      peak: peak / 1024,
      after: after.rssKiB / 1024,
    },
    processes: after.pids,
  };
}

/**
 * What open pages cost a server while nobody touches them: `pages` tabs of
 * the app, each loaded and left alone (a person reading), for `seconds`. The
 * server's CPU over that window, and the requests the pages made in it (a
 * page that keeps a live connection asks again whenever its server closes
 * one). A stack whose pages ask nothing while idle costs its server nothing.
 */
export async function idleCost({ browser, url, pid, pages = 20, seconds = 10, ready, press = null }) {
  const context = await browser.newContext();
  try {
    const opened = [];
    for (let i = 0; i < pages; i++) {
      const page = await context.newPage();
      await page.goto(url);
      await ready(page);
      opened.push(page);
    }
    // Settled: whatever loading started is over before the window opens.
    await new Promise((r) => setTimeout(r, 2000));
    let requests = 0;
    const count = () => requests++;
    context.on("request", count);
    const before = treeCost(pid);
    await new Promise((r) => setTimeout(r, seconds * 1000));
    const after = treeCost(pid);
    context.off("request", count);
    // Then a person presses, in the last of the pages, with the others open:
    // a browser opens at most six connections to one host over HTTP/1.1, and
    // a connection a page holds is one the press waits for.
    const pressMs = press ? await press(opened[opened.length - 1]) : null;
    return {
      pages,
      seconds,
      requests,
      requestsPerPagePerMinute: (requests / pages) * (60 / seconds),
      cpuMsPerSecond: (after.cpuMs - before.cpuMs) / seconds,
      rssMiB: after.rssKiB / 1024,
      pressMs,
    };
  } finally {
    await context.close();
  }
}
