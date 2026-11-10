# ADR-0421: a list a signal holds is a block of its own

Status: accepted 2026-11-10, landed with trains 18 and 19. Proposed 2026-11-10,
on branch `perf/train-18-20261110`, under the owner's delegation of 2026-10-02
and order of 2026-10-25 ("deciding by best judgement without asking"), and the
order of 2026-11-09 that every row of the head-to-head Svelte wins is a defect
until it is fixed or recorded. Milestone: E14. Amends ADR-0137; builds on
ADR-0417.

## Context

ADR-0137 refused an `{#each}` over a list a signal holds outside any block a
signal decides: the browser rendered such a list once, and a page that showed
one would have shown its first rows forever. A list had to sit in a block a
signal decides, and both apps with a long list wrote one for no other reason:

```
{#if List.length(rows) > 0}
    {#each shown(rows, selected) as row (row.id)} .. {/each}
{/if}
```

The condition says nothing a reader needs. Svelte asks for none, and ADR-0417
listed it as a limit of its own. It also costs what the list does
not change: each time the list goes from no rows to some, or back, the block
around it is rendered whole, a second pair of anchors stands in the document
for it, and its arm is morphed before its rows are put in.

Since ADR-0417, a list inside a block is already rendered again by its
rows (`listMade`): by their keys, parsing only those that are new or changed
and moving only those out of place. What the browser lacked was a way to
render a list again that is not inside a block.

## Decision

1. **An `{#each}` at the top of a page over a list a signal holds, or one the
   browser computes from the signals, is a block of its own**: one of the
   plan's live entries, kind `each`, rendered again when a signal it reads
   changes, its list's or one its rows read. Its rows are made by their keys,
   as a list's inside a block are (`rowsMade`, `listMade`).
2. **Its rows read what a block's parts may read**: the signals, and the name
   the loop binds (ADR-0137's rule for a block a signal decides, applied to
   the loop's body). Anything else a row reads is refused, as there.
3. **What its rows read is read inside its loop** (`block_reads`): no part of a
   row is set in place by a signal, since the row is what is rendered again.
4. **Its first render on the device puts its rows in by key**: the server's
   document shows the list with no rows, and `morphBlock` makes them from an
   empty list, as any later render does. A document that shows some rows is
   morphed whole, once.
5. **A list inside a block a signal decides is rendered with its block**, as
   before; so is one a query decides (ADR-0137).
6. **The apps write the list alone**: `examples/rows` and `examples/todomvc`
   lose their `{#if}`.

## Why not the other ways

- **Keep the refusal and the condition.** Every list of a signal's would need
  a block that decides nothing, and the cost of rendering it.
- **Wrap the list in a block the compiler writes.** The same rendering with a
  second pair of anchors in the document and a block in the plan the source
  does not have; a block is what the loop already is.
- **Set each row's parts in place by their signals**, as Svelte compiles: the
  larger change ADR-0417 deferred. A row here is rendered again whole,
  by its plan's holes (ADR-0420).

## Acceptance

- **`compiler/pw-core/tests/signals_render_again.rs`, 6 tests**, 2 of them new:
  a list a signal holds is rendered again by its rows at the top and inside its
  block; a list at the top is its own block, read again for what its rows read,
  whether a signal holds it or the browser computes it.
- **Mutation controls:** `scripts/signals_render_again_mutations.py`, 12
  mutants, 5 of them new, each killed: the list refused again, its rows read as
  the top of the page, no block of its own for a signal's list or a computed
  one, and what its rows read not read again.
- `e2e/rows.spec.mjs` (a seeded walk of sixty operations), `e2e/todomvc.spec.mjs`
  and `e2e/device-runtime.spec.mjs`, in three engines on a production build.

## Result

The head-to-head's figures go here when it has run at this branch's commit.

## Consequences

- ADR-0417's last limit is gone: a list a signal holds is written as it
  is shown.
- A list at the top of a page is one more block of the plan's: a feature that
  walks the plan's blocks meets lists among them.
