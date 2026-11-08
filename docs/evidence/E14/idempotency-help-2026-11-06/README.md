# The InteractionId help names both steps (2026-11-06)

Evidence for branch `fix/idempotency-help-20261106`, from `master` at
`5d4d4d55`: PW0338's, PW0312's and PW6405's helps say to declare
`opaque type InteractionId = String` where the program has none, beside
`idempotent_by InteractionId`.

- Machine: 13th Gen Intel i9-13900K, 32 logical CPUs, 47 GiB; Ubuntu on
  WSL2; rustc 1.97.1; Node 22.21.1. Its own target directory. Other
  sessions' builds and tests ran at the same time.
- `scripts/` holds what produced the logs: `help-verify.sh`, the full check;
  `help_demo.py`, which follows each help as a reader would, with the
  programs in `scripts/help-demo/`; and `reached.py`, which lists the
  mutation scripts with an anchor within 3 lines of a changed line.
- The programs are filed as `.pw.txt`, so that no census of the
  repository's sources (`check_build_programs.py`, `mutant_programs.py`)
  takes them for programs. `help_demo.py` read them as `.pw`.
- `help_demo.py` is as it ran, but for one name: its loop variable `l` is
  `line`, for Ruff's E741, which CI holds new Python to. Run again after
  the rename, it printed the same 9 of 9.

## The help, followed as a reader would (`logs/demo.log`)

For each program, `pw check --plain` with the standard packages, three
times: as written, with only the clause the help names first, and with the
type as well.

| Program | As written | The clause alone | The clause and the type |
|---|---|---|---|
| `twice.pw`, a page's handler sends `post` | PW0338, the new help | PW0335: "`InteractionId` names no type visible here" | clean |
| `retry.pw`, `post` declares a retry | PW0312, the new help | PW0335 | clean |
| `clock.pw`, `remind` is `invoked_by Clock` | PW6405, the new help | PW0335 | clean |

9 of 9 steps as the help says: the second step is needed, and the two are
enough.

`logs/verify.log` also holds the first run of the demo, 7 of 9: that
version of `help_demo.py` passed no standard package, so `twice.pw` and
`retry.pw` could not resolve their imports with either step. The script was
fixed; the compiler was not changed.

## At the commit `bd02a965` (`help-verify.sh`, `logs/verify.log`)

| Check | Result |
|---|---|
| `cargo fmt --all --check` | pass |
| `cargo test -p pw-core`, every test | 1813 passed, 0 failed, 2 ignored |
| `cargo test -p pw-cli --test guide` | 15 passed |
| `just preflight` (format, clippy with `-D warnings`, case check, mutation anchors) | pass |
| `just evidence-gates` | 1177 passed, 5 skipped; Node's 19 passed |
| `scripts/invoked_by_mutations.py`, whole (`reached.py`'s one) | 14 of 14 mutants killed |
| `mutation_anchors.py` after the controls | 4974 mutants in 324 scripts, each matching once |

The third commit adds this directory.
