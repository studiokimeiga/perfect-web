# Independent immutable-reader review — 2026-10-26

Reviewed read-only in isolated WSL worktree `/home/haki/code/pw-wt/review-session-readers-20261026`. All nine snapshot file SHA256s match `work/reader-review-manifest.json`. Compared this turn's delta to `work/session-before/`, and inspected the surrounding identity, query, initial response, worker and data-layer call paths. No tests, mutation campaigns, source changes or remote actions were performed by the reviewer.

Read AGENTS.md, charter operating/privacy/query/testing requirements, architecture and ADRs 0258/0369/0389. Review does not approve the full unfinished session revocation branch for merge.

## Bounded slice assessment

No new blocking correctness defect found in the immutable synchronous-render binding itself. The owning Principals Arc and cookie are both matched, so a concurrent store or another cookie cannot inherit the actor. Explicit Anonymous remains distinct from absent binding. The binding is cloned before work, retained across initial document attempts, and explicitly transferred to Settling threads. Data-layer viewer calls, platform current_user calls, `requires` evaluation and private query keys converge on that same binding. Closure-scoped restoration has no escaping guard; both nesting and unwinding restore prior state. Live `inspect_reader` deliberately bypasses the frozen invocation, preventing the fix from authorizing delivery after revocation. The design is accurately limited to this synchronous host; it is not an async task-local claim.

The new worker tests call the real Settling worker and compiled feed Me query. They validate both the principal-derived fields and the generation partition. They are narrower than an actual browser or socket-level progressive-fill acceptance test, as the ADR acknowledges.

## Actionable test improvement

At `spikes/own-renderer/server/src/tests/session_revocation.rs:500-509`, the anonymous/authenticated/anonymous HTTP regression checks fixture completion and absence of the transient private marker, but does not require HTTP200 or the intended anonymous page. A regression that rejects that cycle with 503/404 would also pass. Add success/status and anonymous-state assertions, so this test demonstrates the legitimate public page still renders through the cycle. This is an evidence gap, not an observed failure of the repair.

The three `identity::invocation_tests` are outside the old `identity::tests::` filter. Root already identified that filter omission; require the broader identity filter in the final recorded run.

## Existing broader delivery gap found during review

`spikes/own-renderer/server/src/main.rs:10470-10494` directly sends `Unread::Moved`, NotFound and Failed outcomes through `moved`/`respond` in the `unavailable` closure, without the final live-authority check used by `respond_streaming`. In particular `fetch_binding` at lines3973-3983 carries a query's redirect String into `Unread::Moved`, and `moved` writes that query-derived destination in Location and HTML. A session-private query that reaches this path after revocation is therefore not covered by the normal initial-HTML guard. This predates this turn's immutable actor delta. Keep it an explicit unfinished acceptance obligation, or add a failing private-redirect fixture and guard the fixed response before delivery. No reproduction was run under this read-only review assignment.

Other acknowledged incompleteness remains: request-local admission ordering, immutable binding of keyed/background rederivations, PostgreSQL migration/authority acceptance, command business-transaction fences and receipt recall, transport-specific controls and full reached recipes/browsers. Nine mutation-anchor drifts were reported by root and must be repaired semantically and checked; no passing-preflight claim is made here.

## Acceptance condition

The bounded immutable-reader source slice is acceptable as incremental local work, subject to root's new controls and correctly filtered final tests/preflight. Do not characterize this review as approval to merge or as complete session revocation acceptance. Final source changes since this snapshot require their own delta review.


## Follow-up validation review

Reviewed the strengthened current anonymous-cycle test, bounded producer `scripts/session_reader_mutations.py`, all nine semantic retargets in five existing mutation producers, `work/retargeted-reader-anchors.json`, and the root-produced `reader-controls.json`/raw native assertion output. This was read-only review; no campaign was run by the reviewer.

The strengthened regression now requires HTTP200 and `SignedIn~holds == false`; this closes the reported false-pass-by-refusal gap. The bounded producer names four exact native tests, requires complete green baseline and complete per-mutant inventory with no ignored/measured tests, validates native assertion source locations, and compares restored source hashes. Its three edits remove the intended production actor lookup, child-worker propagation and live-inspection bypass protections. The observed native failures concern the intended actor fields, actor partition and private initial HTML, rather than fixture/setup assertions.

