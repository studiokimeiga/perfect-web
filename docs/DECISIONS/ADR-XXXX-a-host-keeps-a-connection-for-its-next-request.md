# ADR-XXXX: a host keeps a connection for its next request

Status: proposed, 2026-11-06, on branch `perf/keep-alive-20261106`, on top of
the first press (PR #76) with master merged in (`ac3ea31c`). Proposed under
the owner's standing order of 2026-10-25 (decide by best judgement, without
asking) and the owner's approval on 2026-11-06 of "keep-alive in the host" as
a follow-up of its own. Not landed; not numbered (the integrator numbers
ADRs). Milestone: E14.

## Context

Until now the host (`pw-dev-server`) answered one request on each connection
and closed it: every answer said `connection: close`, and each connection
had a thread of its own for that one request.

- **Under load it lost to SvelteKit** in the head-to-head (ADR-0392's
  measurements; the first press's ADR, "Next" 3). The harness's load
  generator (`benchmarks/headtohead/load.mjs`) keeps its connections, as a
  browser and Node's own `http.Agent` do. Against this host each of its
  requests paid for a new connection, while Node's servers keep connections
  by default (`keepAliveTimeout` 5 s).
- **A browser paid for it on every page**: the runtime, its WebAssembly, the
  handler table, each module, each command and each poll of a live page's
  subscription (about 30 a minute) opened a connection of its own.

HTTP/1.1 keeps a connection unless an answer says otherwise (RFC 9112 §9.3).
Keeping one has known costs, each a decision below: a kept connection holds
its thread; a body framed loosely lets a request be read out of another's
body (request smuggling); and an answer that ends with its connection's close
cannot share it.

## Decision

1. **A connection is kept for its next request** where HTTP/1.1 keeps it:
   the request is HTTP/1.1, does not ask to close (`connection: close`),
   its body was read whole, and its answer was written whole with a known
   length. The answer says `connection: keep-alive` and `keep-alive:
   timeout=75`. `handle` answers a connection's requests one after another,
   on one reader, so a request sent behind another (pipelined) is answered
   in its turn.
2. **Only an answer of a known length keeps its connection.** Every answer a
   host writes whole goes through `write_response`, which writes its length.
   Any other closes the connection, whatever its request allowed: a stream,
   whose body ends with the connection (a settling document; a
   subscription's NDJSON adapter); a fault (an answer left unfinished, a
   connection held); an upload's own answer; and a request answered with
   nothing, as a subscription cut off is (ADR-0175), whose close is its
   answer. A connection not kept is closed before the commit's telling, as
   one that held one request was, so a stream's end is not delayed.
3. **A request's body is framed strictly** (`framed_length`; RFC 9112
   §5.1, §5.2, §6.3): by one `content-length` of digits, or by none.
   Refused, and the connection closed: a `transfer-encoding` (501: this host
   reads no chunked body), two lengths, a length that is no number, a header
   folded onto the line before, and whitespace between a header's name and
   its colon (each 400). An answer written before its request's body is read
   whole (a body over a command's bound, 413; an admission refused) closes
   the connection, so what was sent of the body is never read as a request.
4. **A kept connection holds its thread for a bounded time**: it waits at
   most 75 s (`KEEP_ALIVE_IDLE`, nginx's `keepalive_timeout`) for its next
   request's first byte, and a request's head and body may take at most
   10 s once begun (`REQUEST_READ_AT_MOST`, one deadline for the request),
   the first request's too. Past 256 connections open at once
   (`KEPT_AT_MOST`), an answer closes its connection. Amended by the review
   below: it was 5 s, a wait for each read, and 512.
5. **A commit is told after its answer**, not after its connection closes
   (ADR-0219): `handle` tells after each request, so a kept connection never
   holds back what its commit dropped for every other reader.
6. **The type holds it.** Every writer of an answer takes a `Conn`, the
   connection and what is decided of it. A request owns a twin of its
   connection (`Conn::twin`: the same socket, its decisions shared), so the
   handlers own their connection as they did. A writer that forgets the
   framing does not compile.

## What it costs, plainly

- **A request's twin is a `dup` of its socket**, one system call and one
  close a request.
- **A test host waits 1 ms for a kept connection's next request**
  (`TEST_KEEP_ALIVE_IDLE`): a test's client reads one answer to its end, so
  the host closes at once. Every test's answer goes through the same
  keep-alive path a host's does; a test of keep-alive sets the wait itself.
- **A request with a chunked body is refused.** No browser sends one for a
  form or a `fetch` with a body it knows; the host never read one.
- **HTTP/2** is not this host's: browsers speak it only over TLS, which a
  deployment's front proxy ends (ADR-0389's note on Firefox's six
  connections per origin). Keep-alive is HTTP/1.1's half of the same saving.

## Held by

- `tests::keep_alive::` (`spikes/own-renderer/server/src/tests/keep_alive.rs`),
  17 tests over real connections: a connection kept for its next request;
  a command's body read whole and its connection kept; requests sent
  together answered in turn; `connection: close` and HTTP/1.0 closed; a kept
  connection closed after its idle wait; `transfer-encoding`, two lengths, a
  length that is no number, a folded header and whitespace before a colon
  each refused and closed (a request hidden in a chunked body never
  answered); an answer before its body is read closing; the cap; a stream's
  connection closed with it; a request answered with nothing closing; a
  commit told while its connection is kept; and the `keep-alive` header
  saying the idle wait. And `tests::uploads::an_uploads_connection_is_closed_after_its_answer`.
- `scripts/keep_alive_mutations.py`: a control for each rule above but the
  close before the telling (below), each killed by a complete native
  assertion failure of the frozen 18-test inventory; `just e14-keep-alive`
  records the tests and the controls (`docs/evidence/E14/keep-alive.txt`).
- Four controls of other tracks moved with the lines they anchor, each
  undoing the same rule as before: `cross_session` (a connection tells no
  one), `not_found` (every response says OK), `page_policy` (an answer's
  type may be sniffed) and `upload_authority` (the upload dispatch's
  reader). Each is run whole.

## Not claimed, and next

- **The close before the telling** (decision 2) has no control: no test can
  hold the telling back to see a stream end first. It keeps the order the
  host had.
- **The head-to-head's numbers under load** are not re-measured here.
- **A subscription's poll** now reuses its page's connection, but it is
  still asked every 2 s; holding it longer is its own decision (the
  first press's ADR, "Next" 2).
- **HTTP/2 between a browser and a deployment**, through a front proxy.

## Review, 2026-11-07, and its repairs

An independent review held this decision back from train-14 (PR #93). What
it found, and what is repaired, each with a test that fails without the
repair and a mutation control in `keep_alive_mutations.py`:

1. **Request smuggling (blocker).** The head was read line by line, and a
   read that failed was taken for the head's end. A header that is no UTF-8,
   or a stall, ended the head where it stood; its headers after it, the
   `content-length` among them, were never read, and the rest was read as
   the next request on a kept connection. Behind a proxy that reuses its
   connections, a request could be smuggled through it. Now `read_head`
   reads the head as bytes, line by line:
   - a line that is no text is refused (400);
   - a head past 32 KiB (`HEAD_AT_MOST`) is refused (431);
   - a head not sent by its deadline is refused (408);
   - each refusal closes the connection;
   - a client gone mid-head is answered nothing.
2. **One deadline, not a wait a read.** A client sending a byte every few
   seconds held a thread for ever, and no test held the 10 s. Now the
   request's head and body share one deadline, which a test can set
   (`request_read`), and the body is read within it (`read_body`).
3. **File handles.** Each open connection holds three: 512 held 1536, past
   the 1024 a process is usually allowed, and past about 340 connections
   `accept` failed and its loop asked again at once, spinning a core. Now
   the cap is 256 (768 handles), and a failed `accept` waits 10 ms, doubling
   to a second (`accept_backoff`). A host given more handles by its service
   manager keeps no more connections; the cap is the host's.
4. **The next request waited on the telling.** A kept connection told every
   other reader before it read its next request, which a browser sends on
   the connection it just used: an author's next command waited on readers,
   which ADR-0219 rules out. Now the telling runs on a thread of the
   connection's scope, beside its next request.
5. **HEAD.** A HEAD fell to the 405, which wrote a six-byte body on a kept
   connection, and a client reads no body after a HEAD's answer (RFC 9112
   §6.3). Now a HEAD is answered as its GET, with no body (`write_response`
   writes the head alone), and its connection closes after it, so nothing a
   streamed answer writes can follow it.
6. **Below a proxy's idle wait.** At 5 s the host closed a connection a
   front proxy keeps for 60 s (nginx's upstream `keepalive_timeout`, AWS's
   load balancers), and the proxy's next request on it is answered 502.
   Now the host waits 75 s, past every such proxy's.
7. **A second answer to one request** closes its connection, said in its
   head; the first said it was kept. No route writes two; the rule makes one
   that would harmless.

The review's other finding stands as written in decision 6: `Conn`'s
`DerefMut` lets the identity's and the uploads' routes write an answer
directly. Each closes its connection, so none can leave a kept connection
mid-answer.

Held by the 26 tests of `tests::keep_alive::`, 8 of them new, and by
`keep_alive_mutations.py` and `cross_session_mutations.py` run whole after
the repairs (`docs/evidence/E14/keep-alive-review-2026-11-07/`).
