# ADR-XXXX: a program's declared tables served durably from PostgreSQL, one transaction a command

Status: proposed by track `durable-tables` under the owner's delegation of
2026-10-02 and the standing order of 2026-10-25 (decide by best judgement);
the integrator numbers it (ADR-0397 reserved) and lands it after an
independent review. Date: 2026-11-02; revised the same day after the
independent review of `3ccd083a` and the integrator's rulings on it (its
findings 1 to 6 repaired here, 7 left to the tracks that own it: see
"Corrections" and "Depends on"). Milestone: E14, the app layer.
ADR-0338's D3 joined to D2's routing: track `data-path`'s server (its ADR,
ADR-0394 reserved, `an-app-with-no-rust-data-layer`) serves a program's
declared tables through the gate from memory; ADR-0393's adapter
(`runtime/pw-table-postgres`) holds them on PostgreSQL; this joins the two,
so that the acceptance criterion (a complete, durable, authenticated,
privacy-preserving web app written in `.pw` with no handwritten app-specific
Rust) holds of a running host and not only of a crate. Builds on ADR-0338
(decisions 1.3, 5.1, 5.2, 5.5, D3), ADR-0393, ADR-0394 (data-path),
ADR-0208, ADR-0246, ADR-0325, ADR-0334, ADR-0354 and ADR-0369.

## Context

- **Two pieces, not joined.** `data-path` serves the guide's guestbook with
  no Rust of its own, in three browsers, its data in memory and lost with
  the process. D3's `PgTables` keeps any program's tables on PostgreSQL,
  with `publish_with(Records)` committing the interaction's record and the
  outbox in the command's transaction; ADR-0393's "Not claimed" and "Next"
  name exactly what was missing: the server's `DataLayer` over it.
- **What a durable host needs that neither had**:
  - the **runs a command schedules** (ADR-0334) in the same transaction:
    on a layer-owned commit (`Commit::Applied`) the host writes the runs
    nowhere unless the layer keeps them, and ADR-0393 did not keep them;
    ADR-0338 1.3 and 5.2 count them among the platform's tables every
    adapter keeps;
  - the **reader's session**, kept where a restarted host finds it: kept in
    the host's memory, a restart signs every reader out, and a retried
    interaction is no longer the same reader's (its record's `sent` names
    the reader's generation, ADR-0389), so it is refused as "first sent with
    other arguments" instead of being answered once;
  - for sessions kept in the database, **each command fenced on its
    session** in its own transaction (ADR-0369's, as `feed_pg.rs` and
    `store_pg.rs` fence theirs): `Staged::authorize`'s default refuses a
    persistent session's actor.
- **The review of `data-path` (2026-11-02)**, relayed by the integrator:
  `publish_within` is memory-only (wired to PostgreSQL, the record would sit
  in the host's materializer beside durable rows, and a retry after a
  restart would run again); scheduled runs would be lost silently on the
  layer-owned path; D3's render seat must be shared into the read functions
  a render links; the suite's refused-commit part uses a memory-only hook,
  which `data-path`'s worker makes adapter-neutral.

## Decision

1. **The deployment chooses where a program's tables are kept**: a
   database where it names one, `PW_TABLES_DATABASE_URL` (the name D3's
   tests and recipe already read), in memory otherwise, as before; the
   schema its tables are in is the connection's own, or a test's
   (`PW_TABLES_SCHEMA`, honoured only for a development deployment on this
   machine and refused at start for any other, as `PW_FEED_SCHEMA` is). The
   host says at its start where they are (`pw dev server tables: on
   PostgreSQL, schema …` or `in memory, lost when the host stops`).
2. **One layer, `TablesPg`** (`spikes/own-renderer/server/src/tables_pg.rs`),
   a `DataLayer` over `PgTables`, behind the same gate as the memory layer:
   no operation by name; a query links the reads the gate admits for its
   contract and grant, a command its reads and writes staged in its
   transaction; the build's `tables.json` applied or held to (ADR-0393's
   migration, drift refused); a program importing a hand-written layer's
   operation beside its tables refused at start, as in memory. No table,
   contract or route of any program is named in it: the guestbook and the
   lists app are served alike. `Server::from_build_all` takes the database,
   `from_build_layers` and `main` are unchanged for every other program.
