# Three texts that read wrong, 2026-11-06 (message-text)

Branch `claude/kind-goldberg-c4d715`, base `5d4d4d55` (origin/master then):
`2fd56c14` fixes the three texts, and `c30ef1f3` fixes what an independent
review of it found. Recorded at `c30ef1f3384b921a1ae120dfaef2ba193ee10977c`
unless a file says otherwise, in WSL Ubuntu 22.04 on the owner's PC, by the
scripts in `tools/`. Hosted CI has not run this branch.

A review of pleris.dev on 2026-11-06 found three texts that read wrong to
someone using Pleris:

1. **PW5001's related note stopped mid-sentence** (`compiler/pw-core/src/check.rs`):
   "`session` is Session<SessionId>, so what `Cart` makes from it is". It now
   ends "... is too". The review of `2fd56c14` found the note, and the cause
   chain in the explanation, false where a declaration is given more than one
   labelled parameter: both put the declaration's whole label beside the
   parameter's name ("`session` is Session<SessionId> ⊔
   Organization<OrganizationId>"). Each now states the parameter's own label;
   the message still states all the declaration holds.
2. **PW5048's note opened with a dated story of the project's**
   (`compiler/pw-core/src/holds.rs`). It now gives the reason as a rule: "A
   control the server refuses is a dead control: shown to a reader for whom
   `SignedIn` does not hold, it offers a press that can only be refused."
   `docs/guide/errors.md`'s recorded output is regenerated from the built `pw`.
3. **The benchmark store's comment said a double-tap is one add**
   (`benchmarks/baselines/pleris/store/app.pw`), where its contract holds two
   presses to two adds and ADR-0121 a request sent twice to one. The six task
   patches that quote it changed with it; T01's setup, which wrote its own copy
   of the claim into T01's starting store, says the true one-line version.

## What each file shows

| File | What it holds |
|---|---|
| `red-controls.txt` | Each fix undone in turn, and the tests that hold it run; each control's named tests fail, and no other. PW5001's `check.rs` as at the base fails both PW5001 tests; the note given all the declaration's parameters state again fails `the_parameter_is_noted_with_its_own_label_where_more_are_given`; PW5048's `holds.rs` as at the base fails `a_refused_controls_note_says_the_rule_and_why`; the old PW5048 block in `docs/guide/errors.md` fails the guide's `every_example_in_the_guide_compiles_as_it_says`. The sources are restored after. |
| `pw-core-tests.txt.gz` | `cargo test --locked --no-fail-fast -p pw-core`, every test binary: 241, 1816 tests passed, none failed. |
| `guide-test.txt` | `cargo test --locked -p pw-cli --test guide`: 15 passed. |
| `corpus-fmt-test.txt` | `cargo test --locked -p pw-syntax --test corpus_fmt`: 6 passed. |
| `guide-blocks.txt` | Every `text pw-check` block of `docs/guide`, replayed as the guide test replays it against the built `pw`: none differs. `tools/regen_guide_blocks.py.gz --write` rewrote the PW5048 block. |
| `benchmark-patches.txt` | The 33 Pleris task patches, applied as the harness applies them (the setup first, then the reference or unsafe patch): all apply on this commit's store, and no store says "double-tap". The controls: the base's patches on this store, and this commit's on the base's store, are each refused exactly where the comment is quoted (6 patches, and the 8 that start from those setups). `pw diff` of the store alone, base to this commit: every section `- none`. Its "11 report a change" are the 11 stores `pw diff` will not compare because the store before them does not check by design (an unsafe patch, T08's and T12's setups): the next file compares all 33. |
| `benchmark-stores-text.txt` | Each of the 33 patched stores, the base's against this commit's: 30 differ only in `//` comment lines, and T08's three are identical (its setup replaces the comment). The programs are the same. |
| `ci-plan.txt` | `python3 scripts/ci_plan.py --changed 5d4d4d55 HEAD`: the sixteen recipes the branch reaches. |
| `mutation-anchors.txt`, `preflight.txt`, `evidence-gates.txt.gz` | `python3 scripts/mutation_anchors.py` (4974 mutants in 324 scripts, each matching once), `just preflight`, and `just evidence-gates` (1177 tests, OK, 5 skipped). |
| `recipes/` | Twelve of the sixteen reached recipes, each run whole at `c30ef1f3` in a detached worktree of its own, one after another (`PORT=46400`, `BENCH_PORT=46700`): the evidence file each writes, the runner's `summary.txt` (exit, whether the sources were restored, and the anchors after each), and each recipe's log. Twelve mutation scripts, 101 of 101 mutants killed. |
| `recipes/at-2fd56c14/` | The other four, as run whole at `2fd56c14` (`PORT=45000`, `BENCH_PORT=45300`). `e14-holds`, `e14-metadata` and `e14-contract` are not reached by `c30ef1f3`'s change (`ci_plan.py --changed 2fd56c14 c30ef1f3`), so these hold at `c30ef1f3`: 61 of 61 mutants killed. `e14-pleris-controls` is reached by T01's setup patch; every task's four controls held at `2fd56c14`, and it is left to the branch's hosted verification. |
| `superseded/` | The first `just evidence-gates` run, which errored in 19 tests: see below. |
| `tools/` | `collect.py`, `regen_guide_blocks.py` and `run_recipes.sh`, gzipped. |
| `manifest.json` | Each step's command and exit, and every file's sha256. |

## The recipes against their committed evidence

Each recipe's output differs from its file under `docs/evidence/` (recorded on
CI at `f15117de`) only by the order parallel tests print in, the new tests and
the kills they add (`holds`: 9 tests, was 8; `shared-cache`: 13, was 11), suites
that grew since `f15117de`, a result file's name in `museum.txt`, and
slow-test notices. In `shared-cache.txt` the generality section shows "corpus
conformance: 44 / 44" where the committed file shows "generality-tested: 65 /
65": with `--nocapture`, cargo's quiet-mode progress dots can share a line with
either summary, and the recipe's `^  ` grep then drops it. Run alone
(`--test-threads=1`), the test prints all three: 44 / 44, 65 / 65, 0 untested.
The committed recipe files are not replaced: evidence of record comes from a
verification run (ADR-0281).

## What was recorded more than once, and why

- The first collection exported `GIT_DIR` and `GIT_WORK_TREE` (the desktop app
  had written this worktree's `.git` link as a Windows path WSL's git could
  not follow). `just evidence-gates`' tests make throwaway git repositories,
  and under those variables their `git init`, `git config` and `git commit`
  went to the real repository: `core.worktree` and `[user] t <t@example.com>`
  in the shared `.git/config`, and a `fixture` commit on this branch. Its 19
  errors are in `superseded/`. The settings were removed, the branch reset,
  and the worktree's `.git` given a relative link; the gate passed with no
  `GIT_` variable set, at `2fd56c14` and again here.
- The review of `2fd56c14` led to `c30ef1f3`, and everything was recorded again
  at it.
- The run of the sixteen recipes at `c30ef1f3` was stopped during `e14-holds`,
  the thirteenth, when the PC's C: drive (which holds WSL's disk) fell to 2 GB
  free. It was stopped before that recipe's mutation script began; its partial
  output is not kept.
