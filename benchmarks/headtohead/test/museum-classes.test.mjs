// The museum's bug classes each stack lets ship (museum-classes.mjs), pinned as
// it is: the evidence of `just e14-museum` read per exhibit, the exhibits
// collected from the museum's directories, and the report rendered from them.
//
// Fixtures, in fixtures/museum/: `exhibits/` holds copies of the five
// benchmarks/museum/*/exhibit.json, a file and a directory with no exhibit.json
// (written here: the real museum has a museum.mjs beside its exhibits);
// `museum.txt` is a copy of docs/evidence/E14/museum.txt, the evidence of record
// (commit 5a4378fe); `collected.json` is what `collect` returns on the two,
// which is the recorded run's museum.json byte for byte. The recorded run's
// museum.json and museum.txt (the evidence of record) are read where they are.
// Cases the five exhibits do not reach are written below, in the evidence's own
// words (benchmarks/museum/museum.mjs prints them).
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { collect, parseEvidence, render } from "../museum-classes.mjs";
import { REPO } from "../lib.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const FIXTURES = path.join(HERE, "fixtures/museum");
const EXHIBITS = path.join(FIXTURES, "exhibits");
const EVIDENCE = path.join(FIXTURES, "museum.txt");
const RECORDED = path.join(HERE, "fixtures/recorded-run");
const EXHIBIT_IDS = ["F01-markup-from-text", "G01-retried-command-twice", "H07-n-plus-one", "J01-stale-page-after-save", "J08-shared-server-state"];

const lines = (text) => text.split("\n");
const readIn = (dir, name) => fs.readFileSync(path.join(dir, name), "utf8");
const lined = (...rows) => `${rows.join("\n")}\n`;
const outcome = (stack, passed, failed) => ({ stack, passed, failed });
const record = (title, bug, repaired, verdict) => ({ title, bug, repaired, verdict });

/** Runs `fn` on a new directory holding the files given (a path and its text each), then removes it. */
function inTree(files, fn) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "h2h-museum-"));
  try {
    Object.entries(files).forEach(([file, text]) => {
      fs.mkdirSync(path.dirname(path.join(root, file)), { recursive: true });
      fs.writeFileSync(path.join(root, file), text);
    });
    return fn(root);
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
}

// --- parseEvidence ------------------------------------------------------------

test("the museum's evidence of record is read into the five exhibits: title, where the bug was shown, its repair and Pleris's verdict", () => {
  assert.deepEqual(parseEvidence(readIn(FIXTURES, "museum.txt")), {
    "F01-markup-from-text": record("Text another user wrote runs as script in a customer's page", outcome("sveltekit", 1, 0), outcome("sveltekit", 0, 1), "refused (diagnostic PW5010)"),
    "G01-retried-command-twice": record(
      "One press applied twice when its request is delivered twice",
      outcome("next-react", 1, 0),
      outcome("next-react", 0, 1),
      "refused (diagnostic PW0338; runtime, ADR-0121's interaction per press, applied once)",
    ),
    "H07-n-plus-one": record("One round trip per row: a page whose work grows with its list", outcome("next-react", 1, 0), outcome("next-react", 0, 1), "refused (diagnostic PW0405)"),
    "J01-stale-page-after-save": record("A page shown after a save is the one read before it", outcome("next-react", 1, 0), outcome("next-react", 0, 1), "refused (runtime, ADR-0280, navigate after the commit)"),
    "J08-shared-server-state": record("One customer's cart shown to another, from state the server shares", outcome("sveltekit", 1, 0), outcome("sveltekit", 0, 1), "refused (diagnostic PW5001)"),
  });
});

const FIRST = "== A01-first: The first";
const BUG = "   next-react, the bug: 1 passed, 0 failed";

