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

/** Playwright, as the benchmark's harness installs it (1.58.0): the
 * head-to-head adds no package of its own. */
export function playwright() {
  return createRequire(path.join(HARNESS, "package.json"))("@playwright/test");
}

export function playwrightVersion() {
  const require = createRequire(path.join(HARNESS, "package.json"));
  return require("@playwright/test/package.json").version;
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

/**
 * Whether a script reads as a minifier's output: few line breaks for its
 * size, and little indentation or comment. The rule is printed with every
 * verdict. A minifier writes long lines (Terser, esbuild and Rolldown write
 * a module in a handful), drops indentation and keeps only licence comments;
 * a hand-written or merely bundled script has a line every 30-60 bytes.
 */
export function minifiedVerdict(text) {
  const lines = text.split("\n");
  const bytes = Buffer.byteLength(text);
  const indented = lines.filter((l) => /^(\t| {2,})\S/.test(l)).length;
  const comments = (text.match(/(^|\s)\/\/\s|\/\*(?!!)[\s\S]*?\*\//g) ?? []).length;
  const perLine = bytes / Math.max(1, lines.length);
  const indentShare = indented / Math.max(1, lines.length);
  const minified = bytes < 512 || (perLine >= 200 && indentShare < 0.05);
  return {
    minified,
    rule: "minified: under 512 bytes, or at least 200 bytes a line with under 5% of lines indented",
    bytesPerLine: Math.round(perLine),
    indentedShare: Number(indentShare.toFixed(3)),
    comments,
  };
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
