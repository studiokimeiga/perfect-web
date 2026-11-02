# ADR-0389: session authority is checked at delivery and commit

Status: proposed, implementation and acceptance pending. Date: 2026-10-26.
Milestone: E14; generic platform authority and the reference applications.

## Observed failures

Fresh maintained source8eda43e2 built a locked development server (SHA256
62cdb48645a860f667628e56c90bf07d359ba4668e408362413c55bc7533ddd5)
and compiled the feed from its .pw sources. Actual HTTP observations show:
old documents poll/stream queued private frames after logout, idle expiry and
absolute expiry; on two PostgreSQL18.6 hosts a stream receives a private
message created after logout303 and session-row deletion; a command held at
its observed pre-body refresh barrier commits202 and a post after those same
logout/row-absence barriers. A subsequent new command403, same-host delayed
command403/no post, and relationship withdrawal403 are controls, not repairs.
Only owned fresh schemas were used and removal was independently queried.
These are development-host findings, not production-deployment claims.

## Required authority contract

- The session authority supplies read-only current inspection separately from
  explicit client activity. Internal reads and delivery polls cannot renew idle
  lifetime, reset absolute origin, or resurrect an expired/revoked generation.
  Refresh results cannot overwrite a newer revocation or actor binding.
- A document/subscription binds the reader/session generation it was produced
  for. Each fixed outgoing batch is snapshotted before current authoritative
  inspection, then delivered only for that binding. A failed authority read
  refuses delivery. Already authorized in-flight bytes may finish; bytes already
  delivered cannot be recalled. No later batch borrows an old grant. Queue and
  query cleanup is not a substitute for this check. Anonymous/public readers,
  multiple sessions for one user, reconnect and account rotation need neighbors.
- A browser command binds the actual actor after its body and waits. The adapter
  fences current session/generation/expiry with its business/platform transaction,
  coordinating with revocation until commit/publication. A separate last refresh
  before SQL is insufficient. Receipt recall is a delivery of an answer and must
  have current authority too; a revoked cookie must not regain rights by retry.
  Platform on-behalf scheduled/payment principals are a distinct authority path,
  never accepted from a browser cookie.
- Cross-host invalidation may wake a subscriber for prompt cleanup; it never
  substitutes for the authoritative check. Independent database sources cannot
  silently claim one atomic fence. The final mechanism must be independently
  reviewed before adoption; no competing per-application authorization backend.

## Primary references checked 2026-10-26

