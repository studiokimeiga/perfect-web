# ADR-XXXX: a press changes what it changes

Status: proposed 2026-11-09, on branch `perf/rows-20261109`, under the owner's
delegation of 2026-10-02 and order of 2026-10-25 ("deciding by best judgement
without asking"), and the order of 2026-11-09 to work the gaps with Svelte
largest first. Milestone: E14. Builds on the browser renderer's ADR and the device
runtime's.

## Context

The head-to-head measured two apps, neither with a long list. On the first
measure of a table of a thousand rows, on a production build in Chromium, from
the press to the next frame in ms (Svelte's in brackets): select a row 12.1
(0.7), swap two rows 74.2 (5.0), remove a row 50.6 (8.4), update every tenth
21.6 (10.4), create a thousand 59.6 (33.1), clear ten thousand 295 (30). The
heap held 97 MB where Svelte's held 19, both before a collection.

A profile named why. After every render the page walked the whole document to
index it and scanned that index for each event part to bind what was new
(`buildIndex`, `addressesFor`, `bindEvents`). Every row's text was written again
and compared with the last. A row moved or removed fell to a morph of every
row's elements (`morphNode`, 17 of 60 ms). `rowsOf` found each row's end from the
start of the list, so a thousand rows cost a million steps. A press's log line
held every instance's address, joined. Each row's source indentation was parsed,
twice the row's own markup. A key's token was kept for as long as the page was
open, and `setSignal` stringified both values to compare them.

Every one of those costs what the page holds, not what the press changed.

## Decision

On a page that talks to no server, and for what is said below on every page:

1. **A row's text is kept**, with the value it was written from and what it read
   of the page. A render writes the text of a row whose value or reads changed,
   and reuses the rest. A kept row's text carries no anchors around its text
   parts, which only an address would read. A block's last render is kept by the
   anchor that opens it: a block taken away and shown again by the block around
   it has a new anchor, and so nothing kept, whatever its signals did while it
   was gone.
2. **A list's rows are made by their keys.** `listMade` works from the rows the
   last render wrote and the rows this one does, and the document is changed by
   the difference: rows gone are removed, rows changed are morphed where they
   are, new rows are parsed together and put in at once, and only the rows out of
   place move, the fewest that can, by a longest increasing run of their places
   (`staying`). Anything else about the block having changed, it is morphed
   whole, as before. What a render shows is what morphing the whole block shows:
   a morph sets a box's live state from what was rendered, so each box in the
   block whose state differs from its attribute is set from it, found with
   `:default`, and a box outside the block is not one of them.
3. **A press is heard at the document** (`pressHeard`), on a page with no view
   inside itself: an element's `data-pw` says which of the page's handlers it
   has, so a row needs no listener and nothing need be bound after a render. A
   handler that stops its event is the last to hear it, and an event that does
   not bubble is heard on its way down, for its own target.
4. **Nothing indexes the inside of a row** on such a page, since nothing asks for
   it: the walk steps over a row, and a render that made only rows walks nothing.
5. **A run of what is gone goes at once.** Where only comments and text stand
   beside it the parent's children are replaced in one step; otherwise each node
   is removed. New nodes side by side go in as one fragment.
6. **For every page**: `rowsOf` is linear; a render's log is bounded; a token is
   its key, so nothing is kept for a key that has gone; and `setSignal` compares
   values, stopping at the first difference.
7. **A template's layout is not the page.** A run of white space with a line
   break in it is one space, and nothing at all where text can never be shown:
   between a table's structural elements, in a `<select>`, or in the head. Inside
   `<pre>`, `<textarea>`, `<script>` and `<style>` every character is content, and
   so is a value: text meant to keep its line breaks under a style such as
   `white-space: pre-line` is written as one, `{"first\\nsecond"}`, or in a `<pre>`.

An independent review of this branch's first version found what is kept could
outlive what it described: a block inside another, its signal changed while the
outer block had taken it away, then rendered the old text, matched what was kept
and was never updated. Keeping it by the opening anchor is the repair, and
`e2e/nested-blocks.spec.mjs` holds it.

The new app, `examples/rows`, is in the head-to-head beside the same app in
Svelte and in React, and its operations are rows of the record. `e2e/rows.spec.mjs`
presses a seeded walk of sixty operations in three engines on a production build
and compares the document with the page's own rows after each.

## Why not the other ways

- **A range deletion for what goes.** Tried first: WebKit took three seconds over
  ten thousand rows.
- **Delegation everywhere.** A page with a server can receive a row from a
  frame and a view that contains itself has a template for each instance, so an
  element's `data-pw` alone does not say which part it is. Those keep their
  listeners until something measures them.
- **Fine-grained updates compiled for each dependency**, as Svelte does: the
  larger change, and it may yet be worth it for updating many rows at once, where
  Pleris is still behind. This took most of the cost for a few hundred lines,
  without a second way for a page to change.
- **Removing every space between tags, as JSX does.** A space between two inline
  elements is shown; JSX's rule loses it unless it is written `{" "}`.

## Result

Press to the next frame, Chromium, a production build, the median of seven runs
after warmups, in ms (Svelte's in brackets): select a row 2.5 (0.7), swap two
rows 8.2 (5.0), remove a row 9.7 (8.4), update every tenth 15.1 (10.4), create a
thousand 38.6 (33.1), clear ten thousand 46.8 (29.9). In WebKit, clearing ten
thousand rows ties (97 and 84). The heap with a thousand rows, after a
collection, is 3.2 MB (Svelte's 3.6). The head-to-head's figures go here when it
has run at this branch's commit.

## Consequences

- `examples/rows` is the third app of the head-to-head, and each of its rows
  Svelte wins is a defect until it is fixed or recorded as a trade-off.
- A feature that writes inside a row on such a page has to clear what is kept for
  it, or the comparison answers for a page it no longer describes.
- A page's whitespace changes for every app, so the committed builds in
  `docs/evidence/E10` are regenerated with this. It is a change a page can see:
  static text laid out over several lines under `white-space: pre-line` or
  `pre-wrap` now shows as one line, and has to be written as a value. No app in
  this repository does that; the guide says so.
- `{#each}` of a signal's list has to sit in a block a signal decides (ADR-0137),
  which the large list writes as `{#if List.length(rows) > 0}`. That is a limit
  of its own, and it is on the list.
