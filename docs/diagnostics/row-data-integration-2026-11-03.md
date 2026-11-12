# Declared data and row authority: local integration (2026-11-03)

The user asked to continue after the cache-fence repair. This work reuses the
clean `/home/haki/code/pw-wt/data-path` checkout on `fix/row-data-integration`.
Its first parent is `3ae9c3c5`, which records the verified cache-fence source
`b496bdc3`. No remote publication or deployment is part of this continuation.

## Preservation and baseline

The original row-ownership checkout remains on `track/row-ownership` at
`ff9ff2c6`, with its 19 default porcelain entries (20 actual files). A temporary
Git index captured those edits in local snapshot
`b407ede454126709e7bf217ac82664a742d89c48`, branch
`wip/row-ownership-recovery`. The original branch, real index, status and every
edited file's SHA-256 were checked unchanged. The preservation manifest is in
this Codex chat's `work/row-ownership-snapshot/`, outside the source repository.
No recovered conversation transcripts are copied into this repository.

Before integration, the saved source passed the focused compiler tests (11
predicate-body and 20 table-owner cases) and three real server guarded-query
tests. Those observations do not establish generic server wiring for owned
tables or a shared query/predicate snapshot.

## Integration rulings

- Both tracks reserved PW69xx. Keep all public diagnostic numbers unchanged,
  with one registry owner, `Dating`, for the integrated H1/H2 privacy rules,
  including `ROW_HOLDS_HANDLE` (PW6901). The one-owner-per-block gate remains
  intact. This changes registration ownership, not authored language behavior.
- Preserve both distinct host APIs: the caller-less authorized-table path and
  the row-aware `call_with_tables_as` path. The former explicitly runs as
  `Caller::Nobody`; it must not acquire row authority merely because histories
  were merged. The first combined build caught the missing caller argument.
- The platform retains both `account_of` and `id_of`. Recompute the trusted
  platform source pin only after verifying their merged declarations; do not
  waive its guard.
- Regenerate the diagnostics guide from the merged registry. Preserve both
  recipe imports. For conflicting planner timings, use the larger previously
  recorded value; these are conservative planning estimates, not a new measured
  performance result.

## Next proof boundary

ADR-0115's amendment and the owned-tables ADR require a program predicate and
its guarded query to read one state. The row branch's `CallTables` currently
reads published tables separately; the data-path branch supplies snapshots.
Their integration must preserve the engine's caller, gate and shared fuel
budget while giving both reads one snapshot. Generic server wiring and real
two-user witnesses remain required; PostgreSQL's corresponding transaction
proof is separate. No row-authority completion is claimed by merging code.

## Verification

Pending combined compiler/adapter/server tests, strict whole campaigns,
independent review and exact-source receipts. The earlier cache repair's
receipts remain at its own branch and source; they are not transferred to the
combined tree.

The integration baseline now passes `cargo check --locked -p pw-dev-server`,
the three compiler targets `platform_contracts`, `table_owners`,
`predicate_bodies`, the four adapter targets `compiled_predicates`,
`compiled_owners`, `compiled_guarded`, `compiled_snapshot`, seven combined
guarded-query/cache-fence server cases, and all twelve code-registry tests.
The measured trusted-platform union pin is `0x0c6761b484fa478d`, retaining
`context.account_of` and `capability.id_of`. All 5,091 mutation anchors match.
These are merge-baseline checks, not a whole-campaign or server-wiring claim.

## One query state

The failing regression at `9db05cc4` read a row in its compiled predicate,
committed a separate removal before its body, and observed `None` in the body.
`Snapshot` now implements the existing engine `CallTables` interface, giving
both phases callbacks through `read_host_fns_at` and the same captured state.
The regression also repeats on the old snapshot, then verifies that a fresh
snapshot refuses the removed row before running its body. It passes; both
targeted controls (live body reads and live predicate reads) are killed by
completed assertions, with an 84-test unmutated baseline and no process stop.
Logs: `/tmp/pw-row-snapshot-before.log`, `/tmp/pw-row-snapshot-after.log`,
`/tmp/pw-row-snapshot-controls.log`. Whole campaigns and independent review
remain pending; these targeted controls alone are not acceptance evidence.

## Generic server boundary

Failing-first source `9dc74c3a` contains the served two-user and compiled
predicate witnesses. Both initially failed at startup: the deployment did
not know `Wrote`. The server now validates and admits each compiled predicate
named by a requirement, and the engine asks it as the invocation's bound
principal, under the existing limits and shared fuel. Commands supply their
staged predicate reads; table queries supply one render or standalone
snapshot. No program predicate is answered by a deployment callback.

The first repair denied Bob correctly but lost `Wrote`'s refusal identity.
`InvocationError` now distinguishes an explicit false result from evaluation
failure. The HTTP route reports 403 with the authored words. A read error
whose text looks like a denial is not classified as one. The eight guarded
server tests and fourteen compiled-predicate tests pass, including missing
compiled artifacts, pure predicates and a real intervening removal outside
a render. Controls were adapted to the same ownership boundaries, and the
row campaign now includes the real server witnesses. Whole acceptance is
still pending. Snapshot-only independent review of `a35da34a` found no
actionable code defect; the reviewer requested the dated ADR correction.

## Review repairs: reused answers and refusal identity

