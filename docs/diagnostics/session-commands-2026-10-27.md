# Session command and receipt continuation, 2026-10-27

This candidate extends ADR-0389. No incomplete or skipped gate is a merge pass.
`just e14-session-command-authority` is the new bounded acceptance producer:
real compiled commands over memory and both PostgreSQL adapters, plus resource
reservation controls. Both database URLs are mandatory. Native identities are
crosschecked/frozen per binary; every selected identity must finish; only native
assertions in the selected test boundaries may kill a control. Multiple guards
removed together are explicitly composite controls. Source manifests, durable
raw logs and independent schema-absence queries accompany every run. Unexpected
fixture/neighbor assertion sites require review; a generic assert macro parser
cannot infer semantic intent. The advisory-unlock helper control proves its
boolean contract, separately from the adapter's pool-discard implementation.

Implementation boundaries: immutable explicit request admission; memory gate
through commit/publication; own-source PostgreSQL advisory+row fencing after
waits, late live-clock expiry, and discard on uncertain release; reader-bound
persisted receipt metadata while preserving the original durable key; scoped
reservations and document ownership; no private result/basis after revocation;
unknown post-COMMIT delivery/acknowledgement; same-reader receipt recovery after
page loss; generation-specific operation merge, acknowledgements and basis;
explicit browser helper fencing; and authoritative warm-view regeneration after
inconclusive acknowledgement, with no attempted business event publication.

Review-driven repairs and regression prevention:

- A bounded interactions queue could hide a retained unknown reservation on
  stale-page recall. The actual PG witness evicts bookkeeping, retains unknown,
  forgets the page, retries through HTTP, and requires unknown with no rollback
  assertion or duplicate business write.
- Raw-cookie applied/basis metadata crossed same-cookie generations. Actual
  old/new EntryValue neighbors now prove separated acknowledgements/basis.
- An initial GET discarded admission results. A deterministic later cache
  installation now cannot replace the admitted render actor.
- An ordinary lost Store acknowledgement left an existing local materializer
  current, so its drain returned early. The actual original warm document had
  no updated frames in the preserved red run. Explicitly invalidating its
  reader-bound cart entry makes authoritative regeneration reach that document
  before any new GET or restoring the fault probe. A control removes this step.
- Lifetime ZERO only proved a recheck, not a live clock. The replacement holds
  evaluation until the actual DB clock crosses its original nonzero deadline;
  a transaction_timestamp control must fail physical rollback assertions.
- Void/error response conversion was overbroad. Pre-execution typed argument
  rejection is known negative; COMMIT errors without confirming receipts are
  conservatively unknown. The deferred-constraint test still requires no rows,
  receipt, outbox/audit events or attempted business patches; empty authoritative
  convergence frames are allowed.
- An old PG receipt-race fixture inserted raw sent arguments; it now constructs
  the same current-reader envelope as its simulated legitimate first host.
  A separate legacy-unbound receipt test proves no adoption or duplication.
- Activity admission now has five atomic statements (begin, bound wait, advisory
  serialization, live touch, commit) plus one read-only final delivery check.
  Its exact pin is six; fresh anonymous HTML retains one final inspection.

The first three receipt cases failed before repair; the initial broad native
run had six failures; a follow-up had one stale frame-count contract; preflight
caught newly test-only entry methods and a changed argument count. Those raw
failures remain under docs/evidence/E14/session-commands-2026-10-27. Preliminary
passing runs are not credited to the final candidate commit. Final gates and
separate source review must identify the exact immutable SHA. Existing scheduler
changes remain in the integration ancestry and require their own affected gate.
