// The command line of the head-to-head and what it reads before it measures
// (head-to-head.mjs): its arguments, the wait for a quiet machine, the
// sections the recipe's earlier steps wrote, and how a target is described in
// the report. `measureApp` and `main`, which start servers and browsers, are
// the live measurement's and are not run here.
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { describe, parseArgs, readSections, settle, verdictsOf } from "../head-to-head.mjs";
import { REPO } from "../lib.mjs";

const DEFAULTS = {
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
  verdicts: null,
};

const parsed = (...argv) => parseArgs(argv);
const changed = (...argv) => ({ ...DEFAULTS, ...Object.fromEntries(argv) });

test("with no argument a run is both apps, three engines, cold and warm, seven runs, shaped, with servers measured, and keeps no file", () => {
  assert.deepEqual(parsed(), DEFAULTS);
  assert.deepEqual(Object.keys(parsed()), Object.keys(DEFAULTS));
});

test("each option takes the argument after it, and a list option splits it at commas", () => {
  assert.deepEqual(parsed("--apps", "store"), changed(["apps", ["store"]]));
  assert.deepEqual(parsed("--engines", "chromium,webkit"), changed(["engines", ["chromium", "webkit"]]));
  assert.deepEqual(parsed("--conditions", "warm"), changed(["conditions", ["warm"]]));
  assert.deepEqual(parsed("--stacks", "pleris,react,svelte"), changed(["stacks", ["pleris", "react", "svelte"]]));
});

test("a number option is the argument as a number, whatever it holds", () => {
  assert.deepEqual(parsed("--runs", "5"), changed(["runs", 5]));
  assert.deepEqual(parsed("--seed", "7"), changed(["seed", 7]));
  assert.deepEqual(parsed("--load-rate", "50"), changed(["loadRate", 50]));
  assert.deepEqual(parsed("--load-seconds", "2.5"), changed(["loadSeconds", 2.5]));
  assert.deepEqual(parsed("--server-runs", "3"), changed(["serverRuns", 3]));
  assert.deepEqual(parsed("--settle", "0"), changed(["settleSeconds", 0]));
  assert.deepEqual(parsed("--runs", "many"), changed(["runs", NaN]));
  assert.deepEqual(parsed("--runs", ""), changed(["runs", 0]));
});

test("the two switches turn off the shaped runs and the servers' measurements", () => {
  assert.deepEqual(parsed("--no-shaped"), changed(["shaped", false]));
  assert.deepEqual(parsed("--no-load"), changed(["load", false]));
  assert.deepEqual(parsed("--no-load", "--no-shaped"), changed(["load", false], ["shaped", false]));
});

test("the three file options are made absolute, and the last two exist only where given", () => {
  assert.deepEqual(parsed("--json", "out/runs.json"), changed(["json", path.resolve("out/runs.json")]));
  const both = parsed("--render", "kept.json", "--sections", "/tmp/sections");
  assert.deepEqual(both, { ...DEFAULTS, render: path.resolve("kept.json"), sections: "/tmp/sections" });
  assert.equal("render" in parsed(), false);
  assert.equal("sections" in parsed(), false);
});

test("options may come in any order, a later one replaces an earlier, and an empty argument is a list of one empty name", () => {
  assert.deepEqual(parsed("--runs", "3", "--no-load", "--runs", "9"), changed(["runs", 9], ["load", false]));
  assert.deepEqual(parsed("--apps", ""), changed(["apps", [""]]));
});

test("an argument it does not know is refused by name, whatever it looks like", () => {
  const unknown = ["--nope", "store", "-r", "--apps=store", "constructor", "__proto__", "toString", ""];
  unknown.forEach((arg) => assert.throws(() => parseArgs([arg]), { name: "Error", message: `head-to-head: unknown argument ${arg}` }));
});

test("an option's argument is taken whatever it is, so the argument after it is not read as an option", () => {
  assert.throws(() => parseArgs(["--runs", "--seed", "3"]), { message: "head-to-head: unknown argument 3" });
  assert.deepEqual(parsed("--apps", "--no-load"), changed(["apps", ["--no-load"]]));
});

test("an option with nothing after it is a number that is not one, or a failure to read it", () => {
  assert.deepEqual(parsed("--runs"), changed(["runs", NaN]));
  assert.throws(() => parseArgs(["--apps"]), TypeError);
  assert.throws(() => parseArgs(["--json"]), TypeError);
  assert.throws(() => parseArgs(["--render"]), TypeError);
});

