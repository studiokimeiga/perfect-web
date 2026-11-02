// The same feature change in each stack (feature-change.mjs), pinned as it is:
// a patch's statistics, a warning's identity, the harness's evidence read, the
// rows collected from the tasks' directories and the report rendered from them.
//
// Fixtures, in fixtures/feature-change/: `tasks/T01-*` and `tasks/T08-*` are
// copies of benchmarks/tasks's task.json and reference patches (T08's SvelteKit
// patch left out, for a stack with no reference patch); `patches/` are copies of
// T03's SvelteKit reference and unsafe patches, T04's Pleris reference patch and
// T07's Next.js setup patch; `evidence/harness-T01.txt` and `harness-T08.txt` are
// copies of docs/evidence/E14's. Written here, for what the twelve tasks do not reach: a
// T99 that `CLASS` does not name (its patch, its task.json, its evidence in the
// harness's own format), directory names that are not tasks', and a notes file
// that is not evidence. The recorded run's feature.json and feature.txt (the
// evidence of record) are read where they are.
//
// Not tested, because they run the real checkers on the real baselines:
// runChecker, checkWith, restore, apply, times, dirty and sh, and `collect`
// with `checks: true`.
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { CLASS, collect, diffStats, harnessCaught, newWarnings, render } from "../feature-change.mjs";
import { REPO } from "../lib.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const FIXTURES = path.join(HERE, "fixtures/feature-change");
const TASKS = path.join(FIXTURES, "tasks");
const EVIDENCE = path.join(FIXTURES, "evidence");
const RECORDED = path.join(HERE, "fixtures/recorded-run");

const lines = (text) => text.split("\n");
const readIn = (dir, ...parts) => fs.readFileSync(path.join(dir, ...parts), "utf8");
const lined = (...rows) => `${rows.join("\n")}\n`;
const statsOf = (...rows) => diffStats(lined(...rows));
const stats = (files, created, added, removed, languages, paths) => ({ files, created, added, removed, languages, paths });
const NOTHING = stats(0, 0, 0, 0, [], []);

/** Runs `fn` on a new directory holding `files` (a name and its text each), then removes it. */
function withDir(files, fn) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "h2h-feature-"));
  try {
    Object.entries(files).forEach(([name, text]) => fs.writeFileSync(path.join(dir, name), text));
    return fn(dir);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
}

/** What `fn` returned and what it wrote to stderr (each write, as it was written). */
function captured(fn) {
  const written = [];
  const original = process.stderr.write;
  process.stderr.write = (chunk) => {
    written.push(String(chunk));
    return true;
  };
  try {
    return { value: fn(), written };
  } finally {
    process.stderr.write = original;
  }
}

// --- diffStats ----------------------------------------------------------------

// The statistics of the real patches below agree with `git apply --numstat`.
// T99's patch is the one written here; the others are the repository's.
const T01_PLERIS = stats(1, 0, 1, 0, ["Pleris"], ["store/app.pw"]);
const T01_NEXT_REACT = stats(2, 1, 52, 25, ["TSX"], ["app/stores/[storeId]/StoreView.tsx", "app/stores/[storeId]/page.tsx"]);
const T01_SVELTEKIT = stats(1, 0, 16, 2, ["Svelte"], ["src/routes/stores/[storeId]/+page.svelte"]);
const T08_PLERIS = stats(1, 0, 2, 0, ["Pleris"], ["store/app.pw"]);
const T08_NEXT_REACT = stats(4, 1, 44, 8, ["TSX", "TypeScript"], ["app/stores/[storeId]/AddForm.tsx", "app/stores/[storeId]/actions.ts", "app/stores/[storeId]/page.tsx", "lib/store.ts"]);
const T99_SVELTEKIT = stats(1, 0, 2, 1, ["TypeScript"], ["src/lib/format.ts"]);
const T03_SVELTEKIT_PATHS = ["src/lib/server/store.ts", "src/routes/stores/[storeId]/+page.server.ts", "src/routes/stores/[storeId]/+page.svelte"];

