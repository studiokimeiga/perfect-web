// What a head-to-head run is held to before any number of it is reported
// (ADR-0392, "the head-to-head"). Each rule refuses a run that would report
// something other than what it says it measured:
//
//   runs        every cell (app, stack, engine, condition, network) has the
//               same number of runs, and at least MIN_RUNS: a median of one
//               run is one run, and a spread needs several;
//   build       what was served is the build named: a vendored app's files
//               are its upstream blobs, a Pleris build is of the sources it
//               lists, a framework's build is newer than its sources and is
//               the one its server serves;
//   minified    a stack whose build minifies serves minified scripts and
//               styles (TodoMVC's common files aside); one whose build does
//               not is reported so, a loss, never refused;
//   compressed  a host that compresses sends compressible responses of 1 KiB
//               or more compressed (a host that compresses only its static
//               files, its documents aside); one that does not is reported;
//   shaped      a run under a network profile shows the profile's round trip
//               in its document's last byte (Chromium's shaping holds a body,
//               not its first byte): shaping applied to every stack it is
//               said to apply to;
//   interactive every interaction of every run took effect, in the document
//               the run loaded;
//   warm        a warm run of a stack whose host lets a cache keep its files
//               did not download its main script again;
//   server      the servers' measurements (a fixed load, idle visitors, a
//               press with tabs open) as many runs a stack as each other
//               and at least MIN_RUNS, none failed, every request answered.
//
// Each returns a list of problems, empty when the rule holds. `validate`
// runs them all. The tests are `test/validate.test.mjs`; the mutation
// controls `scripts/head_to_head_mutations.py`.

import fs from "node:fs";
import path from "node:path";
import { gitBlob, sha256 } from "./lib.mjs";

export const MIN_RUNS = 5;

/** Responses of 1 KiB or more of these types are compressed by a host that compresses. */
export const COMPRESSIBLE = /^(text\/|application\/(javascript|json|wasm)|image\/svg\+xml)/;
export const SCRIPT_OR_STYLE = /^(text\/javascript|application\/javascript|text\/css)$/;

const cellKey = (r) => `${r.app}/${r.stack}/${r.engine}/${r.condition}/${r.network ?? "none"}`;

/** How many runs each cell has. */
function cellCounts(runs) {
  const cells = new Map();
  for (const r of runs) cells.set(cellKey(r), (cells.get(cellKey(r)) ?? 0) + 1);
  return cells;
}

export function checkRuns(runs, minRuns = MIN_RUNS) {
  const cells = cellCounts(runs);
  const counts = new Set(cells.values());
  const problems = [...cells]
    .filter(([, n]) => n < minRuns)
    .map(([cell, n]) => `runs: ${cell} has ${n} run(s), fewer than ${minRuns}`);
  if (counts.size > 1) {
    problems.push(`runs: cells have different run counts (${[...counts].sort().join(", ")})`);
  }
  if (!cells.size) problems.push("runs: no run was made");
  return problems;
}

const isCommon = (url, common) => common.some((re) => re.test(new URL(url).pathname));

/** Load-phase responses of a cold run with bodies: what the build served. */
function coldBodies(runs) {
  return runs
    .filter((r) => r.condition === "cold" && !r.network)
    .flatMap((r) => (r.requests ?? []).filter((q) => q.phase === "load" || q.phase.startsWith("interaction")));
}

/** A script or style with a minified verdict (the instrument could tell). */
const judged = (q) => !!q.type && SCRIPT_OR_STYLE.test(q.type) && q.minified != null;

/** A judged script or style, not one of TodoMVC's common files, that is not
 * minified where the stack's build minifies. */
const unminified = (q, expect, common) => judged(q) && !isCommon(q.url, common) && expect.minifies && !q.minified;

export function checkMinified(stackRuns, expect, common = []) {
  const loose = coldBodies(stackRuns).filter((q) => unminified(q, expect, common));
  return [...new Set(loose.map((q) => `minified: ${q.url} is not minified, and this stack's build minifies`))];
}

const small = (q) => (q.raw ?? 0) < 1024;

/** A response a host that compresses must compress: of a compressible type, 1 KiB or more, answered 200. */
const owesCompression = (q) => !!q.type && COMPRESSIBLE.test(q.type) && !small(q) && q.status === 200;

