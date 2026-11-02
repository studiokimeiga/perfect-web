# ADR-0393: any program's declared tables on PostgreSQL, answering as memory does

Status: accepted. Proposed by track `d3-postgres` under the owner's
delegation of 2026-10-02 and the standing order of 2026-10-25 (decide by
best judgement); independently reviewed, repaired and landed by the
coordinator's lander (track `d3-land`, 2026-11-02; its PR holds the landing
record). The review's corrections are in [the review](#the-independent-review-2026-11-02)
below, and the integrator's rulings in [Rulings](#rulings-the-integrator-2026-11-02).
Date: 2026-11-02.
Milestone: E14, the app layer. ADR-0338's D3, the reference adapter after
memory. Builds on ADR-0338 (decisions 1.4, 1.5, 5.2, 5.5 and D3), ADR-0385
and ADR-0386 (the gate, typed answers, staged commands, the memory adapter),
ADR-0246 and ADR-0298 (the feed and the store on PostgreSQL: advisory-lock
migrations, serializable commands, the outbox, bounded pools), ADR-0325 (an
interaction's record committed with its writes), ADR-0336 and ADR-0337 (a
read is a bounded number of statements; set reads), ADR-0354 (one snapshot
per render, in a bounded few seats) and ADR-0369 (the outbox in commit
order).

## Context

- **After D2 a declared table lived only in memory** (`pw-table-memory`,
  ADR-0385, ADR-0386): every primitive, staged commands, the gate. Nothing
  durable served a program's declared data; the store and the feed reach
  PostgreSQL only through hand-written Rust layers (`store_pg.rs`,
  `feed_pg.rs`), which D4 and D5 are to delete.
- **D3 asks for the reference adapter** (ADR-0338, decision 6): migrations
  from the declarations, one statement per primitive, set reads `= ANY`, the
  platform's tables, `provides()`, the conformance suite's parts on
  PostgreSQL 18.6, and four controls: a set read looped per key, a
  transaction at the connection's default, a migration applied twice, drift
  accepted.
- **Two tracks work beside this one**: `data-path` (the development
  server's routing of a program's tables, a conformance suite on the memory
  adapter) and `row-ownership` (owned tables). This track edits none of
  their files, `pw-table-memory` included, and builds behind the interface
  the memory adapter has, so either plugs into the routing.
- **Measured on PostgreSQL 18.6 before deciding** (2026-11-02):
  `'[-0.0, 1e300]'::jsonb::text` is `[0.0, 1000…000]` (the sign lost, the
  number rewritten), while `json` keeps its text; `text` refuses U+0000
  ("invalid byte sequence for encoding UTF8: 0x00"), which a Pleris
  `String` may hold.

## Decision

