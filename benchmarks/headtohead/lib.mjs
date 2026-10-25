// The head-to-head's small helpers (ADR-XXXX, "the head-to-head"): statistics,
// sizes as a static host sends them, the machine, processes and their cost,
// and servers started and stopped. Nothing here measures by itself.

import { spawn, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import fs from "node:fs";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { createRequire } from "node:module";
import { brotliCompressSync, constants, gzipSync } from "node:zlib";

export const HERE = path.dirname(fileURLToPath(import.meta.url));
export const BENCH = path.resolve(HERE, "..");
export const REPO = path.resolve(BENCH, "..");
export const HARNESS = path.join(BENCH, "harness");

/** Playwright, 1.58.0, as this package pins it (`package.json`), the same
 * the benchmark's harness and the own renderer pin. */
export function playwright() {
  return createRequire(path.join(HERE, "package.json"))("@playwright/test");
}

export function playwrightVersion() {
  return createRequire(path.join(HERE, "package.json"))("@playwright/test/package.json").version;
}

// --- statistics ---------------------------------------------------------------

/** The value at quantile q of sorted numbers, by linear interpolation
 * (the "type 7" rule of R and NumPy's default). */
export function quantile(sorted, q) {
  if (!sorted.length) return null;
  const at = (sorted.length - 1) * q;
  const lo = Math.floor(at);
  const hi = Math.ceil(at);
  return sorted[lo] + (sorted[hi] - sorted[lo]) * (at - lo);
}

/** Median, quartiles, extremes and count of the numbers given; nulls (a
 * measurement an engine does not report) are left out and counted. */
export function summary(values) {
  const xs = values.filter((v) => typeof v === "number" && Number.isFinite(v)).sort((a, b) => a - b);
  return {
    n: xs.length,
    missing: values.length - xs.length,
    median: quantile(xs, 0.5),
    q1: quantile(xs, 0.25),
    q3: quantile(xs, 0.75),
    min: xs.length ? xs[0] : null,
    max: xs.length ? xs[xs.length - 1] : null,
  };
}

// --- sizes ------------------------------------------------------------------------

/** A body's bytes, and its bytes compressed as a static host compresses it:
 * Brotli at quality 11 and gzip at level 9 (`static-host.mjs`). */
export function sizes(body) {
  return {
    raw: body.length,
    br: brotliCompressSync(body, {
      params: {
        [constants.BROTLI_PARAM_QUALITY]: 11,
        [constants.BROTLI_PARAM_SIZE_HINT]: body.length,
      },
    }).length,
    gzip: gzipSync(body, { level: 9 }).length,
  };
}

/** Git's name for a file's content: `git hash-object`, without git. */
export function gitBlob(body) {
  return createHash("sha1").update(`blob ${body.length}\0`).update(body).digest("hex");
}

export function sha256(body) {
  return createHash("sha256").update(body).digest("hex");
}

// --- minified or not --------------------------------------------------------------

/** What "minified" means here, printed with every verdict. */
export const MINIFIED_RULE =
  "minified: esbuild 0.28.2's minifier (whitespace, syntax and identifiers) takes less than 10% more off it; under 1 KiB counts as minified";

/**
 * Whether a script or stylesheet is a minifier's output: what a minifier
 * still finds to take off it. A file another minifier wrote (Terser, SWC,
 * Rolldown's) loses a few percent more to esbuild's; a file no minifier
 * touched loses its indentation, comments and long names, typically 30-70%.
 * The minifier is esbuild, as the SvelteKit baseline pins it (this package's
 * `package.json`); a file it cannot parse is reported unknown (`null`), not
 * guessed at. Returns the share esbuild took off, and what it made of it.
 */
export function minifiedVerdict(text, type = "text/javascript") {
  const bytes = Buffer.byteLength(text);
  if (bytes < 1024) return { minified: true, rule: MINIFIED_RULE, share: null, minifiedBytes: bytes };
  let esbuild;
  try {
    esbuild = createRequire(path.join(HERE, "package.json"))("esbuild");
  } catch {
    return { minified: null, rule: MINIFIED_RULE, share: null, minifiedBytes: null, error: "esbuild is not installed" };
  }
  try {
    const loader = /css/.test(type) ? "css" : "js";
    const out = esbuild.transformSync(text, { loader, minify: true, legalComments: "inline", logLevel: "silent" });
    const after = Buffer.byteLength(out.code);
    const share = 1 - after / bytes;
    return { minified: share < 0.1, rule: MINIFIED_RULE, share: Number(share.toFixed(3)), minifiedBytes: after };
  } catch (e) {
    return { minified: null, rule: MINIFIED_RULE, share: null, minifiedBytes: null, error: String(e).split("\n")[0] };
  }
}

// --- the machine --------------------------------------------------------------------

export function machine() {
  const cpus = os.cpus();
  const osRelease = (() => {
    try {
      const t = fs.readFileSync("/etc/os-release", "utf8");
      return /^PRETTY_NAME="?([^"\n]*)"?/m.exec(t)?.[1] ?? null;
    } catch {
      return null;
    }
  })();
  return {
    cpu: cpus[0]?.model?.trim() ?? "unknown",
    logicalCpus: cpus.length,
    memoryGiB: Number((os.totalmem() / 2 ** 30).toFixed(1)),
    os: osRelease ?? `${os.type()} ${os.release()}`,
    kernel: os.release(),
    node: process.version,
    loadAverage: os.loadavg().map((l) => Number(l.toFixed(2))),
  };
}

