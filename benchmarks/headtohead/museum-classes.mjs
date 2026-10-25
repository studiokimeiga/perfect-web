#!/usr/bin/env node
// The museum's bug classes each stack lets ship (the head-to-head, ADR-XXXX):
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
import { REPO } from "./lib.mjs";

const MUSEUM = path.join(REPO, "benchmarks/museum");
const EVIDENCE = path.join(REPO, "docs/evidence/E14/museum.txt");
const LABEL = { "next-react": "Next.js", sveltekit: "SvelteKit", pleris: "Pleris" };

/** Each exhibit's block in the evidence: its title, the bug's and the
 * repair's spec counts, and the verdict. */
export function parseEvidence(text) {
  const out = {};
  let current = null;
  for (const line of text.split("\n")) {
    const head = /^== ([A-Z]\d+-[a-z0-9-]+): (.*)$/.exec(line);
    if (head) {
      current = out[head[1]] = { title: head[2], bug: null, repaired: null, verdict: null };
      continue;
    }
    if (!current) continue;
    const bug = /^\s+([a-z-]+), the bug: (\d+) passed, (\d+) failed/.exec(line);
    if (bug) current.bug = { stack: bug[1], passed: Number(bug[2]), failed: Number(bug[3]) };
    const fixed = /^\s+([a-z-]+), repaired: (\d+) passed, (\d+) failed/.exec(line);
    if (fixed) current.repaired = { stack: fixed[1], passed: Number(fixed[2]), failed: Number(fixed[3]) };
    const verdict = /^\s+=> (.*)$/.exec(line);
    if (verdict) current.verdict = verdict[1];
  }
  return out;
}

export function collect() {
  const text = fs.existsSync(EVIDENCE) ? fs.readFileSync(EVIDENCE, "utf8") : "";
  const meta = {
    commit: /^commit: (\S+)/m.exec(text)?.[1] ?? null,
    ci: /^ci: (\S+)/m.exec(text)?.[1] ?? null,
    summary: /^museum: .*$/m.exec(text)?.[0] ?? null,
  };
  const recorded = parseEvidence(text);
  const exhibits = [];
  for (const dir of fs.readdirSync(MUSEUM).sort()) {
    const file = path.join(MUSEUM, dir, "exhibit.json");
    if (!fs.existsSync(file)) continue;
    const ex = JSON.parse(fs.readFileSync(file, "utf8"));
    const r = recorded[ex.id] ?? null;
    const shown = !!(r?.bug && r.bug.passed > 0 && r.bug.failed === 0 && r.repaired && r.repaired.failed > 0);
    exhibits.push({
      id: ex.id,
      leaf: ex.leaf,
      title: ex.title,
      source: `${ex.source.ref} (${ex.source.kind})`,
      stack: ex.repro.stack,
      shown,
      pleris: r?.verdict ?? "not recorded",
      refused: /^refused/.test(r?.verdict ?? ""),
    });
  }
  return { meta, exhibits };
}

export function render({ meta, exhibits }) {
  const lines = [];
  lines.push(`the museum's exhibits (ADR-0342), as just e14-museum recorded them: commit ${meta.commit}${meta.ci ? `, ${meta.ci}` : ""}; ${meta.summary ?? "no summary"}`);
  for (const stack of ["next-react", "sveltekit"]) {
    const mine = exhibits.filter((e) => e.stack === stack);
    const shown = mine.filter((e) => e.shown);
    lines.push(`  ${LABEL[stack].padEnd(10)} lets ship ${shown.length} of the ${exhibits.length}, each shown happening on its built store, its checker accepting it: ${shown.map((e) => `${e.id} (${e.title})`).join("; ") || "none"}`);
  }
  lines.push(`  (an exhibit shown in one framework is not reproduced in the other: neither claimed to let it ship nor to refuse it)`);
  const refused = exhibits.filter((e) => e.refused);
  lines.push(`  ${"Pleris".padEnd(10)} refuses ${refused.length} of the ${exhibits.length}: ${exhibits.map((e) => `${e.id}: ${e.pleris}`).join("; ")}`);
  lines.push(`  not claimed: that five exhibits sample the census (ADR-0342); each exhibit's source and its kind: ${exhibits.map((e) => `${e.leaf} ${e.source}`).join("; ")}`);
  return `${lines.join("\n")}\n`;
}

if (process.argv[1] && path.resolve(process.argv[1]) === path.resolve(new URL(import.meta.url).pathname)) {
  const argv = process.argv.slice(2);
  const result = collect();
  if (argv.includes("--json")) fs.writeFileSync(path.resolve(argv[argv.indexOf("--json") + 1]), `${JSON.stringify(result, null, 1)}\n`);
  process.stdout.write(render(result));
  if (!result.exhibits.length || result.exhibits.some((e) => e.pleris === "not recorded")) {
    process.stdout.write("museum-classes: REFUSED: an exhibit has no recorded verdict (run just e14-museum)\n");
    process.exit(1);
  }
}