1. **A crate, `runtime/pw-table-postgres`, behind the memory adapter's
   interface.** `PgTables` has `MemoryTables`'s methods, each meaning what
   it means there: `read`, `read_host_imports`, `read_host_fns`,
   `host_tables`, `begin`, `tell_on_wait`, `transaction`; `Command`'s
   `read`, `write`, `host_fns`, `written`, `prepare`, `commit`; `Prepared`'s
   `written`, `publish`; `Committed.written`; and the memory adapter's
   `Error` variants, each meaning the same. PostgreSQL's own:
   `PgTables::open(url, schema, tables.json)` (`open_with` and `Options`:
   isolation, eight connections, eight render seats, 30 s waits, the host's
   id), `Prepared::publish_with(Records)` (the platform's records in the
   command's transaction), `provides`, `recalled`, `snapshot`, and the
   counters `statements` and `prepared`. A crate, not D3's
   `server/src/adapters/postgres.rs`: D2's adapter landed as a crate, the
   server's routing is `data-path`'s, and a crate keeps the dependency one
   way. It uses only the `postgres` crate already in `Cargo.lock`.
2. **Each field's column** (ADR-0338 5.2, decided here):

   | Stored | Column | Why |
   | --- | --- | --- |
   | `Int` | `bigint` | |
   | `String` | `bytea`, the UTF-8 bytes | every string (U+0000 among them); ordered and compared byte by byte, which is code point order and Rust's `String` order, whatever the database's collation |
   | `Bool` | `boolean` | |
   | `Float` | `double precision`, finite by `CHECK` | the binary protocol keeps every bit, `-0.0` among them |
   | `Unit`, `List`, `Record` | `json` | its text kept exactly; `jsonb` loses `-0.0`'s sign |
   | `Option<T>` | `T`'s column, nullable | `NULL` is none |

   The domain is the memory adapter's exactly: an `Option` of an `Option`,
   an `Option<Unit>`, `Set`, `Map`, a type of cases and a recursive type are
   refused where the adapter opens; a name longer than PostgreSQL keeps (63
   bytes) is refused; an object's derived name past it is cut and hashed.
   Two objects a declaration would give one name (`<table>__<what>`, where
   a name may hold `__`: an index `by_rank__check` and a field `by_rank`'s
   check) are refused where the adapter opens, as is a table named as one
   of the platform's relations (the review's finding 3). A float nested in
   a `json` column reads back as the very double written: the workspace's
   serde_json parses with its `float_roundtrip` feature (finding 1).
3. **A refinement is a `CHECK`** (ADR-0338 1.4): an `Int`'s bounds, a
   `String`'s length in code points (`pw_code_points(bytea)`, the bytes not
   continuation bytes), a `Float` finite, a `json` column's shape (`null`,
   `array`, `object`). A refinement inside a list or record is held by the
   adapter, on every write and every read, not by a check.
4. **The schema comes from tables.json, once, under a lock, recorded.** In
   one read-committed transaction (whatever the connection's default, so
   that a host that waited for the lock reads what the first made: finding
   4) under `pg_advisory_xact_lock` on the schema's own key:
   the platform's tables (`pw_platform`'s versions: `pw_schema`, `pw_outbox`,
   `pw_interactions`, `pw_code_points`), then the program's. `pw_schema`
   records each schema applied: its hash, its tables (as `schema.lock`
   holds them), the catalog as it stood, and the server's version.
   - **None recorded**: every table made whole; a table of the program's
     name that no schema recorded refuses the start (drift).
   - **The build's recorded**: nothing is made. The catalog (each column's
     type and nullability, each constraint's and index's definition, each
     trigger not PostgreSQL's own, each table's kind, persistence, row
     security and triggers not firing, a reference's own among them, each
     rule, each table inheriting one, the code points function) is held to
     what was recorded with it, and a difference refuses the start naming
     it (drift); so does a record whose hash is not its schema's. A server
     of another major version is held to the catalog's structure, and the
     catalog recorded again as it writes it.
   - **Another recorded**: the change from it. Where tables.json's migration
     is from the schema recorded, its stated steps (`renamed`, `dropped`,
     retyped and added `backfilled with`) are the program's statements of
     what loses or reinterprets data, and every other change is generated;
     where it is from another (or none), only what adds is generated (a
     table, an `Option` field, an index, a reference, a check), so an older
     build never takes a newer database back. A change nothing states, a
     key changed, a field of the key retyped (finding 2), a table dropped
     or moved between sources, and a stated step that takes the recorded
     schema nowhere refuse the start, each named; a check, a unique index or
     a reference a recorded row breaks refuses it whole.
   - **Applied in this database's terms**: columns renamed (with their
     constraints), dropped, retyped (dropped and added with the backfill as
     a default, then no default) and added, a backfill written as no
     literal the server's string syntax could read otherwise (a `String`'s
     bytes `decode`d from hex, an `Int` quoted: finding 5); then the
     objects reconciled by
     their deterministic names: foreign keys, checks and indexes dropped,
     then checks, indexes and foreign keys made. A unique index is on its
     fields; a plain one on its fields, the order and the key, so its read
     comes in order with no sort.
