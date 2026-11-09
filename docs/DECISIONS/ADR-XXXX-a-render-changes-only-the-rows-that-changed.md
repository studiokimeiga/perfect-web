# ADR-XXXX: a render on a page that talks to no server changes only the rows that changed

Status: proposed 2026-11-09, on branch `perf/after-train-20261109`, under the
owner's delegation of 2026-10-02 and order of 2026-10-25 ("deciding by best
judgement without asking"), and the order of 2026-11-09 to work the measured
gaps with Svelte largest first. Milestone: E14. Builds on the browser renderer's
ADR, which renders a block's text, and on the device runtime's.

## Context

The head-to-head at `3b0e7c4f` has Svelte ahead on TodoMVC's toggle at Slow 4G:
30 ms against 22 on a cold load and 19 against 14 warm. A toggle changes one row.
The browser renderer renders the whole block's text, which is cheap, and the
runtime then parses all of it and walks every row against the page, which is
not: a press on a list of a hundred rows parsed a hundred rows to change one.

Svelte's compiled code writes the one row's class. Pleris's page already knows
more than a morph uses: the row keys are in the text, and on a page that talks
to no server, nothing writes a block but its own renders and the blocks around
it.

## Decision

1. **What each block last rendered is kept** on such a page, by the anchor that
   opens the block. Before anything is parsed the new text is compared with it,
   row by row, by the anchors each row already has. `rowsChanged` says which
   rows differ, or `null` where anything else does: a row added, gone or moved,
   or a change between the rows. A block taken away and shown again by the block
   around it has a new anchor, and so nothing kept, whatever its signals did
   while it was gone: its next render morphs the whole of it.
2. **Only those rows are parsed and morphed** (`morphRows`), where the document
   holds each of them. Otherwise the whole block is morphed, as before, and so
   is the first render of a block, whose text no render has kept.
3. **What a render shows is what morphing the whole block shows.** A morph sets
   a box's live state from what was rendered, so a box a press ticked and its
   model did not stays honest. A row left alone is not morphed, so each box in
   the block whose state differs from its attribute is set from it. The browser
   finds those with `:default` in a single query, and a box outside the block is
   not one of them.

A page with a server is as it was: its frames write blocks the browser did not
render, so what it last rendered says nothing about what it shows.

## Why not the other ways

- **A fine-grained update for each dependency**, Svelte's way, in the compiler.
  It is the larger change and it may yet be worth it. This takes most of the
  cost for a few dozen lines, without a second way for a page to change.
- **A virtual DOM of the block**, kept and diffed. It holds a node per node of
  the document to answer what comparing two strings answers.
- **Asking which rows a handler wrote.** A handler can write through a derived
  value, and the render knows the answer by construction.

## Result

Locally on TodoMVC with 4x CPU throttling, the work a toggle does falls from
5.75 ms to 3.79 ms. `e2e/device-runtime.spec.mjs` holds that a press parses the
one row it changed, and the mutation controls in
`scripts/device_runtime_mutations.py` remove each piece. An independent review
of this branch found what is kept could outlive what it described: a block
inside another, its signal changed while the outer block had taken it away,
then rendered the old text, matched what was kept and was never updated.
`e2e/nested-blocks.spec.mjs` holds that. The head-to-head's figures go here when
it has run at this branch's commit.

## Consequences

- A device page keeps one string per block, the size of what the block shows.
- `rowsOfText` reads anchors out of text the renderer wrote. Those anchors are
  held to the server's renderer by `renderer-agreement.test.mjs`.
- While its anchor stands, nothing but a block's own renders changes what it
  shows; a block around it renders it the same. A feature that writes a block
  any other way has to clear what is kept for it, or this comparison answers
  for a page it no longer describes.
