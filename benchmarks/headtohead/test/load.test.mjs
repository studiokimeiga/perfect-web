// The head-to-head's server instrument (load.mjs), pinned as it is, so that the
// file can be split into smaller functions without changing what it does: the
// visitors' sessions, the open load and what it reads of the server's process
// tree, what open pages cost while idle, and a press with other tabs open. The
// server is a loopback server in this process; the server's /proc, the 50 ms
// sampler's reading and the two functions' seconds are faked where a test
// needs the number exact or the wait short (said where). Costs of a real
// process are asserted as relations.
import assert from "node:assert/strict";
import fs from "node:fs";
import http from "node:http";
import net from "node:net";
import test from "node:test";
import { setTimeout as delay } from "node:timers/promises";
import { PAGE_KIB, TICK_MS } from "../lib.mjs";
import { idleCost, load, pressWithTabsOpen, sessions } from "../load.mjs";

const NEEDS_PROC = { skip: !fs.existsSync("/proc/self/stat") && "needs /proc" };

// --- helpers ------------------------------------------------------------------

const listen = (server) => new Promise((resolve) => server.listen(0, "127.0.0.1", () => resolve(server.address().port)));

const shut = (server) =>
  new Promise((resolve) => {
    server.closeAllConnections?.();
    server.close(resolve);
  });

async function refusedUrl() {
  const server = net.createServer();
  const port = await listen(server);
  await shut(server);
  return `http://127.0.0.1:${port}/`;
}

async function until(condition, ms = 2000) {
  const end = performance.now() + ms;
  while (!condition() && performance.now() < end) await delay(10);
  return condition();
}

/**
 * A loopback HTTP server. `handler(req, res, n)` answers the nth request
 * (from 1). `seen` holds each request's line and headers, `stats` the most
 * requests open at once, the connections made and those still open.
 */
async function serving(handler, fn) {
  const seen = [];
  const stats = { open: 0, mostOpen: 0, connections: 0, sockets: 0 };
  const server = http.createServer((req, res) => {
    seen.push({ line: `${req.method} ${req.url}`, headers: req.headers });
    stats.mostOpen = Math.max(stats.mostOpen, ++stats.open);
    res.on("close", () => stats.open--);
    handler(req, res, seen.length);
  });
  server.on("connection", (socket) => {
    stats.connections++;
    stats.sockets++;
    socket.on("close", () => stats.sockets--);
  });
  const port = await listen(server);
  try {
    return await fn({ url: `http://127.0.0.1:${port}/`, port, seen, stats });
  } finally {
    await shut(server);
  }
}

const ok = (req, res) => res.end("ok");
const slowly = (ms) => (req, res) => setTimeout(() => res.end("ok"), ms);

/** The /proc files of these processes ([pid, children, utime, stime, pages]). */
function snapshot(...processes) {
  const files = {};
  processes.forEach(([pid, children, utime, stime, pages]) => {
    files[`/proc/${pid}/task`] = [`${pid}`];
    files[`/proc/${pid}/task/${pid}/children`] = children.join(" ");
    files[`/proc/${pid}/stat`] = `${pid} (server) S 1 1 1 0 -1 0 0 0 0 0 ${utime} ${stime} 0 0 20 0 1 0 1 1 1\n`;
    files[`/proc/${pid}/statm`] = `${pages * 4} ${pages} 0 0 0 0 0\n`;
  });
  return files;
}

/** Read /proc from `state.files` (changed by the test as the run goes on). */
function installProc(t, state) {
  const reading = (real) => (p, ...rest) => (Object.hasOwn(state.files, p) ? state.files[p] : real.call(fs, p, ...rest));
  t.mock.method(fs, "readdirSync", reading(fs.readdirSync));
  t.mock.method(fs, "readFileSync", reading(fs.readFileSync));
  return state;
}

/** The sampler `load` starts every 50 ms, held back for the test to call. */
function heldSampler(t) {
  const held = { fn: null, ms: null, cleared: [] };
  t.mock.method(globalThis, "setInterval", (fn, ms) => {
    held.fn = fn;
    held.ms = ms;
    return 77;
  });
  t.mock.method(globalThis, "clearInterval", (handle) => held.cleared.push(handle));
  return held;
}

