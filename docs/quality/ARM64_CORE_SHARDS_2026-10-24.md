# ARM64 full-coverage test sharding

The earlier required ARM64 pw-core Cargo job ran sequentially for about
18.2 minutes. The isolated workflow at exact head d0914df25107f3e5126ea85564a7c1c0285d9c4a
https://github.com/studiokimeiga/perfect-web/actions/runs/40138974772
measured four independent integration partitions and a lib/doctest job,
all under the original ARM64 Rust 1.97.1 compiler, no-debug profile,
target-specific LLD 18.1.3, and strict warning settings.

Test results reconciled against passing ARM64 baseline CI
https://github.com/studiokimeiga/perfect-web/actions/runs/40137683192:

- 1,812 passed, 2 intentionally ignored, 0 failed on both paths
- 238 Cargo integration binaries discovered and scheduled exactly once
- Slowest of the five ARM64 test workers: approximately 5.5 minutes
- Prior single ARM64 pw-core job: approximately 18.2 minutes
- Independent end-to-end validation remains required; other jobs may
  dominate the final CI wall-clock. Additional runner-minutes are a cost.

Only the ARM64 pw-core partition is split. pw-dev-server and other
workspace tests remain in their original native jobs. All non-unit
compiler/Koka gates, audits, browsers and mutation checks stay enabled.
The original ARM64 status aggregator still requires the full native
matrix plus the non-unit matrix, refusing skipped/cancelled/failing jobs.
The explicit LLD pin and direct Cargo linker probe remain unchanged.

A new integration test file automatically enters a shard through Cargo
metadata and the source-file inventory, not through a manually curated list.
The ARM64 timing weights influence assignment only, never inclusion.
