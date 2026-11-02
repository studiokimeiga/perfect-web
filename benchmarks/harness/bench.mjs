#!/usr/bin/env node
// E14-B: the benchmark harness, offline (ADR-0124).
//
//   node bench.mjs run --task T08 --stack next-react --agent noop
//   node bench.mjs run --task T08 --stack pleris --agent replay:reference
//   node bench.mjs controls --task T08            # the four controls, every stack
//
// One run is: a sandbox made from the stack's baseline and the task's setup,
// an agent run in it, then grading — the stack's checker and build, and the
// shared contract plus the task's hidden tests against the sandbox's own
// server. Score 1 when every stage passes, 0 otherwise. One JSON per run in
// benchmarks/results/.
//
// Offline, an agent is `noop` or `replay:<reference|unsafe>`, which applies
// the task's patch through the same path an agent's work takes. A model is an
// `--agent` command, and is E14-E's, not this file's.

import { spawn, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { agentFailureKind, readAgentUsage, scoreAgentRun } from "./agent-telemetry.mjs";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const HARNESS = path.dirname(fileURLToPath(import.meta.url));
const BENCH = path.resolve(HARNESS, "..");
const REPO = path.resolve(BENCH, "..");
const TASKS = path.join(BENCH, "tasks");
const RESULTS = path.join(BENCH, "results");

const STACKS = {
  "next-react": {
    baseline: path.join(BENCH, "baselines/next-react"),
    exclude: ["node_modules", ".next"],
    storePath: "/stores/blue-bottle",
    check: [["pnpm", ["--silent", "typecheck"]]],
    build: [["pnpm", ["--silent", "build"]]],
    serve: (dir, port) => ["pnpm", ["--silent", "start"], { PORT: String(port) }],
  },
  sveltekit: {
    baseline: path.join(BENCH, "baselines/sveltekit"),
    exclude: ["node_modules", ".svelte-kit", "build"],
    storePath: "/stores/blue-bottle",
    check: [["pnpm", ["--silent", "typecheck"]]],
    build: [["pnpm", ["--silent", "build"]]],
    serve: (dir, port) => [
      "node",
      ["build"],
      { PORT: String(port), HOST: "127.0.0.1", ORIGIN: `http://127.0.0.1:${port}` },
    ],
  },
  pleris: {
    // The store's sources. The platform packages and the toolchain are the
    // repository's: a Pleris agent edits its program, as a Next agent edits
    // its app and not Next.
    sources: [
      "benchmarks/baselines/pleris/domain.pw",
      "benchmarks/baselines/pleris/lib",
      "benchmarks/baselines/pleris/store",
    ],
    storePath: "/StorePage.html",
    // `pw check` runs inside the build, which refuses a program that does
    // not check; it is its own stage so the result says which one failed.
    check: [[path.join(REPO, "target/debug/pw"), null]],
    build: [["bash", [path.join(REPO, "spikes/own-renderer/run.sh")]]],
    serve: (dir, port) => [
      path.join(REPO, "target/debug/pw-dev-server"),
      [path.join(dir, "..", "dist"), path.join(dir, "..", "dist", "build")],
      { PORT: String(port) },
    ],
  },
};

// --- small helpers ---------------------------------------------------------

function sh(cmd, args, opts = {}) {
  const r = spawnSync(cmd, args, { encoding: "utf8", maxBuffer: 64 << 20, ...opts });
  return { ok: r.status === 0, status: r.status, signal: r.signal ?? null, error_code: r.error?.code ?? null, out: `${r.stdout ?? ""}${r.stderr ?? ""}` };
}

function tail(text, n = 40) {
  return text.split("\n").slice(-n).join("\n");
}

function copy(from, to, exclude = []) {
  fs.cpSync(from, to, {
    recursive: true,
    filter: (src) => !exclude.includes(path.basename(src)),
  });
}

function files(dir) {
  const out = [];
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    if (e.name === "node_modules" || e.name === ".git") continue;
    const p = path.join(dir, e.name);
    if (e.isDirectory()) out.push(...files(p));
    else if (e.isFile()) out.push(p);
  }
  return out;
}

function digest(file) {
  return createHash("sha256").update(fs.readFileSync(file)).digest("hex");
}

async function freePort() {
  const net = await import("node:net");
  return new Promise((resolve) => {
    const s = net.createServer().listen(0, "127.0.0.1", () => {
      const { port } = s.address();
      s.close(() => resolve(port));
    });
  });
}