/** Timers that fire at once and keep the delays asked of them (`onSleep` is told each one). */
function fastClock(t, onSleep = () => {}) {
  const delays = [];
  t.mock.method(globalThis, "setTimeout", (fn, ms, ...rest) => {
    onSleep(delays.push(ms) - 1, ms);
    return setImmediate(fn, ...rest);
  });
  return delays;
}

const FAKE = 5_000_101; // pids above any pid_max (4,194,304): none exists
const KID_A = 5_000_102;
const KID_B = 5_000_103;
const MiB = (pages) => (pages * PAGE_KIB) / 1024;
const NOTHING = { n: 0, missing: 0, median: null, q1: null, q3: null, min: null, max: null };
const LOAD_KEYS = ["url", "rate", "seconds", "sent", "answered", "statuses", "achievedRate", "latencyMs", "p99Ms", "cpuMsPerRequest", "resolution", "rssMiB", "processes"];

// --- sessions ---------------------------------------------------------------------------

test("`sessions` is one cookie string per visitor: the name=value pairs of the first answer's Set-Cookie, attributes dropped, joined by '; '", async () => {
  const cookies = [[], ["sid=abc; Path=/; HttpOnly; SameSite=Lax"], ["a=1; Secure", "b=2"], ["token=x==; Max-Age=60"]];
  const answer = (req, res, n) => res.setHeader("set-cookie", cookies[n - 1]).end("hello");
  await serving(answer, async ({ url, seen }) => {
    assert.deepEqual(await sessions(url, 4), ["", "sid=abc", "a=1; b=2", "token=x=="]);
    assert.deepEqual(seen.map((s) => s.line), ["GET /", "GET /", "GET /", "GET /"]);
    assert.deepEqual(seen.map((s) => "cookie" in s.headers), [false, false, false, false]);
  });
});

test("`sessions` reads the cookies off a redirect and does not follow it", async () => {
  const redirect = (req, res) => res.writeHead(302, { location: "/elsewhere", "set-cookie": "s=1; Path=/" }).end();
  await serving(redirect, async ({ url, seen }) => {
    assert.deepEqual(await sessions(url, 2), ["s=1", "s=1"]);
    assert.deepEqual(seen.map((s) => s.line), ["GET /", "GET /"]);
  });
});

test("`sessions` of none asks for none", async () => {
  await serving(ok, async ({ url, seen }) => {
    assert.deepEqual(await sessions(url, 0), []);
    assert.deepEqual(seen, []);
  });
});

test("`sessions` gives an empty cookie string per visitor where headers cannot list Set-Cookie", async () => {
  const original = Object.getOwnPropertyDescriptor(Headers.prototype, "getSetCookie");
  Object.defineProperty(Headers.prototype, "getSetCookie", { ...original, value: undefined });
  try {
    await serving(
      (req, res) => res.setHeader("set-cookie", "s=1").end("x"),
      async ({ url }) => assert.deepEqual(await sessions(url, 2), ["", ""]),
    );
  } finally {
    Object.defineProperty(Headers.prototype, "getSetCookie", original);
  }
});

test("`sessions` fails with the fetch's error where nothing answers", async () => {
  await assert.rejects(sessions(await refusedUrl(), 1), { name: "TypeError", message: "fetch failed" });
});

// --- load -------------------------------------------------------------------------------------

const WARMUP = 2;
const TOTAL = 8;