// [what it shows, the evidence's lines, what is read]
const PARSES = [
  [
    "lines before any exhibit head are not read, however much they look like an exhibit's",
    ["== the exhibits (benchmarks/museum/museum.mjs)", BUG, "   next-react, repaired: 0 passed, 1 failed", "   => refused (x)"],
    {},
  ],
  ["an exhibit head alone is an exhibit with nothing recorded", [FIRST], { "A01-first": record("The first", null, null, null) }],
  [
    "a repair recorded with no bug is a repair and no bug",
    [FIRST, "   next-react, repaired: 0 passed, 2 failed (the spec fails: it detects the bug)"],
    { "A01-first": record("The first", null, outcome("next-react", 0, 2), null) },
  ],
  [
    "an exhibit's lines are the ones between its head and the next, so a second exhibit does not take the first's",
    [FIRST, BUG, "== B02-second: The second", "   => refused (x)"],
    { "A01-first": record("The first", outcome("next-react", 1, 0), null, null), "B02-second": record("The second", null, null, "refused (x)") },
  ],
  [
    "when a block says the same thing twice, the later line is the one kept",
    [FIRST, BUG, "   next-react, the bug: 3 passed, 0 failed", "   => refused (x)", "   => refused (y)"],
    { "A01-first": record("The first", outcome("next-react", 3, 0), null, "refused (y)") },
  ],
  [
    "an exhibit named twice is its later block alone",
    [FIRST, BUG, "   => refused (x)", "== A01-first: Again"],
    { "A01-first": record("Again", null, null, null) },
  ],
  [
    "lines under a later heading that is no exhibit's still belong to the last exhibit read",
    [FIRST, "   => refused (x)", "== mutation controls (scripts/proof_mutations.py)", "   => a verdict of the later section", "   sveltekit, repaired: 0 passed, 1 failed"],
    { "A01-first": record("The first", null, outcome("sveltekit", 0, 1), "a verdict of the later section") },
  ],
  [
    "only an indented line is read, and a tab indents as well as spaces do",
    [FIRST, "\tnext-react, the bug: 1 passed, 0 failed", "next-react, the bug: 2 passed, 0 failed", "next-react, repaired: 0 passed, 1 failed", "=> refused (flush)"],
    { "A01-first": record("The first", outcome("next-react", 1, 0), null, null) },
  ],
  [
    "a bug that stopped before its spec, and a repair that was not run, record no counts",
    [FIRST, "   next-react, the bug: stopped at build", "   next-react, repaired: not run (--checks-only)", "   => miss (checks clean today)"],
    { "A01-first": record("The first", null, null, "miss (checks clean today)") },
  ],
  [
    "counts are digits and a stack is lower-case letters and hyphens: anything else records nothing",
    [FIRST, "   next-react, the bug: one passed, 0 failed", "   Next-React, the bug: 1 passed, 0 failed", "   next_react, repaired: 1 passed, 0 failed"],
    { "A01-first": record("The first", null, null, null) },
  ],
  [
    "what follows a count, and the whole of a verdict, are read as the museum wrote them",
    [FIRST, "   next-react, the bug: 0 passed, 1 failed  <- the bug did not happen", "   next-react, repaired: 1 passed, 0 failed  <- the repair did not make the spec fail", "   => NOT refused (checks clean)  <- NOT AS THE EXHIBIT EXPECTS"],
    { "A01-first": record("The first", outcome("next-react", 0, 1), outcome("next-react", 1, 0), "NOT refused (checks clean)  <- NOT AS THE EXHIBIT EXPECTS") },
  ],
  [
    "a verdict is what follows the arrow and one space, so a second space is kept and no space makes no verdict",
    [FIRST, "   =>refused", "   =>  two spaces"],
    { "A01-first": record("The first", null, null, " two spaces") },
  ],
  ["an arrow and a space with nothing after them is an empty verdict", [FIRST, "   => "], { "A01-first": record("The first", null, null, "") }],
];

PARSES.forEach(([name, rows, expected]) => test(name, () => assert.deepEqual(parseEvidence(lined(...rows)), expected)));

// [an exhibit head, the id it names, the title it gives]
const HEADS_READ = [
  ["== F01-x: Title", "F01-x", "Title"],
  ["== F1-x: Title", "F1-x", "Title"],
  ["== F01-x: ", "F01-x", ""],
  ["== F01-x-9: A: B", "F01-x-9", "A: B"],
];
const HEADS_NOT = ["== f01-x: Title", "== F-x: Title", "== F01-X: Title", "== F01-x:Title", "==F01-x: Title", " == F01-x: Title", "== F01-: Title", "== F01: Title", "== FF01-x: Title"];

test("an exhibit head is ==, a capital, digits, a hyphen, a slug of lower-case letters, digits and hyphens, a colon, a space, and a title", () => {
  HEADS_READ.forEach(([head, id, title]) => assert.deepEqual(parseEvidence(`${head}\n`), { [id]: record(title, null, null, null) }, head));
});

test("a line not shaped like an exhibit head names no exhibit", () => {
  HEADS_NOT.forEach((head) => assert.deepEqual(parseEvidence(`${head}\n`), {}, head));
});

// --- collect ------------------------------------------------------------------

const collectFixture = (over = {}) => collect({ museum: EXHIBITS, evidence: EVIDENCE, ...over });

