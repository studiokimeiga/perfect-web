# ADR-XXXX: a table states whose its rows are, and a predicate may be written in Pleris

Status: proposed by track `row-ownership` (the coordinator's brief of
2026-11-02), building ADR-0384's H1 and H2 under the integrator's ruling on
ADR-0384 (codes, ADR-0115 amended, ADR-0279 and ADR-0129 confirmed), and
repaired after its independent review under the integrator's rulings of
2026-11-02 (below). The coordinator lands it; the integrator numbers it
(ADR-0395). Date: 2026-11-02, from master `27c1ffba`, merged with master
`45e0c1a6` (D3) before the repair. Milestone: E14. Builds on ADR-0338 and
ADR-0350 (declared tables), ADR-0385 and ADR-0386 (the table gate, reads
and writes), ADR-0393 (D3, PostgreSQL), ADR-0115 (`requires`, amended
here), ADR-0263 and ADR-0270 (a handle is the platform's; the reader's
user), ADR-0129 (a value's label follows it), ADR-0302 (a predicate's
words), ADR-0307 (a page asks a predicate of its reader), ADR-0339's
ruling 3 (`requires` on a query).

## Corrections: what the review found, and what changed

An independent review of `485b10b2` (probes under the session's
`rowown-review/`) reproduced each of the following. Every one is refused
or held now, each with a failing-first regression, a valid neighbour and a
mutation control (`scripts/row_ownership_mutations.py`).

1. **(Blocker) A reference check read and blocked another owner's rows.**
   Neither the compiler's ownership rules (PW0367 to PW0369) nor the host's
   manifest check looked at `refers`, and the memory adapter's commit check
   tests referring and referred rows across every owner. With `Premiums`
   (owned by `user`) and `Crushes` (`owned by from`, `refers target to
   Premiums`), eve learned whether bob held a premium row by whether her own
   write naming him committed, and bob could not cancel his own while hers
   named him. PostgreSQL checks a foreign key past its row security, so a
   durable adapter inherits it. **Now refused, PW6924**
   (`reference_within_one_owner`): a reference into an owned table is made
   by an owned table from its own owner field alone, so it names its
   owner's own row (the referred table's key is its owner, PW0368); a table
   no one owns refers to no owned table; and an owned table refers to no
   table no one owns, since deleting the row it names would be refused
   because of a private row, which the refusal reveals, until a reference
   states what deleting does (Next). **The host refuses the same shapes**
   where it reads the manifest (`Manifest::references_within_one_owner`),
   and with them a reference by a field not its table's or to a table the
   manifest lacks; the memory and PostgreSQL adapters read their manifest
   through it, so neither opens such tables. No example or fixture used a
   refused shape (`examples/owned/` has no `refers`).
