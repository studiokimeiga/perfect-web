# ADR-XXXX: an app with no Rust data layer: its declared tables served through the gate

Status: accepted under the owner's delegation of 2026-10-02 and the standing
order of 2026-10-25 (decide by best judgement); the coordinator lands it
after an independent review. Date: 2026-11-02. Milestone: E14, the app
layer. Track: `data-path`. Amended the same day after its independent
review, by the integrator's rulings on its findings (the corrections
below, which come first). ADR-0338's direction made real for one app:
"an app's data declared in Pleris, served by one generic adapter per
database, no app-specific Rust" (its ruling 1), after ADR-0350 (D1, the
declaration), ADR-0385 (the reads through the gate) and ADR-0386 (the
writes, staged). Builds on ADR-0208, ADR-0209, ADR-0218, ADR-0325 (a
command's records committed with its writes), ADR-0263 and ADR-0270 (a
reader's handle, an id that grants nothing), ADR-0129 (a value's label
follows it), ADR-0384's H1 (`id_of`) and NEXT 4c (a render reads one
state).

## Corrections: the independent review of 2026-11-02

The review reproduced each finding with a probe test against the track's
head (`28d24310`); each probe is now a regression of the suite, failing on
that head and passing here, beside a valid neighbour and a negative
control, and each repair has a mutant its test kills. The integrator's
rulings, and what changed:

