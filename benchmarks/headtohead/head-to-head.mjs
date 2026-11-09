#!/usr/bin/env node
// The head-to-head (ADR-0392): the same apps in Pleris and in the mainstream
// stacks, measured by one instrument, Pleris's losses first.
//
//   node benchmarks/headtohead/head-to-head.mjs [--apps todomvc,rows,store]
//        [--engines chromium,firefox,webkit] [--runs 7] [--conditions cold,warm]
//        [--no-shaped] [--seed 20261025] [--load-rate 100] [--load-seconds 10]
//        [--server-runs 5]
//        [--no-load] [--stacks pleris,react] [--json out.json]
//
// It measures, it does not build: `just e14-head-to-head` builds each stack
// first (the Pleris pages and its host, the frameworks' production builds),
// and this refuses a build that is missing or stale (`validate.mjs`). Then:
//
// 1. each app's servers are started (`targets.mjs`): the Pleris host's
//    release build, the frameworks' production servers, and for the
//    client-rendered TodoMVCs the static host (`static-host.mjs`);
// 2. in each engine, for each condition (cold, warm), `runs` rounds, each
//    visiting every stack once in an order shuffled from the seed and the
//    round (printed): one load and the app's interactions (`measure.mjs`);
//    then, in Chromium alone, the same under Lighthouse's "Slow 4G" shaping
//    as Lighthouse applies it through DevTools (`measure.mjs`'s SLOW_4G);
// 3. each server, `serverRuns` rounds in shuffled orders: under a fixed load,
//    twenty idle visitors, and a press with nineteen other tabs of the app
//    open in one browser (`load.mjs`);
// 4. every rule of `validate.mjs`: a run that breaks one is not reported
//    as measured, and this exits 1 with the problems;
// 5. the report: the machine and versions, what each stack is and who wrote
//    it, where Pleris loses (first), then every table.
//
// `--json` keeps every run, every response and every number the report
// reads, for the ADR and for a run to be checked against.

import { execFileSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { idleCost, load, pressWithTabsOpen, sessions } from "./load.mjs";
import { HERE, REPO, isMain, machine, playwright, playwrightVersion, shuffled, sleep, startServer } from "./lib.mjs";
import { SLOW_4G, measureOnce } from "./measure.mjs";
import { APPS, PW_SERVER, target } from "./targets.mjs";
import { MIN_RUNS, validate, validateServer } from "./validate.mjs";
import { renderReport, verdictRows } from "./report.mjs";

/** How often a press's change is looked for: what its time can tell apart. */
const PRESS_POLL_MS = 10;

// --- the command line -------------------------------------------------------------

const list = (value) => value.split(",");
const absolute = (value) => path.resolve(value);

/** What each option sets, and how it reads the argument after it. */
const OPTIONS = new Map([
  ["--apps", ["apps", list]],
  ["--engines", ["engines", list]],
  ["--runs", ["runs", Number]],
  ["--conditions", ["conditions", list]],
  ["--seed", ["seed", Number]],
  ["--load-rate", ["loadRate", Number]],
  ["--load-seconds", ["loadSeconds", Number]],
  ["--server-runs", ["serverRuns", Number]],
  ["--settle", ["settleSeconds", Number]],
  ["--stacks", ["stacks", list]],
  ["--json", ["json", absolute]],
  ["--verdicts", ["verdicts", absolute]],
  ["--render", ["render", absolute]],
  ["--sections", ["sections", absolute]],
]);

/** What each switch turns off. */
const SWITCHES = new Map([
  ["--no-shaped", "shaped"],
  ["--no-load", "load"],
]);

const defaultArgs = () => ({
  apps: ["todomvc", "rows", "store"],
  engines: ["chromium", "firefox", "webkit"],
  runs: 7,
  conditions: ["cold", "warm"],
  shaped: true,
  seed: 20261025,
  loadRate: 100,
  settleSeconds: 120,
  loadSeconds: 10,
  serverRuns: 5,
  load: true,
  stacks: null,
  json: null,
  verdicts: null,
});

export function parseArgs(argv) {
  const a = defaultArgs();
  for (let i = 0; i < argv.length; i++) {
    const k = argv[i];
    if (SWITCHES.has(k)) a[SWITCHES.get(k)] = false;
    else if (OPTIONS.has(k)) {
      const [key, read] = OPTIONS.get(k);
      a[key] = read(argv[++i]);
    } else throw new Error(`head-to-head: unknown argument ${k}`);
  }
  return a;
}

function git(...args) {
  try {
    return execFileSync("git", args, { cwd: REPO, encoding: "utf8" }).trim();
  } catch {
    return null;
  }
}

/** The source files a framework's build is held to be newer than. */
function frameworkSources(dir) {
  const listed = git("ls-files", "--", path.relative(REPO, dir)) ?? "";
  return listed
    .split("\n")
    .filter(Boolean)
    .map((f) => path.join(REPO, f))
    .filter((f) => fs.existsSync(f));
}

// --- one app: its servers, its pages, its servers under load -----------------------

/** What an app's measurement is made of, and where its results go. */
function planApp(appName, args, log) {
  const app = APPS[appName];
  const stackNames = Object.keys(app.stacks).filter((s) => !args.stacks || args.stacks.includes(s));
  const targets = stackNames.map((s) => target(appName, s));
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), `h2h-${appName}-`));
  return { appName, app, args, log, stackNames, targets, tmp, servers: {}, chromium: null, runs: [], browsers: {}, loads: [], idle: [], presses: [] };
}

