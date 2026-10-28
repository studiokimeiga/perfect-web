# Session authority follow-up — 2026-10-27

Base: merged PR61, a0db8884deb2b3ba37faee59afcdee44a92c6b1c.
This follow-up is not yet accepted or merged. ADR0389 specifies the retained
reader, delivery and commit contract.

The new private-upload witnesses reproduce ten domain failures with one valid
neighbor on the original source. Independent review of the first repair found
an upload admission panic for declared command/stream routes and an authority
inspection while holding upload state. Their separate red records remain.
The provisional-reference repair passes all nineteen focused native tests on
its working source. This includes local expiry during storage, source failure,
shared-byte cleanup, concurrent claim/replacement, full preview-read ordering,
HTTP actor propagation and the two reserved-route neighbors.

Actual committed receipts reproduce a decode error reentering full command
evaluation, malformed stored answers inventing rollback, and real PostgreSQL
Applied/lost-acknowledgement recovery blocked for a stale same-reader page.
The working repair passes all five materializer tests and four selected server
cases, including preservation of a null result. The later cached-refusal neighbor
and strict whole controls still require their final committed-source execution.

[Raw logs and hashes](../evidence/E14/session-followup-2026-10-27/manifest.json)
separate these observations from final acceptance. The upload source-seam
fixtures use memory and the existing Kept interface, not actual two-host
PostgreSQL execution. The receipt case uses a selected real PostgreSQL adapter;
read-only prefix absence was separately checked on both configured databases.
No skipped or incomplete run supplies passing credit.

Reproduce after committing execution inputs:

```sh
just preflight
just e14-upload-authority
just e14-session-receipt-recovery
just e14-session-command-authority
just e14-scheduled-replay
```

Before publication run the affected cheap native checks and Python calibrations.
Before merge independently review the immutable head, then require actual
successful CI/security jobs, full browser engines and every recipe in the
published plan. The whole existing upload/metadata/interaction/account/second-host
campaigns retain their controls; retargeted anchors preserve their faults.
Historical failed diagnostics are not replaced by subsequent green output.


Later review retained separate native red records for contradictory committed
refusal/error fields and for typed negative records in the browser durable
writer context. The refined working receipt checks pass five materializer and
five selected server tests, including actual PostgreSQL lost-acknowledgement
recovery, cached refusal without a durable row and null preservation. An extra
precondition now pins the unknown reservation immediately before stale replay;
its execution at the final source still remains required.

The broader working default-feature server run passed 848 tests with zero
failures; nine existing ignored cases receive no credit. Feature-enabled Clippy
passed with warnings denied. Strict upload producer review reproduced and
repaired post-reap log I/O false-credit, non-test inventory entries, TERM
restoration/reporting gaps and skipped callback fixtures. Twenty controlled
Python calibrations pass; those synthetic libtest calibration outputs are not
Rust or application execution evidence. Whole native mutation and hosted gates
remain pending at a final immutable commit.

## Hosted TodoMVC failure at the first fully audited candidate

The actual original evidence-4 ZIP in verify40617180819 reports
`e14-todomvc` status1 at `f1d19c8e` (source-equivalent PR merge8c153f42).
The mutation script refused a30-second clear-button click timeout even though
another case failed a real assertion. Its14/43 kills are partial and failed;
the green wrapper job is not acceptance. The trace budget retained no trace
for that control. Original logs and metadata remain preserved.

The dirty-toggle-all browser test now waits for its todo's completed row before
checking clear visibility and clicking. Both reviews support this semantic
precondition; actual interleaving remains an inference and the original local
control produced valid assertions rather than reproducing the hosted timeout.
Before and after focused runs each have50green baseline,48pass/2assertion-fail
control,50green restored results, with the exact same50identities. After the
repair the added row assertion passes and the existing clear visibility fails.
See [the raw record](../evidence/E14/todomvc-completed-precondition-2026-10-28/manifest.json).
The initial external helper callback bug receives no acceptance credit.

No producer, operator, retry, timeout or skip rule is relaxed. These are working
source diagnostics. Required final commands remain `just preflight`,
`just e14-todomvc`, the whole command/upload/receipt/scheduler recipes and all
actual hosted planned checks at the next immutable head. No merge has occurred.


The next published candidate `a67c2111` also remains failed: fresh analyzers
correctly treated three historical Python helper copies and two relocated
browser spec copies as executable modules. Local raw reproductions retain15
Ruff and8Fallow introduced findings. Only their evidence storage changes to
lossless gzip; decoded executed source is preserved and hash-bound. No product,
actual test, producer, policy, ignore or threshold changes. Fresh final-head
gates remain mandatory; the earlier focused native passes are historical.
