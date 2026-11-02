// The head-to-head's small helpers (lib.mjs), pinned as they are, so that the
// file can be split into smaller functions without changing what it does: the
// paths and Playwright, the statistics, sizes and blobs, the minified verdict,
// the machine, /proc's processes and their cost, and the servers a run starts
// and stops. Each test asserts a whole value. What a process cannot be made to
// do for real (a /proc that lists a chosen tree, an operating system that
// answers as told, five seconds passing, esbuild missing) is made to happen
// with a fake, said where.
import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import { once } from "node:events";
import fs from "node:fs";
import http from "node:http";
import { createRequire } from "node:module";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { setTimeout as delay } from "node:timers/promises";
import { fileURLToPath, pathToFileURL } from "node:url";
import { brotliCompressSync, constants, gzipSync } from "node:zlib";
import {
  BENCH,
  HARNESS,
  HERE,
  MINIFIED_RULE,
  PAGE_KIB,
  REPO,
  TICK_MS,
  gitBlob,
  machine,
  minifiedVerdict,
  playwright,
  playwrightVersion,
  portFree,
  processTree,
  quantile,
  sha256,
  shuffled,
  sizes,
  startServer,
  summary,
  treeCost,
  waitForHttp,
} from "../lib.mjs";

const TESTS = path.dirname(fileURLToPath(import.meta.url));
// Real files, copied from benchmarks/headtohead/todomvc so that the numbers
// below do not move with that tree (as .txt, which the code checks leave
// alone): todomvc-common.js.txt is Svelte's build's base.js
// (svelte/dist/base.js, untouched upstream, indented and commented);
// todomvc-common.min.js.txt is React's build's (react/dist/base.js, minified);
// todomvc-app.css.txt is the app's stylesheet source (react/src/todo/app.css)
// and todomvc-app.min.css.txt its build (react/dist/app.css, minified).
const fixture = (name) => fs.readFileSync(path.join(TESTS, "fixtures/lib", name));
const realKill = process.kill;
const NEEDS_PROC = { skip: !fs.existsSync(`/proc/self/task/${process.pid}/children`) && "needs /proc with the tasks' children" };

// --- helpers ------------------------------------------------------------------

async function inTemp(fn) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "h2h-lib-"));
  try {
    return await fn(dir);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
}

const listen = (server, host = "127.0.0.1") => new Promise((resolve) => server.listen(0, host, () => resolve(server.address().port)));

const shut = (server) =>
  new Promise((resolve) => {
    server.closeAllConnections?.();
    server.close(resolve);
  });

/** A port nothing listens on: asked of the kernel and given back. */
async function freePort() {
  const server = net.createServer();
  const port = await listen(server);
  await shut(server);
  return port;
}

/** A loopback HTTP server that notes each request line; `handler` answers. */
async function withHttp(handler, fn) {
  const seen = [];
  const server = http.createServer((req, res) => {
    seen.push(`${req.method} ${req.url}`);
    handler(req, res, seen.length);
  });
  const base = `http://127.0.0.1:${await listen(server)}`;
  try {
    return await fn({ base, seen });
  } finally {
    await shut(server);
  }
}

async function until(condition, ms = 3000) {
  const end = performance.now() + ms;
  while (!condition() && performance.now() < end) await delay(10);
  return condition();
}

/** Whether a process is there and not a zombie (one nobody has reaped is dead). */
function running(pid) {
  try {
    const stat = fs.readFileSync(`/proc/${pid}/stat`, "utf8");
    return stat.charAt(stat.lastIndexOf(")") + 2) !== "Z";
  } catch {
    return false;
  }
}

function serve(entry) {
  if (entry instanceof Error) throw entry;
  return entry;
}

/** Answer these reads of the kernel's /proc from a table (a path to its text, to
 * a directory's names, or to an Error to throw); any other path is read for
 * real. The table's pids are above any pid_max (4,194,304): none exists. */
function fakeProc(t, files) {
  ["readdirSync", "readFileSync"].forEach((name) => {
    const real = fs[name];
    t.mock.method(fs, name, (p, ...rest) => (Object.hasOwn(files, p) ? serve(files[p]) : real.call(fs, p, ...rest)));
  });
}

const stat = (pid, utime, stime, comm = "node") => `${pid} (${comm}) S 1 1 1 0 -1 4194560 100 0 0 0 ${utime} ${stime} 0 0 20 0 1 0 100 1000000 100 18446744073709551615\n`;
const statm = (pages) => `${pages * 4} ${pages} 10 5 0 100 0\n`;

// --- the paths and Playwright --------------------------------------------------

test("`HERE`, `BENCH`, `REPO` and `HARNESS` are the instrument's directory, benchmarks, the repository's root and benchmarks/harness", () => {
  assert.equal(HERE, path.resolve(TESTS, ".."));
  assert.equal(BENCH, path.resolve(TESTS, "../.."));
  assert.equal(REPO, path.resolve(TESTS, "../../.."));
  assert.equal(HARNESS, path.resolve(TESTS, "../../harness"));
  assert.equal(path.basename(HERE), "headtohead");
  assert.equal(fs.existsSync(path.join(HERE, "lib.mjs")), true);
  assert.equal(fs.existsSync(path.join(REPO, "PROJECT_CHARTER.md")), true);
  assert.equal(fs.statSync(HARNESS).isDirectory(), true);
});

test("`playwright()` is the Playwright test module of this package with its three browser types, the same one each time", () => {
  const pw = playwright();
  assert.deepEqual([pw.chromium.name(), pw.firefox.name(), pw.webkit.name()], ["chromium", "firefox", "webkit"]);
  assert.equal(typeof pw.test, "function");
  assert.equal(playwright(), pw);
});

test("`playwrightVersion()` is the version package.json pins for @playwright/test", () => {
  const pinned = JSON.parse(fs.readFileSync(path.join(HERE, "package.json"), "utf8")).devDependencies["@playwright/test"];
  assert.equal(playwrightVersion(), pinned);
});

// --- statistics ----------------------------------------------------------------