test("`load` reports what it sent, what was answered, the server's CPU and memory from its process tree, and what it can tell apart", async (t) => {
  const state = installProc(t, { files: snapshot([FAKE, [], 300, 200, 2560]) });
  const sampler = heldSampler(t);
  // The nth request moves /proc on. The first (a warm-up's) spends 5 ticks and
  // grows a little: the run starts from there. The memory peaks at the first
  // measured request and is read (by the sampler) there, and lower at the
  // second; at the last the server has two children and has spent 95 more
  // ticks since the run began.
  const moves = {
    1: () => {
      state.files = snapshot([FAKE, [], 305, 200, 2600]);
    },
    [WARMUP + 1]: () => {
      state.files = snapshot([FAKE, [], 310, 210, 7680]);
      sampler.fn();
    },
    [WARMUP + 2]: () => {
      state.files = snapshot([FAKE, [], 320, 220, 5120]);
      sampler.fn();
    },
    [WARMUP + TOTAL]: () => {
      state.files = snapshot([FAKE, [KID_A, KID_B], 340, 260, 3840], [KID_A, [], 0, 0, 100], [KID_B, [], 0, 0, 100]);
    },
  };
  await serving(
    (req, res, n) => {
      moves[n]?.();
      res.end("ok");
    },
    async ({ url, seen, stats }) => {
      const result = await load({ pid: FAKE, url, rate: 80, seconds: 0.1, warmup: WARMUP });
      const { achievedRate, latencyMs, p99Ms, ...rest } = result;
      assert.deepEqual(Object.keys(result), LOAD_KEYS);
      assert.deepEqual(rest, {
        url,
        rate: 80,
        seconds: 0.1,
        sent: TOTAL,
        answered: TOTAL,
        statuses: { 200: TOTAL },
        cpuMsPerRequest: (95 * TICK_MS) / TOTAL,
        resolution: { cpuMsPerRequest: TICK_MS / TOTAL, latencyMs: 0.001, rssMiB: PAGE_KIB / 1024 },
        rssMiB: { before: MiB(2600), peak: MiB(7680), after: MiB(4040) },
        processes: 3,
      });
      assert.equal(seen.length, WARMUP + TOTAL);
      assert.deepEqual([sampler.ms, sampler.cleared], [50, [77]]);
      assert.equal(await until(() => stats.sockets === 0), true);
    },
  );
});

// The memory (in pages) before the run and after it: the peak is the higher
// of the two where the sampler read none higher.
const BEFORE_AFTER = [
  [5120, 2560],
  [2560, 5120],
];

test("`load` takes the higher of the memory before and after the run for its peak where nothing sampled was higher, and reports no CPU where none was spent", async (t) => {
  const state = installProc(t, { files: {} });
  heldSampler(t);
  for (const [before, after] of BEFORE_AFTER) {
    state.files = snapshot([FAKE, [], 300, 200, before]);
    const last = (req, res, n) => {
      if (n === WARMUP + TOTAL) state.files = snapshot([FAKE, [], 300, 200, after]);
      res.end("ok");
    };
    await serving(last, async ({ url }) => {
      const result = await load({ pid: FAKE, url, rate: 80, seconds: 0.1, warmup: WARMUP });
      assert.deepEqual(result.rssMiB, { before: MiB(before), peak: MiB(5120), after: MiB(after) });
      assert.deepEqual([result.cpuMsPerRequest, result.processes], [0, 1]);
    });
  }
});

test("`load` keeps the latency of each answered request from when it was due, and its 99th percentile at the index 0.99 n of the sorted ones", async () => {
  // Two slow requests among n: the slowest takes 300 ms and the next 150 ms.
  const slowest = (n) => ({ 41: 300, 81: 150 })[n] ?? 0;
  const answer = (req, res, n) => setTimeout(() => res.end("ok"), slowest(n));
  const runs = [];
  for (const seconds of [0.2, 0.24]) {
    await serving(answer, async ({ url }) => {
      runs.push(await load({ pid: FAKE, url, rate: 500, seconds, warmup: 0 }));
    });
  }
  // At 100 the index is the last: the slowest. At 120 it is the one before it.
  const [hundred, hundredTwenty] = runs;
  assert.deepEqual([hundred.latencyMs.n, hundredTwenty.latencyMs.n], [100, 120]);
  assert.equal(hundred.p99Ms, hundred.latencyMs.max);
  assert.equal(hundredTwenty.latencyMs.max > 250, true, String(hundredTwenty.latencyMs.max));
  assert.deepEqual([hundred.p99Ms > 250, hundredTwenty.p99Ms >= 100, hundredTwenty.p99Ms < hundredTwenty.latencyMs.max], [true, true, true], JSON.stringify(runs.map((r) => r.p99Ms)));
});