// [what it shows, the patch under fixtures/feature-change, what is read of it]
const FIXTURE_PATCHES = [
  ["T01's Pleris reference patch is one Pleris file with one line added", "tasks/T01-optimistic-cart/reference/pleris.patch", T01_PLERIS],
  ["T01's Next.js reference patch is two TSX files, one created, over three hunks: 52 lines added and 25 removed", "tasks/T01-optimistic-cart/reference/next-react.patch", T01_NEXT_REACT],
  ["T01's SvelteKit reference patch is one Svelte file over three hunks: 16 lines added and 2 removed", "tasks/T01-optimistic-cart/reference/sveltekit.patch", T01_SVELTEKIT],
  ["T08's Pleris reference patch is one Pleris file over two hunks: 2 lines added", "tasks/T08-duplicate-submissions/reference/pleris.patch", T08_PLERIS],
  ["T08's Next.js reference patch is four files in TSX and TypeScript, one created: 44 lines added and 8 removed", "tasks/T08-duplicate-submissions/reference/next-react.patch", T08_NEXT_REACT],
  ["T99's synthetic SvelteKit reference patch is one TypeScript file: 2 lines added and 1 removed", "tasks/T99-unclassed/reference/sveltekit.patch", T99_SVELTEKIT],
  [
    "T03's SvelteKit reference patch is three files in Svelte and TypeScript, none created: 28 lines added and 1 removed",
    "patches/T03-reference-sveltekit.patch",
    stats(3, 0, 28, 1, ["Svelte", "TypeScript"], T03_SVELTEKIT_PATHS),
  ],
  [
    "T03's SvelteKit unsafe patch is the same three files with 41 lines added and 1 removed",
    "patches/T03-unsafe-sveltekit.patch",
    stats(3, 0, 41, 1, ["Svelte", "TypeScript"], T03_SVELTEKIT_PATHS),
  ],
  ["T04's Pleris reference patch is two Pleris files, a line added to each", "patches/T04-reference-pleris.patch", stats(2, 0, 2, 0, ["Pleris"], ["domain.pw", "store/app.pw"])],
  [
    "T07's Next.js setup patch is five files in TSX and TypeScript, three of them created: 101 lines added and 1 removed",
    "patches/T07-setup-next-react.patch",
    stats(5, 3, 101, 1, ["TSX", "TypeScript"], ["app/api/menu/route.ts", "app/bench/category/route.ts", "app/stores/[storeId]/Browse.tsx", "app/stores/[storeId]/page.tsx", "lib/store.ts"]),
  ],
];

FIXTURE_PATCHES.forEach(([name, file, expected]) => test(name, () => assert.deepEqual(diffStats(readIn(FIXTURES, file)), expected)));