const QUANTILES = [
  [[], 0.5, null],
  [[5], 0, 5],
  [[5], 0.37, 5],
  [[5], 1, 5],
  [[1, 2, 3, 4], 0, 1],
  [[1, 2, 3, 4], 1, 4],
  [[1, 2, 3, 4], 0.5, 2.5],
  [[1, 2, 3, 4], 0.25, 1.75],
  [[1, 2, 3, 4], 0.75, 3.25],
  [[1, 2, 4], 0.75, 3],
  [[2, 4], 0.5, 3],
  [[0, 10], 0.3, 3],
];

test("`quantile` is null of nothing, the element at 0 and 1, and linear between its neighbours elsewhere", () => {
  QUANTILES.forEach(([sorted, q, expected]) => assert.equal(quantile(sorted, q), expected, `quantile(${JSON.stringify(sorted)}, ${q})`));
});

const NOTHING = { n: 0, missing: 0, median: null, q1: null, q3: null, min: null, max: null };

test("`summary` of no values counts nothing and gives null for every statistic", () => {
  assert.deepEqual(summary([]), NOTHING);
});

test("`summary` leaves out what is not a finite number (null, text, NaN, infinities, booleans, objects) and counts it as missing", () => {
  assert.deepEqual(summary([null, null, null]), { ...NOTHING, missing: 3 });
  assert.deepEqual(summary(["a", undefined]), { ...NOTHING, missing: 2 });
  assert.deepEqual(summary(["1", NaN, Infinity, -Infinity, undefined, 5]), { n: 1, missing: 5, median: 5, q1: 5, q3: 5, min: 5, max: 5 });
  assert.deepEqual(summary([true, {}, [], 7]), { n: 1, missing: 3, median: 7, q1: 7, q3: 7, min: 7, max: 7 });
});

test("`summary` sorts by value and not as text, takes any order, negative and fractional values, one value, and leaves its input alone", () => {
  assert.deepEqual(summary([7]), { n: 1, missing: 0, median: 7, q1: 7, q3: 7, min: 7, max: 7 });
  const input = [10, 9, 100, 1];
  assert.deepEqual(summary(input), { n: 4, missing: 0, median: 9.5, q1: 7, q3: 32.5, min: 1, max: 100 });
  assert.deepEqual(input, [10, 9, 100, 1]);
  assert.deepEqual(summary([-3.5, 2, 0, 1.25]), { n: 4, missing: 0, median: 0.625, q1: -0.875, q3: 1.4375, min: -3.5, max: 2 });
});

const SIX = ["a", "b", "c", "d", "e", "f"];
const SHUFFLES = [
  [0, ["f", "c", "d", "e", "a", "b"]],
  [1, ["e", "b", "f", "c", "a", "d"]],
  [42, ["b", "a", "e", "f", "c", "d"]],
  [1234, ["b", "f", "c", "e", "d", "a"]],
];

test("`shuffled` gives the golden order of each seed (Mulberry32 draws, Fisher-Yates swaps) on six names", () => {
  SHUFFLES.forEach(([seed, order]) => assert.deepEqual(shuffled(SIX, seed), order, `seed ${seed}`));
});

// A seed is taken as an unsigned 32-bit integer: `seed >>> 0`.
const SEED_ALIASES = [
  [2 ** 32, 0],
  [2 ** 32 + 1, 1],
  [2 ** 32 + 42, 42],
  [1.5, 1],
  [-1, 2 ** 32 - 1],
  [NaN, 0],
  [undefined, 0],
];

test("`shuffled` reads its seed as an unsigned 32-bit integer: 2**32 is 0, a fraction is cut, a negative wraps, no number is 0", () => {
  SEED_ALIASES.forEach(([seed, same]) => assert.deepEqual(shuffled(SIX, seed), shuffled(SIX, same), `seed ${seed}`));
  assert.deepEqual(shuffled(SIX, 2 ** 32 - 1), ["e", "b", "d", "c", "a", "f"]);
  assert.deepEqual(shuffled(SIX, 123456789012), ["e", "a", "b", "f", "d", "c"]);
});

test("`shuffled` returns a new array with every element once, whatever the seed, and leaves its list as it was", () => {
  const list = [...SIX];
  const out = shuffled(list, 7);
  assert.notEqual(out, list);
  assert.deepEqual(list, SIX);
  const spread = Array.from({ length: 64 }, (_, seed) => [...shuffled(list, seed)].sort().join(""));
  assert.deepEqual(new Set(spread), new Set(["abcdef"]));
  const things = [{}, {}, {}];
  assert.equal(shuffled(things, 9).every((thing) => things.includes(thing)), true);
});

test("`shuffled` of no element, one, and two elements (seed 0 swaps them, seed 1 does not)", () => {
  assert.deepEqual(shuffled([], 3), []);
  assert.deepEqual(shuffled(["x"], 3), ["x"]);
  assert.deepEqual(shuffled(["x", "y"], 0), ["y", "x"]);
  assert.deepEqual(shuffled(["x", "y"], 1), ["x", "y"]);
});

// --- sizes and blobs -------------------------------------------------------------

const REGULAR = Buffer.from("function f(){return 1}\n".repeat(400));

test("`sizes` are the length and the Brotli 11 and gzip 9 sizes of a body: none, a regular one, and a real script", () => {
  assert.deepEqual(sizes(Buffer.alloc(0)), { raw: 0, br: 1, gzip: 20 });
  assert.deepEqual(sizes(REGULAR), { raw: 9200, br: 29, gzip: 86 });
  // A real script's compressed sizes can differ by a byte from one Brotli or
  // zlib build to the next: they are asked of zlib here with the parameters
  // spelled out, and told from the next quality down (Brotli 10, gzip 6).
  const script = fixture("todomvc-common.js.txt");
  const brotli = (quality) => brotliCompressSync(script, { params: { [constants.BROTLI_PARAM_QUALITY]: quality, [constants.BROTLI_PARAM_SIZE_HINT]: script.length } }).length;
  const got = sizes(script);
  assert.deepEqual(got, { raw: 7253, br: brotli(11), gzip: gzipSync(script, { level: 9 }).length });
  assert.notEqual(got.br, brotli(10));
  assert.notEqual(got.gzip, gzipSync(script, { level: 6 }).length);
});

