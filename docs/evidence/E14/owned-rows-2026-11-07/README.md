# A row says who may change it: PW0370 and `owner` (2026-11-07)

Evidence for `docs/DECISIONS/ADR-XXXX-a-row-says-who-may-change-it.md`.
Branch `feat/owned-rows-20261107` (draft PR #91, base
`feat/mandatory-requires-20261106`, PR #83), from #83's head `9b348f1c`
and the ADR's commit `8e0c71f6`:

- `79eb2ce7`, the change;
- `ac17131c`, PostgreSQL's owned put guarded on the row its conflict found
  (a race at read committed, found while building);
- `ecc229f6`, the independent review's repairs (a scheduled write is the
  reader's; the adapters agree on `written`; four tests sharpened);
- `1c714776`, R-098's charter category in `corpus-check`, and the
  PostgreSQL tests asserting through their own crate's helper (both found
  by the first `verify` run).

Machine: 13th Gen Intel i9-13900K, 32 logical CPUs; Ubuntu on WSL2; rustc
1.97.1; PostgreSQL 18 (local, systemd); Node 22.21.1. Its own target
directory, built with no debug info and no incremental cache.

`scripts/` holds what made the migration of the adapters' tests and what
listed the reached mutation scripts:
- `reader_helper.py`: each test's `begin()` and `transaction(..)` given a
  test reader (`a_reader()`), in both adapters' tests and `pw-cli`'s;
- `schema_owner.py`: each `TableSchema { .. }` literal in a test given
  `owner: None`;