const targetOf = (ctx, stack) => ctx.targets.find((x) => x.stack === stack);

const firstLine = (e) => String(e?.message ?? e).split("\n")[0];

function assertBuilt(t) {
  if (t.server.cmd === PW_SERVER && !fs.existsSync(PW_SERVER)) {
    throw new Error(`${PW_SERVER} is not built: cargo build --release -p pw-dev-server`);
  }
}

async function startServers(ctx) {
  for (const t of ctx.targets) {
    assertBuilt(t);
    ctx.servers[t.stack] = await startServer({ ...t.server, log: path.join(ctx.tmp, `${t.server.name}.log`) });
    ctx.log(`  started ${t.server.name} on ${t.server.port} (pid ${ctx.servers[t.stack].pid})`);
  }
}

/** The blocks of page runs: each engine and condition, then in Chromium alone the same under the shaping. */
function pageBlocks(args) {
  const unshaped = args.engines.flatMap((engine) => args.conditions.map((condition) => ({ engine, condition, network: null })));
  const shaped = args.shaped && args.engines.includes("chromium") ? args.conditions.map((condition) => ({ engine: "chromium", condition, network: SLOW_4G })) : [];
  return [...unshaped, ...shaped];
}

const blockLabel = (block) => `${block.engine} ${block.condition}${block.network ? ` ${block.network.name}` : ""}`;

/** One stack's one load and interactions in a block, kept; a run that failed is kept as one, with what failed: the rules refuse it (`validate.mjs`), and the rest is still reported. */
async function measureCell(ctx, browser, block, stack, i, order) {
  const cell = { app: ctx.appName, stack, engine: block.engine, condition: block.condition, network: block.network?.name ?? null, i, order: order.join(" ") };
  try {
    const r = await measureOnce({ browser, engine: block.engine, target: targetOf(ctx, stack), condition: block.condition, network: block.network });
    ctx.runs.push({ ...cell, ...r });
  } catch (e) {
    const error = firstLine(e);
    ctx.runs.push({ ...cell, error, requests: [], interactions: [], timings: {} });
    ctx.log(`  ${ctx.appName}: ${stack} ${blockLabel(block)} run ${i} FAILED: ${error}`);
  }
}

/** Every stack once, in an order shuffled from the seed and the round. */
async function measureRound(ctx, browser, block, blockIndex, i) {
  const order = shuffled(ctx.stackNames, ctx.args.seed + i * 7919 + blockIndex * 104729);
  for (const stack of order) await measureCell(ctx, browser, block, stack, i, order);
}

async function measureBlock(ctx, block, blockIndex, launchers) {
  const browser = await launchers[block.engine].launch();
  ctx.browsers[block.engine] = browser.version();
  try {
    for (let i = 0; i < ctx.args.runs; i++) await measureRound(ctx, browser, block, blockIndex, i);
  } finally {
    await browser.close();
  }
  ctx.log(`  ${ctx.appName}: ${blockLabel(block)}: ${ctx.args.runs} rounds of ${ctx.stackNames.length}`);
}