test("`load` counts each answer by its status and a dropped connection or a cut-off body as an error, and only 200 to 399 as answered", async (t) => {
  const state = installProc(t, { files: snapshot([FAKE, [], 300, 200, 2560]) });
  heldSampler(t);
  const status = (code) => (req, res) => res.writeHead(code, { location: "/elsewhere" }).end("x");
  const cutOff = (req, res) => {
    state.files = snapshot([FAKE, [], 330, 200, 2560]);
    res.writeHead(200, { "content-length": "100" });
    res.write("0123456789");
    setTimeout(() => req.socket.destroy(), 5);
  };
  // The first is the warm-up's, which nothing counts.
  const answers = [status(418), status(200), status(302), status(399), status(400), status(404), status(500), (req) => req.socket.destroy(), cutOff];
  await serving(
    (req, res, n) => answers[n - 1](req, res),
    async ({ url, seen }) => {
      const result = await load({ pid: FAKE, url, rate: 80, seconds: 0.1, warmup: 1 });
      assert.equal(result.sent, 8);
      assert.equal(result.answered, 3);
      assert.deepEqual(result.statuses, { 200: 1, 302: 1, 399: 1, 400: 1, 404: 1, 500: 1, error: 2 });
      assert.deepEqual([result.latencyMs.n, result.latencyMs.missing], [6, 0]);
      assert.deepEqual(seen.map((s) => s.line), Array(9).fill("GET /"));
      // The CPU spent is shared among the answered requests, not all sent.
      assert.equal(result.cpuMsPerRequest, (30 * TICK_MS) / 3);
      assert.equal(result.resolution.cpuMsPerRequest, TICK_MS / 3);
    },
  );
});

test("`load` against a server that refuses every connection counts every request an error and reports no latency, tail or CPU per request", async (t) => {
  installProc(t, { files: snapshot([FAKE, [], 300, 200, 2560]) });
  const url = await refusedUrl();
  const { achievedRate, ...rest } = await load({ pid: FAKE, url, rate: 50, seconds: 0.1, warmup: 2 });
  // Five requests over four intervals of 20 ms: the rate counts every one that ended, answered or not.
  assert.deepEqual([achievedRate >= 10, achievedRate <= 50 * 1.5], [true, true], String(achievedRate));
  assert.deepEqual(rest, {
    url,
    rate: 50,
    seconds: 0.1,
    sent: 5,
    answered: 0,
    statuses: { error: 5 },
    latencyMs: NOTHING,
    p99Ms: null,
    cpuMsPerRequest: null,
    resolution: { cpuMsPerRequest: null, latencyMs: 0.001, rssMiB: PAGE_KIB / 1024 },
    rssMiB: { before: MiB(2560), peak: MiB(2560), after: MiB(2560) },
    processes: 1,
  });
});

test("`load` of no measured request still warms up, one request at a time on one connection, and reports nothing but its quiet tree", async (t) => {
  installProc(t, { files: snapshot([FAKE, [], 300, 200, 2560]) });
  await serving(ok, async ({ url, seen, stats }) => {
    const result = await load({ pid: FAKE, url, rate: 10, seconds: 0.01, warmup: 3 });
    assert.deepEqual(result, {
      url,
      rate: 10,
      seconds: 0.01,
      sent: 0,
      answered: 0,
      statuses: {},
      achievedRate: 0,
      latencyMs: NOTHING,
      p99Ms: null,
      cpuMsPerRequest: null,
      resolution: { cpuMsPerRequest: null, latencyMs: 0.001, rssMiB: PAGE_KIB / 1024 },
      rssMiB: { before: MiB(2560), peak: MiB(2560), after: MiB(2560) },
      processes: 1,
    });
    assert.equal(seen.length, 3);
    assert.deepEqual([stats.mostOpen, stats.connections], [1, 1]);
    assert.deepEqual(seen.map((s) => "cookie" in s.headers), [false, false, false]);
  });
});

test("`load` sends rate x seconds requests, rounded half up", async () => {
  await serving(ok, async ({ url }) => {
    const [three, two] = [await load({ pid: FAKE, url, rate: 100, seconds: 0.025, warmup: 0 }), await load({ pid: FAKE, url, rate: 100, seconds: 0.024, warmup: 0 })];
    assert.deepEqual([three.sent, three.answered, two.sent, two.answered], [3, 3, 2, 2]);
  });
});

