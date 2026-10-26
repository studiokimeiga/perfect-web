# ADR-0386: a command writes its tables through the table gate, staged, and every read is gated

Status: accepted under the owner's delegation of 2026-10-02 and the standing
order of 2026-10-25 (decide by best judgement); the coordinator lands it
after an independent review. Date: 2026-10-25. Milestone: E14, the app
layer. Track: `d2-writes`. ADR-0338's D2, after ADR-0385's read path: the
writes, staged per command, and the reads other than `get` and `get-many`.
Builds on ADR-0338 (decision 1, 5.2, D2's controls), ADR-0350 (D1: the
generated functions and their types), ADR-0385 (the gate, link-time types,
answers held), ADR-0218 (a command's writes staged until it commits),
ADR-0208 and ADR-0209 (a command's events and invalidations, staged with
its writes), ADR-0325 (an interaction's record committed with them),
ADR-0099 (PW0618: a failure is handled) and ADR-0157 (a declared error
commits nothing).

## Context

- **After ADR-0385 a compiled component could read a declared table by its
  key, and nothing else.** D1 generates, for every table, the reads by a
  prefix of the key (`of_<fields>`), by each index (`by_<index>`), their set
  forms (`_many`), `all` of a bounded table, and the writes `insert`, `put`,
  `delete` and `delete_of_<fields>`; the gate refused every one of them, and
  the memory adapter's writes were reachable only from Rust, inside a
  closure holding its lock.
- **What D1 typed** (ADR-0350): `insert(row) -> Result<(), Row>` and, where
  the table has a unique index, `put(row) -> Result<(), Row>`, "whose error
  carries the conflicting row"; otherwise `put -> ()`; `delete(key) -> ()`;
  `delete_of_<p>(prefix) -> ()`; a unique index's read `Option<Row>`, every
  other read `List<Row>`, in D1's order (`sorted_by`).
- **What the platform's transactions are.** A command is one transaction
  (ADR-0218): the development server begins the layer's staging, runs the
  component with its operations, and commits the layer's rows, the outbox's
  events and invalidations (ADR-0208, ADR-0209), the interaction's record
  (ADR-0325) and its scheduled runs (ADR-0334) together; a declared error
  or a trap commits nothing (`command_held`). Within it the store's memory
  layer reads its own staged cart (`MemRows::cart` reads `staged` first),
  and on PostgreSQL every statement of the command runs in one transaction,
  where "each query does see the effects of previous updates executed within
  its own transaction, even though they are not yet committed" (PostgreSQL
  18, 13.2.2; 13.2.3: Serializable "works exactly the same as Repeatable Read
  except that it also monitors" for serialization anomalies; read
  2026-10-25). Its commit is two steps: `Staged::commit`, which may refuse,
  before the materializer's transaction, and `Staged::publish`, which may
  not, after it.
- **Found reading the read path for this:** the adapter's JSON rows hold
  none as `null`, so an `Option<Option<T>>` field's `Some(None)` came back
  `None`; and D1 admits two indexes on one field set, one unique and one
  not, whose plain read D1 types a list and orders as one row
  (`sorted_by` empty), which the adapter answered as one row.

## Decision

1. **A table's write links only through the gate, into a command.**
   `Manifest::admit_command` admits a command's reads, as
   `Manifest::admit_reads` admits a query's, and its writes, each carrying
   exactly its table's write capability (`database.write<Lists>`) and
   granted; a write into anything else refuses the whole component, named.
   The engine takes a call's kind from its contract's export
   (`export_kind`): a write links only into an export declared `command`
   (`gate_links(.., command)`); `call_within`, `instantiate*` and the rest
   link no table import, as before.
2. **Typed where it links, held when it answers.** `table_link::check_import`
   holds a write's import to tables.json: `insert` and `put` take the row
   record, `delete` the key's fields, `delete_of_<p>` the prefix's, each by
   its stored type; a refusable write (an `insert`, a unique table's `put`)
   answers `result<_, row>` and any other nothing. `check_answer` holds each
   answer: a write that cannot be refused answers nothing; a refusal's row
   is exactly the table's and holds what the write would: an `insert`'s, its
   key or a unique value of it; a `put`'s, another key's row holding a unique
   value of it. A written row reaches the adapter only exactly its table's
   (`table_selectors::decode_write_arguments`, the engine's `exact_row`).
3. **Staged per command.** A command's reads see its own writes, as a
   PostgreSQL transaction's do and the store's memory layer's do; no other
   read sees any of them until it commits: a query reads the published
   tables. One command writes at a time (serializable by running one at a
   time, as the development server's write guard already orders its
   commands, ADR-0352). Its writes commit all or nothing: a command dropped
   without committing (a declared error, a trap, a refused commit) is undone
   whole, its index and reference entries with it.
4. **A refused write is a value; a broken commit refuses the command.** A
   write refused by the row holding its key or a unique value changes
   nothing and answers that row as its `Err`, which PW0618 makes the program
   handle; returned as the command's declared error, nothing of the command
   commits. Each write is held to every unique index before anything
   changes, so no state, staged or committed, holds a unique value twice. A
   table's row bound is also held before each growth write changes any row,
   index, reference or touched-key map. A replacement at the bound is valid;
   to replace one key with another at capacity, delete first. Key/unique
   conflicts still answer the existing row before the resource guard. A
   resource refusal traps the write and leaves staging unchanged. References
   remain deferred until `Command::prepare`; prepare rechecks the bound as
   defense in depth. A broken commit publishes nothing. `all` answers no
   more rows than the bound, inside a command too.
5. **Prepare and publish in this standalone adapter.** `Command::prepare`
   checks the candidate and ends further writes; `Prepared::publish` must
   return `Result<Committed, Error>` and report internal storage failure.
   `Command::written` reports names; `Prepared::written` and `Committed`
   additionally report the resulting versions. A poisoned lock must never
   yield a successful commit. This supersedes the original infallible
   publication claim. **This API is not yet the platform's `Staged`**:
   that platform persists outbox/interaction records before infallible
   publication, so adding `?` afterward cannot give atomic rollback. Do not
   wire that sequence to this fallible prototype. Durable rows and platform
   records need one transaction, or a separately designed recoverable
   publication protocol, before the generic host integration can land.
6. **Which queries a write invalidates is what the command declares**
   (ADR-0007: invalidation is explicit). A table's write performs
   `database.write<T>`, so PW5106 refuses a command that writes `T` and
   neither invalidates each cached query reading `T` nor emits an event it
   listens for; app.pw's `make_list` hands `EveryList`, `ListAt(slug)` and
   `EveryItem` to the platform's invalidations before it writes (ADR-0209),
   which drops them once the writes commit.
7. **Every read is gated, typed and held.** A prefix's read and its set
   form, an index's and its set form, a unique index's (`option<row>`) and
   `all` link through the gate as `get` does; each answer's rows are of what
   it was asked (the prefix, the index's value), each key once, in D1's
   order; a unique index's one row is of the value asked; `all` within its
   table's bound.
8. **A manifest is whole for every primitive** (`Manifest::from_json`): each
   function reads by what its primitive reads by (the key; a proper prefix
   of it; one of the table's indexes; nothing for `all`, `insert`, `put`),
   under the name D1 gives it, in D1's order; an index is one of its
   fields, each once, no two on one field set; `all` only of a bounded
   table; a bound is at least one row; a key names each field once.
9. **The memory adapter keeps maps by key and by index** (ADR-0338 5.2):
   each table's rows by key, each index a map of its values to the keys
   holding them, each reference a map of the key it refers to; one mutation
   (`Data::set`) keeps them, everything that can fail done before anything
   changes. The writer holds a working copy, which only the open command
   reads; the published tables are behind an `Arc` per table, so a read
   takes them for an `Arc`'s clone, and a commit replays only the keys it
   changed (a table is copied only while a reader still holds it). A
   command's undo restores the rows it first touched. Both replays remove
   every key first, so no unique value is held twice between two steps.
10. **Not served, refused:** an `Option` of an `Option` (its `Some(None)`
    cannot be stored apart from its `None`), refused where it links, where a
    row is decoded, and where the adapter is made; two indexes on one field
    set, refused where the manifest is read.

## Alternatives

- **A command's writes invisible to its own reads**, as the feed's memory
  layer reads the state its `begin` took. Rejected: a transaction sees its
  own updates on PostgreSQL, the reference database, and in the store's
  memory layer; a command reading back what it wrote (`insert_then_get`)
  would see the table without its write, and an adapter would differ from
  the next.
- **A unique violation traps the command** (PostgreSQL's default: an error
  aborts the transaction). Rejected: D1 types `insert` and a unique table's
  `put` to answer the row that refuses them, so the program can handle it
  (insert or else read), and PW0618 makes it; PostgreSQL itself keeps the
  transaction where it is asked to: "ON CONFLICT DO NOTHING simply avoids
  inserting a row as its alternative action", and without a conflict target
  "conflicts with all usable constraints (and unique indexes) are handled"
  (`INSERT`, read 2026-10-25).
- **Unique values held at commit only.** Rejected: a write's refusal must
  name the row that refuses it when it is made, and PostgreSQL's
  non-deferrable unique constraints are held per statement too; a swap goes
  through a free value.
- **The bound held only at commit.** Rejected on integrator review:
  refusing `all` did not prevent arbitrarily many staged rows and index
  entries. The bound is a resource limit as well as a query contract. A
  command replacing keys at capacity must delete before inserting. This
  supersedes the original insert-before-delete ruling.
- **Stage in an overlay merged into every read.** Rejected for the working
  copy: one read implementation serves a command and a query, where an
  overlay needs a merged form of every primitive.
- **Copy the tables per command, or for every commit.** Rejected: each is
  proportional to the data; a command touches its own keys, and a commit
  replays only those.
- **Published tables behind a reader-writer lock.** Rejected: a render's
  snapshot (NEXT 4c) would hold the lock; an `Arc` is a snapshot for its
  clone.

## Evidence

`docs/evidence/E14/d2-writes.txt`, by `just e14-d2-writes`:

- **`pw-table-memory`, 64 tests**: `compiled_app.rs` 10 (app.pw's five
  queries through every read primitive, its four commands through every
  write primitive, a command's writes invisible until it commits and none
  published where it is not, PW5106 for a table's write; writer.pw's
  command reading its own writes before it commits, and one stopping after
  a write); `compiled_gate.rs` 5 (a write only into a command, a forged or
  ungranted write, a write typed otherwise, a write answering what
  tables.json says it does not, an answer not its write's);
  `compiled_reads.rs` 3 (the other reads typed and held, a manifest whole
  for every primitive, 16 refusals); `staged.rs` 11 (the adapter's
  staging, maps, unique values, bound, references, turns, ended commands,
  what a command wrote); and the read path's 35, unchanged in what they
  assert but for two fixtures brought to D1's order and `all`, now a read
  the gate admits.
- **`pw-host --features engine --lib`, 25** (the link-time types, answers,
  codecs and write callback); **`pw-cli --test tables`, 5** (the compiler's
  tables.json executed by the adapter).
- **`scripts/d2_writes_mutations.py`, 53 mutants**, each killed by a failing
  test, ADR-0338's two named controls among them ("a staged write visible
  before its commit", "an index not kept on a write", here two: a row's new
  values missed, and its old values kept). The first whole run killed 44 of
  45 ("a written row goes untyped where it links" survived: the refusal's
  row, the same record, refused it first), and the second 52 of 53 (an
  index named as an earlier one's set form); each got its case, and the
  record is a whole run.
- **`scripts/table_host_mutations.py`**, re-anchored where this generalized
  ADR-0385's checks (15 anchors), run whole again by `just e14-table-host`
  (`docs/evidence/E14/table-host.txt`).

## Not claimed

- **The development server serving a program's tables**: `from_build_layers`
  reading tables.json, a `DataLayer` over `MemoryTables` (`reads` its
  `read_host_fns`, `begin` its `Command`, `rows` its `written`, `commit` its
  `prepare`, `publish` its `publish`), and calls through `call_with_tables`.
- **A render's snapshot of the memory adapter**: the published `Arc` is one,
  not yet offered (`DataLayer::snapshot`).
- **The platform's records in the adapter's own transaction**: in memory,
  the host's materializer commits the outbox, the record and the runs with
  the rows a command reports, as for every memory layer.
- **The generated conformance suite** (ADR-0338 5.5) and the first app with
  no Rust data layer (the coordinator's ruling: the guide's guestbook
  tutorial, on the feed's Rust layer today).
- **Keys of nullary cases**, composite references, references across
  sources, and an `Option` of an `Option`: refused, not served.
- **Durability and performance**: memory is the reference adapter, lost
  with its process; no throughput or latency is measured here. What is
  stated is each primitive's work: a key's read a lookup, a prefix's or an
  index's a range of its map and a sort of the rows found, a write a lookup
  per index, a commit a replay of the keys it changed; the working copy
  doubles the maps (the rows themselves are shared).

## Questions for the integrator

- **D1 admits two indexes on one field set** (a `unique` and a plain
  `index`), and then types the plain one's read a list while ordering it as
  one row (`sorted_by` empty, `tables::sorted_by`'s "any unique index on
  these fields"); the adapter answered it one row. The host now refuses
  such a manifest where it reads it; should PW0361 refuse the declaration
  (D1's code, `compiler/pw-core/src/tables.rs`), so that it never builds?


## Review repair: exact stored float equality (2026-10-25)

A failing-first staged-transaction regression reproduced `put(-0.0)` after
`0.0` returning success without publishing the changed row: JSON equality
considered the rows equal, leaving working and published copies different.
Change detection now compares floating representations by bits, recursively,
while preserving exact integer comparisons. The regression checks isolated
staging, publication, change reporting, rollback and rejection of two NaN
payloads (nonfinite JSON becomes null and cannot enter the declared Float).
A control restores ordinary JSON equality. This closes F1 only; the other
contract, staging-bound and poisoned-lock review findings remain open.


### Review rulings: staged authority and resource bounds

`Command::host_fns` must require exactly one explicit `command` export,
matching the compiler's current one-export component contract. Missing,
query, mixed or ambiguous exports receive no staged callbacks, including
read-only callbacks. Query callbacks continue to read the published state.
The per-write bound ruling above precedes its implementation; references
remain deferred so related rows may still be written in either order.


Failing-first regressions reproduced both staged query access and growth
past the declared row bound. The repaired adapter passes all 67 memory
adapter tests (14 staged tests). Two new controls remove admission and
per-write growth checks. The former prepare-only and `all`-only bound
controls are now equivalent under the earlier growth guard and are removed
from the catalog, without crediting kills; those checks remain in production
as defense in depth. The D2 mutation producer now requires a complete
native named inventory and assertion-site failures, refuses build/crash
failures, and restores source with the shared mutation guard. Historical
53-control evidence predates these changes; the revised whole campaign
and independent review remain required. Poison/waiter/publication findings
remain open.


### Review ruling: waiters, callbacks and storage failure

A wait observer is cloned under its own mutex and invoked with no storage
or observer lock held. Begin reacquires the working lock and rechecks the
open command before atomically waiting, preventing a lost wakeup. Internal
working/published poison fails the database closed for reads and writes,
wakes every waiter and returns `StoragePoisoned`. Working-lock ownership
must notify on unwind after releasing its guard; an external observer panic
must leave database locks and health intact. These failure semantics precede
the implementation. The standalone fallible publish API above deliberately
withdraws any claim of platform outbox atomicity.

The five failing-first storage/observer regressions all failed at native
assertions on the prior implementation. The repair passes all 75 adapter
tests, including eight failure/publication tests. Six new controls each
compile and produce native assertion failures against the frozen eight-test
failure inventory. Internal injected panics are suppressed only for the
exact reserved injection payload on the named fault thread; an unexpected
panic still reaches the ordinary hook and cannot earn an assertion kill.
The lease releases its mutex before marking failure and notifying waiters.
Prepare/publish failures never report `Committed`; a prepared command
unpublished on drop restores its rows and releases its turn. These local
results are bounded proofs, not a claim that the revised whole campaign,
independent final review or platform transaction integration has completed.