test("the five exhibits and the evidence of record are collected into the museum's rows: sources, stacks, whether shown, and Pleris's verdicts", () => {
  const collected = collectFixture();
  assert.deepEqual(collected.meta, {
    commit: "5a4378fe51fd38460239ffb295f9e4fea9bfcbac",
    ci: "https://github.com/studiokimeiga/perfect-web/actions/runs/39892771765",
    summary: "museum: 5 exhibits, 5 refused, 0 missed, 0 not as expected",
  });
  assert.deepEqual(collected, JSON.parse(readIn(FIXTURES, "collected.json")));
});

test("a file in the museum's directory, and a directory with no exhibit.json, are no exhibit", () => {
  assert.deepEqual(fs.readdirSync(EXHIBITS).sort(), [...EXHIBIT_IDS, "NOTES.txt", "Z99-unfinished"]);
  assert.deepEqual(collectFixture().exhibits.map((e) => e.id), EXHIBIT_IDS);
});

test("exhibits are read in the sorted order of their directories' names, however the museum lists them", (t) => {
  const readdir = fs.readdirSync;
  t.mock.method(fs, "readdirSync", (...args) => readdir(...args).reverse());
  assert.deepEqual(collectFixture().exhibits.map((e) => e.id), EXHIBIT_IDS);
});

test("with no evidence file, nothing is recorded: no commit, ci or summary, every exhibit not recorded, not shown and not refused", () => {
  const { meta, exhibits } = collectFixture({ evidence: path.join(FIXTURES, "no-such-museum.txt") });
  assert.deepEqual(meta, { commit: null, ci: null, summary: null });
  assert.deepEqual(exhibits.map((e) => [e.id, e.shown, e.pleris, e.refused]), EXHIBIT_IDS.map((id) => [id, false, "not recorded", false]));
  assert.deepEqual(exhibits[0].source, "GHSA-9f4f-jv96-8766 (CVE-2025-46719); S21, S27 (a CVE, the advisory of the app that had it (Open WebUI, a SvelteKit app); security guidance; W3C specification)");
});

const NO_META = { commit: null, ci: null, summary: null };

// [what it shows, the evidence's text, the meta read]
const METAS = [
  [
    "the commit is the first commit line's word, the ci the first word of its line, and the summary the museum's own line",
    lined("commit: 5a4378fe51fd", "ci: https://ci.example/runs/7 (Ubuntu 24.04.5 LTS on GitHub Actions, x86_64)", "museum: 3 exhibits, 3 refused, 0 missed, 0 not as expected"),
    { commit: "5a4378fe51fd", ci: "https://ci.example/runs/7", summary: "museum: 3 exhibits, 3 refused, 0 missed, 0 not as expected" },
  ],
  ["a run with no ci line has a null ci and, with no summary line, a null summary", lined("commit: 5a4378fe51fd"), { commit: "5a4378fe51fd", ci: null, summary: null }],
  [
    "a commit line that is not first is read, and one with other words after it gives its first word, whatever characters it holds",
    lined("produced by: just e14-museum", "commit: 5a4378fe51fd-dirty and more"),
    { commit: "5a4378fe51fd-dirty", ci: null, summary: null },
  ],
  [
    "the first line of each kind is the one read",
    lined("commit: aaaa", "commit: bbbb", "ci: https://one", "ci: https://two", "museum: first", "museum: second"),
    { commit: "aaaa", ci: "https://one", summary: "museum: first" },
  ],
  [
    "an indented line, or a summary after a label, is none of the three",
    lined("  commit: aaaa", " ci: https://one", "baseline: museum: 4 exhibits, 4 refused, 0 missed, 0 not as expected"),
    NO_META,
  ],
  ["an evidence file with nothing in it records nothing", "", NO_META],
];

test("the commit, the ci and the summary are read from their lines of the evidence, or are null", () => {
  METAS.forEach(([why, text, expected]) =>
    inTree({ "museum.txt": text }, (root) => assert.deepEqual(collectFixture({ evidence: path.join(root, "museum.txt") }).meta, expected, why)),
  );
});

const SHOWN = ["   next-react, the bug: 1 passed, 0 failed", "   next-react, repaired: 0 passed, 1 failed (the spec fails: it detects the bug)"];

