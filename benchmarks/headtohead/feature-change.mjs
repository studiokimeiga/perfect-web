#!/usr/bin/env node
// The same feature change, made in each stack (the head-to-head, ADR-XXXX):
// what each pull request is, and what in it a reviewer must verify by hand.
//
//   node benchmarks/headtohead/feature-change.mjs [--json out.json] [--no-checks]
//
// The changes are the benchmark's twelve tasks (benchmarks/tasks/T01-T12):
// each asks for one feature or repair of ADR-0120's store, and each has the
// change made in every stack (`reference/<stack>.patch`), written by this
// project's sessions for the benchmark, graded by the task's hidden tests,
// and a plausible wrong version of it (`unsafe/<stack>.patch`), the change a
// hurried author might make. For each task and stack:
//
// - **the pull request**: the reference patch's lines added and removed,
//   files touched and languages (by extension), after the task's setup;
// - **what the stack's checker proves**: the stack's own checker (`pw
//   check`, `tsc --noEmit`, `svelte-check`) run on the baseline with the
//   setup and the reference (it must check clean), and with the setup and
//   the unsafe change: refused (a code, a first error) or accepted. An
//   accepted unsafe change is what a reviewer must find by reading, or a test
//   someone wrote must catch: the task's hidden test did, in the harness's
//   controls (`docs/evidence/E14/harness-T*.txt`, cited with their commit).
//
// Each change is applied in place to the worktree's baseline, on the
// dependencies the workspace installed (as `benchmarks/museum/museum.mjs`
// does), and taken back after, whatever happens; a baseline git reports
// changed is refused. The checkers' runs leave every baseline as it was, but
// with new modification times: build the frameworks' stores after this.

import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { REPO } from "./lib.mjs";

const TASKS = path.join(REPO, "benchmarks/tasks");
const STACKS = {
  pleris: { label: "Pleris", dir: "benchmarks/baselines/pleris", checker: "pw check" },
  "next-react": { label: "Next.js", dir: "benchmarks/baselines/next-react", checker: "tsc --noEmit" },
  sveltekit: { label: "SvelteKit", dir: "benchmarks/baselines/sveltekit", checker: "svelte-check" },
};

/** What each task's unsafe change gets wrong, in the brief's checklist's
 * terms (authorization, cache invalidation, loading and error states, types
 * across the network boundary) and the others the tasks raise. */
export const CLASS = {
  T01: "loading and error states: an optimistic count that is not taken back when the add fails",
  T02: "shared reads: concurrent asks of a slow source not shared",
  T03: "authorization and privacy: one customer's cart lines kept where another's request reads them",
  T04: "types across the network boundary: a new order state the page does not handle",
  T05: "loading and error states: a slow section that holds up the page",
  T06: "accessibility: a form's error not tied to its field or announced",
  T07: "races and stale answers: an answer for a category the customer moved past is shown",
  T08: "retries: one press applied twice when its request is delivered twice",
  T09: "cache invalidation: a kept answer older than the freshness asked for",
  T10: "loading and error states: a failing estimate that breaks or holds up the page",
  T11: "accessibility: a confirmation that is not a modal dialog to keyboard and screen reader",
  T12: "authorization and privacy: a cached cart served to another session",
};

const LANGUAGE = {
  ".pw": "Pleris",
  ".ts": "TypeScript",
  ".tsx": "TSX",
  ".svelte": "Svelte",
  ".js": "JavaScript",
  ".css": "CSS",
  ".rs": "Rust",
  ".json": "JSON",
  ".html": "HTML",
};

/**
 * A unified diff's files, lines added and removed, and languages. Each
 * hunk's lines are read by the counts its `@@ -a,b +c,d @@` header gives,
 * so a content line that begins `--- ` or `+++ ` is content, not a header.
 */
