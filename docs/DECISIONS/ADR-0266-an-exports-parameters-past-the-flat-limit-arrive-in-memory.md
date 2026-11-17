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
row, so no compiled command can write a row wider than 16 flat values. A
row of an id and two `Option<PostImage.Ref>` (a `Ref` is seven, its option
eight) is 17, and is refused at build ("a call whose arguments exceed the
flat limit"). The feed's post as a table's row would be 17 too, so L7 needs
a call's arguments past the limit: the component stores them in its own
memory, a tuple laid out as a record is, and passes the one pointer, as the
host does for an export here. It is NEXT's, before L7.
