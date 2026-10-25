# x64 pw-core full-coverage integration sharding

The earlier x64 baseline ran `cargo test --locked -p pw-core --no-fail-fast`
sequentially in 24.3 minutes. In experiment run 40135659315, four
integration-binary partitions plus a separate library/doctest job passed
**exactly the same 1,812 tests**, with two intentionally ignored,
238 distinct integration binaries, and no failures. The critical path was
about 7.7 minutes (~3.2x faster); additional runner-minutes are a tradeoff.

New targets are discovered from Cargo metadata and checked against direct
tests/*.rs filenames; each target joins exactly one of four shards, with
the expected runtimes based on historical baseline job 120972413421.
Unknown/malformed metadata or weights fail closed. Each binary runs all
its normal tests, unchanged, with the same compiler, RUSTFLAGS and debug
profile. Unit and doctests are executed separately without filters.

The required x64 status is still `ubuntu-24.04`, which depends on both
the original non-unit matrix (Koka, lint and mutation controls) and the
complete native test matrix. Every skipped, cancelled or failed shard
makes that aggregate fail. The x64 cache saver is reassigned to exactly
one group, pw-core-int-0, preserving shared-key x64-native-tests.

Do not interpret passing CI as proof every runtime/perf issue is resolved.
Evaluate subsequent end-to-end wall time and runner-minute costs after
merging, and preserve the original parity baseline and rollback path.

Evidence: https://github.com/studiokimeiga/perfect-web/actions/runs/40135659315
Original: https://github.com/studiokimeiga/perfect-web/actions/runs/40127610536

## Reproduced measured job breakdown

The complete isolated trial on the exact commit in
[GitHub Actions run 40135659315](https://github.com/studiokimeiga/perfect-web/actions/runs/40135659315)
passed the Cargo target inventory, every integration group, and library and
doctests. Reconciled libtest output: **1,812 passed, 2 intentionally ignored,
0 failed**, with 238 distinct integration binaries. The job wall times were:

| Parallel job | Wall time |
|---|---:|
| Integration shard 0 | 7.27 min |
| Integration shard 1 | 7.07 min |
| Integration shard 2 | 7.07 min |
| Integration shard 3 | 4.97 min |
| Library and doctests | 1.30 min |
| Inventory planner | 0.30 min |

The gate's wall-time critical path was approximately 7.3 minutes versus
24.3 minutes for the prior single x64 `pw-core` job on master
([baseline](https://github.com/studiokimeiga/perfect-web/actions/runs/40127610536)).
However, the five worker jobs plus planning consumed approximately
28 runner-minutes in total, compared with 24.3 for the unsharded job.
This exchanges additional runner capacity for shorter wall-clock latency;
it does **not** imply a 3.3x reduction in total CI latency or compute use.
The complete CI benefit depends on the other required x64 and ARM64 jobs,
particularly the non-unit compiler/mutation gate.

## ARM64 full-coverage sharding

The independent ARM64 experiment run 40138974772 at d0914df2 used
four measured integration shards and a complete lib/doctest job with
Rust 1.97.1 and target-specific LLD. All 1,812 tests passed, with the
same two intentional ignores as the unsharded run 40137683192.
The slowest ARM shard completed in approximately 5.5 minutes, compared
with an 18-minute ARM pw-core job. The ARM weight source is
arm_weights.json (234 historically measured targets; newly introduced
binaries are discovered and assigned even without weights).
All original non-unit/Koka work and other native package partitions
remain required. Extra concurrent runners can increase aggregate
runner-minutes; compare complete CI wall-clock and expense after merge.

Evidence: https://github.com/studiokimeiga/perfect-web/actions/runs/40138974772