5. **Every primitive is one prepared statement**, prepared once on each
   connection that runs it: a key's, a prefix's or an index's read
   (`WHERE (..) = (..)`), its set form (`= ANY($1::type[])` for one field,
   `(..) IN (SELECT * FROM unnest($1, $2, ..))` for several: one statement
   whatever the number of keys, none for no keys), `all` (`LIMIT bound + 1`,
   past its bound refused), each `ORDER BY` tables.json's `sorted_by` then
   the key. An `insert` or a `put` is one statement: the row holding its
   key, then one holding each unique value, in their declared order (the
   memory adapter's order), the bound counted to its bound, the write, and
   the row it replaced, all at once; refused, it raises nothing, so the
   transaction stays whole and the refusal is a value, as in memory. A
   `delete` and a `delete_of_*` return the rows they removed. Another
   transaction's racing unique value is refused by PostgreSQL as a
   serialization failure (the holder's read took the predicate lock), never
   answered as a row this transaction did not see.
6. **A command is one serializable transaction, with the platform's
   records.** One command of an adapter at a time (its turn, as memory's
   and every PostgreSQL layer's: a host never conflicts with itself); its
   connection and `BEGIN ISOLATION LEVEL SERIALIZABLE` taken at its first
   statement, so a command touching no table costs nothing; its reads see its
   writes and no other read sees them before its commit. References are
   foreign keys `DEFERRABLE INITIALLY DEFERRED`, held where the command
   prepares by `SET CONSTRAINTS <its own foreign keys> IMMEDIATE`, so
   related rows are written in either order and a missing one is refused as
   memory refuses it (`Things references a missing Owners`). `written` is
   kept from the rows each statement returns: a key's row before the command
   first wrote it, and now, compared as memory compares them (`-0.0` is not
   `0.0`); a statement whose answer cannot be read (a row another writer
   left out of its type) fails its command, every later statement and its
   commit with it, since its write may be made and unrecorded (finding 7).
   `publish_with` writes the interaction's record (one statement:
   older ones forgotten, this one inserted, a live one refusing it) and the
   outbox's rows (one statement, under the outbox's advisory lock held to
   the commit, so ids are in commit order across hosts; every row it was
   given answered back, or the commit refused: finding 8), then `COMMIT`. An
   `ERROR` answering the `COMMIT` rolled it back: a serialization failure
   (40001, 40P01) or a refusal; no answer, or a session ended (`FATAL`), is
   an unknown outcome, never taken for a refusal.
7. **A serialization failure, retried as the platform retries.** The host
   runs a compiled command once per interaction and answers a refused commit
   refused (ADR-0246's alternative, ADR-0352's ruling: "serializable
   conflicts are legitimate refusals"); its client's retry is answered once
   by the interaction's record (ADR-0325), and a scheduled run's by the
   scheduler's retries. `PgTables::transaction`, whose closure may run again,
   runs it again from its beginning, four attempts at most (PostgreSQL 18,
   13.2.3).
8. **Reads and renders.** A read outside a command is one statement on a
   bounded pool (the latest commit). A render's `snapshot()` is a seat among
   the deployment's few, taken where the render begins, its read-only
   repeatable-read transaction opened at its first read and ended when it is
   dropped; a failed read ends it and the next opens another (ADR-0354).
9. **`provides()`** measures a transaction opened as a command's
   (ADR-0246): serializable, or what the connection's default gives where the
   deployment leaves it; a read-only database refuses to open.

## The conformance suite's parts (ADR-0338 5.5), on PostgreSQL

`data-path`'s suite was not on master when this track began; its parts are
mirrored here, against the memory adapter's interface, so that suite runs on
`PgTables` as written:

1. **Answers**: `tests/parity.rs`: a seeded workload of 160 commands of random
   writes and reads over every primitive and every kind of field, each
   committed or dropped, every answer, `written` and commit compared with
   the memory adapter's (more than 6,000); `examples/tables`' `app.pw` and
   `writer.pw`, compiled and run by the engine through the gate on both.
2. **Guarantees**: `provides()` serializable; read committed and
   serializable defaults measured; a read-only database refused.
3. **Anomalies**: write skew between two hosts refused under serializable,
   and committed at read committed (the negative control).
4. **Costs**: every primitive one statement at 4 and 400 rows, a set read at
   1 and 400 keys; a statement prepared once a connection; a command's
   statements pinned (a write: `BEGIN`, it, its references, `COMMIT`).
5. **Commits**: a commit refused by a deferred trigger keeps no write, record
   or outbox row; a commit never answered is said so; a reference refused at
   prepare keeps nothing.
6. **Delivery**: outbox rows committed if and only if their commit, ids in
   commit order (a second host's commit seen waiting for the first's lock).
7. **Idempotency**: the interaction's record in the commit; a second host
   meeting it refused, keeping nothing; forgotten after a day.

And a commit outliving its host (`tests/durability.rs`): a process commits,
ends with a command open, and the next host reads every commit back and
nothing of the open command.

## Alternatives

- **`jsonb` for a nested value** (ADR-0338 5.2's table). Rejected: it lost
  `-0.0`'s sign, measured; `json` is the value's exact text, and no
  statement compares inside one.
- **`text COLLATE "C"` for a `String`.** Rejected: it refuses U+0000, so a
  string the memory adapter keeps would trap a command here.
- **Unique values left to PostgreSQL's constraint errors** (`23505`).
  Rejected: an error aborts the transaction, and a refused write must be a
  value the program handles (ADR-0386, PW0618).
- **`SET CONSTRAINTS ALL IMMEDIATE`.** Rejected, found by the refused-commit
  test: it would make every deferred trigger of the database immediate, a
  deployment's and the outbox's, not only the program's references.
- **Every change generated from any schema recorded.** Rejected: an older
  build's host would take a newer database back (drop its index, its
  check), silently.
- **Retrying a refused command inside the host.** Rejected, as ADR-0246
  rejected it: a compiled command runs once per interaction.
- **A per-table version row** (for `Committed.written`'s versions shared by
  hosts). Rejected: every two commands writing one table would conflict on
  it; versions are this adapter's commits, as memory's are its process's.

## The independent review (2026-11-02)

The lander's review (AGENTS.md's PR rule 4). Each finding was reproduced by
a test that failed before its repair, and each repair has a mutation control
its test kills (`scripts/d3_postgres_mutations.py`, part 7). Corrections
first:

1. **A float nested in a `json` column read back one or two units in the
   last place off.** The column keeps the float's shortest digits, but
   serde_json without its `float_roundtrip` feature parses many to a
   neighbouring double (measured on 1.0.151: 593,094 of 2,000,000 random
   doubles, 26,747 of 199,999 values like `1.5666666666666667`; none with
   the feature). A record's or a list's `Float` came back as another double
   than the memory adapter answers. The parity workload passed it, as every
   float it chose (`0.0`, `-0.0`, `1.5`, `5e-324`, `-2.25`, `0.5`) parses
   exactly. Repaired in the workspace's manifest (serde_json's own feature,
   no crate added), so every JSON float the workspace reads is the one
   written, a browser's among them. Tests: `a_nested_float_is_answered_bit_for_bit`
   (400 doubles, from their bits; 134 differed before), and the workload's
   nested floats, three of which differed.
