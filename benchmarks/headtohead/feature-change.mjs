#!/usr/bin/env node
// The same feature change, made in each stack (the head-to-head, ADR-0392):
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
//   the unsafe change: refused (a code, a first error) or accepted. Its
//   warnings are read too: an accepted unsafe change that draws a warning
//   its reference does not (svelte-check's accessibility warnings, say) was
//   warned of, and counts with the refusals as the checker's catch. An
//   unsafe change accepted with no new warning is what a reviewer must find
//   by reading, or a test someone wrote must catch: the task's hidden test
//   did, in the harness's controls (`docs/evidence/E14/harness-T*.txt`,
//   cited with their commit).
//
// Each change is applied in place to the worktree's baseline, on the
// dependencies the workspace installed (as `benchmarks/museum/museum.mjs`
// does), and taken back after, whatever happens; a baseline git reports
// changed is refused. The checkers' runs leave every baseline as it was, but
// with new modification times: build the frameworks' stores after this.

import fs from "node:fs";
import path from "node:path";
import { REPO, isMain, pathAfter, sh } from "./lib.mjs";

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

const HUNK = /^@@ -\d+(?:,(\d+))? \+\d+(?:,(\d+))? @@/;

/** The lines a hunk header counts for one side: one where it gives no count. */
const hunkSide = (count) => (count === undefined ? 1 : Number(count));

/** A line inside a hunk: an added or a removed line of the file, or one both
 * sides have. ("\ No newline at end of file" is a line of neither side.) */
function hunkLine(s, line) {
  const f = s.files.get(s.current);
  if (line.startsWith("+")) {
    f.added++;
    s.newLeft--;
  } else if (line.startsWith("-")) {
    f.removed++;
    s.oldLeft--;
  } else if (!line.startsWith("\\")) {
    s.oldLeft--;
    s.newLeft--;
  }
}