Independent review of `42999fe8` identified cached-query authority as a
hypothesis. It was reproduced at `d65dc77b`: a positive-freshness private
`Club` query was warmed for its reader, the reader's post was deleted, a
cold `OwnsPost` call denied, and HTTP still returned the warm secret. The
cache really was reused (no new query request started). Cached/deduplicated
answers now pass an engine authorization-only call against this request's
bound caller and selected read state; a false predicate or evaluation error
escapes the fallback layer. Fresh executions still authorize once and spend
predicate/body fuel from their existing single budget. The engine's
authorization-only witness proves no body callback runs and fuel is spent.
Ordinary data freshness remains the cache policy's; authorization is current
to the invocation's snapshot, not a claim about the instant HTTP is delivered.

Review also predicted qualified names losing words. At `c5d9a1b0`, a real
HTTP denial of `guards.Allowed` said an empty string. The typed denial now
carries the compiled predicate's declared name, the key of its authored
words. Its diagnostic still preserves the requirement spelling.

While repairing reuse, a denied query refreshed by a committed command was
found contaminating its command's thread-local refusal record. Query checks
now evaluate without command refusal reporting; only command denials enter
that record. The warm-cache regression verifies the removal still commits
and its receipt decodes. A shared-flight witness uses the query-cache seam
directly: HTTP serializes requests in one session, so the first attempted
HTTP-based fixture timed out and earns no passing-test credit. The corrected
witness proves a real deduplicated flight, permits the earlier invocation,
and refuses a joining read that began after the removal.

All eleven guarded server witnesses and fifteen compiled-predicate tests
pass locally. The full server suite at the earlier `42999fe8` checkpoint
passed 915 tests with nine ignored; that is interim history, not acceptance
of these review repairs. New whole campaigns and final review are pending.

## Strict campaign witness correction

The whole row campaign at `16f91956` completed with 66 controls killed and
two **INVALID**, so the recipe exited 1 and earns no acceptance credit.
The controls rejecting every guarded query and every page binding hit the
existing feed fixture helper's `expect("the feed builds")`. A compiler
refusal there is a harness panic, not a completed assertion failure. The
helper now asserts its build result before reading it; the native assertion
validator is unchanged. All source files restored and all 5,103 anchors
matched before this correction. No process was stopped by the memory bound.
Log: `/tmp/pw-row-campaign.log`. Rerun the whole campaign after preflight.

Independent read-only final review of `16f91956` has no outstanding actionable
findings; its prior cache and qualified-word findings are addressed. The
helper correction changes test failure reporting only, with whole verification
still pending. PR #72's separate hosted audit found 15 matching archive
digests and 160 successful recipe receipts, all headers on GitHub test merge
`5bf6d4b947769a413377c5165aa696705fc112bf` (PR head `687dd922`). Those results
do not transfer to this newer integration tree. The local broader plan at
`16f91956` reaches 170 recipes in 16 shards, still pending beyond the local
affected gates run here.

Preflight first caught Clippy's type-complexity rule in the test-only
concurrent-read hook. Naming its callback type resolves that without a lint
waiver. `just preflight` now exits 0: formatting, both Clippy configurations,
case check (4,759 paths), and all 5,103 mutation anchors pass. Logs:
`/tmp/pw-row-preflight.log` (expected failed lint),
`/tmp/pw-row-preflight-final.log` (pass). No product behavior changed after
the reviewed `16f91956` checkpoint; the two changes are test failure reporting
and a test callback type alias. Whole verification is rerun on the resulting
source commit rather than credited from the diagnostic campaign.

## Final local verification

Code/test source **`497c0a6c73f86134746791faed5afd9b994023f3`** passed:

| Command | Observed result |
| --- | --- |
| `just e14-row-ownership` | 68/68 controls killed; mutation baseline 97 passed/0 failed |
| `just e14-data-path` | 60/60 controls killed; baseline 62/0; 15 browser checks across Chromium, Firefox and WebKit |
| `just e14-refusal` | 23/23 controls killed; restored browser baseline passes |
| `just e14-refusal-2` | 7/7 controls killed; restored browser baseline passes |
| `RUST_TEST_THREADS=8 just ci` | exit 0, `ci: OK`; server 918 passed/9 ignored; tooling ran 1,177 tests with 5 explicit skips |
| `python3 scripts/mutation_anchors.py` | 5,103 anchors in 326 scripts, each matching once |

CI also judged all 351 source mutants in 567 checks, with 20 intentional
named-rule refusals and zero failing. Skipped/ignored tests are not credited
as passing. No campaign process was stopped by the memory bound. Independent
review at this source has no outstanding actionable findings. The original
row worktree's branch, real index and all 20 file hashes were rechecked
unchanged after the complete run.

Raw receipts, red regressions, the failed diagnostic campaign, original-work
preservation, PR #72 audit and hashes are retained under
[`docs/evidence/E14/row-data-integration/`](../evidence/E14/row-data-integration/manifest.json).
The CI code/tests were the exact committed source; only four generated
receipt files were dirty at its start, and their exact hashes and bytes are
retained. This is local evidence, not a clean published-head landing receipt.
The later evidence/documentation commit changes no code or tests.

Remaining: the row track's browser witnesses and query-refusal presentation,
PostgreSQL `CallTables`/durable integration, private write-flow/H3/H4 and the
broader publication plan. No push, PR mutation, deployment or paid model
benchmark was performed. The PR #72 audit covers its separate test-merge
source, never this newer local code.