// [what it shows, the patch's lines, what is read of it]
const SYNTHETIC_PATCHES = [
  [
    "git's headers for a rename, a mode change and a binary file name no file: only a --- and +++ pair does",
    ["diff --git a/old.ts b/new.ts", "similarity index 100%", "rename from old.ts", "rename to new.ts", "diff --git a/run.sh b/run.sh", "old mode 100644", "new mode 100755", "diff --git a/logo.png b/logo.png", "Binary files a/logo.png and b/logo.png differ"],
    NOTHING,
  ],
  ["a file's headers with no hunk make a file of no lines", ["--- a/x.ts", "+++ b/x.ts"], stats(1, 0, 0, 0, ["TypeScript"], ["x.ts"])],
  ["a hunk before any file header is not read, nor are its lines", ["@@ -1 +1 @@", "-a", "+b"], NOTHING],
  ["a +++ line with no --- line before it names a file that is not created", ["+++ b/x.ts", "@@ -0,0 +1 @@", "+a"], stats(1, 0, 1, 0, ["TypeScript"], ["x.ts"])],
  [
    "a hunk header without counts is one line on each side, and a line past them is not read",
    ["--- a/x.ts", "+++ b/x.ts", "@@ -1 +1 @@", "-a", "+b", "+c"],
    stats(1, 0, 1, 1, ["TypeScript"], ["x.ts"]),
  ],
  [
    "a hunk header with a count on the new side only reads one line of the old",
    ["--- a/x.ts", "+++ b/x.ts", "@@ -1 +1,2 @@", "-a", "+b", "+c"],
    stats(1, 0, 2, 1, ["TypeScript"], ["x.ts"]),
  ],
  [
    "a hunk header with a count on the old side only reads one line of the new",
    ["--- a/x.ts", "+++ b/x.ts", "@@ -1,2 +1 @@", "-a", "-b", "+c"],
    stats(1, 0, 1, 2, ["TypeScript"], ["x.ts"]),
  ],
  [
    "a hunk header that inserts one line has no new count, and an old count of 0",
    ["--- a/x.ts", "+++ b/x.ts", "@@ -4,0 +5 @@", "+new"],
    stats(1, 0, 1, 0, ["TypeScript"], ["x.ts"]),
  ],
  [
    "a hunk header that deletes one line has no old count, and a new count of 0",
    ["--- a/x.ts", "+++ b/x.ts", "@@ -5 +4,0 @@", "-gone"],
    stats(1, 0, 0, 1, ["TypeScript"], ["x.ts"]),
  ],
  [
    "a no-newline marker is a line of neither side, whether it comes inside the hunk or after its last line",
    ["--- a/x.ts", "+++ b/x.ts", "@@ -1 +1 @@", "-old", "\\ No newline at end of file", "+new", "\\ No newline at end of file"],
    stats(1, 0, 1, 1, ["TypeScript"], ["x.ts"]),
  ],
  [
    "a line that begins --- or +++ inside a hunk is content, added or removed, and the hunk's end is its counts",
    ["--- a/x.ts", "+++ b/x.ts", "@@ -1,3 +1,3 @@", " keep", "--- not a header", "+++ not a header", " keep"],
    stats(1, 0, 1, 1, ["TypeScript"], ["x.ts"]),
  ],
  [
    "an empty line inside a hunk is a context line, as an editor leaves a blank one",
    ["--- a/x.ts", "+++ b/x.ts", "@@ -1,3 +1,3 @@", " a", "", "-b", "+c"],
    stats(1, 0, 1, 1, ["TypeScript"], ["x.ts"]),
  ],
  [
    "a hunk that claims more lines than the patch holds takes the next file's headers for content",
    ["--- a/a.ts", "+++ b/a.ts", "@@ -1,4 +1,4 @@", "-x", "+y", "--- a/b.ts", "+++ b/b.ts", "@@ -1 +1 @@", "-p", "+q"],
    stats(1, 0, 3, 3, ["TypeScript"], ["a.ts"]),
  ],
  [
    "a deleted file is a file named /dev/null of no language",
    ["--- a/x.ts", "+++ /dev/null", "@@ -1,2 +0,0 @@", "-x", "-y"],
    stats(1, 0, 0, 2, [""], ["/dev/null"]),
  ],
  [
    "every deleted file is that one file named /dev/null",
    ["--- a/a.ts", "+++ /dev/null", "@@ -1,2 +0,0 @@", "-x", "-y", "--- a/b.ts", "+++ /dev/null", "@@ -1,3 +0,0 @@", "-p", "-q", "-r"],
    stats(1, 0, 0, 5, [""], ["/dev/null"]),
  ],
  [
    "the b/ prefix is taken off once, a path without it is kept, and blanks after a path are trimmed",
    ["--- a/lib/a.ts", "+++ b/lib/a.ts", "--- lib/b.ts", "+++ lib/b.ts", "--- a/b/c.ts", "+++ b/b/c.ts", "--- a/d.ts", "+++ b/d.ts   "],
    stats(4, 0, 0, 0, ["TypeScript"], ["lib/a.ts", "lib/b.ts", "b/c.ts", "d.ts"]),
  ],
  [
    "a file named in two sections is one file with both sections' lines, created if its first header says so",
    ["--- /dev/null", "+++ b/x.ts", "@@ -0,0 +1 @@", "+a", "--- a/x.ts", "+++ b/x.ts", "@@ -1 +1 @@", "-a", "+b", "--- a/y.ts", "+++ b/y.ts", "@@ -1 +1 @@", "-a", "+b", "--- /dev/null", "+++ b/y.ts", "@@ -0,0 +1 @@", "+c"],
    stats(2, 1, 4, 2, ["TypeScript"], ["x.ts", "y.ts"]),
  ],
  [
    "a --- /dev/null line makes only the next +++ line's file created: a second +++ line after it is a file that is not",
    ["--- /dev/null", "+++ b/x.ts", "+++ b/y.ts"],
    stats(2, 1, 0, 0, ["TypeScript"], ["x.ts", "y.ts"]),
  ],
  [
    "a created file counts for the file whose header follows its own, not for the files after it",
    ["diff --git a/new.svelte b/new.svelte", "new file mode 100644", "--- /dev/null", "+++ b/new.svelte", "@@ -0,0 +1,2 @@", "+<p>one</p>", "+<p>two</p>", "diff --git a/lib/a.ts b/lib/a.ts", "--- a/lib/a.ts", "+++ b/lib/a.ts", "@@ -1,2 +1,2 @@", " keep", "-old", "+new"],
    stats(2, 1, 3, 1, ["Svelte", "TypeScript"], ["new.svelte", "lib/a.ts"]),
  ],
];

SYNTHETIC_PATCHES.forEach(([name, rows, expected]) => test(name, () => assert.deepEqual(statsOf(...rows), expected)));

test("an empty text, or a blank line, is a patch of nothing", () => {
  assert.deepEqual(diffStats(""), NOTHING);
  assert.deepEqual(diffStats("\n"), NOTHING);
});