async function waitFor(url, ms) {
  const until = Date.now() + ms;
  while (Date.now() < until) {
    try {
      const r = await fetch(url);
      if (r.status < 500) return true;
    } catch {
      /* not up yet */
    }
    await new Promise((r) => setTimeout(r, 250));
  }
  return false;
}

// --- the task ----------------------------------------------------------------

function loadTask(id) {
  const dir = fs.readdirSync(TASKS).find((d) => d === id || d.startsWith(`${id}-`));
  if (!dir) throw new Error(`no task ${id} under ${TASKS}`);
  const root = path.join(TASKS, dir);
  const task = JSON.parse(fs.readFileSync(path.join(root, "task.json"), "utf8"));
  return { ...task, root, prompt: path.join(root, "prompt.md") };
}

/** Everything the agent must not see: hidden tests and every patch. */
function secrets(task) {
  return ["hidden", "reference", "unsafe"]
    .map((d) => path.join(task.root, d))
    .filter((d) => fs.existsSync(d))
    .flatMap(files);
}

// --- the sandbox -----------------------------------------------------------

/**
 * A fresh copy of the stack's baseline with the task's setup applied, as a
 * git repository with one commit: what the agent starts from, and what its
 * changed lines are measured against.
 */
export function sandbox(task, stack) {
  const s = STACKS[stack];
  const root = fs.mkdtempSync(path.join(os.tmpdir(), `pw-bench-${task.id}-${stack}-`));
  // A sandbox that could not be made leaves nothing behind (ADR-0158).
  try {
    const app = path.join(root, "app");
    if (s.baseline) {
      copy(s.baseline, app, s.exclude);
      // Its own dependencies, from a lockfile the harness pins, installed from
      // the local store and never the network: the same versions every run.
      // Not a symlink to the workspace's: Turbopack refuses a `node_modules`
      // outside the project's root, and a symlink would share one tree between
      // runs.
      fs.copyFileSync(path.join(HARNESS, "locks", `${stack}.pnpm-lock.yaml`), path.join(app, "pnpm-lock.yaml"));
      const install = sh("pnpm", ["install", "--offline", "--frozen-lockfile", "--ignore-workspace", "--silent"], {
        cwd: app,
      });
      if (!install.ok) throw new Error(`the sandbox's dependencies did not install: ${tail(install.out)}`);
    } else {
      fs.mkdirSync(app);
      for (const src of s.sources) {
        copy(path.join(REPO, src), path.join(app, path.basename(src)));
      }
    }
    const git = (...args) => sh("git", args, { cwd: app });
    git("init", "-q");
    git("config", "user.email", "bench@localhost");
    git("config", "user.name", "bench");
    fs.writeFileSync(path.join(app, ".gitignore"), "node_modules\n.next\n.svelte-kit\nbuild\n");
    const setup = path.join(task.root, "setup", `${stack}.patch`);
    if (fs.existsSync(setup)) {
      const r = git("apply", setup);
      if (!r.ok) throw new Error(`the setup patch does not apply: ${r.out}`);
    }
    git("add", "-A");
    git("commit", "-q", "-m", "baseline");
    fs.copyFileSync(task.prompt, path.join(root, "PROMPT.md"));

    // Isolation, checked rather than assumed: no file in the sandbox is, byte
    // for byte, one the agent must not see.
    const hidden = new Set(secrets(task).map(digest));
    const leaked = files(root).filter((f) => hidden.has(digest(f)));
    if (leaked.length > 0) {
      throw new Error(`refusing to run: the sandbox holds hidden files: ${leaked.join(", ")}`);
    }
    return { root, app };
  } catch (error) {
    fs.rmSync(root, { recursive: true, force: true });
    throw error;
  }
}

/**
 * **The lines a run changed**: added and removed, in the files the agent
 * edited and in the files it made, what the sandbox ignores aside. `git diff
 * --numstat` alone lists only the files git tracks, and until 2026-10-21 a
 * patch that makes a file counted only its edits: T06's Next.js reference,
 * its 41-line form in a file of its own, counted 3. Intent to add puts each
 * new file in the diff without staging what it holds.
 */
