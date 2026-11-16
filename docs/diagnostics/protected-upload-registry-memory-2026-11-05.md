# Upload registry and memory row bookkeeping — 2026-11-05

Source `ae09ede16c71743ef6d22640639fd0e52681e3fc`, frozen with clean guards.
Accepted generation transition/instance-affinity/memory integration decisions
are in [the migration ADR](../DECISIONS/ADR-XXXX-protected-feed-store.md).

The shared pure planner validates exact immutable descriptors and same-declaration
LIVE authority or a trusted fresh proposal, computes checked final net counts,
and captures last-reference tombstones/cleanup. The indexed memory registry
retains retired identities and current hash pointers. Memory row transactions
derive uses only from each touched key's original/final declared asset fields,
prepare with row constraints, and publish or undo rows/registry together.

Review found foreign draft/patch acceptance between different registries at an
equal revision. Three independent native regressions reproduced propose, empty
prepare and actual same-generation/different-hash patch corruption before the
private instance witness repair. Native tests also pin per-declaration liveness,
duplicate fields, rejected writes, final moves, repeated same-key writes,
composite prefix deletion, captured old cleanup after fresh publication, health,
exact registry operation costs and cleanup limits 0/1/2 with two genuine jobs.

Final checks: host **145 passed, 9 existing ignored**; memory **166 passed**;
preflight **5,335 exact anchors**. Strict controls: **20/20** registry faults
(complete native inventory19) and **12/12** row/retained-guard faults (baseline28:
row10 + compiled binding18), no invalid/stopped producer credit. An earlier
campaign stopped at an incidental unwrap/index panic; assertions were repaired
and complete campaigns rerun. Separate immutable review is clear.

Commands are preserved in the frozen runner and strict helper/producer receipts
under [the raw evidence directory](../evidence/E14/protected-upload-registry-memory-2026-11-05).
`cargo test --locked -p pw-host --features engine`,
`cargo test --locked -p pw-table-memory`, `just preflight`, then both focused
strict scripts ran sequentially on the exact clean source.

**Limit:** trusted descriptor proposals stand in for authenticated claims.
The explicit memory constructor is not enabled by server startup. Ordinary
unbound construction still refuses assets. No storage, lease ownership,
durable receipt/unknown-outcome, PostgreSQL, HTTP or canonical application port
acceptance follows. Generation IDs retained after applied plans do not prove
physical reservation after rollback/orphaning. Those integration gates remain.