// [a file's path, the language its extension names]
const EXTENSIONS = [
  ["a.pw", "Pleris"],
  ["a.ts", "TypeScript"],
  ["a.tsx", "TSX"],
  ["a.svelte", "Svelte"],
  ["a.js", "JavaScript"],
  ["a.css", "CSS"],
  ["a.rs", "Rust"],
  ["a.json", "JSON"],
  ["a.html", "HTML"],
  ["a.d.ts", "TypeScript"],
  ["a.TS", ".TS"],
  ["a.md", ".md"],
  ["archive.tar.gz", ".gz"],
  ["Makefile", ""],
  [".gitignore", ""],
];

test("each extension the language table names is its language, and any other extension is its own name", () => {
  EXTENSIONS.forEach(([file, language]) => assert.deepEqual(statsOf(`+++ b/${file}`).languages, [language], file));
});

test("a patch's languages are listed once each and sorted by code unit, upper case before lower", () => {
  const files = ["a.pw", "b.ts", "c.tsx", "d.svelte", "e.js", "f.css", "g.rs", "h.json", "i.html", "j.md", "Makefile", "k.ts"];
  const d = statsOf(...files.map((file) => `+++ b/${file}`));
  assert.deepEqual(d, stats(12, 0, 0, 0, ["", ".md", "CSS", "HTML", "JSON", "JavaScript", "Pleris", "Rust", "Svelte", "TSX", "TypeScript"], files));
});

// [a --- line, whether it makes the next file created]
const CREATED_HEADERS = [
  ["--- /dev/null", 1],
  ["--- /dev/null   ", 1],
  ["--- /dev/null\t2026-11-02 10:00:00.000000000 +0000", 0],
  ["--- /dev/nullity", 0],
  ["--- a/x.ts", 0],
];

test("a file is created where its --- line is /dev/null and nothing but blanks", () => {
  CREATED_HEADERS.forEach(([header, created]) => assert.equal(statsOf(header, "+++ b/x.ts", "@@ -0,0 +1 @@", "+a").created, created, header));
});

test("a patch whose lines end in CRLF is read as the same patch", () => {
  const text = readIn(TASKS, "T01-optimistic-cart/reference/next-react.patch");
  assert.ok(text.includes("\n") && !text.includes("\r"));
  assert.deepEqual(diffStats(text.replaceAll("\n", "\r\n")), T01_NEXT_REACT);
});

// --- newWarnings --------------------------------------------------------------

const ALT = 'WARNING "src/routes/+page.svelte" 12:3 "A11y: <img> element should have an alt attribute"';
const ALT_MOVED = 'WARNING "src/routes/+page.svelte" 48:17 "A11y: <img> element should have an alt attribute"';
const LABEL = 'WARNING "src/routes/+page.svelte" 20:5 "A11y: A form label must be associated with a control"';
const LABEL_UNPLACED = 'WARNING "src/routes/+page.svelte" "A11y: A form label must be associated with a control"';

test("a warning the unsafe change draws at another line and column than its reference's is not new", () => {
  assert.deepEqual(newWarnings([ALT], [ALT_MOVED]), []);
});

test("a warning the reference does not draw is new, and is returned without its place", () => {
  assert.deepEqual(newWarnings([ALT], [ALT_MOVED, LABEL]), [LABEL_UNPLACED]);
});

test("a warning only the reference draws is not new", () => {
  assert.deepEqual(newWarnings([ALT, LABEL], [ALT_MOVED]), []);
});

test("runs of white space are one space and the ends are trimmed, so the same warning spaced otherwise is the same", () => {
  const spaced = '  WARNING   "src/routes/+page.svelte"\t20:5\n  "A11y: A form label must be associated with a control"  ';
  assert.deepEqual(newWarnings([LABEL], [spaced]), []);
  assert.deepEqual(newWarnings([], [spaced]), [LABEL_UNPLACED]);
});

test("a warning drawn several times is new once, and the new ones keep the order they were first drawn in", () => {
  const second = 'WARNING "src/lib/Card.svelte" 3:9 "A11y: no autofocus"';
  const third = 'WARNING "src/lib/Card.svelte" 8:1 "A11y: no positive tabindex"';
  assert.deepEqual(newWarnings([ALT], [LABEL, second, ALT_MOVED, LABEL, third, second]), [
    LABEL_UNPLACED,
    'WARNING "src/lib/Card.svelte" "A11y: no autofocus"',
    'WARNING "src/lib/Card.svelte" "A11y: no positive tabindex"',
  ]);
});

test("a side that is not given draws no warnings", () => {
  assert.deepEqual(newWarnings(), []);
  assert.deepEqual(newWarnings(undefined, [LABEL]), [LABEL_UNPLACED]);
  assert.deepEqual(newWarnings([LABEL]), []);
});

test("any two numbers about a colon are taken for a place, in a warning's message too", () => {
  assert.deepEqual(newWarnings([], ['WARNING "a.svelte" "aspect ratio 16:9 expected"', "time 12:30 and 7:8:9"]), ['WARNING "a.svelte" "aspect ratio expected"', "time and :9"]);
});

