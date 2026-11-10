# ADR-0424: a row computes what it shows

Status: accepted 2026-11-10, landed with trains 18 and 19. Proposed 2026-11-10,
on branch `perf/train-19-20261110`, under the owner's delegation of 2026-10-02
and order of 2026-10-25 ("deciding by best judgement without asking"), and the
order of 2026-11-09 that every row of the head-to-head Svelte wins is a defect
until it is fixed or recorded. Milestone: E14. Extends ruling 0073-a, ADR-0228
and ADR-0420.

## Context

The head-to-head counts the source each stack's developer writes. The large
list was 116 lines to Svelte's 81 and React's 99. A third of the difference
was one line Svelte writes and Pleris could not:

```
<tr class={selected === row.id ? "danger" : ""}>
```

Ruling 0073-a made a value a template computes compile, and ADR-0228 made
one in a row of a query's list the row's, computed by a host from the row's
item alone. A row of a list a signal holds computed nothing: the plan refused
one there by name, and one from two values anywhere. So the app made a type,
`Shown`, and a function to fill it with each row's class, and TodoMVC a
`Row` and `rows()` for its class and whether a row was edited.

## Decision

1. **A value a row of a list the browser renders computes, from the row's
   item and the signals beside it, is the row's** (`browser_rows`,
   `Computes::BrowserRow`): the list a signal holds, or one the browser
   computes from the signals. Its path is the item's, `row.#P~5`, as a
   query's row's is (ADR-0228), named whenever it reads the item.
2. **The browser computes it for each row it renders**, in the row's scope,
   by the page's computed module's `rows`, keyed by that name; the manifest
   names what each reads, in its function's order (`rowsComputed`). A row's
   computed value is read when its path is, as the rest of the path read
   whole at each record.
3. **A host computes the first render's**: each row of the list's first
   value is given the member, computed by the hole's component from the
   item and the signals' first values, as a host gives a query's row what it
   reads through a member (ADR-0170). The signal the page holds is not
   changed.
4. **What it reads of the page is the row's computed reads**, apart from
   what the row's markup reads, and what it computes is kept with the row. A
   change to one keeps the row where each value, computed again, is what it
   showed (`computedHold`): selecting a row in a thousand writes the rows
   whose class it changes, not every row whose class reads the selection.
5. **The list is read again for what its rows compute from**: the signals
   among a row's value's inputs are among its list block's reads.
6. **Refused by name still**: a row of a list inside another row the
   browser renders, a row that is no record (what it computes is held in its
   item), and one that reads another value than its item and the signals.

## Why not the other ways

- **Keep the view types.** Every app with a list that shows something about
  each row from a signal writes a type and a function for it, which no other
  stack asks for.
- **Compute each row's values into the item when the list changes.** A
  change of the selection would then change every item, and every row would
  compare unequal and be written again.
- **Fine-grained updates compiled per binding**, as Svelte does: the larger
  change ADR-0417 deferred. A row's computed values, checked against
  what they were, do what Svelte's per-row effect does here, without a
  second way for a page to change.

## Acceptance

- `compiler/pw-core/tests/computed_rows.rs`: a row of a signal's list
  computes from its item and the selection (its path, its inputs, its list's
  reads, the module's function), and the refusals it keeps.
- `compiler/pw-core/tests/device_store.rs`: a row of a list the device keeps
  is each row's, where it was refused.
- `tests::browser_rows` (the host): the first render's classes, as a
  request renders the page and prerendered, the signal as it is, the
  manifest and the module.
- `e2e/rows.spec.mjs`, 54 in three engines, a press writing one row among a
  thousand; `e2e/todomvc.spec.mjs` and `e2e/device-runtime.spec.mjs`, 195.
- **Mutation controls:** `scripts/computed_rows_mutations.py` (eleven new:
  the plan's entry, its inputs, its list's reads, the module's function, the
  two refusals, and the host's first render in both paths, its inputs, its
  module and its manifest) and `scripts/device_runtime_mutations.py` (four
  new: the value not computed, the module's rows not given, a kept row's
  values not computed again, and what they read taken for the markup's).

## Result

The large list counts 77 lines (Svelte's 81, React's 99), from 116, with
`List.swap` (ADR-0425). TodoMVC counts 179, from 188, its `Row` and
`rows()` gone. The head-to-head's figures go here when it has run at this
branch's commit.

## Consequences

- A row's markup may compute from its item and the page's signals as a
  page's top may, and a feature that walks a row's reads meets two kinds:
  its markup's and its computed values'.
- A row of a list inside a row, and a row that is no record, are the next
  of this kind; each is refused by name until then.