2. **A field of the key retyped** by a stated `backfilled with` (the
   compiler plans a retype for any field) was refused only by accident, as
   `Schema("Things has unknown field n")` from the model of the change; had
   the model held, the column's drop would have taken the primary key with
   it and given every row one key. Now refused by name, as a key changed.
   Test: `a_key_field_retyped_is_refused_and_its_key_kept`.
3. **Two objects given one name.** An object is named `<table>__<what>`,
   and an identifier may hold `__`: an index `by_rank__check` is named as a
   field `by_rank`'s check. The objects were kept in a map by name, so the
   index silently replaced the check, which was never made (and the catalog
   recorded without it). Now refused where the adapter opens, as is a table
   named as one of the platform's relations. Test:
   `a_name_two_objects_would_share_is_refused_at_open`.
4. **The migration read at the connection's default isolation.** Where a
   deployment makes every transaction serializable or repeatable read, the
   snapshot is the lock's own `SELECT`'s, taken before the lock is granted
   (PostgreSQL 18, 13.4.2), so a host that waited for the first did not see
   the schema it made, and failed (`relation "pw_schema" already exists`).
   Now read committed. Test:
   `hosts_racing_under_a_serializable_default_each_open_it_made_once` (the
   lock held by the test until four hosts wait for it).
5. **A backfill written as a literal the server could read otherwise**:
   `'\x6162'::bytea` is `a62` where `standard_conforming_strings` is off,
   and `-9223372036854775808::bigint` is out of range. Now a `String`'s
   bytes are `decode`d from hex and an `Int` is quoted. Test:
   `a_backfill_is_the_value_stated_whatever_the_servers_string_syntax`.
