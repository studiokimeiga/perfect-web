# ADR-XXXX: a table states whose its rows are, and a predicate may be written in Pleris

Status: proposed by track `row-ownership` (the coordinator's brief of
2026-11-02), building ADR-0384's H1 and H2 under the integrator's ruling on
ADR-0384 (codes, ADR-0115 amended, ADR-0279 and ADR-0129 confirmed), and
repaired after two independent reviews under the integrator's rulings of
2026-11-02 (below). The coordinator lands it; the integrator numbers it
(ADR-0395). Date: 2026-11-02, from master `27c1ffba`, merged with master
`45e0c1a6` (D3) before the repair. Milestone: E14. Builds on ADR-0338 and
ADR-0350 (declared tables), ADR-0385 and ADR-0386 (the table gate, reads
and writes), ADR-0393 (D3, PostgreSQL), ADR-0115 (`requires`, amended
here), ADR-0263 and ADR-0270 (a handle is the platform's; the reader's
user), ADR-0129 (a value's label follows it), ADR-0302 (a predicate's
words), ADR-0307 (a page asks a predicate of its reader), ADR-0339's
ruling 3 (`requires` on a query).

## Corrections: what the second review found, and what changed

A second independent review, of `ff9ff2c6` (probes under the session's
`review-ro2-probes/`), found the first round's repairs sound (PW6924 in all
three shapes and across modules, PW0368, PW6925, PW6905 through helpers and
lambdas, the host's admission per call, the shared fuel, the predicate
before the body) and two new blockers, both from letting a query `requires`
a predicate, which master allows a command alone. Each is refused now, its
probe kept as a regression that fails without the repair (the compiler's
with this round's changes stashed, 4 of 4), beside a valid neighbour and a
mutation control.

1. **(Blocker) A query's predicate was asked of the first reader of a
   shared read alone.** PW6907 refused only an explicit `cache shared`. A
   public query with no `cache` clause compiles as cache `none`, privacy
   `public`; the development server keys its read by its arguments alone
   (`entry_key`), and `pw-resource` joins a read of a key already in flight.
   With `public query Club() requires SignedIn` and no cache clause, the
   anonymous reader alone was refused, and the same reader asking while
   bob's read was held in flight joined it and was answered the secret.
   **Now PW6907 holds every read of such a query to its reader**: a query
   that `requires` a predicate is cached `private` or is a `user` or
   `session` query (the server keys those by the reader's session), and no
   `materialize .. depends_on` names one (a materialization is kept for
   every reader alike, and asks none). Its sentence widens before landing.
   The review's server probe is kept (`required_queries`): the unkeyed
   program refused at build, and the race at the allowed shape, the
   anonymous reader answered by a read of its own, refused, joining no
   flight.
2. **(Blocker) Calling a guarded query skipped its predicate.** A query's
   body called from another declaration is compiled into it, and that
   declaration's contract requires nothing: `Wrapper() { Secret() }`, a
   `fn` around it, and `command peek() { Secret() }` each answered
   "SECRET" through the engine, and the server's `/lobby/p-nobodys` the
   secret through `Club(post)`. A command is held to its handler (PW0339);
   a query had no such rule. **Now PW6926** (`required_query_bound_by_a_page`):
   a query that `requires` a predicate is read by a page's binding alone,
   `query Q(..)` in a page, a view or a layout, which the host reads as a
   request of its own for the page's reader; a call to it, its name used as
   a value, or a binding of it in a query, a command, a function, a
   predicate or a materialization is refused. Every call site is checked,
   so one through a helper is refused at the helper.
3. **The ABI hash ignored who answers a requirement** (`schema_of`), against
   ADR-0115's amendment (decision 3): a command whose predicate the program
   gives a body, and the same command with none, hashed alike. **Now each
   requirement a component answers adds `@<component>` to `abi_schema`**; a
   requirement the deployment answers adds nothing, so every contract with
   no program predicate keeps its hash.
4. **`Predicate::new`'s checks had no test that failed without them** (the
   contract is one `predicate` export, requiring nothing of its own). **Now
   held**: a command's contract, a predicate requiring one itself, and two
   exports are each refused, each check with its control.
5. **Recorded, not changed** (H4): others can run up a predicate's cost by
   writing rows addressed to its reader, which nothing bounds without
   `written when` (H3) or bounded reads (H4), and so starve that reader's
   commands (Not claimed).

## Corrections: what the first review found, and what changed

An independent review of `485b10b2` (probes under the session's
`rowown-review/`) reproduced each of the following. Every one is refused
or held now, each with a regression that fails without its repair (the
compiler's run with the repair stashed, the host's manifest tests too, and
every one undone by its mutation control), a valid neighbour, and the
control itself (`scripts/row_ownership_mutations.py`, 46 of 46 killed).

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
   predicate is answered to its reader alone: every read of it is keyed by
   its reader, cached `private` or a `user` or `session` query, and no
   materialization depends on it (PW6907); and it is read by a page's
   binding alone, `query Q(..)`, where the host asks its reader, never
   called, named or bound by a query, a command, a function, a predicate or
   a materialization, into whose body its own would be compiled unasked
   (PW6926), as a command is sent by a handler alone (PW0339). Who answers
   each requirement is part of the contract's `abi_schema`.
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
  verify 41491093145 at `83761ca3` (fetched by `just evidence-fetch
  41491093145 --any-commit --known summary`): `pw-core --test table_owners`
  20, `--test predicate_bodies` 7, `pw-host --lib table_owners` 4,
  `pw-table-memory --test compiled_owners` 11, `--test compiled_predicates`
  10; `scripts/row_ownership_mutations.py`, baseline 77 passed, **46 of 46
  controls killed**, each at a native assertion. Before the review, 27 of
  27 (verify 41441594505 at `cfc32cde`).
- The same run: every other recipe the track's change reaches (`ci_plan.py
  --changed 45e0c1a6 HEAD --current`), and `e14-d3-postgres` beside them,
  since the host's manifest check runs inside D3's adapter. 50 of 51 passed
  with every control killed, among them `e14-tables` 44 of 44,
  `e14-table-host` 49 of 49, `e14-d2-writes` 69 of 69 and `e14-d3-postgres`
  (49 tests, 53 of 53 on PostgreSQL). **`e14-check-is-build` failed**: its
  census (`scripts/check_build_programs.py`) held no program for
  `examples/owned/`, the track's fixture since its first round, which no
  run of that recipe had reached before. The census checks it now, as its
  own program (`1e237ccf`), and the recipe passed again on its own, the
  census being all that changed since `83761ca3` (`ci_plan.py --changed
  83761ca3 1e237ccf` plans it alone): verify 41519854384, `owned: clean`,
  657 of 657 tracked sources covered, 51 of 51 controls killed, and the
  browser suites in three engines green beside it.
- CI: run 41489790837 at `83761ca3`, every job green. The repair's first CI
  run (41485542822 at `0f63750f`) found two things the local runs had not:
  the concurrency test's log raced the command it let in (the
  serialization held; "edit committed" was written after the commit
  returned), and `pw-resource` pins the accepted corpus's written
  resources (A-058 adds two). Both were fixed in `83761ca3`, the test read
  where each fact is made, then 100 runs clean locally, 25 of them beside
  forty busy loops; the verify dispatched at `0f63750f` was cancelled for
  them.
- Locally (Linux, PostgreSQL 18): `cargo test --locked -p pw-core`, every
  binary (243); `-p pw-host --features engine`, `-p pw-table-memory`, `-p
  pw-cli`, and the workspace less `pw-core` and `pw-dev-server`, every
  binary; `PW_TABLES_DATABASE_URL=… cargo test --locked -p
  pw-table-postgres` 49; `just preflight`, `just test-compile` and `just
  evidence-gates` (1169 tests). Each new compiler test failed with the
  compiler's changes stashed (6 of 6), and the host's manifest tests with
  the host's.
- The corpus: R-114 to R-116 beside A-056 to A-058; a GENERAL witness and a
  neighbour for each new invariant (generality 72 / 72);
  `docs/evidence/E2D/corpus-enforcement.txt` 102 / 102 (`just
  evidence-corpus`).

## Not claimed

- **The development server** (historical boundary, superseded locally by
  the 2026-11-03 integration below): it served no program's tables yet
  (track `data-path`). It must make each table call as the session's principal
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
- **One snapshot for a query's predicate and its body** on memory was
  unproved at the row track's original boundary. The 2026-11-03 integration
  gives `Snapshot` a `CallTables` implementation and a concurrent-removal
  witness; see the correction below. The live `MemoryTables` provider
  remains live and must not be selected for a consistent query invocation.
- **The compiled predicate is the embedder's to load**: the engine runs the
  component registered under a requirement's id, held to one `predicate`
  export, and does not hold its bytes to the build, as for every component
  a deployment loads.
- **`shares with` without `written when`** (H3's PW6911): anyone may write a
  row addressed to anyone, so a sharer's view holds whatever others address
  to them.
- **A predicate's cost is others' to run up** (the second review's note, H4):
  a predicate whose body reads the rows addressed to its reader (a sharer's
  index) reads as many as others choose to write, unbounded by `written
  when` (H3) or by bounded reads (H4), and spends its fuel from the
  invocation's one budget, so others can starve that reader's commands.
- **A reader's key is its session's**: the development server keys a
  `private` or `user` read by the session, not the user, so one user's two
  sessions read apart; never two users' together.
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
- **ADR-0384's PW6906 and PW6908**: a body's reads with the policy's
  authority (ids in the owner's place, inside a body alone), a predicate
  requiring another, and set forms of a predicate. (A query that requires
  read by a materialization, which this ADR listed here, is refused now by
  PW6907 and PW6926.) Two-owner rows (`owned by a, b`), `shares when`,
  `written when` and `reveals` are H3's.
- **A page asking a predicate whose body reads a table** is told PW0401 ("a
  page may not perform `database.read`") beside PW6925: the effect
  inference counts the ask as a call of the body. One defect told twice,
  until a host asks the body for the document.
- **`!=` on an `Option`** does not lower in the backend; a body matches.

## Next

### Local integration correction, 2026-11-03

The preserved row work and declared-data routing now share one local tree.
`Snapshot` supplies both predicate and body reads through `CallTables`.
The generic server selects one snapshot per table query (the render's when
present), passes its invocation-bound principal as caller, loads the compiled
predicates named by each requirement, and gives command predicates reads on
that command's staging. The engine still evaluates them and owns shared fuel.
An explicit denial is carried by `InvocationError::refused`, separately from
evaluation failures; the HTTP route tells the program's words without parsing
error text. Requirements without a compiled body remain the deployment's.

Focused native witnesses cover two users, forged handles and writes, shared
projections, command and page-query predicates, a missing artifact, pure
predicates, typed denial versus an error with denial-like text, and a commit
between a standalone query's predicate and body. This is local implementation
evidence. Code/test source `497c0a6c` subsequently passed whole row (68/68),
data-path (60/60), both refusal parts (23/23 and 7/7) and local CI; independent
final review is clear. The receipts are retained in
`docs/evidence/E14/row-data-integration/`. This does not prove
the row track's requested browser behavior in three engines, PostgreSQL
`CallTables`, private write-flow or H3/H4. See
`docs/diagnostics/row-data-integration-2026-11-03.md` for source and receipts.

The old identity-cookie limitation above is also historical in this combined
tree: the data-path branch already uses domain-separated guest/session
digests. This integration preserves that implementation and its controls.

Review correction: a reader's own cached guarded query could outlive the
predicate that allowed it. A reused cached, deduplicated or fallback answer
must pass the same engine authorization against the new invocation's read
state, without running the export merely to authorize reuse. A denial cannot
fall back to the old protected value. Fresh executions retain one shared
predicate/body budget. Qualified program predicates report their declared
name for authored refusal words. Query denials never become a refreshing
command's refusal record. These refinements preserve ordinary declared data
freshness; they do not claim authorization at HTTP/frame delivery time.

- **H4**: a bound per owner (`bounded 3 per owner`), cursors, and a bound on
  what a predicate's reads may cost its reader, whoever wrote the rows.
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

1. **Codes**: the repairs' refusals are in the track's PW69xx block, per the
   rulings: PW6924 (references), PW6925 (a body answered by it alone) and
   PW6926 (a guarded query read by a page's binding alone) next free after
   ADR-0384's planned PW6901 to PW6923, and PW6907, ADR-0384's code for a
   query's `requires`, for its reads keyed by their reader (its sentence
   widened by the second review, revision 1, since it has not landed).
   PW6924 is a table clause's refusal, which ADR-0384's ruling 1 put in
   PW03xx; PW69xx keeps it clear of `data-path`'s next PW03xx. Confirm, or
   renumber at landing. PW0367 to PW0369 remain "next free after PW0366".
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
5. **Figures for STATUS and NEXT**, which this track does not edit:
   generality 73 / 73 (was 69), corpus enforcement 103 / 103 (was 99), 228
   registered codes (was 224), and the accepted corpus's written resources
   22 (`pw-resource`, was 20).
6. **A server test module** (`spikes/own-renderer/server/src/tests/required_queries.rs`,
   registered in `main.rs`'s list) keeps the second review's server probes;
   `data-path` edits that file's neighbourhood, a one-line merge.
7. The guestbook (track `data-path`) could declare its posts `owned by
   author` and drop `OwnsPost`: `remove(post)` then deletes the reader's own
   post by construction.

## Browser and durable continuation, 2026-11-03 (locally verified; hosted landing pending)

The row-specific browser fixture uses the declared Notes tables, independent
browser contexts and the real development account provider. It must prove
same-key isolation, refused forged writes, shared projections without drafts,
and a guarded page answered 403 in the predicate's authored words. A query
denial must retain its typed identity across the query cache boundary; strings
that resemble denials remain ordinary failures. No command-refusal TLS is used.
PostgreSQL command predicates must read the command transaction; snapshot
predicates and bodies must share the same repeatable-read transaction. A real
concurrent conflicting write must refuse the serializable command. Durable
server wiring will reuse the existing durable-tables implementation after
reviewing its delta, while preserving its original dirty receipts.

Primary references checked: Playwright BrowserContext API (independent
contexts and shared-context requests), PostgreSQL 18 transaction isolation
(repeatable-read snapshots and serializable conflict refusal). Existing pinned
dependencies are retained; no upstream package upgrade is needed.

The generic query path retains `InvocationError` until it becomes an explicit
`Unread::Refused` at the page boundary. `pw-resource` still transports failure
strings: a load retains its typed error locally, and a failed joined flight
asks this invocation's requirements for its own typed refusal. Ordinary errors
cannot acquire refusal identity from their text. Both deployment predicates
and compiled predicates take the typed engine path. HTTP answers a refusal
403 with escaped authored words, after the existing delivery fence. Query
failures never write the command's thread-local refusal record.

The dedicated `examples/owned/pages.pw` and `e2e/owned-rows.spec.mjs` exercise
two actual development accounts, same-key rows, forged authors, private draft
projection, and a guarded query's words in Chromium, Firefox and WebKit.
`e14-row-ownership` builds and runs them in memory; `e14-durable-tables` runs
the identical spec on PostgreSQL. Fresh final-source campaign receipts remain
the acceptance boundary. H3/H4 and private write-flow are unchanged.

### Verification recipe correction, 2026-11-03

The f81e200c whole runtime campaigns completed all272controls, but local CI
failed three tooling tests: the stand-in tree did not stub the new owned.sh
builder, and the strict browser-status checker rejects an OR-list even where
its right side exits. The fixture now includes that builder and the browser
helper runs directly under the existing errexit/pipefail shell and receipt
trap. The checker and the failure-injection tests are unchanged. Rerun the
complete local gates at the corrected commit; no f81 CI success is claimed.

The recipe also builds the shared runtime fixture explicitly, as the browser
recipe contract requires. A failed suite's temporary report is retained as
the canonical row receipt before the trap exits with the original status,
so its failure remains reviewable. The existing build-order and injected
failed-suite tests cover both behaviors; run these boundary checks when
registering a browser build or converting a native-only recipe to browsers.

### Canonical fixture formatting, 2026-11-03

The b89051a0 runtime campaigns completed 272 controls, and CI's native
workspace tests passed (server 951 passed, 9 ignored). The same CI command
then failed `test-compile`: the new owned-row page fixture was not in the
canonical Pleris format. It receives no complete-CI acceptance credit.
The formatter changes seven lines of spacing only (`git diff -w` is empty).
The existing `test-compile` gate already checks `examples/owned/*.pw`;
`preflight` does not run that Pleris formatting check. Run `test-compile`
and the remaining compiler controls before freezing the successor, then
complete every local gate at that source. No checker or bound was changed.

### Completed local verification, 2026-11-03

Code/test source `6b31f4ba3468fdd50d0fb4ce357a86b0e571f9b6` completed the whole row 71/71,
data-path 60/60, D3 PostgreSQL 60/60, durable 51/51 and refusal 30/30 controls,
plus local CI. Owned-row browser checks pass 6/6 on memory and 6/6 on PostgreSQL,
in Chromium, Firefox and WebKit. Independent final review is clear. Raw
receipts, original-worktree preservation and generated CI-input hashes are in
`docs/evidence/E14/row-browser-postgres/`. This closes the local browser and
PostgreSQL `CallTables` obligations, superseding the earlier pending wording;
remaining H3/H4, private write-flow and deployment limits are unchanged.
