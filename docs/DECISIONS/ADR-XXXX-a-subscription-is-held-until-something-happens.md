# ADR-XXXX: a subscription is held until something happens

Status: proposed, 2026-11-06, on branch `perf/longer-holds-20261106`, on top
of the first press (PR #76) with master merged in (`ac3ea31c`). Proposed
under the owner's standing order of 2026-10-25 (decide by best judgement,
without asking) and the owner's approval on 2026-11-06 of "longer-held
subscriptions" as a follow-up with an ADR of its own. Not landed; not
numbered (the integrator numbers ADRs). Milestone: E14.

## Context

A live page subscribes to its document's changes: by a stream (newline
JSON, wherever `ReadableStream` exists, which is every engine the project
runs) or by a long poll (its fallback). Until now a stream was held two
seconds and a long poll one, then answered with nothing, and the page asked
again at once.

- **An idle page cost the host about 30 requests a minute**, 60 by long
  poll. The first press's ADR lists it among what Pleris still loses: the
  store's live page asked 29.7 times a minute where a Next.js or SvelteKit
  page asks nothing (they have no live updates).
- **The short holds no longer buy anything.** They were set when a held
  subscription looked at its subscriber every 25 ms (eighty passes of a
  stream, forty of a poll). Since the first press a held subscription waits
  on the subscriber table and is woken by a push: a change is answered at
  once, however long the hold.
- **The short cycle seemed to carry two other jobs.** Read in the code,
  neither needs it:
  - **Presence** (ADR-0297): a commit's telling derives a document only if
    its page asked within `LIVE`, 3 s. Each pass of a held subscription
    renews its document's `seen`, and a pass runs on every wake and at
    least every `WAKE_AT_LEAST` (250 ms), so a page whose subscription is
    held is live throughout. A subscriber silent for `IDLE` (120 s) is
    forgotten.
  - **Acknowledgement**: the next request's cursor (`since`) says what the
    page applied, as before.

## Decision

1. **A subscription is held until something happens, up to `HOLD`, 25 s**, a
   stream's and a long poll's alike. A push answers it at once, as before.
   An open page then asks about 2.4 times a minute.
2. **25 s** is under the 30 s a hosting router allows before an answer's
   first byte (Heroku's; a long poll's answer has no byte before it), under
   the 60 s a proxy commonly allows a quiet connection (nginx's
   `proxy_read_timeout`, AWS's load balancers), and longer than `LIVE`,
   which every pass renews.
3. **A page gone ends its hold at the next pass** (`gone`): the browser
   closed the connection the subscription is held on, as a page leaving does
   when its runtime aborts the request. A browser sends nothing else on a
   connection while it waits for that connection's answer, so whatever there
   is to read is its close. Looked at without waiting, once a pass. Without
   it a page gone would hold its thread for up to 25 s instead of 2.
4. **A test's host holds as hosts did** (`TEST_STREAM_HOLD`, 2 s;
   `TEST_POLL_HOLD`, 1 s), as `Server` fields, so a test that waits out a
   hold waits as it did; a test of the hold sets its own.

## What it costs, plainly

- **A held subscription still holds a thread and a connection**, as it did:
  a page asked again at once, so its thread was nearly always held already.
  A page gone now frees its thread within a pass.
- **Each pass sets its socket non-blocking, peeks, and sets it back**: two
  system calls and a peek, at most four times a second.
- **HTTP/1.1's six connections per origin**: each open page holds one, as
  before.

## Held by

- `tests::longer_holds::`
  (`spikes/own-renderer/server/src/tests/longer_holds.rs`), 6 tests:
  - an idle long poll held its whole hold, and not taken for a page gone;
  - a held long poll, and a held stream, keeping their pages live past
    `LIVE`;
  - a long poll's, and a stream's, page gone ending its hold well inside
    `HOLD`;
  - and `HOLD` itself.
- `tests::an_idle_subscription_waits_on_the_table_rather_than_asking_it`
  (the first press's), now against the host's own stream hold.
- `scripts/longer_holds_mutations.py`, 6 controls, recorded by `just
  e14-longer-holds`:
  - a stream, or a long poll, that never looks for its page gone;
  - a quiet connection taken for a page gone;
  - a hold that no longer keeps its page live, for each kind;
  - and a hold of two seconds again.

## Not claimed, and next

- **The head-to-head's idle requests** are not re-measured here.
- **With the keep-alive ADR (#86)** a long poll's connection is kept from
  one hold to the next. The two land apart, and either holds alone.

## Repairs, 2026-11-09

An independent review of this ADR's first version (`docs/NEXT.md`, after
train-14) found three defects, each of the 25 s hold's own, and each repaired
here before it lands. The hold above is now what a page asks for.

1. **A page is held long only where that costs no other request a connection.**
   A browser opens six connections to an HTTP/1.1 origin; six tabs each holding
   one for 25 s would leave a press none. The runtime asks (`hold=long`) where
   the page's connection is HTTP/2 or later, which shares one connection, and on
   HTTP/1.1 one tab of the origin at a time, by a Web Lock its next tab takes when
   it closes. Otherwise a page is held as every page was, two seconds a stream
   and one a poll, and its idle cost is as it was.
2. **A regeneration that failed is tried again within the hold** (ADR-0176). Its
   only retry was the page's next request, which a long hold puts up to 25 s
   away. A held subscription now tries as often as a page held briefly would
   have asked.
3. **A held stream ends before its queue fills.** A frame is forgotten when its
   page acknowledges it, which is its next request; 25 s of a busy page's frames
   could pass `MAX_WAITING` and tell a page that was keeping up to reload. A
   stream that has half that waiting ends, and its page asks again at once.