// --- settle ---------------------------------------------------------------------

/** The machine's cores and its one-minute load, as the next calls will see them. */
function machineWith(t, cores, loads) {
  t.mock.method(os, "cpus", () => new Array(cores).fill({}));
  const left = [...loads];
  t.mock.method(os, "loadavg", () => [left.length > 1 ? left.shift() : left[0], 0, 0]);
}

/** A clock that moves only as `setTimeout` is asked to wait, and a log. */
function clockAndLog(t) {
  const clock = { now: 1_000_000, waits: [] };
  t.mock.method(Date, "now", () => clock.now);
  t.mock.method(globalThis, "setTimeout", (fn, ms) => {
    clock.now += ms;
    clock.waits.push(ms);
    fn();
  });
  const lines = [];
  return { clock, lines, log: (line) => lines.push(line) };
}

test("settle with no seconds to wait says what it saw and waits for nothing", async (t) => {
  machineWith(t, 32, [3.14159]);
  const { clock, lines, log } = clockAndLog(t);
  assert.deepEqual(await settle(0, log), { goal: 8, before: 3.14, after: 3.14, waitedSeconds: 0 });
  assert.deepEqual(lines, ["  settled: one-minute load 3.14 -> 3.14 in 0 s (goal under 8)"]);
  assert.deepEqual(clock.waits, []);
});

test("settle's goal is a quarter of the logical CPUs, and never under two", async (t) => {
  machineWith(t, 4, [1]);
  assert.equal((await settle(0, () => {})).goal, 2);
  machineWith(t, 16, [1]);
  assert.equal((await settle(0, () => {})).goal, 4);
});

test("settle waits two seconds at a time while the load is at the goal or above, until it falls below it", async (t) => {
  machineWith(t, 32, [21.2, 21.2, 12, 7.456]);
  const { clock, lines, log } = clockAndLog(t);
  assert.deepEqual(await settle(120, log), { goal: 8, before: 21.2, after: 7.46, waitedSeconds: 4 });
  assert.deepEqual(clock.waits, [2000, 2000]);
  assert.deepEqual(lines, ["  settled: one-minute load 21.2 -> 7.46 in 4 s (goal under 8)"]);
});

test("settle gives up after the seconds it was given, the load still high", async (t) => {
  machineWith(t, 32, [30]);
  const { clock, lines, log } = clockAndLog(t);
  assert.deepEqual(await settle(5, log), { goal: 8, before: 30, after: 30, waitedSeconds: 6 });
  assert.deepEqual(clock.waits, [2000, 2000, 2000]);
  assert.deepEqual(lines, ["  settled: one-minute load 30 -> 30 in 6 s (goal under 8)"]);
});

test("settle at a load exactly the goal still waits, for the goal is a load to fall under", async (t) => {
  machineWith(t, 32, [8, 8, 7.99]);
  const { clock, log } = clockAndLog(t);
  assert.deepEqual(await settle(60, log), { goal: 8, before: 8, after: 7.99, waitedSeconds: 2 });
  assert.deepEqual(clock.waits, [2000]);
});

// --- readSections -----------------------------------------------------------------

const SECTION_TITLES = [
  ["behaviour.txt", "TodoMVC's behaviour on every stack (Cypress, and the specification in three engines)"],
  ["loc.txt", "the source each stack's developer writes (scripts/head_to_head_loc.py)"],
  ["feature.txt", "the same feature change in each stack (feature-change.mjs)"],
  ["museum.txt", "the museum's bug classes each stack lets ship (museum-classes.mjs)"],
];

function inTemp(fn) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "h2h-sections-test-"));
  try {
    return fn(dir);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
}

test("with no directory there are no sections", () => {
  assert.deepEqual(readSections(undefined), {});
  assert.deepEqual(readSections(null), {});
  assert.deepEqual(readSections(""), {});
});

test("in a directory with nothing in it every section's text is '(not run)' and its data is null", () => {
  inTemp((dir) => {
    const want = { texts: Object.fromEntries(SECTION_TITLES.map(([, title]) => [title, "(not run)"])), behaviour: null, loc: null, feature: null, museum: null };
    assert.deepEqual(readSections(dir), want);
    assert.deepEqual(Object.keys(readSections(dir).texts), SECTION_TITLES.map(([, title]) => title));
  });
});

