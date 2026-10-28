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