// --- harnessCaught ------------------------------------------------------------

const control = (id, stack, stage) => `${id} ${stack}: negative ok, positive ok, unsafe ok (caught at ${stage}), harness ok`;
const caught = (stage, commit) => ({ stage, commit });
const HASH = "0123456789abcdef0123456789abcdef01234567";

test("the harness's evidence is read for each task and stack: the stage that caught the unsafe change, and the evidence's commit", () => {
  assert.deepEqual(harnessCaught(EVIDENCE), {
    T01: { pleris: caught("check", "0c608dc3"), "next-react": caught("hidden", "0c608dc3"), sveltekit: caught("hidden", "0c608dc3") },
    T08: { pleris: caught("check", "4f75868b"), "next-react": caught("hidden", "4f75868b"), sveltekit: caught("contract", "4f75868b") },
    T99: { pleris: caught("check", "7e3d9a15") },
  });
});

test("a directory that is not there holds no evidence", () => {
  assert.deepEqual(harnessCaught(path.join(FIXTURES, "no-such-evidence")), {});
});

test("only files named harness-T and a number, .txt, are read", () => {
  const line = lined(control("T05", "pleris", "check"));
  const other = lined(control("T06", "pleris", "check"));
  const files = { "harness-T05.txt": line, "harness-T05.txt.bak": other, "Harness-T07.txt": other, "harness-T.txt": other, "harness-T08.md": other, "xharness-T09.txt": other, "notes.txt": other };
  withDir(files, (dir) => assert.deepEqual(harnessCaught(dir), { T05: { pleris: caught("check", null) } }));
});

test("a line is a catch only if it is a task, a stack and a colon, then an unsafe change caught at a stage", () => {
  const text = lined(
    `commit: ${HASH}`,
    "ok 1 - a sandbox holding a hidden file's bytes is refused",
    control("T05", "pleris", "check"),
    "T05 next-react: negative ok, positive ok, unsafe FAIL, harness FAIL",
    `  ${control("T05", "sveltekit", "hidden")}`,
    control("t05", "sveltekit", "hidden"),
    "T05 sveltekit negative ok, positive ok, unsafe ok (caught at hidden), harness ok",
    control("T05", "sveltekit", "type-check"),
    "T05 sveltekit: unsafe ok (caught at )",
  );
  withDir({ "harness-T05.txt": text }, (dir) => assert.deepEqual(harnessCaught(dir), { T05: { pleris: caught("check", "01234567") } }));
});

// [why, the file's commit line or lines, the commit recorded]
const COMMITS = [
  ["the first eight characters of a full hash", `commit: ${HASH}`, "01234567"],
  ["a hash of eight characters", "commit: 89abcdef", "89abcdef"],
  ["the first eight hex digits, whatever follows them", "commit: 01234567zzzz", "01234567"],
  ["the first of two commit lines", `commit: ${HASH}\ncommit: fedcba9876543210`, "01234567"],
  ["none for a hash of seven characters", "commit: 89abcde", null],
  ["none for upper-case digits", "commit: 89ABCDEF", null],
  ["none for a word that is not a hash", "commit: unknown", null],
  ["none for an indented commit line", `  commit: ${HASH}`, null],
  ["none where there is no commit line", "node: v22.21.1", null],
];

test("an evidence file's commit is the first eight hex digits of its commit line, or null", () => {
  COMMITS.forEach(([why, commitLines, expected]) => {
    const files = { "harness-T05.txt": lined("E14-B - the offline harness", commitLines, control("T05", "pleris", "check")) };
    withDir(files, (dir) => assert.deepEqual(harnessCaught(dir).T05.pleris.commit, expected, why));
  });
});

test("a later line for the same task and stack replaces an earlier one, and a line is filed under its own task, not its file's", () => {
  const text = lined(`commit: ${HASH}`, control("T05", "pleris", "check"), control("T05", "pleris", "hidden"), control("T05", "sveltekit", "contract"), control("T06", "next-react", "check"));
  const expected = { T05: { pleris: caught("hidden", "01234567"), sveltekit: caught("contract", "01234567") }, T06: { "next-react": caught("check", "01234567") } };
  withDir({ "harness-T05.txt": text }, (dir) => assert.deepEqual(harnessCaught(dir), expected));
});

test("with no directory given, the evidence read is the repository's docs/evidence/E14", () => {
  const byDefault = harnessCaught();
  assert.deepEqual(byDefault, harnessCaught(path.join(REPO, "docs/evidence/E14")));
  assert.ok(Object.keys(byDefault).length > 0);
});

// --- collect ------------------------------------------------------------------

