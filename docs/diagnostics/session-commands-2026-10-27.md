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


## Browser protocol classification and final-run discipline

The first hosted three-engine run at d8c84940 had the same single failure in
all engines: a request missing an interaction ID returned an unknown outcome,
while no evaluation could start. The shared interaction validator now runs in
HTTP preflight as well as the command gateway. Missing, empty, malformed and
oversized IDs are known negative protocol refusals; commit/recall uncertainty
retains its existing meaning. An actual HTTP native regression failed before
repair and passes with a legitimate write/receipt neighbor; the focused browser
spec passes all nine tests across Chromium, Firefox and WebKit. No browser
expectation was weakened. Packages containing compiled Pleris standard/platform
sources are also included in the canonical committed-input guard and manifest.

A local broad native run overlapped a rebuild of the same test executable.
Three child self-probes could not reopen its replaced path. That setup-invalid
run is preserved, not a product failure or a passing gate. Final compilation and
native/control execution must run serially. Earlier d8c8 source/control evidence
remains historical and cannot be restamped onto the protocol repair commit.


## Complete check audit and private diagnostics

All local gates at a46d47b0 completed: 822 default native passes (nine existing
Kiokun data measurements ignored and not counted as passes), reader controls 3/3,
command controls 24/24, and scheduled replay 21/21. Independent reviews verified
the command artifacts and all 1,799 compiled inputs/restored bytes, including 22
package files, plus owned-schema absence on both configured databases. Scheduler
review retains its recorded native-output durability and one-URL scope limits.
These exact-a46 results are historical after the next source change.

The final complete PR-check audit found two additional blockers missed by looking
only at the successful PR-triggered workflows. The separate push CI run 40463525070
failed `test_producer_ownership`: it treated a PID file's existence as readiness
before its writer had supplied the number. The aggregate CodeQL check 122053616987
reported 125 alerts even though the dedicated analysis workflow succeeded. Neither
failure is waived or replaced by the other run's green status.

The PID fixture now closes an adjacent staging record before an atomic rename
publishes readiness. A controlled writer pause reproduced all four producer
subcases failing before repair; the full module now passes 11 tests. The child
reaping and original-source restoration checks run unchanged before the added
publication assertion. The publication seam has bounded failure cleanup.

Independent SARIF review traced 121 new high findings to the two Account reader
constructors and overtaint through Result/control-flow error paths; 122 of the 123
high sinks were test diagnostics. This is not proof that principal bytes reach
every error string. It did identify a real independent Debug exposure: the six
principal/reader/actor/cache debug surfaces could disclose cookies, user fields
and generations. A regression failed all six and passes after kind-only Debug
and session-map redaction; actual equality and authority checks still succeed.

Authority admission, inspection and actor checks now return a fieldless AuthorityError.
Its fixed Display categories contain no backend payload. Existing adapter/HTTP
boundaries convert that typed failure to String, preserving known changed-reader
and uncertain-outcome handling. Kept session/inspection failure maps to SourceUnavailable; failed admission evicts the old cache entry before advancing its revision;
cached identity cannot grant authority, and a same-generation source recovery
still succeeds. A real unavailable-source probe and an unsafe legacy trace probe
both fail their intended native confidentiality assertions, then pass with the
restored repair. Command traces emit only operation/outcome metadata, never raw
why, result or request identifiers. Two old privacy-test diagnostics no longer
print token/secret bytes; two network-test diagnostics escape line separators.
Their boolean predicates and legitimate neighboring cases remain intact.

The CodeQL query configuration, severities, Account variant, results and business
receipt payloads are unchanged. No alert is dismissed or suppressed. The next
frozen commit requires its own aggregate security gate and all planned exact-head
CI/browser/recipe checks. Source guidance: the official cleartext-logging query
help and the pinned CodeQL2.27.3 fieldless-enum barrier reviewed separately.

Reproduce the focused probes with `python3 -m unittest -v
scripts/tests/test_producer_ownership.py`, `cargo test --locked -p pw-dev-server
--bin pw-dev-server -- identity::invocation_tests:: --test-threads=1`, and the
session_commands native module. The raw red/green records are under the dated
evidence directory. The initial Debug selection matched zero tests; its explicitly
INVALID log is retained and supplies no red or green credit.


## Remaining trace channels and explicit authority branches

The fresh aggregate analysis at 9fed8179 reported 122 findings: 120 high and two
medium. The real command-log sink and the two old privacy assertions were
resolved. Independent review parsed all 478 remaining authority flows: each
crossed a private Reader success through one of three TryExpr sites into a
residual error path. Typed fieldless errors alone did not stop that extraction.
The authority factories and their business/receipt callers now keep success and
bounded error branches explicit. The same checks, categories, lock order and
uncertain disposition are retained. One expression-scoped expectation for the
stylistic Clippy question_mark lint documents the deliberate explicit boundary;
no security query, severity or alert is excluded. Both remaining network-test
diagnostics now use fixed messages and retain their original predicates.

Independent review then found older PW_TRACE paths that still printed bearer
cookies, derived cart values and document/backend details. The in-progress 9fed
command campaign was deliberately cancelled in a verified non-PG ownership
phase: ten of 24 controls completed, report valid=False, restored=True, and no
full gate credit. Independent audit verified all 70 artifacts and 1,799 restored
inputs; all 154 owned schemas were absent from both actual databases. No
scheduler campaign ran at that head. Its cancelled evidence is retained.

The new process-level regression self-spawns the same exact native test with
PW_TRACE enabled only in the child. It builds the canonical Store with an
explicit memory adapter, renders a real private document, commits a distinctive
private quantity, commits a neighboring write while regeneration is faulted,
checks the actual stale materializer entry, then verifies its recovered body and
the real unshowable-document path. Before repair, its complete child fixture
revealed cookie, backend, cart and recovered-cart categories on stderr.

All nine audited PW_TRACE call sites now accept a TraceEvent whose fields are
only operation roles, counts and booleans. Cookies, Doc objects, query values,
operation payloads, request identifiers and backend messages cannot be passed
through this API. The parent privacy assertion sees no such canaries after
repair. The backend token is tested independently of newline escaping. Setup,
child failure, zero selection, timeout and missing progress use non-assert
expect failures so they cannot earn a native domain assertion kill. The child
has a bounded lifetime and a kill/reap guard; captured private output is never
dumped into the parent's failure message. These claims cover the audited trace
and identity Debug surfaces, not every startup/provider diagnostic.

The canonical command instrument now selects 26 controls/30 edits, retaining all
prior controls plus separate actual trace-payload and escaped-error leak
controls. The receipt-owner composite and older cross-host receipt control were
reanchored to the same statements after explicit-branch indentation changed.
All repository mutation anchors pass. The new native trace witness expands the
frozen memory inventory to 17; resource remains one and real PostgreSQL 14.
Only fresh execution at the next immutable head can establish its full gate.

Focused reproduction adds `cargo test --locked -p pw-dev-server --bin
pw-dev-server -- tests::session_commands::tracing_an_actual_store_command_never_discloses_private_state
--exact --test-threads=1`. The process red/green and precommit outputs are in the
dated evidence directory. The two trace controls must be killed at the parent
privacy assertion after the complete actual child fixture, never at a setup
expectation. A fresh unchanged aggregate CodeQL result remains required.