export function changedLines(app) {
  const marked = sh("git", ["add", "--intent-to-add", "--all"], { cwd: app });
  if (!marked.ok) throw new Error(`the changed files could not be listed: ${tail(marked.out)}`);
  const diff = sh("git", ["diff", "--numstat"], { cwd: app }).out.trim();
  return diff
    ? diff.split("\n").reduce((n, l) => n + l.split("\t").slice(0, 2).map(Number).reduce((a, b) => a + (b || 0), 0), 0)
    : 0;
}

// --- the agent -------------------------------------------------------------

export function runAgent(task, stack, agent, box) {
  const started = Date.now();
  if (agent === "noop") {
    return { ok: true, seconds: 0, out: "" };
  }
  const replay = /^replay:(reference|unsafe)$/.exec(agent);
  if (replay) {
    const patch = path.join(task.root, replay[1], `${stack}.patch`);
    if (!fs.existsSync(patch)) return { ok: false, seconds: 0, out: `no ${patch}` };
    const r = sh("git", ["apply", patch], { cwd: box.app });
    return { ok: r.ok, seconds: (Date.now() - started) / 1000, out: tail(r.out) };
  }
  // Real agent telemetry lives outside the graded app.
  const usageFile = path.join(box.root, "AGENT_USAGE.json");
  // A real agent: a shell command, run in the sandbox, with the prompt's path.
  const r = sh("bash", ["-c", agent], {
    cwd: box.app,
    env: { ...process.env, BENCH_PROMPT: path.join(box.root, "PROMPT.md"), BENCH_STACK: stack, BENCH_USAGE: usageFile },
    timeout: Number(process.env.BENCH_AGENT_SECONDS ?? 1800) * 1000,
  });
  const seconds = (Date.now() - started) / 1000;
  let usage = null;
  try {
    usage = readAgentUsage(usageFile);
  } catch (error) {
    return { ok: false, seconds, out: tail(r.out), usage: null, failure_kind: "infra", problem: error.message };
  }
  return { ok: r.ok, seconds, out: tail(r.out), usage, failure_kind: r.ok ? null : agentFailureKind(r) };
}

// --- grading ---------------------------------------------------------------

function stage(name, steps, box, env = {}, cwd = box.app) {
  const started = Date.now();
  for (const [cmd, args] of steps) {
    const r = sh(cmd, args, { cwd, env: { ...process.env, ...env } });
    if (!r.ok) {
      return { name, ok: false, seconds: (Date.now() - started) / 1000, out: tail(r.out) };
    }
  }
  return { name, ok: true, seconds: (Date.now() - started) / 1000 };
}

function plerisSources(box) {
  return [
    ...files(path.join(REPO, "packages/pw-std")),
    ...files(path.join(REPO, "packages/pw-platform-web")),
    path.join(box.app, "domain.pw"),
    ...files(path.join(box.app, "lib")),
    ...files(path.join(box.app, "store")),
  ].filter((f) => f.endsWith(".pw"));
}

