# ADR-XXXX: an app with no Rust data layer: its declared tables served through the gate

Status: accepted under the owner's delegation of 2026-10-02 and the standing
order of 2026-10-25 (decide by best judgement); the coordinator lands it
after an independent review. Date: 2026-11-02. Milestone: E14, the app
layer. Track: `data-path`. ADR-0338's direction made real for one app:
"an app's data declared in Pleris, served by one generic adapter per
database, no app-specific Rust" (its ruling 1), after ADR-0350 (D1, the
declaration), ADR-0385 (the reads through the gate) and ADR-0386 (the
writes, staged). Builds on ADR-0208, ADR-0209, ADR-0218, ADR-0325 (a
command's records committed with its writes), ADR-0263 and ADR-0270 (a
reader's handle, an id that grants nothing), ADR-0384's H1 (`id_of`) and
NEXT 4c (a render reads one state).

## Context

- **No program could be served from its declared tables.** ADR-0385 and
  ADR-0386 proved the gate, the types and the staged adapter with compiled
  components in `pw-table-memory`'s tests; the development server picked a
  data layer by its contracts' prefix (`feed:`, `kiokun:`, the store's), so
  a `.pw` program declaring `table Entries` had nowhere to run. ADR-0386
  named what remained: the routing, "authorized commands using tables"
  (`call_with_tables` refused any `requires`, and `call_authorized_within`
  links no table import), a render's snapshot, and its decision 5's demand:
  the adapter's publication may fail, so a host that commits the platform's
  records and then publishes would keep records of writes no reader sees;
  "durable rows and platform records need one transaction, or a separately
  designed recoverable publication protocol, before the generic host
  integration can land".
- **The guide's guestbook ran on the feed's Rust layer**: its type `Post`,
  its `feed:data/posts#timeline`, `#publish`, `#delete`, `users#of-session`,
  `users#viewer`, the feed's seeded post, and `OwnsPost`, a predicate the
  deployment answers by reading the feed's posts.
- **A program with no Rust could not name its reader** (ADR-0384, gap 1):
  `current_user()` is a handle whose representation only `capability`
  reads; the reader's id and name came from the feed's layer.
- **The conformance suite** (ADR-0338 decision 5.5, ruling 7: one suite,
  written once against the `DataLayer` seam) did not exist.

## Decision

1. **A program whose contracts import a table's function is served from
   its build's `tables.json` by the generic memory adapter**
   (`spikes/own-renderer/server/src/tables.rs`, `TablesLayer`, a
   `DataLayer` over `MemoryTables`). No operation is supplied by name; the
   layer grants each table's read and write; a build with no `pages`
   directory (a program of queries and commands) is served.
2. **The gate is the only path, per component.** Every call of such a
   program goes through `call_authorized_with_tables`, its host map the
   platform's operations and the functions the gate admits for that
   component's contract and grant alone (`DataLayer::table_reads` for a
   query, `Staged::table_ops` for a command; `Server::run` is given the
   call's table source). A component that reads no table links none.
3. **A guarded command answers its predicates before its tables link**
   (`pw_host::engine::call_authorized_with_tables`): arguments held,
   `requires` evaluated by the deployment, and only then the table
   functions linked through the gate; refused, nothing runs and nothing is
   staged. This closes ADR-0386's "authorized commands using tables".