test("`load` sends 100 requests a second for 10 seconds after 50 warm-up requests, with no cookie, unless told otherwise (timers that fire at once)", async (t) => {
  installProc(t, { files: snapshot([FAKE, [], 300, 200, 2560]) });
  fastClock(t);
  await serving(ok, async ({ url, seen }) => {
    const result = await load({ pid: FAKE, url });
    assert.deepEqual([result.rate, result.seconds, result.sent, result.answered, result.statuses], [100, 10, 1000, 1000, { 200: 1000 }]);
    assert.equal(seen.length, 1050);
    assert.equal(seen.filter((s) => "cookie" in s.headers).length, 0);
  });
});

test("`load` sends the headers of a browser and each visitor's cookie in turn, the warm-up's starting over where the run's does", async () => {
  const cookies = ["a=1", "", "c=3"];
  await serving(ok, async ({ url, seen, port }) => {
    await load({ pid: FAKE, url: `${url}store?x=1`, rate: 100, seconds: 0.05, warmup: 2, cookies });
    const headers = (cookie) => ({ accept: "text/html,*/*", "accept-encoding": "br, gzip", connection: "keep-alive", host: `127.0.0.1:${port}`, ...(cookie && { cookie }) });
    assert.deepEqual(seen.map((s) => s.line), Array(7).fill("GET /store?x=1"));
    // Two warm-up requests, then five: a, none, c, a, none.
    assert.deepEqual(seen.map((s) => s.headers), ["a=1", "", "a=1", "", "c=3", "a=1", ""].map(headers));
  });
});

test("`load` queues behind its sockets and counts latency from when each request was due: one socket against a slow server builds a queue", async () => {
  await serving(slowly(50), async ({ url, stats }) => {
    const result = await load({ pid: FAKE, url, rate: 100, seconds: 0.08, warmup: 0, maxSockets: 1 });
    assert.deepEqual([stats.mostOpen, stats.connections], [1, 1]);
    assert.equal(result.latencyMs.n, 8);
    assert.equal(result.latencyMs.max > 300, true, JSON.stringify(result.latencyMs));
    assert.equal(result.latencyMs.max > 2 * result.latencyMs.min, true, JSON.stringify(result.latencyMs));
    assert.equal(result.p99Ms, result.latencyMs.max);
  });
});

test("`load` does not wait for answers and keeps up to 64 sockets by default: slow answers overlap", async () => {
  await serving(slowly(50), async ({ url, stats }) => {
    const result = await load({ pid: FAKE, url, rate: 100, seconds: 0.08, warmup: 0 });
    assert.equal(result.answered, 8);
    assert.equal(stats.mostOpen >= 3, true, JSON.stringify(stats));
    assert.equal(stats.connections >= 3, true, JSON.stringify(stats));
  });
});

test("`load` of a live process: its costs are whole ticks and pages, its rate at most the rate asked, its latencies in order", NEEDS_PROC, async () => {
  await serving(ok, async ({ url }) => {
    const result = await load({ pid: process.pid, url, rate: 80, seconds: 0.1, warmup: 2 });
    const { latencyMs: l, rssMiB: r } = result;
    assert.deepEqual(Object.keys(result), LOAD_KEYS);
    assert.deepEqual(Object.keys(l), Object.keys(NOTHING));
    const rate = result.achievedRate;
    assert.deepEqual([rate >= 80 / 16, rate <= 80 * 1.2, Number(rate.toFixed(1)) === rate], [true, true, true], String(rate));
    // From when each was due, a quick server's answers take a millisecond or so, not the run's length.
    assert.deepEqual([l.min <= l.q1, l.q1 <= l.median, l.median <= l.q3, l.q3 <= l.max, l.median < 20], [true, true, true, true, true], JSON.stringify(l));
    assert.equal(result.p99Ms, l.max);
    assert.equal(result.processes, 1);
    assert.deepEqual([r.before > 0, r.peak >= r.before, r.peak >= r.after], [true, true, true], JSON.stringify(r));
    assert.equal(((result.cpuMsPerRequest * 8) / TICK_MS) % 1, 0);
    assert.deepEqual(result.resolution, { cpuMsPerRequest: TICK_MS / 8, latencyMs: 0.001, rssMiB: PAGE_KIB / 1024 });
  });
});

// --- idleCost and pressWithTabsOpen ------------------------------------------------------------

const PAGE_URL = "http://app.test/";

