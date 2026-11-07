# ADR-0407: one model of row ownership, a field the platform checks

Status: accepted 2026-11-07, the integrator's ruling under the owner's
delegation of 2026-10-02, landed with train-14. Milestone: E14, the app
layer. Rules between two designs of one question that were built apart;
neither has landed.

## Context

Two designs say who owns a table's row:

- **A, `owned by <field>`.** ADR-0384 (accepted 2026-10-25) sketched it.
  The row-ownership track built it (`ff9ff2c6`, reserved as ADR-0395),
  and a later integration carried it with the data path, durable tables
  and PostgreSQL on an unpublished branch (`fix/row-data-integration`).
  - The owner is a visible `UserId` field. The program writes it, and the
    engine refuses a row whose owner is not the writer, with a handle only
    the platform makes (`capability.User`).
  - A table's key leads with its owner, so two owners' rows with one id are
    two rows. Every read leads with an owner's or a sharer's handle, and
    sharers see a row without its private fields.
  - Hosted evidence for the track (46 of 46 controls); browser checks on
    memory and PostgreSQL are local only.
- **B, `owner user | session | anyone`.** PR #91, built on 2026-11-07
  without knowing of A (its survey missed ADR-0384).
  - The platform records a hidden owner at insert; a `put` or `delete` of
    another reader's row is treated as absent. Reads are unchanged.
  - Declaring ownership is mandatory: PW0370 refuses a table that a
    reader's command writes and that says nothing.
  - 54 of 54 controls, and green hosted checks.

An independent comparison on 2026-11-07, which read both at their commits,
found B's flaw that neither of B's reviews had found:

- **Key squatting.** B's keys are global across owners. A `put` to a key
  no row holds records its writer as the owner, and a `put` to another
  reader's row returns as written and changes nothing
  (`runtime/pw-table-memory/src/lib.rs` at `c2130674`, the `Insert | Put`
  arm). So a reader who first writes a predictable key, such as a
  per-user settings row or a browser-chosen id, owns it. The rightful
  reader's later writes then succeed silently and are lost.
- B protects writes only. A program cannot list "my rows", show a row's
  author, or keep a row private.

## Decision

1. **A is master's model of row ownership.** A row's owner is a field the
   platform checks against the writer, its key leads with the owner, and
   reads are scoped by an owner's or a sharer's handle. It lands as its own
   PR from the integration branch, under its reserved ADR-0395. It is
   merged with master and re-verified on GitHub, and its codes are
   renumbered: ADR-0406's PW0367 landed first.
2. **PR #91 does not merge.** Its hidden owner column, its adapters'
   `Writer` and `Reader`, `owner session` and the `owner anyone` migration
   are dropped.
3. **What B found is carried into A's landing** (an amendment of
   ADR-0395):
   - ownership is declared, not optional: a table that a reader's command
     writes, directly or through a schedule, says `owned by` or that it is
     public on purpose (B's `written_by_readers`, merged with ADR-0384's
     PW6920);
   - a public-read, owner-written form, so that a guestbook's `remove` is
     structural rather than a hand-written check;
   - a clock run is admitted as the reader who scheduled it (B's
     `runs_for`);
   - B's race scripts, and its test that a reader's command is never the
     platform's.
4. **Two defects both designs share are fixed on master now,** each in a
   PR of its own:
   - the development server's scheduler keys a run by its command and
     arguments only, so one reader's schedule can move another's pending
     run: a run is keyed by the writer whose command scheduled it too;
   - D3's PostgreSQL `put` is answered from its statement's snapshot,
     which a concurrent command can make wrong: it is answered from the row
     the statement replaced.

## Alternatives

- **B as the model, with keys made per owner.** That removes squatting, but
  B would still protect writes only. The move to A's reads would then
  change every owned table's key, which no migration can do (PW0366).
- **Both, for different tables.** Rejected: two concepts of one thing, and
  AGENTS.md asks for one coherent architecture.
- **Neither until the dating app needs it.** Rejected: changing another
  reader's row by its id is the commonest broken access control (OWASP's
  first risk), and the tables a reader's commands write exist now.

## Consequences

- Until A lands, a table's rows are anyone's to change, as they are on
  master today, and ADR-0406's `requires` is the only check a command
  states.
- A's landing is large and its browser evidence local. It is split from
  the integration branch's later work (uploads, the outbox, the feed and
  store), which lands in PRs of its own.
- A enforces in the engine; host code that calls an adapter directly
  bypasses it. A's landing says where that holds.