3. **A command is one PostgreSQL transaction, with every record of the
   platform's** (`Staged::commit` answers `Commit::Applied`;
   `keeps_interactions`, `recalled` and `runs` are the layer's, so the
   host's materializer keeps nothing of it): the run it completes taken
   first, its row locked and held to the generation selected, then deleted
   (not the run selected any more, nothing commits: `Commit::NotCurrent`;
   the run selected and its deletion suppressed by another hand, a refusal,
   retried after its backoff, never taken for a run done), so that a run
   that schedules itself again is taken before its next generation replaces
   it under its key; then the runs it schedules or cancels written, its
   references held (`prepare`), its interaction's record and its outbox
   written (the outbox under its lock, in commit order), its reader's session
   held to its fence, and `COMMIT`.
   - **What the host is answered is what the database did.** A commit the
     database rolled back (a serialization failure or deadlock, 40001 or
     40P01; a refusal; a statement that failed), or that never reached its
     `COMMIT`, keeps nothing, and the layer says so
     (`Staged::definitely_refused`, false by default for every other layer):
     the host answers it not committed, at once, with nothing to reconcile.
     A `COMMIT` sent and never answered (`Error::OutcomeUnknown`), and an
     interaction another commit recorded first (`Error::Recorded`, which
     ADR-0393 answered as a plain refusal), are reconciled by the host from
     the interaction's record, as for every PostgreSQL layer: committed where
     the record says so, unknown where there is none, never refused.
   - **Why the adapter changed** (the structural change the brief
     foresaw): ADR-0393's command keeps its connection to itself, and every
     statement of its transaction must be its own. A statement of another
     hand that failed and whose failure was swallowed would leave the
     transaction aborted, and PostgreSQL answers the `COMMIT` of an aborted
     transaction `ROLLBACK` with no error: the adapter would answer committed.
     So the adapter keeps the platform's remaining tables itself (ADR-0338
     5.2), in a second platform migration, `0002_runs_and_sessions`
     (`pw_runs`, `pw_sessions`, `pw_sign_ins`, reserved beside the program's
     names), with typed operations, each failure its own:
     `Command::schedule` and `Command::consume` (statements of the command),
     `PgTables::begin_fenced` and `Command::revalidate`, and on `PgTables`
     the runs outside a command (`due_runs`, `next_run`, `run_current`,
     `retry_run`, `runs`), the sessions and sign-ins (`admit_session`,
     `inspect_session`, `open_session`, `close_session`, `begin_sign_in`,
     `take_sign_in`, `sweep_sessions`, `sweep_sign_ins`) and the outbox's
     forgetting (`forget_outbox`, `trim_outbox`), all in
     `runtime/pw-table-postgres/src/platform.rs`. `PgTables::open_json`
     takes tables.json's text, so the host links no compiler for it.
   - **A command takes its connection's place before the host's turn**
     (`PgTables::begin`, the review's finding 2): ADR-0393's command took
     its place at its first statement, inside the turn; with fenced commands
     holding places while they wait for the turn (decision 5), a command
     holding the turn could wait out the pool's thirty seconds for a place
     the commands waiting for it held. The place is cheap (the connection is
     still opened, or taken from the idle ones, at the first statement), and
     a command holding the turn now waits for nothing of the pool's.
4. **One state a render** (NEXT 4c, ADR-0354): a render's snapshot is one
   of the adapter's seats, and every read function its queries link reads
   in it (`Snapshot::read_host_fns` shares the seat). A seat not taken
   within its wait fails the render's reads, saying so; it never reads the
   latest commit instead. On PostgreSQL the state is the render's first
   table read's (ADR-0393's decision 8, PostgreSQL 18 13.2.2), not its
   start's, as in memory. **A read that fails ends the render's reads**
   (the review's finding 5, in D3's `SnapshotState::read`): its transaction
   ended with it, and every later read of the render fails, saying which
   read failed first; ADR-0393's snapshot opened another transaction at the
   next read, which read a later state.