/**
 * A browser that notes what it is asked in `events`, in order, and fails the
 * step named by `failAt`. Each context it makes fires one more request than
 * the one before when it is listened to; `added` and `removed` are the
 * listeners it was given and told to drop.
 */
function rig(failAt = "") {
  const events = [];
  const added = [];
  const removed = [];
  const counts = { contexts: 0, pages: 0 };
  const step = async (label) => {
    events.push(label);
    if (label === failAt) throw new Error(`${label} failed`);
  };
  const newPage = async () => {
    const id = counts.pages++;
    await step(`page ${id}`);
    return { id, goto: (url) => step(`goto ${id} ${url}`) };
  };
  const newContext = async () => {
    const id = counts.contexts++;
    await step(`context ${id}`);
    const on = (name, fn) => {
      events.push(`on ${id} ${name}`);
      added.push(fn);
      queueMicrotask(() => Array.from({ length: id + 1 }, fn));
    };
    const off = (name, fn) => {
      events.push(`off ${id} ${name}`);
      removed.push(fn);
    };
    return { newPage, on, off, close: () => step(`close ${id}`) };
  };
  const press = async (page) => {
    await step(`press ${page.id}`);
    return 12.5;
  };
  return { events, added, removed, browser: { newContext }, ready: (page) => step(`ready ${page.id}`), press };
}

const opened = (ids) => ids.flatMap((i) => [`context ${i}`, `page ${i}`, `goto ${i} ${PAGE_URL}`, `ready ${i}`]);
const range = (n) => Array.from({ length: n }, (_, i) => i);

test("`idleCost` loads each page in a context of its own, settles 2 s, counts the requests over the window and reports the cost", async (t) => {
  const state = installProc(t, { files: snapshot([FAKE, [], 300, 200, 2560]) });
  const { events, added, removed, browser, ready } = rig();
  // The server spends 100 ticks and holds 5120 pages by the end of the window.
  const delays = fastClock(t, (i, ms) => {
    events.push(`sleep ${ms}`);
    if (i === 1) state.files = snapshot([FAKE, [], 340, 260, 5120]);
  });
  const result = await idleCost({ browser, url: PAGE_URL, pid: FAKE, pages: 4, seconds: 0.5, ready });
  const listening = range(4).map((i) => `on ${i} request`);
  const ignoring = range(4).map((i) => `off ${i} request`);
  const closing = range(4).map((i) => `close ${i}`);
  assert.deepEqual(delays, [2000, 500]);
  assert.deepEqual(events, [...opened(range(4)), "sleep 2000", ...listening, "sleep 500", ...ignoring, ...closing]);
  assert.deepEqual([added.length, new Set(added).size, removed], [4, 1, added]);
  assert.deepEqual(result, {
    pages: 4,
    contexts: 4,
    seconds: 0.5,
    requests: 10,
    requestsPerPagePerMinute: 300,
    cpuMsPerSecond: (100 * TICK_MS) / 0.5,
    rssMiB: MiB(5120),
    resolution: { requestsPerPagePerMinute: 30, cpuMsPerSecond: TICK_MS / 0.5, rssMiB: PAGE_KIB / 1024 },
  });
});

test("`idleCost` opens 20 pages and watches them for 10 s unless told otherwise", async (t) => {
  installProc(t, { files: snapshot([FAKE, [], 300, 200, 2560]) });
  const delays = fastClock(t);
  const { events, browser, ready } = rig();
  const result = await idleCost({ browser, url: PAGE_URL, pid: FAKE, ready });
  assert.deepEqual(delays, [2000, 10000]);
  assert.deepEqual(events.slice(0, 8), opened([0, 1]));
  assert.deepEqual([result.pages, result.contexts, result.seconds, result.requests], [20, 20, 10, 210]);
  assert.equal(result.requestsPerPagePerMinute, 63);
  assert.deepEqual([result.cpuMsPerSecond, result.rssMiB], [0, MiB(2560)]);
  assert.equal(events.filter((e) => e.startsWith("close")).length, 20);
});

const IDLE_FAILURES = [
  ["context 2", ["close 0", "close 1"]],
  ["page 2", ["close 0", "close 1", "close 2"]],
  [`goto 2 ${PAGE_URL}`, ["close 0", "close 1", "close 2"]],
  ["ready 2", ["close 0", "close 1", "close 2"]],
];

