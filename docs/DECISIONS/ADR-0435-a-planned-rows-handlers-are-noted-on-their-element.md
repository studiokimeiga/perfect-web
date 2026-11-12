# ADR-0435: a planned row's handlers are noted on their element

Status: accepted 2026-11-12, to land with trains 27 and 28 (train-30, on
branch `perf/train-30-20261112`, `dd8d8ca0`, carried onto theirs). Under the
owner's delegation of 2026-10-02 and order of 2026-10-25, and the order of
2026-11-09 that every row Svelte wins be a defect until it is fixed or
recorded. Milestone: E14. Builds on ADR-0427 and ADR-0432.

## Context

A row a plan makes (ADR-0427) holds no captured text: what a handler on it
captures is computed from the row when it is pressed (`heldCaptures`), the
row found from the handler's element in a `WeakMap` (`rowOfHandler`), set
for each handler's element as the row is made or taken over. The large list's
rows have two handlers each, so a thousand rows were two thousand entries
set, each an entry an engine's collector visits apart from the objects it
keeps (a weak map's entries are ephemerons, kept for as long as their key
is).

## Decision

1. **The element holds the row it captures for**, under a symbol of the
   runtime's own (`ROW_OF`): no page's code names it, no enumeration of the
   element shows it, and it goes when the element does, as the weak map's
   entry did.
2. **`heldCaptures` reads it back** from the element pressed; nothing else
   changes.

## Why not the other ways

- **The weak map, as before**: the entries are what this takes away.
- **A property under a name** (`el.pwRow`): a name a page, a library or a
  later DOM could use as well.
- **A map from each element**: it would keep the element after its row
  went.

## Result

The device's runtime 48 bytes lighter with Brotli (the large list's 13,413
to 13,365, 7 minified). Against train-28, the large list's two builds
visited in turn by the head-to-head's own visit, eleven rounds a cell, this
less train-28 (quartiles): "create 1,000 rows" 3.4 ms quicker cold in
Chromium (−7.1..−1.3) and 2.0 warm (−3.3..1.1), "replace all rows" 2.7 warm
(−5.7..0.1), every press of a visit 4.7 quicker cold and 4.5 warm, with
quartiles across zero; under Slow 4G and in Firefox within the rounds'
spread. `rows.spec.mjs` in Chromium, Firefox and WebKit, 39 tests built to
report and 48 as served; the two mutants on the weak map ("a row a plan
made holds no captures for its press", "a handler's element the plan takes
over holds no captures for its press") re-anchored on the property, run in
the full verify.

## Consequences

- A press on a planned row reads its row from its element, as quickly as
  any property.
- Lands with trains 27 and 28, after their full verify: the pull request's
  verify runs again each recipe that reads the runtime.