async function measurePages(ctx) {
  const { chromium, firefox, webkit } = playwright();
  ctx.chromium = chromium;
  const launchers = { chromium, firefox, webkit };
  for (const [index, block] of pageBlocks(ctx.args).entries()) await measureBlock(ctx, block, index, launchers);
}

// Each server measurement `serverRuns` times, as the pages' are: every
// stack once a round, in an order shuffled from the seed and the round
// (kept with each run), so that each number is a median with its
// spread, never one run, and no stack is always measured first.
const rounds = (ctx, salt) => Array.from({ length: ctx.args.serverRuns }, (_, i) => shuffled(ctx.stackNames, ctx.args.seed + salt + i * 7919));

/** The cell a round's measurement of a stack is kept in. */
const cellOf = (ctx, stack, i, order) => ({ app: ctx.appName, stack, i, order: order.join(" ") });

/** One server measurement of a round, kept with its cell, or kept as the failure it was (which the rules refuse; the rest is reported). */
async function keepOrFail(ctx, list, cell, what, measure) {
  try {
    list.push({ ...cell, ...(await measure()) });
  } catch (e) {
    list.push({ ...cell, error: firstLine(e) });
    ctx.log(`  ${ctx.appName}: ${cell.stack}: ${what}, round ${cell.i}, FAILED: ${list.at(-1).error}`);
  }
}

/** Each stack's virtual visitors: the store's twenty sessions, TodoMVC's none. */
async function visitorsOf(ctx) {
  const visitors = {};
  for (const t of ctx.targets) visitors[t.stack] = ctx.appName === "store" ? await sessions(t.url, 20) : [""];
  return visitors;
}

async function loadRounds(ctx) {
  const visitors = await visitorsOf(ctx);
  for (const [i, order] of rounds(ctx, 15485863).entries()) {
    for (const stack of order) {
      const url = targetOf(ctx, stack).url;
      const { loadRate: rate, loadSeconds: seconds } = ctx.args;
      await keepOrFail(ctx, ctx.loads, cellOf(ctx, stack, i, order), "the fixed load", () =>
        load({ pid: ctx.servers[stack].pid, url, rate, seconds, cookies: visitors[stack] }),
      );
    }
  }
  ctx.log(`  ${ctx.appName}: ${ctx.args.serverRuns} rounds of ${ctx.stackNames.length} under ${ctx.args.loadRate}/s for ${ctx.args.loadSeconds} s`);
}

const readyCheck = (t) => (page) =>
  t.ready === "pw"
    ? page.waitForFunction(() => document.documentElement.dataset.pwReady === "1")
    : page.locator(t.appSelector).waitFor({ state: "attached" });

/** Whether a press has taken effect, in the page: the todos listed, or the cart's count. */
const pressChanged = ([cond, a]) => {
  if (cond === "todos") return document.querySelectorAll(".todo-list li").length === a;
  if (cond === "count") return document.querySelector("#cart-count")?.textContent.trim() === a;
  return false;
};

/** The app's first interaction, timed from the press to the change on the
 * page (the instrument's own clock: no page observers here), to the
 * change's polling, PRESS_POLL_MS. */
const pressOf = (t) => async (page) => {
  const step = t.interactions[0];
  const arg = step.before ? await step.before(page) : step.arg;
  const started = performance.now();
  await step.run(page);
  await page.waitForFunction(pressChanged, [step.cond, arg], { timeout: 30_000, polling: PRESS_POLL_MS });
  return performance.now() - started;
};

/** Twenty idle visitors, then a press with nineteen other tabs open, for a stack in a round. */
async function measureIdleAndPress(ctx, browser, stack, i, order) {
  const t = targetOf(ctx, stack);
  const cell = cellOf(ctx, stack, i, order);
  const pid = ctx.servers[stack].pid;
  await keepOrFail(ctx, ctx.idle, cell, "idle visitors", () => idleCost({ browser, url: t.url, pid, pages: 20, seconds: 10, ready: readyCheck(t) }));
  await keepOrFail(ctx, ctx.presses, cell, "a press with tabs open", () =>
    pressWithTabsOpen({ browser, url: t.url, tabs: 20, settleSeconds: 4, ready: readyCheck(t), press: pressOf(t), resolutionMs: PRESS_POLL_MS }),
  );
}

