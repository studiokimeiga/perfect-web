# ADR-0266: an export's parameters past the flat limit arrive in memory

Status: accepted under the owner's delegation of 2026-10-02; found by the
uploads track (ADR-0260) and queued at its merge. Date: 2026-10-08.
Milestone: E14.

## Context

- **The Canonical ABI passes a function's parameters flat up to a limit**:
  `MAX_FLAT_PARAMS`, 16 core values. Past it, the caller stores them in
  memory, a tuple of them laid out as a record is, each at an offset
  aligned as its type is, and passes one pointer (wit-parser 0.257.1's
  `wasm_signature`, `indirect_params`, which this backend already reads).
  Calling a component's export, the host does this itself, allocating the
  tuple through the component's `cabi_realloc`.
- **The backend refused it**: "an export whose parameters exceed the flat
  limit". A timeline row's derived value takes its whole item as its
  export's parameters, and an `Item` with an `Option<Image>` flattened to
  18 values, so the uploads track carried a post's image as a list.
- **A value held in memory is the backend's already**: `Held::Memory`, a
  type and the local holding its address, beside `Held::Flat`'s locals.

## Decision

1. **Past the flat limit, each parameter is held where the host stores
   it**: the core export takes one pointer, and each parameter is
   `Held::Memory` at the pointer plus its offset in the tuple, the offsets
   accumulated in order, each rounded up to its type's alignment. No value
   is copied out of memory before it is read, so one parameter past the
   limit by itself, a record of 17 `Int`s, arrives too.
2. **Within the limit, nothing changes**: sixteen flat values arrive flat.

## Acceptance

- **`compiler/pw-conformance/tests/wide_parameters.rs`, 2 tests**, through
  the host: seventeen `Int` parameters, each weighted by its position so a
  value read from another's place answers another sum, beside sixteen,
  which arrive flat; a record of 17 fields after a `Bool`, at the next
  multiple of its alignment, the `Bool` read too, and a `String` after the
  record's 136 bytes.
- **`scripts/wide_parameter_mutations.py`, 4 mutants**: the parameters
  read as though they arrived flat, read unaligned, their offsets not
  accumulated, and each read at the pointer's start. Recorded by `just
  e14-wide-parameters`.
- **The whole workspace's tests**, the corpus among them.

## Not claimed

- **A call's arguments past the limit**, to a host's operation: the
  component would store them in its own memory and pass the pointer. It is
  refused as before ("a call whose arguments exceed the flat limit"), and
  no operation needs it yet.
- **The feed's image as a list stays**: `images: List<Image>` is the
  uploads track's choice, Twitter's own model, and a handler still takes no
  `Option` apart (ADR-0033).

## Amended 2026-11-17: an operation that needs a call past the limit

ADR-0440's asset contract suite found the first: a table's `put` takes its
row, so no compiled command can write a row wider than 16 flat values (14
for an owned table, whose `put` takes its owner's handle beside it). A
row of an id and two `Option<PostImage.Ref>` (a `Ref` is seven, its option
eight) is 17, and is refused at build ("a call whose arguments exceed the
flat limit"). The feed's post as a table's row would be 17 too, so L7 needs
a call's arguments past the limit: the component stores them in its own
memory, a tuple laid out as a record is, and passes the one pointer, as the
host does for an export here. It is NEXT's, before L7: made the same day,
below.

## Amended 2026-11-17: a call's arguments past the limit are passed in memory

ADR-0440's asset contract suite found the first operation that needs what
"Not claimed" left: a table's `put` takes its row, so no compiled command
could write a row wider than 16 flat values. A `Ref` is seven (its key and
generation, two each; its kind, width and height), an `Option<Ref>` eight,
so a row of an id and two images is 17; the feed's post as a table's row
would be 17 too, and L7 ports it.

**Decision.**

1. **Past the flat limit, a call's arguments are one tuple in the
   component's own memory**, laid out as a record is: each at its offset,
   the offsets accumulated in order, each rounded up to its type's
   alignment, the tuple allocated by `cabi_realloc` at its own alignment
   (the host refuses a pointer that is not). The core import takes the one
   pointer, and the host reads the arguments from it. An argument held flat
   is stored at its offset; one held in memory is in its canonical layout
   already, and is copied to its offset whole, a string's or a list's
   pointer among its bytes, into the same memory.
2. **Within the limit, nothing changes**: sixteen flat values are passed
   flat.
3. **An answer past its own limit** comes back through the return pointer
   as before, passed after the arguments' one.

**Acceptance.**

- `compiler/pw-conformance/tests/wide_arguments.rs`, 4 tests, through the
  host, which records what each operation received: seventeen `Int`s each in
  its place, held in memory (the export's own arrived past the limit) and
  computed in the body (each held flat), beside sixteen passed flat; a
  `Bool`, a record of 17 built in
  the body and a `String`, the record at the next multiple of its
  alignment; the same call with each argument held in memory (the export's
  own parameters arrived past the limit), after a name of odd length that
  leaves the region's top unaligned; and an operation past the limit both
  ways, its `String` answered through the return pointer.
- `scripts/wide_argument_mutations.py`, 6 mutants, each killed at a test's
  assertion (the strict producer): the arguments' alignment, their offsets'
  accumulation, a flat argument's and a held one's offset, a held one copied
  whole, and the tuple's alignment. Recorded by `just e14-wide-parameters`,
  beside the export's four.
- ADR-0440's asset contract suite keeps two images in its row (19 flat
  values), and holds a generation two fields name, kept while either does,
  on memory and on PostgreSQL alike, through the compiled command.

**Not mutated, each for its reason**: the call taking the one pointer at
all (a core module that passes flat values to an import that takes one
pointer does not validate, so no test runs it), and the tuple's size
(nothing is allocated between the stores and the call but the answer's
area, which the host writes after it has read the arguments).