[OWASP session management](https://cheatsheetseries.owasp.org/cheatsheets/Session_Management_Cheat_Sheet.html)
requires server-side invalidation/expiry. [PostgreSQL18 locking](https://www.postgresql.org/docs/18/explicit-locking.html)
defines conflicting row locks, held through transaction completion, and warns
that deadlocks require consistent acquisition order. Those mechanisms do not
by themselves establish an end-to-end application/session fence.

## Acceptance still required

Maintain exact native/HTTP regressions for all observed barriers, current-reader
positive neighbors, logout before/after authority acquisition, unchanged-key
receipt replay, expiry during work, remote revocation, store/feed/memory adapters,
failed authority reads and cleanup, and multiple readers. Controls must remove
each actual delivery/commit authority check and produce complete native assertion
failures; setup/SQL/compiler/IO/timeouts/worker crashes earn no kill credit.
Actual browser application and all reached legacy recipes remain separate gates.
No source or complete-app readiness is claimed by a red characterization.

## Delivery foundation, selected before implementation

The retained reader identity includes an OS-random16-byte epoch, changed on
 every open, even when an internal caller repeats a session id and principal.
Its authority source is the server's fixed identity adapter; its cookie/session
key, principal and epoch are compared together. PostgreSQL adds a NOT NULL,
16-byte-checked epoch to both session schemas. Existing rows receive a zero
epoch scoped to their existing hashed cookie, which is not a new credential.
Old workers must be quiesced before rollout, as they do not know this column
or the new delivery checks. A fresh sign-in assigns a fresh nonce.

Read-only inspection uses the database clock and writes no seen/opened state.
The local equivalent never touches Held.seen. Internal principal reads cease
being client activity. Actual request admission is explicit activity; cache
refreshes preserve the same epoch's local timing origin and cannot install over
an intervening local close/newer admission. These are delivery prerequisites;
the transaction-bound command actor/fence remains a separate unfinished step.

## Independent review: local admission ordering

The first delivery implementation used one global cache revision. A deterministic
barrier regression reproduces both authoritative None and failed authority read
being suppressed by an unrelated session's open, retaining the target principal.
Both native cases fail before the repair. Per-session weak opaque tickets now
coordinate completed cache writes without unbounded abandoned ticket retention
or counter wrap/address ABA. The identity cache write lock precedes a short ticket
lock; no network request holds either. The same-session request still needs its
own explicit admission result and immutable invocation actor; this cache ordering
repair does not by itself supply transaction or render authority.


## Immutable read actors (2026-10-26 continuation)

A deterministic real HTTP regression starts an anonymous feed render, signs
in a transient named account between bindings, then signs out after Me is
read. Before the repair the initial response contains that transient account's
private display name despite beginning and ending anonymous. A reader label
and a final authority comparison alone cannot prevent that cycle.

Synchronous reads now scope one immutable ReaderIdentity to the owning
Principals instance and cookie. Both data-layer principal lookups and platform
current_user/requires calls use that binding, including explicit Anonymous;
private query keys use the same actor generation. Initial route handling binds
before query/stream argument evaluation; direct document callers bind before
drain and retain it across retries. Settling explicitly copies the binding into
each child worker. Nested scopes restore the previous binding on return or
unwind. Live authority inspection deliberately bypasses this context. This
mechanism follows the synchronous host's existing clock/render-snapshot scope;
it is not an async task-local contract or a replacement for transaction fencing.

The [Rust LocalKey documentation](https://doc.rust-lang.org/std/thread/struct.LocalKey.html)
was checked 2026-10-26: each child thread has its own storage. Therefore worker
propagation is explicit and separately regression-tested with compiled Me
queries, for both account and anonymous readers. Live revocation must still
refuse their delivery. Request-local admission ordering, all background
rederivations/keyed paths, PostgreSQL acceptance and atomic command/receipt
fences remain separate acceptance obligations; this slice does not close them.


Independent review of the immutable-reader slice found no new actor-scope
blocker. Its requested valid-neighbor check now requires HTTP200 and an
anonymous SignedIn manifest after the account cycle, as well as no private
marker. The same review identified a remaining initial-response path: query-
derived redirects/errors in serve_bound's unavailable closure bypass the
successful-page delivery check. Those outcomes, and serve_page's error paths,
need their own final authority decision and regressions before complete
initial-response protection can be claimed. They remain unmerged/open here.


The first broader default-feature server run completed with801passed,6failed,
9ignored. Four privacy witnesses parsed the newly structured partition field as
if it were the raw cookie; they now require one exact fixture reader partition
and retain the independent user-argument equality check. One old SQL pin allowed
only request admission: successful existing-cookie HTML now has exactly two
session statements (one activity admission, one final read-only inspection), and
a fresh cookie exactly one inspection. The lifetime witness now separately proves
internal of does not touch seen and explicit refresh does. These are contract
updates to legacy tests, not removal of privacy assertions or arbitrary budget
increases. The failed first run is retained, and affected/final runs are separate.


## Browser command and receipt fence, selected 2026-10-27

The three new native witnesses first fail: another cookie recalls a page owner's
receipt, a logged-out cookie recalls its old receipt, and a replacement actor
under the same cookie receives the predecessor's success. No repair is credited
by those red runs.

A browser command carries an immutable CommandActor, distinct from trusted
platform/Clock invocations. Its generation comes from an explicit request-local
admission result after the body, survives turn waits, and is checked again before
staged operations. A separate weak per-cookie local read/write gate coordinates
memory command evaluation, materializer commit and publish with open/close and
cache admission; its lock order is gate then sessions map, never the existing
revision ticket followed by a cache lock. Expiry is checked at the final commit
admission point; a command already admitted cannot retrospectively be recalled.
All staging/authority locks end before post-commit derivation or receipt recovery.

PG staging validates source identity against its own SessionsPg. It acquires one
cookie-hash advisory session lock outside the business transaction, then starts
the business transaction so its MVCC snapshot follows any lock wait, locks the
session row by hash FOR UPDATE, and compares full principal/epoch/lifetimes after
waiting. Identity open/close take the same advisory key in their own real atomic
transaction, including absent anonymous rows. Final DB-clock inspection is on
the business Client after blocking claim/outbox work, immediately before COMMIT.
Lock acquisition is bounded; repeated acquisition refuses; rollback/unlock
uncertainty discards the socket, and unlock's boolean must be true before reuse.
All old workers must be quiesced; raw privileged SQL that ignores the advisory
protocol is not a supported session-open operation. Existing-row UPDATE/DELETE
still conflicts with FOR UPDATE.

Persisted receipt keys stay cookie+component+interaction. The stored sent value
becomes a versioned envelope of immutable reader identity and arguments. Thus a
known unbound legacy record or another generation's record refuses without
returning its private answer or rerunning writes; there is no unsafe migration
miss that treats an old committed operation as new. In-process reservations and
outcomes additionally use a reader-bound key, so another generation cannot join
a flight. Stale-page receipt lookup uses only the caller's cookie, never a page
owner chosen by an untrusted document number. Document ownership also includes
the retained generation. Authorized same-reader retries after page loss remain
valid neighbors. This explicitly supersedes the old cross-cookie guest-to-account
receipt exception: cart adoption does not grant access to an old receipt.

The immutable actor remains active through fixed response/basis construction.
Every cached, deduplicated, persisted, no-write and declared-error outcome needs
current delivery authority. If revocation wins after a legitimate commit, the
response says its outcome is unknown to this reader and omits result/basis; it
must not assert rollback. Failed source reads refuse. Unknown commit recovery
first releases all old staging/claims/locks, then reads an authorized receipt.
The independently reviewed design is still pending implementation and all gates.


### Review refinements, 2026-10-27

Initial page handlers now retain the explicit admission result through render,
including redirect/error delivery, rather than consulting a later cache write.
Automatic stream polls do not renew client activity. Background telling/keyed
reads use each document's immutable reader. Interaction acknowledgements and
speculated-version histories carry the same full reader tag: a reopened cookie
cannot inherit its predecessor's acknowledgements or committed basis.

The browser-facing store-write helpers use explicit request admission and the
same command fence. The order helper computes its owner under that reader before
staging; trusted maintenance retains its separate internal entry. Ordinary
uncertain acknowledgements refresh from authoritative state after releasing old
staging, without publishing attempted business events. Only uncertain commit
outcomes poison a resource reservation; pre-commit argument/invariant failures
remain known negative outcomes. An evicted interaction bookkeeping row cannot
hide a retained unknown reservation on stale-page retry.

The selected PostgreSQL expiry witness waits for the actual database clock to
cross a nonzero original session deadline after evaluation, then checks physical
business rows, receipt and outbox rollback. A frozen transaction clock must fail
that witness. A false advisory-unlock helper result is separately witnessed;
that helper test alone does not prove the adapter's pool-discard branch. These
statements specify required behavior; final source review and exact-commit gate
receipts remain the acceptance authority.


## Upload leases and receipt recovery follow-up, 2026-10-27

PR61 landed at a0db8884. A further audit of that source demonstrates private
upload previews after completed logout and account-generation replacement,
a body-held upload publishing after logout, and a request borrowing a later
upload actor. Ten of eleven native witnesses fail on the original upload
source; the current-reader/public-committed-image neighbor passes. The raw
[diagnostics](../evidence/E14/session-followup-2026-10-27/manifest.json) retain
that characterization and the review follow-ups separately.

Temporary upload leases now retain the full immutable reader. The request's
explicit admission governs dispatch even for declared upload routes named
under `/command/` or at `/stream`. Metadata and command claims compare the
retained reader; private previews freeze all staged bytes before read-only
current inspection. Failed source reads and changed/expired authority refuse.

Publication holds the existing local authority gate after body parsing and
rewriting. One provisional blob reference and capacity charge survive while
source inspection runs outside the upload-state mutex. Final publication
requires the exact observed predecessor lease to remain unclaimed and unchanged;
quota is checked again, and local expiry is checked after the last state wait.
An owned reservation releases only its own reference on refusal, preserving a
shared or concurrently replaced image. Capacity counts the prepared and old
images together, so a near-cap replacement can refuse earlier. A PostgreSQL
lease is ephemeral private cache state, not an atomic business transaction;
the later compiled command retains the existing database commit fence.

The receipt audit also reproduces three failures after actual business commits:
a SQLite decode error treated as a missing receipt, malformed stored answers
converted into a known negative outcome, and a now-visible exact receipt blocked
by an old unknown reservation on a stale page. `recalled` now distinguishes a
real no-row result from read/decode failure. Answer decoding requires an object
and boolean commitment, validates optional text fields, and preserves null versus
absent results. Invalid records yield bounded uncertainty without reexecution.
Unknown stale-page recovery may consult the same reader's exact durable receipt;
absence/read failure preserves uncertainty, and the existing known-refusal cache
remains valid when no durable record exists.

These are implementation refinements of this ADR, with local characterizations
and working-tree neighbors, not final immutable-head acceptance. Strict native
controls, full reached legacy recipes, actual browser suites, independent source
review and all published exact-head checks remain required before merge. The
source-seam upload revocation fixtures do not claim a two-host PostgreSQL upload
proof, and the actual PostgreSQL receipt witness uses one selected database URL.


Independent follow-up review additionally reproduced typed but incoherent
`committed: true` plus refusal/error fields, and a typed negative browser
receipt after an actual commit. The generic decoder rejects commitment combined
with error/refusal; the browser durable decoder also requires commitment, since
that writer records only committed business operations. Cached known negatives
and Clock terminal negatives retain their generic decoder and valid neighbors.
This is validation of receipt shape, coherence and writer context, not
cryptographic authentication of arbitrary coherent stored values.


## Command producer cleanup keeps URI diagnostics private, 2026-10-28

A direct helper probe at `5371f0f2` found a further evidence defect. Actual
`psql` parsing of a synthetic malformed URI echoed its sentinel password into
both cleanup logs. Cleanup correctly refused acceptance, but retained private
input in diagnostics. This probe used no actual endpoint or database connection;
whole-campaign reachability under that malformed environment was not executed.

The command producer now applies the receipt producer's cleanup policy: discard
`psql` stderr at child launch, retain the fixed owned-name SELECT stdout and
native status, and record the capture policy on completed processes. Ordinary
native test stderr stays merged and retained. Missing/leftover schemas, nonzero
status, setup/process errors and incomplete inventories still refuse. Exception
records without a completed policy receipt remain invalid. This deliberately
removes cleanup stderr as independently reparsable evidence; it does not filter
stdout or turn a failed query into absence.

Three additive real-child regressions cover both URI logs and result metadata,
stdout/status neighbors, and ordinary native diagnostics. The final 17-test
working baseline/restored runs pass. A captured final privacy testcase against
the exact old producer fails its log-content assertion; two scoped diagnostic
faults each fail one intended assertion across the complete 17-case inventory.
These are producer calibrations, not additional authority controls. All original
26 authority controls and their native inventory rules remain unchanged.

Independent actual `psql` probing on the working repair retains empty logs but
still returns both native exit2 refusals. A separate two-database working fixture
creates only two fresh owned schemas: their presence is refused with exact stdout
retained, their later absence is accepted, and final read-only queries verify
both owned names absent. No configured connection value is saved. The
[raw diagnostic manifest](../evidence/E14/session-command-cleanup-2026-10-28/manifest.json)
separates working results from final-head gates and retains the initial test
provenance and refused stale-bytecode pilot without acceptance credit.
Fresh `just preflight`, producer calibrations, `just e14-session-command-authority`
and every planned exact-head hosted gate remain required before merge.

## Public runtime artifacts skip admission, and overlapping admissions, 2026-11-02

PR #63 (its author's commits 34261a8 to 9ddcfec, finished on 2026-11-02)
changes where admission begins, so it is recorded here.

- **Changed.** A `GET` of exactly `/pw-runtime.mjs`, `/pw-resume.wasm` or
  `/pw-render.wasm` (the path without its query, so the next document's
  `pw-runtime.mjs?document=N` too) is answered from the build's `dist`
  before the session is admitted. The three files are the same for every
  reader, and their answer was `serve_file`'s before as after: no cookie,
  no reader. Every other route is admitted first and fails closed as
  before. `public_runtime_assets_load_while_session_authority_is_unavailable`
  holds both, with a session source that always fails, the query form
  included; it kills three mutants of `scripts/second_host_mutations.py`
  (suite `assets`): the files admitted, exempt only without a query, and
  the handler manifest exempt with them (3 of 3 here, 2026-11-02).
- **Why.** Run 41202348600's `e14-second-host` (Firefox, 2026-10-31): the
  first page loaded, its `pw-runtime.mjs` was answered, and its
  `pw-resume.wasm`, asked for 38 ms after that answer with the cookie the
  page had set, was answered 503 "The session could not be confirmed"
  92 ms later; the runtime never booted.
- **Found, not changed: two overlapping admissions of one session answer
  one of them 503.** `Principals::admit` reads the kept session without a
  lock, then refuses its read as `Superseded` when the session's revision
  moved meanwhile, and every admission moves it when it ends. So with a
  source that never fails, two requests of one session whose reads overlap
  get 200 and 503: reproduced here on 2026-11-02 by a temporary test (not
  kept) with a 300 ms source, for `/` and for `/pw-handlers` alike. Which
  of this or a failed read answered run 41202348600's 503 is not known:
  both answer the same body, and the server logs neither. The exemption
  takes the three runtime files out of it; a page, `/pw-handlers`, a
  handler module and an image are still admitted, and a browser asks for
  some of them at once (two tabs, a page and its icon).
  - The repair is this ADR's own decision: a request superseded by another
    admission (not by a close or an open) answered from its own read
    without installing it, or admission of one session serialized. Not
    taken here: it changes the admission order that the "Independent
    review" section above set, and needs its own regression, mutation
    control and review. A log line naming the refusal's kind would have
    told the two causes apart.
- **What the exemption does not do.** In an outage of the session source,
  the runtime now boots, but `/pw-handlers` and the handler modules are
  still refused: the runtime knows no handler, and a press is recovered by
  reloading onto the 503 (`recoverOnPress`; from reading, not run). The
  handler manifest is built from the build alone (`#build`, `#page` and
  handler lines) and could be exempted as the three files are; it is not,
  pending the repair above.

## Overlapping admissions of one session are each answered, 2026-11-02 (track session-overlap)

- **Corrections first.**
  - The section above recorded PR #63's exemption as the change and the
    overlap as found and not changed. The overlap is now repaired where it
    arose, and **the exemption is removed**: `pw-runtime.mjs`,
    `pw-resume.wasm` and `pw-render.wasm` are admitted as every other
    request is. Its test,
    `public_runtime_assets_load_while_session_authority_is_unavailable`,
    and the three `assets` mutants of `scripts/second_host_mutations.py`
    went with it.
  - The overlap refused more than a page or a file. A command whose
    admission overlapped another of its session was answered 403
    `session-changed` ("Your session changed. Reload the page."), though
    nothing had changed: reproduced below, beside a page and beside another
    command.
  - "`Principals::admit` reads the kept session without a lock" is so in
    this host, not in the database: on PostgreSQL each admission's statement
    takes the session's advisory transaction lock (`SessionsPg::session`),
    so the database already took one session's admissions one at a time. The
    refusal came from this host's ticket alone.
- **Cause.** One opaque ticket per session, replaced at every write of this
  host's copy: an open, a close, an expiry, and every admission's own
  install or removal. An admission took the ticket, read the source holding
  nothing, then refused its read `Superseded` if the ticket had moved. Of
  two admissions of one session whose reads overlapped, the second to end
  always found it moved by the first. Memory alone (no source beyond this
  host) reads under its lock, and was never refused.
- **Characterized first**, at 1fded1a7 (the tests, before any repair):
  `cargo test -p pw-dev-server --bin pw-dev-server -- identity::
  tests::session_overlap::` failed the four admission tests, each
  `Err(Superseded)`, and both overlap tests, each trying every pair and
  naming every refusal: in this process, with a source whose reads meet in
  pairs, a page beside a page, beside `/pw-handlers` and beside a handler
  module, 503 each time; on PostgreSQL, where the test holds the session's
  advisory lock until both admissions wait on it, the same three 503, a post
  beside a page 403 and a post beside a post 403. The runtime's files were
  not admitted at all (the exemption): their pairs' reads met no one. The
  memory guard passed.
- **Repair.** Two tickets per session (`Revision`): `authority`, replaced
  only when this host opens, closes or expires the session, and `written`,
  replaced at every write of its copy, an admission's too. An admission
  answers from its own read, a read made during its request, as a lone
  request's is. A read another admission overtook installs no principal
  over that one's write; a removal (no one, or a failed read) still
  removes, failing closed. A read this host's open, close or expiry
  overtook may predate it, so it is read again, at most `ADMISSION_READS`
  (3) times, then refused `Superseded`: a session is opened once and closed
  once, so a real request reads at most twice. Nothing is held across a
  read, as before, and the lock order (gate, sessions, ticket) is
  unchanged. A refused admission, a page's 503 and a command's 403, now
  writes one line naming its bounded kind (`pw dev server: session
  admission refused: <kind>`), which no session, reader or source error
  reaches: a failed read and a superseded admission are told apart.
- **What still holds.** The admission is the request's own read, never this
  host's copy. Delivery still inspects the source and refuses a changed or
  revoked reader; a command's PostgreSQL fence (advisory lock, row lock,
  final database-clock check) is unchanged; a failed read refuses; no
  overtaken read puts a revoked principal back in the copy. A request
  answered from an overtaken read was admitted on a read made while it was
  under way, which is all a lone request ever had.
- **Not taken: one admission of a session at a time in this host.** The
  database already orders them; a host-side lock would be held across a
  network read, which this ADR's admission ordering forbids, and would
  queue a page's five or more requests behind each other for no authority
  gained.
- **The exemption.** Its stated reason was a database outage: the page is
  admitted first and is refused in one, so the exemption served only a page
  loaded just before an outage, whose handler manifest and modules were
  still refused (the section above says so). Its occasion, run
  41202348600's 503 on `pw-resume.wasm` 38 ms after the runtime was
  answered, is what this repair answers: a page asks for its runtime, its
  WebAssembly, its handler manifest and its stream at once. Every request is
  admitted again; `every_request_is_admitted_and_a_failed_read_refuses_it`
  holds that, the runtime's files, the next document's runtime with its
  query, a page, the manifest, a handler module and a command each refused
  on a failed read, each refusal logged by its kind.
- **Evidence** (2026-11-02, this PC: PostgreSQL 18.6, Playwright 1.58.0).
  - The tests above, at 613f54bd: `identity::` and `tests::session_overlap::`
    27 passed (the PostgreSQL test included); the whole `pw-dev-server`
    suite at 915a53fa, 856 passed, 9 ignored, 0 failed.
  - `scripts/session_overlap_mutations.py`, run whole at 613f54bd: 12 of 12
    controls killed, each by a complete native assertion failure of its
    frozen 13-test inventory, the source restored (valid=True). Each rule
    of the repair is a control: an overtaken admission refused again, an
    overtaken read answered stale or refused instead of read again, the
    reads unbounded, a close that moves no authority ticket, an overtaken
    read installed, an install, a removal or a failed read's removal that
    moves no ticket, PR #63's exemption back, a page's or a command's
    refusal unlogged. It refuses to run without a database, so the
    PostgreSQL test never passes by skipping; its producer contract has
    eight unit tests (`scripts/tests/test_session_overlap_mutations.py`).
  - In three engines (`e2e-hosts/second-host.spec.mjs`, "pages loading
    their runtime and modules while commands run are each answered"): two
    pages of one session, each round loading one again while a command
    runs and pressing on it while the other loads again, every request
    answered and no admission refused in the host's log: 3 passed. With
    the first control applied by hand (an overtaken admission refused
    again), all three failed at once by name: Chromium and Firefox a
    command 403 `session-changed`, WebKit a 503 on `pw-render.wasm`.
  - `just e14-session-overlap` records the tests, the browser test and the
    controls (`docs/evidence/E14/session-overlap.txt`); `e14-second-host`,
    whose spec and controls this changes, is run again whole.
- **Found by the browser test, not changed: past six open pages of one
  origin, Firefox queues every other request.** The first form of the test
  opened two new pages a round and kept them; at the seventh page Firefox
  (`network.http.max-persistent-connections-per-server`, 6) had no
  connection left beside the pages' streaming `/stream` answers: a press's
  command was aborted at its 10 s deadline and a handler module was never
  sent, twice in two runs; Chromium and WebKit passed. Each open page
  holds one HTTP/1.1 connection, so a reader with seven tabs of one origin
  in Firefox would see the same. HTTP/2 between the browser and the
  deployment would carry every page's requests on one connection; the
  development server speaks HTTP/1.1. A limitation for the integrator to
  record; the test now keeps two pages open, as a reader with two tabs.
- **Not claimed, and next.**
  - Admitting the build's own files costs each of them one admission (on
    PostgreSQL one five-statement transaction holding the session's
    advisory lock). A rule that a response reading no session state is
    answered before admission (every build file: the runtime, the handler
    manifest, handler, speculation and computed modules, stylesheets,
    images) would be one structural decision, measured, not three names.
  - A command whose admission read fails is answered 403 `session-changed`,
    as before; 503 would say what happened. Unchanged here.

## The overlap repair's review: an admission takes no session lock, 2026-11-02 (track session-overlap)

An independent review of the section above (at 81026f05) found two rules
it states that no test held, a defect it gave as a reason, and a filter that
counted a test it does not own. The integrator ruled on each; this section
records the findings and what changed.

- **Corrections first.**
  - **"The database already took one session's admissions one at a time"**
    (the section above, twice: its corrections and "Not taken") was so, and
    it was a defect, not a safeguard. A browser command holds its session's
    advisory lock while it runs (`hosts::command_session_lock`, a
    session-level lock, held from `authorize` to its staging's end, a
    payment's outbound call of up to 10 s included). Every admission of that
    session was a five-statement transaction that took the same lock
    (`pg_advisory_xact_lock`), so it waited up to the 5 s `lock_timeout`,
    holding one of the host's 8 session connections (`SESSION_CONNECTIONS`)
    meanwhile. The review reproduced it: eight of one session's runtime-file
    requests all waiting on the lock, one answered 503 at 5001 ms, and
    another reader's page taking 4969 ms where it took 18 ms alone. Removing
    PR #63's exemption added the runtime's three files to the requests so
    exposed; pages, `/pw-handlers` and handler modules were exposed before.
    Admissions are no longer ordered by the database, and nothing relies on
    their being ordered (below).
  - **Two of the repair's rules had no test.** "A removal (no one, or a
    failed read) still removes, failing closed" and "the tickets are compared
    under the gate and the sessions lock": the review's mutants M1 and M2 (a
    removal made only where nothing was written meanwhile) and M3 (the
    tickets compared right after the read, before the locks) each survived
    all 13 tests of the frozen inventory. `overtaken_by`'s overtaken read
    always found someone, and no test let a close land between a read's
    return and its locks.
  - **The recipe counted a test it does not own.** `just e14-session-overlap`
    filtered `identity::`, which also selects
    `tests::receiver_identity::...` (main.rs); the "27 passed" above counted
    it. The filter is now `identity::tests:: identity::invocation_tests::
    tests::session_overlap::`, exactly identity.rs's tests and this track's.
  - "On PostgreSQL one five-statement transaction holding the session's
    advisory lock" (the section above's "Not claimed") is now one statement
    that takes no lock, where no write is due.
- **An admission on PostgreSQL** (the integrator's ruling 3, for every
  admitted route; `SessionsPg::session`). **One statement, run in no
  transaction of its own, that takes no session lock** (`ADMISSION_READ`):
  the session's row, whether it is live by both lifetimes on the database's
  clock, and whether its use is due to be recorded (live, and recorded more
  than `touched_after` ago, at most a minute). A write is due where its use
  is, or where the row is present and no longer live (its deletion).
  - No row: no one. No write due: answered from the read, one statement in
    all, so a page is again two statements (its admission and its delivery
    inspection), as the 2026-10-26 section above counted them before the
    command fence made the admission a transaction.
  - A write due is tried, never waited for (`SessionsPg::written_if_free`):
    a transaction of its own takes `pg_try_advisory_xact_lock`. **Taken**,
    the combined statement the admission always ran (`ADMISSION_WRITE`: its
    use recorded, an expired row deleted) runs under it, its READ COMMITTED
    snapshot taken after the lock, and its answer is the admission's: a
    session closed since the read is no one. **Not taken** (a command, an
    open or a close holds the lock), nothing is written and the read
    answers: an expired session is no one all the same, and its row is
    deleted by a later admission; a use left unrecorded is recorded by a
    later one, which only delays idle expiry, by at most `touched_after`.
  - Why the write waits for nothing yet is made only under the lock:
    PostgreSQL's READ COMMITTED "SELECT query sees a snapshot of the
    database as of the instant the query begins to run", and a deletion
    decided on a snapshot taken before the lock could remove a row an open
    rewrote meanwhile. Open, close and every command take the same lock
    ("session-level and transaction-level lock requests for the same
    advisory lock identifier will block each other",
    [PostgreSQL 18, 13.3.5](https://www.postgresql.org/docs/18/explicit-locking.html#ADVISORY-LOCKS);
    `pg_try_advisory_xact_lock` "will either obtain the lock immediately and
    return true, or return false without waiting",
    [9.28.10](https://www.postgresql.org/docs/18/functions-admin.html#FUNCTIONS-ADVISORY-LOCKS),
    both checked 2026-11-02), so nothing of the session changes under the
    write. A command holds the row's `FOR UPDATE` lock only while it holds
    the advisory lock, so the write waits on no row lock either.
  - The read and the write share their two conditions, live and due to be
    recorded, as one text each (`live!` and `stale!` in hosts.rs), so they
    cannot disagree. `scripts/second_host_mutations.py`'s five expiry
    controls (idle, lifetime, use not recorded, use written at every
    request, expired row kept) are anchored on those texts now, and each
    changes both statements.
  - **Every lock wait stays bounded.** The read runs in no transaction, so
    `SET LOCAL lock_timeout` no longer bounds it, and a migration's lock on
    the table would hold it as long as the migration ran. Each session
    connection now sets `lock_timeout = '5s'` when it is made
    (`SESSION_CONNECTION_LOCK_TIMEOUT`; "the limit applies both to explicit
    locking requests ... and to implicitly-acquired locks",
    [19.11.1](https://www.postgresql.org/docs/18/runtime-config-client.html#GUC-LOCK-TIMEOUT)):
    the read fails at 5 s and its request is refused, as when the admission
    was a transaction. Inspections and sign-ins on those connections are
    bounded the same way; the transactions keep their own `SET LOCAL`.
  - The lock order is unchanged: this host's gate, sessions lock and ticket
    as before; on PostgreSQL the advisory lock before any row lock, the one
    order every command, open and close keeps. A failed read, a failed try
    or a failed write refuses the request, and discards the connection
    where a transaction's end is uncertain, as before.
- **Authority does not rest on the admission lock.** Delivery inspects the
  source again before private bytes leave; a command's commit is fenced
  (advisory lock, the row `FOR UPDATE`, the final database-clock check on
  its own transaction); an upload's publication checks again. An admission
  read without the lock may be a few milliseconds behind a close on
  another host, as one made just before that close always could be; such a
  request delivers nothing private and commits nothing. What relied on the
  ordering: the section above's claim, and its PostgreSQL overlap test,
  which held the session's lock until both admissions waited on it. The
  test now makes the two reads meet after they read (a test's step in
  `SessionsPg::session`, `Kept::after_each_read`), as the in-process test's
  source does. `session_commands_pg`'s `admitted_first`, which watches a
  logout wait for a command's commit, now sees the logout's close wait, not
  its admission; it passes unchanged.
- **The review's findings 1 and 2, held.** In identity.rs, adopted from the
  review: `a_read_of_no_one_another_admission_overtook_still_removes_the_copy`
  and `a_failed_read_another_admission_overtook_still_removes_the_copy`
  (the overtaken read is the removal, the overtaking one installs the
  reader; the copy holds no principal afterwards), and
  `a_close_between_the_read_and_the_locks_is_seen` (the test holds the
  sessions lock, waits until the admission holds the gate, then closes).
  M1, M2 and M3 are controls of `scripts/session_overlap_mutations.py`.
- **The new rules' tests** (tests/session_overlap.rs, on PostgreSQL, the
  test holding the session's lock as a command does, through
  `command_session_lock` on a connection of its own): a live session's
  admission answered as the reader, by one statement, within 2 s (the lock
  timeout is 5 s); a due touch skipped and answered without waiting, the use
  unrecorded while the lock is held and recorded once it is free; a session
  past its lifetime no one without waiting, its row kept while the lock is
  held and deleted once it is free; a session another host signs out
  between the read and the write no one (the write's answer); the review's
  scenario, eight of Ada's runtime files under her command's lock and then
  Bob's page, each answered 200 within 2 s; and an admission held by a
  locked sessions table refused at the lock timeout. Each rule is a control,
  killed at an assertion on the answer and the measured time: the read
  waiting for the lock; a write due at every admission; the due write
  waiting for the lock; the lock held, a live session refused, an expired
  one answered as its principal, or the write made anyway; a use never
  recorded; an expired row never deleted; the lock free, the read answering
  instead of the write; a connection that bounds no lock wait.
  - **Characterized first**: the four tests that hold the session's lock,
    run against the admission as it was (hosts.rs as at 81026f05, with
    these tests), each failed at its assertion: the admission
    `Err(SourceUnavailable)` after 5.00 s (4 statements run), the due touch
    and the expired session each refused after 5.00 s, and the review's
    scenario reproduced (Ada's eight files 503 at 5.00 to 5.06 s, Bob's page
    5.01 s); a page's statements 6, not 2. The locked-table test passed
    there: the transaction's `SET LOCAL` bounded that wait, which the
    connection's setting now keeps. The test of a session signed out
    between the read and the write has no counterpart there: the read and
    the write were one statement.
- **Found, not changed: the sign-in callback decides `was_guest` from this
  host's copy** (identity.rs `callback`: `self.principals.of(session)`;
  main.rs answers the identity routes with no reader bound), not from the
  request's admitted reader. The two differ when an admission of the same
  session that read no one (a sign-out on another host, or an expiry)
  empties the copy after the callback's own admission read the reader,
  during the provider's exchange or as an overlapping admission. Then
  `was_guest` is true for a session that was signed in, and where a store
  keys its carts by session (`cart_by_reader` false), `joined` moves that
  reader's cart to the next account: "what it held is its user's, never the
  next one's", the callback's own comment, fails. A hypothesis from
  reading; not reproduced. The fix, the admitted reader passed to the
  callback, changes the store-accounts seam's input and needs its own
  regression (a session-keyed store, a provider, and an admission landing
  inside the callback), so it is not taken here.
- **Evidence** (2026-11-02, this PC: PostgreSQL 18.6, Playwright 1.58.0).
  Pending; recorded below when run.
- **Not claimed, and next.**
  - The section above's first "next" stands: a response that reads no
    session state could be answered before admission. Admission now costs
    such a response one statement without a lock, which makes that rule a
    measurement, not a repair.
  - Local runs of the whole server suite need at most 8 test threads here:
    this PC's PostgreSQL allows 100 connections, and 32 threads of two-host
    tests reach that before and after this change ("sorry, too many clients
    already"); CI's runners have fewer cores.