5. **The reader's session is the database's** (ADR-0369): kept by its id's
   SHA-256 with its principal and generation, alive by its two lifetimes on
   the database's clock, its use recorded at most once a minute, one past a
   lifetime forgotten by the admission that finds it and by the host's
   sweeps (decision 6), a sign-in under way taken once and deleted as it is
   taken; each change a read-committed transaction under the session's
   lock, on connections of their own (at most eight), each connection
   marked not to be kept before its transaction begins and kept only where
   it committed (as `SessionsPg::mutation` keeps its own). **A command is
   fenced on it** as the other PostgreSQL layers fence theirs, from its
   first statement or never (`PgTables::begin_fenced`; there is no fencing
   a command that ran): the session's lock taken first, on the connection
   the command then runs on, **outside the host's turn**, so that a command
   waiting for its session (up to its five seconds) holds no other session's
   command up (the review's finding 2; a session's own commands are one at a
   time on a host already); then the turn; then the command's transaction
   begun (its snapshot after both waits), the session's row locked in it,
   and the session held to the one the command was admitted as, there, by
   `revalidate` (a command writing nothing or answering a declared error)
   and just before `COMMIT`. The lock is given back once the command ends
   (or the connection closed, which gives it back). A session's change waits
   for a command fenced on it, on any host, and a command fenced after a
   sign-out is refused, keeping nothing. The server's staging takes its
   transaction where the command first needs it, so the fence comes first;
   a staging asked for its authority after its transaction began is refused
   it and is unusable from then on. A guest is the host's own, as in memory.
6. **What the database keeps of the platform's stays bounded** (the
   review's findings 1 and 4):
   - **the outbox**: no host reads this layer's outbox after its commit (it
     has no `tail()`: see "Not claimed"), so each commit's rows are
     forgotten by the committing host once its commit has answered
     (`forget_outbox`, one statement after the `COMMIT`, outside the turn);
     rows a host that ended between the two left behind are forgotten once
     a minute old (`trim_outbox`). ADR-0393's outbox kept every row for good.
   - **sessions and sign-ins**: past their lifetimes, swept by the host,
     **on a timer, in bounded batches**, not at each sign-in: the host's
     scheduler calls `DataLayer::housekeeping` on each of its passes (at
     most a second apart), and this layer sweeps at most once a minute on
     the host's clock, once a second while a batch comes back full; each
     sweep is one statement of at most 500 rows of a kind, those no
     transaction holds (`FOR UPDATE SKIP LOCKED`), so it waits for nothing
     and a session a command holds is the next sweep's. Chosen over a sweep
     at each sign-in: sign-ins come in bursts and not at all for days, and a
     sweep there would put a variable cost on a reader's sign-in and leave
     the expired sessions of a quiet deployment kept for good; the
     scheduler runs whether readers come or not. A sign-in's nonce and PKCE
     verifier are deleted as it is taken, and by the sweep ten minutes after
     it began (`identity::SIGN_IN_LIFETIME`), as before by the next sign-in.
