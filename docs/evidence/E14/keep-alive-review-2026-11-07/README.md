# Keep-alive, its review's repairs (2026-11-07)

Evidence for the keep-alive ADR's section "Review, 2026-11-07, and its
repairs" (`docs/DECISIONS/ADR-XXXX-a-host-keeps-a-connection-for-its-next-request.md`),
on branch `perf/keep-alive-20261106`.

- Machine: 13th Gen Intel i9-13900K, 32 logical CPUs; Ubuntu on WSL2;
  rustc 1.97.1. Target `/home/haki/pw-targets/keep-alive`, built with
  `CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0`.

| Check | Result |
|---|---|
| `cargo test -p pw-dev-server keep_alive::` | 26 passed, 8 of them new |
| `cargo test -p pw-dev-server`, every test | 907 passed, 0 failed, 9 ignored |
| `cargo clippy -p pw-dev-server --all-targets -D warnings` | clean |
| `scripts/keep_alive_mutations.py`, whole (`keep-alive-mutations.log`) | 23 of 23 killed, each by a complete native assertion failure of the 27-test inventory; valid, restored |
| `scripts/cross_session_mutations.py`, whole (`cross-session-mutations.log`) | 8 of 8 killed |
| `scripts/mutation_anchors.py`, after | 5039 mutants in 326 scripts, each matching once |

The new tests: a head cut short by a header that is no text (400, closed,
the hidden request never answered); a head that trickles past its deadline
(408, closed); a head past its size (431, closed); a HEAD answered with no
body and closed; a body that trickles past its deadline (no answer,
closed); the next request waiting on no telling; the accept loop's backoff;
a second answer to one request closing its connection.

`cross_session_mutations.py`'s "the author waits for every reader" is
counted killed because it does not build: it calls a `tell_others` that no
longer exists, as master's own record of it shows. It is no control, and
train-15 replaces it with one that builds.
