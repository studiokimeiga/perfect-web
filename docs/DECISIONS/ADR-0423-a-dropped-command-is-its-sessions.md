# ADR-0423: a dropped command is its session's

Status: accepted 2026-11-10, landed with trains 18 and 19. Proposed 2026-11-10,
on branch `perf/train-18-20261110`, under the owner's delegation of 2026-10-02
and order of 2026-10-25 ("deciding by best judgement without asking").
Milestone: E14. Amends ADR-0175's dropped command, as ADR-0401 kept connections
for it.

## Context

ADR-0175's test faults are each a session's own: "Each fault is the
session's own, so the suite shares its host" (`e2e/connections.spec.mjs`).
A dropped command (`/bench/drop`) has to reach its page, and a browser sends
a request again by itself where a kept connection it reused closes before
any answer. So since ADR-0401 arming a drop closed every kept connection
waiting for its next request, and until the drop was taken no answer kept
its connection, and one listed after the arming was closed once listed.

None of the three looked at whose connection it was. Every session's kept
connections were closed under browsers that had been told, by the answer
before, that they were kept, and every session's answers closed theirs while
one test's drop was armed. A browser reusing a connection the host closes is
a race HTTP/1.1 leaves to the client (RFC 9112 §9.3.1). A host otherwise
keeps it out of the way by saying how long it keeps an idle connection
(`keep-alive: timeout=75`), so that a client stops reusing it first; a drop
closed them at once.

Verify runs 42772395298 and 42785265911 failed the cut-off test in WebKit,
right beside the two dropped-command tests. In the first the page's long poll
failed with "WebKit encountered an internal error", its network process gone;
in the second the long poll sent as the cut ended was never answered, and the
page sent nothing more. Neither trace says which connection carried it. A
host closing a connection under a browser reusing it is one way to come to
either, and the one the host can remove.

## Decision

1. **A drop is armed for its session** (`dropping_a_command_for`): only an
   answer to that session closes its connection while it is armed.
2. **A kept connection is listed with the session its last answer was for**
   (`Idle`), as a browser's connection carries its context's cookie, and
   arming a drop closes that session's alone.
3. **A connection listed after a drop was armed closes** only where its
   session's drop is armed.

## Why not the other ways

- **A host per fault.** The suite shares the host so that its tests run
  side by side as pages do; a fault that is its session's needs no host of
  its own.
- **Leave the drop global and retry the cut-off test.** The suite retries
  nothing (`playwright.config.mjs`): a test that fails once is a finding.

## Acceptance

- `tests::keep_alive::a_drop_one_session_arms_leaves_another_sessions_connections_kept`:
  another session's kept connection is not closed by the arming, its next
  answer keeps it, and it is still there once listed again.
- `tests::keep_alive::while_a_dropped_command_is_armed_no_answer_keeps_its_connection`,
  armed now for the session that asks.
- **Mutation controls:** `scripts/keep_alive_mutations.py`, four new: a drop
  that closes every session's kept connections, one armed for every
  session, a connection listed for no session, a request answered for none;
  the four on the drop re-anchored.
- The cut-off test, with the runtime's log and the server's trail for its
  document attached when it fails, on the verify runs that follow.

## Consequences

- Whether this was the WebKit failures' cause is said by those runs, not
  here. If the cut-off test fails again, its records say where the change
  it waited for went.