export function diffStats(text) {
  const files = new Map();
  let current = null;
  let createdNext = false;
  let oldLeft = 0;
  let newLeft = 0;
  for (const line of text.split("\n")) {
    if (oldLeft > 0 || newLeft > 0) {
      const f = files.get(current);
      if (line.startsWith("+")) {
        f.added++;
        newLeft--;
      } else if (line.startsWith("-")) {
        f.removed++;
        oldLeft--;
      } else if (line.startsWith("\\")) {
        // "\ No newline at end of file": no line of either side.
      } else {
        oldLeft--;
        newLeft--;
      }
      continue;
    }
    const hunk = /^@@ -\d+(?:,(\d+))? \+\d+(?:,(\d+))? @@/.exec(line);
    if (hunk && current) {
      oldLeft = hunk[1] === undefined ? 1 : Number(hunk[1]);
      newLeft = hunk[2] === undefined ? 1 : Number(hunk[2]);
      continue;
    }
    if (line.startsWith("--- ")) {
      createdNext = line.slice(4).trim() === "/dev/null";
      continue;
    }
    if (line.startsWith("+++ ")) {
      current = line.slice(4).replace(/^b\//, "").trim();
      if (!files.has(current)) files.set(current, { added: 0, removed: 0, created: createdNext });
      createdNext = false;
    }
  }
  const languages = [...new Set([...files.keys()].map((f) => LANGUAGE[path.extname(f)] ?? path.extname(f)))].sort();
  return {
    files: files.size,
    created: [...files.values()].filter((f) => f.created).length,
    added: [...files.values()].reduce((n, f) => n + f.added, 0),
    removed: [...files.values()].reduce((n, f) => n + f.removed, 0),
    languages,
    paths: [...files.keys()],
  };
}

function sh(cmd, args, opts = {}) {
  const r = spawnSync(cmd, args, { encoding: "utf8", maxBuffer: 64 << 20, ...opts });
  return { ok: r.status === 0, status: r.status, out: `${r.stdout ?? ""}${r.stderr ?? ""}` };
}

function dirty(dir) {
  return sh("git", ["status", "--porcelain", "--untracked-files=all", "--", dir], { cwd: REPO }).out.trim();
}

/** Every tracked file's modification time under `dir`. */
function times(dir) {
  const listed = sh("git", ["ls-files", "--", dir], { cwd: REPO }).out.split("\n").filter(Boolean);
  return listed.map((f) => [path.join(REPO, f), fs.statSync(path.join(REPO, f))]);
}

/** The baseline as git has it, every file's time as it was: the same bytes
 * at the same times, so a build made before is still the build of these
 * sources (`validate.mjs`'s build rule reads the times). */
function restore(dir, before) {
  sh("git", ["checkout", "--", dir], { cwd: REPO });
  sh("git", ["clean", "-fdq", "--", dir], { cwd: REPO });
  for (const [file, st] of before) fs.utimesSync(file, st.atime, st.mtime);
}

function apply(patch, dir) {
  const r = sh("git", ["apply", `--directory=${dir}`, patch], { cwd: REPO });
  if (!r.ok) throw new Error(`git apply ${path.relative(REPO, patch)}: ${r.out.trim()}`);
}

const ANSI = /\x1b\[[0-9;]*[A-Za-z]/g;

/** The stack's checker on its baseline as it stands: clean, or its first error. */
export function runChecker(stack) {
  const dir = path.join(REPO, STACKS[stack].dir);
  if (stack === "pleris") {
    const pw = path.join(process.env.CARGO_TARGET_DIR ?? path.join(REPO, "target"), "debug/pw");
    const files = [
      ...["packages/pw-std", "packages/pw-platform-web"].flatMap((d) =>
        fs.readdirSync(path.join(REPO, d)).filter((f) => f.endsWith(".pw")).map((f) => path.join(REPO, d, f)),
      ),
      path.join(dir, "domain.pw"),
      ...["lib", "store"].flatMap((d) =>
        fs.readdirSync(path.join(dir, d)).filter((f) => f.endsWith(".pw")).map((f) => path.join(dir, d, f)),
      ),
    ];
    const r = sh(pw, ["check", ...files]);
    const error = r.out.replace(ANSI, "").split("\n").find((l) => /^error/.test(l.trim()));
    return { clean: r.ok, error: r.ok ? null : (error ?? r.out.trim().split("\n")[0]).trim() };
  }
  if (stack === "next-react") {
    const r = sh("pnpm", ["--silent", "typecheck"], { cwd: dir });
    const error = r.out.replace(ANSI, "").split("\n").find((l) => /error TS\d+/.test(l));
    return { clean: r.ok, error: r.ok ? null : (error ?? r.out.trim().split("\n")[0]).trim() };
  }
  const r = sh("pnpm", ["--silent", "typecheck", "--output", "machine"], { cwd: dir });
  const summary = /(\d+) ERRORS (\d+) WARNINGS/.exec(r.out);
  const errors = summary ? Number(summary[1]) : null;
  const error = r.out.split("\n").find((l) => /\bERROR\b/.test(l) && !/ERRORS/.test(l));
  return {
    clean: r.ok && errors === 0,
    error: r.ok && errors === 0 ? null : (error ?? (summary ? summary[0] : r.out.trim().split("\n")[0])).trim(),
  };
}

/** The checker on the baseline with the task's setup and one change. */
function checkWith(task, stack, change) {
  const s = STACKS[stack];
  const changed = dirty(s.dir);
  if (changed) throw new Error(`${s.dir} has changes git reports: refusing to patch it in place\n${changed}`);
  const before = times(s.dir);
  try {
    const setup = path.join(TASKS, task, "setup", `${stack}.patch`);
    if (fs.existsSync(setup)) apply(setup, s.dir);
    apply(path.join(TASKS, task, change, `${stack}.patch`), s.dir);
    return runChecker(stack);
  } finally {
    restore(s.dir, before);
  }
}

/** Where the harness's controls caught each task's unsafe change, per stack,
 * as its evidence records it (the stage, and the evidence's commit). */
export function harnessCaught(evidenceDir = path.join(REPO, "docs/evidence/E14")) {
  const out = {};
  for (const f of fs.existsSync(evidenceDir) ? fs.readdirSync(evidenceDir) : []) {
    const m = /^harness-(T\d+)\.txt$/.exec(f);
    if (!m) continue;
    const text = fs.readFileSync(path.join(evidenceDir, f), "utf8");
    const commit = /^commit: ([0-9a-f]{8})/m.exec(text)?.[1] ?? null;
    for (const line of text.split("\n")) {
      const c = /^(T\d+) (\S+): .*unsafe ok \(caught at (\w+)\)/.exec(line);
      if (c) (out[c[1]] ??= {})[c[2]] = { stage: c[3], commit };
    }
  }
  return out;
}

export function collect({ checks = true } = {}) {
  const tasks = fs.readdirSync(TASKS).filter((d) => /^T\d+-/.test(d)).sort();
  const caught = harnessCaught();
  const rows = [];
  for (const task of tasks) {
    const id = task.split("-")[0];
    const meta = JSON.parse(fs.readFileSync(path.join(TASKS, task, "task.json"), "utf8"));
    const row = { id, task, title: meta.title, class: CLASS[id] ?? "", stacks: {} };
    for (const stack of Object.keys(STACKS)) {
      const ref = path.join(TASKS, task, "reference", `${stack}.patch`);
      if (!fs.existsSync(ref)) continue;
      const cell = { pr: diffStats(fs.readFileSync(ref, "utf8")), harness: caught[id]?.[stack] ?? null };
      if (checks) {
        cell.reference = checkWith(task, stack, "reference");
        cell.unsafe = checkWith(task, stack, "unsafe");
      }
      row.stacks[stack] = cell;
      process.stderr.write(`  ${id} ${stack}: ${cell.pr.added}+/${cell.pr.removed}-${checks ? `; reference ${cell.reference.clean ? "clean" : "REFUSED"}; unsafe ${cell.unsafe.clean ? "accepted" : "refused"}` : ""}\n`);
    }
    rows.push(row);
  }
  return { checkers: Object.fromEntries(Object.entries(STACKS).map(([k, v]) => [k, v.checker])), rows };
}

export function render(result) {
  const lines = [];
  lines.push("each task's change in each stack: the pull request (reference patch, after the task's setup) and what the stack's checker says of it and of its unsafe version");
  lines.push("  checkers: " + Object.entries(result.checkers).map(([k, v]) => `${STACKS[k].label} ${v}`).join("; "));
  lines.push("  stage that caught the unsafe version in the harness's controls (docs/evidence/E14/harness-T*.txt) is shown with that evidence's commit");
  const totals = Object.fromEntries(Object.keys(STACKS).map((s) => [s, { added: 0, removed: 0, files: 0, languages: new Set(), refused: 0, accepted: 0, cleanRefs: 0, n: 0 }]));
  for (const row of result.rows) {
    lines.push(`  ${row.id} ${row.title} [${row.class}]`);
    for (const [stack, c] of Object.entries(row.stacks)) {
      const t = totals[stack];
      t.n++;
      t.added += c.pr.added;
      t.removed += c.pr.removed;
      t.files += c.pr.files;
      c.pr.languages.forEach((l) => t.languages.add(l));
      let checker = "";
      if (c.reference) {
        if (c.reference.clean) t.cleanRefs++;
        if (c.unsafe.clean) t.accepted++;
        else t.refused++;
        checker = `reference ${c.reference.clean ? "checks clean" : `REFUSED: ${c.reference.error}`}; unsafe ${c.unsafe.clean ? "ACCEPTED: a reviewer must find it" : `refused: ${c.unsafe.error}`}`;
      }
      const harness = c.harness ? `; harness caught it at ${c.harness.stage} (${c.harness.commit})` : "";
      lines.push(
        `    ${STACKS[stack].label.padEnd(10)} +${String(c.pr.added).padEnd(3)} -${String(c.pr.removed).padEnd(3)} ${c.pr.files} file(s)${c.pr.created ? ` (${c.pr.created} new)` : ""}, ${c.pr.languages.join("+")}; ${checker}${harness}`,
      );
    }
  }
  lines.push("  totals over the tasks:");
  const checked = result.rows.some((r) => Object.values(r.stacks).some((c) => c.reference));
  for (const [stack, t] of Object.entries(totals)) {
    lines.push(
      `    ${STACKS[stack].label.padEnd(10)} ${t.n} changes: +${t.added} -${t.removed} lines, ${t.files} file(s) touched, languages ${[...t.languages].sort().join(", ")}` +
        (checked
          ? `; references clean ${t.cleanRefs}/${t.n}; unsafe versions refused by the checker ${t.refused}/${t.n}, left to a reviewer ${t.accepted}/${t.n}`
          : "; checkers not run (--no-checks)"),
    );
  }
  return `${lines.join("\n")}\n`;
}

if (process.argv[1] && path.resolve(process.argv[1]) === path.resolve(new URL(import.meta.url).pathname)) {
  const argv = process.argv.slice(2);
  const json = argv.includes("--json") ? path.resolve(argv[argv.indexOf("--json") + 1]) : null;
  const result = collect({ checks: !argv.includes("--no-checks") });
  if (json) fs.writeFileSync(json, `${JSON.stringify(result, null, 1)}\n`);
  process.stdout.write(render(result));
  const bad = result.rows.flatMap((r) => Object.entries(r.stacks).filter(([, c]) => c.reference && !c.reference.clean).map(([s]) => `${r.id} ${s}`));
  if (bad.length) {
    process.stdout.write(`feature-change: REFUSED: a reference change does not check clean: ${bad.join(", ")}\n`);
    process.exit(1);
  }
}
