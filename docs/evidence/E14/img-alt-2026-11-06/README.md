# An image says what it shows: PW5053 (2026-11-06)

Evidence for `docs/DECISIONS/ADR-XXXX-an-image-says-what-it-shows.md`. Branch
`fix/img-alt-20261106`, from `master` at `5d4d4d55`.

- Machine: 13th Gen Intel i9-13900K, 32 logical CPUs, 47 GiB; Ubuntu on
  WSL2; rustc 1.97.1; Node 22.21.1; Playwright 1.58.0. Its own target
  directory. Other sessions' builds and tests ran at the same time.
- `scripts/` holds what produced each log: `img-tests.sh` (before the
  commit), `img-verify.sh` (at the first commit), `img-final.sh` and
  `img-guide.sh` (at the second), and `reached.py`, which lists the mutation
  scripts with an anchor within 3 lines of a changed line.

## Before the commit (`img-tests.sh`, `logs/tests.log`)

- `diagnostics.md` regenerated and current.
- `just evidence-corpus`: 96/96 enforce their declared invariant (R-096
  added).
- The guide's test: 15 passed. The corpus's formatting: 6 passed.
- Two things found and fixed before the commit:
  - generality's published figure moved 65 -> 66, and the test's history
    line says so;
  - one control of `guide_mutations.py`, "a check gap is closed: the check
    refuses it", was anchored on the gap example this change removes. It
    now marks the title example, which the check refuses, as a check gap:
    the same verdict.

## At the first commit `9c5e1cf4` (`img-verify.sh`, `logs/verify.log`)

| Check | Result |
|---|---|
| `just preflight` (format, clippy `-D warnings`, case check, anchors) | pass |
| `cargo test -p pw-core`, every test | 1819 passed, 1 failed: see below |
| `cargo test -p pw-cli --test guide`; `corpus_fmt` | 15 passed; 6 passed |
| `evidence_integrity_2_mutations.py` (reached) | 37 of 37 killed |
| `images_mutations.py` | 10 of 10 killed |
| `labels_mutations.py` (reached) | 16 of 16 killed |
| `just evidence-gates` | 6 errors: no Node dependencies in this worktree yet |
| `guide_mutations.py` | refused its baseline: no Playwright yet |

The failure was `code_in_markup.rs`'s `<img onerror={msg} src="/a.png" />`,
a case about the handler, which PW5053 now also refused for its `alt`. The
second commit gives that image `alt=""`.

## At the second commit `7bd477fa` (`logs/final.log`, `logs/guide.log`)

The worktree's Node dependencies were installed from pnpm's store, offline.

| Check | Result |
|---|---|
| `cargo fmt --all --check` | pass |
| `cargo test -p pw-core`, every test | 1820 passed, 0 failed, 2 ignored |
| `just evidence-gates` | 1177 passed, 5 skipped; Node's 19 passed |
| `guide_mutations.py`, whole | 74 of 74 killed, 0 refused, 0 survived; baseline: the guide's 22 tests and the guestbook's 4 in Chromium |
| `mutation_anchors.py` | 4984 mutants in 325 scripts, each matching once |

The first run of the guide's controls at this commit (`logs/final.log`)
refused its baseline again: this target had no `debug/pw-dev-server`, which
the browser plan starts (exit 127). `img-guide.sh` built it and ran the
controls whole (`logs/guide.log`).

The third commit adds this directory.
