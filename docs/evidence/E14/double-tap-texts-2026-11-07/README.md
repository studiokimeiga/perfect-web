# What two presses are, 2026-11-07 (double-tap-texts)

Commit `de690e45` on `claude/double-tap-texts`, base `5d4d4d55`. Three texts
said a double press is one add, or that the Pleris stack does not do what the
store's contract leaves out. The independent review of PR #88 found them.

| File | What it holds |
|---|---|
| `store-builds.txt` | The canonical store built at the base and at this commit: every file it builds to (105) is byte-identical, and its `templates.json` is the committed `spikes/own-renderer/store-ir.json`. The changed comment line keeps its length, so no span after it moves. `pw fmt --check` and `pw check` are clean. |
| `guide-test.txt` | `cargo test --locked -p pw-cli --test guide`: 15 passed (`docs/guide/tutorial.md` is one of its pages). |
| `checks.txt` | `scripts/mutation_anchors.py`, and the parts of `just preflight` a change of comments and prose reaches: rustfmt, the case check, compileall. Its clippy is left to CI: no Rust source changed. |
| `ci-plan.txt` | The 41 recipes the change reaches, almost all because they read `examples/store`. They are left to this branch's hosted verification. |

Recorded in WSL Ubuntu 22.04 by `collect_dt.sh` (kept in the session's
scratchpad), with `CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
CARGO_INCREMENTAL=0`.
