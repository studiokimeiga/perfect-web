# A key that changes is asked where it changes: PW0325 folds into PW5309 (2026-11-06)

Evidence for `docs/DECISIONS/ADR-XXXX-a-key-that-changes-is-asked-where-it-changes.md`.
Branch `fix/stale-key-rule-20261106`, from `master` at `5d4d4d55`.

- Machine: 13th Gen Intel i9-13900K, 32 logical CPUs, 47 GiB; Ubuntu on
  WSL2; rustc 1.97.1; Node 22.21.1. Its own target directory.
- Other sessions' builds and tests ran on the machine at the same time, as
  did this session's two sibling branches; see the browser test below.
- `scripts/` holds each script that produced a log here, as it ran:
  `stale-tests.sh`, `stale-refix.sh`, `stale-verify.sh`, `stale-final.sh`, and `reached.py`, which
  lists the mutation scripts with an anchor within 3 lines of a changed line.

## Before the commit: four failures, fixed (`stale-tests.sh`, `stale-refix.sh`)

The first run of `pw-core` whole (`logs/pw-core.log.gz`, `logs/tests.log`)
failed three tests, and `mutation_anchors.py` one mutant. Each was the
change's to finish:

| What failed | Why | Fix |
|---|---|---|
| `diagnostics::tests::no_comparison_translates_an_old_number` | a comment in `rules.rs` named `DEPRECATED_ALIASES`, which only the files that print old numbers may | the comment names `diagnostics.rs` instead |
| `the_committed_enforcement_table_is_what_the_corpus_checks_now` | R-027 is enforced by PW5309 now | `just evidence-corpus` regenerated `docs/evidence/E2D/corpus-enforcement.txt` |
| `the_c8_text_of_every_repaired_rejected_fixture_is_still_caught` | R-027's C8 text, a query alone, is no longer caught for `stale_key_policy` | classified `FixtureDidNotExpressIt` with a control of its own; C30 opened, and its texts held to checking clean |
| `one_checker_mutations.py`: "a keyed query's stale-work rule is not explained" | its anchor was PW0325's explanation, removed | retargeted to PW0312's explanation, which R-014 reaches |

`stale-refix.sh` then ran (`logs/refix.log`):
- the formatter;
- `just evidence-corpus` (95/95 enforce their declared invariant);
- the four tests again (all pass);
- `just generality` (65 / 65 generality-tested);
- the anchors (4974 mutants in 324 scripts, each matching once).

## At the commit `0f3e1cc5` (`stale-verify.sh`, `logs/verify.log`)

| Check | Result |
|---|---|
| `just preflight` (format, clippy with `-D warnings`, case check, mutation anchors) | pass |
| `cargo test -p pw-core`, every test | 1814 passed, 0 failed, 2 ignored |
| `cargo test -p pw-cli --test guide` | 15 passed |
| `cargo test -p pw-syntax --test corpus_fmt` | 6 passed |
| `scripts/one_checker_mutations.py`, whole | 5 of 5 mutants killed |
| `mutation_anchors.py` after the controls | 4974 mutants in 324 scripts, each matching once |
| `just evidence-gates` | 1177 tests, 1 error: see below |

`reached.py` found no mutation script with an anchor within 3 lines of the
change (`logs/reached.txt` is empty). `one_checker_mutations.py` ran because
one of its mutants changed.

**The `evidence-gates` error** was
`test_browser_assertion_proof.BrowserAssertions.test_poll_callback_errors_are_not_assertion_results`:
its Playwright run passed its 30-second bound while three branches' checks
and another session's ran at once (`logs/evidence-gates.log.gz`). The change
touches no browser code. The sibling branch's `evidence-gates`, a few minutes
later on the same machine, passed every test.

## At the commit `6272dc1e` (`stale-final.sh`, `logs/final.log`)

The second commit changes the codes page's introduction to its old numbers
alone (`scripts/guide_diagnostics.py`, `docs/guide/diagnostics.md`).
`reached.py` against `0f3e1cc5` finds no mutation script that it reaches,
and every anchor matches once (`logs/final-anchors.log`).

| Check | Result |
|---|---|
| `scripts/guide_diagnostics.py --check` | current |
| `scripts/tests/test_guide_diagnostics.py` | 7 passed |
| `cargo test -p pw-cli --test guide` | 15 passed |
| `test_browser_assertion_proof`, alone | 11 run: 8 passed, 3 skipped |
| `just evidence-gates`, whole | 1177 passed, 5 skipped; Node's 19 passed |

The third commit adds this directory.