const collectFixture = (over = {}) => captured(() => collect({ checks: false, tasksDir: TASKS, evidenceDir: EVIDENCE, ...over }));

const COLLECTED = {
  checkers: { pleris: "pw check", "next-react": "tsc --noEmit", sveltekit: "svelte-check" },
  rows: [
    {
      id: "T01",
      task: "T01-optimistic-cart",
      title: "Optimistic cart update with rollback",
      class: CLASS.T01,
      stacks: {
        pleris: { pr: T01_PLERIS, harness: caught("check", "0c608dc3") },
        "next-react": { pr: T01_NEXT_REACT, harness: caught("hidden", "0c608dc3") },
        sveltekit: { pr: T01_SVELTEKIT, harness: caught("hidden", "0c608dc3") },
      },
    },
    {
      id: "T08",
      task: "T08-duplicate-submissions",
      title: "Prevent duplicate submissions",
      class: CLASS.T08,
      stacks: {
        pleris: { pr: T08_PLERIS, harness: caught("check", "4f75868b") },
        "next-react": { pr: T08_NEXT_REACT, harness: caught("hidden", "4f75868b") },
      },
    },
    { id: "T99", task: "T99-unclassed", title: "A change the classes do not name", class: "", stacks: { sveltekit: { pr: T99_SVELTEKIT, harness: null } } },
  ],
};

test("a tasks directory is collected into a row for each task: its title, its class and, for each stack with a reference patch, the patch's statistics and the harness's catch", () => {
  assert.deepEqual(collectFixture().value, COLLECTED);
});

test("collected rows follow the sorted task names, however the directory lists them, and a row's stacks follow the checkers' order", (t) => {
  const readdir = fs.readdirSync;
  t.mock.method(fs, "readdirSync", (...args) => readdir(...args).reverse());
  const { rows, checkers } = collectFixture().value;
  assert.deepEqual(Object.keys(checkers), ["pleris", "next-react", "sveltekit"]);
  assert.deepEqual(rows.map((r) => r.task), ["T01-optimistic-cart", "T08-duplicate-submissions", "T99-unclassed"]);
  assert.deepEqual(rows.map((r) => Object.keys(r.stacks)), [["pleris", "next-react", "sveltekit"], ["pleris", "next-react"], ["sveltekit"]]);
});

test("only a directory named T and a number, then a hyphen, is a task: other directories and files are left alone", () => {
  assert.deepEqual(fs.readdirSync(TASKS).sort(), ["NOTES.txt", "T01-optimistic-cart", "T08-duplicate-submissions", "T7", "T99-unclassed", "t05-lowercase"]);
  assert.equal(collectFixture().value.rows.length, 3);
});

test("a stack with no reference patch is left out of its row, with its evidence, and a task the classes do not name has an empty class", () => {
  const { rows } = collectFixture().value;
  assert.equal(rows[1].stacks.sveltekit, undefined);
  assert.equal(harnessCaught(EVIDENCE).T08.sveltekit.stage, "contract");
  assert.equal(rows[2].class, "");
  assert.equal(CLASS.T99, undefined);
});

test("a stack the harness's evidence has no catch for has a null harness: its line was no catch, or its task has no evidence", () => {
  const harnesses = (over) => collectFixture(over).value.rows.flatMap((r) => Object.values(r.stacks)).map((cell) => cell.harness);
  assert.deepEqual(harnesses({ evidenceDir: path.join(FIXTURES, "no-such-evidence") }), [null, null, null, null, null, null]);
  assert.deepEqual(harnesses().slice(-1), [null]);
});

test("collecting without checks writes a progress line to stderr for each stack's patch, in the rows' order", () => {
  assert.deepEqual(collectFixture().written, [
    "  T01 pleris: 1+/0-\n",
    "  T01 next-react: 52+/25-\n",
    "  T01 sveltekit: 16+/2-\n",
    "  T08 pleris: 2+/0-\n",
    "  T08 next-react: 44+/8-\n",
    "  T99 sveltekit: 2+/1-\n",
  ]);
});

test("with no directories given, the tasks collected are the repository's benchmarks/tasks and the evidence its docs/evidence/E14", () => {
  const byDefault = captured(() => collect({ checks: false }));
  const named = captured(() => collect({ checks: false, tasksDir: path.join(REPO, "benchmarks/tasks"), evidenceDir: path.join(REPO, "docs/evidence/E14") }));
  assert.deepEqual(byDefault.value, named.value);
  assert.deepEqual(byDefault.written, named.written);
  assert.ok(byDefault.value.rows.length > 0);
});

// --- render -------------------------------------------------------------------

