# ADR-0082: a `derived` value performs no effect

Status: accepted under the owner's instruction of 2026-09-26 ("keep working
until its perfect"). Date: 2026-09-26. Milestone: E10 (charter §7.5, "no
universal lifecycle effect").

## Context

The charter lists `derived` among its primitives as "a pure value computed
from other values" (§7.5), and `docs/SEMANTICS.md` repeats it. A derived
value is recomputed whenever what it reads changes, so an effect inside one
would run an unknown number of times. ADR-0047 made `derived e` parse as a
value and recorded, under "Not done": "`derived`'s purity is not checked".

On 2026-09-26, at a5d4785, `let t = derived clock.now()` checked, in a
declaration whose row allows the clock. The row check asks only whether the
declaration may perform the effect, not whether the `derived` value may.

## Decision

**A `derived` value performs no effect** (PW0334, "a `derived` value is
computed from other values, and performs no effect"). Everything the value
performs counts: its calls, the members it reads, and the functions it
names (ADR-0078). An effect beside it in the same body is the body's. The
diagnostic names the effect and the route it took.

## Acceptance

- **`compiler/pw-core/tests/derived_purity.rs`**, 2 tests, each with
  controls:
  - a clock read;
  - a function named as a value, and one written in place;
  - a geometry read through a member.
  Both fail at a5d4785, the commit before.
- **No rejected, rule or generality fixture's diagnostics change.** The
  accepted corpus's `derived` values (A-016, A-018) are pure and check
  clean, as do the store and kiokun.
- **Mutation controls:** `scripts/derived_mutations.py`, `just e10-derived`,
  3 mutants.

## Amended, 2026-10-25: a control that had become equivalent

The nightly of 2026-10-25, the first to run every recipe since train-12,
failed `e10-derived` at `cc47a64b`: "only what a derived value calls
counts" survived, and `master` at `4345726a` reproduces it on a clean tree.
The mutant replaced the check's `infer_in_at`, which counts what a body
calls, the members it reads and the declarations it names, with
`infer_at`, which counted its calls alone. ADR-0351 (train-12) made
`infer_at` read the members and values of every body `run` typed, so the
replacement found the same effects and the tests could not tell. Nothing
in the check had weakened; the control had stopped expressing its fault.

It now removes the member and value effects from `infer_in_reading`, the
inference `infer_in_at` performs, so the check counts calls alone, as
before 2026-09-26. The derived-purity tests kill it (1 of 2 fail), and the
other two controls are unchanged: 3 of 3 killed.

No push planned the recipe in the five days between: a push plans a
mutation script's recipe when its anchored lines change, not when the code
those lines call does. Sunday's run of every recipe is what found it.