- `reached.py` (the requires ADR's, unchanged): the mutation scripts with
  an anchor within 3 lines of a changed line (`logs/reached.txt`, for
  `8e0c71f6..79eb2ce7`).

## The rule's mutation script, whole

`CARGO_INCREMENTAL=0 python3 scripts/who_may_change_mutations.py`, with the
local PostgreSQL named (`PW_STORE_DATABASE_URL`). Its producer is D3's
strict one: each test binary's whole named inventory, and every failure an
assertion's in the binary's own sources; a mutant that does not build, or
fails elsewhere, is INVALID, never killed.

| Commit | Mutants | Result | Log |
|---|---|---|---|
| `ac17131c` | 35 | 34 killed, 1 INVALID: "PostgreSQL makes no owner column", whose race test's setup failed at an `expect`, outside an assertion; the setup asserts since | `logs/mut-who_may_change-ac17131c.log` |
| `ecc229f6` | 39 | 39 of 39 killed | (the same as below) |
| `1c714776` | 39 | **39 of 39 killed**, the baseline's 48 tests passing | `logs/mut-who_may_change.log` |

The first whole run, at `79eb2ce7`, stopped at its baseline: the parser's
library holds a `should_panic` test the producer does not count, so the
script runs the parser's table tests alone. A second, at `79eb2ce7` with
that, was stopped by SIGINT after 8 kills to make the race fix
(`mutation_source` restored the applied mutant: `git status` clean,
`mutation_anchors.py` 5017 of 5017 after).

Also at `ac17131c`: the race test, alone, under the put's guard removed
(`OR true`), fails with Bob's text in Alice's row, and passes with the
guard (run by hand; the script's mutant "a put on PostgreSQL changes
another reader's row" is the same change, killed).

D3's script, for the mutant its `verify` run judged INVALID at `79eb2ce7`
("a put is refused by its own row's unique value"), at `1c714776`: killed,
5 of 59 tests failing (`python3 scripts/d3_postgres_mutations.py "a put is
refused by its own"`).

## Before each commit

- **The whole compiler** (`cargo test -p pw-core --no-fail-fast`,
  `logs/pw-core.log.gz`, before `79eb2ce7`): 1830 passed, 1 failed, the
  published generality figure (66 -> 67, the new `who_may_change`), then
  updated in `tests/generality.rs`. `docs/STATUS.md` and `docs/NEXT.md` are
  the integrator's (PARALLEL.md), so the figure moves there at the landing.
  An earlier run had stopped at `checking_source.rs`: A-050 was read before
  its migration, and the corpus's enforcement table predated R-098
  (`just evidence-corpus`: 97/97 enforced, committed).
- **Every suite the change touches** (`logs/suites.log`, before
  `79eb2ce7`): `pw-syntax`, `pw-host`, `pw-table-memory`,
  `pw-table-postgres` (on PostgreSQL), `pw-cli` (the guide's 15 among
  them), and `pw-core`'s `who_may_change`, `generality`, `handles`,
  `checking_source`, `tables` and clause tests: 515 passed, 0 failed.
  Before `ecc229f6`: the rule's, generality, the corpus, tables, scheduled,
  invoked-by and handles tests, `pw-host`, both adapters and `pw-cli`'s
  guide and tables, 373 passed, 0 failed; the guide again 15 of 15.
- **`just preflight`** (`logs/preflight.log`, before `79eb2ce7`): `cargo fmt
  --check`, clippy `-D warnings` over the workspace and `pw-host`'s engine,
  the case check, 5017 mutation anchors in 326 scripts. Its first run found
  an unneeded `let` in the adapters' test harness, fixed. Before each later
  commit: `cargo fmt --all`, clippy `-D warnings` on the crates it changed,
  `mutation_anchors.py` (5022 in 326 scripts at `1c714776`).
- **`just evidence-gates`** at `1c714776` (`logs/evidence-gates.log.gz`):
  1177 tests OK (5 skipped); Node's 19 of 19.

## On GitHub

A pull request into another branch than `master` plans no `verify`, so each
was dispatched on the branch with the recipes `ci_plan.py --changed` plans:

- **Run 42339343014** at `79eb2ce7` (35 recipes in 24 shards, and the
  browser suites in three engines; `logs/verify-42339343014-summary.txt`):
  31 recipes and every browser suite passed. Four failed:
  - `e14-d3-postgres`: one of D3's own mutants judged INVALID, this
    branch's test asserting through another crate's helper (fixed in
    `1c714776`, above);
  - `e14-page-policy` and `e14-registered-codes`: each runs `corpus-check`,
    which refused R-098's category (fixed in `1c714776`) and R-097's, the
    requires ADR's (#83's);
  - `e14-scheduled`: five server tests whose store `pw` no longer builds,
    for two commands a server test adds with no `requires` (PW0367, #83's;
    `logs/found-pr83-store-scheduled.log`, reproduced here).
- **Run 42342749184** at `ecc229f6` (`e14-d2-writes`, `e14-d3-postgres`):
  superseded by `1c714776` and cancelled; its `e14-d2-writes` and browser
  suites had passed.
- **Run 42345065803** at `1c714776` (`e14-d2-writes`, `e14-d3-postgres`):
  **success**, every job, the browser suites in three engines among them.
  Fetched with `just evidence-fetch 42345065803`:
  `docs/evidence/E14/d2-writes.txt` (69 of 69 mutants killed) and
  `docs/evidence/E14/d3-postgres.txt` (PostgreSQL 18.6; 54 of 54 mutants
  killed, its baseline 59 tests: the five binaries it lists and
  `owned_rows`' 9, which its inventory holds since this change), and
  `scripts/ci_seconds.json`'s two recipes' times.

**#83's own CI fails the same way**, which this branch inherits: `ci`'s
Rust tests fail the same 6 tests in `pw-conformance` and the same 14 in the
development server on #83 (run 42332222311) as on #91 (run 42342709320).
Each is a program inside a test, written before PW0367, refused by it for a
command with no `requires` (`pw-conformance`'s `tipped`, `sized`, `maybe`,
`either`, ..., read from the run's log; the server's `add_in_five` and
`sweep_in_five`, reproduced here). And `test-compile`'s `corpus-check`
refuses R-097's category. They are #83's to repair; this branch carries none
of those repairs, so that the two do not collide.