async function pressRounds(ctx) {
  const browser = await ctx.chromium.launch();
  try {
    for (const [i, order] of rounds(ctx, 32452843).entries()) {
      for (const stack of order) await measureIdleAndPress(ctx, browser, stack, i, order);
    }
    ctx.log(`  ${ctx.appName}: ${ctx.args.serverRuns} rounds of ${ctx.stackNames.length}: 20 idle visitors for 10 s, then a press with 19 other tabs open`);
  } finally {
    await browser.close();
  }
}

const isFramework = (t) => t.build.kind === "next" || t.build.kind === "sveltekit";

/** The sources of the frameworks' builds, each held to be older than its build. */
const sourcesOf = (targets) => Object.fromEntries(targets.filter(isFramework).map((t) => [t.stack, frameworkSources(t.build.dir)]));

/** Every rule of `validate.mjs`, the servers' too where they were measured. */
function problemsOf(ctx) {
  const { args } = ctx;
  const manifest = JSON.parse(fs.readFileSync(path.join(HERE, "todomvc/upstream.json"), "utf8"));
  const network = args.shaped ? SLOW_4G : null;
  const problems = validate({ runs: ctx.runs, targets: ctx.targets, network, minRuns: MIN_RUNS, manifest, sources: sourcesOf(ctx.targets) });
  if (args.load) {
    const { loads, idle, presses } = ctx;
    problems.push(...validateServer({ app: ctx.appName, stacks: ctx.stackNames, loads, idle, presses, minRuns: MIN_RUNS }));
  }
  return problems;
}

async function measureApp(appName, args, log) {
  const ctx = planApp(appName, args, log);
  try {
    await startServers(ctx);
    await measurePages(ctx);
    if (args.load) {
      await loadRounds(ctx);
      await pressRounds(ctx);
    }
  } finally {
    for (const s of Object.values(ctx.servers)) await s.stop();
  }
  const problems = problemsOf(ctx);
  const { loads, idle, presses, browsers } = ctx;
  return { app: appName, title: ctx.app.title, targets: ctx.targets.map(describe), runs: ctx.runs, loads, idle, presses, browsers, problems };
}

// --- before and after the measurement ------------------------------------------------

/** Waits, at most `seconds`, for the one-minute load average to fall below a
 * quarter of the logical CPUs; returns what it saw. */
export async function settle(seconds, log) {
  const goal = Math.max(2, os.cpus().length / 4);
  const before = os.loadavg()[0];
  const startedAt = Date.now();
  while (os.loadavg()[0] >= goal && Date.now() - startedAt < seconds * 1000) {
    await sleep(2000);
  }
  const out = {
    goal,
    before: Number(before.toFixed(2)),
    after: Number(os.loadavg()[0].toFixed(2)),
    waitedSeconds: Math.round((Date.now() - startedAt) / 1000),
  };
  log(`  settled: one-minute load ${out.before} -> ${out.after} in ${out.waitedSeconds} s (goal under ${goal})`);
  return out;
}

/**
 * What the recipe's earlier steps wrote (`just e14-head-to-head`): each
 * section's text, printed whole after the summary, and its data, which the
 * summary's rows read. A section that is not there is left out, and says so.
 */
export function readSections(dir) {
  if (!dir) return {};
  const json = (name) => {
    const f = path.join(dir, `${name}.json`);
    return fs.existsSync(f) ? JSON.parse(fs.readFileSync(f, "utf8")) : null;
  };
  const texts = {};
  for (const [file, title] of [
    ["behaviour.txt", "TodoMVC's behaviour on every stack (Cypress, and the specification in three engines)"],
    ["loc.txt", "the source each stack's developer writes (scripts/head_to_head_loc.py)"],
    ["feature.txt", "the same feature change in each stack (feature-change.mjs)"],
    ["museum.txt", "the museum's bug classes each stack lets ship (museum-classes.mjs)"],
  ]) {
    const f = path.join(dir, file);
    texts[title] = fs.existsSync(f) ? fs.readFileSync(f, "utf8") : "(not run)";
  }
  return {
    texts,
    behaviour: json("behaviour"),
    loc: json("loc"),
    feature: json("feature"),
    museum: json("museum"),
  };
}