async function grade(task, stack, box) {
  const s = STACKS[stack];
  const stages = [];
  const pleris = stack === "pleris";
  const buildEnv = pleris
    ? {
        PW_SOURCES: box.app,
        PW_OUT: path.join(box.root, "dist"),
        PW_STORE_IR: path.join(box.root, "store-ir.json"),
        BUILD_ONLY: "1",
      }
    : {};

  const check = pleris
    ? stage("check", [[s.check[0][0], ["check", "--plain", ...plerisSources(box)]]], box)
    : stage("check", s.check, box);
  stages.push(check);
  if (!check.ok) return stages;
  // Pleris's build runs from the repository, whose `rust-toolchain.toml`
  // pins the compiler's toolchain; the sandbox holds the program only.
  const build = stage("build", s.build, box, buildEnv, pleris ? REPO : box.app);
  stages.push(build);
  if (!build.ok) return stages;

  // The sandbox's own server, on a port of its own.
  const port = await freePort();
  const [cmd, args, env] = s.serve(box.app, port);
  const server = spawn(cmd, args, {
    cwd: box.app,
    env: { ...process.env, ...env },
    stdio: ["ignore", "pipe", "pipe"],
    detached: true,
  });
  let serverOut = "";
  server.stdout.on("data", (d) => (serverOut += d));
  server.stderr.on("data", (d) => (serverOut += d));
  let specs = null;
  try {
    const base = `http://127.0.0.1:${port}`;
    if (!(await waitFor(`${base}${s.storePath}`, 60_000))) {
      stages.push({ name: "serve", ok: false, out: tail(serverOut) });
      return stages;
    }
    stages.push({ name: "serve", ok: true });

    // The contract and the hidden tests, copied outside the sandbox, beside
    // the harness so they resolve its `@playwright/test`.
    fs.mkdirSync(path.join(HARNESS, ".grading"), { recursive: true });
    specs = fs.mkdtempSync(path.join(HARNESS, ".grading", "run-"));
    for (const f of fs.readdirSync(path.join(HARNESS, "contract"))) {
      fs.copyFileSync(path.join(HARNESS, "contract", f), path.join(specs, f));
    }
    fs.copyFileSync(path.join(task.root, task.hidden), path.join(specs, path.basename(task.hidden)));
    const report = path.join(specs, "report.json");
    const started = Date.now();
    sh("pnpm", ["exec", "playwright", "test", "--config", path.join(HARNESS, "grade.config.mjs")], {
      cwd: HARNESS,
      env: {
        ...process.env,
        BENCH_SPECS: specs,
        BENCH_REPORT: report,
        BENCH_STACK: stack,
        BENCH_BASE_URL: base,
        BENCH_STORE_PATH: s.storePath,
      },
    });
    const tests = readReport(report);
    const hiddenName = path.basename(task.hidden);
    for (const [name, mine] of [
      ["contract", tests.filter((t) => t.file !== hiddenName)],
      ["hidden", tests.filter((t) => t.file === hiddenName)],
    ]) {
      const failed = mine.filter((t) => !t.ok);
      stages.push({
        name,
        ok: mine.length > 0 && failed.length === 0,
        passed: mine.length - failed.length,
        failed: failed.length,
        failures: failed.map((t) => `${t.title}: ${t.error}`.slice(0, 400)),
        seconds: (Date.now() - started) / 1000,
      });
    }
  } finally {
    if (specs) fs.rmSync(specs, { recursive: true, force: true });
    try {
      process.kill(-server.pid, "SIGTERM");
    } catch {
      /* already gone */
    }
  }
  return stages;
}

