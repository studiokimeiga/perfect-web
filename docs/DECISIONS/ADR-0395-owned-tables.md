# ADR-0395: a table says whose its rows are, and a predicate may be written in Pleris

Status: accepted 2026-11-12, the integrator's ruling under the owner's
delegation of 2026-10-02: L2 of the declared data layer's landing (its
plan of 2026-11-12, the coordinator's), on L1 (ADR-0397,
`wip/l1-durable-tables` at `e1133034`): branch
`data/l2-row-ownership-20261112`, pushed as `wip/l2-row-ownership`,
`fix/row-data-integration` at `a9a8d79e` merged onto L1 in `04d06a17`, its
last change `3134212d`. Built by track `row-ownership` (the coordinator's brief of
2026-11-02, ADR-0384's H1 and H2) and carried with the data path and
PostgreSQL by the integration branch `fix/row-data-integration`, whose
slice up to `a9a8d79e` lands here; amended as ADR-0407 ruled (design A,
with what PR #91's design B found). Milestone: E14. Builds on ADR-0338 and
ADR-0350 (declared tables), ADR-0385 and ADR-0386 (the table gate),
ADR-0393 (D3, PostgreSQL), ADR-0394 (the data path), ADR-0397 (durable
tables), ADR-0115 (`requires`, amended here), ADR-0263 and ADR-0270 (a
handle is the platform's; the reader's user), ADR-0129, ADR-0302,
ADR-0307, ADR-0339's ruling 3 and ADR-0406 (`requires`, PW0367).

## The landing (L2, 2026-11-12)

### Rulings taken (the integrator, 2026-11-12)

The plan's section 4 is the integrator's default; its rulings 2 to 5 are
L2's and are recorded here, with the defaults L2 applied besides.

1. **L1 before L2** (the plan's ruling 1): L2 merges onto L1, so durable
   tables land apart from the new design of ownership.
2. **PW0367 collides** (ruling 2). ADR-0406's `who_may_run` landed first as
   PW0367; the track's `owner_names_a_principal` is **PW0370**. PW0368 and
   PW0369 keep their numbers. No number had been published: the constant,
   its fixture R-110, its tests, comments and mutant name moved with it.
3. **PW69xx's owner is master's `DataPath`** (ruling 3). The track had
   renamed the block's owner `Dating`, for an app that does not exist; the
   track's codes (PW6905, PW6907, PW6924 to PW6926) join PW6901 under
   `DataPath`, and so does PW6920 (ruling 5).
4. **A public table, owned and written by its owner** (ruling 4): `public
   table` beside `owned by <field>`.
   - Reads go through any key, prefix, index or unique index, and `all()`
     where the table is bounded, each by plain values and with no handle:
     its rows are every reader's (`Entries.get(author, book, number)`,
     `Entries.by_book(book)`).
   - Writes are held to the owner: `insert(me, row)` and `put(me, row)`
     take the writer's handle beside a row whose owner field the host holds
     to it; `delete(me, ..)` and `delete_of_<prefix>(me, ..)` take it in the
     owner's place.
   - The key still leads with the owner (PW0368), so no reader writes a key
     another's row holds: no squatting, as ADR-0407 found of design B.
   - Bounds and unique indexes may cross owners: a public table's rows tell
     no one anything a refusal by its bound or by a unique value would.
   - It shares with no one (PW0370) and keeps nothing private (PW0369), and
     for references it is a table no one owns (PW6924): its rows are every
     reader's.
   - `public` is the one word a table is declared with (PW0370): a `private
     table`, a `user table` or a `session table` was read as a table and
     kept nothing from anyone, so each is refused.
   - tables.json states `public` beside `owner`; the host holds a public
     table's manifest whole by the same rules (`Table::public_whole`): every
     read given no handle, every write its owner's.
   - **The guestbook** keys its entries `(author, book, number)`, `order
     number`, `index by_book on book`, `unique by_number on book, number`,
     `owned by author`, `public table Entries`, still `bounded 1000`; its
     `remove` deletes the reader's own entry by construction
     (`Entries.delete(current_user(), ..)`), reading the reader's own entry
     only to say `NotYours` or `NotFound`, and `sign` inserts by the
     reader's handle. Its schema lock is rebased: none was committed, and
     nothing is deployed. The tutorial (`docs/guide/tutorial.md`) builds the
     same files, byte for byte.
5. **The mandatory declaration is PW6920** (ruling 5), not #91's PW0370
   (`who_may_write`): a table that a command a reader can run writes,
   itself, through what it calls, or through a command it schedules that
   the clock runs on its behalf, says who may write its rows: `owned by
   <field>`, each row its owner's alone, or `written by anyone`, any
   reader's request writing any row, on purpose; never both. `written by`
   reads only `anyone` (PW0361), and `written when <Predicate>` is H3's, not
   built. A table only the platform's other commands (`invoked_by`) write
   needs neither. Migrated: the guestbook (`Books` `written by anyone`,
   `Entries` owned), `examples/tables` (`Lists`, `Items`), A-050 (`Chores`,
   now R-118's control), the guide's tour (`Notes`), and the test programs
   that write tables. The analysis is #91's `written_by_readers`, carried.
6. **Receipts** (the plan's ruling 6): the merge took L1's (master's) side
   of the 52 conflicting receipts, recorded again by the full verify; the
   integration's local campaign receipts (`row-browser-postgres/`,
   `row-data-integration/`, 101 files) stay on `fix/row-data-integration` at
   `a9a8d79e`, which this ADR and the two diagnostics records cite, and the
   track's `row-ownership.txt` lands as its last CI record (verify
   41491093145 at `83761ca3`).

### What ADR-0407 carried from #91, and where it is

- **Ownership is declared**: PW6920 (ruling 5).
- **A public-read, owner-written form**: ruling 4; the guestbook's `remove`
  is structural.
- **A clock run is admitted as the reader who scheduled it** (B's
  `runs_for`): master's scheduler keys a run by the user whose command made
  it (ADR-0334 as amended, ADR-0407 decision 4) and runs it on that user's
  behalf (`principals().on_behalf`); the integration's witness
  (`4f80e543`) is ported as `tests/owned_clock.rs`: the clock reads and
  writes the scheduling reader's owned cart after their browser signed out,
  a run made stale by a reschedule writes nothing, a completed one is never
  run again, no request runs it, and bob's cart is his alone, on memory and
  on PostgreSQL (the row, its receipt and its event in one transaction).
- **B's race scripts** (`race_delete_put.sh`, `race_replaced_put.sh` and
  `owned_rows.rs`'s races at `c2130674`), rewritten for A in
  `pw-table-postgres`'s `compiled_owned.rs`:
  `two_owners_rows_of_one_id_are_two_rows_whatever_runs_between_them` (B's
  key was global, so a put met, waited on and could take another reader's
  row of its id; A's leads with its owner, so bob's put of his `n1` neither
  waits on alice's uncommitted `n1` nor changes it, at read committed and
  at serializable) and
  `a_put_waiting_on_its_owners_row_is_answered_by_what_it_wrote` (bob's put
  waits on his own row's delete alone and writes his row anew, alice's of
  that id untouched, answered by what it wrote: master's `RETURNING old`).
- **A reader's command is never the platform's** (`c2130674`), rewritten
  for A on memory and PostgreSQL: a reader's command made by a host as no
  one (`Caller::Nobody`) is refused its owned table before it writes,
  whatever its platform's principal says; made as bob it removes his row of
  the id alone; made as alice, hers. Design A has no platform writer of
  owned rows, so "the platform" is a call no reader makes.

### What the merge did that git did not show

- `codes.rs` held PW0367 twice, and PW69xx's owner had become `Dating`
  (rulings 2 and 3).
- Thirteen commands said nothing of who may run them (ADR-0406's PW0367):
  `examples/owned`'s `write`, `keep`, `remove` and `forget`, the
  `required_query_bound_by_a_page` witness's `order`, and the test programs'
  `crush`, `subscribe`, `cancel`, `like`, `keep`, `peek` (twice) and `drop`.
  The plan counted five; it read the `.pw` files, not the programs written
  in tests. Each `requires Anyone`.
- The host registry: master's `rows` and `row-events` roles took the port
  offsets 213 to 218 the track's `owned` role had; `owned` is 219 to 221.
- `owned.sh`, the owned fixture's builder (2026-11-03), built the browser's
  WebAssembly renderer (`pw-render-wasm`), which master no longer has, so
  `e14-row-ownership` and `e14-durable-tables` could not build their
  fixture; it builds with `pw build --dist` now, as `guestbook.sh` does
  (ADR-0429).
- Two repairs of one count: L1 begins a transaction in the conformance
  suite before it measures (`begun`), and the integration made the
  PostgreSQL layer's measured statements its operation statements, the
  transaction's boundaries left out (`operation_statements`). Both stand,
  one more than the count needs (Not claimed).
- `ADR-0397` had taken the integration's notes on the owned-row PostgreSQL
  work beside its own; they are this ADR's (below), and ADR-0397 is L1's
  text.

### Evidence of the landing

Measured here (Linux in WSL, PostgreSQL 18.6, 8 build jobs), each recipe
whole, its receipt read and then left as L1's for the full verify to
record (ruling 6); the record is that verify, not yet run. Each at the
commit its receipt names:

- `e14-row-ownership` (`3134212d`): `table_owners` 31, `predicate_bodies`
  11, the host's `table_owners` 4, `compiled_owners` 15,
  `compiled_predicates` 15, the server's `required_queries` 14; the
  owned-row specs in Chromium, Firefox and WebKit, 6 passed;
  `row_ownership_mutations.py` baseline 168 passed, **94 of 94 mutants
  killed**, L2's 23 among them. Its first run refused its baseline: the
  grammar's tests hold a `should panic` test, which the strict producer
  does not read (`71487eb2` filters it out).
- `e14-durable-tables` (`71487eb2`): `compiled_owned` 8 (#91's races and
  its platform test among them), the adapter's platform 15, the server on
  PostgreSQL 33 (`tests::tables_pg::` and `tests::owned_clock::`), in
  memory 30, the server as a process 1; the guestbook on PostgreSQL in
  three engines 18 passed, the owned rows on PostgreSQL 6, the guestbook in
  memory 15; the conformance suite over the guestbook's tables on both
  layers, 406 reads checked, refusals `Books` 1 and `Entries` 2, 8 writes
  measured; baseline 57 passed, **53 of 53 mutants killed**, the clock
  witness's ("an owned Clock action forgets its scheduling owner", 2 of 57
  tests fail) among them.
- `e14-d3-postgres` (`71487eb2`): baseline 75 passed, **62 of 62**.
- `e14-data-path`: at `71487eb2` 59 of 60, its mutant "a primitive answers
  another table's rows" refused by the strict producer, its failures
  panicking inside the memory adapter (an index read of another table's
  state, a path no guestbook read took before its entries had indexes);
  the adapter's lookup is a schema error since `3134212d`, and at it
  baseline 62 passed, **60 of 60 mutants killed**, that one with 24 of 62
  tests failing at their own assertions.
- `e14-tables` (`8ec67c93`): baseline 41 passed, **44 of 44**.
- `e14-refusal` and `e14-refusal-2` (`71487eb2`): **23 of 23** and **7 of
  7**.
- `e14-check-is-build` (`71487eb2`): the program census, 495 program
  assemblies, 684 of 684 tracked sources covered; **51 of 51**.
- `e14-who-may-run` (`71487eb2`): baseline 44 passed, **10 of 10**.
- `e14-guide` (`3134212d`): the guide's examples 22, the guestbook in
  Chromium 5; **75 of 75**.
- `evidence-corpus`: 106/106 enforce their declared invariant.
- `ci_plan.py --changed e1133034 HEAD` reaches 199 recipes in 19 shards;
  the ones above are those whose scripts or programs L2 changed, and the
  plan's named ones.
- The workspace: `cargo test --workspace --no-fail-fast`, 4,143 tests
  passed, none failed (26 ignored); `cargo clippy -D warnings` (the
  workspace and `pw-host`'s engine) clean; `cargo fmt --check` clean;
  `mutation_anchors.py`, 5,847 mutants in 347 scripts, each anchor once;
  `just evidence-gates`, 1,207 tests OK and the node suites; fallow's
  new-only audit against `origin/master`, verdict pass, nothing introduced.
- Test first: L2's eleven compiler tests (`table_owners.rs`), run against
  the merge's compiler before the rules were let in: ten failed, and the
  eleventh (a public table's key still led by its owner) guards a rule
  that held already; the grammar's `written by anyone` test failed without
  its grammar, its control passing both ways.

## Context

The commonest web bug class is a read or write of another user's object
named by its id (broken object-level authorization, OWASP API1). Until now a
declared table's generated reads took plain ids, so `public query` could read
anyone's rows, and "only its author may remove a post" was a Rust predicate
(`identity.rs`'s `OwnsPost`) the deployment held, outside the program
(ADR-0384, gaps 1 to 3). And a table a reader's command wrote said nothing
of who might change its rows, so any request changed any row its key named
(ADR-0407).

## Decision

### H1: an owned table

```pleris
table Notes
    row     Note
    key     author, id
    order   added
    index   by_to on to
    owned   by author
    shares  with to
    private draft
```

1. **`owned by <field>`** names the field holding each row's owner's id, a
   `capability.UserId` (PW0370); **`shares with <fields>`** the fields
   holding the ids of the others who read a row (PW0370: each a `UserId`,
   never the owner); **`private <fields>`** what its owner alone reads.
   `shares with <Predicate>` is ADR-0384's `shares when`, H3's, refused as
   not built (PW0370 says so).
2. **Every generated read of a table read by its reader takes the reader's
   handle**, a `capability.User` only the platform makes (ADR-0263), in the
   place of the field it leads with: the owner's reads answer the row
   (`Notes.of_author(me)`), a sharer's answer the **seen row**
   (`Notes.by_to(me) -> List<NoteSeen>`), the record the compiler declares
   beside the row without the private fields, so no page, cache entry or
   answer of theirs can hold one. A set form takes the handle and a list of
   what follows it, and none is given where nothing follows; no read leads
   with another field and none reads the table whole (PW0368: the key leads
   with the owner, each index with a reader, a unique index holds the owner,
   `all()` refused, and no bound, which would count every owner's rows).
   Writes take the owner's handle: `insert(me, row)`, `put(me, row)`,
   `delete(me, id)`, `delete_of_author(me)`. A program reads or writes no
   other user's rows by naming an id: a plain id is PW0605, a made handle
   PW5037, a shared cache PW5001, a binding by hand PW0362.
3. **A private field** is a field of the row, never the owner, a sharer, in
   the key, the order of a table that shares, or an index a sharer reads by
   (PW0369): a field every reader selects or orders rows by tells its values.
   The seen row's name is the compiler's (PW0369).
4. **A reference stays within one owner's rows** (PW6924): a database holds
   a reference across every row of both tables, whoever owns them, so an
   owned table refers to an owned table from its own owner field alone,
   naming its owner's own row (`refers member to Memberships`, A-057); a
   table no one owns refers to no owned table; an owned table to no table no
   one owns. A public table refers and is referred to as a table no one
   owns (ruling 4). A reference between two tables no one owns is D1's.
5. **A public table** (ruling 4): `public table` beside `owned by`, read by
   anyone and written by its owner alone, as above.
6. **Who may write a table** (ruling 5, PW6920): `owned by`, or `written by
   anyone` where any reader's command writes it on purpose.
7. **tables.json** states `owner`, `shares`, `private` and `public` per
   table and each function's `reader`, outside the schema: who reads a row
   is no part of what is stored, so `schema.lock` and its hash are
   unchanged. `written by anyone` is the compiler's alone: it changes
   nothing a host serves.
8. **The host serves an owned function to its caller alone**, whatever
   adapter answers it. The engine is told who a call is made as
   (`tables::Caller`, from the session's principal, `call_with_tables_as`);
   before any adapter reads, it holds the handle the component passed to
   that caller, refuses every owned function to a call no one makes (an
   `invoked_by` command; `call_with_tables` is such a call), holds an
   `insert`'s or `put`'s row to its writer by its owner field, wherever that
   is in the row, and refuses one not exactly the table's (PostgreSQL's
   `WITH CHECK`), gives a set form's handle back to each key, then answers a
   sharer the seen row (`engine::table_owners`). A public table's reads are
   given no handle and admitted as a plain table's. The manifest is held
   whole for its owners where the host reads it, its bounds and references
   among it, and an owned import typed otherwise is refused where it links.
   The memory and PostgreSQL adapters need no ownership of their own: what
   reaches them is a plain table's call.

### H2: a predicate may be written in Pleris (ADR-0115 amended)

```pleris
predicate Wrote(id: NoteId)
    says "You can only change a note you wrote."
{
    match Notes.get(current_user(), id) {
        Some(_) => true,
        None => false,
    }
}
```

9. A predicate the program gives a body answers a `Bool` (PW0606) and only
   reads (PW6905: no effect but `database.read` and `session.read`). It is
   compiled as a component of its own, kind `predicate`, never a route or a
   page's read; each export requiring it names that component in its
   `authorization` (`component`). It is answered by that body alone
   (PW6925): a `requires` naming it sees it where it is written, imported as
   any declaration is, and no page asks it of its reader yet.
10. **`requires` belongs to a command or a query** (ADR-0339's ruling 3),
    its arguments the declaration's parameters. A query that requires a
    predicate is answered to its reader alone: every read of it is keyed by
    its reader, cached `private` or a `user` or `session` query, and no
    materialization depends on it (PW6907); and it is read by a page's
    binding alone, `query Q(..)`, where the host asks its reader, never
    called, named or bound by a query, a command, a function, a predicate or
    a materialization, into whose body its own would be compiled unasked
    (PW6926), as a command is sent by a handler alone (PW0339). Who answers
    each requirement is part of the contract's `abi_schema`.
11. **The engine asks a program's predicate itself**
    (`Prepared::call_authorized_with_tables_as`, `engine::program_predicates`):
    each requirement in the order the export states it, before the export's
    component is instantiated. One naming a component is asked of the
    program's compiled predicate, from `Predicates` (each `Predicate::new`
    held to one `predicate` export requiring nothing of its own; one missing
    refuses the invocation): run as the export's `Caller`, with the same
    platform operations, so `current_user()` and every owned read answer the
    same reader; its reads from the same `CallTables` as the export's table
    functions (a command's staging: the memory adapter's `Command`, the
    PostgreSQL adapter's `Command` in the transaction its session fenced; a
    query's snapshot: memory's `Snapshot`, PostgreSQL's one repeatable-read,
    read-only transaction), so what it read is what the export reads and
    writes against; under the export's `Limits`, its fuel spent from the
    invocation's one budget. Any other requirement is the deployment's
    (`identity.rs`'s `holds`), as before. The first that does not hold, or
    cannot be asked, refuses the invocation before its component runs, and
    no other path asks a program predicate: `authorize_export`, the
    deployment's, refuses one unasked. An explicit denial is carried as
    `InvocationError::refused`, apart from an evaluation's failure, and the
    route answers it 403 in the predicate's words; a reused cached,
    deduplicated or fallback answer passes the same authorization against
    the new invocation's reads first, and a denial never falls back to the
    old protected value.

## Alternatives

- **B as the model** (PR #91): ADR-0407 rejected it, keys global across
  owners (squatting) and writes protected alone.
- **The owner field typed as the handle** (`author: User<UserId>`): writes
  would be refused at compile time, but a sharer's row would carry another
  user's handle; an id that grants nothing (ADR-0270) and the host's write
  check were chosen.
- **Enforcement in each adapter**: D3's and every later adapter would have
  to repeat it; the engine's one place serves them all.
- **A public table as a clause** (`reads anyone`) rather than the word
  `public` a declaration's visibility is written with: `public` is already
  what a query's answers are, alike for every reader (ADR-0118), and a
  second word for one thing was refused.
- **A guestbook whose `remove` keeps its own check of authorship**: correct
  while the check is, and the check is what the owned table makes
  unnecessary; the program's answer (`NotYours`) is kept, its authority
  gone from it.
- **PW6920 for every table holding a `UserId`** (ADR-0384's H9 reason,
  erasure): a table only the platform writes (a payment's settlement) could
  not be owned yet, since a call no reader makes writes no owned row; H9's.
- **The predicate compiled into the guarded component's body**: the host
  could no longer ask it before instantiation, and a component would
  authorize itself (ADR-0115's point).
- **A reference across owners allowed, its check moved out of the
  database**: a missing referred row would then be written, which a
  reference exists to refuse, and PostgreSQL's foreign key could not be
  used. Refused instead, until a reference declares what deleting does.
- **An embedder callback for a program predicate**: an embedder could still
  run it in another transaction or unbounded; the engine running it from
  the call's own `CallTables` leaves nothing to get wrong.

## Not claimed

- **A predicate's cost is others' to run up** (H4): a predicate whose body
  reads the rows addressed to its reader reads as many as others choose to
  write, unbounded by `written when` (H3) or bounded reads (H4).
- **Time**: `Limits` bounds no call by wall-clock time; fuel bounds a
  predicate's running as it bounds every call's.
- **The compiled predicate is the embedder's to load**: the engine runs the
  component registered under a requirement's id, held to one `predicate`
  export, and does not hold its bytes to the build.
- **`shares with` without `written when`** (H3's PW6911): anyone may write a
  row addressed to anyone.
- **A reader's key is its session's**: the development server keys a
  `private` or `user` read by the session, so one user's two sessions read
  apart; never two users' together.
- **`Caller` carries no session generation**: an in-flight call's owned
  reads and writes outlive a revocation until the server fences them
  (ADR-0389).
- **PW6920 covers what a reader's command writes**, not every table holding
  a `UserId` (ADR-0384's H9, erasure), and a platform command (`invoked_by
  Payments`) writes no owned row: a call no reader makes is refused every
  owned function. A settlement writing its owner's rows is the plan's
  open capability (section 3).
- **A shared row tells what it refers to**, and **PW6903** (a private
  field's value written where another reads it) is not built.
- **ADR-0384's PW6906 and PW6908**, two-owner rows, `shares when`,
  `written when` and `reveals` are H3's.
- **A page asking a predicate whose body reads a table** is told PW0401
  beside PW6925, one defect told twice.
- **Two counts of a transaction's boundaries**: L1's `begun` in the
  conformance suite and the PostgreSQL layer's `operation_statements` both
  stand (the landing, above); one of them should go, with a test of the
  other's that fails without it.
- **Browser evidence on GitHub**: `e14-row-ownership` runs the owned-row
  specs in three engines and `e14-durable-tables` the guestbook's on
  PostgreSQL; their record is the full verify, not yet run.

## Next

- **H4**: a bound per owner (`bounded 3 per owner`), cursors, and a bound on
  what a predicate's reads may cost its reader.
- **H3 and H9**: a reference that states what deleting the row it names
  does; `written when`; erasure, and with it PW6920's `UserId` clause.
- **The feed and the store** (L3 to L7): `MayMessage` and `MayRate` as
  bodies, their arms in `identity.rs` gone; the store's carts owned.
- **A wall-clock deadline for every call**, a host track of its own.
- **A page asking a program predicate**: the host asks the body for the
  document, and PW6925's page clause lifts.
- **One count of a transaction's boundaries**.

## History

The track's two independent reviews, and what each changed, are on
`origin/wip/row-ownership` (`ff9ff2c6`) and `fix/row-data-integration`
(`a9a8d79e`), in this ADR's draft as it stood (`ADR-XXXX-owned-tables.md`
there); in brief:

- **The first review** (of `485b10b2`): a reference check that read and
  blocked another owner's rows (PW6924, with the host's
  `references_within_one_owner`); `bounded` on an owned table one quota
  across every owner (PW0368); the deployment answering a predicate the
  program gave a body (`authorize_export` refuses one unasked); a predicate
  not imported where it was required becoming the deployment's (PW6925,
  and its page clause); the predicate's transaction, caller and limits the
  embedder's (the engine asks it, decision 11); tests that passed falsely
  (the predicate after the body; an owner check reading a row's first
  field; a concurrent change between predicate and body untested); a row
  not exactly the table's passed by the owner check; `cache shared` on a
  guarded query (PW6907). 46 of 46 controls (verify 41491093145).
- **The second review** (of `ff9ff2c6`): a query's predicate asked of the
  first reader of a shared read alone (PW6907 widened: every read keyed by
  its reader); calling a guarded query skipped its predicate (PW6926); the
  ABI blind to who answers a requirement (`@<component>` in `abi_schema`);
  `Predicate::new`'s checks unheld. Its repairs (`b407ede4`) are in
  `a9a8d79e`'s history, each of their 20 files unchanged or grown.
- **The integration** (2026-11-03, `cb34e638..a9a8d79e`): the server's
  seam calls `call_authorized_with_tables_as` with its staged command or
  query snapshot as `CallTables` and the build's `Predicates`; `Snapshot`
  supplies both a query's predicate and its body; typed refusals carried
  across the query cache; PostgreSQL's `Command` and `Snapshot` as
  `CallTables` (`compiled_owned`: staged reads, refusal before the body, a
  delete between predicate and body, a real serializable conflict after a
  predicate read, ended and wrong-kind callbacks); the owned-row browsers
  (`examples/owned/pages.pw`, `e2e/owned-rows.spec.mjs`) on memory and
  PostgreSQL. Local campaigns at `6b31f4ba`: row 71/71, data-path 60/60,
  D3 PostgreSQL 60/60, durable 51/51, refusal 30/30, owned-row browsers
  6/6 on each layer in three engines; receipts on that branch in
  `docs/evidence/E14/row-browser-postgres/` and `row-data-integration/`,
  and `docs/diagnostics/row-browser-postgres-2026-11-03.md` and
  `row-data-integration-2026-11-03.md` here.
