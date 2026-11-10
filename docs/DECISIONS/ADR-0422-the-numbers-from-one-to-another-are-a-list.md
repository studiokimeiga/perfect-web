# ADR-0422: the numbers from one to another are a list

Status: accepted 2026-11-10, landed with trains 18 and 19. Proposed 2026-11-10,
on branch `perf/train-18-20261110`, under the owner's delegation of 2026-10-02
and order of 2026-10-25 ("deciding by best judgement without asking").
Milestone: E14. Extends ADR-0055's lists.

## Context

ADR-0055's record says what it does not claim: `String.split`, `List.range`,
an `Int` sum. The rows app needed a range and wrote one, twelve lines that
halve the count so the list's length is no depth of calls:

```
fn ints(from: Int, n: Int) -> List<Int> !{} {
    if n <= 0 { [] } else if n == 1 { [from] }
    else { List.concat(ints(from, n / 2), ints(from + n / 2, n - n / 2)) }
}
```

A reader has to check it, and it allocates a list for every level of its
halving. Each language an author of Pleris comes from has one: Rust's
`a..b`, Python's `range(a, b)`, Elm's `List.range`. JavaScript has none, which
is why `Array.from({ length: n }, (_, i) => i)` is a common line.

## Decision

**`List.range(start, end) -> List<Int>`**: the whole numbers from `start` up to
and not including `end`, and none where `end` is not past `start`. That is how
`List.slice` counts places, and Rust's and Python's ranges, so
`List.range(0, List.length(xs))` is the places of `xs`. Elm's includes its end;
here it would be the one operation of the standard library that did.

**A range of more than 2^28 - 1 numbers traps**, for the cause the component
names `a value too long for memory` (ADR-0267), in both backends: no list whose
bytes an `i32` counts holds more. Every other range is computed, at either end
of the 64-bit integers.

It is an intrinsic, `list.range`, of one loop in each backend: the JavaScript
module's `range` helper over `BigInt`s, and the component's `Helper::Range`,
which counts, allocates the list's bytes once and writes each number.

## Why not the other ways

- **Written in Pleris.** It needs the halving above, or a depth of calls as
  long as the list, and a list for each level. One loop is what both backends
  do best.
- **A step.** No app needs one; `List.map(List.range(0, n), i => a + i * step)`
  is one.
- **Syntax, `a..b`.** A change to the language for one function, and one more
  thing a reader of Pleris has to learn where a name says it.

## Acceptance

- `compiler/pw-conformance/tests/stdlib.rs`:
  `ranges_agree_with_rust_and_a_range_too_long_traps_for_its_cause`, the
  component against Rust's `a..b` on nine cases at the bounds and 400
  generated, and for each range too long, the cause it stops for.
- `compiler/pw-conformance/tests/javascript.rs`: `Range` among the queries the
  component and the module agree on under Node, traps by their causes.
- **Mutation controls:** `scripts/slice_mutations.py` (`just e10-slices`), six
  mutants of `List.range`, each killed: a bound that reads a wrapped difference
  as signed, a trap with no cause, an empty range's count as its difference,
  numbers that start at zero in each backend, and the module's trap for another
  cause.

The first version of the component's bound compared the difference of the two
ends as signed. `List.range(MIN, MAX)` wraps a 64-bit subtraction to -1, which
passed it, and the list's count was -1: it stopped as `memory exhausted`, not
for its cause. Writing the test found it; the first control holds it.

## Consequences

- `examples/rows` makes its rows' ids with `List.range(from, from + n)`.
- `e10-slices`' record no longer says `List.range` is not claimed.