// [an exhibit's id, its block in the evidence (null: none), whether its bug is shown, the verdict read, whether it is refused]
const RECORDS = [
  ["A01-shown-and-refused", [...SHOWN, "   => refused (diagnostic PW0405)"], true, "refused (diagnostic PW0405)", true],
  ["A02-no-block", null, false, "not recorded", false],
  ["A03-block-with-no-verdict", SHOWN, true, "not recorded", false],
  [
    "A04-bug-did-not-happen",
    ["   next-react, the bug: 0 passed, 1 failed  <- the bug did not happen", SHOWN[1], "   => miss (checks clean today)"],
    false,
    "miss (checks clean today)",
    false,
  ],
  ["A05-bug-with-a-failure", ["   next-react, the bug: 2 passed, 1 failed", SHOWN[1], "   => refused after all (PW0405)"], false, "refused after all (PW0405)", true],
  [
    "A06-no-test-ran",
    ["   next-react, the bug: 0 passed, 0 failed", SHOWN[1], "   => NOT refused (checks clean)  <- NOT AS THE EXHIBIT EXPECTS"],
    false,
    "NOT refused (checks clean)  <- NOT AS THE EXHIBIT EXPECTS",
    false,
  ],
  ["A07-bug-stopped-at-build", ["   next-react, the bug: stopped at build", SHOWN[1], "   => refused (diagnostic PW0405)"], false, "refused (diagnostic PW0405)", true],
  [
    "A08-repair-passes-too",
    [SHOWN[0], "   next-react, repaired: 1 passed, 0 failed  <- the repair did not make the spec fail", "   => refused (diagnostic PW0405)"],
    false,
    "refused (diagnostic PW0405)",
    true,
  ],
  ["A09-repair-not-run", [SHOWN[0], "   next-react, repaired: not run (--checks-only)", "   => refused (diagnostic PW0405)"], false, "refused (diagnostic PW0405)", true],
  ["A10-many-specs-and-a-capital", ["   next-react, the bug: 3 passed, 0 failed", "   next-react, repaired: 1 passed, 2 failed", "   => Refused (diagnostic PW0405)"], true, "Refused (diagnostic PW0405)", false],
  [
    "A11-stack-of-the-record",
    ["   sveltekit, the bug: 1 passed, 0 failed", "   sveltekit, repaired: 0 passed, 1 failed", "   => refused (runtime, ADR-0280, navigate after the commit)"],
    true,
    "refused (runtime, ADR-0280, navigate after the commit)",
    true,
  ],
  ["A12-empty-verdict", [...SHOWN, "   => "], true, "", false],
  ["A13-verdict-that-only-begins-refuse", [...SHOWN, "   => refuses (diagnostic PW0405)"], true, "refuses (diagnostic PW0405)", false],
];

const exhibitOf = (id) => ({ id, leaf: id.slice(0, 3), title: `Title of ${id}`, source: { ref: "S1, S2", kind: "a kind" }, repro: { stack: "next-react" } });
const evidenceOf = (records) => lined(...records.flatMap(([id, block]) => (block ? [`== ${id}: Title of ${id}`, ...block] : [])));

test("an exhibit's bug is shown where its spec passed on the bug with no failure and failed on the repair, and its verdict is what Pleris's line says", () => {
  const files = { "evidence.txt": evidenceOf(RECORDS) };
  RECORDS.forEach(([id]) => (files[`museum/${id}/exhibit.json`] = JSON.stringify(exhibitOf(id))));
  inTree(files, (root) => {
    const { exhibits } = collect({ museum: path.join(root, "museum"), evidence: path.join(root, "evidence.txt") });
    assert.deepEqual(
      exhibits.map((e) => [e.id, e.stack, e.shown, e.pleris, e.refused]),
      RECORDS.map(([id, , shown, pleris, refused]) => [id, "next-react", shown, pleris, refused]),
    );
    assert.deepEqual(exhibits[0], {
      id: "A01-shown-and-refused",
      leaf: "A01",
      title: "Title of A01-shown-and-refused",
      source: "S1, S2 (a kind)",
      stack: "next-react",
      shown: true,
      pleris: "refused (diagnostic PW0405)",
      refused: true,
    });
  });
});

test("with no directories given, the museum read is the repository's benchmarks/museum and its evidence docs/evidence/E14/museum.txt", () => {
  const byDefault = collect();
  assert.deepEqual(byDefault, collect({ museum: path.join(REPO, "benchmarks/museum"), evidence: path.join(REPO, "docs/evidence/E14/museum.txt") }));
  assert.ok(byDefault.exhibits.length > 0);
});

// --- render -------------------------------------------------------------------

const rendered = (result) => lines(render(result));
const NOT_REPRODUCED = "  (an exhibit shown in one framework is not reproduced in the other: neither claimed to let it ship nor to refuse it)";
const SHOWN_ON_ITS_STORE = "each shown happening on its built store, its checker accepting it";