const BLOBS = [
  [Buffer.alloc(0), "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391"],
  [Buffer.from("hello\n"), "ce013625030ba8dba906f756967f9e9ca394464a"],
  [Buffer.from("héllo\n"), "5fb50d3c93474f139362304b663fe44e9d17a26e"],
  [Buffer.from([0, 255, 128, 10, 13]), "63704f70f22024b8a0dff17a9449437b35a4807f"],
  [fixture("todomvc-common.js.txt"), "a56b5aaca980fb608faf3eb9624750c8740b0025"],
];

const DIGESTS = [
  [Buffer.alloc(0), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"],
  [Buffer.from("abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"],
  [Buffer.from("hello\n"), "5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03"],
  [Buffer.from("héllo\n"), "b95becd154aa095f76c4ca47a5aeb8350d6dfcb838404edfc9dae06628de938d"],
  [Buffer.from([0, 255, 128, 10, 13]), "037e68ac6512d2c57a0d7247a76b3ab486a7f7e9c05ea50cae197782c80b63d4"],
  [fixture("todomvc-common.js.txt"), "84090789021f5f8206800503e18adbc38b5fddbfb0a84bfe36e86e683da60c20"],
];

test("`gitBlob` is `git hash-object` on the bytes, by their count and not their characters (vectors from git itself)", () => {
  BLOBS.forEach(([body, blob]) => assert.equal(gitBlob(body), blob, `${body.length} bytes`));
});

test("`sha256` is the SHA-256 of the bytes in hex (vectors from sha256sum)", () => {
  DIGESTS.forEach(([body, digest]) => assert.equal(sha256(body), digest, `${body.length} bytes`));
});

// --- minified or not -------------------------------------------------------------

const verdictOf = (fields) => ({ rule: MINIFIED_RULE, ...fields });
const SHARES = (minified, share, minifiedBytes) => verdictOf({ minified, share, minifiedBytes });

test("`MINIFIED_RULE` names esbuild's version, the 10% rule and the 1 KiB floor, and the version is the one installed", () => {
  assert.equal(MINIFIED_RULE, "minified: esbuild 0.28.2's minifier (whitespace, syntax and identifiers) takes less than 10% more off it; under 1 KiB counts as minified");
  assert.equal(createRequire(path.join(HERE, "package.json"))("esbuild/package.json").version, "0.28.2");
});

const SMALL = [
  ["", 0],
  ["var = ;", 7],
  ["function f() {\n  return 1;\n}\n".repeat(20), 580],
  ["a=1;" + " ".repeat(1019), 1023],
  ["é".repeat(500), 1000],
];

test("`minifiedVerdict` calls a script under 1 KiB minified without asking esbuild, even one that does not parse, counting bytes", () => {
  SMALL.forEach(([text, bytes]) => assert.deepEqual(minifiedVerdict(text), SHARES(true, null, bytes), `${bytes} bytes`));
  assert.deepEqual(minifiedVerdict(fixture("todomvc-app.css.txt").toString(), "text/css"), SHARES(true, null, 477));
});

test("`minifiedVerdict` asks esbuild from 1 KiB on, and by the text's bytes and not its characters", () => {
  assert.deepEqual(minifiedVerdict("a=1;" + " ".repeat(1020)), SHARES(false, 0.995, 5));
  const accented = `/* ${"é".repeat(600)} */\na{color:red}\n`;
  assert.equal(accented.length < 1024, true);
  assert.deepEqual(minifiedVerdict(accented, "text/css"), SHARES(false, 0.989, 13));
});

test("`minifiedVerdict` of real scripts: TodoMVC's common script as Svelte's build copies it is not minified, as React's build minifies it is", () => {
  assert.deepEqual(minifiedVerdict(fixture("todomvc-common.js.txt").toString()), SHARES(false, 0.48, 3770));
  assert.deepEqual(minifiedVerdict(fixture("todomvc-common.min.js.txt").toString()), SHARES(true, 0.003, 3742));
});

test("`minifiedVerdict` of real stylesheets: the app's build is minified, its source three times over (past 1 KiB) is not", () => {
  assert.deepEqual(minifiedVerdict(fixture("todomvc-app.min.css.txt").toString(), "text/css"), SHARES(true, 0.021, 7246));
  assert.deepEqual(minifiedVerdict(fixture("todomvc-app.css.txt").toString().repeat(3), "text/css"), SHARES(false, 0.806, 277));
});

const MINIFIED_JS = fixture("todomvc-common.min.js.txt").toString();
const LOADERS = [
  [undefined, SHARES(true, 0.003, 3742)],
  ["text/javascript", SHARES(true, 0.003, 3742)],
  ["application/javascript", SHARES(true, 0.003, 3742)],
  ["", SHARES(true, 0.003, 3742)],
  [null, SHARES(true, 0.003, 3742)],
  ["text/css", SHARES(true, 0.006, 3730)],
  ["text/css; charset=utf-8", SHARES(true, 0.006, 3730)],
  ["application/x-cssx", SHARES(true, 0.006, 3730)],
];

test("`minifiedVerdict` reads a type that names css as a stylesheet and any other (or none) as JavaScript", () => {
  LOADERS.forEach(([type, expected]) => assert.deepEqual(minifiedVerdict(MINIFIED_JS, type), expected, String(type)));
});

const BASE = `a="${"x".repeat(1800)}";`;

test("`minifiedVerdict` prints the share to three decimals but judges the share itself: 9.97% printed 0.1 is minified, 10.01% printed 0.1 is not", () => {
  assert.deepEqual(minifiedVerdict(BASE + " ".repeat(201)), SHARES(true, 0.1, 1806));
  assert.deepEqual(minifiedVerdict(BASE + " ".repeat(202)), SHARES(false, 0.1, 1806));
});

test("`minifiedVerdict` calls a script minified when esbuild's output is longer than it (a negative share)", () => {
  assert.deepEqual(minifiedVerdict(`var a="${"é".repeat(1100)}";`), SHARES(true, -0.996, 4410));
});

test("`minifiedVerdict` keeps a legal comment as esbuild is told to, and takes any other comment off", () => {
  const legal = `/*! license ${"x".repeat(1100)} */\na=1;`;
  assert.deepEqual(minifiedVerdict(legal), SHARES(true, 0, 1120));
  assert.deepEqual(minifiedVerdict(legal.replace("/*!", "/*")), SHARES(false, 0.996, 5));
});

test("`minifiedVerdict` of a script esbuild cannot parse is unknown, with the first line of esbuild's message", () => {
  const unknown = (error) => verdictOf({ minified: null, share: null, minifiedBytes: null, error });
  assert.deepEqual(minifiedVerdict("var = ;\n".repeat(200)), unknown("Error: Transform failed with 1 error:"));
  assert.deepEqual(minifiedVerdict("<p>hello</p>\n".repeat(100)), unknown("Error: Transform failed with 2 errors:"));
  assert.deepEqual(minifiedVerdict("a { color: red; }\n".repeat(100)), unknown("Error: Transform failed with 1 error:"));
});

function canRequire(require, name) {
  try {
    require.resolve(name);
    return true;
  } catch {
    return false;
  }
}

// The branch is reached by a copy of the modules in a directory above which no
// node_modules has esbuild: lib.mjs looks for it from its own directory.
test("`minifiedVerdict` of 1 KiB or more is unknown, naming the missing module, where esbuild is not installed", async (t) => {
  await inTemp(async (dir) => {
    copyModules(dir);
    if (canRequire(createRequire(path.join(dir, "package.json")), "esbuild")) return t.skip("esbuild resolves from the temp directory too");
    const lonely = await import(pathToFileURL(path.join(dir, "lib.mjs")).href);
    const expected = verdictOf({ minified: null, share: null, minifiedBytes: null, error: "esbuild is not installed" });
    assert.deepEqual(lonely.minifiedVerdict("a = 1;\n".repeat(200)), expected);
    assert.deepEqual(lonely.minifiedVerdict("a = 1;\n"), SHARES(true, null, 7));
  });
});

// --- the machine -----------------------------------------------------------------

test("`machine()` is seven keys: the CPU, its logical count, memory in GiB, OS, kernel, Node and the load average", () => {
  const m = machine();
  assert.deepEqual(Object.keys(m), ["cpu", "logicalCpus", "memoryGiB", "os", "kernel", "node", "loadAverage"]);
  assert.equal(m.cpu, os.cpus().map((c) => c.model.trim())[0] ?? "unknown");
  assert.equal(m.logicalCpus, os.cpus().length);
  assert.equal(m.memoryGiB, Number((os.totalmem() / 2 ** 30).toFixed(1)));
  assert.notEqual(m.os, "");
  assert.equal(m.kernel, os.release());
  assert.equal(m.node, process.version);
  assert.equal(m.loadAverage.length, 3);
  assert.deepEqual(m.loadAverage, m.loadAverage.map((load) => Number(load.toFixed(2))));
});

test("`machine()` names the OS as /etc/os-release's PRETTY_NAME, where there is one", { skip: !fs.existsSync("/etc/os-release") }, () => {
  const line = fs.readFileSync("/etc/os-release", "utf8").split("\n").find((l) => l.startsWith("PRETTY_NAME="));
  const expected = line ? line.slice("PRETTY_NAME=".length).replaceAll('"', "") : `${os.type()} ${os.release()}`;
  assert.equal(machine().os, expected);
});

/** os and /etc/os-release answering as the returned object says (read when asked). */
function osAnswers(t) {
  const answers = { cpus: [{ model: "Test CPU" }], totalmem: 2 ** 30, loadavg: [0, 0, 0], osRelease: "" };
  const real = fs.readFileSync;
  t.mock.method(os, "cpus", () => answers.cpus);
  t.mock.method(os, "totalmem", () => answers.totalmem);
  t.mock.method(os, "loadavg", () => answers.loadavg);
  t.mock.method(os, "type", () => "Testix");
  t.mock.method(os, "release", () => "9.9.9-test");
  t.mock.method(fs, "readFileSync", (p, ...rest) => (p === "/etc/os-release" ? serve(answers.osRelease) : real.call(fs, p, ...rest)));
  return answers;
}

const OS_RELEASES = [
  ['NAME=Ubuntu\nPRETTY_NAME="Ubuntu 22.04.3 LTS"\nVERSION_ID="22.04"\n', "Ubuntu 22.04.3 LTS"],
  ["ID=alpine\nPRETTY_NAME=Alpine Linux v3.20\n", "Alpine Linux v3.20"],
  ["PRETTY_NAME='Debian GNU/Linux 12'\n", "'Debian GNU/Linux 12'"],
  ['PRETTY_NAME=""\n', ""],
  ["ID=arch\n", "Testix 9.9.9-test"],
  ["", "Testix 9.9.9-test"],
  [new Error("no such file"), "Testix 9.9.9-test"],
];

test("`machine()` reads the OS from PRETTY_NAME, quoted or not, and falls back to the type and release when there is none or no file", (t) => {
  const answers = osAnswers(t);
  OS_RELEASES.forEach(([text, expected]) => {
    answers.osRelease = text;
    assert.equal(machine().os, expected, String(text));
  });
  assert.equal(machine().kernel, "9.9.9-test");
});

const CPUS = [
  [[], "unknown", 0],
  [[{}], "unknown", 1],
  [[{ model: "  Test CPU @ 3.00GHz  " }, { model: "second" }], "Test CPU @ 3.00GHz", 2],
];

test("`machine()` names the first CPU, trimmed, or 'unknown' where there is none or it has no model, and counts them all", (t) => {
  const answers = osAnswers(t);
  CPUS.forEach(([cpus, cpu, logicalCpus]) => {
    answers.cpus = cpus;
    assert.deepEqual([machine().cpu, machine().logicalCpus], [cpu, logicalCpus]);
  });
});

test("`machine()` rounds memory to a tenth of a GiB and each load average to a hundredth", (t) => {
  const answers = osAnswers(t);
  [[2 ** 30, 1], [8e9, 7.5], [2 ** 35, 32], [2 ** 34 + 4e7, 16]].forEach(([bytes, gib]) => {
    answers.totalmem = bytes;
    assert.equal(machine().memoryGiB, gib, `${bytes} bytes`);
  });
  answers.loadavg = [0.126, 1.5, 12.999];
  assert.deepEqual(machine().loadAverage, [0.13, 1.5, 13]);
});

// --- processes ----------------------------------------------------------------------

const getconf = (name) => Number(spawnSync("getconf", [name], { encoding: "utf8" }).stdout.trim());

test("`TICK_MS` and `PAGE_KIB` are /proc's CPU tick in milliseconds and its memory page in KiB, as getconf gives them", () => {
  assert.equal(TICK_MS, 1000 / (getconf("CLK_TCK") || 100));
  assert.equal(PAGE_KIB, (getconf("PAGESIZE") || 4096) / 1024);
  assert.equal(TICK_MS > 0 && PAGE_KIB > 0, true);
});

/** The instrument's modules, copied to `dir` (to be loaded from where nothing is installed). */
function copyModules(dir) {
  fs.mkdirSync(dir, { recursive: true });
  fs.readdirSync(HERE)
    .filter((name) => name.endsWith(".mjs"))
    .forEach((name) => fs.copyFileSync(path.join(HERE, name), path.join(dir, name)));
}

/** lib.mjs, copied to `dir` and imported with a `getconf` first on PATH that
 * runs `script` (the module asks getconf as it loads). */
async function libWithGetconf(dir, script) {
  const bin = path.join(dir, "bin");
  fs.mkdirSync(bin, { recursive: true });
  fs.writeFileSync(path.join(bin, "getconf"), script, { mode: 0o755 });
  copyModules(dir);
  const saved = process.env.PATH;
  process.env.PATH = `${bin}${path.delimiter}${saved}`;
  try {
    return await import(pathToFileURL(path.join(dir, "lib.mjs")).href);
  } finally {
    process.env.PATH = saved;
  }
}

test("`TICK_MS` and `PAGE_KIB` follow what getconf prints, and are a 10 ms tick and a 4 KiB page when it prints nothing", async () => {
  await inTemp(async (dir) => {
    const told = await libWithGetconf(path.join(dir, "told"), '#!/bin/sh\ncase "$1" in CLK_TCK) echo 250;; PAGESIZE) echo 16384;; esac\n');
    const silent = await libWithGetconf(path.join(dir, "silent"), "#!/bin/sh\nexit 1\n");
    assert.deepEqual([told.TICK_MS, told.PAGE_KIB], [4, 16]);
    assert.deepEqual([silent.TICK_MS, silent.PAGE_KIB], [10, 4]);
  });
});

const A = 5_000_010; // the root: two threads, the second with no children
const B = 5_000_020; // its child: a second thread whose children file cannot be read
const C = 5_000_030; // its child: a command name with spaces and parentheses
const D = 5_000_040; // B's child, whose children file names the root and C again
const GONE = 5_000_050; // no such process
const TREE = {
  [`/proc/${A}/task`]: [`${A}`, `${A + 1}`],
  [`/proc/${A}/task/${A}/children`]: `${B}  ${C} \n`,
  [`/proc/${A}/task/${A + 1}/children`]: "\n",
  [`/proc/${B}/task`]: [`${B}`, `${B + 1}`],
  [`/proc/${B}/task/${B}/children`]: `${D}\n`,
  [`/proc/${B}/task/${B + 1}/children`]: new Error("the thread has gone"),
  [`/proc/${C}/task`]: [`${C}`],
  [`/proc/${C}/task/${C}/children`]: "",
  [`/proc/${D}/task`]: [`${D}`],
  [`/proc/${D}/task/${D}/children`]: `${A} ${C}\n`,
  [`/proc/${A}/stat`]: stat(A, 100, 20),
  [`/proc/${A}/statm`]: statm(1000),
  [`/proc/${B}/stat`]: stat(B, 3, 4),
  [`/proc/${B}/statm`]: new Error("gone between the two reads"),
  [`/proc/${C}/stat`]: stat(C, 7, 8, "my (odd) cmd"),
  [`/proc/${C}/statm`]: statm(50),
};

test("`processTree` lists a process and its descendants, last child first, each once, over every thread's children and whatever cycles", (t) => {
  fakeProc(t, TREE);
  assert.deepEqual(processTree(A), [A, C, B, D]);
  assert.deepEqual(processTree(C), [C]);
});

test("`processTree` of a process that has gone is that process alone", (t) => {
  fakeProc(t, TREE);
  assert.deepEqual(processTree(GONE), [GONE]);
});

test("`treeCost` sums CPU ticks and resident pages over the tree, counts every process, and takes what a vanished process still had", (t) => {
  fakeProc(t, TREE);
  // A 100+20 ticks, 1000 pages; B 3+4 ticks and no pages (its statm failed
  // after its stat was added); C 7+8 ticks (a command name read past its last
  // parenthesis), 50 pages; D gone, but counted.
  assert.deepEqual(treeCost(A), { pids: 4, cpuMs: 142 * TICK_MS, rssKiB: 1050 * PAGE_KIB });
  assert.deepEqual(treeCost(C), { pids: 1, cpuMs: 15 * TICK_MS, rssKiB: 50 * PAGE_KIB });
});

test("`treeCost` of a process that has gone counts the one pid and nothing else", (t) => {
  fakeProc(t, TREE);
  assert.deepEqual(treeCost(GONE), { pids: 1, cpuMs: 0, rssKiB: 0 });
});

const FAMILY = `import { spawn } from "node:child_process";
const grandchild = spawn(process.execPath, ["-e", "setInterval(() => {}, 1000)"], { stdio: "ignore" });
const until = performance.now() + 50;
while (performance.now() < until);
process.stdout.write(grandchild.pid + "\\n");
setInterval(() => {}, 1000);
`;

function ends(pid) {
  try {
    realKill(pid, "SIGKILL");
  } catch {
    /* gone */
  }
}

/** A child process and its own child, both up; `fn` gets their pids. */
async function withFamily(fn) {
  return inTemp(async (dir) => {
    const script = path.join(dir, "family.mjs");
    fs.writeFileSync(script, FAMILY);
    const child = spawn(process.execPath, [script], { stdio: ["ignore", "pipe", "inherit"] });
    const exited = once(child, "exit");
    const family = [child.pid];
    try {
      const [first] = await once(child.stdout, "data", { signal: AbortSignal.timeout(10_000) });
      family.push(Number(String(first)));
      return await fn(...family, exited);
    } finally {
      family.reverse().forEach((pid) => ends(pid));
      await exited;
    }
  });
}

test("`processTree` and `treeCost` of a live process and the child it started, and of both once they are gone", NEEDS_PROC, async () => {
  await withFamily(async (child, grandchild, exited) => {
    assert.deepEqual(processTree(child), [child, grandchild]);
    assert.deepEqual(processTree(grandchild), [grandchild]);
    const tree = treeCost(child);
    assert.equal(tree.pids, 2);
    assert.equal(tree.cpuMs > 0 && tree.cpuMs % TICK_MS === 0, true, JSON.stringify(tree));
    assert.equal(tree.rssKiB > treeCost(grandchild).rssKiB && tree.rssKiB % PAGE_KIB === 0, true, JSON.stringify(tree));
    ends(grandchild);
    ends(child);
    await exited;
    assert.deepEqual(processTree(child), [child]);
    assert.deepEqual(treeCost(child), { pids: 1, cpuMs: 0, rssKiB: 0 });
  });
});

// --- servers ---------------------------------------------------------------------------

test("`portFree` is false for a port a server listens on, on the loopback or on every interface, and true once it has closed", async () => {
  const onLoopback = net.createServer();
  const onAll = net.createServer();
  const [first, second] = [await listen(onLoopback), await listen(onAll, "0.0.0.0")];
  assert.deepEqual([await portFree(first), await portFree(second)], [false, false]);
  await Promise.all([shut(onLoopback), shut(onAll)]);
  assert.deepEqual([await portFree(first), await portFree(second)], [true, true]);
});

test("`portFree` leaves the port free: a server can listen on it right after", async () => {
  const port = await freePort();
  assert.equal(await portFree(port), true);
  const server = net.createServer();
  await new Promise((resolve) => server.listen(port, "127.0.0.1", resolve));
  assert.equal(await portFree(port), false);
  await shut(server);
});

test("`waitForHttp` counts any answer as up, a 404 or a 500 too, after the one request", async () => {
  for (const status of [404, 500]) {
    await withHttp(
      (req, res) => res.writeHead(status).end("no"),
      async ({ base, seen }) => {
        assert.equal(await waitForHttp(`${base}/missing`, 2000), true);
        assert.deepEqual(seen, ["GET /missing"]);
      },
    );
  }
});

test("`waitForHttp` does not follow a redirect: its answer is the redirect", async () => {
  const redirect = (req, res) => (req.url === "/start" ? res.writeHead(302, { location: "/next" }).end() : res.end("next"));
  await withHttp(redirect, async ({ base, seen }) => {
    assert.equal(await waitForHttp(`${base}/start`, 2000), true);
    assert.deepEqual(seen, ["GET /start"]);
  });
});

test("`waitForHttp` returns when the answer has ended, not when its headers come", async () => {
  const events = [];
  const slowly = (req, res) => {
    res.writeHead(200);
    res.write("first");
    setTimeout(() => {
      events.push("the server ends the answer");
      res.end("last");
    }, 100);
  };
  await withHttp(slowly, async ({ base }) => {
    assert.equal(await waitForHttp(`${base}/`, 2000), true);
    events.push("waitForHttp returns");
  });
  assert.deepEqual(events, ["the server ends the answer", "waitForHttp returns"]);
});

test("`waitForHttp` asks again when an answer is cut off in its body", async () => {
  const cutOffOnce = (req, res, n) => {
    if (n > 1) return res.end("ok");
    res.writeHead(200, { "content-length": "100" });
    res.write("0123");
    setTimeout(() => req.socket.destroy(), 5);
  };
  await withHttp(cutOffOnce, async ({ base, seen }) => {
    assert.equal(await waitForHttp(`${base}/cut`, 5000), true);
    assert.deepEqual(seen, ["GET /cut", "GET /cut"]);
  });
});

test("`waitForHttp` is false when its budget is spent, having asked `alive` before each try, and tries nothing on a budget of 0", async () => {
  const url = `http://127.0.0.1:${await freePort()}/`;
  for (const [ms, asked] of [[100, 1], [0, 0], [-5, 0]]) {
    let calls = 0;
    const alive = () => ++calls > 0;
    assert.equal(await waitForHttp(url, ms, alive), false, `${ms} ms`);
    assert.equal(calls, asked, `${ms} ms`);
  }
});

test("`waitForHttp` waits between its tries, a moment and not a spin: some 150 ms", async () => {
  const url = `http://127.0.0.1:${await freePort()}/`;
  let calls = 0;
  assert.equal(await waitForHttp(url, 400, () => ++calls > 0), false);
  assert.deepEqual([calls >= 2, calls <= 4], [true, true], `${calls} tries in 400 ms`);
});

test("`waitForHttp` is false at once, with no request, when `alive` says the server is gone", async () => {
  await withHttp(
    (req, res) => res.end("up"),
    async ({ base, seen }) => {
      assert.equal(await waitForHttp(`${base}/`, 5000, () => false), false);
      assert.deepEqual(seen, []);
    },
  );
});

test("`waitForHttp` tries again after a refusal and finds a server that came up between the tries", async () => {
  const port = await freePort();
  const server = http.createServer((req, res) => res.end("up"));
  let tries = 0;
  // The server starts to listen 20 ms after the first try, well before the second.
  const alive = () => {
    if (++tries === 1) setTimeout(() => server.listen(port, "127.0.0.1"), 20);
    return true;
  };
  try {
    assert.equal(await waitForHttp(`http://127.0.0.1:${port}/`, 5000, alive), true);
    assert.equal(tries, 2);
  } finally {
    await shut(server);
  }
});

// What `startServer` is given to start: a tiny Node script per case, in a temp
// directory of its own, on a port the kernel chose. `plain` serves, logs each
// request line and exits on /quit; `chatty` first logs what it was started with;
// `family` starts a grandchild first; `deaf` ignores SIGTERM; `failing` (30
// lines, code 1), `silent` (code 3), `blanks`, `partial` (no newline at the end)
// and `killed` (by SIGKILL) exit at once; `mute` stays up and never listens.
const LISTEN = `http.createServer((req, res) => { console.log(req.method + " " + req.url); res.end("ok"); if (req.url === "/quit") setTimeout(() => process.exit(0), 20); }).listen(Number(process.argv[2]), "127.0.0.1");`;
const GRANDCHILD = `import { spawn } from "node:child_process";
const grandchild = spawn(process.execPath, ["-e", "setInterval(() => {}, 1000)"], { stdio: "ignore" });
fs.writeFileSync("grandchild.pid", String(grandchild.pid));`;
const SCRIPTS = {
  plain: `import http from "node:http";\n${LISTEN}\n`,
  chatty: `import fs from "node:fs";\nimport http from "node:http";
console.log(JSON.stringify({ cwd: process.cwd(), inherited: process.env.H2H_TEST_INHERITED, overridden: process.env.H2H_TEST_OVERRIDDEN, added: process.env.H2H_TEST_ADDED, stdin: fs.readSync(0, Buffer.alloc(1)) }));
console.error("to stderr");\n${LISTEN}\n`,
  family: `import fs from "node:fs";\nimport http from "node:http";\n${GRANDCHILD}\n${LISTEN}\n`,
  deaf: `import http from "node:http";\nprocess.on("SIGTERM", () => {});\n${LISTEN}\n`,
  failing: `for (let i = 1; i <= 30; i++) console.log("line " + i);\nprocess.exit(1);\n`,
  silent: `process.exit(3);\n`,
  blanks: `console.log("a");\nconsole.log("");\nconsole.log("b");\nprocess.exit(2);\n`,
  partial: `process.stdout.write("first\\npartial");\nprocess.exit(4);\n`,
  killed: `console.log("about to die");\nprocess.kill(process.pid, "SIGKILL");\n`,
  mute: `import fs from "node:fs";\nconsole.log("starting");\n${GRANDCHILD}\nfs.writeFileSync("mute.pid", String(process.pid));\nsetInterval(() => {}, 1000);\n`,
};

/** The options for `startServer` to run one of the scripts in `dir`. */
async function prepared(dir, script, over) {
  const port = await freePort();
  const file = path.join(dir, `${script}.mjs`);
  fs.writeFileSync(file, SCRIPTS[script]);
  return { name: "tiny", cmd: process.execPath, args: [file, String(port)], cwd: dir, port, log: path.join(dir, "tiny.log"), ...over };
}

/** Stop a server for good, with a signal no test has faked if its own stop could not. */
async function reap(server) {
  await server.stop();
  if (server.alive()) {
    ends(-server.pid);
    await until(() => !server.alive());
  }
}

async function serving(options, fn) {
  const server = await startServer(options);
  try {
    return await fn(server, options);
  } finally {
    await reap(server);
  }
}

const withServer = (script, over, fn) => inTemp(async (dir) => serving(await prepared(dir, script, over), fn));

/** What `startServer` throws for a script that does not come up. */
const failure = (script, over, fn) =>
  inTemp(async (dir) => {
    const options = await prepared(dir, script, over);
    const error = await startServer(options).then(
      (server) => server.stop().then(() => assert.fail("the server started")),
      (e) => e,
    );
    return fn(error, options, dir);
  });

const pidOf = (dir, name) => Number(fs.readFileSync(path.join(dir, name), "utf8"));

/** Watch (and with `refuse`, fail) the signals sent to processes. */
function watchKills(t, refuse = null) {
  return t.mock.method(process, "kill", (pid, signal) => {
    if (signal === refuse) throw new Error("ESRCH");
    return realKill.call(process, pid, signal);
  });
}

const signals = (kill) => kill.mock.calls.map((call) => call.arguments);

test("`startServer` returns the server's name, pid, port, base, log, `alive` and `stop` once its probe is answered, in a group of its own", async () => {
  await withServer("plain", {}, async (server, options) => {
    const { alive, stop, pid, ...rest } = server;
    assert.deepEqual(Object.keys(server), ["name", "pid", "port", "base", "log", "alive", "stop"]);
    assert.deepEqual(rest, { name: "tiny", port: options.port, base: `http://127.0.0.1:${options.port}`, log: options.log });
    assert.equal(Number.isInteger(pid) && pid > 0, true);
    assert.equal(alive(), true);
    assert.doesNotThrow(() => realKill(-pid, 0));
    assert.equal(fs.readFileSync(options.log, "utf8"), "GET /__h2h-ready\n");
    await stop();
    assert.equal(alive(), false);
  });
});

test("`startServer` asks for the probe path it is given, and `/__h2h-ready` otherwise", async () => {
  await withServer("plain", { probe: "/up?check=1" }, async (server, options) => {
    assert.equal(fs.readFileSync(options.log, "utf8"), "GET /up?check=1\n");
  });
});

test("`startServer` runs the command in `cwd` with the parent's environment and `env` over it, no stdin, one log for both outputs, started empty", async (t) => {
  process.env.H2H_TEST_INHERITED = "parent";
  process.env.H2H_TEST_OVERRIDDEN = "parent";
  t.after(() => {
    delete process.env.H2H_TEST_INHERITED;
    delete process.env.H2H_TEST_OVERRIDDEN;
  });
  await inTemp(async (dir) => {
    fs.writeFileSync(path.join(dir, "tiny.log"), "left over from the last run\n");
    const options = await prepared(dir, "chatty", { env: { H2H_TEST_OVERRIDDEN: "child", H2H_TEST_ADDED: "added" } });
    await serving(options, async () => {
      const first = { cwd: fs.realpathSync(dir), inherited: "parent", overridden: "child", added: "added", stdin: 0 };
      assert.equal(fs.readFileSync(options.log, "utf8"), `${JSON.stringify(first)}\nto stderr\nGET /__h2h-ready\n`);
    });
  });
});

test("`startServer`'s stop ends the whole process group, a grandchild too, with SIGTERM to the group, as soon as the server is gone, and asking again does nothing", NEEDS_PROC, async (t) => {
  await withServer("family", {}, async (server, options) => {
    const grandchild = pidOf(options.cwd, "grandchild.pid");
    assert.equal(running(grandchild), true);
    const kill = watchKills(t);
    const started = performance.now();
    await server.stop();
    assert.equal(performance.now() - started < 2000, true);
    assert.equal(server.alive(), false);
    assert.equal(await until(() => !running(grandchild)), true);
    await server.stop();
    assert.deepEqual(signals(kill), [[-server.pid, "SIGTERM"]]);
  });
});

/** A clock that runs a second a reading, counted: `stop()` reads it six times
 * over its 5 s (once for the deadline, then at each poll), so its polls take
 * a few hundred milliseconds. */
function steppedClock(t) {
  let readings = 0;
  const mock = t.mock.method(Date, "now", () => 1_000_000 + ++readings * 1000);
  return { readings: () => readings, restore: () => mock.mock.restore() };
}

test("`startServer`'s stop sends SIGKILL to the group when SIGTERM has not ended the server in 5 s (a clock that runs a second a reading)", NEEDS_PROC, async (t) => {
  await withServer("deaf", {}, async (server) => {
    const kill = watchKills(t);
    const clock = steppedClock(t);
    await server.stop();
    clock.restore();
    assert.equal(clock.readings(), 6);
    assert.deepEqual(signals(kill), [[-server.pid, "SIGTERM"], [-server.pid, "SIGKILL"]]);
    assert.equal(await until(() => !running(server.pid)), true);
  });
});

test("`startServer`'s stop gives up quietly when even SIGKILL cannot be sent, the server still there (a clock that runs a second a reading)", async (t) => {
  await withServer("deaf", {}, async (server) => {
    const kill = watchKills(t, "SIGKILL");
    const clock = steppedClock(t);
    assert.equal(await server.stop(), undefined);
    clock.restore();
    assert.deepEqual(signals(kill), [[-server.pid, "SIGTERM"], [-server.pid, "SIGKILL"]]);
    assert.equal(server.alive(), true);
    kill.mock.restore();
    realKill(-server.pid, "SIGKILL");
    assert.equal(await until(() => !server.alive()), true);
  });
});

test("`startServer`'s stop signals nothing to a server that has exited by itself", async (t) => {
  await withServer("plain", {}, async (server) => {
    await fetch(`${server.base}/quit`).then((res) => res.arrayBuffer());
    assert.equal(await until(() => !server.alive()), true);
    const kill = watchKills(t);
    await server.stop();
    assert.deepEqual(signals(kill), []);
  });
});

test("`startServer`'s stop returns at once when the group cannot be signalled, leaving the server as it was", async (t) => {
  await withServer("plain", {}, async (server) => {
    const kill = watchKills(t, "SIGTERM");
    assert.equal(await server.stop(), undefined);
    assert.deepEqual(signals(kill), [[-server.pid, "SIGTERM"]]);
    assert.equal(server.alive(), true);
    kill.mock.restore();
  });
});

test("`startServer` throws that the port is taken, before it spawns or opens its log", async () => {
  // One that answers, so that a start that went ahead would find its probe answered and be seen to.
  const taken = http.createServer((req, res) => res.end("taken"));
  const port = await listen(taken);
  try {
    await failure("plain", { port }, (error, options) => {
      assert.equal(error.message, `tiny: port ${port} is already taken`);
      assert.equal(error.name, "Error");
      assert.equal(fs.existsSync(options.log), false);
    });
  } finally {
    await shut(taken);
  }
});

const LAST_LINES = Array.from({ length: 19 }, (_, i) => `line ${i + 12}`).join("\n");
const FAILURES = [
  ["failing", (port) => `tiny: did not start on ${port} (exited {"code":1,"signal":null})\n${LAST_LINES}\n`],
  ["silent", (port) => `tiny: did not start on ${port} (exited {"code":3,"signal":null})\n`],
  ["blanks", (port) => `tiny: did not start on ${port} (exited {"code":2,"signal":null})\na\n\nb\n`],
  ["partial", (port) => `tiny: did not start on ${port} (exited {"code":4,"signal":null})\nfirst\npartial`],
  ["killed", (port) => `tiny: did not start on ${port} (exited {"code":null,"signal":"SIGKILL"})\nabout to die\n`],
];

test("`startServer` throws, for a server that exits before it answers, that it did not start, how it exited and the log's last lines as they are", async () => {
  await Promise.all(
    FAILURES.map(([script, message]) =>
      failure(script, {}, (error, options) => {
        assert.equal(error.message, message(options.port));
        assert.equal(error.name, "Error");
      }),
    ),
  );
});

test("`startServer` keeps the whole log of a server that did not start: its 30 lines, of which the message shows the last 19 and the empty end", async () => {
  await failure("failing", {}, (error, options) => {
    const log = Array.from({ length: 30 }, (_, i) => `line ${i + 1}\n`).join("");
    assert.equal(fs.readFileSync(options.log, "utf8"), log);
  });
});

test("`startServer` gives up on a server that stays up and never answers after 90 s, kills its group, and says nothing of an exit (a clock that jumps)", NEEDS_PROC, async (t) => {
  await inTemp(async (dir) => {
    const options = await prepared(dir, "mute", {});
    const real = Date.now;
    // Real time until the server has logged and written its pid, then 20 s more
    // at each reading: the 90 s are spent at the fifth.
    let jumps = 0;
    t.mock.method(Date, "now", () => (fs.existsSync(path.join(dir, "mute.pid")) ? real.call(Date) + ++jumps * 20_000 : real.call(Date)));
    const error = await startServer(options).then(
      (server) => server.stop().then(() => assert.fail("the server started")),
      (e) => e,
    );
    assert.equal(jumps, 5);
    assert.equal(error.message, `tiny: did not start on ${options.port}\nstarting\n`);
    const [server, grandchild] = [pidOf(dir, "mute.pid"), pidOf(dir, "grandchild.pid")];
    assert.equal(await until(() => !running(server) && !running(grandchild)), true);
  });
});
