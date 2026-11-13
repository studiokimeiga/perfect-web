# ADR-0436: an Option a match takes apart where it is made is never made

Status: accepted 2026-11-12 (train-31, branch `perf/train-31-20261112`,
`b6f7b6db`), on trains 27 and 28; its full verify follows theirs (#106).
Under the owner's delegation of 2026-10-02 and order of 2026-10-25, and the
order of 2026-11-09 that every row Svelte wins be a defect until it is fixed
or recorded. Milestone: E14. Builds on ADR-0044, ADR-0057 and ADR-0433;
NEXT's first item after trains 27 and 28.

## Context

A browser's module holds an `Option` as `{ $case: "some", value }` or the
module's one `none` (ADR-0044), and a match switches on its case. The large
list's label is three words, each `match List.get(words, at %
List.length(words)) { Some(w) => w, None => "" }` (`examples/rows/app.pw`):
for each word a call to `some`, an object made, a string compared to choose
the arm, a field read, and the object left to the collector. "Create 1,000
rows" made three thousand of them, and so did "append" and "replace". A
press the head-to-head times is the first on a page just loaded (ADR-0432's
result), so it runs in an engine's first tier, where nothing removes an
object made only to be read once: V8's escape analysis does, in code it has
optimized, which these presses never reach. The examples call `List.get` 42
times and `List.find` 30, most of them matched where they are made.

## Decision

1. **An `Option` that `List.get`, `Map.get` or `List.find` makes, read only
   by the match after it, is never made.** The match is an `if` on the test
   that holds where it is `Some` — `i >= 0 && i < xs.length`, the place
   `Map.get` found, the index `List.find` found — and `Some`'s arm reads the
   payload where the test holds; `None`'s is the `else`. Only a match whose
   arms are `Some` alone and `None` alone, in either order, is taken apart
   so; a literal in `Some`'s pattern lowers to such arms.
2. **What a value is read by is counted before the body is written**
   (`ir::reads`): every instruction's operands (`Instr::operands`, which
   names every kind of instruction, so one added is a decision there), each
   region's value and the body's. One read, by the next instruction, is the
   only case taken apart; a value read again is made, and matched as any
   other.
3. **Each maker is opened in one place** (`opened`): the lines that come
   first, the test and the payload. An `Option` that is made is `test ?
   some(payload) : none` from the same three, so the two ways agree by
   construction; `Map.get` and `List.find` are no longer functions called
   where they are written (`List.find` is `findIndex`, the first that holds).
4. **Refused where it is built**: a body that names a value it left unmade
   (`unmade_unread`). A read `reads` did not count would be a name nothing
   declares, a `ReferenceError` where the page runs; the module is refused
   instead.

## Why not the other ways

- **The engine's**: an optimizing tier removes the object, and the presses
  the record times are not optimized.
- **A match on any `Option` as an `if` on `$case`**: the object is still
  made, and read.
- **In the lowering, for every backend**: the test (`i < xs.length`) is
  JavaScript's representation of a list; the Wasm component's is its own.
- **Moving a maker down to its match**: only the next instruction is
  looked at, so nothing is evaluated in another order than the program
  writes it.

## Result

- **Held**: the differential test's program gains six queries (`List.find`
  with its arms reversed, a wildcard arm, a literal in `Some`'s pattern, and
  an `Option` read again by an arm's value, by the body's and by `Some`),
  all agreeing with the component; `an_option_matched_where_it_is_made_is_never_made`
  names which modules make one, and failed on the base (`b7eb672e`). Six
  mutants added to `javascript_mutations.py` (the read count, a region's
  value, the body's value, a case's payload, the test's sense, `findIndex`)
  and two re-anchored; the eight run here all killed, every script run in
  full by the full verify.
- **Bytes**: the large list's three handlers that make rows 5,436 bytes to
  4,769 as built (1,733 to 1,616 with Brotli); the store's `dollars_typed`,
  which asks whether its text begins with `$`, 9,685 to 9,498 (its evidence,
  `7e11333c`); every other compiled handler as it was.
- **Time: within the rounds' spread.** The large list's two builds visited
  in turn by the head-to-head's own visit, eleven rounds a cell, this less
  train-30 (quartiles): "create 1,000 rows" −1.3 ms cold in Chromium
  (−4.2..5.7) and 0.0 warm (−0.6..2.5), −4.7 cold under Slow 4G (−13.6..1.4)
  and −8.0 warm (−10.3..4.0), 1.0 in Firefox cold (−7.0..3.0) and warm
  (−2.0..4.0); every press of a cold visit −3.4 in Chromium (−8.6..9.5). No
  press moved past its spread either way: three thousand small objects are
  less than a press timed to the frame after its change resolves.

## Consequences

- Kept for what it removes, nothing measured slower: an object, a call and
  a comparison for each `Option` a match takes apart where it is made, in
  every module the compiler writes.
- A profile of the first press, not the record, is what would say its share
  of the large list's presses (NEXT).
