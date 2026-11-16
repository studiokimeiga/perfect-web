# Concrete upload HTTP composition — 2026-11-05

Frozen source `f22de943783db700aaadadca486eb3373500016f`; [raw output and strict receipts](../evidence/E14/protected-upload-http-2026-11-05).
Bound generic table manifests select a DataLayer current-generation reader in
the actual Server wiring. PostgreSQL delegates to the proved same-declaration
LIVE/count resolver under namespace exclusion. The upload handler freezes and
checks bytes/hash/length/measured kind/dimensions before validators, including
HEAD. No authority404, LIVE missing/corrupt/unreadable503 before304. External
SHA.kind paths, JPEG spelling, image/security/cache headers are preserved.

Six native tests combine compiler-produced declarations/bindings, actual image
sanitization, PostgreSQL, LocalBlobs and HTTP request/response bytes over TCP via
the public Uploads handler. PNG/JPEG positive200/304/HEAD neighbors, actual file
corruption/missing503, physically retained unreferenced404, hash-valid files
whose kind/dimensions differ from descriptor503, and two independently enrolled
declarations sharing one physical generation are exercised. Removing one
declaration's last ref makes only its route404; removing the last total ref
retires the generation. Body/headers and actual stored bytes are checked.

The concrete backend-death schedule pauses A after known COMMIT/fresh no-ref
inspection before LocalBlobs unlink. It observes the held advisory lock, kills
the actual backend, lets B commit same hash/new physicalG and serve HTTP200,
then releases A. Timeout earns no credit. Old file absence/new file bytes,
known-applied A acknowledgement, pending old job, bounded retry and later HTTP
200/304 are checked. A fault targeting the newer physicalG is killed natively.

Frozen selected server gates **235 passed** (uploads128, blob15, handler6,
generic memory14, generic PG37, protected23, conformance12); preflight **5,392
anchors**; strict **10/10** killed, full baseline7 (new6 + retained legacy1),
no invalid/stopped/compile-error credit. Separate immutable review is clear.
Before enabling the new path, a bypass reproduced two native404-vs200 failures.
The old legacy layer-error control's generic503 anchor was narrowed to its own
logging context after the new handler added503 branches; its native refusal
still kills the retained fault. Commands preserved in the frozen runner; the
strict recipe is `just e14-generic-upload-http` with actual throwaway PG URL.

**Limits:** handler TCP integration deliberately isolates unfinished full Server
startup/compiled resource/session claims. Proposals are trusted test inputs;
manual reader registration proves handler composition, not full startup wiring.
Generic startup guard remains closed; canonical feed/store remain legacy.
Actual declaration/session claim ledger, attempt nonce/raw-receipt reconciliation,
compiled upload/row commands on both adapters and full app/browser campaigns
remain next. No production conversion or deployment is accepted.
