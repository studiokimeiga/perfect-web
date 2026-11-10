# ADR-XXXX: a list's two places exchanged

Status: proposed 2026-11-10, on branch `perf/train-19-20261110`, under the
owner's delegation of 2026-10-02 and order of 2026-10-25 ("deciding by best
judgement without asking"). Milestone: E14. Extends ADR-0055's lists, as the
ranges' ADR does.

## Context

The large list swaps its second and its nine hundred and ninety-ninth rows,
as every keyed list benchmark does. Svelte writes it in five lines on an
array. Pleris wrote sixteen: two matches on `List.get`, each with a case for
a place the list does not have, and the list rebuilt from four slices. A
reader has to check every bound of it.

## Decision

**`List.swap(items, i, j) -> List<T>`**: the list with the elements at
places `i` and `j` exchanged, each counted from 0 as `List.get` counts, and
the list as it is where either is no place in it: a negative place, one past
the end, or the bounds of an `Int`. A list holds no place a program could
not name, so nothing traps.

An intrinsic, `list.swap`, in each backend: the component copies the list,
each element's bytes moved whole as `List.reverse` moves them, and shares the
list where a place is outside it; the module copies the array.

## Why not the other ways

- **`List.set(items, i, value)`.** The swap would still read both elements
  first, through two `Option`s.
- **Written in Pleris.** The sixteen lines this replaces.
- **A swap that traps outside the list.** `List.get` and `List.slice` answer
  a place outside the list without trapping; a swap that did would be the
  one operation of the standard library to.

## Acceptance

- `compiler/pw-conformance/tests/stdlib.rs`: against `Vec::swap`, Ints and
  records, places outside the list among them.
- `compiler/pw-conformance/tests/javascript.rs`: `Swapped`, and `SwapLeaves`
  (the list swapped from is as it was), among the 94 queries the component
  and the module agree on under Node.

## Consequences

- `examples/rows` swaps with `List.swap(rows, 1, 998)` in its handler.
