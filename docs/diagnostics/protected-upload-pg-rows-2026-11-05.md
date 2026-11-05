# PostgreSQL upload rows and storage lifecycle — 2026-11-05

Frozen source `738da836d938e785ee6aff8b530f07f6ef7716ba`; [raw gates/strict receipts](../evidence/E14/protected-upload-pg-rows-2026-11-05).
The [accepted lifecycle](../DECISIONS/ADR-XXXX-protected-feed-store.md) now runs
on the reserved PostgreSQL command connection: session fence, namespace SESSION
exclusion, host turn, then actual command BEGIN/snapshot. Migration shares that
namespace exclusion through DDL/COMMIT. Touched first/final asset fields drive
the shared planner and indexed descriptor/count/tombstone/captured-job writes.
Actual touched-key reads project only key and asset columns and verify exact
rows/registry before keep and again after receipt/outbox/deferred hooks.

Claim-marked commands require a real durable interaction, with exact requested
key/sent/answer/stored-instant readback after immediate/outbox work before keep
and after deferred constraints before the final clock fence/COMMIT. Only fresh
final-used generations are kept. Known rollback performs no collection; actual
lost COMMIT replies remain OutcomeUnknown and perform no collection. Known
commits collect captured old generations under fresh no-reference checks;
collection/completion failures preserve the applied acknowledgement and pending
job. Completion checks both returned row and actual captured-job absence.

Native34 cover actual SQL pre/late/deferred rewrites and rollback, receipt hook
rewrites/removal, required receipts even held/discarded without publication,
net moves/repeated keys/rejected and ignored publication, unrelated live assets,
bounded key/asset-only work, per-declaration serving, NUL/composite prefix keys,
namespace/migration waits and fresh snapshots, actual TCP COMMIT reply loss,
final session expiry during keep on one connection, authoritative missing-vs-
unreferenced reads, bounded/idempotent captured cleanup, and actual backend death
after committed no-ref check before old physical-provider callback resumes.
That race explicitly observes retained namespace lock and B's successful
same-hash fresh-generation commit before releasing A; timeout earns no credit.

Review reproduced seven receipt-integrity failures plus completion reinsertion;
all now have repaired regressions and valid neighbors. Frozen full actualPG18:
**157 passed**; preflight **5,383 anchors**; strict **28/28** killed with complete
baseline35 (lifecycle34 + retained unbound asset guard1), valid native assertions
only. Separate immutable review is clear. Source is clean after all mutations.
Commands are preserved in the runner: full pw-table-postgres tests, just
preflight, focused strict runner. Actual throwaway PG URL is required.

Superseded campaigns are retained and excluded: initial3a98 run was stopped at
an unbounded polling control; d29 rowcount-only omission survived because the
new absence verifier independently refused it. Revised native wait has a five-
second wall-clock bound; final namespace fault releases its temporary lock and
returns no lifetime, without an unrelated unlock failure. Final complete-
completion omission and independent absence omission each produce native faults.

**Scope:** trusted canonical proposals and a ByteStore test double; not actual
session/lease/claim authority, sanitizer or concrete LocalBlobs/HTTP composition.
The lower read_asset/collect_assets APIs are proved, but public HTTP and actual
compiled resources still need integration. Default unbound constructor and
server startup guards remain closed. Canonical feed/store remain legacy. No
production conversion or orphan collector is implemented.