function readReport(file) {
  if (!fs.existsSync(file)) return [];
  const report = JSON.parse(fs.readFileSync(file, "utf8"));
  const out = [];
  const walk = (suite) => {
    for (const spec of suite.specs ?? []) {
      for (const t of spec.tests ?? []) {
        const last = t.results?.[t.results.length - 1];
        out.push({
          file: path.basename(spec.file ?? suite.file ?? ""),
          title: spec.title,
          ok: t.status === "expected",
          error: (last?.error?.message ?? "").replace(/\x1b\[[0-9;]*m/g, "").split("\n")[0],
        });
      }
    }
    for (const s of suite.suites ?? []) walk(s);
  };
  for (const s of report.suites ?? []) walk(s);
  return out;
}

// --- one run -----------------------------------------------------------------

export async function run({ task: id, stack, agent }) {
  if (!STACKS[stack]) throw new Error(`no stack ${stack}`);
  const task = loadTask(id);
  if (!task.stacks.includes(stack)) throw new Error(`${task.id} is not written for ${stack}`);
  const started = new Date();
  const box = sandbox(task, stack);
  // However the run ends (ADR-0158), unless it is kept to be looked at.
  try {
    const agentRun = runAgent(task, stack, agent, box);
    const changed = changedLines(box.app);
    const stages = agentRun.ok ? await grade(task, stack, box) : [];
    const { score, failedAt } = scoreAgentRun(agentRun, stages);
    const result = {
      task: task.id,
      stack,
      agent,
      score,
      failed_at: failedAt,
      changed_lines: changed,
      agent_run: agentRun,
      stages,
      started: started.toISOString(),
      seconds: (Date.now() - started.getTime()) / 1000,
      commit: sh("git", ["rev-parse", "HEAD"], { cwd: REPO }).out.trim(),
    };
    fs.mkdirSync(RESULTS, { recursive: true });
    const file = path.join(
      RESULTS,
      `${task.id}-${stack}-${agent.replace(/[^a-z0-9]+/gi, "_")}-${started.getTime()}.json`,
    );
    fs.writeFileSync(file, `${JSON.stringify(result, null, 2)}\n`);
    return { ...result, file };
  } finally {
    if (!process.env.BENCH_KEEP) fs.rmSync(box.root, { recursive: true, force: true });
  }
}

// --- the controls --------------------------------------------------------------

/**
 * A score is evidence only if the instrument has shown it can tell good from
 * bad (docs/RISK_QUEUE.md's admissibility rule), per task and per stack:
 *
 *   negative   the unchanged starting point fails the hidden tests
 *   positive   the reference patch passes everything
 *   unsafe     a plausible wrong patch scores 0, by a test or a checker
 *   harness    the no-op agent scores 0 (it is the negative, through the
 *              agent path) and the reference replay scores 1
 */
export async function controls(id, stacks) {
  const task = loadTask(id);
  const rows = [];
  for (const stack of stacks ?? task.stacks) {
    const noop = await run({ task: id, stack, agent: "noop" });
    const reference = await run({ task: id, stack, agent: "replay:reference" });
    const unsafe = await run({ task: id, stack, agent: "replay:unsafe" });
    // Every verdict needs tests that ran: a stage of zero tests proves
    // nothing, whichever way it reads (docs/RISK_QUEUE.md's recurring bug).
    const ran = (r, name) => {
      const s = r.stages.find((x) => x.name === name);
      return s ? (s.passed ?? 0) + (s.failed ?? 0) : 0;
    };
    const stage = (r, name) => r.stages.find((x) => x.name === name);
    rows.push({
      stack,
      // The unchanged start passes the contract and fails the hidden tests;
      // or, for a task that starts from a broken program, fails somewhere
      // real: a checker, a build, or a test that ran. A task starts broken
      // on every stack (`"start": "broken"`, T12), or on the stacks it names
      // (`"start": {"pleris": "broken"}`, T08 since ADR-0154, where a checker
      // refuses what the frameworks build).
      negative:
        noop.score === 0 &&
        (task.start === "broken" || task.start?.[stack] === "broken"
          ? ["check", "build"].includes(noop.failed_at) || (stage(noop, noop.failed_at)?.failed ?? 0) > 0
          : stage(noop, "contract")?.ok === true && (stage(noop, "hidden")?.failed ?? 0) > 0),
      positive: reference.score === 1 && ran(reference, "contract") > 0 && ran(reference, "hidden") > 0,
      // Caught by a checker, a build, or a test that ran; never by a stage
      // that did not run.
      unsafe:
        unsafe.score === 0 &&
        (["check", "build"].includes(unsafe.failed_at) ||
          (stage(unsafe, unsafe.failed_at)?.failed ?? 0) > 0),
      unsafe_caught_at: unsafe.failed_at,
      harness: noop.score === 0 && reference.score === 1,
      results: [noop.file, reference.file, unsafe.file].map((f) => path.relative(REPO, f)),
    });
  }
  return rows;
}

// --- the command line ----------------------------------------------------------

function arg(name) {
  const i = process.argv.indexOf(`--${name}`);
  return i > 0 ? process.argv[i + 1] : undefined;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const command = process.argv[2];
  if (command === "run") {
    const r = await run({ task: arg("task"), stack: arg("stack"), agent: arg("agent") ?? "noop" });
    console.log(
      `${r.task} ${r.stack} ${r.agent}: ${r.score === null ? "ungraded" : "score " + r.score}${r.failed_at ? ` (failed at ${r.failed_at})` : ""}, ` +
        `${r.changed_lines} line(s) changed -> ${path.relative(REPO, r.file)}`,
    );
  } else if (command === "controls") {
    const stacks = arg("stack") ? [arg("stack")] : undefined;
    const rows = await controls(arg("task"), stacks);
    let ok = true;
    for (const r of rows) {
      const all = r.negative && r.positive && r.unsafe && r.harness;
      ok &&= all;
      console.log(
        `${arg("task")} ${r.stack}: negative ${r.negative ? "ok" : "FAIL"}, positive ${r.positive ? "ok" : "FAIL"}, ` +
          `unsafe ${r.unsafe ? `ok (caught at ${r.unsafe_caught_at})` : "FAIL"}, harness ${r.harness ? "ok" : "FAIL"}`,
      );
    }
    process.exit(ok ? 0 : 1);
  } else {
    console.error("usage: node bench.mjs run --task T08 --stack <stack> [--agent noop|replay:reference|replay:unsafe|<command>]");
    console.error("       node bench.mjs controls --task T08 [--stack <stack>]");
    process.exit(2);
  }
}