export function describe(t) {
  return {
    stack: t.stack,
    label: t.label,
    who: t.who,
    version: t.version,
    url: t.url,
    ready: t.ready,
    expect: t.expect,
    standIn: t.standIn,
    common: t.common.map(String),
    server: {
      name: t.server.name,
      cmd: t.server.cmd === process.execPath ? "node" : path.relative(REPO, t.server.cmd),
      args: t.server.args.map((x) => x.replaceAll(`${REPO}/`, "")),
    },
  };
}

/** The run's start: when, at what commit and whether it is changed from it (as the
 * recipes judge it: the evidence a run writes, and the store's IR a build rewrites,
 * are not changes to what is measured), the machine, and the shaping applied, as
 * it was sent: the report prints this, never a copy of its numbers. */
function startMeta(args, settled) {
  return {
    started: new Date().toISOString(),
    commit: git("rev-parse", "HEAD"),
    dirty: git("status", "--porcelain", "--untracked-files=no", "--", ".", ":(exclude)docs/evidence", ":(exclude)spikes/own-renderer/store-ir.json")
      ? "uncommitted changes"
      : null,
    settled,
    machine: machine(),
    playwright: playwrightVersion(),
    args: { ...args, json: args.json ? path.relative(REPO, args.json) : null },
    minRuns: MIN_RUNS,
    network: args.shaped ? SLOW_4G : null,
  };
}

function finishMeta(meta) {
  meta.finished = new Date().toISOString();
  meta.loadAverageAfter = os.loadavg().map((l) => Number(l.toFixed(2)));
}

/** A kept run, reported again: no measurement. */
function renderKept(file, sections, verdicts) {
  const result = JSON.parse(fs.readFileSync(file, "utf8"));
  writeVerdicts(verdicts, result, sections);
  process.stdout.write(renderReport(result, sections));
}

/** **The rows' verdicts, as JSON** (`--verdicts`): the measured commit and
 * whether the tree was changed from it, the machine, the runs a cell holds,
 * the problems that refuse the run (none, or nothing in it is a
 * measurement), and each row Pleris is in with its values and verdict, as
 * the report judges it. pleris.dev reads this file, and builds from no run
 * that is refused or changed from its commit (train-15). */
export function verdictsOf(result, sections) {
  const { meta, apps } = result;
  return {
    commit: meta.commit,
    dirty: meta.dirty,
    started: meta.started,
    finished: meta.finished,
    machine: meta.machine.cpu,
    runs: meta.args.runs,
    serverRuns: meta.args.serverRuns,
    network: meta.network?.describe ?? null,
    problems: apps.flatMap((a) => a.problems),
    rows: verdictRows(result, sections),
  };
}

function writeVerdicts(file, result, sections) {
  if (file) fs.writeFileSync(file, `${JSON.stringify(verdictsOf(result, sections), null, 1)}\n`);
}

/** The verdict after the report: every rule held, or the problems that refuse it (exit 1). */
function verdict(apps) {
  const problems = apps.flatMap((a) => a.problems);
  if (!problems.length) {
    process.stdout.write("\nhead-to-head: every rule held (validate.mjs)\n");
    return;
  }
  process.stdout.write(`\nhead-to-head: REFUSED: ${problems.length} problem(s); nothing above is a measurement\n`);
  for (const p of problems.slice(0, 50)) process.stdout.write(`  ${p}\n`);
  process.exit(1);
}

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const sections = readSections(args.sections);
  if (args.render) return renderKept(args.render, sections, args.verdicts);
  const log = (line) => process.stderr.write(`${line}\n`);
  // The recipe's builds and checks just before leave the machine busy: wait
  // for its one-minute load to fall below a quarter of its CPUs, at most
  // `settleSeconds`, and say what the wait was.
  const settled = await settle(args.settleSeconds, log);
  const meta = startMeta(args, settled);
  const apps = [];
  for (const app of args.apps) {
    log(`head-to-head: ${app}`);
    apps.push(await measureApp(app, args, log));
  }
  finishMeta(meta);
  const result = { meta, apps };
  if (args.json) fs.writeFileSync(args.json, `${JSON.stringify(result, null, 1)}\n`);
  writeVerdicts(args.verdicts, result, sections);
  process.stdout.write(renderReport(result, sections));
  verdict(apps);
}

if (isMain(import.meta.url)) {
  main().catch((e) => {
    console.error(`head-to-head: ${e.stack ?? e}`);
    process.exit(1);
  });
}