/** A host that compresses only its static files owes its documents nothing. */
const exempt = (q, expect) => expect.compresses === "static" && q.kind === "document";

export function checkCompressed(stackRuns, expect) {
  if (!expect.compresses) return [];
  const plain = coldBodies(stackRuns).filter((q) => owesCompression(q) && !exempt(q, expect) && !q.encoding);
  return [...new Set(plain.map((q) => `compressed: ${q.url} (${q.raw} bytes of ${q.type}) was sent uncompressed`))];
}

/** A served response a vendored build is judged on: under its prefix, answered 200, with a body. */
const isServedFile = (build, q, p) => p.startsWith(build.prefix) && q.status === 200 && q.blob != null;

/** A served file of a vendored build against the manifest: its problem, or null where it is upstream's. */
function servedProblem(build, files, q, p) {
  const rel = `${build.dir}/${p.slice(build.prefix.length) || "index.html"}`;
  const want = files[rel];
  if (!want) return `build: ${p} is not a file of the vendored build (${rel})`;
  if (q.blob !== want.blob) return `build: ${p} is not upstream's ${rel} (blob ${q.blob}, upstream ${want.blob})`;
  return null;
}

/** One verdict (a problem, or null) for each served file under the build's prefix. */
function servedVerdicts(build, files, served) {
  return served
    .map((q) => [q, new URL(q.url).pathname])
    .filter(([q, p]) => isServedFile(build, q, p))
    .map(([q, p]) => servedProblem(build, files, q, p));
}

/** A file of the manifest against the vendored copy on disk: missing, or not upstream's. */
function copyProblem(build, rel, want, { exists, readFile }) {
  const p = path.join(build.root, rel);
  if (!exists(p)) return `build: ${rel} is missing from the vendored copy`;
  const blob = gitBlob(readFile(p));
  return blob === want.blob ? null : `build: the vendored ${rel} is not upstream's (blob ${blob}, upstream ${want.blob})`;
}

const copyProblems = (build, files, io) =>
  Object.entries(files).map(([rel, want]) => copyProblem(build, rel, want, io)).filter(Boolean);

function checkVendored(build, served, io) {
  const files = io.manifest?.files ?? {};
  const verdicts = servedVerdicts(build, files, served);
  const problems = [...verdicts.filter(Boolean), ...copyProblems(build, files, io)];
  if (!verdicts.some((v) => v === null)) problems.push("build: no served file was matched to the vendored build");
  return problems;
}

/** A source the Pleris build lists: gone, or changed since it was built. */
function sourceProblem(f, { exists, readFile }) {
  if (!exists(f.path)) return `build: its source ${f.path} is gone`;
  if (sha256(readFile(f.path)) !== f.sha256) return `build: its source ${f.path} changed since it was built`;
  return null;
}

const noSources = (build, files) => (files?.length ? [] : [`build: ${build.sources} lists no source`]);

/** The sources a Pleris build lists (`sources.json`), each against its hash. */
function plerisSources(build, io) {
  if (!io.exists(build.sources)) return [`build: ${build.sources} is missing: the build did not finish`];
  const { files } = JSON.parse(io.readFile(build.sources, "utf8"));
  return [...noSources(build, files), ...(files ?? []).map((f) => sourceProblem(f, io)).filter(Boolean)];
}

/** The local file the build holds for a served runtime or Wasm module; only these are compared. */
const localFile = (build, p) => (p === "/pw-runtime.mjs" || /^\/pw-[a-z]+\.wasm$/.test(p) ? path.join(build.dist, p) : null);

/** A served runtime or Wasm module that is not the build's own file. */
function servedPlerisProblem(build, q, p, io) {
  const local = localFile(build, p);
  if (local && io.exists(local) && gitBlob(io.readFile(local)) !== q.blob) {
    return `build: ${p} served is not the build's ${path.relative(build.dist, local)}`;
  }
  return null;
}

const servedPlerisProblems = (build, served, io) =>
  served
    .map((q) => [q, new URL(q.url).pathname])
    .filter(([q]) => q.status === 200 && q.blob != null)
    .map(([q, p]) => servedPlerisProblem(build, q, p, io))
    .filter(Boolean);

const checkPleris = (build, served, io) => [...plerisSources(build, io), ...servedPlerisProblems(build, served, io)];

/** Where each framework's build says it is built. */
const MARKERS = new Map([
  ["next", ".next/BUILD_ID"],
  ["sveltekit", "build/index.js"],
]);

