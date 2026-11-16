# Upload lease declaration scope — 2026-11-05

Frozen source `1f20302df34d720748dfb171cf60ecfb15b83242`; [raw gates, full inventories and process receipts](../evidence/E14/protected-upload-lease-scope-2026-11-05).
Actual upload staging now keys one lease by (session, declaration). Attach and
replacement leave the other declarations' leases alone. Each private preview
route selects its own declaration. Shared identical canonical bytes retain one
physical staged blob and one hold/capacity charge per lease. Explicit offline
shown_for/claim_for take an actual declaration and captured immutable reader;
legacy wrappers select only their first declaration while the port is pending.
Claimed values carry exact declaration/reader/lease identity, used on take/return,
and redact private cookie/token/image-hash diagnostics.

Three actual native reds reproduced cross-declaration replacement, private-route
borrowing and lost shared-byte holds. Ten final staging tests cover independent
replacement/consume/return, held claims blocking only their own declaration,
foreign session/declaration/reader lookup refusal, exact metadata routes and
redaction. Four expiry reds also reproduced lookup visibility/acquisition after
an unclaimed lease elapsed. Lookup now enforces TTL before periodic sweeping;
held claims survive elapsed time and sweep until an explicit outcome. Returned
expired leases are hidden immediately. The older expiry fixture previously used
ZERO lifetime then claimed an already-expired lease; it now claims before aging
actual monotonic timestamps, retaining its original assertions.

Frozen selected server **245 passed** (uploads138, blob15, concrete HTTP6,
generic memory14, generic PG37, protected23, conformance12); preflight **5,402
anchors**. New scope strict **11/11** killed, complete baseline11 (new10 plus
retained private-session route1). Retained session-authority campaign **11/11**
also passed FULL with native19 baseline and final restored baseline; domain
assertion sites, producer reaping and input hashes are checked. Separate immutable
review is clear. Exact source is clean before/after both campaigns. Frozen runner
preserves commands; focused recipe is `just e14-generic-upload-leases`.

Initial8cb fault run stopped on invalid Option.expect panics and earns no credit.
An explicit assertion helper now pins missing scoped values; a test-only helper
type correction followed an intermediate compile error, also no gate credit.
Four retained controls were re-anchored to tuple keys/borrowed reader syntax;
their original domain assertions remain and were caught in the complete run.

**Limits:** supplied immutable reader/declaration are trusted internal lookup
inputs; current authority still requires actual invocation/root/final fences.
These tests do not establish compiled generic resource callbacks, attempt-nonce
receipt settlement or unknown-COMMIT reconciliation. Existing legacy Claims
Drop is not the new generic outcome protocol. Generic startup guard remains
closed; canonical feed/store remain legacy. The actual claim/row integration
continues next, with no deployment or production conversion accepted.
