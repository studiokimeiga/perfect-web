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
  by the first `verify` run);
- `70dec392`, #83's repairs (`bb36ac61`) merged;
- `bc4cd704`, the second review's repairs (the last section).

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

## The second review (at `70dec392`), and its repairs (`bc4cd704`)

`bc4cd704` holds the review's four repairs and its two records (the ADR's
"The second review"). Each repair was reproduced, or shown by a test,
before it was made, and each test fails without its repair:

- **The put answered by its snapshot.** The reviewer's own script
  (`scripts/race_delete_put.sh`, copied unchanged) reproduces it on the
  local PostgreSQL 18.6: `count 1, old_held t, pw_mine f`, Bob's row in the
  table (`logs/race-delete-put.log`). `scripts/race_replaced_put.sh` makes
  the mirror case: `count 0, old_held t, pw_mine t`, Alice's row left
  (`logs/race-replaced-put.log`). Each is refused (40001) where both
  transactions are serializable. With the repair undone by hand
  (`git checkout` of `lib.rs` for one run), `a_put_is_answered_by_what_it_
  wrote_not_by_its_snapshot` fails at its first race, `owned_race_deleted`,
  and passes with the repair.
- **The admits call untested.** With the call made to admit any writer by
  hand, the ported `a_readers_command_is_never_the_platforms` fails at its
  first assertion (the platform's run admitted), and passes with it.
- **The owners given up, and the clock's runs**: the compiler's, the
  memory adapter's and PostgreSQL's tests, each held by the script's
  mutants (below).
- `just e8-contracts` regenerated `docs/evidence/E8/component-contracts.json`
  and its `.txt`. Two contracts change, each with its `abi_schema`: the
  store's `expire_order` (`runs_for`: reader) and `send_to_store`
  (platform, reader). The recipe's own host tests pass on them.
- PostgreSQL 18.6, checked for the follow-up the ADR names: `INSERT .. ON
  CONFLICT .. DO UPDATE .. RETURNING old.v, new.v, old IS NULL` answers the
  replaced row's value, and null for a row it inserted
  (`logs/returning-old.log`).

Before the commit:
- **Every crate the change touches, whole** (`cargo test --locked
  --no-fail-fast -p <crate>`, PostgreSQL named; `logs/suites-2.log.gz`):
  `pw-core` 1834, `pw-syntax` 121, `pw-host` 39, `pw-table-memory` 97,
  `pw-table-postgres` 61, `pw-cli` 43 passed; none failed.
- **`just preflight`** and **`just test-compile`**: both pass
  (`cargo fmt --check`, clippy `-D warnings` over the workspace and
  `pw-host`'s engine, the case check, 5037 mutation anchors in 326 scripts;
  every corpus file formatted and checked; `logs/preflight-2.log`,
  `logs/test-compile-2.log.gz`).
- **`just evidence-gates`** at `4df6414b`, with this section's files
  (`logs/evidence-gates-2.log.gz`): 1171 tests OK, 5 skipped, and 6 errors,
  every one `test_browser_assertion_proof` failing to load
  `@playwright/test`: this worktree's `node_modules` had been removed for
  disk. Linked to the main checkout's for that module alone, its 11 run, 8
  OK and 3 skipped by design (`logs/browser-assertion-proof.log`); the link
  was removed after. The pull request's `ci` runs `evidence-gates` whole
  (`just ci-without-unit`, the `non-unit` jobs): both passed at
  `4df6414b`.
- **`just evidence-gates` again**, after `just evidence-fetch` (below),
  with the main checkout's `node_modules` linked for the run
  (`logs/evidence-gates-3.log.gz`): the Python gates' 1177 tests OK, 5
  skipped. Node's 17 of 19 pass. The other 2 (the SvelteKit baseline's
  vendored `deepmerge` consumer test) cannot resolve
  `@sveltejs/vite-plugin-svelte` and `@sveltejs/adapter-node`: the main
  checkout's store holds them built against `vite@8.3.2`, and this branch's
  lockfile links the ones built against `vite@8.3.3`. They read nothing
  this change touches; the `non-unit` jobs ran them, and passed.
- **The reached scripts** (`logs/reached-2.txt`, `70dec392..bc4cd704`):
  the rule's, `tables_mutations.py` (re-anchored: `shaped` takes the
  table's schema), and `invoked_by`, `idempotent_commands` and `invariants`
  (anchors beside the contract's new field). The last four run on GitHub,
  below.

The rule's mutation script, whole (`CARGO_INCREMENTAL=0 python3
scripts/who_may_change_mutations.py`, PostgreSQL named), now 54 mutants,
its baseline 53 tests:

| Commit | Result | Log |
|---|---|---|
| `bc4cd704` | 50 killed, 4 INVALID: each failed the race test as intended (`owned_race_deleted` answered `Ok([])` for `Ok(["Notes"])`, and the like), at an `assert_eq!` written inside a match arm, whose line the strict producer does not count | `logs/mut-who_may_change-bc4cd704.log` |
| `4df6414b` | **54 of 54 killed**, the baseline's 53 tests passing; sources restored, `mutation_anchors.py` 5037 of 5037 after | `logs/mut-who_may_change-2.log` |

`4df6414b` puts each of the race test's assertions on a line of its own,
and the one arm of the same shape in the compiler's tests; it changes no
test's meaning.

On GitHub (`logs/dispatched-recipes-2.txt`), `verify` was dispatched with
the 8 recipes `ci_plan.py --changed 70dec392 HEAD` plans, with or without
`--current`, and with `e14-d2-writes` (its script runs every memory test
binary and `pw-host`'s library tests, both changed), `e14-scheduled` (the
development server's `payments.rs` changed) and `e14-guide` (the tour
changed):

- **Run 42365521769** at `bc4cd704`: cancelled for `4df6414b`, its D3
  shard running. Every shard that had ended had passed: the browser suites
  in three engines and ten of the recipes' shards.
- **Run 42368284590** at `4df6414b`: **success**, every job, the browser
  suites in three engines among them. Fetched with `just evidence-fetch
  42368284590`, each file's commit `4df6414b`:

  | Recipe | Its mutation controls | Its baseline |
  |---|---|---|
  | `e14-d3-postgres` (PostgreSQL 18.6) | 54 of 54 killed | 61 tests, `owned_rows`' 11 among them |
  | `e14-d2-writes` | 69 of 69 killed | 158 tests |
  | `e14-tables` | 44 of 44 killed | 41 tests |
  | `e14-invoked-by` | 14 of 14 killed | 9 tests |
  | `e14-idempotent-commands` | 6 of 6 killed | 70 tests |
  | `e14-invariants` | 25 of 25 killed | 12, 104 and 3 tests |
  | `e14-scheduled` | 37 of 37 killed | 43, 12 and 15 tests |
  | `e14-guide` | 74 of 74 killed | 22 tests, and 4 in Chromium |
  | `e14-payments` | 39 of 39 killed | 41 and 12 tests |
  | `e14-outbound-https` | 111 of 111 killed | 55 and 11 tests |
  | `e14-check-is-build` | 51 of 51 killed | 99 tests |

  `scripts/ci_seconds.json` keeps the eleven recipes' times
  (`e14-d3-postgres`: 1663 s, from 2304).

The pull request's `ci` at `4df6414b`: 23 checks pass (`weekly` skipped).