2. **`bounded` on an owned table was one quota across every owner**: eve's
   three votes refused alice's first and told her the total. **Now refused**
   by PW0368, whose sentence widens before landing ("no bound counts its
   rows across owners": a write refused by a bound is a read of every
   owner's count, as one refused by a unique value is), and by the host's
   `owned_whole`. A bound per owner is H4's (Next).
3. **The deployment answered a predicate the program gave a body** on the
   old path: `authorize_export` and `call_authorized_within` ignored the
   requirement's `component`, and with `predicate SignedIn() { false }` the
   development server's evaluator answered its own `SignedIn` and the
   command ran. **Now `authorize_export` refuses a requirement naming a
   component unasked** ("could not be evaluated: it is the program's"), so
   every path built on it does: `call_authorized_within`, the development
   server's, and data-path's `call_authorized_with_tables` once merged.
4. **A predicate with a body not imported where it is required became the
   deployment's**: `contract::asked_by` resolved nothing and named no
   component, and the PW0351 check skipped it as the deployment's. **Now
   refused, PW6925** (`predicate_answered_by_its_body`): a `requires X` in
   a module that sees no `X`, where the program gives an `X` a body
   elsewhere, is told to import it. Found while repairing it, the same
   hole in pages: a page's `{#if X}` (ADR-0307) is answered by the
   deployment by name (`identity.rs`'s `answers`), so a page asking a
   predicate with a body is refused by PW6925 too, until a host asks the
   body there (Next). A body's and a deployment's declaration of one name
   are PW0351's already: a predicate is declared once.
5. **The predicate's transaction, caller and limits were the embedder's**:
   `authorize_export_with` handed a program predicate to a `program`
   callback, given neither the caller nor the command's reads, and
   `limits` bound the guarded component alone; the track's own harness ran
   every predicate unbounded, and an exponentially recursive one ran past
   five seconds holding the memory adapter's one open command. **Now the
   engine asks it itself** (`engine::program_predicates`; decision 8).
   `authorize_export_with` is gone: no embedder evaluates a program
   predicate.
6. **Tests that passed falsely.** Asking the predicate after the body passed
   all five `compiled_predicates` tests, the refusal protected only because
   the harness committed nothing a refusal answered; a write check reading
   a row's first field passed everything, every fixture putting its owner
   first; and ADR-0384's "a concurrent change between predicate and body
   refused by serializability" test did not exist. **Now**:
   `a_refused_predicate_leaves_nothing_committed_and_its_body_never_runs`
   commits whatever the command staged and counts the body's table calls;
   `Likes { to, from } owned by from` is written by its owner, not its first
   field (`an_owner_who_is_not_the_rows_first_field_writes_their_rows_alone`);
   and `a_change_between_a_predicate_and_its_body_waits_for_the_command`
   runs a removal of the note between `edit`'s predicate and its body:
   the memory adapter makes it wait its turn until the edit commits. That
   proves the memory adapter's serialization, with the predicate inside
   its one open command; a durable adapter, whose transactions run at once
   and are refused on conflict, must prove it again with its own (Next).
7. **Notes.** (a) The owner check passed a row it could not read
   (`table_owners.rs`, "the write decoder's to refuse"): **now refused**
   before any adapter, a row not exactly the table's. (b) `cache shared` on
   a query that `requires` a predicate compiled: **now refused, PW6907**
   (`required_query_unshared`, ADR-0384's code for a query's `requires`),
   whichever answers the predicate; a `user` or `session` query in a
   shared cache is PW5001's already. (c) ADR-0115's amendment was pasted
   twice, the first copy cut off: **fixed**, and amended for decisions 3
   and 5 above. (d) What the review noted and this track does not change is
   under Not claimed.

Also found while repairing: **merging D3 left `pw-table-postgres` unbuilt**
(E0063: `migrate::shapes_of` makes a `TableJson`, which this track gave
`owner`, `shares` and `private`). A recorded schema states no owner, as
who reads a row is no part of what is stored; fixed in its own commit, and
D3's 49 tests pass on PostgreSQL 18 with it and with the host's new
manifest checks.

## Context

The commonest web bug class is a read or write of another user's object
named by its id (broken object-level authorization, OWASP API1). Until now a
declared table's generated reads took plain ids, so `public query` could read
anyone's rows, and "only its author may remove a post" was a Rust predicate
(`identity.rs`'s `OwnsPost`) the deployment held, outside the program
(ADR-0384, gap 1 to 3).

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
   `capability.UserId` (PW0367); **`shares with <fields>`** the fields
   holding the ids of the others who read a row (PW0367: each a
   `UserId`, never the owner); **`private <fields>`** what its owner alone
   reads. `shares with <Predicate>` is ADR-0384's `shares when`, H3's, refused
   as not built (PW0367 says so).
2. **Every generated read takes the reader's handle**, a `capability.User`
   only the platform makes (ADR-0263), in the place of the field it leads
   with: the owner's reads answer the row (`Notes.of_author(me)`), a
   sharer's answer the **seen row** (`Notes.by_to(me) -> List<NoteSeen>`),
   the record the compiler declares beside the row without the private
   fields, so no page, cache entry or answer of theirs can hold one. A set
   form takes the handle and a list of what follows it, and none is given
   where nothing follows; no read leads with another field and none reads
   the table whole (PW0368: the key leads with the owner, each index with a
   reader, a unique index holds the owner, `all()` refused, and **no
   bound**, which would count every owner's rows). Writes take the
   owner's handle: `insert(me, row)`, `put(me, row)`, `delete(me, id)`,
   `delete_of_author(me)`. A program reads or writes no other user's rows
   by naming an id: a plain id is PW0605, a made handle PW5037, a shared
   cache PW5001 (ADR-0384's PW6902: the handle labels the read, one defect
   told once), a binding by hand PW0362.
3. **A private field** is a field of the row, never the owner, a sharer, in
   the key, the order of a table that shares, or an index a sharer reads by
   (PW0369): a field every reader selects or orders rows by tells its values.
   The seen row's name is the compiler's (PW0369).
4. **A reference stays within one owner's rows** (PW6924): a database holds
   a reference across every row of both tables, whoever owns them, so an
   owned table refers to an owned table from its own owner field alone,
   naming its owner's own row (`refers member to Memberships`, A-057); a
   table no one owns refers to no owned table; an owned table to no table no
   one owns. A reference between two tables no one owns is D1's, as before.
5. **tables.json** states `owner`, `shares` and `private` per table and each
   function's `reader`, outside the schema: who reads a row is no part of
   what is stored, so `schema.lock` and its hash are unchanged.
6. **The host serves an owned function to its caller alone**, whatever
   adapter answers it. The engine is told who a call is made as
   (`tables::Caller`, from the session's principal, `call_with_tables_as`);
   before any adapter reads, it holds the handle the component passed to that
   caller, refuses every owned function to a call no one makes (an
   `invoked_by` command; `call_with_tables` is such a call), holds an
   `insert`'s or `put`'s row to its writer by its owner field, wherever that
   is in the row, and refuses one not exactly the table's (PostgreSQL's
   `WITH CHECK`), gives a set form's handle back to each key, then answers a
   sharer the seen row (`engine::table_owners`). The manifest is held whole
   for its owners where the host reads it, its bounds and references among
   it, and an owned import typed otherwise is refused where it links. The
   memory adapter needed no ownership of its own: what reaches it is a plain
   table's call.

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

7. A predicate the program gives a body answers a `Bool` (PW0606) and only
   reads (PW6905: no effect but `database.read` and `session.read`). It is
   compiled as a component of its own, kind `predicate`, never a route or a
   page's read; each export requiring it names that component in its
   `authorization` (`component`). It is answered by that body alone
   (PW6925): a `requires` naming it sees it where it is written, imported as
   any declaration is, and no page asks it of its reader yet.
8. **`requires` belongs to a command or a query** (ADR-0339's ruling 3),
   its arguments the declaration's parameters. A query that requires a
   predicate is answered to its reader alone, never cached shared (PW6907).
9. **The engine asks a program's predicate itself**
   (`Prepared::call_authorized_with_tables_as`, `engine::program_predicates`):
   each requirement in the order the export states it, before the export's
   component is instantiated. One naming a component is asked of the
   program's compiled predicate, from `Predicates` (each `Predicate::new`
   held to one `predicate` export requiring nothing of its own; one missing
   refuses the invocation): run as the export's `Caller`, with the same
   platform operations, so `current_user()` and every owned read answer the
   same reader; its reads from the same `CallTables` as the export's table
   functions (the memory adapter's `Command`: the command's own
   transaction; `MemoryTables`: the published tables a query reads), so
   what it read is what the export reads and writes against; under the
   export's `Limits`, its fuel spent from the invocation's one budget, so a
   predicate and the body it guards cost what one invocation may. Any other
   requirement is the deployment's (`identity.rs`'s `holds`), as before. The
   first that does not hold, or cannot be asked, refuses the invocation
   before its component runs, and no other path asks a program predicate:
   `authorize_export`, the deployment's, refuses one unasked.

## Alternatives

- **The owner field typed as the handle** (`author: User<UserId>`): writes
  would be refused at compile time, but a sharer's row would carry another
  user's handle; an id that grants nothing (ADR-0270) and the host's write
  check were chosen.
- **Enforcement in each adapter**: D3's and every later adapter would have
  to repeat it; the engine's one place serves them all.
- **The predicate compiled into the guarded component's body**: the host
  could no longer ask it before instantiation, and a component would
  authorize itself (ADR-0115's point); a second export in one component
  would break the one-export contract the staged adapter relies on.
- **A reference across owners allowed, its check moved out of the
  database** (the adapter checking only rows the writer may read): a
  missing referred row would then be written, which a reference exists to
  refuse, and PostgreSQL's foreign key could not be used. Refused instead,
  until a reference declares what deleting does (Next).
- **An embedder callback for a program predicate, given the caller and the
  reads** (the review's first suggestion): an embedder could still run it in
  another transaction or unbounded. The engine running it from the call's
  own `CallTables` leaves nothing to get wrong.

## Evidence

- `docs/evidence/E14/row-ownership.txt`, by `just e14-row-ownership` on
  verify 41441594505 at `cfc32cde` (before the review; fetched by `python3
  scripts/evidence_fetch.py 41441594505 --any-commit`): 27 of 27 controls
  killed. **The repaired track's record is to be fetched from a verify run
  at its head** (the integrator's; see Questions): the tests and 46
  controls, each new rule's and test's among them.
- Locally, at the repair's working tree (Linux, PostgreSQL 18):
  `cargo test --locked -p pw-core --test table_owners --test
  predicate_bodies` 20 and 10 passed; `-p pw-host --features engine` every
  binary passed; `-p pw-table-memory` every binary passed
  (`compiled_owners` 11, `compiled_predicates` 10);
  `PW_TABLES_DATABASE_URL=… cargo test --locked -p pw-table-postgres` 49
  passed. Each new compiler test failed with the compiler's changes
  stashed (6 of 6), and the host's manifest tests with the host's.
- The corpus: R-114 to R-116 beside A-056 to A-058; a GENERAL witness and a
  neighbour for each new invariant (generality 72 / 72);
  `docs/evidence/E2D/corpus-enforcement.txt` regenerated.

## Not claimed

- **The development server**: it serves no program's tables yet (track
  `data-path`). It must make each table call as the session's principal
  (`call_with_tables_as(.., Caller::User(..), ..)`) and an export with
  requirements through `call_authorized_with_tables_as`, its staged command
  or the query's tables as `CallTables` and the build's `Predicates`. Until
  then the browser specs (one user cannot see or change another's rows; a
  refused query told the predicate's words, three engines) are not written.
- **PostgreSQL**: no `CallTables` for `pw-table-postgres`'s command or
  snapshot, so a program predicate is not asked there yet (the embedder has
  none to give, so it fails closed), and "a concurrent change between
  predicate and body" is proven on the memory adapter only, whose one open
  command serializes by construction; PostgreSQL proves it with its own
  serializable transactions, refused on conflict, not here.
- **Time**: `Limits` bounds no call by wall-clock time, the guarded
  export's included; fuel bounds a predicate's running as it bounds every
  call's (the review's exponential predicate stops out of fuel under five
  million). A wall-clock deadline for every call (Wasmtime's epoch
  interruption) is a host-wide change, not built here.
- **One snapshot for a query's predicate and its body** on memory: the
  published tables are read twice, so a commit between them is seen by one
  (data-path's `Snapshot`, as a `CallTables`, would give one).
- **The compiled predicate is the embedder's to load**: the engine runs the
  component registered under a requirement's id, held to one `predicate`
  export, and does not hold its bytes to the build, as for every component
  a deployment loads.
- **`shares with` without `written when`** (H3's PW6911): anyone may write a
  row addressed to anyone, so a sharer's view holds whatever others address
  to them.
- **`Caller` carries no session generation**: an in-flight call's owned
  reads and writes outlive a revocation (a sign-out) until the server's
  wiring fences them (ADR-0389).
- **Ownership is opt-in**: a table holding a `UserId` with no `owned by` is
  the program's public data; ADR-0384's default deny (PW6920) is not built.
- **Identity**: the guest's user id is derived from the session cookie
  (being fixed on `data-path`), and ADR-0258's `sub` is unique per issuer
  alone, so an owner's id is one issuer's.
- **A shared row tells what it refers to**: an owned table that shares its
  rows and refers from its owner field tells each sharer that the owner's
  referred row exists; that is the owner's own fact, disclosed by the
  program's own declaration, as a private value copied into a shared row
  would be (PW6903, not built).
- **PW6903**, a private field's value written into a row another reads
  (ADR-0129's write as a sink): not built. Reads are structural (the seen
  row); a program can still copy its own private value into a field a
  sharer reads.
- **ADR-0384's PW6906 and PW6908**, and PW6907's other half: a body's reads
  with the policy's authority (ids in the owner's place, inside a body
  alone), a predicate calling another, set forms of a predicate, and a
  query that requires a predicate read by a materialization (no reader to
  ask). Two-owner rows (`owned by a, b`), `shares when`, `written when` and
  `reveals` are H3's.
- **`!=` on an `Option`** does not lower in the backend; a body matches.

## Next

- **H4**: a bound per owner (`bounded 3 per owner`), and cursors.
- **H3 and H9**: a reference that states what deleting the row it names
  does (cascade, set none, or a refusal told to its owner alone), so an
  owned table may refer to a table no one owns; and erasure.
- **data-path**: the server's seam calls `call_authorized_with_tables_as`
  with its staged command or query snapshot as `CallTables` and the build's
  `Predicates`; its `Snapshot` implements `CallTables`, so a query and its
  predicate read one snapshot; the feed's `MayMessage` and the store's
  `MayRate` become bodies, their arms in `identity.rs` go.
- **D3 and durable tables**: `CallTables` for PostgreSQL's command and
  snapshot, and the concurrency test again there, a conflicting command
  refused by serializability.
- **A wall-clock deadline for every call**, a host track of its own.
- **A page asking a program predicate**: the host asks the body for the
  document, and PW6925's page clause lifts.

## Questions for the integrator

1. **Codes**: the repair's refusals are in the track's PW69xx block, per the
   ruling: PW6924 (references) and PW6925 (a body answered by it alone)
   next free after ADR-0384's planned PW6901 to PW6923, and PW6907,
   ADR-0384's code for a query's `requires`, for the shared cache. PW6924
   is a table clause's refusal, which ADR-0384's ruling 1 put in PW03xx;
   PW69xx keeps it clear of `data-path`'s next PW03xx. Confirm, or renumber
   at landing. PW0367 to PW0369 remain "next free after PW0366".
2. **The bound is PW0368's** (its sentence widened, revision 1, since the
   code has not landed) rather than a new code: a write refused by a bound
   reads every owner's count, as one refused by a unique value does.
3. **`call_authorized_with_tables_as` changed shape** (platform operations,
   a `CallTables`, the program's `Predicates`, no `program` callback), and
   `authorize_export_with` is gone. `data-path`'s
   `call_authorized_with_tables`, deployment-only, refuses a program
   predicate through `authorize_export` once merged; its seam should take
   the engine path.
4. **PW6925's page clause** is a finding beyond the review's: a page asking
   a predicate with a body was answered by the deployment by name.
5. The guestbook (track `data-path`) could declare its posts `owned by
   author` and drop `OwnsPost`: `remove(post)` then deletes the reader's own
   post by construction.
