# Immutable synchronous readers: local continuation, 2026-10-26

The session branch remains unmerged and incomplete. The preceding working tree
contained an unfinished delivery/epoch repair and an assertion-failing anonymous
account-cycle regression. This continuation preserves that work and closes the
specific mutable-render-actor gap. Master was fetched read-only and remained at
`a13b3c3806b928d4a7e135819d58ff5f39c2a6a6`; its unrelated .claude/ is preserved.

## Behavior

An anonymous feed render could observe an account signed in between bindings,
then deliver that account's private Me display name after it signed out again.
The document's reader label and final live-authority comparison agreed on
Anonymous and therefore did not detect the intermediate actor.

The host now binds principal reads, current_user/requires and private query keys
to the same immutable ReaderIdentity for a synchronous initial render and its
retries. Explicit Anonymous is a binding. The binding belongs to one Principals
instance and cookie, and nested scopes restore on return or unwind. Initial
routes bind before query/stream argument evaluation. Settling explicitly carries
the binding into each child worker. Live authority inspection bypasses the
binding, so computation as an old actor never grants delivery after revocation.
ADR-0389 records the design and checked Rust thread-local primary documentation.

The native witnesses include a successful HTTP200 anonymous account cycle,
an account-bound compiled Me worker and an explicitly anonymous compiled Me
worker. Worker assertions inspect the actual result and resource-flight partition;
failed queries/setup receive no credit. Scope tests cover nested identity stores,
other cookies, unwind restoration and live revocation while computation is bound.

## Evidence and reproduction

Raw local receipts, source hashes, commands and the independent review are under
[`docs/evidence/E14/session-readers-2026-10-26`](../evidence/E14/session-readers-2026-10-26/).
They establish the bounded source contract, not a hosted canonical receipt.
Local tool setup sources `/home/haki/pw-setup/env.sh`, prepends the maintained
`.toolchain/prefix/bin`, sets PW_STORE_TEST_LAYER=memory, and reuses the scheduler
worktree's target directory for cache only. No concurrent source campaigns ran.

Run from the repository root after that environment setup:

```sh
cargo test --locked -p pw-dev-server -- --test-threads=1
cargo test --locked -p pw-dev-server --bin pw-dev-server -- identity:: tests::session_revocation:: --test-threads=1
python3 -m unittest scripts.tests.test_session_reader_controls scripts.tests.test_cargo_assertion_results scripts.tests.test_cargo_test_results
python3 scripts/session_reader_mutations.py --report /tmp/session-reader-controls.json
just preflight
cargo clippy --locked -p pw-dev-server --all-targets -- -D warnings
cargo build --locked -p pw-dev-server
```

The new recipe freezes four exact native identities, requires zero ignored or
measured tests, and attributes failures to assertion macro sites in the dedicated
test source. Its three controls remove principal binding, worker propagation and
live-inspection bypass respectively. Build/setup/IO/timeouts are invalid; all
selected tests must complete for every control and all source hashes must restore.
Six producer calibrations include a final source mismatch that must return nonzero,
and cancellation that must restore its source and propagate. The shared parser
calibrations run alongside them. Preliminary validation/line-ending/compiler
attempts are not counted as passing results.

## Independent review and remaining acceptance

A separate reviewer inspected a frozen nine-file worktree, verified every hash,
and found no new blocker in this synchronous-reader slice. The reviewer requested
HTTP200 and anonymous-manifest assertions; both were added. A follow-up review
accepted the strict producer and nine semantic anchor retargets, after correcting
its final exit to reflect the restoration result. The review does not approve
merging the full session branch.

The same review found that query-derived redirects/errors bypass the successful
HTML delivery check. That path needs its own failing-first regression and repair.
Existing-document rederivations, keyed reads, shared teardown/timers, explicit
request-local admission, transaction-bound browser commands and receipt recall,
session-specific PostgreSQL migrations/two-host races and actual browsers remain
open. Nine retargeted legacy controls have unique anchors; their full campaign
outcomes remain unmeasured here. No hosted workflow, browser acceptance, complete
application capability, production migration, push or merge is claimed.
