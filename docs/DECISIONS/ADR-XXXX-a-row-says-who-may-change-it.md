# ADR-XXXX: a row says who may change it

Status: proposed, 2026-11-07, on branch `feat/owned-rows-20261107`, stacked
on the requires ADR's branch (#83). The owner asked for "phase 2" of
mandatory `requires`: no command may change another reader's record by
naming its id. Built on that branch the same day (below, "How it is
built"); not landed; not numbered (PARALLEL.md: the integrator numbers
ADRs).

## Findings first (2026-11-07, while building)

1. **Another design of the same class is accepted, and partly built.**
   ADR-0384 (accepted 2026-10-25 as the dating app's plan, docs only)
   sketches `owner <field>`: an owner held in a `UserId` field, each read
   that leads with it taking the reader's handle, each write checked against
   it, with `shares` and `private`; its syntax is marked provisional. Track
   row-ownership built its H1 and H2 as `owned by <field>` on
   `wip/row-ownership` (`ff9ff2c6`, 2026-11-02; no pull request, "lands after
   #72"). This ADR's survey read only master and did not see either. The
   two can stand together (`owner user|session|anyone` takes only those
   three words, and the track writes `owned by`), but they are two ways to
   say that a user owns a row, one hidden and one a field: **the
   integrator rules which governs**, or how one becomes the other's step.
2. **Codes.** That track registers PW0367 (`owner_names_a_principal`),
   PW0368 and PW0369. The requires ADR's PW0367 (#83) already collides with
   it. This change takes **PW0370**, leaving PW0368 and PW0369 to the track
   (`codes.rs` says so), so that one renumbering, not two, settles them.
3. **The handle check is PR #72's PW6901.** Track data-path (#72, open)
   refuses a row whose field's type holds a handle (`row_holds_handle`,
   commit `28590090`), after its review found `pw check` taking `who:
   User<UserId>` and a shared query serving the stored handle to every
   session. Decision 5's test settles the same thing, so this branch carries
   that commit's compiler half unchanged (its registry entry, the check in
   `tables::shape`, `check::handle_in` made visible to it, its tests in
   `tests/handles.rs`, the guide's row): whichever lands first carries it,
   and the other merges the same lines.
4. **A reference reaches across owners** (not decided here, below). With
   `Lists` and `Items` both `owner session`, a reader's `keep_item` may put
   an item into another reader's list by naming the list: the reference
   only asks that the list exist. And an item of one reader's that refers to
   another's list stops that list's delete at the commit. Whether a
   reference means "part of" (an item of a list: the same owner's alone) or
   "about" (a comment on a post: anyone's) is the program's to say, and no
   clause says it yet.
5. **A bound counts every reader's rows.** `bounded 100` on an owned table
   is one quota for all readers: one reader can fill it, and every other's
   insert is refused. Track row-ownership refused `bounded` on owned tables
   for this; here it is unchanged, and listed with the tables a narrowing
   would reach.
6. **A program cannot ask whose a row is.** The owner is in no field, so
   `add_item(list)` cannot check that the list is its reader's before
   writing beside it (finding 4). ADR-0384's field-based owner can; this
   design would need a read the platform answers (a row if it is the
   reader's, or none).
7. **No host serves a program's tables yet.** The development server's
   routing of declared tables is #72's; until it lands, the reader reaches
   a table only through the adapters' API (`begin(Writer)`), which every
   caller must now give one. #72's seam, once merged, begins each request's
   command for its reader, by an id its host makes: never the session's
   cookie, which #72 already keeps out of everything the host stores.

## Context

- **The class.** Broken access control is OWASP's first web risk, and its
  commonest shape is an insecure direct object reference:
  `delete_item(item: ItemId)` deletes whatever item the request names, so
  any reader can delete anyone's by sending its id. PW0367 makes a command
  say who may run it, but `requires Anyone` or `requires SignedIn` says
  nothing of whose rows it may change.
- **What the compiler sees today** (a survey of 2026-11-07, all in this
  repository):
  - A `table` (ADR-0338) declares its row, key, order, indexes, references,
    bound and migrations, and generates its reads and writes
    (`insert`, `put`, `delete`, `delete_of_<prefix>`). It declares no owner.
    A test's `owner: UserId` field with `refers owner to Users` means
    nothing to any rule.
  - Ownership today is a deployment predicate written in Rust per app
    (`OwnsPost` reads `posts#thread` and compares the author;
    `srv/identity.rs`). Nothing ties a predicate to a table, a key or a write.
  - Nothing tracks that a value came from the browser. A `PostId` from a
    parameter and one read from storage are alike to every rule.
  - The runtime cannot yet run a table command that carries requirements
    (`call_with_tables` refuses an export with any, ADR-0386), so a rule
    that forced `requires OwnsItem(item)` on table commands would leave them
    unrunnable.
- **What can be trusted:** a handle (`Session`, `User`, `Organization`) is
  made only by the platform (PW5037–PW5039). The reader of a request is
  known to the host, never to the browser.

## Decision

1. **A table says who may change its rows**: `owner user`, `owner session`,
   or `owner anyone`.
   - `owner user` and `owner session`: the platform records each row's owner
     when the row is inserted, as the request's reader (its user, or its
     session). No field holds it, so no program writes it and no browser
     sends it.
   - A `put`, `delete` or `delete_of_<prefix>` from a request changes only
     rows its reader owns. A row of another reader is treated as absent:
     the write changes nothing, and the answer does not say the row exists.
   - `owner anyone` says any reader may change any row, outright, where a
     reviewer reads it, as `requires Anyone` says it of a command.
   - Reads are unchanged: a query's audience (ADR-0270) governs who sees a
     row, and ownership governs who changes it.
2. **A new code: a table that a command a reader can run writes declares
   `owner`.** One that says nothing is refused at its declaration, beside
   each command that writes it. A table only the platform's commands write
   (`invoked_by`) needs none.
3. **Host bindings are out of this rule.** The compiler cannot see inside
   them, so they keep their predicates (`requires OwnsPost(post)`). Data
   moved to tables gains the rule.
4. **The migration keeps every program's meaning.** Each table a reader's
   command writes today says `owner anyone`, since anyone could change any
   row before. The tables example and A-050 are listed for the owner to
   narrow. A-050's chores, and the tables example's lists and items, are
   per visitor by intent and are the first candidates for `owner session`.
5. **One check comes with it.** The survey could not show that a table row
   refuses a handle-typed field (`keyable`, and PW5038's exemption for `pw:`
   operations). If a row can hold a `User<UserId>`, a read could hand a
   program another reader's handle. A test settles it, and a refusal is
   added if one is missing.

## Alternatives

- **Require a predicate over the id** (`requires OwnsItem(item)`), traced
  from a parameter to a write's key. Rejected for tables:
  - the predicate's meaning stays hand-written per deployment;
  - the runtime cannot yet run such a command;
  - a predicate checked before the body is a race with a concurrent change
    of owner, where a scoped write is not.
- **An owner field the program writes** (`owner: UserId`). Rejected: a
  program can construct any `UserId`, so the field proves nothing unless
  the platform sets it. Then it need not be a field.
- **Row-level security in each deployment's database.** Rejected as the
  first answer: it moves the rule out of the program, where the compiler
  cannot see it is missing.

## Not decided here

- Ownership by an organization (`owner organization`), and a row shared by
  several readers (a conversation's two sides).
- Transferring a row's ownership.
- Moving the feed's and store's host-bound data to tables, so their
  ownership becomes checkable.

## Consequences

- A table a reader's command writes says who may change its rows, and for
  `owner user` or `owner session` the platform enforces it: the class above
  cannot be written against such a table.
- `owner anyone` is greppable, beside `requires Anyone`: together they list
  everything anyone may run and change.
- The host's table operations take the request's reader (the tables host,
  `runtime/pw-host`), and the contract carries each table's owner kind.

## How it is built (2026-11-07)

- **The clause** (`pw-syntax`'s grammar, `policy.rs`, `tables.rs`): `owner`
  heads a clause only where its word follows on its line, as a table's
  other word heads do, so `owner` stays a name a body may begin with. It is
  a table's alone (PW5105 elsewhere), of the words `user`, `session` and
  `anyone` (PW0335 for another, told once: PW0370 is not told beside it).
  `pw fmt` aligns it as each clause of its table.
- **The rule, PW0370 `who_may_change`** (`tables::written_by_readers` and
  `tables::who_may_change`, called beside PW0367): every command not
  `invoked_by` the platform whose effects, inferred through every call it
  makes, hold `database.write<T>` of a declared table `T`, or that schedules
  a command the clock runs, which writes one, on its behalf (below, the
  review's first finding). A table with no
  `owner` is refused at its name, once, each such command named beside it at
  the call that writes (in its own unit where it is another's).
- **What a database records.** `TableSchema.owner` is `user` or `session`;
  `owner anyone` and a table that says nothing record none, so their schema,
  and its hash, are what they were: no lock and no database changes for a
  table that says `owner anyone`. tables.json carries it (flattened), the
  host's manifest reads it, refusing a kind it does not know, and
  `Manifest::owner_of` answers it.
- **PW0366** (the schema released): an owner recorded from now on, or given
  up, is a step (`Step::ChangeOwner`); rows released before are owned by no
  reader, so no request changes them, and the platform's commands still
  may. An owner of another kind (`user` to `session`, or back) is refused:
  each recorded owner would be read as another kind of reader.
- **The writer** (`pw_host::tables`): every command's transaction is begun
  for a `Writer`, a request's `Reader` (its session's id, and its user's
  where it is signed in, each an id its host makes, never a secret) or the
  platform's own command, which changes any row and owns none it inserts.
  `Writer::scope` is the rows a write may change: any, in a table no reader
  owns or for the platform; else the reader's own, by its id of the table's
  kind. A reader signed in as no one is refused every write to an `owner
  user` table, whatever the row, so that the answer says nothing of
  another's. `Writer::admits` holds the writer to the command's contract: a
  reader's for a command a request runs; the platform's for one another of
  its own invokes (`invoked_by Payments`); and for one the clock invokes,
  the reader whose command scheduled it, on whose behalf it runs, or the
  platform where the platform's own command scheduled it. No host runs a
  reader's command as the platform.
- **The memory adapter**: a row is held as its fields and, beside them, its
  owner, so the owner moves with the row through every write and undo of
  the tables commands write, and every answer (a read, a refusal) is the
  fields alone. An insert records its writer; a put keeps the owner a row
  records; another reader's row is absent to a put, a delete and a prefix's
  delete: nothing changes, and the answer is the one a missing row has. A
  row put back as it was for another owner is no change a read sees, and
  neither adapter counts it written.
- **PostgreSQL**: an owned table has a column of the adapter's own,
  `"pw:owner"` (no field of a program's can be named so), null for a row no
  reader owns. The owned insert, put and delete statements take the writer
  (`$me`, `$any`) and decide in the same statement: a put's update is
  guarded on the row its conflict found (`ON CONFLICT .. DO UPDATE .. WHERE`
  its writer changes it), and another's row is left as it was, the put
  answered as written; a delete's `WHERE` holds its writer's rows alone.
  **Found while building:** the first draft judged the put on the
  statement's snapshot (`old`) alone. Under read committed, which a
  deployment that leaves the isolation to its connections may run at
  (`Isolation::ConnectionDefault`), a row another reader inserted after the
  snapshot was then taken by the update. `a_row_inserted_while_a_put_runs_
  is_left_as_it_was` makes that interleaving with two connections; it fails
  with the guard removed (Bob's text in Alice's row) and passes with it. At
  serializable, PostgreSQL refuses the same interleaving itself (40001). The
  migration adds the column where a table comes to record owners (each row
  then null), drops it only where the build's migration is from the schema
  recorded, and refuses an owner of another kind.
- **An insert of a key another reader's row holds** is refused as any
  insert of a held key is, answered with that row: reads are unchanged, so
  the same command could read it, and the key is taken.

## The independent review (2026-11-07), and what changed

A separate reviewer read `79eb2ce7`. Each finding was reproduced or settled
by a test before its repair, and each repair has a mutant in
`who_may_change_mutations.py`:

1. **(High) A reader's command could change any row by scheduling a
   command the clock runs.** `schedule forget(shelf, id) after ..` charges
   the reader's command only `clock.schedule`, so PW0370 did not see
   `forget`'s writes; and the clock's run, `invoked_by Clock`, was admitted
   only as the platform, which changes any row, with the arguments the
   reader chose. A command the clock invokes runs on the behalf of the
   reader whose command scheduled it (`invoked.rs`), so now:
   - PW0370 follows each `schedule` of a reader's command to the command it
     names, and each schedule that command makes in turn, and tells the
     write at the `schedule` (`a_write_a_readers_command_schedules_is_the_
     readers`);
   - `Writer::admits` takes a clock's run as the reader's who scheduled it
     (or the platform's, for a schedule the platform's own command made).
     **No scheduler reaches the table adapters yet**: when one does, it
     records the reader's ids (its session's and its user's) with each
     schedule and begins the run for them.
2. **(Medium) The put's race under read committed**: found the same day,
   before the review saw it fixed (`ac17131c`, above).
3. **(Low) The adapters answered `written` differently** for a row put back
   as it was for another owner (memory counted it, PostgreSQL did not). No
   read sees an owner, so neither counts it now; the memory adapter's owner
   lives in the tables commands write, where every write is decided, and the
   undo restores it (now with a positive control: Alice's own delete works
   after the undo, which a lost owner would refuse).
4. **Tests that proved less than they said**, each sharpened: the
   platform's fixture was itself refused (PW6402) and is now a valid
   `invoked_by Payments` command that checks clean; "no owner column" was
   any SQL error and is now the catalog's answer; the race's setup panicked
   outside an assertion, which the strict producer refused (the one INVALID
   mutant of the run at `ac17131c`).

Noted by the review, and not decided here:
- `invoked_by Payments`: a settlement acts on the order a reader named when
  the payment began, a confused deputy if that order is another's.
- A command that `requires Anyone` and writes an `owner user` table
  compiles; a signed-out reader's request then fails at its write. Pairing
  them at compile time needs a predicate the platform knows means "signed
  in".
- An older host still running when a newer one migrates the database
  writes unscoped until it stops: the schema is held to at a host's start,
  not at each transaction (D3's rule for every change, not this one's).

## The tables this migration says `owner anyone` (for the owner)

Each was written by commands any reader runs, and anyone could change any of
its rows; each now says so. The first candidates for `owner session`, as
decision 4 says, with what narrowing them would reach (findings 4 to 6):

| Table | Written by | A candidate for | What narrowing reaches |
|---|---|---|---|
| `examples/tables`' `Lists` | `make_list`, `insert_then_get`, `rename`, `insert_then_divide` | `owner session`: a visitor's lists | its `bounded 100` is one quota for every visitor (finding 5) |
| `examples/tables`' `Items` | `keep_item`, `drop_item`, `clear_list`, `put_then_open` | `owner session`: a visitor's items | an item may name another visitor's list (finding 4) |
| A-050's `Chores` | `add_chore`, `finish_room` | `owner session`: a household's chores | its `bounded 200`, as above |

No other program, fixture or test of the repository has a table a reader's
command writes. A table only the platform's commands write needs none, and
none exists yet.

## Not decided here, found while building

- A reference's meaning across owners (finding 4), and a bound per owner
  (finding 5).
- A read that answers a row only if it is the reader's (finding 6).
- A platform command that inserts a row for a reader: its rows are no
  reader's.

## Evidence

`docs/evidence/E14/owned-rows-2026-11-07/`, its `README.md` and the
commands that made each file.