/** A `+++ ` line: the file the following hunks change, new to the count unless seen. */
function fileHeader(s, line) {
  s.current = line.slice(4).replace(/^b\//, "").trim();
  if (!s.files.has(s.current)) s.files.set(s.current, { added: 0, removed: 0, created: s.createdNext });
  s.createdNext = false;
}

/** A `--- ` line says whether the file that follows is created (from /dev/null); a `+++ ` line names it. */
function headerLine(s, line) {
  if (line.startsWith("--- ")) s.createdNext = line.slice(4).trim() === "/dev/null";
  else if (line.startsWith("+++ ")) fileHeader(s, line);
}

/** A line outside a hunk: a hunk header (of a file named before it), or a file header. */
function outsideHunk(s, line) {
  const hunk = HUNK.exec(line);
  if (hunk && s.current) {
    s.oldLeft = hunkSide(hunk[1]);
    s.newLeft = hunkSide(hunk[2]);
  } else headerLine(s, line);
}

function readLine(s, line) {
  if (s.oldLeft > 0 || s.newLeft > 0) hunkLine(s, line);
  else outsideHunk(s, line);
}

const languageOf = (file) => LANGUAGE[path.extname(file)] ?? path.extname(file);

/** The files a diff changes, summed. */
function summarize(files) {
  const counts = [...files.values()];
  return {
    files: files.size,
    created: counts.filter((f) => f.created).length,
    added: counts.reduce((n, f) => n + f.added, 0),
    removed: counts.reduce((n, f) => n + f.removed, 0),
    languages: [...new Set([...files.keys()].map(languageOf))].sort(),
    paths: [...files.keys()],
  };
}

/**
 * A unified diff's files, lines added and removed, and languages. Each
 * hunk's lines are read by the counts its `@@ -a,b +c,d @@` header gives,
 * so a content line that begins `--- ` or `+++ ` is content, not a header.
 */
export function diffStats(text) {
  const s = { files: new Map(), current: null, createdNext: false, oldLeft: 0, newLeft: 0 };
  for (const line of text.split("\n")) readLine(s, line);
  return summarize(s.files);
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

/** A warning without its place (a line and column move as a change adds
 * lines above it), so that the same warning is the same on both sides. */
const unplaced = (w) => w.replace(/\b\d+:\d+\b/g, "").replace(/\s+/g, " ").trim();

/** The warnings `unsafe` draws that `reference` does not, each once. */
export function newWarnings(reference = [], unsafe = []) {
  const known = new Set(reference.map(unplaced));
  return [...new Set(unsafe.map(unplaced))].filter((w) => !known.has(w));
}

const withoutColour = (out) => out.replace(ANSI, "");
const firstLine = (out) => out.trim().split("\n")[0];

/** A checker's first error as reported: the line found, else the output's first, trimmed. */
const reported = (found, fallback) => (found ?? fallback).trim();

/** `pw check` on the standard library, the platform and the baseline's own modules. */
function plerisFiles(dir) {
  const modules = (d) => fs.readdirSync(d).filter((f) => f.endsWith(".pw")).map((f) => path.join(d, f));
  return [
    ...["packages/pw-std", "packages/pw-platform-web"].flatMap((d) => modules(path.join(REPO, d))),
    path.join(dir, "domain.pw"),
    ...["lib", "store"].flatMap((d) => modules(path.join(dir, d))),
  ];
}

const plerisBinary = () => path.join(process.env.CARGO_TARGET_DIR ?? path.join(REPO, "target"), "debug/pw");

/** What `pw check` said: clean, or its first `error` line; its `warning` lines. */
function plerisVerdict(r) {
  const lines = withoutColour(r.out).split("\n").map((l) => l.trim());
  const found = lines.find((l) => /^error/.test(l));
  const warnings = lines.filter((l) => /^warning/.test(l));
  return { clean: r.ok, error: r.ok ? null : reported(found, firstLine(r.out)), warnings };
}

/** What `tsc --noEmit` said: clean, or its first `error TSnnnn` line. `tsc` has errors only. */
function tscVerdict(r) {
  const found = withoutColour(r.out).split("\n").find((l) => /error TS\d+/.test(l));
  return { clean: r.ok, error: r.ok ? null : reported(found, firstLine(r.out)), warnings: [] };
}

const SVELTE_SUMMARY = /(\d+) ERRORS (\d+) WARNINGS/;
const isSvelteError = (line) => /\bERROR\b/.test(line) && !/ERRORS/.test(line);
const errorCount = (summary) => (summary ? Number(summary[1]) : null);
const summaryOrFirstLine = (summary, out) => (summary ? summary[0] : firstLine(out));

/** Its machine output: `<time> WARNING "<file>" <line>:<column> "<message>"`; the warnings without the time. */
const svelteWarnings = (lines) => lines.filter((l) => /^\d+ WARNING /.test(l)).map((l) => l.replace(/^\d+ /, "").trim());

/** What `svelte-check --output machine` said: clean where it ran and counted no errors; else its first ERROR line. */
function svelteCheckVerdict(r) {
  const summary = SVELTE_SUMMARY.exec(r.out);
  const lines = r.out.split("\n");
  const clean = r.ok && errorCount(summary) === 0;
  const found = lines.find(isSvelteError);
  return { clean, error: clean ? null : reported(found, summaryOrFirstLine(summary, r.out)), warnings: svelteWarnings(lines) };
}

/** The stack's checker on its baseline as it stands: clean, or its first error. */
export function runChecker(stack, shell = sh) {
  const dir = path.join(REPO, STACKS[stack].dir);
  if (stack === "pleris") return plerisVerdict(shell(plerisBinary(), ["check", ...plerisFiles(dir)]));
  if (stack === "next-react") return tscVerdict(shell("pnpm", ["--silent", "typecheck"], { cwd: dir }));
  return svelteCheckVerdict(shell("pnpm", ["--silent", "typecheck", "--output", "machine"], { cwd: dir }));
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

const HARNESS_FILE = /^harness-(T\d+)\.txt$/;
const CAUGHT = /^(T\d+) (\S+): .*unsafe ok \(caught at (\w+)\)/;
const evidenceCommit = (text) => /^commit: ([0-9a-f]{8})/m.exec(text)?.[1] ?? null;
const evidenceFiles = (dir) => (fs.existsSync(dir) ? fs.readdirSync(dir) : []);

/** A task's stage and commit, filed under its id and stack. */
const fileCaught = (out, c, commit) => ((out[c[1]] ??= {})[c[2]] = { stage: c[3], commit });

/** One harness evidence file: each line that says an unsafe change was caught, into `out`. */
function readHarness(out, text) {
  const commit = evidenceCommit(text);
  for (const line of text.split("\n")) {
    const c = CAUGHT.exec(line);
    if (c) fileCaught(out, c, commit);
  }
}

/** Where the harness's controls caught each task's unsafe change, per stack,
 * as its evidence records it (the stage, and the evidence's commit). */
export function harnessCaught(evidenceDir = path.join(REPO, "docs/evidence/E14")) {
  const out = {};
  for (const f of evidenceFiles(evidenceDir)) {
    if (HARNESS_FILE.test(f)) readHarness(out, fs.readFileSync(path.join(evidenceDir, f), "utf8"));
  }
  return out;
}

const taskDirs = (tasksDir) => fs.readdirSync(tasksDir).filter((d) => /^T\d+-/.test(d)).sort();

/** The checkers' verdicts on a task's reference change and its unsafe one,
 * and the warnings the unsafe one draws that the reference does not. */
function checkCell(task, stack, cell) {
  cell.reference = checkWith(task, stack, "reference");
  cell.unsafe = checkWith(task, stack, "unsafe");
  cell.unsafe.newWarnings = newWarnings(cell.reference.warnings, cell.unsafe.warnings);
}

const checksNote = (cell) => `; reference ${cell.reference.clean ? "clean" : "REFUSED"}; unsafe ${cell.unsafe.clean ? "accepted" : "refused"}`;

/** The line collect writes to stderr as a cell is done. */
const progress = (id, stack, cell, checks) =>
  `  ${id} ${stack}: ${cell.pr.added}+/${cell.pr.removed}-${checks ? checksNote(cell) : ""}\n`;

const harnessOf = (caught, id, stack) => caught[id]?.[stack] ?? null;

/** A stack's cell of a task, or null where the task has no reference patch for it. */
function cellOf(ctx, task, id, stack) {
  const ref = path.join(ctx.tasksDir, task, "reference", `${stack}.patch`);
  if (!fs.existsSync(ref)) return null;
  const cell = { pr: diffStats(fs.readFileSync(ref, "utf8")), harness: harnessOf(ctx.caught, id, stack) };
  if (ctx.checks) checkCell(task, stack, cell);
  process.stderr.write(progress(id, stack, cell, ctx.checks));
  return cell;
}

/** A task's row: its title and class, and a cell for each stack that has a reference patch. */
function rowOf(ctx, task) {
  const id = task.split("-")[0];
  const meta = JSON.parse(fs.readFileSync(path.join(ctx.tasksDir, task, "task.json"), "utf8"));
  const row = { id, task, title: meta.title, class: CLASS[id] ?? "", stacks: {} };
  for (const stack of Object.keys(STACKS)) {
    const cell = cellOf(ctx, task, id, stack);
    if (cell) row.stacks[stack] = cell;
  }
  return row;
}

export function collect({ checks = true, tasksDir = TASKS, evidenceDir } = {}) {
  const tasks = taskDirs(tasksDir);
  const ctx = { checks, tasksDir, caught: harnessCaught(evidenceDir) };
  const checkers = Object.fromEntries(Object.entries(STACKS).map(([k, v]) => [k, v.checker]));
  return { checkers, rows: tasks.map((task) => rowOf(ctx, task)) };
}

const newTotals = () => ({ added: 0, removed: 0, files: 0, languages: new Set(), refused: 0, warned: 0, accepted: 0, cleanRefs: 0, n: 0 });

/** The warnings an accepted unsafe version drew that its reference did not. */
const warningsOf = (unsafe) => unsafe.newWarnings ?? [];
const wasWarned = (unsafe) => unsafe.clean && warningsOf(unsafe).length > 0;

/** What the checker did with a cell's unsafe version, into the totals. */
function tallyChecks(t, c) {
  if (c.reference.clean) t.cleanRefs++;
  if (!c.unsafe.clean) t.refused++;
  else if (wasWarned(c.unsafe)) t.warned++;
  else t.accepted++;
}

/** A cell into its stack's totals. */
function tally(t, c) {
  t.n++;
  t.added += c.pr.added;
  t.removed += c.pr.removed;
  t.files += c.pr.files;
  c.pr.languages.forEach((l) => t.languages.add(l));
  if (c.reference) tallyChecks(t, c);
}

function unsafeText(unsafe) {
  if (!unsafe.clean) return `refused: ${unsafe.error}`;
  if (wasWarned(unsafe)) return `accepted, but WARNED of: ${unsafe.newWarnings[0]}`;
  return "ACCEPTED, with no warning its reference lacks: a reviewer must find it";
}

/** What the checker said of a cell's reference change and of its unsafe version: nothing, where it did not run. */
function checkerText(c) {
  if (!c.reference) return "";
  const reference = c.reference.clean ? "checks clean" : `REFUSED: ${c.reference.error}`;
  return `reference ${reference}; unsafe ${unsafeText(c.unsafe)}`;
}

/** One stack's line of a task: the change's size, and what the checker and the harness said. */
function stackLine(stack, c) {
  const { pr } = c;
  const created = pr.created ? ` (${pr.created} new)` : "";
  const harness = c.harness ? `; harness caught it at ${c.harness.stage} (${c.harness.commit})` : "";
  return `    ${STACKS[stack].label.padEnd(10)} +${String(pr.added).padEnd(3)} -${String(pr.removed).padEnd(3)} ${pr.files} file(s)${created}, ${pr.languages.join("+")}; ${checkerText(c)}${harness}`;
}

/** A task's lines, its cells counted into the totals as they are written. */
function rowLines(row, totals) {
  const lines = [`  ${row.id} ${row.title} [${row.class}]`];
  for (const [stack, c] of Object.entries(row.stacks)) {
    tally(totals[stack], c);
    lines.push(stackLine(stack, c));
  }
  return lines;
}

/** The tasks by class of mistake (a task's class before its colon). */
function classesOf(rows) {
  const classes = new Map();
  for (const row of rows) {
    const name = row.class.split(":")[0];
    if (!classes.has(name)) classes.set(name, []);
    classes.get(name).push(row);
  }
  return classes;
}

/** A stack's unsafe versions among the rows, as [task id, cell]. */
const unsafeCells = (rows, stack) => rows.map((r) => [r.id, r.stacks[stack]]).filter(([, c]) => c?.unsafe);

/** An unsafe version the checker caught: refused, or warned of. */
const caught = (unsafe) => !unsafe.clean || warningsOf(unsafe).length > 0;

const refusalCode = (error) => (/\[(PW\d+)\]/.exec(error) ?? [])[1] ?? "refused";
const proofOf = ([id, c]) => `${id} ${c.unsafe.clean ? "a warning" : refusalCode(c.unsafe.error)}`;

/** What a stack's checker proves of the unsafe versions of a class's tasks, and what is left to a reviewer. */
function stackProof(stack, rows) {
  const cells = unsafeCells(rows, stack);
  const proved = cells.filter(([, c]) => caught(c.unsafe)).map(proofOf);
  const left = cells.filter(([, c]) => !caught(c.unsafe)).map(([id]) => id);
  const checker = proved.length ? `checker proves ${proved.join(", ")}` : "checker proves none";
  return `${STACKS[stack].label}: ${checker}${left.length ? `; reviewer: ${left.join(", ")}` : ""}`;
}

const classLine = ([name, rows]) =>
  `    ${name} (${rows.map((r) => r.id).join(", ")}): ${Object.keys(STACKS).map((stack) => stackProof(stack, rows)).join(" | ")}`;

/** The reviewer's checklist: each class of mistake, the tasks that carry
 * it, and who catches the unsafe version in each stack. */
const checklist = (rows) => [
  "  what a reviewer must verify by hand, by class of mistake (the tasks' unsafe versions):",
  ...[...classesOf(rows)].map(classLine),
];

/** A stack's totals over the tasks, with the checkers' where they ran. */
function totalLine(stack, t, checked) {
  const head = `    ${STACKS[stack].label.padEnd(10)} ${t.n} changes: +${t.added} -${t.removed} lines, ${t.files} file(s) touched, languages ${[...t.languages].sort().join(", ")}`;
  const tail = checked
    ? `; references clean ${t.cleanRefs}/${t.n}; unsafe versions refused by the checker ${t.refused}/${t.n}, warned of ${t.warned}/${t.n}, left to a reviewer ${t.accepted}/${t.n}`
    : "; checkers not run (--no-checks)";
  return head + tail;
}

const heading = (result) => [
  "each task's change in each stack: the pull request (reference patch, after the task's setup) and what the stack's checker says of it and of its unsafe version",
  "  checkers: " + Object.entries(result.checkers).map(([k, v]) => `${STACKS[k].label} ${v}`).join("; "),
  "  stage that caught the unsafe version in the harness's controls (docs/evidence/E14/harness-T*.txt) is shown with that evidence's commit",
];

export function render(result) {
  const totals = Object.fromEntries(Object.keys(STACKS).map((s) => [s, newTotals()]));
  const rows = result.rows.flatMap((row) => rowLines(row, totals));
  const checked = result.rows.some((r) => Object.values(r.stacks).some((c) => c.reference));
  const lines = [
    ...heading(result),
    ...rows,
    ...(checked ? checklist(result.rows) : []),
    "  totals over the tasks:",
    ...Object.entries(totals).map(([stack, t]) => totalLine(stack, t, checked)),
  ];
  return `${lines.join("\n")}\n`;
}

/** The references that do not check clean, as `<task> <stack>`. */
const badReferences = (rows) =>
  rows.flatMap((r) => Object.entries(r.stacks).filter(([, c]) => c.reference && !c.reference.clean).map(([s]) => `${r.id} ${s}`));

/** The command line: every change, written out, and exit 1 where a reference change does not check clean. */
function cli(argv) {
  const json = pathAfter(argv, "--json");
  const result = collect({ checks: !argv.includes("--no-checks") });
  if (json) fs.writeFileSync(json, `${JSON.stringify(result, null, 1)}\n`);
  process.stdout.write(render(result));
  const bad = badReferences(result.rows);
  if (bad.length) {
    process.stdout.write(`feature-change: REFUSED: a reference change does not check clean: ${bad.join(", ")}\n`);
    process.exit(1);
  }
}

if (isMain(import.meta.url)) cli(process.argv.slice(2));