1. **B1, a blocker: `account_of` handed a program the session credential
   when the platform ran a command on a reader's behalf.** `account_of`
   decided "a guest" by the principal's issuer being `guest`. A scheduled
   run's stand-in principal (`identity::on_behalf`, issuer `clock`, no
   name; a payment's settlement's, issuer `payments`) carries the user id
   of whoever scheduled it, which for a guest or a signed-out reader was `u-`
   and the `pw-session` cookie's value: written into a row, another session
   read `u-sess-SECRET…`. An account's run was answered the name `""`.
   **Rulings 1 and 2, and what changed**:
   - **A guest's identity is never its session.** `Principal::guest` made
     its user id, handle and display name from the session itself (`u-{s}`,
     `@{s}`, `Guest {s}`), and so did `Principals::user_of` for a signed-out
     reader, `feed::user_of`, and the uploads' budget key. All are now made
     from a one-way, domain-separated digest of the session
     (`identity::guest_user`: `u-` and 128 bits, in hex, of SHA-256 over
     `pw-guest-v1:` and the session; `sha2`, already the server's), the
     handle and name from its first ten digits, as the feed names a guest it
     knows no profile of. The domain keeps it apart from the digest a
     session is kept by (`kept_hash`, SHA-256 of the session alone). From an
     id no one recovers the session or makes another session's id. **This
     corrects ADR-0220's guest, named for its session**, and with it every
     place a guest's id reached: rows' authors, scheduled runs' users, a
     payment's owner (which the payment provider keeps as metadata, as
     Stripe's would), notifications, follows, carts and orders.
   - **`account_of` answers from the platform's account record**
     (`notifications::account`): an account exists only for a user signed
     in to one. A reader's own session answers the account a sign-in opened
     it for, and none for no one and none for a guest. A session the
     platform opens on a reader's behalf (`identity::ON_BEHALF`) is never an
     account of its own: it answers the account its reader signed in to, as
     the platform records it (`Principals::account_name`, written by every
     sign-in, `Principals::sign_in`, and kept past a sign-out), which is the
     account, id and name, a call of theirs answers; none for a guest's or a
     signed-out reader's run.
2. **B2, a blocker: a reader's handle could be stored in a table and served
   to every reader.** `pw check` took a row field `who: User<UserId>`; the
   table's boundary keeps no label, so PW5001 never fired, and the stored
   value was `u-sess-SECRET…`. **Ruling 3, and what changed**: a row that
   holds a session's, a user's or an organization's handle, wherever its
   field's type holds it (an `Option`, a `List`, a record's field, a case's
   payload), is refused by the compiler, **PW6901** `row_holds_handle`, in
   this track's own block (PW69xx, `Owner::DataPath`; registered in the
   diagnostics guide), at the row; and again where the server starts
   (`tables::servable`, defence in depth): a tables.json whose field's type
   names a handle (`User<…>`, bare or `capability.`'s) refuses the start,
   the table, the field and the handle named. A program records an author
   by their account's id, as the guestbook does.
3. **S1: a command's rows were visible before its outbox insert and its
   COMMIT.** The layer published inside the materializer's closure, and the
   outbox insert (`.expect`) and the COMMIT (`.expect`) came after it: with
   the outbox refusing inserts (the review's SQLite trigger) the command
   panicked, another reader read the entry, and its interaction's record and
   its event were lost. **Ruling, and what changed**: a command's
   transaction is now `Materializer::command_then`, which writes the
   command's state, records and runs, then its outbox, each refusal there
   answered and rolled back (never a panic), then runs the layer's
   publication as the last step, immediately before the COMMIT. A COMMIT
   that fails after publishing is rolled back and told
   (`Staged::unpublished`), and the tables fail closed
   (`MemoryTables::fail_closed`): every read and write of them is refused
   from then on, so no reader goes on reading a write whose records were
   lost; the host says so in its log. A publication cannot be taken back
   from readers who may already have read it, so the host stops serving
   the tables rather than undo it. The review's trigger is a test of the
   suite, and so is a COMMIT that fails after publishing (a deferred foreign
   key the outbox's trigger breaks, which SQLite holds at COMMIT).
4. **S2: the conformance suite's cost check could not fail, and the suite
   was not adapter-neutral.** It counted the calls of a wrapper the test
   itself called once (a layer calling the adapter three times for each
   read passed all 17 tests), and refused a commit of a table with no
   reference by a hook in the memory layer (`FAIL_NEXT_PUBLISH`), which no
   other adapter has. **Ruling, and what changed**: the suite counts at the
   seam, by the adapter's own count of the statements it ran
   (`DataLayer::statements`, `MemoryTables::statements`, each primitive
   one): a read is at least one statement and at most what its source's
   `sets` states; the program's query pins count the adapter too. Its
   refusals are ones any adapter must give, provoked through what the
   tables declare: a row referring to no row, a row deleted while another
   refers to it, a table written past its bound; each refused at its write
   or its commit, whichever the adapter refuses at, and nothing of the
   command kept. And the suite takes the layer under test as a parameter
   (`conformance::Layer`, `Server::from_build_tables`), so the PostgreSQL
   layer runs the same suite (ADR-0338's ruling 7). Two mutation controls:
   a read made three times, and an adapter that counts no statement.
5. **S3: the guestbook's page stopped working at tens of thousands of
   entries.** `Latest` reads every entry (`Entries.of_book`): 16,000 took
   274 ms (debug), 48,000 exhausted the component's 16 MiB, and any reader
   with an account could sign without limit. **Ruling, and what changed**:
   `Entries` is `bounded 1000`; `sign` refuses the 1,001st with `Full`
   before it writes (a declared error, the program's own words, which the
   page says), and the table would refuse it anyway. The page at the bound
   is measured in a test: rendered in 25 ms, `Latest` read whole (all 1,000
   entries) in 28 ms, on a debug build, each within the query's 2 s. The
   real fix, a descending read of the latest N by cursors, is ADR-0384's H4
   (Next).
6. **S4, process: the evidence was before the master merge, and two
   claims had no evidence file.** **Ruling, and what changed**: `just
   e14-data-path` records everything this ADR claims, the guestbook's
   browser spec in three engines among it (it was run locally, and its
   "12 of 12" recorded nowhere); the integrator produces the record from CI
   at the track's final head. The whole runs of other tracks' scripts that
   this track re-anchored are their recipes' records, which the verify run
   at that head plans (Evidence; a question below).
7. **The review's notes, ruled**:
   - **a.** In the guest model the composer was shown (a guest holds
     `SignedIn`), signing answered `SignedOut`, and the handler dropped it
     silently. Now the composer is shown only to a reader with an account
     (`<div hidden={!me.signed_in}>`), and every refusal of `sign` and
     `remove` is said where the composer is (`refused`, `said`), since the
     guide teaches this app.
   - **b.** A program of tables declaring an `upload` was served, its
     images kept nowhere (the tables' layer keeps no blob). It is refused at
     start, the upload named (`tables::servable`).
   - **c.** `guestbook.sh` is executable again.
   - **d.** Recorded as found, not changed: a render may read a commit's
     rows before its queries are invalidated (the rows are published within
     the transaction, its invalidations delivered after the COMMIT, as for
     every layer); and `pw-core`, the compiler, is the server's dependency
     twice over, its tests' (to build programs) and now its release
     binary's, through `pw-table-memory` (decision 12). The S1 repair
     changes neither.

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
   `DataLayer` over `MemoryTables`), or by the tables layer a test hands
   the server (`Server::from_build_tables`). No operation is supplied by
   name; the layer grants each table's read and write; a build with no
   `pages` directory (a program of queries and commands) is served.
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
4. **A command's rows and its records are one transaction, its rows
   published last** (amended by correction 3). Every refusal its tables can
   make (a row bound, a missing reference) is made before the
   materializer's transaction (`Staged::commit` is the adapter's
   `prepare`). In it (`Materializer::command_then`) its state rows, its
   interaction's record (ADR-0325), its runs (ADR-0334) and its outbox are
   written, each refusal answered and the transaction rolled back; then its
   rows are published (`Staged::publish_within`), immediately before the
   COMMIT, and a publication refused rolls it all back. No record is kept
   of a write no reader sees, and no write is published without its
   records written. A COMMIT that fails after the publication is rolled
   back and the tables fail closed (`Staged::unpublished`). This is the
   "one transaction" ADR-0386 decision 5 asked for, in memory; D3 makes it
   one PostgreSQL transaction.
5. **One render reads one state of the tables** (NEXT 4c):
   `MemoryTables::snapshot` (the published `Arc`'s clone, copying nothing)
   is the layer's `DataLayer::snapshot`, and each query of a render reads
   it (`MemoryTables::read_host_fns_at`), counted and merged as the layer's
   reads are; a snapshot another adapter took is refused.
6. **What a build's tables may be at all, whichever layer serves them**
   (`tables::servable`, refused at start, named): one layer a program (a
   program importing a hand-written layer's operation beside its tables; T1
   routes several layers in one host, ADR-0338 5.1); no row's field of a
   handle's type (PW6901 again, correction 2); no `upload` (note b).
7. **The reader's account, the platform's** (amended by correction 1):
   `context.account_of(User<UserId>) -> Option<Account>`, `Account { id:
   UserId, name: String }` (`pw:host/principal#account`): the id the
   reader's handle names (what ADR-0384's H1 calls `id_of`, built early)
   and the name their sign-in provider last gave them, for the reader's own
   handle alone (another handle is refused); none where no one signed in to
   an account: no one, a guest, or the guest or signed-out reader a
   platform-run command is run for. A command the platform runs on an
   account's behalf (a scheduled run, a payment's settlement) is answered
   that account, as a call of theirs is. The answer is labelled as the
   handle it was read from, so a shared cache refuses it (PW5001).
8. **A guest's identity is its session's one-way digest** (correction 1):
   `identity::guest_user`, the id, handle and name of the guest model's
   guest and of a signed-out reader, wherever the server makes one.
9. **A row holds no handle** (PW6901, correction 2): the compiler refuses
   it at the row, the host at start.
10. **The guestbook declares its data** (`examples/guestbook`): `Books`
    (key `name`; `signed`, every entry ever signed: the sequence, so a
    removed entry's number is never given again) and `Entries` (key `book,
    number`, `refers book to Books`, `bounded 1000`, its text stored as
    `Words`), read and written only through the functions the compiler
    writes for them. `Me` and `Latest` are `user` queries and the page a
    `user page`: `Latest`'s rows carry `mine`, computed with the reader's
    id, so they are the reader's, and a page whose handler holds one must
    name its reader (PW5007). Who may remove an entry is the program's own
    code, in the command's transaction (`OwnsPost` was the feed's Rust);
    `requires SignedIn` stays the deployment's. Signing needs an account
    (`SignedOut` otherwise) and room (`Full` at the bound, correction 5);
    the composer is a reader's with an account, and each refusal is said
    on the page (note a). The tutorial teaches it, byte for byte the app;
    `tour-server.md` gains "Tables" and `why.md` no longer says a program
    cannot declare its storage.
11. **The conformance suite** (`spikes/own-renderer/server/src/tests/
    conformance.rs`, amended by correction 4): it reads tables.json itself,
    generates rows for every table (a key's leading fields from a few
    values, a unique index's one a row, a reference one of its table's
    keys; a stored type it generates no value of fails it, named), drives
    the layer under test, which it is given (`conformance::Layer`), through
    the seam the server uses (`table_reads`, `table_ops`, the server's own
    `commit_staged`), and holds each answer to an oracle's (the rows
    written, filtered and ordered by D1's `sorted_by`). Its parts that
    apply to every adapter: answers (every read function: present keys, a
    missing one, set forms given duplicates, misses and nothing) at 3 and
    24 rows a table; costs (each read's statements of the adapter, counted
    below the layer, at least one and within its source's `sets`; each of
    the programs' queries pinned to one call of each table function and
    one adapter statement a call, at both sizes); staging (a write read by
    its command alone until it commits); refused commands (dropped;
    referring to no row; deleting a row another refers to; past a bound:
    nothing kept); idempotency (each `idempotent_by` command, sent again,
    answered as it was, writing nothing more). Run for the guestbook's
    tables and the lists app's (`examples/tables`), on the memory adapter.
12. **The development server links `pw-core`** through `pw-table-memory`,
    which reads tables.json as the compiler's `TablesJson` and its schema
    hash. ADR-0018 has a runtime not depend on the compiler; the adapter
    reading the host's mirror (`pw_host::tables`) takes the debt away. Not
    done here: it is the adapter's crate boundary, not the routing's.

## What is proven

- **A real app runs with no Rust of its own**: the guide's guestbook (sign
  up, sign, an entry shown before the server answers and then as it wrote
  it, removal by its author alone, every open page told without a reload, a
  refusal said where the composer is) is served from the tables it
  declares, through the gate, in Chromium, Firefox and WebKit; its tutorial
  is the app, byte for byte; it keeps at most its bound of entries, its
  page measured at it.
- **The gate is the only path and each component holds its own**: every
  table function a call links is the gate's for that component; a guarded
  command links nothing until its predicates answer.
- **A command commits whole**: its rows, its interaction's record, its
  events and its runs in one transaction, its rows published after every
  record and its outbox, refused together where any part is, and the tables
  closed where the COMMIT fails after their publication; a staged write
  read by its command alone; a render reading one state.
- **No program, page or table is handed a session**: a guest's id, handle
  and name are its session's digest; `account_of` answers an account a
  sign-in opened, never a guest's and never a stand-in's, a platform-run
  command its reader's; no row holds a handle.
- **The adapter answers as an oracle does** for every read function of
  every table of two programs at two sizes, each read within its stated
  cost counted at the adapter, every table refused as any adapter must, and
  a retried interaction is answered once.

## Alternatives

- **Publish the rows after the materializer commits** (the hand-written
  memory layers' order, `Staged::publish`). Rejected: the adapter's
  publication can fail (its storage failed closed), and the records would
  then be kept for writes no reader sees (ADR-0386 decision 5).
- **Publish inside the materializer's closure, before its outbox** (this
  ADR's first form). Rejected by the review: an outbox refused after the
  publication left a write read by others with no record and no event.
- **Undo a publication whose COMMIT failed.** Rejected: the rows were
  readable from the publication, and a reader may have read them; undoing
  would also have to undo what a command begun since read. Failing closed
  stops every reader at once; the failure is a broken store's (an
  in-memory SQLite's COMMIT), not a refusal a program can make.
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
  sketched) **and a separate `name_of`**. Built first, and rejected: then a
  guest's user id held its session, and a pure `id_of` handed any program a
  handle's value as a writable id. One answer, an account or none, read
  once, keeps the id and the name of one principal together.
- **Record each scheduled run's and payment's account with it**, so a
  stand-in answers it from its own record. Rejected for now: it changes the
  runs' and payments' schemas in memory and on PostgreSQL, other tracks'
  tables; the platform's account record answers the same where the reader
  signed in on this host (Not claimed).
- **Count the entries the guestbook keeps in its book row**, for an O(1)
  `Full`. Rejected: a count kept beside the rows is one more thing a
  removal must keep right; `Entries.all()` reads at most the bound, which
  `Latest` reads anyway.
- **Number entries by counting the entries there are.** Rejected: a removed
  entry's number would be given again, to another entry, while a page still
  shows the first.

## Evidence

`docs/evidence/E14/data-path.txt`, by `just e14-data-path`, which records
everything this ADR claims; the integrator produces the record from CI at
the track's final head (the review's S4). It runs:

- **The server**, `pw-dev-server`, 32 tests: `tests::tables` (10: routing,
  a run on a reader's behalf for a guest, a signed-out reader and an
  account, a row of a handle's type refused at start, an upload refused),
  `tests::guestbook` (10: the outbox's refusal, a COMMIT failing after the
  publication, the bound measured, the composer an account's), the suite
  (4: both programs, 113 reads of the guestbook's tables and 164 of the
  lists app's against the oracle, each table's refusals; idempotency; the
  query pins), `notifications::tests` (7: a guest's identity, the account
  operation, a stand-in's account), `tables::tests` (1: a handle named in
  tables.json).
- **The engine and adapter**: `pw-table-memory --test compiled_guarded`
  (3), `--test compiled_snapshot` (2); **the platform's transaction**:
  `pw-materialize`'s `command_then` (1: its last step after its records and
  outbox, a COMMIT failing after it told).
- **The compiler**: `pw-core --test handles` (6: PW6901 among them),
  `--test platform_contracts` (4), the registry's block test; **the
  guide**: `pw-cli --test guide` (15: the tutorial is the app) and the
  reference of codes current.
- **The guestbook in three engines** (`e2e/guestbook.spec.mjs`, 5 tests,
  15 runs: a refusal said among them).
- **`scripts/data_path_mutations.py`, 42 mutants**, whole, D2's strict
  producer (a kill is a test failing at its assertion, each binary's
  inventory frozen at the baseline).

Other tracks' scripts this track's changes reach are each run whole by
their own recipes, and recorded there, not here: the verify run at the
track's head plans them (`scripts/ci_plan.py --changed origin/master HEAD`
planned 158 recipes at the repairs' last commit, the track's whole range).
Re-anchored by these repairs: `committed_events` (`e10-committed-events`,
`e14-command-events`), `session_command` (`e14-session-command-authority`)
and `guide` (`e14-guide`, `e14-guide-producer`; it also gains the control
"a refused signing is said nowhere"). Their tests changed for a guest's
id: `e14-feed`, `e14-feed-postgres`, `e14-follows`, `e14-identity`,
`e14-notifications`, `e14-render-merge`, `e14-store-postgres`,
`e14-uploads`. Re-anchored by the track's first form, and in the plan
again: `computed_holes` (`e14-computed-holes`), `notifications` and
`render_merge`.

## Not claimed

- **Durable storage**: the memory adapter's tables are lost with its
  process; the platform's records in the materializer likewise. D3 (the
  PostgreSQL adapter: migrations from tables.json, statements, set reads,
  the platform's tables, the rows and the records in one PostgreSQL
  transaction) is next.
- **A durable account record**: the platform's record of accounts is this
  host's memory. A command run on an account's behalf on a host where that
  reader never signed in, or after a restart, is answered none (never
  another's account, never a credential) until they sign in there again.
- **Undoing a publication**: a COMMIT failing after the rows were published
  closes the tables for the life of the process; a reader who read in the
  instant between the publication and the failed COMMIT read a write whose
  records were lost (the instant is the COMMIT of an in-memory SQLite).
- **Row ownership and privacy of rows** (ADR-0384's H1): a table has no
  `owner`; who may write a row is the program's code, and a generated read
  takes plain ids. **Predicates in Pleris** (H2): `SignedIn` is the
  deployment's. The dating app (ADR-0384) waits for both.
- **Paged reads** (H4): `Entries.of_book` reads every entry, and the
  guestbook is bounded at 1,000 so that it stays readable whole.
- **A handle inside a record type, at the host**: tables.json writes a
  field's type as the row's record writes it (a record's name), so the
  host's check sees a handle named in a field's type; one inside a record
  type is the compiler's to refuse (PW6901 is).
- **Seeds, migrations applied, drift refused**: tables start empty; the
  build's `schema.lock` is not held by the server.
- **The suite's parts that need a database** (ADR-0338 5.5's guarantees,
  anomalies, delivery to a second host) and a second host; the suite has
  run on memory alone (D3 runs it on PostgreSQL through its parameter).
- **A program of several layers** (T1), and the store's and the feed's
  data declared (D4, D5).
- **The sequence's contention**: every signing reads and writes the book's
  row, which serializes signings; fine in memory, a hot row on PostgreSQL.

## Findings

- **CORRECTION (security) of the platform's guest identity**, made here at
  the integrator's ruling (correction 1): a guest's and a signed-out
  reader's user id held the session cookie's value, and so did a guest's
  handle and name, which the feed and the guestbook showed and a payment
  sent to its provider as metadata.
- **A stand-in passes the identity's `is_guest()` as an account**: the
  feed's `profile_of` and `viewer_val` read it so, and would write or show
  its empty name. No feed command runs on a reader's behalf today; the
  identity track's to settle (`account_of` no longer reads it so).
- **`Words` holds a length, not a trim**: a client that sends three spaces
  as `sign`'s argument directly signs an entry of spaces (the page trims,
  and never sends one). A refinement has no trim to state; recorded, not
  changed.
- **A command whose commit fails is answered "outcome unknown"** by the
  browser's route (ADR-0325's rule for an interaction), even where the
  materializer rolled back before its COMMIT; the press's retry is then
  answered the same. Recorded, not changed.
- ADR-0386's open item "authorized commands using tables" is closed
  (decision 3).
- A template value computed from two values (`mine` and a row) is refused
  (ruling 0073-a), so a per-reader control on a public row needs the row
  computed for the reader.
- `clock.now()` has the body `0` (no host binding); a program holds no
  instant as a value, so a row's order comes from a sequence it keeps.

## Next

- **Paged reads** (ADR-0384's H4): `first`/`after` cursors and a
  descending read of the latest N, so `Latest` reads its `limit` and the
  guestbook's bound can go.
- **The platform's account record, durable**, beside the sessions the
  hosts keep (ADR-0369), with D3 or the second-host track.
- **D3's PostgreSQL layer through the suite** (`conformance::Layer`), and
  the rows and records in one PostgreSQL transaction.
- **Move `pw-table-memory` onto the host's mirror of tables.json**
  (decision 12), before D3 adds a second adapter that the dev server links.

## Questions for the integrator

- **The block PW69xx** (`Owner::DataPath`) for this track's codes: the next
  free one (PW67xx is the Docs app's, ADR-0339; PW68xx the device store's),
  taken as ruling 3 asked a block of the track's own. Renumbering before
  the merge is cheap.
- **Reached scripts' evidence** (S4): this ADR claims the whole runs of
  other tracks' scripts it re-anchored or whose tests it changed as their
  own recipes' records at the head, not as this recipe's; running them
  inside `e14-data-path` too would double their hours on CI.
- **The feed's stand-in** (Findings): `is_guest()` is the identity
  track's; a positive "an account" there would close it for every reader.
