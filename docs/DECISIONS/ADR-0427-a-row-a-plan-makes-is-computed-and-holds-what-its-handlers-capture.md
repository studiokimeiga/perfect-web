# ADR-0427: a row a plan makes is computed, and holds what its handlers capture

Status: accepted 2026-11-11, landed with trains 20 and 21. Proposed 2026-11-10,
on branch `perf/train-21-20261110`, under the owner's delegation of 2026-10-02
and order of 2026-10-25 ("deciding by best judgement without asking"), and the
order of 2026-11-09 that every row of the head-to-head Svelte wins is a defect
until it is fixed or recorded. Milestone: E14. Builds on ADR-0417, ADR-0420 and
ADR-0424; keeps ADR-0210's ruling 0033-a for the markup a device did not make,
and its rule against stale captures (urgent 2) for the rows it did.

## Context

Making a thousand rows was the large list's widest gap to Svelte: about one
and a half times Svelte's time to its frame. Measured at train-20's commit
(`66dac6ed`) in Chromium, each a fresh page, the press timed from the click,
medians of 24 (scratchpad `profile_sum.mjs`): the rows were in the document
28.5 ms after the click, Svelte's 11.9; the frame after came 64.3 ms after it,
Svelte's 46.8. The time from the rows being in the document to the frame was
the same for both (35 ms): the whole gap was the script before it, 24.1 ms of
script to Svelte's 10.4, and 6.1 ms of garbage collection to 2.2.

Each part was taken out of a build in turn and the press measured again
(`variants.py`): what each row's handlers capture, written as JSON into an
attribute of each of the row's two buttons, cost 5 ms of script; finding
each hole by `childNodes[i]` from the row's root, 3; the two comment anchors
around each row, 2 ms of script and no measurable time to the frame. The rest
was the row's markup, written whole for each row and kept as an array of
pieces, though a plan's row is made from its holes and its text is read only
by a morph or a parse.

## Decision

1. **A row a plan makes holds what its handlers capture, and a press
   computes it** (`rowOfHandler`, `heldCaptures`). The element of each of a
   planned row's handlers names the row's nodes, `[its anchors, the node of
   each hole, its row]`, and the row is the one the plan last made or set.
   A press on it computes what the handler captures from that row, the text
   its attribute would have held, as the parser reads it back; the handler is
   given what it was given before. No JSON is written as the row is made.
2. **Markup written as text keeps its attribute, and a press reads it
   first.** The server's document, a morph's rows and a parse's carry
   `data-pw-captures`, as ruling 0033-a has it: a page resumed from its
   markup is a page whose rows the device did not make. Where the plan takes
   a row over from the document or a morph (ADR-0420, decision 6), the
   attribute is taken away as its handlers' elements are pointed at the row
   (`capturesHeld`), so no element holds an attribute older than its row.
3. **What a row's handlers capture follows the row** (ADR-0210, urgent 2):
   a row whose value changed is given to the plan whenever its handlers
   capture anything, though none of its holes changed, and the plan points
   it at the new row (`rowDiffers`, `rowsChanged`). A row the plan does not
   take is morphed, which writes its attribute.
4. **A capture that cannot be carried fails its press**, where it failed the
   render: an `Int` past 2^53 a row's handler captures, or a case, is found
   as the press computes the captures, and the press fails visibly
   (ADR-0302), as a handler's failed load does. The row is shown. The host
   still refuses to render such a row, as before: what it writes is markup.
5. **Each hole of a planned row is computed alone, and its text only when it
   is read** (`plannedRowOf`, `holesOf`): what each hole sets is computed from
   the row's scope in the plan's order, a handler's run of one element as one
   hole, and the row's markup is written when a morph or a parse asks for it,
   from the same scope, noting nothing it computes again.
6. **A hole is found by its siblings** (`rowsPlanned`): `firstChild` and
   `nextSibling` along its path, where `childNodes` makes a list at each
   step.
7. **A path is cut once** (`cutsOf`): what `read` and `keyOf` split, slice
   and join at each read is kept for each path the page reads, which are its
   template's.
8. **The browser's module computes as it did, with less made at each
   call** (`js_pure.rs`): an `Int`'s overflow is found as `BigInt.asIntN(64,
   v) !== v`, where `2n ** 63n` was computed twice a call; and a list whose
   elements are all literals is a frozen value of the module, named by what
   it holds, where a function made it at each call (the large list made three
   lists of words for each row). The literals only such a list read are not
   written.

## Why not the other ways

- **No anchors around a row of one element**: measured, the anchors cost 2
  ms of script and nothing to the frame; a row found by its key in the
  document, by a morph, needs them.
- **The row's nodes on the element, where a `WeakMap` holds them**: measured
  the same, to a tenth of a millisecond.
- **The attribute written lazily**: a press reads the element, and nothing
  reads the attribute before it; computing the captures as it is pressed is
  that, without an attribute.

## Result

`profile_sum.mjs`, Chromium, fresh pages, the three builds interleaved run
by run, two rounds of forty, medians:

| | in the document | its frame | script | GC |
|---|---|---|---|---|
| train-20 (`66dac6ed`) | 27.3, 25.6 ms | 62.2, 62.6 ms | 23.0, 22.3 ms | 3.4, 4.4 ms |
| this train | 16.0, 14.5 ms | 58.5, 44.1 ms | 15.7, 14.2 ms | 2.7, 2.1 ms |
| Svelte | 11.1, 10.6 ms | 48.5, 45.1 ms | 10.1, 9.3 ms | 1.9, 1.9 ms |

The rows are in the document in about 60% of the time they took. The frame
after them is where the two rounds part: the browser lays a thousand rows out
in about 30 ms after they are in, so this train's frame falls on either side
of a vsync, one frame (16.7 ms) apart, and its median is the earlier frame in
one round and the later in the other; Svelte's falls on the earlier in both.
The head-to-head's runs say which it is, more often (the record).

What it sends: the large list's one module is 336 bytes more as Brotli
sends it (16,126 from 15,790), 288 of them the runtime's and 40 the page's
own code (each of its three handlers holds its three lists of words); a
trade the record's bytes row holds against the time. TodoMVC's grows by 30
bytes (16,627 from 16,597): its rows are no plan's, as each holds an
`{#if}`, so its build holds none of this.

Still behind Svelte by about 4 ms to the document. Of what is left
(`profile_tree.mjs`, at `aea083eb`): the press's handler 2.1 ms, in
`BigInt` where Svelte's is in numbers; computing the rows 4.7 ms, most of it
each row's scope and its computed class; making them 6.1 ms, 1.8 of it the
clone; and putting them in 2.4. These are the next train's (NEXT).

## Consequences

- A planned row's handlers' elements carry no `data-pw-captures`, which tests
  and a person reading the document no longer see on a row the device made.
- A capture the platform cannot carry is refused at the press on the device,
  where the host still refuses it at the render.
- Mutation controls, each killed: eighteen new in
  `device_runtime_mutations.py`'s rows suite and three of it re-anchored;
  seven new in `javascript_mutations.py` and two in `handler_mutations.py`;
  three re-anchored elsewhere (`browser_renderer_mutations.py`,
  `handler_mutations.py`, `javascript_mutations.py`). The merge of a
  function's lists of literals into its module's has no control: the inliner
  copies each callee's first call into its caller, list and all, so no
  program yet has a list that a callee alone holds.
- The audit's ceilings: `emitPart` is back to its score before trains 18 and
  19, and `holeSet` to 380 from 702; `plannedRowOf`, `holesOf`,
  `rowsPlanned`, `rowDiffers`, `acted` and the renderer are raised or added
  at the scores they landed with.