const TITLE = "each task's change in each stack: the pull request (reference patch, after the task's setup) and what the stack's checker says of it and of its unsafe version";
const CHECKERS = "  checkers: Pleris pw check; Next.js tsc --noEmit; SvelteKit svelte-check";
const STAGE_NOTE = "  stage that caught the unsafe version in the harness's controls (docs/evidence/E14/harness-T*.txt) is shown with that evidence's commit";
const rendered = (result) => lines(render(result));

test("the recorded feature-change result renders to the feature-change evidence of record, line for line", () => {
  const result = JSON.parse(readIn(RECORDED, "feature.json"));
  assert.deepEqual(rendered(result), lines(readIn(RECORDED, "feature.txt")));
});

test("a result collected without checks renders each change with no checker's verdict, the harness's catch after an empty one, and no checklist", () => {
  assert.deepEqual(rendered(collectFixture().value), [
    TITLE,
    CHECKERS,
    STAGE_NOTE,
    `  T01 Optimistic cart update with rollback [${CLASS.T01}]`,
    "    Pleris     +1   -0   1 file(s), Pleris; ; harness caught it at check (0c608dc3)",
    "    Next.js    +52  -25  2 file(s) (1 new), TSX; ; harness caught it at hidden (0c608dc3)",
    "    SvelteKit  +16  -2   1 file(s), Svelte; ; harness caught it at hidden (0c608dc3)",
    `  T08 Prevent duplicate submissions [${CLASS.T08}]`,
    "    Pleris     +2   -0   1 file(s), Pleris; ; harness caught it at check (4f75868b)",
    "    Next.js    +44  -8   4 file(s) (1 new), TSX+TypeScript; ; harness caught it at hidden (4f75868b)",
    "  T99 A change the classes do not name []",
    "    SvelteKit  +2   -1   1 file(s), TypeScript; ",
    "  totals over the tasks:",
    "    Pleris     2 changes: +3 -0 lines, 2 file(s) touched, languages Pleris; checkers not run (--no-checks)",
    "    Next.js    2 changes: +96 -33 lines, 6 file(s) touched, languages TSX, TypeScript; checkers not run (--no-checks)",
    "    SvelteKit  2 changes: +18 -3 lines, 2 file(s) touched, languages Svelte, TypeScript; checkers not run (--no-checks)",
    "",
  ]);
});

const pr = (added, removed, files, created, languages) => ({ added, removed, files, created, languages, paths: [] });
const CLEAN = { clean: true, error: null, warnings: [] };
const REFERENCE_REFUSED = { clean: false, error: "error: [PW0001] the reference no longer checks", warnings: [] };
const refused = (error) => ({ clean: false, error, warnings: [], newWarnings: [] });
const accepted = (...newWarnings) => ({ clean: true, error: null, warnings: [], newWarnings });
const cell = (change, harness, reference, unsafe) => ({ pr: change, harness, reference, unsafe });
const row = (id, title, klass, stacks) => ({ id, task: `${id}-task`, title, class: klass, stacks });

const CHECKED = {
  checkers: { pleris: "pw check", "next-react": "tsc --noEmit", sveltekit: "svelte-check" },
  rows: [
    row("T01", "First change", "loading and error states: one", {
      pleris: cell(pr(1, 0, 1, 0, ["Pleris"]), caught("check", "0c608dc3"), CLEAN, refused("error: [PW0330] `add_to_cart` performs a write")),
      "next-react": cell(pr(52, 25, 2, 1, ["TypeScript"]), caught("hidden", "0c608dc3"), CLEAN, accepted()),
      sveltekit: cell(pr(16, 2, 1, 0, ["Svelte"]), null, CLEAN, accepted('WARNING "src/a.svelte" "A11y: autofocus"', 'WARNING "src/a.svelte" "A11y: a second one"')),
    }),
    row("T05", "Second change", "loading and error states: two", {
      pleris: cell(pr(14, 0, 1, 0, ["Pleris"]), caught("contract", null), REFERENCE_REFUSED, CLEAN),
      "next-react": cell(pr(25, 1, 1, 0, ["TSX"]), null, CLEAN, CLEAN),
    }),
    row("T99", "Third change", "", {
      "next-react": cell(pr(3, 3, 1, 0, ["TypeScript"]), null),
      sveltekit: cell(pr(3, 3, 1, 0, ["TypeScript"]), null, CLEAN, refused(`ERROR "src/a.ts" 3:5 "Type 'x' is not assignable"`)),
    }),
    row("T14", "Fourth change", "a class with no colon", {
      pleris: cell(pr(2, 2, 1, 0, ["Pleris"]), null, CLEAN, refused("error: [PW5001] `Cart` is Session<SessionId> and declares a shared cache")),
      sveltekit: cell(pr(120, 100, 3, 0, ["Svelte", "TypeScript"]), null, CLEAN, accepted('WARNING "src/b.svelte" "A11y: label"')),
    }),
    row("T15", "Fifth change", "retries: a press twice", {}),
  ],
};

