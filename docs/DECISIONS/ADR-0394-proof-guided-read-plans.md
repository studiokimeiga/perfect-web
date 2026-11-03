# ADR-0394: proof-guided read plans begin as a checked, non-executing boundary

Status: **Proposed** (2026-11-03). The PR's test evidence, reviewer and owner acceptance determine whether this becomes part of the permanent semantic spine. It does not alter the project's charter.

## Problem

Pleris already derives resolved identities, effects, privacy, placement, resource dependencies and generated table operations. PW0405 refuses a read keyed by each element of a loop, and generated tables provide both keyed and set operations. But those facts do not yet form a general, inspectable way to compare equivalent execution plans, or to automatically lower an application's arbitrary loop to a set operation.

Optimizing by recognizing application types ("social feed", "shopping cart") would grow a pattern catalog. Instead, a plan must be defined by the meaning of a read, the necessary preconditions for each transformation, and the cost of legal physical implementations.

## Decision for the experimental slice

1. Add `pw-core::read_plans` with a minimal logical read algebra: input keys, per-key call, set call, and reassembly into the caller's original key sequence. This sits beside checked semantic analyses, not in the syntax or Wasm encoder. It does not become another authority for effects or types.
2. Discover keyed/set operation pairs **only** from compiler-generated `TablesJson` metadata: `get/get_many`, prefix/prefix-many, and index/index-many. Use typed `Primitive`, exact read effects, and identical lookup fields; never string-match application source.
3. Record evidence with a provenance category: compiler-derived fact, schema-enforced constraint, adapter-conformance contract, or unsupported assertion. An assertion is **not** a proof. Schema or adapter evidence must remain bounded to the generated schema/contract, not taken to prove unrelated host functions.
4. A rewrite must establish all of: one read snapshot, pure key projection, no per-key observable effects, unchanged privacy partition, equivalent error/retry behavior, and a supported set-read adapter. Unproved means reference plan, not an optimization hint silently applied. Batching may deduplicate and sort; explicitly reassemble keys so duplicates, missing keys and input ordering are preserved.
5. Compare valid plans using a separately supplied *abstract* cost model (call overhead, per-key work, batch setup, batch work). No wall-time estimates, benchmark claims or "fastest possible" assertions follow from the model. The reference wins ties. The model may later be calibrated with measured workloads.
6. The build publishes `read-plans.json` **only when tables exist**, names it as part of the content-addressed build, and removes it when a rebuild has no tables. `pw explain` exposes the same generated read families and provenance. It does **not** claim they were executed.
7. The reference executor and adapter differential tests exercise selected plans against the actual memory adapter, not an invented social-feed API. They do not authorize rewriting a user program until the entire compiler-to-host path is integrated.

The original `table` functions, generated interfaces, `PW0405`, schemas and existing application behavior remain unchanged. The normal runtime does **not** consume the new inventory.

## What this does not claim

- This is **not** a universal optimizer, an SMT/Lean theorem prover, a SQL engine, a replacement for PostgreSQL query planning, or a new source-level language policy.
- Conformance-tested adapter behavior is not a machine-checked proof of all C/Rust/SQL execution.
- A chosen plan minimizes an *explicit cost formula* over a small candidate set, not real-world latency or any global optimum.
- The paired `get_many` API already existed. The executor uses it in tests; ordinary Pleris `List.map` loops are not automatically transformed today. Lowering such loops requires a separate executable compiler path with cross-layer differential/mutation evidence.

## Why this is the right abstraction to test

The generic semantic unit is **keyed read of a set of inputs under a defined snapshot and effect boundary**, not a feed, store or ORM method. One normal form expresses the same problem for PostgreSQL, an in-memory adapter and future databases. This is a narrow vertical experiment; broaden the algebra only when a second independently tested optimization needs it.

## Rejected alternatives

- **Rewrite every PW0405 case now.** Rejected: callbacks may have effects, per-key errors, changing session authority, and snapshot timing. The host must preserve these, not merely the successful returned rows.
- **Treat a batched host import as a proof.** Rejected: it needs a verified contract and tests for missing/duplicate keys, ordering, transactions and fallback behavior.
- **Add a general e-graph or Lean dependency immediately.** Rejected: a rewrite vocabulary and proof boundary should be validated on real compiled apps before adding solver complexity.
- **Include plan advice in application syntax.** Rejected: physical decisions belong to the compiler and adapters; semantic invariants should not expose vendor-specific execution recipes.

## Gates for landing this slice

- Rust unit tests: positive and negative preconditions, provenance, cost counterexamples, zero/extreme cardinalities, stale or unsupported adapter, and reassembly.
- Differential tests with generated table metadata and actual `MemoryTables` reads, including missing and repeated keys.
- CLI and build tests: `pw explain` and `read-plans.json` derive from the same validated tables, no generated-path leakage, deterministic artifact naming and stale-artifact cleanup.
- All applicable CI, security, quality and exact-head evidence gates, as required by the charter.
- An independent review of the trusting boundary: a forged or unverified read contract must never unlock a rewrite.

## Deferred integration obligations

1. Prove the `ReadContext` fields from the existing effect/placement/binding/transaction analyses, not from caller-supplied booleans. Preserve a causal proof ledger through lowering.
2. Generate the executable set-read + reassembly path for a supported source-language read-in-loop form, and keep the existing reference path for every unsupported case. Test both in the same real snapshot with the same error and privacy semantics. Only then narrow or replace a PW0405 refusal.
3. Let the source adapter advertise a bounded batching capability; feed measured distributions, CPU, memory, network, latency and error data into an audited cost model. Evaluate not only typical workloads but adversarial ones and regressions.
4. Generalize the logical algebra only as new, independently verified optimizations justify it (predicate/projection pushdown, incremental materialization, placement). Do not build a SQL optimizer or optimize effects away.
