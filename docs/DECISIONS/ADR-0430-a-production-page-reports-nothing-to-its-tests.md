# ADR-0430: A production page reports nothing to its tests

Status: accepted 2026-11-11, to land with train-25. Proposed 2026-11-11, on
branch `perf/train-25-20261111`, under the owner's delegation of 2026-10-02
and order of 2026-10-25 ("deciding by best judgement without asking"), and the
order of 2026-11-09 that every row of the head-to-head Svelte wins is a defect
until it is fixed or recorded. Milestone: E14. Builds on ADR-0365, ADR-0400,
ADR-0413 and ADR-0429.

## Context

The runtime tells its tests what it does. `window.__pw` is an object of
thirteen members, among them its log: 73 lines written in the source, the
last 4,000 kept. As the runtime works it writes counters and snapshots into
the object: what it fetched, read, refused, ignored and settled, its signals,
its transport. And it sets four hooks a test drives it by: `__pwTestApply`,
`__pwTestBatch`, `__pwHeld` and `__pwKnown`.

- **Every production page carried all of it.** TodoMVC's one script, the
  device's runtime and its page's code in one module (ADR-0426), held 2,580
  bytes of it decoded and 694 with Brotli; the large list's 1,879 and 576; a
  server's page's runtime, the store's, 5,272 and 1,436.
- **No page read it, and no developer.** A failure the log records is in no
  console. ADR-0400 put a refusal on the console for that reason, and left
  the log's line beside it.
- **Two of its members were the runtime's own.** `window.__pw.reloading`
  was read to read a page again once, not twice. And the move a row's
  reconciliation reported was the very call that moved it:
  `window.__pw.moveKind = moveNodes(...)`.
- **The suites read it**: on development builds, and in `e14-device-runtime`
  on production ones. TodoMVC's suite read its refusals from the log, and 12
  of the large list's 28 tests compare the document with the page's own
  signals, or read its log and its index.
- **The head-to-head measures production builds**, so the reports were in
  every one of its byte rows.

## Decision

1. **A report is a statement labeled `REPORT`.** Each of the 109 statements
   that write the log, the object or a hook is labeled so, the object's
   assignment among them. As written, a label is nothing: a development
   build, whose runtime is served as written, reports as before.
2. **A production build drops each.** The packaging gives esbuild
   `dropLabels: ["REPORT"]` for the device's runtime, a page alone in its
   build and a server's page's runtime: each labeled statement goes, its
   arguments with it (esbuild 0.28.2, checked: a call among a dropped
   report's arguments goes too, and an empty `catch` stays). Its
   `delivery.json` says `report: false`.
3. **A suite that reads the reports asks for them**: `pw build --dist DIR
   --production --report`, which hands the packaging `--report`, and its
   `delivery.json` says `report: true`. `--report` without `--production` is
   refused: a development build reports already.
4. **Nothing the runtime does reads a report.** `reloading` is the runtime's
   own variable; the move is made, then reported. A test reads the source:
   the log and the object are in `REPORT` statements alone, and a report
   calls nothing but what reads a value (`push`, `join`, `JSON.stringify`
   and the like) and sets nothing but the object.
5. **A test that reads the page's reports says so**, tagged `@report`.
   `just e14-device-runtime` runs its four suites on the builds as they are
   served, less those tests, and then the large list's `@report` tests on
   the large list built to report. A test that reads a report untagged fails
   on the build as served, where `window.__pw` is not; one tagged that is
   given such a build says how to build one.
6. **TodoMVC's refusals are read where a developer reads them**, on the
   console (ADR-0400's `Pleris: handler 5 not attached: PWR04, ...`), on the
   build as served.

## Why not the other ways

- **A constant, `PW_REPORT`, folded as the features are** (ADR-0413): the
  same bytes, but each of the 109 reports an `if`, and dozens of the
  runtime's functions over the analyzers' ceilings for branches no
  production page takes. A label adds none.
- **The suites watch only what a page does** (the document, the console,
  the network): the large list's tests compare the document with the
  page's own signals, and its controls are killed by lines no document
  shows ("parses no row"). Suites that read only the document would hold
  the runtime to less.
- **Reports as codes, not words**: smaller, but still in every page, and
  still read by nothing there.
- **Every production build reports, and the head-to-head subtracts them**:
  the bytes measured would not be the bytes sent.
- **A failure's line on the console instead of the log**: a production page
  should tell a developer of a failure it recovers from, but which failures,
  in what words and at what cost is a decision of its own (NEXT).

## Result

Each app's development build packed twice, by the runtime and packaging
before this and after, as written and with Brotli at 11:

| | before | after |
|---|---|---|
| TodoMVC's one script | 48,670 / 16,054 | 46,090 / 15,360 |
| the large list's one script | 45,639 / 15,526 | 43,760 / 14,950 |
| TodoMVC's device runtime | 41,698 / 14,324 | 39,110 / 13,622 |
| a server's page's runtime | 71,794 / 23,247 | 66,522 / 21,811 |

The head-to-head's record is the train's, after its run.

## Consequences

- `pw build --dist DIR --production --report`; `rows.sh` asks for it where
  `PW_REPORT` is 1; `node optimize-assets.mjs DIR [--report]`.
- The runtime's own globals stay, which are no reports: `__pwUnloading`,
  `__pwUnloadWatched` and `__pwSoftNavigations`, which carry what a page
  holds across the runtime a soft navigation loads.
- Tests: four in `optimize-assets.test.mjs` (a production build holds none
  of the reports, a build for a suite keeps them, each report is a
  statement labeled `REPORT` that does nothing but report, a page alone in
  its build holds none) and two in `whole_app.rs` (the packaging is asked to
  keep reports only where `--report` asks, and `--report` alone is
  refused).
- Controls: eleven in `scripts/device_runtime_mutations.py`, three in
  `scripts/whole_app_mutations.py`, each killed by a failing test. Eleven
  anchors in seven scripts name a report's line as the runtime now holds
  it, labeled, and the packaging's with `report`; none changes what its
  mutant undoes.
- `e2e/rows.spec.mjs`: 12 tests tagged `@report`. `e2e/device-runtime.spec.mjs`
  reads the console, and its device runtime's bound is 14,000 bytes as
  sent, from 15,500, of a build that says it reports nothing.
- The analyzers (ADR-0411): the labels add no branch, and the audit finds
  one function above its ceiling, the source test's reader of a line
  without its strings (`codeOf`, cyclomatic 23), whose ceiling is its score
  and one. Run here before the push for the first time: Fallow 3.23.0, the
  version the pull request's audit runs (MIT, `fallow-rs/fallow`), from npm
  with `--ignore-scripts` into `~/.local/share/fallow-3.23.0`, 137 MB with
  its platform binary, which it reports signed and verified; no dependency
  of the repository's.
- Left for NEXT: a failure the runtime recovers from is told no developer,
  in a production build as in a development one.
