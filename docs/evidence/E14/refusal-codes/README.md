# The refusal codes, checked whole (2026-11-06)

Evidence for `docs/DECISIONS/ADR-XXXX-a-refusal-has-a-code-and-a-page.md`,
on branch `feat/refusal-codes-20261106`, stacked on #76's head `14a3a7f6`.

## The recipe's evidence

`../refusal-codes.txt` is `just e14-refusal-codes`, run at `04155835`:
- the decision's tests with its trace (`pw-resume`'s default features) and
  without it, as the browser's build has it;
- the two browser tests in Chromium, in their specs: 17 passed;
- the mutation controls (`scripts/refusal_codes_mutations.py`): 4 of 4
  killed.

## The whole check at the exact commit

`final-verify-rc.log`, from `final-verify-rc.sh`, which ran from the
worktree's root with `SCRATCH` set to the session's scratchpad. The scripts
it ran are here, compressed.

| Check | Result |
|---|---|
| CI's Fallow audit, as CI runs it, against #76's head (`fallow-rc.json.gz`) | passed: none introduced |
| The mutation scripts the change reaches (`reached.py 14a3a7f6 3`) | `refusal_codes` alone |
| `just preflight` (`rc1-preflight.log.gz`) | passed |
| `just evidence-gates` (`rc1-evidence-gates.log.gz`) | 1,187 tests, 5 skipped |
| `pw-render`, `pw-render-wasm` and `pw-dev-server`'s tests (`rc1-cargo-test.log.gz`) | passed: 863 in the server |
| Every own-renderer app built | passed |
| The whole suite, Chromium and Firefox (`rc1-specs.log.gz`) | 1,076 tests: 1,062 passed, 14 skipped by their own conditions, none failed |
| Gates 7b and 7d (`rc1-size.log`) | 44,395 + 19,552 = 63,947 B of 65,536; the renderer 77,336 B |
| The recipe (`rc-recipe.log.gz`) | passed; 4 of 4 killed |
| The sources after | restored: 4,997 anchors, each matching once |

The suite has one more test than #76's head, the decision's words test in
`runtime-size.spec.mjs`. It runs in Chromium only, so Firefox skips it,
which makes 14 skipped where #76 had 13.

## The first run, which failed (`run1-*`)

The same check first ran at `6edb530a`. `just evidence-gates` failed one test
of 1,187: `test_mutation_baseline.EveryCallOfExplain`. That test holds every
mutation script's call of `mutation_baseline.explain()` to follow the words
"the unmutated baseline is not green". The new script said "the unmutated
source does not build" instead. The run was stopped there, before any
browser suite or mutant ran. `04155835` makes the script count a source that
does not build as a red baseline, and say so as every script does. Nothing
else changed.

`SHA256SUMS` covers every file here but this README.
