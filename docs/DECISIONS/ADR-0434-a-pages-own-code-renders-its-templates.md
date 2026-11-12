# ADR-0434: a page's own code renders its templates

Status: proposed, 2026-11-12, under the owner's delegation of 2026-10-02 and
order of 2026-10-25 ("deciding by best judgement without asking"), and the
owner's of 2026-11-09, which asked for the browser's code generated per page
from the template IR and that every row Svelte wins be a defect until it is
fixed or recorded. To be built in trains of its own, each with its result;
nothing of it is built yet. Builds on ADR-0413 (a page that talks to no
server), ADR-0426 (a page alone in its build), ADR-0427 (a row a plan
makes), ADR-0428 (what a device's page holds is what its build reaches),
ADR-0432 and ADR-0433. Date: 2026-11-12. Milestone: E14. Its first step,
planned rows, is ADR-0437 (proposed 2026-11-12, train-32), which amends
decision 5 for that step: its bytes said, landed where its presses are
quicker past the rounds' spread.

## Context

The device's runtime renders a page's parts with one general renderer
(`const renderer = …` in `pw-runtime.mjs`): an interpreter of the template
IR the document carries (`pw-parts`), its `emit` walking a template's
chunks, its `read` each path by name, its planned rows (ADR-0427) each hole
by its path. Measured on TodoMVC's production build at train-28's
`8096c4e7`:

- **The renderer is a third of the runtime a page is sent**: 12,465 bytes
  minified and 4,557 with Brotli, cut from the source and packed alone with
  the flags of a page that talks to no server, of a device runtime of 13,663
  with Brotli; the document's parts JSON 7,784 bytes decoded and 1,404 with
  Brotli, of it the blocks' templates 2,121 and the parts' 2,421.
- **The bytes are rows Svelte wins**: TodoMVC's first load as a static host
  compresses it 19,056 bytes to Svelte's 16,992; the large list's 16,007 to
  12,567 (the head-to-head at `8096c4e7`).
- **Each press pays for the interpretation**: a list's rows are kept and
  made by a pass over every row through the general code (`rowsKept`,
  `holdsStill`, `plannedRowOf`, `read`), each row an environment, an object of
  its holes and an object of what it read. A swap of two rows still passes
  over all thousand (train-29, not landed, NEXT); in code not yet optimized,
  which is what a press on a page just loaded runs (ADR-0432), the general
  code's calls and look-ups by name are what a press spends.

Svelte compiles each component to code that builds and updates its own DOM,
and ships the small runtime that code calls.

## Decision (proposed)

1. **The packaging writes each page's own renderer**: for each template a
   page's build holds, the functions the general renderer would run over it,
   written out with its paths, its contexts' escaping and its holes known:
   the general renderer partially evaluated over the page's IR, at build
   time (`optimize-assets.mjs`, beside ADR-0426's page alone and ADR-0428's
   shared helpers). It offers the twelve calls the runtime makes of the
   general one (`block`, `part`, `instance`, `changes`, `attribute`,
   `capturesOf`, `captures`, `textOf`, `same`, `env`, `rowsComputed`,
   `letGo`), so nothing else in the runtime changes.
2. **The general renderer stays for what is not generated**: a build any of
   whose templates holds a part the generator does not write is packed with
   the general renderer, as now; one all of whose are generated holds none
   of it (a flag the packaging folds, as ADR-0413's are).
3. **One semantics, held three ways**: the server's renderer (`pw-render`),
   the general one and the generated one agree byte for byte on the
   templates `renderer-agreement.test.mjs` generates of every part and
   context, and on the compiler's own; the browser suites run on generated
   builds.
4. **Built in steps, each measured by the builds visited in turn**:
   - **planned rows first**: a list's holes computed by the row's own code,
     its paths read as fields, its escaping known (`plannedRowOf`'s work);
   - **then a block's markup**, which TodoMVC's rows are (each holds an
     `{#if}`, and is morphed);
   - **then the general renderer folded away** where every template is
     generated, and the templates' bodies left out of the parts JSON that no
     longer reads them.
5. **What a step costs in bytes is said**, a build's generated code against
   the general renderer it replaces; a build where the generated code would
   be larger keeps the general renderer (decision 2).

## Why not the other ways

- **The compiler writes it, in Rust**: the server's renderer is Rust, and
  the compiler's JavaScript backend (`js_pure.rs`) writes a page's handlers
  and computed values; but the packaging already reads each page's IR to
  specialize its runtime (ADR-0413, ADR-0426, ADR-0428), and a generator
  written beside the renderer it specializes can be held to it construct by
  construct. Either is the same decision; where it lives is the first
  step's to settle, by which can be tested against the general renderer
  more closely.
- **A smaller general renderer**: ADR-0428 took from it what a build does
  not reach; what is left is what every page with a list reaches.
- **DOM built by code rather than markup parsed**: the planned rows build
  DOM by cloning (ADR-0427); a block's markup stays markup, which the
  server's agreement holds byte for byte.

## Consequences

- The byte rows Svelte wins are this decision's to turn: the renderer and the
  templates' bodies are some 5 KB of Brotli where TodoMVC is 2 KB behind
  Svelte, less what the generated code weighs, which the first step measures.
- A press's cost becomes the row's own code's: what `rowsKept` and
  `plannedRowOf` spend in general code is what the first step measures.
- A third renderer to hold to the other two: the agreement test grows a
  column, and a template the generator does not write is a refusal the
  packaging says, not a page that breaks.
