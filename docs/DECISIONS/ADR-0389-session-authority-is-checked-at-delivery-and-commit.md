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
