#!/usr/bin/env node
// The head-to-head (ADR-0392): the same apps in Pleris and in the mainstream
// stacks, measured by one instrument, Pleris's losses first.
//
//   node benchmarks/headtohead/head-to-head.mjs [--apps todomvc,store]
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
import { HERE, REPO, machine, playwright, playwrightVersion, shuffled, startServer, summary } from "./lib.mjs";
import { SLOW_4G, measureOnce } from "./measure.mjs";
import { APPS, PW_SERVER, target } from "./targets.mjs";
import { MIN_RUNS, validate, validateServer } from "./validate.mjs";
import { lossesFirst, renderReport } from "./report.mjs";

/** How often a press's change is looked for: what its time can tell apart. */
const PRESS_POLL_MS = 10;

export function parseArgs(argv) {
  const a = {
    apps: ["todomvc", "store"],
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
  };
  for (let i = 0; i < argv.length; i++) {
    const k = argv[i];
    const v = () => argv[++i];
    if (k === "--apps") a.apps = v().split(",");
    else if (k === "--engines") a.engines = v().split(",");
    else if (k === "--runs") a.runs = Number(v());
    else if (k === "--conditions") a.conditions = v().split(",");
    else if (k === "--no-shaped") a.shaped = false;
    else if (k === "--seed") a.seed = Number(v());
    else if (k === "--load-rate") a.loadRate = Number(v());
    else if (k === "--load-seconds") a.loadSeconds = Number(v());
    else if (k === "--server-runs") a.serverRuns = Number(v());
    else if (k === "--no-load") a.load = false;
    else if (k === "--settle") a.settleSeconds = Number(v());
    else if (k === "--stacks") a.stacks = v().split(",");
    else if (k === "--json") a.json = path.resolve(v());
    else if (k === "--render") a.render = path.resolve(v());
    else if (k === "--sections") a.sections = path.resolve(v());
    else throw new Error(`head-to-head: unknown argument ${k}`);
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

async function measureApp(appName, args, log) {
  const app = APPS[appName];
  const stackNames = Object.keys(app.stacks).filter((s) => !args.stacks || args.stacks.includes(s));
  const targets = stackNames.map((s) => target(appName, s));
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), `h2h-${appName}-`));
  const servers = {};
  const runs = [];
  const browsers = {};
  const loads = [];
  const idle = [];
  const presses = [];
  try {
    for (const t of targets) {
      if (t.server.cmd === PW_SERVER && !fs.existsSync(PW_SERVER)) {
        throw new Error(`${PW_SERVER} is not built: cargo build --release -p pw-dev-server`);
      }
      servers[t.stack] = await startServer({ ...t.server, log: path.join(tmp, `${t.server.name}.log`) });
      log(`  started ${t.server.name} on ${t.server.port} (pid ${servers[t.stack].pid})`);
    }
    const blocks = [];
    for (const engine of args.engines) {
      for (const condition of args.conditions) blocks.push({ engine, condition, network: null });
    }
    if (args.shaped && args.engines.includes("chromium")) {
      for (const condition of args.conditions) blocks.push({ engine: "chromium", condition, network: SLOW_4G });
    }
    const { chromium, firefox, webkit } = playwright();
    const launchers = { chromium, firefox, webkit };
    for (const block of blocks) {
      const browser = await launchers[block.engine].launch();
      browsers[block.engine] = browser.version();
      try {
        for (let i = 0; i < args.runs; i++) {
          const order = shuffled(stackNames, args.seed + i * 7919 + blocks.indexOf(block) * 104729);
          for (const stack of order) {
            const t = targets.find((x) => x.stack === stack);
            const cell = { app: appName, stack, engine: block.engine, condition: block.condition, network: block.network?.name ?? null, i, order: order.join(" ") };
            try {
              const r = await measureOnce({ browser, engine: block.engine, target: t, condition: block.condition, network: block.network });
              runs.push({ ...cell, ...r });
            } catch (e) {
              // A run that failed is kept as one, with what failed: the rules
              // refuse it (`validate.mjs`), and the rest is still reported.
              const error = String(e?.message ?? e).split("\n")[0];
              runs.push({ ...cell, error, requests: [], interactions: [], timings: {} });
              log(`  ${appName}: ${stack} ${block.engine} ${block.condition}${block.network ? ` ${block.network.name}` : ""} run ${i} FAILED: ${error}`);
            }
          }
        }
      } finally {
        await browser.close();
      }
      log(`  ${appName}: ${block.engine} ${block.condition}${block.network ? ` ${block.network.name}` : ""}: ${args.runs} rounds of ${stackNames.length}`);
    }
    if (args.load) {
      // Each server measurement `serverRuns` times, as the pages' are: every
      // stack once a round, in an order shuffled from the seed and the round
      // (kept with each run), so that each number is a median with its
      // spread, never one run, and no stack is always measured first.
      const rounds = (salt) => Array.from({ length: args.serverRuns }, (_, i) => shuffled(stackNames, args.seed + salt + i * 7919));
      const visitors = {};
      for (const t of targets) visitors[t.stack] = appName === "store" ? await sessions(t.url, 20) : [""];
      for (const [i, order] of rounds(15485863).entries()) {
        for (const stack of order) {
          const t = targets.find((x) => x.stack === stack);
          const cell = { app: appName, stack, i, order: order.join(" ") };
          try {
            loads.push({ ...cell, ...(await load({ pid: servers[stack].pid, url: t.url, rate: args.loadRate, seconds: args.loadSeconds, cookies: visitors[stack] })) });
          } catch (e) {
            // Kept as a failure, which the rules refuse; the rest is reported.
            loads.push({ ...cell, error: String(e?.message ?? e).split("\n")[0] });
            log(`  ${appName}: ${stack}: the fixed load, round ${i}, FAILED: ${loads.at(-1).error}`);
          }
        }
      }
      log(`  ${appName}: ${args.serverRuns} rounds of ${stackNames.length} under ${args.loadRate}/s for ${args.loadSeconds} s`);
      const browser = await chromium.launch();
      try {
        for (const [i, order] of rounds(32452843).entries()) {
          for (const stack of order) {
            const t = targets.find((x) => x.stack === stack);
            const cell = { app: appName, stack, i, order: order.join(" ") };
            const ready = (page) =>
              t.ready === "pw"
                ? page.waitForFunction(() => document.documentElement.dataset.pwReady === "1")
                : page.locator(t.appSelector).waitFor({ state: "attached" });
            // The app's first interaction, timed from the press to the change
            // on the page (the instrument's own clock: no page observers here),
            // to the change's polling, PRESS_POLL_MS.
            const step = t.interactions[0];
            const press = async (page) => {
              const arg = step.before ? await step.before(page) : step.arg;
              const started = performance.now();
              await step.run(page);
              await page.waitForFunction(
                ([cond, a]) => {
                  if (cond === "todos") return document.querySelectorAll(".todo-list li").length === a;
                  if (cond === "count") return document.querySelector("#cart-count")?.textContent.trim() === a;
                  return false;
                },
                [step.cond, arg],
                { timeout: 30_000, polling: PRESS_POLL_MS },
              );
              return performance.now() - started;
            };
            try {
              idle.push({ ...cell, ...(await idleCost({ browser, url: t.url, pid: servers[stack].pid, pages: 20, seconds: 10, ready })) });
            } catch (e) {
              idle.push({ ...cell, error: String(e?.message ?? e).split("\n")[0] });
              log(`  ${appName}: ${stack}: idle visitors, round ${i}, FAILED: ${idle.at(-1).error}`);
            }
            try {
              presses.push({ ...cell, ...(await pressWithTabsOpen({ browser, url: t.url, tabs: 20, settleSeconds: 4, ready, press, resolutionMs: PRESS_POLL_MS })) });
            } catch (e) {
              presses.push({ ...cell, error: String(e?.message ?? e).split("\n")[0] });
              log(`  ${appName}: ${stack}: a press with tabs open, round ${i}, FAILED: ${presses.at(-1).error}`);
            }
          }
        }
        log(`  ${appName}: ${args.serverRuns} rounds of ${stackNames.length}: 20 idle visitors for 10 s, then a press with 19 other tabs open`);
      } finally {
        await browser.close();
      }
    }
  } finally {
    for (const s of Object.values(servers)) await s.stop();
  }
  const manifest = JSON.parse(fs.readFileSync(path.join(HERE, "todomvc/upstream.json"), "utf8"));
  const sources = {};
  for (const t of targets) {
    if (t.build.kind === "next" || t.build.kind === "sveltekit") {
      sources[t.stack] = frameworkSources(t.build.dir);
    }
  }
  const problems = validate({ runs, targets, network: args.shaped ? SLOW_4G : null, minRuns: MIN_RUNS, manifest, sources });
  if (args.load) problems.push(...validateServer({ app: appName, stacks: stackNames, loads, idle, presses, minRuns: MIN_RUNS }));
  return { app: appName, title: app.title, targets: targets.map(describe), runs, loads, idle, presses, browsers, problems };
}

