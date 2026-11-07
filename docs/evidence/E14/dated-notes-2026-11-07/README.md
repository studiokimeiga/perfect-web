# What the compiler prints tells no story of the project's, 2026-11-07 (dated-notes)

Commit `103db6dc` on `claude/dated-diagnostic-notes`, base `5d4d4d55`.
Fifteen diagnostic notes told the compiler's history, dated, where the reader
of `pw check` needs the rule and its reason: PW5048's (the same rewrite as
PR #88), and PW0021 (a name in a policy term), PW0027, PW0028, PW0335,
PW0618, PW0629, PW5019, PW5020, PW5021, PW5022, PW5023, PW5024, PW5028 and
PW5103. Each now states the rule and why; each dated sentence is a comment
above its note. `compiler/pw-core/tests/printed_texts_carry_no_dates.rs`
holds the rule.

| File | What it holds |
|---|---|
| `guard.txt` | The guard at this commit, and its red control: the four changed sources as the base holds them, against which it reports all fifteen dated literals and fails. The sources are restored after. |
| `pw-core.txt.gz` | `cargo test --locked --no-fail-fast -p pw-core`: 242 test binaries, 1815 passed, none failed. |
| `guide.txt` | `cargo test --locked -p pw-cli --test guide`: 15 passed. |
| `clippy.txt` | `cargo clippy --locked -p pw-core --all-targets -- -D warnings`: clean. |
| `guide-blocks.txt` | Every `text pw-check` block of `docs/guide` replayed against the built `pw`: none differs. Only PW5048's block was regenerated, byte-identical to #88's `docs/guide/errors.md`. |
| `checks.txt` | `scripts/mutation_anchors.py` (4974 mutants, each matching once) and rustfmt. |
| `ci-plan.txt` | The 12 recipes the change reaches, left to this branch's hosted verification. |
| `collect_dn.sh.gz` | The script that wrote these files. |

`pw-core.txt.gz`, `guide.txt` and `clippy.txt` were run on the tree committed
as `103db6dc`, begun before the commit was made; no file they read changed
since. Everything here was recorded in WSL Ubuntu 22.04 with
`CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0`.
