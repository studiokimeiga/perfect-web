# Protected feed/store prerequisite: fixed relation windows

Design preceded implementation at `07c33f3f`. Implementation checkpoint
`275ca488` and strengthened frozen source `d8c634c9e513810dc5db300e0a44478718edd355` are independently reviewed.
The test-only authorization-reuse successor is `9451277ab47943424fed6c35d5ab20f142903847`. No runtime or adapter
bytes changed at that successor. **Canonical feed/store still use legacy Rust
data layers.** This source-scoped prerequisite does not complete their ports.

## Behavior

A declared relation returns public target rows selected by membership in an
owned/shared/public source table, using both explicit source identities and
declared lookup paths. Compiler nominal/refinement identity and target-row
annotation visibility are checked; the host independently checks stored types,
source/target metadata, public target/order, complete ABI and both table grants
in the contract and actual Granted set. An unused import or compiled predicate
cannot borrow the body target's authority. Reader handles remain caller-bound;
private source fields do not reach target rows. Owner-only private matching is
allowed; a sharer path revealing a private match is refused.

First/last is one GLOBAL target window per source selector, complete target
order+PK including ties. Duplicate edges use EXISTS semantics, not duplicate
target rows. Every eligible set form uses one common limit and returns canonical
key/rows groups including zeros; overlapping groups may each contain the same
target PK. The gate checks exact source group coverage/order/membership and
per-group target row ABI, order, uniqueness and bounds.

Memory retains bounded borrowed target references while looking up memberships.
PostgreSQL runs one scalar EXISTS query or typed-array/ordinality/LATERAL query
with LIMIT inside each selector. At 12/1,500 authors with 3 posts each, scalar N=2
returns 2 SQL rows and <=700 fixture payload bytes under 10,000 fuel; set returns 4
rows for 2 matching groups plus a missing group, <=1,400 bytes under 20,000 fuel.
Unbounded neighbors grow. This bounds returned result work, not internal DB scans.

Staged predicate/body reads see both staged membership and target writes; held
snapshots pin both while a later writer changes both. Real warm resource-cache
checks run on memory and PostgreSQL: Alice/Bob get different sentinels, repeated
reads add no adapter/table calls, and Bob cannot reuse Alice's warmed reader key.
The successor checks actual `authorize_with_tables_as` with permitted/refused
neighbors, a missing predicate target requirement, predicate reads and body callbacks at zero.

A real transition bug found during implementation is repaired: old-guest owner
classification must use the source membership PK, not the public target PK.
The new SourceKey witness went native red before repair. Review also found a
PW0405 literal plus and several false-green proof gaps; regression witnesses
now isolate each condition. Source-only review and raw native receipts are
recorded separately; they are not hosted landing evidence.

## Frozen local checks

At `d8c634c9e513810dc5db300e0a44478718edd355`: compiler 350, CLI 5, host 118 (9 existing ignored), memory 132,
PostgreSQL 95, selected server 180; test-compile and preflight 5,253anchors pass.
Strict complete-inventory campaigns: PG 11/11 (baseline95), row 14/14 (232),
data 7/7 (85), bounded 2/2 (40), host 2/2 (171). No invalid/surviving controls or
instrument-stopped processes receive credit. Sources restored clean.

The 23 new relation faults include wrong source/match, duplicate joins, global
set limit, independent grant axes, schema/type/visibility, SourceKey authority,
group coverage/order and memory selection/bounds. Older moved controls still
kill on their original ordinary paths. Correct-answer fetchall and per-author
then-hostclip faults each use ONE SQL and pass final-answer equality; the real
row counter catches 36 and 24 rows respectively instead of 2 at the small fixture.
The per-key SQL fault preserves answers and is caught by the operation count.

At the test-only successor: memory 8, preflight 5,253 and strict dual-grant 2/2
(baseline 8) pass with exact source/clean guards. Earlier source results remain
scoped to their named SHA. [Raw evidence](../evidence/E14/protected-relations-2026-11-04/)
includes command inputs, per-binary native/strict receipts and SHA256SUMS.

## Remaining migration

Generic upload claim/publication/reference/collection lifetime is next. Its
existing startup refusal remains. Public initialization, stable IDs/private
child deletion as required, canonical Pleris feed/store ports, deletion of their
legacy data adapters, unchanged assertions, final whole campaigns/browser/CI
and independent final review remain open. H3/H4, remote landing, production
conversion and paid benchmarks are outside this local prerequisite claim.