6. **Drift the catalog did not see**: a rule (a delete that deletes
   nothing), a table inheriting the program's (its rows read with it), a
   reference's triggers disabled or firing only on a replica (references
   no longer held), a table made unlogged (emptied by a crash). Each is now
   a line of the catalog; `a_catalog_changed_by_hand_refuses_the_start`
   holds the five.
7. **A write whose answer could not be read left its command whole.** A
   `put` over a row another writer left out of its type replaced it, then
   failed decoding the old row; the command still committed the write, its
   `written` naming no table. Now the command fails. Test:
   `a_write_whose_answer_cannot_be_read_fails_its_command` (a put, a
   delete, a prefix's delete).
8. **An outbox row dropped by another hand's trigger** (`BEFORE INSERT`
   answering `NULL`) was committed without it, the event lost. Now the
   commit is refused short of a row. Test:
   `a_commit_keeps_every_outbox_row_it_was_given_or_nothing`.

Held, against the strongest counterexamples tried: every identifier is
quoted (a `"` doubled) and qualified by the schema, NUL refused, and every
value a parameter or a number (a backfill aside, now hex); a `COMMIT`
answered `ERROR` is a refusal, one unanswered or ended `FATAL` an unknown
outcome, never a refusal or a success; a refused write leaves the
transaction whole, and a statement's error fails the command, whose commit
is then refused (a `COMMIT` of an aborted transaction would answer
`ROLLBACK` without an error); every path that gives a connection back ends
its transaction or discards the connection; an older build is refused on a
newer schema; a concurrent unique value is a serialization failure, never a
row the command did not see; the parity test compares JSON texts and every
error's words, and refuses a workload that meets no rows, conflicts,
refused values, refused references or commits.

## Evidence

`docs/evidence/E14/d3-postgres.txt`, by `just e14-d3-postgres` in CI's
verification run at the landing's commit, fetched by `just evidence-fetch`:
the crate's tests (primitives 14, commands 17, migrations 12, parity 4,
durability 2: 49) and `scripts/d3_postgres_mutations.py`'s 53 mutants (the
author's 41 and the review's 12), each killed at a native assertion (the
strict producer of ADR-0386), ADR-0338's four named controls among them.
The recipe is in `scripts/ci_plan.py`'s `NEEDS_DATABASE`.

The author's first whole run (local, 2026-11-02) killed 39 of 41, and is no
record: the `jsonb` control's kill was invalid, since the durability test
echoed its child process's harness output (a second summary, a second panic
site) into its own assertion's message, and each of the child's lines is now
marked as the child's; and the one-command-at-a-time control survived, since
a second command slow to open its connection began after the first had
committed, and the test now holds that the second does not begin while the
first is open, and reads what the first committed. Each was then killed (41
of 41 at `89deaf86`, the author's record, which the landing's replaces).

