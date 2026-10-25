// What a head-to-head run is held to before any number of it is reported
// (ADR-XXXX, "the head-to-head"). Each rule refuses a run that would report
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
//               did not download its main script again.
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

export function checkRuns(runs, minRuns = MIN_RUNS) {
  const problems = [];
  const cells = new Map();
  for (const r of runs) cells.set(cellKey(r), (cells.get(cellKey(r)) ?? 0) + 1);
  const counts = new Set(cells.values());
  for (const [cell, n] of cells) {
    if (n < minRuns) problems.push(`runs: ${cell} has ${n} run(s), fewer than ${minRuns}`);
  }
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

export function checkMinified(stackRuns, expect, common = []) {
  const problems = [];
  for (const q of coldBodies(stackRuns)) {
    if (!q.type || !SCRIPT_OR_STYLE.test(q.type) || q.minified == null) continue;
    if (isCommon(q.url, common)) continue;
    if (expect.minifies && !q.minified) {
      problems.push(`minified: ${q.url} is not minified, and this stack's build minifies`);
    }
  }
  return [...new Set(problems)];
}

export function checkCompressed(stackRuns, expect) {
  const problems = [];
  if (!expect.compresses) return problems;
  for (const q of coldBodies(stackRuns)) {
    if (!q.type || !COMPRESSIBLE.test(q.type) || (q.raw ?? 0) < 1024 || q.status !== 200) continue;
    if (expect.compresses === "static" && q.kind === "document") continue;
    if (!q.encoding) problems.push(`compressed: ${q.url} (${q.raw} bytes of ${q.type}) was sent uncompressed`);
  }
  return [...new Set(problems)];
}

/**
 * What was served is the build named. `served` is the cold runs' load and
 * interaction responses; `build` the target's build description.
 */
export function checkBuild(build, served, { manifest = null, readFile = fs.readFileSync, exists = fs.existsSync, mtime = (p) => fs.statSync(p).mtimeMs, sources = [] } = {}) {
  const problems = [];
  if (build.kind === "vendored") {
    const files = manifest?.files ?? {};
    let matched = 0;
    for (const q of served) {
      const p = new URL(q.url).pathname;
      if (!p.startsWith(build.prefix) || q.status !== 200 || q.blob == null) continue;
      const rel = `${build.dir}/${p.slice(build.prefix.length) || "index.html"}`;
      const want = files[rel];
      if (!want) {
        problems.push(`build: ${p} is not a file of the vendored build (${rel})`);
        continue;
      }
      if (q.blob !== want.blob) problems.push(`build: ${p} is not upstream's ${rel} (blob ${q.blob}, upstream ${want.blob})`);
      else matched++;
    }
    for (const [rel, want] of Object.entries(files)) {
      const p = path.join(build.root, rel);
      if (!exists(p)) {
        problems.push(`build: ${rel} is missing from the vendored copy`);
        continue;
      }
      const blob = gitBlob(readFile(p));
      if (blob !== want.blob) problems.push(`build: the vendored ${rel} is not upstream's (blob ${blob}, upstream ${want.blob})`);
    }
    if (!matched) problems.push("build: no served file was matched to the vendored build");
  } else if (build.kind === "pleris") {
    if (!exists(build.sources)) {
      problems.push(`build: ${build.sources} is missing: the build did not finish`);
    } else {
      const { files } = JSON.parse(readFile(build.sources, "utf8"));
      if (!files?.length) problems.push(`build: ${build.sources} lists no source`);
      for (const f of files ?? []) {
        if (!exists(f.path)) problems.push(`build: its source ${f.path} is gone`);
        else if (sha256(readFile(f.path)) !== f.sha256) problems.push(`build: its source ${f.path} changed since it was built`);
      }
    }
    for (const q of served) {
      const p = new URL(q.url).pathname;
      if (q.status !== 200 || q.blob == null) continue;
      const local = p === "/pw-runtime.mjs" || /^\/pw-[a-z]+\.wasm$/.test(p) ? path.join(build.dist, p) : null;
      if (local && exists(local) && gitBlob(readFile(local)) !== q.blob) {
        problems.push(`build: ${p} served is not the build's ${path.relative(build.dist, local)}`);
      }
    }
  } else if (build.kind === "next" || build.kind === "sveltekit") {
    const marker = build.kind === "next" ? path.join(build.dir, ".next/BUILD_ID") : path.join(build.dir, "build/index.js");
    if (!exists(marker)) {
      problems.push(`build: ${marker} is missing: the stack is not built`);
    } else {
      const built = mtime(marker);
      for (const s of sources) {
        if (mtime(s) > built) problems.push(`build: ${path.relative(build.dir, s)} is newer than the build`);
      }
      if (build.kind === "next") {
        const id = readFile(marker, "utf8").trim();
        const doc = served.find((q) => q.kind === "document" && q.status === 200);
        if (doc && doc.text != null && !doc.text.includes(id)) {
          problems.push(`build: the page served does not name the build ${id}`);
        }
      }
    }
  } else {
    problems.push(`build: unknown build kind ${build.kind}`);
  }
  return [...new Set(problems)];
}

export function checkShaped(runs, network) {
  const problems = [];
  for (const r of runs) {
    if (!r.network) continue;
    if (r.timings?.documentEnd == null || r.timings.documentEnd < network.latency) {
      problems.push(
        `shaped: ${cellKey(r)} run ${r.i}: its document's last byte came at ${r.timings?.documentEnd?.toFixed?.(0)} ms, under the profile's ${network.latency} ms round trip`,
      );
    }
  }
  return problems;
}

export function checkInteractive(runs, steps) {
  const problems = [];
  for (const r of runs) {
    if (r.error) {
      problems.push(`interactive: ${cellKey(r)} run ${r.i}: the run failed: ${r.error}`);
      continue;
    }
    if (!r.sameDocument) problems.push(`interactive: ${cellKey(r)} run ${r.i}: an interaction loaded a new document`);
    if ((r.interactions ?? []).length !== steps.length) {
      problems.push(`interactive: ${cellKey(r)} run ${r.i}: ${(r.interactions ?? []).length} of ${steps.length} interactions completed`);
    }
    for (const x of r.interactions ?? []) {
      if (!(x.toDom >= 0) || !(x.toFrame >= x.toDom)) {
        problems.push(`interactive: ${cellKey(r)} run ${r.i}: ${x.name} has no latency (${x.toDom}, ${x.toFrame})`);
      }
    }
    if (r.timings?.ready == null) problems.push(`interactive: ${cellKey(r)} run ${r.i}: never interactive`);
  }
  return problems;
}

/**
 * A warm run of a stack whose host lets a cache keep its files: its largest
 * cold script must not have been downloaded again. Held in Chromium alone,
 * where the DevTools protocol says which response a cache answered: in
 * Firefox and WebKit, Playwright reports a cached response's size as if it
 * had been received (a probe of 2026-10-25: the server saw no request for
 * an immutable file the engines' sizes reported in full).
 */
export function checkWarm(runs, expect) {
  const problems = [];
  if (!expect.caches) return problems;
  const cold = runs.find((r) => r.condition === "cold" && !r.network && r.engine === "chromium");
  if (!cold) return problems;
  const scripts = (cold.requests ?? []).filter((q) => q.phase === "load" && /javascript/.test(q.type ?? ""));
  const biggest = scripts.sort((a, b) => (b.raw ?? 0) - (a.raw ?? 0))[0];
  if (!biggest) return problems;
  for (const r of runs.filter((x) => x.condition === "warm" && !x.network && x.engine === "chromium")) {
    const again = (r.requests ?? []).find((q) => q.url === biggest.url && q.phase === "load");
    if (again && again.status === 200 && (again.receivedBody ?? 0) >= 0.5 * (biggest.receivedBody ?? biggest.raw)) {
      problems.push(`warm: ${cellKey(r)} run ${r.i}: ${new URL(biggest.url).pathname} was downloaded again (${again.receivedBody} bytes)`);
    }
  }
  return [...new Set(problems)];
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
