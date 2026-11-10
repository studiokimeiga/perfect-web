# ADR-0420: a planned row is written when it is read

Status: accepted 2026-11-10, landed with trains 18 and 19. Proposed 2026-11-10,
on branch `perf/train-18-20261110`, under the owner's delegation of 2026-10-02
and order of 2026-10-25 ("deciding by best judgement without asking"), and the
order of 2026-11-09 that every row of the head-to-head Svelte wins is a defect
until it is fixed or recorded. Milestone: E14. Builds on ADR-0417.

## Context

ADR-0417 made a list's new rows from a plan (`planOf`): the row's
template, cloned, with each hole set, where before every new row was parsed.
Updates became faster than Svelte's or close to them. Creation did not: the
head-to-head at `6a112c3d` measured create a thousand rows, append a thousand
and replace a thousand behind Svelte's by 10 to 60 per cent, and behind the
branch's own figure before plans.

A profile of the thousand rows' creation named what the plan had not removed.
Each planned row was still rendered as any row is: through the writer that
waits for a view to be written later (`rendered`), with a scope built for its
name and its frame by two calls, its whole text joined into one string, and,
beside it, its holes as escaped pieces and what its handlers capture as raw
values. The text was then compared with the last render's to find whether the
row changed, and the holes were read back from their escaped pieces to set
them: a text's by unescaping it, an attribute's by parsing it.

A planned row needs none of that. Nothing in it is written later: a row with
a view in it has no plan. Its holes say all that differs from its template.

## Decision

1. **A planned row is emitted straight into its holes** (`plannedRowOf`): one
   scope for its name and its frame, made once, and no writer waiting for
   anything.
2. **Its text is joined only when something reads it**: a morph of the row, or
   a parse where a plan is not taken. A row the plan makes is never joined.
3. **Whether a row changed is read from its holes** where both renders made
   them by the plan (`rowDiffers`): equal holes in the same template are the
   same markup. Rows made otherwise compare their text, as before.
4. **Each hole holds its value as it was before it was escaped** (`raws`), as a
   handler's captures already did: a text's characters, an attribute's name
   and value, or none for an attribute or a flag not written. A hole is set
   from it with no unescaping: a text as the parser would read the escaped
   text, a carriage return a line feed and a NUL nothing; an attribute by the
   escapes the renderer writes, refused where it writes one it does not
   (`unescaped`). A row whose hole is refused is morphed, as before.
5. **On a server's page nothing changes**: a server's runtime holds none of it
   (`PW_SERVER`), and its rows are written as text.
6. **A row the plan did not make is set by its holes where the plan finds
   them** (`holesFound`; added 2026-11-10, from this train's full verify). A
   row the document holds, or one a morph wrote, is its template with its
   values, so each hole is where the plan's is. Where each node on a hole's
   path is of the kind and tag of the plan's, among as many siblings, the
   row's holes are those nodes, and it is set by them from its first change,
   as one the plan made. Where one is not (text beside a hole's text is one
   text node in a document, an empty hole's text none), it is morphed, as
   before. The run found the press fixture's list, a block of its own since
   ADR-0421, keeping the rows its document held and morphing them at every
   change: no control of the holes' text and attributes was reached, and
   eleven survived (run 42825062947). The block's first render on the device
   still morphs a document's rows whole, once (ADR-0421, decision 4).

## Why not the other ways

- **Keep the text and drop only the compare.** The join was the larger part of
  the cost for a thousand rows, and a row the plan makes never needs it.
- **Compile each row's creation to code**, as Svelte does: the larger change ADR-0417
  deferred. A plan is data, the same for every page; generated code
  for each list is a second way to make a row, which the head-to-head may yet
  ask for.

## Acceptance

- `e2e/rows.spec.mjs`: a seeded walk of sixty operations in three engines on a
  production build, the document compared with the page's own rows after each,
  and the text a hole sets compared with the parser's reading of it.
- `e2e/device-runtime.spec.mjs` and `e2e/todomvc.spec.mjs`, in three engines.
- **Mutation controls:** `scripts/device_runtime_mutations.py` and
  `scripts/browser_renderer_mutations.py`, re-anchored to the planned row.

## Result

The head-to-head's figures go here when it has run at this branch's commit.

## Consequences

- A row's markup is not kept for a planned row: a feature that reads a row's
  text reads it through `text`, which joins it then.
- A hole holds two values, its escaped piece and its raw value, for as long as
  its row is shown.
