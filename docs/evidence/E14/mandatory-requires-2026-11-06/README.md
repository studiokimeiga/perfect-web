# A command says who may run it: PW0367 (2026-11-06)

Evidence for `docs/DECISIONS/ADR-XXXX-a-command-says-who-may-run-it.md`.
Branch `feat/mandatory-requires-20261106`, from `master` at `5d4d4d55`.

- Machine: 13th Gen Intel i9-13900K, 32 logical CPUs, 47 GiB; Ubuntu on
  WSL2; rustc 1.97.1; Node 22.21.1. Its own target directory.
- `scripts/` holds what produced the logs and the migration:
  - `migrate_requires.py`: the `.pw` files and the guide's examples
    (`logs/migrated.txt` lists each command it changed);
  - `migrate_requires_rs.py` and `migrate_requires_rs2.py`: the
    programs inside the Rust tests;
  - `guide_outputs.py`: the guide's recorded output, rebuilt from the
    built `pw`;
  - `requires-tests.sh`, `requires-tests2.sh`, `requires-verify.sh` and
    `requires-who.sh`: the runs;
  - `reached.py`: lists the mutation scripts with an anchor within 3
    lines of a changed line.

## Before the commit

- **First run** (`requires-tests.sh`; `logs/tests.log`,
  `logs/first-pw-core.log.gz`): pw-core 1775 passed, 39 failed; the guide
  2 failed; the corpus's formatting passed.
  - Two of the failures were expected: the enforcement table, and
    generality's 65 -> 66.
  - 37 were tests whose own programs had a command with no `requires`.
    They were migrated to `requires Anyone`, where no test is about
    authorization, and the two guide outputs whose lines moved were
    rebuilt.
- **Second pass** (`requires-tests2.sh`, `logs/tests2.log`): 232 of the
  affected and new tests passed, and one failed, a payments control. It now
  says `requires Anyone`, its point being that a command a reader presses
  is told nothing of `invoked_by`. Also: the guide 15 passed;
  `just evidence-corpus` 96/96 enforced.

## At the commit `815ec259`

| Check | Result |
|---|---|
| `just preflight` (format, clippy `-D warnings`, case check, anchors) | pass |
| `cargo test -p pw-core`, every test | 1821 passed, 0 failed, 2 ignored |
| `cargo test -p pw-cli --test guide`; `corpus_fmt` | 15 passed; 6 passed |
| `just evidence-gates` | 1177 passed, 5 skipped; Node's 19 passed |
| `command_retry_mutations.py` (reached) | 9 of 9 killed |
| `evidence_integrity_2_mutations.py` (reached) | 37 of 37 killed |
| `guide_mutations.py` | 74 of 74 killed |
| `who_may_run_mutations.py`, whole (`requires-who.sh`, `logs/who.log`) | 9 of 9 killed; the baseline's 43 tests passed |
| `mutation_anchors.py`, after | 4983 mutants in 325 scripts, each matching once |

The first five rows and the three reached scripts are in
`logs/verify.log`. That run was stopped mid-`who_may_run` when the PC's
disk filled (`logs/mut-who_may_run-stopped.log`):
- it left the mutant "reported at its declaration" applied in `check.rs`,
  which was restored before anything else built;
- the pushed commit predates it;
- the script was then run whole again.

GitHub's `verify` for #83 plans 16 shards of the recipes the change
reaches. Its evidence is fetched with `just evidence-fetch <run>`.