The intermediate producer returned0 before its finally block could discover restoration failure. Root noticed and repaired this during review. In the reviewed final script the successful return happens after finally and uses the final `report["valid"]`, so restoration failure cannot exit successfully. The report also records its producer hash. Reviewed producer SHA256: `fb7e16fbfc83f6935c27f60a3fb001d46c9b791b6a3cc8f74e9f60ef2beb6022`.

Nine existing-anchor retargets preserve the named mutation's meaning: long-poll cutoff disabled in the same loop; document rendering moved under the same subscriber lock; only the pushed-count guard disabled while the added reader guard remains; NotFound status404 changed to503; persisted open removed with the new epoch argument; authoritativeNone no longer clears its local session; default lifetimes substituted; local expired held principal returned instead of forgotten; title forced to Store. No additional source mutation or loosened baseline criteria was found. Actual kills for those nine retargeted legacy controls have not been demonstrated by this review; in particular admission now also checks expiry, so the expiry mutant's discrimination must be measured when that recipe runs.

No additional blocker found in the bounded validation source after the exit-status repair. Evidence produced before the final producer edit is historical; record a fresh run for the final producer hash before claiming exact-final-tooling acceptance. Scope remains incremental local work, with the broader initial redirect/error authority gap, PostgreSQL, command/receipt fences, keyed/background paths and full recipe/browser gates still open.


## Follow-up review: six broader-suite failures

Reviewed the current HEAD diff in messages.rs, notifications.rs, second_host.rs and ADR0389, and the six native assertion failures in `work/reader-full-server.txt` (801passed/6failed/9ignored, 0filtered). This was read-only; no tests or campaigns were run by the reviewer. Runtime identity/main source hashes still match the original immutable review snapshot.

No blocker found in these test-contract repairs. The original four message/notification failures compare actual user arguments such as carol/alice against a synthetic guest name made by mistakenly decoding the new versioned generation field as a raw cookie. The new code requires the entire partition field to equal exactly one known fixture reader's cookie-plus-current-generation partition, then independently compares the query's user argument to that reader's actual user. Unknown/shared fields, multiple matching owners and wrong-user arguments fail. Existing rendered-page, private-message/notification, keyed-read and public-cache assertions remain intact; this is not a privacy assertion removal. The fixture identities remain unchanged throughout these witnesses, so matching their current generations is appropriate.

The second-host statement test remains an exact equality pin, not an increased upper bound: existing-cookie HTML is exactly2 statements (activity admission plus final read-only authority inspection); fresh anonymous HTML is exactly1 (inspection only). The handler and SessionsPg call paths support that accounting. The renamed test has no stale executable references in scripts/just/source. Its counters exclude the completed sign-in fixture.

The memory lifetime test now explicitly verifies that `of` leaves seen at exactly90seconds before calling `refresh`; only explicit client admission must advance it to at least100seconds. Existing idle/absolute expiry and removal assertions remain. This strengthens the new non-renewing-read contract instead of loosening expiry expectations.

ADR0389 accurately retains the failed first full run and separates these contract updates from final acceptance. Focused six-test rerun, complete concurrent final suite and final three-control rerun remain the root's pending verification; this review does not turn those into passes. Legacy anchor mutation campaigns, browsers and the broader session authority gaps remain open.

Reviewed file hashes:
- `spikes/own-renderer/server/src/tests/messages.rs`: `57f12d52db6f2beae52bcf1792e5d525163ed42627eef0fff1b15459058dd428`
- `spikes/own-renderer/server/src/tests/notifications.rs`: `045dcb84b03a11e77ef8db91fa03abcbacce8e2d08dae07e9214e175ac277ef9`
- `spikes/own-renderer/server/src/tests/second_host.rs`: `f88a5b324ad4b094fbd703808578368b4b7a3a66237072dddf1f13fc780c792d`
- `docs/DECISIONS/ADR-0389-session-authority-is-checked-at-delivery-and-commit.md`: `6e2d387f7be3917cb6e1ec2efae13aaa80d829e8ab863146b91c6fe96c3cdcc1`
