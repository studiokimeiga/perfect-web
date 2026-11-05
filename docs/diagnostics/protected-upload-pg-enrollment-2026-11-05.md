# PostgreSQL upload enrollment — 2026-11-05

Frozen source `a79727d40b242f696f21b9529b25b56c89770728`, clean before/after all gates; see
[the accepted decisions](../DECISIONS/ADR-XXXX-protected-feed-store.md) and
[raw output/strict receipts](../evidence/E14/protected-upload-pg-enrollment-2026-11-05).

Migration0004 appends indexed namespace/declaration/binding/generation/reference/
captured-cleanup tables; 0001-0003 bytes are unchanged (exact comparison/hash in
evidence). The explicit trusted constructor validates both manifests, enrolls
fresh storage before namespace commit, and verifies existing markers on reopen.
Persisted base declarations/associations cannot change or disappear. Validation
precedes application DDL, including an unbound reopen with no asset clauses.
New asset associations on ANY existing table refuse, even a new optional Ref
field; safe new asset tables and optional non-asset fields remain additive.
The pw_asset_ relation prefix is reserved before connecting.

Review reproduced eight distinct native failure scenarios: BEFORE INSERT
namespace/declaration/binding rewrites3; AFTER namespace/app-schema-record
rewrites2; deferred schema-record rewrite1; bound complete/partial metadata loss2.
An additional unbound lost-metadata regression reproduced historical downgrade.
Repairs compare recorded associations for every constructor, flush pending
deferrable constraints/triggers, then re-read exact namespace/full immutable maps
after ALL DDL/schema recording immediately before COMMIT. Tests pin exact refusal
causes, rollback and valid neighbors. Initial late-trigger setup used an
unqualified table and gave unrelated refusal; it was corrected, then reproduced
real acceptance before repair. That earlier setup is not gate evidence.

Frozen gates: actual PostgreSQL18 **123 passed**; selected server **86 passed**;
preflight **5,358 anchors**; strict **13/13** faults killed, complete baseline22
(enrollment21 + retained unbound-asset guard1), no invalid/stopped producer credit.
Separate immutable review is clear. Controls for early immutable/suppressed
metadata checks prove refusal before DDL; final verification is a second defense.
The earlier redundant early-unbound omission control was superseded by reached
final unbound-verification and common recorded-history controls.

Commands preserved in the frozen runner: `cargo test --locked -p pw-table-postgres`,
selected server protected-migration/conformance/generic memory/PG paths,
`just preflight`, then the focused strict script. The actual PG URL was required
and the native retained0002 upgrade/record-preservation witness ran. Superseded
3477 gate output is retained and explicitly excluded from final acceptance.

**Limits:** MarkerStore models enrollment only; physical providers were tested
separately. This does not yet prove their combined actual row/lease/HTTP path.
Upload-bound command begin/turn still refuses the unfinished lifecycle, and the
server startup guard remains closed. Transactional registry/keep/GC, actual claim
ownership, final session fence, unknown outcome reconciliation and the actual
backend-death/HTTP witness remain next. No production/legacy conversion.