/** The sources newer than the build. */
const staleSources = (build, built, io) =>
  io.sources.filter((s) => io.mtime(s) > built).map((s) => `build: ${path.relative(build.dir, s)} is newer than the build`);

/** Next.js writes its build id into the pages it serves: the document served must name it. */
function unnamedBuild(marker, served, io) {
  const id = io.readFile(marker, "utf8").trim();
  const doc = served.find((q) => q.kind === "document" && q.status === 200);
  return doc && doc.text != null && !doc.text.includes(id) ? [`build: the page served does not name the build ${id}`] : [];
}

function checkFramework(build, served, io) {
  const marker = path.join(build.dir, MARKERS.get(build.kind));
  if (!io.exists(marker)) return [`build: ${marker} is missing: the stack is not built`];
  const stale = staleSources(build, io.mtime(marker), io);
  return [...stale, ...(build.kind === "next" ? unnamedBuild(marker, served, io) : [])];
}

const BUILD_CHECKS = new Map([
  ["vendored", checkVendored],
  ["pleris", checkPleris],
  ["next", checkFramework],
  ["sveltekit", checkFramework],
]);

/**
 * What was served is the build named. `served` is the cold runs' load and
 * interaction responses; `build` the target's build description.
 */
export function checkBuild(build, served, { manifest = null, readFile = fs.readFileSync, exists = fs.existsSync, mtime = (p) => fs.statSync(p).mtimeMs, sources = [] } = {}) {
  const check = BUILD_CHECKS.get(build.kind);
  const problems = check ? check(build, served, { manifest, readFile, exists, mtime, sources }) : [`build: unknown build kind ${build.kind}`];
  return [...new Set(problems)];
}

/** A shaped run whose document's last byte came before the profile's round trip, or never. */
const tooFast = (r, network) => r.timings?.documentEnd == null || r.timings.documentEnd < network.latency;

const shapedProblem = (r, network) =>
  `shaped: ${cellKey(r)} run ${r.i}: its document's last byte came at ${r.timings?.documentEnd?.toFixed?.(0)} ms, under the profile's ${network.latency} ms round trip`;

export function checkShaped(runs, network) {
  return runs.filter((r) => r.network && tooFast(r, network)).map((r) => shapedProblem(r, network));
}

const where = (r) => `interactive: ${cellKey(r)} run ${r.i}`;

const newDocument = (r) => (r.sameDocument ? [] : [`${where(r)}: an interaction loaded a new document`]);

function incomplete(r, steps) {
  const done = (r.interactions ?? []).length;
  return done === steps.length ? [] : [`${where(r)}: ${done} of ${steps.length} interactions completed`];
}

const noLatency = (r) =>
  (r.interactions ?? [])
    .filter((x) => !(x.toDom >= 0) || !(x.toFrame >= x.toDom))
    .map((x) => `${where(r)}: ${x.name} has no latency (${x.toDom}, ${x.toFrame})`);

const neverReady = (r) => (r.timings?.ready == null ? [`${where(r)}: never interactive`] : []);

const RUN_CHECKS = [newDocument, incomplete, noLatency, neverReady];

/** A run's problems: that it failed, or the others, in order. */
function interactiveProblems(r, steps) {
  if (r.error) return [`${where(r)}: the run failed: ${r.error}`];
  return RUN_CHECKS.flatMap((check) => check(r, steps));
}

export function checkInteractive(runs, steps) {
  return runs.flatMap((r) => interactiveProblems(r, steps));
}

/** A cold or warm run in Chromium, not shaped. */
const chromiumUnshaped = (condition) => (r) => r.condition === condition && !r.network && r.engine === "chromium";

const loadedScripts = (run) => (run?.requests ?? []).filter((q) => q.phase === "load" && /javascript/.test(q.type ?? ""));

/** The largest script (by decoded bytes) the first cold unshaped Chromium run loaded, if any. */
function biggestCold(runs) {
  const scripts = loadedScripts(runs.find(chromiumUnshaped("cold")));
  return scripts.sort((a, b) => (b.raw ?? 0) - (a.raw ?? 0))[0];
}

const received = (q, fallback) => q.receivedBody ?? fallback;

/** Whether a warm run's answer for the script was a download: 200, with half or more of the cold run's bytes. */
const downloadedAgain = (again, biggest) =>
  !!again && again.status === 200 && received(again, 0) >= 0.5 * received(biggest, biggest.raw);

