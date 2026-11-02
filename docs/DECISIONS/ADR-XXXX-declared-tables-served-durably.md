# ADR-XXXX: a program's declared tables served durably from PostgreSQL, one transaction a command

Status: proposed by track `durable-tables` under the owner's delegation of
2026-10-02 and the standing order of 2026-10-25 (decide by best judgement);
the integrator numbers it (ADR-0397 reserved) and lands it after an
independent review. Date: 2026-11-02. Milestone: E14, the app layer.
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
   first (not the run selected any more, nothing commits:
   `Commit::NotCurrent`), then the runs it schedules or cancels written, its
   references held (`prepare`), its interaction's record and its outbox
   written (the outbox under its lock, in commit order), its reader's session
   held to its fence, and `COMMIT`. Refused anywhere, none of it is kept; a
   `COMMIT` never answered is said so, and the host reconciles it from the
   record, as for every PostgreSQL layer.
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
     `Command::fence` and `Command::revalidate`, and on `PgTables` the runs
     outside a command (`due_runs`, `next_run`, `run_current`, `retry_run`,
     `runs`) and the sessions and sign-ins (`admit_session`,
     `inspect_session`, `open_session`, `close_session`, `begin_sign_in`,
     `take_sign_in`), all in `runtime/pw-table-postgres/src/platform.rs`.
     `PgTables::open_json` takes tables.json's text, so the host links no
     compiler for it.
4. **One state a render** (NEXT 4c, ADR-0354): a render's snapshot is one
   of the adapter's seats, and every read function its queries link reads
   in it (`Snapshot::read_host_fns` shares the seat). A seat not taken
   within its wait fails the render's reads, saying so; it never reads the
   latest commit instead. On PostgreSQL the state is the render's first
   table read's (ADR-0393's decision 8, PostgreSQL 18 13.2.2), not its
   start's, as in memory.
5. **The reader's session is the database's** (ADR-0369): kept by its id's
   SHA-256 with its principal and generation, alive by its two lifetimes on
   the database's clock, its use recorded at most once a minute, one past a
   lifetime forgotten by the admission that finds it, a sign-in under way
   taken once; each a read-committed transaction under the session's lock,
   on connections of their own (at most eight). **A command is fenced on
   it** as the other PostgreSQL layers fence theirs: the session's lock
   taken on the command's own connection before its transaction begins (its
   snapshot after the wait) and given back once it ends (or the connection
   closed, which gives it back); the session's row locked in the
   transaction; the session held to the one the command was admitted as at
   the fence, by `revalidate` (a command writing nothing or answering a
   declared error) and just before `COMMIT`. A session's change waits for a
   command fenced on it, on any host, and a command fenced after a sign-out
   is refused, keeping nothing. A guest is the host's own, as in memory.
6. **The conformance suite over this layer, as written**: `conformance.rs`'s
   suite (answers and costs at 3 and 24 rows, staging, refused commits)
   instantiated over PostgreSQL for the guestbook's tables and the lists
   app's, its oracle the same (111 and 164 reads checked, as in memory), and
   its idempotency and query pins on this layer. The suite's refused-commit
   hook (`tables::FAIL_NEXT_PUBLISH`) is honoured by this layer as a refusal
   before its `COMMIT`, until `data-path`'s adapter-neutral refusal lands;
   a refusal the database itself makes at `COMMIT` (a deferred trigger of
   another hand's) is tested apart, keeping none of the rows, the record,
   the event or the run.
7. **The guestbook in three engines on PostgreSQL**:
   `playwright.tables.config.mjs` runs the main suite's own guestbook spec,
   as written, against hosts whose tables are a schema of each engine's
   own, and `e2e-tables/guestbook-durable.spec.mjs` restarts a host of its
   own (SIGKILL, then SIGTERM) with a reader's page open. The main suite's
   hosts keep their tables in memory whatever the caller's environment
   names (`host-plan.mjs` clears `PW_TABLES_DATABASE_URL`, as it clears the
   feed's and the store's).
8. **Controls and evidence**: `scripts/durable_tables_mutations.py` (the
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

## Evidence

`docs/evidence/E14/durable-tables.txt`, by `just e14-durable-tables` in a
verification run at the track's commit, fetched by `just evidence-fetch`
(its run named in the file): the adapter's `tests/platform.rs` (7: runs
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
  told of a commit on the other, and the two number their pages alike.
- **Several sources a program** (T1, ADR-0338 5.1): one database holds a
  program's tables, whichever source declares them.
- **Uploads beside declared tables**: the layer keeps no upload
  (`uploaded_by` is the default).
- **The platform's own tables held by the catalog at start** (ADR-0393's
  "Not claimed" extends to `pw_runs`, `pw_sessions`, `pw_sign_ins`).
- **A graceful stop**: the development server has no handler for SIGTERM,
  which ends it as SIGKILL does; what a stop keeps is what each `COMMIT`
  kept.
- **Latency and throughput.** Statement counts only (each pinned query one
  statement a read).
- **CockroachDB** (T5).

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
  sessions the same way. The spec now waits for the page's own reload before
  its reader's; the race is the identity track's to repair (an admission
  whose read is the identity installed meanwhile could answer it).
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

## Questions for the integrator

- When `data-path`'s adapter-neutral refused commit lands, this layer's
  honouring of `tables::FAIL_NEXT_PUBLISH` goes, replaced by whatever the
  suite then asks of a layer.
- The admission race (finding 1): the identity track's repair, or a ruling
  that a 503 the page tells its reader to reload past is acceptable.
- The second host told live (`tail()`, `instance()` over `pw_outbox`) as
  this layer's next, beside D4 and D5.
- `PW_TABLES_DATABASE_URL` as one database a program, until ADR-0338 5.1's
  `PW_SOURCE_<NAME>` routes sources apart (T1).