test("each section is its text file whole and its data file parsed, wherever the directory has them", () => {
  inTemp((dir) => {
    fs.writeFileSync(path.join(dir, "loc.txt"), "app lines\n  7\n");
    fs.writeFileSync(path.join(dir, "loc.json"), '{"counts":{"store":{}}}');
    fs.writeFileSync(path.join(dir, "museum.json"), '{"exhibits":[]}');
    const got = readSections(dir);
    assert.equal(got.texts[SECTION_TITLES[1][1]], "app lines\n  7\n");
    assert.equal(got.texts[SECTION_TITLES[0][1]], "(not run)");
    assert.deepEqual([got.behaviour, got.loc, got.feature, got.museum], [null, { counts: { store: {} } }, null, { exhibits: [] }]);
  });
});

test("a data file that is not JSON is a failure to read it, not a section that is not there", () => {
  inTemp((dir) => {
    fs.writeFileSync(path.join(dir, "feature.json"), "{ not json");
    assert.throws(() => readSections(dir), SyntaxError);
  });
});

// --- describe -----------------------------------------------------------------------

const target = (over = {}) => ({
  stack: "react",
  label: "React",
  who: "TodoMVC's maintainers",
  version: "React 19.2.5",
  url: "http://localhost:13662/react/dist/",
  ready: "dom",
  expect: { minifies: true, compresses: "all", caches: true },
  standIn: true,
  common: [/\/base\.js$/, /\/learn\.json$/],
  server: { name: "static-react", cmd: process.execPath, args: [path.join(REPO, "benchmarks/headtohead/static-host.mjs"), "--port", "13662"], cwd: REPO, port: 13662 },
  ...over,
});

test("a target is described by what the report prints of it: the node of the host as 'node', its paths from the repository's root, its patterns as text", () => {
  assert.deepEqual(describe(target()), {
    stack: "react",
    label: "React",
    who: "TodoMVC's maintainers",
    version: "React 19.2.5",
    url: "http://localhost:13662/react/dist/",
    ready: "dom",
    expect: { minifies: true, compresses: "all", caches: true },
    standIn: true,
    common: ["/\\/base\\.js$/", "/\\/learn\\.json$/"],
    server: { name: "static-react", cmd: "node", args: ["benchmarks/headtohead/static-host.mjs", "--port", "13662"] },
  });
});

test("a target's host that is not node is shown by its path from the repository's root, its arguments with the root taken off", () => {
  const host = path.join(REPO, "target/release/pw-dev-server");
  const t = target({ common: [], server: { name: "pleris-store", cmd: host, args: ["dist-baseline", `${REPO}/spikes/x`, "a/b"], cwd: REPO, port: 1 } });
  const got = describe(t);
  assert.deepEqual(got.server, { name: "pleris-store", cmd: "target/release/pw-dev-server", args: ["dist-baseline", "spikes/x", "a/b"] });
  assert.deepEqual(got.common, []);
});

test("a description holds nothing else of the target: not its build, its steps or its working directory", () => {
  assert.deepEqual(Object.keys(describe(target())), ["stack", "label", "who", "version", "url", "ready", "expect", "standIn", "common", "server"]);
});

test("the verdicts pleris.dev reads carry the run's problems, and whether its tree was changed from its commit", () => {
  const meta = {
    commit: "c0ffee",
    dirty: "uncommitted changes",
    started: "2026-11-08T00:00:00.000Z",
    finished: "2026-11-08T00:30:00.000Z",
    machine: { cpu: "a CPU" },
    args: { runs: 7, serverRuns: 5 },
    network: null,
  };
  const app = { app: "todomvc", title: "TodoMVC", targets: [], runs: [], loads: [], idle: [], presses: [], browsers: {}, problems: ["a cell of one run"] };
  const out = verdictsOf({ meta, apps: [app] }, {});
  assert.deepEqual(out.problems, ["a cell of one run"]);
  assert.equal(out.dirty, "uncommitted changes");
  assert.deepEqual([out.commit, out.machine, out.runs, out.serverRuns, out.network, out.rows], ["c0ffee", "a CPU", 7, 5, null, []]);
});