test("`idleCost` closes every context it made, in turn, when a step fails, and listens to none", async (t) => {
  installProc(t, { files: snapshot([FAKE, [], 300, 200, 2560]) });
  fastClock(t);
  for (const [failAt, closes] of IDLE_FAILURES) {
    const { events, browser, ready } = rig(failAt);
    await assert.rejects(idleCost({ browser, url: PAGE_URL, pid: FAKE, pages: 4, seconds: 0.5, ready }), { message: `${failAt} failed` });
    assert.deepEqual(events.slice(events.indexOf(failAt) + 1), closes, failAt);
    assert.equal(events.some((e) => e.startsWith("on ")), false, failAt);
  }
});

test("`idleCost` stops closing at a context that will not close, and fails with that", async (t) => {
  installProc(t, { files: snapshot([FAKE, [], 300, 200, 2560]) });
  fastClock(t);
  const { events, browser, ready } = rig("close 1");
  await assert.rejects(idleCost({ browser, url: PAGE_URL, pid: FAKE, pages: 3, seconds: 0.5, ready }), { message: "close 1 failed" });
  assert.deepEqual(events.slice(-3), ["off 2 request", "close 0", "close 1"]);
});

test("`pressWithTabsOpen` opens the tabs in one context, settles, presses in the last tab and reports the press and its resolution", async (t) => {
  const { events, browser, ready, press } = rig();
  const delays = fastClock(t, (i, ms) => events.push(`sleep ${ms}`));
  const result = await pressWithTabsOpen({ browser, url: PAGE_URL, tabs: 3, settleSeconds: 0.25, ready, press, resolutionMs: 0.5 });
  const tabsOpened = range(3).flatMap((i) => [`page ${i}`, `goto ${i} ${PAGE_URL}`, `ready ${i}`]);
  assert.deepEqual(events, ["context 0", ...tabsOpened, "sleep 250", "press 2", "close 0"]);
  assert.deepEqual(delays, [250]);
  assert.deepEqual(Object.keys(result), ["tabs", "settleSeconds", "pressMs", "resolution"]);
  assert.deepEqual(result, { tabs: 3, settleSeconds: 0.25, pressMs: 12.5, resolution: { pressMs: 0.5 } });
});

test("`pressWithTabsOpen` opens 20 tabs and settles 4 s unless told otherwise, and passes on an unknown resolution as it is", async (t) => {
  const delays = fastClock(t);
  const { events, browser, ready, press } = rig();
  const result = await pressWithTabsOpen({ browser, url: PAGE_URL, ready, press });
  assert.deepEqual(delays, [4000]);
  assert.deepEqual(events.slice(-3), ["ready 19", "press 19", "close 0"]);
  assert.equal(events.filter((e) => e.startsWith("page ")).length, 20);
  assert.deepEqual(result, { tabs: 20, settleSeconds: 4, pressMs: 12.5, resolution: { pressMs: undefined } });
});

const PRESS_FAILURES = [
  [`goto 1 ${PAGE_URL}`, ["close 0"]],
  ["ready 1", ["close 0"]],
  ["press 2", ["close 0"]],
  ["page 0", ["close 0"]],
];

test("`pressWithTabsOpen` closes its context when a step or the press fails, and does not press after a failed tab", async (t) => {
  fastClock(t);
  for (const [failAt, closes] of PRESS_FAILURES) {
    const { events, browser, ready, press } = rig(failAt);
    await assert.rejects(pressWithTabsOpen({ browser, url: PAGE_URL, tabs: 3, settleSeconds: 0.25, ready, press }), { message: `${failAt} failed` });
    assert.deepEqual(events.slice(events.indexOf(failAt) + 1), closes, failAt);
    assert.equal(events.includes("press 2") && failAt !== "press 2", false, failAt);
  }
});

test("`pressWithTabsOpen` has no context to close where the browser cannot make one", async (t) => {
  fastClock(t);
  const { events, browser, ready, press } = rig("context 0");
  await assert.rejects(pressWithTabsOpen({ browser, url: PAGE_URL, ready, press }), { message: "context 0 failed" });
  assert.deepEqual(events, ["context 0"]);
});