const againIn = (r, biggest) => (r.requests ?? []).find((q) => q.url === biggest.url && q.phase === "load");

const warmProblem = (r, biggest, again) =>
  `warm: ${cellKey(r)} run ${r.i}: ${new URL(biggest.url).pathname} was downloaded again (${again.receivedBody} bytes)`;

/**
 * A warm run of a stack whose host lets a cache keep its files: its largest
 * cold script must not have been downloaded again. Held in Chromium alone,
 * where the DevTools protocol says which response a cache answered: in
 * Firefox and WebKit, Playwright reports a cached response's size as if it
 * had been received (a probe of 2026-10-25: the server saw no request for
 * an immutable file the engines' sizes reported in full).
 */
export function checkWarm(runs, expect) {
  const biggest = expect.caches ? biggestCold(runs) : undefined;
  if (!biggest) return [];
  const problems = runs.filter(chromiumUnshaped("warm")).flatMap((r) => {
    const again = againIn(r, biggest);
    return downloadedAgain(again, biggest) ? [warmProblem(r, biggest, again)] : [];
  });
  return [...new Set(problems)];
}

/** How many runs each stack has of one server measurement. */
function stackCounts(stacks, list) {
  const counts = new Map(stacks.map((s) => [s, 0]));
  for (const r of list) counts.set(r.stack, (counts.get(r.stack) ?? 0) + 1);
  return counts;
}

/** A server measurement's run that failed or (a fixed load) left requests unanswered; null otherwise. */
function runProblem(kind, app, r) {
  const at = `${kind}: ${app}/${r.stack} round ${r.i}`;
  if (r.error) return `${at}: the measurement failed: ${r.error}`;
  if (kind === "load" && r.answered < r.sent) return `${at}: ${r.sent - r.answered} of ${r.sent} requests went unanswered`;
  return null;
}

const unevenRuns = (kind, app, counts) =>
  new Set(counts.values()).size > 1 ? [`${kind}: ${app}'s stacks have different run counts`] : [];

/** One kind of server measurement (a fixed load, idle visitors, a press): its failed runs, then the stacks short of runs, then uneven counts. */
function serverProblems(kind, list, { app, stacks, minRuns }) {
  const counts = stackCounts(stacks, list);
  const failed = list.map((r) => runProblem(kind, app, r)).filter(Boolean);
  const few = [...counts]
    .filter(([, n]) => n < minRuns)
    .map(([s, n]) => `${kind}: ${app}/${s} has ${n} run(s), fewer than ${minRuns}`);
  return [...failed, ...few, ...unevenRuns(kind, app, counts)];
}

/**
 * The servers' measurements are held as the pages' runs are (`checkRuns`):
 * every stack the same number of runs of each, at least `minRuns`, so that
 * each number is a median with its spread; a failed run is kept as the
 * failure it was; and a fixed load's every request answered.
 */
export function validateServer({ app, stacks, loads = [], idle = [], presses = [], minRuns = MIN_RUNS }) {
  const lists = [["load", loads], ["idle", idle], ["press", presses]];
  return lists.flatMap(([kind, list]) => serverProblems(kind, list, { app, stacks, minRuns }));
}

/** Every rule, for one app's runs: a list of problems, empty when the run may be reported. */
export function validate({ runs, targets, network, minRuns = MIN_RUNS, manifest, sources = {} }) {
  const problems = [...checkRuns(runs, minRuns)];
  for (const t of targets) {
    const mine = runs.filter((r) => r.app === t.app && r.stack === t.stack);
    const served = coldBodies(mine);
    problems.push(...checkBuild(t.build, served, { manifest, sources: sources[t.stack] ?? [] }).map((p) => `${t.app}/${t.stack}: ${p}`));
    problems.push(...checkMinified(mine, t.expect, t.common).map((p) => `${t.app}/${t.stack}: ${p}`));
    problems.push(...checkCompressed(mine, t.expect).map((p) => `${t.app}/${t.stack}: ${p}`));
    problems.push(...checkInteractive(mine, t.interactions));
    problems.push(...checkWarm(mine, t.expect).map((p) => `${t.app}/${t.stack}: ${p}`));
    if (network) problems.push(...checkShaped(mine, network));
  }
  return problems;
}
