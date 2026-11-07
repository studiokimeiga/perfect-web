# ADR-XXXX: a row says who may change it

Status: proposed, 2026-11-07, on branch `feat/owned-rows-20261107`, stacked
on the requires ADR's branch (#83). The owner asked for "phase 2" of
mandatory `requires`: no command may change another reader's record by
naming its id. Not landed; not numbered (PARALLEL.md: the integrator numbers
ADRs).

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

## Evidence

`docs/evidence/E14/owned-rows-2026-11-07/` (to come).