4. **A command's rows and its records are one transaction.** Every refusal
   its tables can make (a row bound, a missing reference) is made before
   the materializer's transaction (`Staged::commit` is the adapter's
   `prepare`); its rows are published as that transaction's last step
   (`Staged::publish_within`, new and defaulted to nothing), after its
   state rows, its interaction's record (ADR-0325) and its runs
   (ADR-0334) are written, before it commits. An error there (the
   adapter's storage failed closed) rolls them all back: no record is kept
   of a write no reader sees, and no write is published without its
   records. The materializer's own COMMIT and outbox insert stay
   infallible (`expect`), as before, on an in-memory SQLite. This is the
   "one transaction" ADR-0386 decision 5 asked for, in memory; D3 makes it
   one PostgreSQL transaction.
5. **One render reads one state of the tables** (NEXT 4c):
   `MemoryTables::snapshot` (the published `Arc`'s clone, copying nothing)
   is the layer's `DataLayer::snapshot`, and each query of a render reads
   it (`MemoryTables::read_host_fns_at`), counted and merged as the layer's
   reads are; a snapshot another adapter took is refused.
6. **One layer a program**: a program importing a hand-written layer's
   operation beside its tables is refused at start, by name (T1 routes
   several layers in one host, ADR-0338 5.1).
7. **The reader's account, the platform's**:
   `context.account_of(User<UserId>) -> Option<Account>`, `Account { id:
   UserId, name: String }` (`pw:host/principal#account`): the id the
   reader's handle names (what ADR-0384's H1 calls `id_of`, built early)
   and the name their sign-in provider gave them, for the reader's own
   handle alone (another handle is refused), and none where no one signed
   in to an account: no one, or a guest, whose user's id holds its session
   (see the first finding). The answer is labelled as the handle it was
   read from, so a shared cache refuses it (PW5001).
8. **The guestbook declares its data** (`examples/guestbook`): `Books`
   (key `name`; `signed`, every entry ever signed: the sequence, so a
   removed entry's number is never given again) and `Entries` (key `book,
   number`, `refers book to Books`, its text stored as `Words`), read and
   written only through the functions the compiler writes for them. `Me`
   and `Latest` are `user` queries and the page a `user page`: `Latest`'s
   rows carry `mine`, computed with the reader's id, so they are the
   reader's, and a page whose handler holds one must name its reader
   (PW5007). Who may remove an entry is the program's own code, in the
   command's transaction (`OwnsPost` was the feed's Rust); `requires
   SignedIn` stays the deployment's. The tutorial teaches it, byte for
   byte the app; `tour-server.md` gains "Tables" and `why.md` no longer
   says a program cannot declare its storage.
9. **The conformance suite** (`spikes/own-renderer/server/src/tests/
   conformance.rs`): it reads tables.json itself, generates rows for every
   table (a key's leading fields from a few values, a unique index's one a
   row, a reference one of its table's keys; a stored type it generates no
   value of fails it, named), drives the layer through the seam the server
   uses (`table_reads`, `table_ops`, the server's own `commit_staged`), and
   holds each answer to an oracle's (the rows written, filtered and
   ordered by D1's `sorted_by`). Its parts that apply to memory: answers
   (every read function: present keys, a missing one, set forms given
   duplicates, misses and nothing) at 3 and 24 rows a table; costs (each
   read one call of the layer at both sizes, within its source's `sets`;
   each of the programs' queries pinned to one call of each table function
   at both sizes); staging (a write read by its command alone until it
   commits); refused commits (dropped, refused by a missing reference or
   by its publication: nothing kept); idempotency (each `idempotent_by`
   command, sent again, answered as it was, writing nothing more). Run for
   the guestbook's tables and the lists app's (`examples/tables`).
10. **The development server links `pw-core`** through `pw-table-memory`,
    which reads tables.json as the compiler's `TablesJson` and its schema
    hash. ADR-0018 has a runtime not depend on the compiler; the adapter
    reading the host's mirror (`pw_host::tables`) takes the debt away. Not
    done here: it is the adapter's crate boundary, not the routing's.

## What is proven

- **A real app runs with no Rust of its own**: the guide's guestbook (sign
  up, sign, an entry shown before the server answers and then as it wrote
  it, removal by its author alone, every open page told without a reload)
  is served from the tables it declares, through the gate, in Chromium,
  Firefox and WebKit; its tutorial is the app, byte for byte.
- **The gate is the only path and each component holds its own**: every
  table function a call links is the gate's for that component; a guarded
  command links nothing until its predicates answer.
- **A command commits whole**: its rows, its interaction's record, its
  events and its runs in one transaction, refused together where any part
  is; a staged write read by its command alone; a render reading one state.
- **The adapter answers as an oracle does** for every read function of
  every table of two programs at two sizes, one call each, and a retried
  interaction is answered once.

## Alternatives

- **Publish the rows after the materializer commits** (the hand-written
  memory layers' order, `Staged::publish`). Rejected: the adapter's
  publication can fail (its storage failed closed), and the records would
  then be kept for writes no reader sees (ADR-0386 decision 5).
- **Hold the published lock from `prepare` to the commit**, so the
  publication cannot fail. Rejected: readers would wait on a materializer's
  transaction, and a render that held the materializer's lock while it
  read a table would deadlock against it.
- **Hand every component the whole manifest's functions** (as a
  hand-written layer's `reads` hands every query every operation).
  Rejected: the gate links only the admitted ones anyway, and a host map
  holding more is a leak waiting for a second linker.
- **Keep `OwnsPost` and teach it to read the guestbook's table.** Rejected:
  that is app-specific Rust in the deployment; a predicate written in
  Pleris is H2's.
- **A `session page` for the guestbook.** Refused by PW5007: a row
  computed with the reader's id is `User<UserId>`, and a session-scoped
  manifest may not hold it.
- **A pure `id_of` in `capability.pw`** (`UserId(user.value)`, as H1
  sketched) **and a separate `name_of`**. Built first, and rejected: a
  guest's and a signed-out reader's user id is `u-` and its session id, the
  session cookie's value, so a pure `id_of` handed any program the
  credential as a writable value. A host's `id_of` answering a derived id
  for a guest was rejected too: a guest's id from it would not be the key
  its handle-keyed entries are kept by. One answer, an account or none,
  read once, keeps the id and the name of one principal together.
- **Number entries by counting the entries there are.** Rejected: a removed
  entry's number would be given again, to another entry, while a page still
  shows the first.

## Evidence

`docs/evidence/E14/data-path.txt`, by `just e14-data-path` at `ea192e1b`
(a clean tree):

- Server, `pw-dev-server`, 18: `tests::tables` (7), `tests::guestbook`
  (6), `tests::conformance` (4: the suite for both programs, 111 reads of
  the guestbook's tables and 164 of the lists app's checked against the
  oracle; idempotency; the query pins), the account operation's unit test.
- Engine and adapter: `pw-table-memory --test compiled_guarded` (3),
  `--test compiled_snapshot` (2).
- Compiler: `pw-core --test handles` (5, the reader's account labelled),
  `--test platform_contracts` (4, the platform's hash, moved for `Account`
  and `account_of`); `pw-cli --test guide` (15: the tutorial is the app).
- `scripts/data_path_mutations.py`, whole, D2's strict producer (a kill is
  a test failing at its assertion, each binary's inventory frozen at the
  baseline of 28): **25 of 25 mutants killed**. Two earlier whole runs are
  history, not the record: the first (before the account change) read 24
  of 26, two INVALID (a `debug_assert!` in the layer's `publish`, and an
  assertion inside a render's hook on the server's thread, each a panic
  site outside the tests; both repaired); the second 24 of 25 (one anchor
  `cargo fmt` had joined onto one line; re-anchored, killed alone).
- The guestbook's browser spec (`e2e/guestbook.spec.mjs`, 4 tests), 12 of
  12 in Chromium, Firefox and WebKit, run locally; `just e14-guide` runs it.
- Six mutants of other tracks' scripts re-anchored where this track moved
  their code (committed_events, computed_holes, guide, notifications (2),
  render_merge, session_command); each script reached is run whole.

## Not claimed

- **Durable storage**: the memory adapter's tables are lost with its
  process; the platform's records in the materializer likewise. D3 (the
  PostgreSQL adapter: migrations from tables.json, statements, set reads,
  the platform's tables, the rows and the records in one PostgreSQL
  transaction) is next.
- **Row ownership and privacy of rows** (ADR-0384's H1): a table has no
  `owner`; who may write a row is the program's code, and a generated read
  takes plain ids. **Predicates in Pleris** (H2): `SignedIn` is the
  deployment's. The dating app (ADR-0384) waits for both.
- **Paged reads** (H4): `Entries.of_book` reads every entry; the guestbook
  reads all and takes the newest `limit`.
- **Seeds, migrations applied, drift refused**: tables start empty; the
  build's `schema.lock` is not held by the server.
- **The suite's parts that need a database** (ADR-0338 5.5's guarantees,
  anomalies, delivery to a second host) and a second host.
- **A program of several layers** (T1), and the store's and the feed's
  data declared (D4, D5).
- **The sequence's contention**: every signing reads and writes the book's
  row, which serializes signings; fine in memory, a hot row on PostgreSQL.

## Findings

- **CORRECTION (security) of the platform's guest identity**: a guest's
  user id, and a signed-out reader's, is `u-` followed by its session id
  (`identity.rs`'s `Principal::guest` and `user_of`; `feed.rs`'s
  `user_of`), and the session id is the `pw-session` cookie's value. ADR-0263
  holds that "an id is the program's, and grants nothing"; this one is a
  credential. `account_of` therefore answers none for a guest or no one
  (decision 7), and a program with no Rust can name only an account. Where
  else it
  reaches others is the identity track's to audit: the feed names a guest
  by the first ten characters of its id (`Guest s-1234abcd`, 32 bits of the
  session), and its rows' author ids are the guests' ids.
- ADR-0386's open item "authorized commands using tables" is closed
  (decision 3).
- A template value computed from two values (`mine` and a row) is refused
  (ruling 0073-a), so a per-reader control on a public row needs the row
  computed for the reader.
- `clock.now()` has the body `0` (no host binding); a program holds no
  instant as a value, so a row's order comes from a sequence it keeps.

## Questions for the integrator

- The guest identity (finding 1): derive a guest's user id from its session
  without revealing it (a hash), everywhere, as the identity track's change.
- Move `pw-table-memory` onto the host's mirror of tables.json (decision
  10), before D3 adds a second adapter that the dev server links.