/** Waits, at most `seconds`, for the one-minute load average to fall below a
 * quarter of the logical CPUs; returns what it saw. */
export async function settle(seconds, log) {
  const goal = Math.max(2, os.cpus().length / 4);
  const before = os.loadavg()[0];
  const startedAt = Date.now();
  while (os.loadavg()[0] >= goal && Date.now() - startedAt < seconds * 1000) {
    await new Promise((r) => setTimeout(r, 2000));
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

async function main() {
  const args = parseArgs(process.argv.slice(2));
  const sections = readSections(args.sections);
  if (args.render) {
    // A kept run, reported again: no measurement.
    process.stdout.write(renderReport(JSON.parse(fs.readFileSync(args.render, "utf8")), sections));
    return;
  }
  const log = (line) => process.stderr.write(`${line}\n`);
  // The recipe's builds and checks just before leave the machine busy: wait
  // for its one-minute load to fall below a quarter of its CPUs, at most
  // `settleSeconds`, and say what the wait was.
  const settled = await settle(args.settleSeconds, log);
  const started = new Date().toISOString();
  const meta = {
    started,
    commit: git("rev-parse", "HEAD"),
    // As the recipes judge it: the evidence a run writes, and the store's IR
    // a build rewrites, are not changes to what is measured.
    dirty: git("status", "--porcelain", "--untracked-files=no", "--", ".", ":(exclude)docs/evidence", ":(exclude)spikes/own-renderer/store-ir.json")
      ? "uncommitted changes"
      : null,
    settled,
    machine: machine(),
    playwright: playwrightVersion(),
    args: { ...args, json: args.json ? path.relative(REPO, args.json) : null },
    minRuns: MIN_RUNS,
    // The shaping applied, as it was sent: the report prints this, never a
    // copy of its numbers.
    network: args.shaped ? SLOW_4G : null,
  };
  const apps = [];
  for (const app of args.apps) {
    log(`head-to-head: ${app}`);
    apps.push(await measureApp(app, args, log));
  }
  meta.finished = new Date().toISOString();
  meta.loadAverageAfter = os.loadavg().map((l) => Number(l.toFixed(2)));
  const result = { meta, apps };
  if (args.json) fs.writeFileSync(args.json, `${JSON.stringify(result, null, 1)}\n`);
  process.stdout.write(renderReport(result, sections));
  const problems = apps.flatMap((a) => a.problems);
  if (problems.length) {
    process.stdout.write(`\nhead-to-head: REFUSED: ${problems.length} problem(s); nothing above is a measurement\n`);
    for (const p of problems.slice(0, 50)) process.stdout.write(`  ${p}\n`);
    process.exit(1);
  }
  process.stdout.write("\nhead-to-head: every rule held (validate.mjs)\n");
}

export { lossesFirst, summary };

if (process.argv[1] && path.resolve(process.argv[1]) === path.resolve(new URL(import.meta.url).pathname)) {
  main().catch((e) => {
    console.error(`head-to-head: ${e.stack ?? e}`);
    process.exit(1);
  });
}