7. **The conformance suite over this layer, as written**: `conformance.rs`'s
   suite (answers and costs at 3 and 24 rows, staging, refused commits)
   instantiated over PostgreSQL for the guestbook's tables and the lists
   app's, its oracle the same (111 and 164 reads checked, as in memory), and
   its idempotency and query pins on this layer. The suite's refused-commit
   hook (`tables::FAIL_NEXT_PUBLISH`) is honoured by this layer as a refusal
   before its `COMMIT`, until `data-path`'s adapter-neutral refusal lands;
   a refusal the database itself makes at `COMMIT` (a deferred trigger of
   another hand's) is tested apart, keeping none of the rows, the record,
   the event or the run.
8. **The guestbook in three engines on PostgreSQL**:
   `playwright.tables.config.mjs` runs the main suite's own guestbook spec,
   as written, against hosts whose tables are a schema of each engine's
   own, and `e2e-tables/guestbook-durable.spec.mjs` restarts a host of its
   own (SIGKILL, then SIGTERM) with a reader's page open. The main suite's
   hosts keep their tables in memory whatever the caller's environment
   names (`host-plan.mjs` clears `PW_TABLES_DATABASE_URL`, as it clears the
   feed's and the store's).
9. **Controls and evidence**: `scripts/durable_tables_mutations.py` (the
   strict native-assertion producer), `just e14-durable-tables` writing
   `docs/evidence/E14/durable-tables.txt`, planned beside a database
   (`scripts/ci_plan.py`'s `NEEDS_DATABASE`).

## Alternatives

- **The rows on PostgreSQL, the records in the host's materializer**
  (`publish_within`, memory's order). Rejected (the review's first
  finding): after a restart the record is gone and the row is not, so a
  retry runs again; or the other way round.
- **Refuse, at start, a program on PostgreSQL that schedules a run** (the
  integrator's other admissible answer). Rejected: ADR-0338 counts the runs
  among every transactional adapter's platform tables, they cost the
  adapter a table and six statements, and a refusal would keep a whole class
  of programs off durable storage.
- **An escape hatch on the adapter's command: a raw client in its
  transaction**, the server's existing `schedules::pg` and
  `hosts::command_session_*` reused. Rejected: a statement that failed and
  was swallowed leaves an aborted transaction whose `COMMIT` answers
  `ROLLBACK` with no error, which the adapter would answer committed; and
  those functions name unqualified tables (`schedules`, `sessions`) that a
  program's table could be named as.
- **Sessions in the host's memory.** Rejected: a restart signs every reader
  out and a retried interaction is refused rather than answered once
  (Context).
- **The fence as a transaction-level lock taken by the transaction's first
  statement** (no lock to give back). Rejected: a serializable
  transaction's snapshot is its first statement's, taken before the wait,
  so a sign-out committing during the wait is met as a serialization
  failure at the row lock, or not met at all without it; the other
  PostgreSQL layers lock first, outside the transaction, for this reason.
- **A shared trait for both adapters now** (the integrator's ruling 2 of
  ADR-0393, allowed here). Not introduced: the layer needs what memory has no
  counterpart for (the fence, the runs, the sessions), and a trait shaped by
  one adapter's needs would be the wrong one; the follow-up stands.
- **A schema setting for deployments** (`PW_TABLES_SCHEMA` everywhere).
  Not taken: a deployment's connection says its search path; the setting is
  a test's, as `PW_FEED_SCHEMA` is.
- **The session's lock on a connection of its own**, apart from the
  command's (so a command could take its connection inside the turn).
  Rejected: a lock on another connection outlives that connection's death
  no better than the command does; were it to close while the command
  runs, a sign-in could commit under a command fenced as no one, and a
  serializable command would not see it. On the command's own connection the
  lock and the transaction end together.
- **The outbox's delivered rows forgotten later**: by the scheduler's next
  pass, or by the next command's transaction. Not taken: either keeps
  delivered rows for a while and needs a queue of ids of its own to bound;
  one statement after the commit, outside the turn, keeps none, and the
  trim catches a host that ended between the two.
- **A run whose deletion removed nothing answered not current** (ADR-0393's
  answer, and the review's finding 3). Rejected: the run is still the one
  due, so the scheduler selected it again within the millisecond, a
  thousand transactions a second for as long as the other hand suppressed
  it. Refused, it is retried after its backoff, and kept failed after its
  retries, as any refused run is.
- **The host running a command again after a serialization failure.** Not
  taken here: a command's interaction is answered once (ADR-0325), and its
  running again is the host's decision for every layer, not this one's;
  answered refused, the reader's next press runs it again.

## Evidence

**The review's repairs (round two).** The record is the verify run named
in the evidence commit that follows this one (`just e14-durable-tables`,
`e14-d3-postgres` and `e14-data-path`, each whole). What it holds: the
adapter's `tests/platform.rs`, 14 (the 7 below, and: a command waiting for
its session holding no other up; a command holding the turn never waiting
for a connection; a failed session's change's connection closed; a
suppressed taking refused; sessions and sign-ins swept, bounded, a held
session passed by; the outbox's delivered rows and leftovers forgotten; a
render's reads ended at a read that fails); the server on PostgreSQL,
`tests::tables_pg::`, 27 (the 18 below, the outbox's two among them now
reading an audit trigger's copy, and: the outbox keeping no delivered row;
another session's command not held up by one waiting for its session; a
suppressed taking retried after its backoff; a run scheduling itself again,
each generation once; a `COMMIT` ended under it answered unknown, one whose
answer was lost answered committed from its record; a 40001 at `COMMIT`
answered refused; sessions and sign-ins swept by the host; a command fenced
from its first statement or never); and the mutation script's 47 mutants
(the first round's 29, three of them re-anchored and two re-aimed, at the
generation the taking's locked row holds and at a taking whose deletion
removed nothing; and 18 new). Locally, before the record (PostgreSQL 18.6):
the 23 new, re-anchored and re-aimed mutants killed, one after a repair of
the mutant itself (its replacement needed a type to build).

**The first round.** `docs/evidence/E14/durable-tables.txt`, by
`just e14-durable-tables` in
verify 41480579645 at `d13a6059` (PostgreSQL 18.6), fetched by
`scripts/evidence_fetch.py 41480579645 --known summary --skip e14-guide`;
the same run recorded what the change reaches as the brief named it:
`e14-d3-postgres` 53 of 53 mutants killed (its baseline 56 tests, this
track's platform binary among them), `e14-data-path` 25 of 25,
`e14-browser-host-map` 54 of 54; and `e14-guide` failed, on a mutant of
its own the checker refuses (finding 2). `ci` passed at `ce8ec461`
(41469462379) and `d13a6059` (41480567122). The planner reaches about 150
recipes from this change (finding 5); the four were chosen as those whose
inputs it changes in substance, the rest by the rebuilt server alone.

The record: the adapter's `tests/platform.rs` (7: runs
committed with their command or not at all, taken once by their
generation, retried by it alone; sessions by their lifetimes; a fenced
command committing only as its session, a session's change waiting for it);
the server on PostgreSQL, `tests::tables_pg::` (18: the guestbook's
behaviours; one `xmin` for a command's entry, book, record and event, and
for a note and its run; refusals at `COMMIT` keeping none of them; a run's
replaced generation committing nothing; one state a render and none without
a seat; removal by its author alone and a guarded command calling no table
function; a sign-out before a command's authority, one waiting for a command
admitted first, a session past its lifetime at the commit; the conformance
suite over this layer, 111 and 164 reads, its idempotency and its pins, one
statement a read); and in memory (`tests::tables:: tests::guestbook::
tests::conformance::`); the server as a process (`tests/durable_tables.rs`);
the guestbook in three engines on PostgreSQL (15: the main suite's spec, 12,
and the restart, 3) and in memory (12); and
`scripts/durable_tables_mutations.py`'s 29 mutants, each killed at a native
assertion.

Local runs (PostgreSQL 18.6, before the record): the whole server suite,
882 passed and 9 ignored, with this layer's tests among them; the browser
suite on PostgreSQL three times, 15 of 15 each (`run_suite.sh --runs 3`).
The first whole mutation run is history, not the record: 26 of 29, and
three repairs of the tests, not the code. `a run no longer current
completes anyway` survived because the scheduler's own check, before the
command, refused the old generation first: the test now replaces the
generation as the command begins, so only the taking in its transaction
can refuse it. `a render with no seat reads the latest commit` survived
because the test read the raw page, whose part markers split `first kept`:
it now reads the page as a reader does. `a fenced command's session lock is
never given back` was detected (three tests failed) but invalid, one of
them failing in `signed_in_for_test`'s `expect`: the test now asserts that
the session's change waits for no ended command first. Each was then
killed (3 of 3).

## Not claimed

- **A second host told live** of the first's commits: the layer has no
  `tail()` over `pw_outbox` (ADR-0369's reader, `outbox_trimmed`) and no
  `instance()`, so two hosts on one database read each other's commits and
  share their sessions and records (tested), but an open page on one is not
  told of a commit on the other, and the two number their pages alike. Not
  next (the integrator's ruling (c) of 2026-11-02): the target deployment is
  a single VM, one host on its database. Where a tail comes, the outbox's
  rows are forgotten by the slowest reader's cursor, as ADR-0369 trims, and
  not as each commit's host delivers them (decision 6).
- **Several sources a program** (T1, ADR-0338 5.1): one database holds a
  program's tables, whichever source declares them.
- **Uploads beside declared tables**: the layer keeps no upload
  (`uploaded_by` is the default).
- **The platform's own tables held by the catalog at start** (ADR-0393's
  "Not claimed" extends to `pw_runs`, `pw_sessions`, `pw_sign_ins`).
- **A session's change at read committed whatever the connection's
  default** is reasoned (ADR-0393's finding 4: a change that waited for a
  command must read what it committed), not held by a test: no test runs
  the host under a serializable default, so its mutant would survive, and
  none is planted. Nor is the session's row lock inside a fenced command,
  which guards against a writer that takes no session lock, and no test
  has one.
- **A graceful stop**: the development server has no handler for SIGTERM,
  which ends it as SIGKILL does; what a stop keeps is what each `COMMIT`
  kept.
- **Latency and throughput.** Statement counts only (each pinned query one
  statement a read).
- **CockroachDB** (T5).
- **Hosts with different session lifetimes on one database**: each host
  sweeps by its own (`PW_SESSION_IDLE`, `PW_SESSION_LIFETIME`), so the
  shortest wins; a deployment states one pair.

## Depends on

- **Track `data-path`'s keys of a host's interactions** (the review's
  finding 7, theirs): the host builds an interaction's key from the raw
  session cookie (`Server::receipt_keys`), and this layer keeps the key in
  `pw_interactions` as it is given, so a reader of the database reads live
  session cookies there. `data-path`'s worker is changing how the host
  builds the keys; this layer takes whatever key it is given, and keeps no
  cookie of its own (its sessions and sign-ins are by SHA-256). Until that
  lands, this ADR's durability is a durability of those keys too.
- **The deploy track's production allow-list for `/bench/*`** (the review's
  finding 7, theirs): the development server answers `/bench/*` to an
  anonymous client (a test's controls, among them its clock), which the
  development deployment binds to loopback; a production host must refuse
  them, and that list is the deploy track's.
- **PR #67 (`session-overlap`)**, the admission race of finding 1 below:
  once it lands on `master`, this track merges `master`, takes out the
  restart spec's wait for the page's own reload, and adds a test of one
  session's concurrent requests on this layer (six `GET /` at once, which
  answered five 503s, and two signing `POST`s at once, which answered one
  403 `session-changed`; each must succeed).
- **`data-path`'s adapter-neutral refused commit**: when it lands on
  `wip/data-path`, this layer's honouring of `tables::FAIL_NEXT_PUBLISH`
  goes, replaced by what the suite then asks of a layer (the integrator's
  ruling (a) of 2026-11-02).

## Corrections (the review of 3ccd083a, 2026-11-02)

- **CORRECTION: `pw_outbox` grew for good** (finding 1). The layer has no
  tail, so no trim ran, and every commit's events stayed. Each commit's rows
  are now forgotten once it has answered, and leftovers by age (decision
  6); `the_outbox_keeps_no_row_its_host_delivered` holds the outbox at
  nothing undelivered across commits, and two mutants (rows kept, leftovers
  never trimmed) are killed by it.
- **CORRECTION: a command waiting for its session held its host up**
  (finding 2). The fence waited, for its connection (thirty seconds at
  most) and for the session's lock (five), inside the host's single
  command turn, so one reader whose session another host's command held
  delayed every reader of the host. The lock and the place are now taken
  before the turn (decisions 3 and 5); the server test
  `a_command_waiting_for_its_session_holds_no_other_session_up` and the
  adapter's `a_command_waiting_for_its_session_holds_no_other_command_up`
  bound another session's command to two seconds while one waits, and
  `a_command_holding_the_turn_never_waits_for_a_connection` holds the turn's
  command to its place; the mutants that hold the turn through the wait, or
  take a place inside the turn, are killed.
- **CORRECTION: a run whose taking was suppressed ran a busy loop**
  (finding 3). `consume` answered `false` both for a run no longer current
  and for a deletion that removed nothing; the scheduler took the second for
  the first and selected the run again at once. Told apart now, as
  `schedules::pg::consume` tells them (decision 3); tested with a trigger
  suppressing every deletion of a run, the run retried after its backoff
  (server) and the taking refused (adapter), with a stale generation still
  not current beside it.
- **CORRECTION: expired sessions and sign-ins stayed** (finding 4). A
  session past its lifetimes was deleted only when its own cookie came back,
  and a sign-in's PKCE verifier and nonce stayed until a later sign-in.
  Swept now by the host (decision 6); tested in the adapter (bounded
  batches, a held session passed by without waiting) and in the server (the
  scheduler's pass sweeping both, a minute apart).
- **CORRECTION (D3's): a render's read after one that failed read another
  state** (finding 5; decision 4). D3's own test pinned the old behaviour
  ("the next opens another"), and is corrected with it.
- **Hardening** (finding 6), each with its test: a run that schedules
  itself again (taken before its next generation is written: the mutant
  writing it first is killed, the generation's command then finding no run
  to take); a `COMMIT` whose connection ended under it (answered unknown,
  never refused) and one that landed and whose answer was lost (through a
  proxy that drops the answer: reconciled from the record, answered
  committed, once); a session's change's connection not kept unless it
  committed; a command fenced from its first statement or never (the late
  `fence` is gone from the adapter, and a staging asked for its authority
  late is unusable); a rollback the database answered (40001 at `COMMIT`, a
  refusal) answered refused, not unknown.

## Findings

- **CORRECTION (the identity's): two admissions of one session overlapping
  on a host whose sessions are kept in a database answer the later one 503,
  "The session could not be confirmed. Reload the page."**
  `Principals::admit` advances the session's revision at every admission
  and refuses one whose revision moved while it read (`Superseded`); kept
  sessions serialize their admissions on the session's lock, so two that
  overlap always conflict. Seen in the restart spec, a reader's reload
  racing the page's own reload, told by the new process (one of three
  engines in a first run); the feed's and the store's PostgreSQL layers keep
  sessions the same way. The spec waits for the page's own reload before its
  reader's until PR #67 (`session-overlap`) repairs the race ("Depends on").
- **CORRECTION (track `data-path`'s, open): `e14-guide` fails at this
  track's commit**, and would at `data-path`'s: `scripts/guide_mutations.py`'s
  mutant "the entries are not told of a signed entry" (`invalidates_on
  Removed(_)` for `Signed(_), Removed(_)`) is a program the checker refuses
  (PW5107, `sign` speculates on `Latest` and does not invalidate it), since
  `data-path`'s guestbook speculates on `Latest`; the script counts it
  refused, 73 of 74 (verify 41480579645). Neither file is this track's; the
  mutant needs a well-typed replacement.
- **A render's state on PostgreSQL is its first table read's.**
  `data-path`'s render test commits between `Me` (which reads no table) and
  `Latest`, so on PostgreSQL that commit is the render's; the PostgreSQL
  test uses a page whose two bindings each read the table, which the shared
  seat holds to one state, and which a seat a query is a control on.
- **The main suite's hosts read the caller's `PW_TABLES_DATABASE_URL`**:
  `host-plan.mjs` cleared the feed's and the store's databases, not the
  tables', so a caller naming one ran the main suite's guestbook on
  PostgreSQL, every engine's host in one schema. Corrected, with its test.
- **A change to the server's `main.rs` or `data.rs` plans about 150
  recipes** (`ci_plan.py --changed`): every recipe that builds the server.
- **Two small pieces of the memory layer are repeated** in `tables_pg.rs`:
  the refusal of a hand-written layer beside declared tables, and the
  interning of the grants' words. `tables.rs` is under `data-path`'s review
  repairs, and this track leaves its files alone meanwhile; they are one
  function each once those land.

## Rulings taken (the integrator, 2026-11-02)

- (a) `tables::FAIL_NEXT_PUBLISH` stays until `data-path`'s adapter-neutral
  refused commit lands on `wip/data-path`; the integrator says when to
  merge it ("Depends on").
- (b) The admission race is PR #67's to repair ("Depends on").
- (c) The second host told live is not next: the target is a single VM
  ("Not claimed").

## Questions for the integrator

- `PW_TABLES_DATABASE_URL` as one database a program, until ADR-0338 5.1's
  `PW_SOURCE_<NAME>` routes sources apart (T1).
- The host running a command again after a serialization failure, for
  every layer at once (Alternatives), as a follow-up of the host's.
