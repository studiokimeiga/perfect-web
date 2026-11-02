# ADR-XXXX: a table states whose its rows are, and a predicate may be written in Pleris

Status: proposed by track `row-ownership` (the coordinator's brief of
2026-11-02), building ADR-0384's H1 and H2 under the integrator's ruling on
ADR-0384 (codes, ADR-0115 amended, ADR-0279 and ADR-0129 confirmed). The
coordinator lands it after an independent review; the integrator numbers it.
Date: 2026-11-02, from master `27c1ffba`. Milestone: E14. Builds on ADR-0338
and ADR-0350 (declared tables), ADR-0385 and ADR-0386 (the table gate, reads
and writes), ADR-0115 (`requires`, amended here), ADR-0263 and ADR-0270 (a
handle is the platform's; the reader's user), ADR-0129 (a value's label
follows it), ADR-0302 (a predicate's words), ADR-0339's ruling 3 (`requires`
on a query).

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
   reader, a unique index holds the owner, `all()` refused). Writes take the
   owner's handle: `insert(me, row)`, `put(me, row)`, `delete(me, id)`,
   `delete_of_author(me)`. A program reads or writes no other user's rows
   by naming an id: a plain id is PW0605, a made handle PW5037, a shared
   cache PW5001 (ADR-0384's PW6902: the handle labels the read, one defect
   told once), a binding by hand PW0362.
3. **A private field** is a field of the row, never the owner, a sharer, in
   the key, the order of a table that shares, or an index a sharer reads by
   (PW0369): a field every reader selects or orders rows by tells its values.
   The seen row's name is the compiler's (PW0369).
4. **tables.json** states `owner`, `shares` and `private` per table and each
   function's `reader`, outside the schema: who reads a row is no part of
   what is stored, so `schema.lock` and its hash are unchanged.
5. **The host serves an owned function to its caller alone**, whatever
   adapter answers it. The engine is told who a call is made as
   (`tables::Caller`, from the session's principal, `call_with_tables_as`);
   before any adapter reads, it holds the handle the component passed to that
   caller, refuses every owned function to a call no one makes (an
   `invoked_by` command; `call_with_tables` is such a call), holds an
   `insert`'s or `put`'s row to its writer (PostgreSQL's `WITH CHECK`),
   gives a set form's handle back to each key, then answers a sharer the
   seen row (`engine::table_owners`). The manifest is held whole for its
   owners where the host reads it, and an owned import typed otherwise is
   refused where it links. The memory adapter needed no ownership of its
   own: what reaches it is a plain table's call.

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

6. A predicate the program gives a body answers a `Bool` (PW0606) and only
   reads (PW6905: no effect but `database.read` and `session.read`). It is
   compiled as a component of its own, kind `predicate`, never a route or a
   page's read; each export requiring it names that component in its
   `authorization` (`component`).
7. **`requires` belongs to a command or a query** (ADR-0339's ruling 3),
   its arguments the declaration's parameters.
8. **The order of asking** (`engine::authorize_export_with`): each
   requirement in the order the export states it; one naming a component is
   asked of that compiled predicate, and the deployment never; any other is
   the deployment's (`identity.rs`'s `holds`), as before. The first that does
   not hold refuses the invocation before its component runs. A command's
   predicate reads inside the command's own transaction
   (`Command::predicate_host_fns`), so what it read is what the command
   writes against; a query's reads the published tables.

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

## Evidence

`just e14-row-ownership` (`docs/evidence/E14/row-ownership.txt`), on GitHub
(verify run at the track's head): `pw-core --test table_owners` (16),
`--test predicate_bodies` (4), `pw-host --lib table_owners` (3),
`pw-table-memory --test compiled_owners` (9), `--test compiled_predicates`
(5); `scripts/row_ownership_mutations.py`, 27 controls (counts recorded by
the run). `tables_mutations.py`'s two re-anchored controls run whole with
`e14-tables`.

## Not claimed

- **The development server**: it serves no program's tables yet (track
  `data-path`). It must make each table call with
  `call_with_tables_as(.., Caller::User(<session principal's user>), ..)`,
  and an export with requirements with `call_authorized_with_tables_as`,
  asking a component predicate with `Command::predicate_host_fns` (a
  command) or `read_host_fns` (a query). Until then the browser specs
  (one user cannot see or change another's rows; a refused query told the
  predicate's words, three engines) are not written: they need that route.
- **PW6903**, a private field's value written into a row another reads
  (ADR-0129's write as a sink): not built. Reads are structural (the seen
  row); a program can still copy its own private value into a field a
  sharer reads.
- **ADR-0384's PW6906 to PW6908**: a body's reads with the policy's
  authority (ids in the owner's place, inside a body alone), a predicate
  calling another, set forms of a predicate, and a query required by a
  materialization (no reader to ask). Two-owner rows (`owned by a, b`),
  `shares when`, `written when` and `reveals` are H3's.
- **One snapshot for a query's predicate and its body** on memory: the
  published tables are read twice, so a commit between them is seen by one.
- **`!=` on an `Option`** does not lower in the backend; a body matches.

## Questions for the integrator

1. PW0367 to PW0369 are "next free after PW0366" (ruling 1); a concurrent
   track may want PW03xx too.
2. The guestbook (track `data-path`) could declare its posts `owned by
   author` and drop `OwnsPost`: `remove(post)` then deletes the reader's own
   post by construction.
