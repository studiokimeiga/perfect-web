# ADR-0408: pleris.dev shows the head-to-head's record, never a copy

Status: accepted 2026-11-08, landed with train-15 under the owner's delegation
of 2026-10-02, on the owner's request of 2026-11-07: "make sure the website
can prove it". Milestone: E14.

## Context

pleris.dev's home page showed two tables of the head-to-head (ADR-0392's
instrument), sixteen rows, and a score: Pleris "leads 6 of the 16 rows".

- **The rows were a copy, typed by hand** from an exploratory run on PR
  #76's branch (2026-11-06), before its review. Nothing held them to a
  record, and the record at train-15 differs from them in most rows: the
  store's median latency under load is 0.27 ms where the page said 1.09,
  and its server CPU a request 0.49 ms where it said 0.20.
- **The score counted the rows the page chose**, so choosing the rows set
  the score.
- **A marked cell was the only verdict**: its weight and colour said who was
  ahead, which a screen reader does not read out.

The home page's claim has a test ("no other framework wins a measured row"),
and the claim is only as good as the way its numbers reach the page.

## Decision

1. **The instrument writes its verdicts as JSON.** `head-to-head.mjs
   --verdicts <file>` (and `--render` from a kept run) writes the measured
   commit and whether the tree was changed from it, the machine, the runs a
   cell holds, the shaping, the problems that refuse the run, and each row
   Pleris is in: its values as the report shows them and the report's own
   verdict (`classify`, through `verdictRows`), with the stacks ahead of
   Pleris on a loss. `just e14-head-to-head` writes
   `docs/evidence/E14/head-to-head.json` beside the text record, from the
   same run.
2. **The site reads that file when it is built** (`site/build.mjs`):
   - the home page's tables name their rows by app and by the instrument's
     words (`SPEED`), under the page's own names. A row the record does not
     hold stops the build;
   - each value is the record's, the stacks the instrument found ahead are
     marked, and a last column, "Ahead", names them: Pleris, each stack
     ahead of it, or none;
   - the standing in the hero counts the whole record, not the rows a page
     chose: "Of the 151 rows its head-to-head measures, it is ahead in 55
     and behind in 36" (the record at `d10fd577`);
   - `/measured`, a new page, shows every row in the instrument's words,
     with its verdict;
   - under the tables: the measured commit, the day and the machine, and
     links to the record and its commands at the commit that holds the
     record.
3. **The build stops on a record it may not show** (`checked`): a run the
   instrument's rules refused, in which nothing is a measurement; a run on a
   tree changed from its commit; a verdict the site does not know, or a
   loss that names no stack; and a record changed since its last commit,
   which its link would not show.
4. **Held on both sides.** `site/build.test.mjs` reads the record and the
   pages' HTML and judges each row's marks itself. Four mutants in
   `head_to_head_mutations.py` (a loss naming no stack, a verdict not the
   report's, a refused run's problems dropped, a changed tree written clean)
   and seven in `site_mutations.py` (`just e14-site`: a refused run shown, a
   changed tree shown, a loss naming no stack shown, a cell from another
   stack, an unmarked stack ahead, ties counted as losses, `/measured`
   without its ties) are each killed by an assertion of their witness.

## Consequences

- The site's numbers change only with a recorded run: `just
  e14-head-to-head`, its record committed, then a build and a deploy from a
  pushed commit, so that each link resolves.
- **A row at the line moves.** Two runs with no app changed between them
  (`97b6b918`, `d10fd577`) differ in ten rows' verdicts, each within a few
  milliseconds or hundredths, where the spread rule draws its line; the
  standing moved from 38 rows behind to 36. `/measured` says so. A standing
  read across several runs is the instrument's to add, not the site's.
- The home page shows correctness and code beside speed, from the same
  record and judged alike: TodoMVC's specification and Cypress suite, the
  bug classes each store lets ship, the twelve changes, and the store's
  lines with the server code it needs, which is Pleris's largest loss
  (2,422 lines against 151 and 134: its storage is written in Rust).
- Rejected:
  - a hand-kept copy with a test against the record: two copies to keep in
    step, for no gain;
  - reading the record from GitHub as the page loads: the site runs no
    script (`site/README.md`);
  - a score over the rows shown: the page's choice of rows would set it.
