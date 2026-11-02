#!/usr/bin/env node
// The museum's bug classes each stack lets ship (the head-to-head, ADR-0392):
// ADR-0342's exhibits as `just e14-museum` last recorded them
// (`docs/evidence/E14/museum.txt`), read per stack.
//
//   node benchmarks/headtohead/museum-classes.mjs [--json out.json]
//
// An exhibit's bug is shown happening in ONE framework (its `repro.stack`):
// its spec passes on the bug, as the stack's own checker accepted it, and
// fails on the framework's documented repair. That stack lets the class
// ship. The other framework is "not reproduced here": neither claimed to let
// it ship nor to refuse it. Pleris's answer is the museum's verdict:
// refused by a diagnostic, refused at runtime, or a miss. This reads the
// recorded evidence; it reruns nothing (`just e14-museum` does, and its
// commit and run are printed with it).

import fs from "node:fs";
import path from "node:path";
import { REPO, isMain, pathAfter } from "./lib.mjs";

const MUSEUM = path.join(REPO, "benchmarks/museum");
const EVIDENCE = path.join(REPO, "docs/evidence/E14/museum.txt");
const LABEL = { "next-react": "Next.js", sveltekit: "SvelteKit", pleris: "Pleris" };

const HEAD = /^== ([A-Z]\d+-[a-z0-9-]+): (.*)$/;
const BUG = /^\s+([a-z-]+), the bug: (\d+) passed, (\d+) failed/;
const REPAIRED = /^\s+([a-z-]+), repaired: (\d+) passed, (\d+) failed/;
const VERDICT = /^\s+=> (.*)$/;

/** A spec count line (`<stack>, ...: N passed, M failed`) as an object, or null. */
function counts(re, line) {
  const m = re.exec(line);
  return m && { stack: m[1], passed: Number(m[2]), failed: Number(m[3]) };
}

const verdictOf = (line) => VERDICT.exec(line)?.[1];

/** One more line of an exhibit's block, into what is known of it. */
function readLine(current, line) {
  current.bug = counts(BUG, line) ?? current.bug;
  current.repaired = counts(REPAIRED, line) ?? current.repaired;
  current.verdict = verdictOf(line) ?? current.verdict;
}

/** Each exhibit's block in the evidence: its title, the bug's and the
 * repair's spec counts, and the verdict. */
export function parseEvidence(text) {
  const out = {};
  let current = null;
  for (const line of text.split("\n")) {
    const head = HEAD.exec(line);
    if (head) current = out[head[1]] = { title: head[2], bug: null, repaired: null, verdict: null };
    else if (current) readLine(current, line);
  }
  return out;
}

const capture = (re, text, group) => re.exec(text)?.[group] ?? null;

/** The evidence's own header: the commit and run it was recorded at, and its summary line. */
function metaOf(text) {
  return {
    commit: capture(/^commit: (\S+)/m, text, 1),
    ci: capture(/^ci: (\S+)/m, text, 1),
    summary: capture(/^museum: .*$/m, text, 0),
  };
}

const readEvidence = (evidence) => (fs.existsSync(evidence) ? fs.readFileSync(evidence, "utf8") : "");

const bugShown = (bug) => !!bug && bug.passed > 0 && bug.failed === 0;
const repairFails = (repaired) => !!repaired && repaired.failed > 0;
/** The bug passed the exhibit's spec and the documented repair failed it. */
const isShown = (r) => !!r && bugShown(r.bug) && repairFails(r.repaired);

/** One exhibit: what it is, whether its bug was shown, and Pleris's verdict on it. */
function exhibitOf(ex, r) {
  const verdict = r?.verdict;
  return {
    id: ex.id,
    leaf: ex.leaf,
    title: ex.title,
    source: `${ex.source.ref} (${ex.source.kind})`,
    stack: ex.repro.stack,
    shown: isShown(r),
    pleris: verdict ?? "not recorded",
    refused: /^refused/.test(verdict ?? ""),
  };
}

/** The exhibit in `dir` of the museum, or null for a directory that holds none. */
function readExhibit(museum, dir, recorded) {
  const file = path.join(museum, dir, "exhibit.json");
  if (!fs.existsSync(file)) return null;
  const ex = JSON.parse(fs.readFileSync(file, "utf8"));
  return exhibitOf(ex, recorded[ex.id] ?? null);
}

export function collect({ museum = MUSEUM, evidence = EVIDENCE } = {}) {
  const text = readEvidence(evidence);
  const recorded = parseEvidence(text);
  const dirs = fs.readdirSync(museum).sort();
  const exhibits = dirs.map((dir) => readExhibit(museum, dir, recorded)).filter(Boolean);
  return { meta: metaOf(text), exhibits };
}

const heading = (meta) =>
  `the museum's exhibits (ADR-0342), as just e14-museum recorded them: commit ${meta.commit}${meta.ci ? `, ${meta.ci}` : ""}; ${meta.summary ?? "no summary"}`;

const shownIn = (exhibits, stack) => exhibits.filter((e) => e.stack === stack).filter((e) => e.shown);

/** What a framework lets ship, each exhibit shown happening in it. */
function letsShip(stack, exhibits) {
  const shown = shownIn(exhibits, stack);
  const list = shown.map((e) => `${e.id} (${e.title})`).join("; ") || "none";
  return `  ${LABEL[stack].padEnd(10)} lets ship ${shown.length} of the ${exhibits.length}, each shown happening on its built store, its checker accepting it: ${list}`;
}

export function render({ meta, exhibits }) {
  const refused = exhibits.filter((e) => e.refused);
  const lines = [
    heading(meta),
    ...["next-react", "sveltekit"].map((stack) => letsShip(stack, exhibits)),
    `  (an exhibit shown in one framework is not reproduced in the other: neither claimed to let it ship nor to refuse it)`,
    `  ${"Pleris".padEnd(10)} refuses ${refused.length} of the ${exhibits.length}: ${exhibits.map((e) => `${e.id}: ${e.pleris}`).join("; ")}`,
    `  not claimed: that five exhibits sample the census (ADR-0342); each exhibit's source and its kind: ${exhibits.map((e) => `${e.leaf} ${e.source}`).join("; ")}`,
  ];
  return `${lines.join("\n")}\n`;
}

/** The command line: the exhibits and what each stack lets ship; exit 1 where an exhibit has no recorded verdict. */
function cli(argv) {
  const result = collect();
  const json = pathAfter(argv, "--json");
  if (json) fs.writeFileSync(json, `${JSON.stringify(result, null, 1)}\n`);
  process.stdout.write(render(result));
  if (!result.exhibits.length || result.exhibits.some((e) => e.pleris === "not recorded")) {
    process.stdout.write("museum-classes: REFUSED: an exhibit has no recorded verdict (run just e14-museum)\n");
    process.exit(1);
  }
}

if (isMain(import.meta.url)) cli(process.argv.slice(2));