## Not claimed

- **The development server serving declared tables on PostgreSQL**: a
  `DataLayer` over `PgTables` (`reads` its `read_host_fns`, `snapshot` and
  `reads_in` its `snapshot`, `begin` its `Command`, `rows` its `written`,
  `commit` its `publish_with` and `Commit::Applied`, `keeps_interactions`,
  `recalled`, `provides`) beside `data-path`'s routing; so neither
  ADR-0338's "TodoMVC on PostgreSQL" (TodoMVC has no host data layer since
  ADR-0366) nor a browser on it.
- **Scheduled runs** (ADR-0334) in the adapter's transaction, **the second
  host's reader** of the outbox (ADR-0369: its cursor, `pg_notify`,
  trimming; the rows keep `host`, `created` and `committed_in` for it) and
  **sessions kept** in the database.
- **References of a composite key or across sources**, refused as memory
  refuses them.
- **Latency and throughput**: statement counts only.
- **Every growth write of a bounded table reads it** (its count, to the
  bound), so two hosts' concurrent growth of one bounded table conflict and
  one is refused (found by this track's ordering test, which then wrote
  unbounded rows). Inherent to holding a bound serializably; not measured
  under load.
- **A refinement inside a list or a record** is held by the adapter, not by
  a `CHECK`; another writer of the database can store one, which every read
  then refuses, and a write over it fails its command (finding 7). So is a
  `String` column's UTF-8: its bytes are held to it by every read, not by a
  `CHECK`.
- **A field named as one of PostgreSQL's system columns** (`xmin`, `xmax`,
  `cmin`, `cmax`, `ctid`, `tableoid`) is refused where the adapter opens, by
  PostgreSQL's own `CREATE TABLE` (a `Migration` refusal), though the memory
  adapter serves it: the adapter names each column as its field.
- **The platform's own tables are not held by the catalog at start**: a
  trigger or a rule another hand puts on `pw_outbox` or `pw_interactions` is
  met only at a commit (a dropped outbox row refuses it, finding 8).
- **The interaction's record forgets older ones in every command that
  records one** (the feed's way, `feed_pg.rs`), so two hosts' concurrent
  commands may conflict over it under serializable; not measured.
- **CockroachDB** (T5).

## Rulings (the integrator, 2026-11-02)

1. **The number** is ADR-0393 (reserved), and **the names** are kept: the
   crate `runtime/pw-table-postgres` and the recipe `e14-d3-postgres`, not
   ADR-0338's `server/src/adapters/postgres.rs` and `e14-postgres-adapter`
   (the author's question 3).
2. **Follow-ups, recorded here, not built by this ADR:**
   - **one adapter interface**: a trait the memory and PostgreSQL adapters
     both implement (the methods of decision 1), in a crate both use, after
     track `data-path` lands and its routing says which methods the host
     calls (the author's question 1);
   - **the memory adapter keeping a `Float` as a float**, type-exact at the
     boundary: a `Float` given as a JSON integer through the Rust API (`3`
     for `3.0`) is kept an integer there and answered a float here; the
     engine always gives a float, so no component sees it (question 2);
   - **the development server's layer over `PgTables`** (`data-path`'s);
   - **scheduled runs** (ADR-0334) in the adapter's transaction, and **the
     second host's reader of the outbox** (ADR-0369).

## Next

- The rulings' follow-ups, in their order: the server's `DataLayer` over
  `PgTables` with `data-path`'s routing (and the store's and the feed's
  server suites on it as D4 and D5 declare their data); the shared trait;
  the memory adapter's `Float`; scheduled runs and the outbox's reader in
  the adapter (the platform's remaining tables, ADR-0338 5.2).
- Each column named by the adapter (a field named as a system column
  served), and the platform's tables held by the catalog at start.