test("a checked result renders each verdict in its words, a class's tasks together, the checklist by class and the totals with their counts", () => {
  assert.deepEqual(rendered(CHECKED), [
    TITLE,
    CHECKERS,
    STAGE_NOTE,
    "  T01 First change [loading and error states: one]",
    "    Pleris     +1   -0   1 file(s), Pleris; reference checks clean; unsafe refused: error: [PW0330] `add_to_cart` performs a write; harness caught it at check (0c608dc3)",
    "    Next.js    +52  -25  2 file(s) (1 new), TypeScript; reference checks clean; unsafe ACCEPTED, with no warning its reference lacks: a reviewer must find it; harness caught it at hidden (0c608dc3)",
    '    SvelteKit  +16  -2   1 file(s), Svelte; reference checks clean; unsafe accepted, but WARNED of: WARNING "src/a.svelte" "A11y: autofocus"',
    "  T05 Second change [loading and error states: two]",
    "    Pleris     +14  -0   1 file(s), Pleris; reference REFUSED: error: [PW0001] the reference no longer checks; unsafe ACCEPTED, with no warning its reference lacks: a reviewer must find it; harness caught it at contract (null)",
    "    Next.js    +25  -1   1 file(s), TSX; reference checks clean; unsafe ACCEPTED, with no warning its reference lacks: a reviewer must find it",
    "  T99 Third change []",
    "    Next.js    +3   -3   1 file(s), TypeScript; ",
    `    SvelteKit  +3   -3   1 file(s), TypeScript; reference checks clean; unsafe refused: ERROR "src/a.ts" 3:5 "Type 'x' is not assignable"`,
    "  T14 Fourth change [a class with no colon]",
    "    Pleris     +2   -2   1 file(s), Pleris; reference checks clean; unsafe refused: error: [PW5001] `Cart` is Session<SessionId> and declares a shared cache",
    '    SvelteKit  +120 -100 3 file(s), Svelte+TypeScript; reference checks clean; unsafe accepted, but WARNED of: WARNING "src/b.svelte" "A11y: label"',
    "  T15 Fifth change [retries: a press twice]",
    "  what a reviewer must verify by hand, by class of mistake (the tasks' unsafe versions):",
    "    loading and error states (T01, T05): Pleris: checker proves T01 PW0330; reviewer: T05 | Next.js: checker proves none; reviewer: T01, T05 | SvelteKit: checker proves T01 a warning",
    "     (T99): Pleris: checker proves none | Next.js: checker proves none | SvelteKit: checker proves T99 refused",
    "    a class with no colon (T14): Pleris: checker proves T14 PW5001 | Next.js: checker proves none | SvelteKit: checker proves T14 a warning",
    "    retries (T15): Pleris: checker proves none | Next.js: checker proves none | SvelteKit: checker proves none",
    "  totals over the tasks:",
    "    Pleris     3 changes: +17 -2 lines, 3 file(s) touched, languages Pleris; references clean 2/3; unsafe versions refused by the checker 2/3, warned of 0/3, left to a reviewer 1/3",
    "    Next.js    3 changes: +80 -29 lines, 4 file(s) touched, languages TSX, TypeScript; references clean 2/3; unsafe versions refused by the checker 0/3, warned of 0/3, left to a reviewer 2/3",
    "    SvelteKit  3 changes: +139 -105 lines, 5 file(s) touched, languages Svelte, TypeScript; references clean 3/3; unsafe versions refused by the checker 1/3, warned of 2/3, left to a reviewer 0/3",
    "",
  ]);
});

test("a result with no changes lists the checkers it names in its order, and every stack's totals as none, with the checkers not run", () => {
  const result = { checkers: { sveltekit: "svelte-check", pleris: "pw check" }, rows: [row("T01", "A task with a change in no stack", "retries: one press twice", {})] };
  assert.deepEqual(rendered(result), [
    TITLE,
    "  checkers: SvelteKit svelte-check; Pleris pw check",
    STAGE_NOTE,
    "  T01 A task with a change in no stack [retries: one press twice]",
    "  totals over the tasks:",
    "    Pleris     0 changes: +0 -0 lines, 0 file(s) touched, languages ; checkers not run (--no-checks)",
    "    Next.js    0 changes: +0 -0 lines, 0 file(s) touched, languages ; checkers not run (--no-checks)",
    "    SvelteKit  0 changes: +0 -0 lines, 0 file(s) touched, languages ; checkers not run (--no-checks)",
    "",
  ]);
});
