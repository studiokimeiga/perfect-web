# Architecture

Source map reviewed 2026-09-16 from `49398dc02be6f2c3ea1a28fdfb77bf66c15266b6`
with the resolved-signature cutover.
This describes ownership and open boundaries, not a claim that every charter
feature is implemented. [STATUS](STATUS.md) owns completion claims and
[NEXT](NEXT.md) owns the current implementation order.

The previous architecture file began "No compiler exists yet". It is retained
byte-for-byte as [the E0-era architecture snapshot](ARCHITECTURE-history-2026-09-15.md).
The charter and accepted ADRs remain authoritative for design decisions.

## Compiler ownership

`compiler/pw-syntax` owns syntax. `compiler/pw-core` owns HIR, resolution,
semantic analyses, diagnostics, manifests, contracts and backend work.
`compiler/pw-cli` exposes commands and renders results; it is not a second
semantic checker. Existing analysis modules include effects, privacy, placement,
resources, scopes, templates, resumption, binding, WIT and Wasm emission.

The Pleris declaration is the ABI authority for operations declared by the
program. An operation's identity is distinct from the capabilities authorizing
it (ADR-0026). Generated projections must not become independently maintained
semantic authorities.

`Signature` now owns recursive semantic parameter/return slots. Missing and
unresolved annotations are explicit alternatives. `Interface` is a projection
of that signature, not a second derivation from syntax. Member and boundary
facts are keyed by resolved receiver identity; WIT and contract identities are
derived projections. The remaining source-fragment readers use the language's
existing type grammar, not independent string splitters (ADR-0030).

Comprehensive ordinary-call argument/return checking remains unfinished. The
backend's remaining internal value-layout approximations and E10-I integration
are separate obligations; a resolved callable signature does not prove a full
backend implementation.

## Runtime ownership

| Area | Source location | Responsibility represented in the repository |
|---|---|---|
| Resources | `runtime/pw-resource` | Resource entry identity and resource behavior |
| Tasks | `runtime/pw-tasks` | Scoped task runtime work |
| Materialization | `runtime/pw-materialize` | Stored materializations and outbox work |
| Document identity | `runtime/pw-document` | Document-part address vocabulary |
| Transport contract | `runtime/pw-protocol` | Patch and recovery protocol data |
| Rendering | `runtime/pw-render` | Renderer implementation |
| Resumption | `runtime/pw-resume`, `runtime/pw-resume-wasm` | The resumption decision's specification; pages decide in the runtime, held to its Wasm build by `decision-agreement.test.mjs` (ADR-0414) |
| Host | `runtime/pw-host` | Component contracts, admission and engine integration |

These locations identify ownership. Presence of a crate does not establish its
entire charter contract. One semantic resource-entry identity has distinct storage
and wire projections; one operation can require multiple capabilities, and two
operations can share an authority without sharing an ABI.

## Compiled execution and remaining data-layer boundary

E10 closed on 2026-10-02 by [ADR-0119](DECISIONS/ADR-0119-e10-closes-and-the-ai-benchmark-comes-next.md).
Its recorded gate at `bff437c` builds the store without Koka or Marko,
compares compiled execution with the oracle, and measures compiled component
calls. The old paragraph here still called E10-I unperformed; that was stale.
Those historical measurements are not new measurements of the current tree.

App-specific host composites remain a separate deferred obligation
(`E10-S10`, ADR-0119). The store and feed still use their Rust data layers.
ADR-0338 accepts declarations of tables with generic per-database adapters;
D1 declarations, the D2 gated memory reads/writes and the D3 PostgreSQL
adapter are in fetched master `5d4d4d55` (2026-11-02); ADR-0350, ADR-0385,
ADR-0386 and ADR-0393 govern them. Generic development-server routing and
the guestbook without app-specific Rust are on unmerged PR #72 at `687dd922`,
including a reusable conformance suite. Durable server integration and row
authority remain separate obligations. The render-merge implementation is in
main history; its remaining snapshot limits remain explicit in its ADR and
tests. This corrects the older October track inventory without promoting
unmerged work to a landing.

## Current integration boundary (2026-10-15)

E14 is active before E11-E13. Train-9 and train-10 state and test evidence
are maintained in STATUS/PARALLEL; a local merge is not a verified landing.
Train-10 integrates semantic each-header expressions (ADR-0345), invariant
checks at component/page boundaries (ADR-0346), and scheduled-order set
estimates (ADR-0347/0348). Generic-result effects, evidence failure propagation
and table declarations each have isolated validation tracks. None of this
establishes production readiness or the planned collaborative-docs runtime.

## Evidence and proposed extensions

The [failure census](../research/failures/README.md) connects proposed rules to
invariants, assumptions, enforcement boundaries and adversarial obligations.
ADR-0027 separates specification, representation, implementation and observed
behavior. The [recipe-gate repair](evidence/tooling/evidence-gates-2026-09-15.md)
protects the exit-status path by which some of that evidence is produced.

Temporal authority, compatibility across live versions, commitment outcomes,
browser-owned state and composed budgets remain explicit design/implementation
obligations where their census records say so. No new language semantics or
production-readiness claim is introduced by this documentation repair.