// --- processes ------------------------------------------------------------------------

const TICK_MS = 1000 / Number(spawnSync("getconf", ["CLK_TCK"], { encoding: "utf8" }).stdout.trim() || 100);
const PAGE_KIB = Number(spawnSync("getconf", ["PAGESIZE"], { encoding: "utf8" }).stdout.trim() || 4096) / 1024;

function children(pid) {
  try {
    const tasks = fs.readdirSync(`/proc/${pid}/task`);
    const out = [];
    for (const t of tasks) {
      try {
        const c = fs.readFileSync(`/proc/${pid}/task/${t}/children`, "utf8").trim();
        if (c) out.push(...c.split(/\s+/).map(Number));
      } catch {
        /* gone */
      }
    }
    return out;
  } catch {
    return [];
  }
}

/** A process and every descendant it has now. */
export function processTree(pid) {
  const seen = new Set();
  const stack = [pid];
  while (stack.length) {
    const p = stack.pop();
    if (seen.has(p)) continue;
    seen.add(p);
    stack.push(...children(p));
  }
  return [...seen];
}

/** CPU time (user + system, ms) and resident memory (KiB) of a process tree,
 * from /proc. A process that has gone counts nothing. */
export function treeCost(pid) {
  let cpuMs = 0;
  let rssKiB = 0;
  const pids = processTree(pid);
  for (const p of pids) {
    try {
      const stat = fs.readFileSync(`/proc/${p}/stat`, "utf8");
      const fields = stat.slice(stat.lastIndexOf(")") + 2).split(" ");
      // utime and stime are fields 14 and 15 of stat(5); here, after the
      // command's closing parenthesis, at 11 and 12.
      cpuMs += (Number(fields[11]) + Number(fields[12])) * TICK_MS;
      const statm = fs.readFileSync(`/proc/${p}/statm`, "utf8").split(" ");
      rssKiB += Number(statm[1]) * PAGE_KIB;
    } catch {
      /* gone */
    }
  }
  return { pids: pids.length, cpuMs, rssKiB };
}

// --- servers --------------------------------------------------------------------------

/** Whether a port on the loopback is free: a server already there must never
 * be taken for the one a run starts. */
export function portFree(port) {
  return new Promise((resolve) => {
    const s = net.createServer();
    s.once("error", () => resolve(false));
    s.listen(port, "127.0.0.1", () => s.close(() => resolve(true)));
  });
}

export async function waitForHttp(url, ms, alive = () => true) {
  const until = Date.now() + ms;
  while (Date.now() < until) {
    if (!alive()) return false;
    try {
      const r = await fetch(url, { redirect: "manual" });
      await r.arrayBuffer();
      return true;
    } catch {
      await new Promise((r) => setTimeout(r, 150));
    }
  }
  return false;
}

/**
 * A server, started in its own process group so that it and everything it
 * starts can be stopped together. `probe` is a path no route serves: the
 * readiness check must not be the first read of what a page reads (ADR-0342,
 * finding 8).
 */
export async function startServer({ name, cmd, args, cwd, env = {}, port, probe = "/__h2h-ready", log }) {
  if (!(await portFree(port))) throw new Error(`${name}: port ${port} is already taken`);
  const out = fs.openSync(log, "w");
  const child = spawn(cmd, args, {
    cwd,
    env: { ...process.env, ...env },
    stdio: ["ignore", out, out],
    detached: true,
  });
  let exited = null;
  child.on("exit", (code, signal) => (exited = { code, signal }));
  const up = await waitForHttp(`http://127.0.0.1:${port}${probe}`, 90_000, () => exited === null);
  if (!up) {
    try {
      process.kill(-child.pid, "SIGKILL");
    } catch {
      /* gone */
    }
    const tail = fs.readFileSync(log, "utf8").split("\n").slice(-20).join("\n");
    throw new Error(`${name}: did not start on ${port}${exited ? ` (exited ${JSON.stringify(exited)})` : ""}\n${tail}`);
  }
  return {
    name,
    pid: child.pid,
    port,
    base: `http://127.0.0.1:${port}`,
    log,
    alive: () => exited === null,
    async stop() {
      if (exited) return;
      try {
        process.kill(-child.pid, "SIGTERM");
      } catch {
        return;
      }
      const until = Date.now() + 5000;
      while (exited === null && Date.now() < until) await new Promise((r) => setTimeout(r, 50));
      if (exited === null) {
        try {
          process.kill(-child.pid, "SIGKILL");
        } catch {
          /* gone */
        }
      }
    },
  };
}

/** Fisher-Yates, from a seeded generator, so that a run's order is printed
 * and can be run again. */
export function shuffled(list, seed) {
  let s = seed >>> 0 || 1;
  const rand = () => {
    s ^= s << 13;
    s ^= s >>> 17;
    s ^= s << 5;
    return (s >>> 0) / 2 ** 32;
  };
  const out = [...list];
  for (let i = out.length - 1; i > 0; i--) {
    const j = Math.floor(rand() * (i + 1));
    [out[i], out[j]] = [out[j], out[i]];
  }
  return out;
}