test("the recorded museum result renders to the museum evidence of record, line for line", () => {
  const result = JSON.parse(readIn(RECORDED, "museum.json"));
  assert.deepEqual(rendered(result), lines(readIn(RECORDED, "museum.txt")));
});

const exhibit = (id, stack, shown, pleris, refused) => ({ id, leaf: id.slice(0, 3), title: `The ${id} bug`, source: `${id} source (a kind)`, stack, shown, pleris, refused });

test("a result with no ci and no summary says so, counts every exhibit but lists only those shown, and keeps an exhibit of another stack out of both lists", () => {
  const result = {
    meta: { commit: "5a4378fe", ci: null, summary: null },
    exhibits: [
      exhibit("A01-first", "next-react", true, "refused (diagnostic PW0405)", true),
      exhibit("B02-second", "next-react", false, "refused (runtime, ADR-0280)", true),
      exhibit("C03-third", "sveltekit", false, "not recorded", false),
      exhibit("D04-fourth", "pleris", true, "miss (checks clean today)", false),
    ],
  };
  assert.deepEqual(rendered(result), [
    "the museum's exhibits (ADR-0342), as just e14-museum recorded them: commit 5a4378fe; no summary",
    `  Next.js    lets ship 1 of the 4, ${SHOWN_ON_ITS_STORE}: A01-first (The A01-first bug)`,
    `  SvelteKit  lets ship 0 of the 4, ${SHOWN_ON_ITS_STORE}: none`,
    NOT_REPRODUCED,
    "  Pleris     refuses 2 of the 4: A01-first: refused (diagnostic PW0405); B02-second: refused (runtime, ADR-0280); C03-third: not recorded; D04-fourth: miss (checks clean today)",
    "  not claimed: that five exhibits sample the census (ADR-0342); each exhibit's source and its kind: A01 A01-first source (a kind); B02 B02-second source (a kind); C03 C03-third source (a kind); D04 D04-fourth source (a kind)",
    "",
  ]);
});

test("a result with a ci and a summary prints them after the commit, and lists each framework's shown exhibits with their titles, in the order given", () => {
  const result = {
    meta: { commit: "abc", ci: "https://ci.example/runs/7", summary: "museum: 4 exhibits, 4 refused, 0 missed, 0 not as expected" },
    exhibits: [
      exhibit("A01-first", "next-react", true, "refused (diagnostic PW0405)", true),
      exhibit("B02-second", "sveltekit", true, "refused (diagnostic PW5010)", true),
      exhibit("C03-third", "next-react", true, "refused (diagnostic PW0338)", true),
      exhibit("D04-fourth", "sveltekit", true, "refused (diagnostic PW5001)", true),
    ],
  };
  assert.deepEqual(rendered(result), [
    "the museum's exhibits (ADR-0342), as just e14-museum recorded them: commit abc, https://ci.example/runs/7; museum: 4 exhibits, 4 refused, 0 missed, 0 not as expected",
    `  Next.js    lets ship 2 of the 4, ${SHOWN_ON_ITS_STORE}: A01-first (The A01-first bug); C03-third (The C03-third bug)`,
    `  SvelteKit  lets ship 2 of the 4, ${SHOWN_ON_ITS_STORE}: B02-second (The B02-second bug); D04-fourth (The D04-fourth bug)`,
    NOT_REPRODUCED,
    "  Pleris     refuses 4 of the 4: A01-first: refused (diagnostic PW0405); B02-second: refused (diagnostic PW5010); C03-third: refused (diagnostic PW0338); D04-fourth: refused (diagnostic PW5001)",
    "  not claimed: that five exhibits sample the census (ADR-0342); each exhibit's source and its kind: A01 A01-first source (a kind); B02 B02-second source (a kind); C03 C03-third source (a kind); D04 D04-fourth source (a kind)",
    "",
  ]);
});

test("a result with no exhibits and no commit renders its zeros, with none shown and empty lists after the colons", () => {
  assert.deepEqual(rendered({ meta: NO_META, exhibits: [] }), [
    "the museum's exhibits (ADR-0342), as just e14-museum recorded them: commit null; no summary",
    `  Next.js    lets ship 0 of the 0, ${SHOWN_ON_ITS_STORE}: none`,
    `  SvelteKit  lets ship 0 of the 0, ${SHOWN_ON_ITS_STORE}: none`,
    NOT_REPRODUCED,
    "  Pleris     refuses 0 of the 0: ",
    "  not claimed: that five exhibits sample the census (ADR-0342); each exhibit's source and its kind: ",
    "",
  ]);
});
