# ADR-0437: a planned list's rows are kept and made by its page's own code

Status: accepted 2026-11-12 (train-32, branch `perf/train-32-20261112`), on
train-31; the first step of ADR-0434, whose decision 5 it amends for this
step. Under the owner's delegation of 2026-10-02 and order of 2026-10-25, and
the order of 2026-11-09 that every row Svelte wins be a defect until it is
fixed or recorded. Milestone: E14. Builds on ADR-0413, ADR-0426, ADR-0427,
ADR-0428, ADR-0432 and ADR-0434.

## Context

A list whose rows a plan makes (ADR-0427) was kept and made by the general
renderer: each row looked up by its key through `keyOf`, held by `holdsStill`
(`same`, the names around it, what it read of the page, `computedHold`), made
by `rowOf` and `plannedRowOf` (a scope spread from the block's, each hole by
`needText` or `attributeOf` through `read`, `bound`, `cutsOf` and `escaped`),
and its new nodes reached by `rowsPlanned`'s walk from the clone's root along
each hole's path. A press on a page just loaded runs that code unoptimized,
where its calls and look-ups by name are what it spends (ADR-0432). Profiled
on train-31's large list, the first "create 1,000 rows" of a cold visit spent
12.40 ms of JavaScript to Svelte's 9.12, 48.71 to 41.36 with the processor
slowed four times (`t32/prof_first32.log`); `rowsPlanned`, `rowOf`,
`plannedRowOf`, `holeSet` and `read` were most of the difference.

No test held a planned row to the server's renderer: the agreement loaded the
device's renderer with views on, so no list of it was a plan's.

## Decision

1. **The packaging writes each planned list's own code** (`own-rows.mjs`,
   called where the device's runtime is packed, `optimize-assets.mjs`): for
   each list a block of a device's page keeps whose rows a plan makes, its
   key, its hold and its making, by its page's template and its part,
   spliced into the renderer where the runtime holds a place for them, with
   the helpers they call written once. It is the general code partially
   evaluated over the list's IR:
   - each path from the row read as the item's own fields in `read`'s order,
     the whole rest of a path first; a value the row computes (ADR-0424) the
     item's own where a host set it, else its function's, called with its
     inputs read as `need` reads them, else its path's fields, as the
     general code reads them where it holds no function (the two are given
     each together); a value of the page by `bound`'s rule, noted in the
     row's reads;
   - each context's escaping chosen where it is written (`escaped`'s switch
     gone), formatting `shown`'s, as the IR has no types;
   - the row the same plain object `plannedRowOf` makes (ADR-0432), its scope
     made only when its text or a press reads it (`scopeOf`); a row that
     reads nothing of the page shares one empty map, and is kept where its
     item is the one it was made from and what it computes still holds.
2. **Each hole of a new row is reached by steps written for the row's
   markup**: the packaging reads the markup as written, element inside
   element, and writes `firstChild` and `nextSibling` from the row's clone,
   each node reached once (the large list's five holes in eight steps, where
   its paths took eighteen). `planOf` compares the paths read with the
   browser's own parse once a plan, and the general walk reaches a row's
   holes where they differ: a reading that is not the parser's costs speed,
   never correctness (a table's row, which the parser puts in a `<tbody>`).
3. **What the packaging does not write, it says, and the general code keeps
   it**: a list whose row reads a name a block around it binds; one reading
   a value a row computes inside another, or inside a value of the page, or
   whose computed value reads what is neither its row nor a value of the
   page; a page holding a value under a path from the row; a row bound by a
   name holding a dot; an attribute in a context escaped for no value. `pw
   build` prints a line for each (`pw packaging: list N of PAGE is kept by
   the general code: WHY`).
4. **What a build holds is what it reaches.** The runtime holds the lines
   only a page's own code needs between `// own rows:` and `// :own rows`,
   and the packaging writes what the general code becomes where a list is
   its own (`OWN_EDITS`): a build that writes no list is minified from the
   runtime as it was before there was any, its own lines cut (folded away,
   they still moved esbuild's names, which it gives by what the whole of a
   source holds); one whose every list is its own holds no general keeping
   or making of a planned row, nor its test of whether a list is planned.
5. **ADR-0434's decision 5, for this step**: what it costs in bytes is said,
   and it lands only where its presses are quicker past the rounds' spread,
   the builds visited in turn by the head-to-head's own visit.

## Why not the other ways

- **In the compiler's Rust, beside `js_pure.rs`**: it would know each value's
  type, but the row's shape and the renderer's internals would become an
  interface between two languages; written beside the renderer it
  specializes, the agreement test loads both in one engine and holds them
  to each other row by row.
- **The general walk made shorter at run time** (each hole from the last
  reached): fewer steps, the same loop and look-ups; written steps are both.
- **The packaging trusted with the markup's reading**: a parser does not
  read every markup as written; the comparison with its parse is once a plan.
- **A row's two comment anchors gone in this step** (NEXT 4): they are a
  format both renderers write and read, and this step changes none; its own
  decision, measured alone.
- **The keep loop given each list's own key, hold and making as hooks**:
  measured 383 bytes more with Brotli than the loop of the page's own code,
  which replaces the general one where every list is its own.

## Result

- **Found and fixed first** (`01fc394b`): a row that read `NaN` of the page
  was made again at every render (`!==` of what it read), and its computed
  value computed again; found by the planned rows' agreement (`fb18e636`),
  which holds a planned row to the server's renderer: 3,565 planned rows
  and 5,626 holes of 6,000 generated lists and the generated templates,
  96 rows and 288 holes of the compiler's own (store, kiokun, the large list
  and its fixture), every row's text, each hole, each refusal and each row
  kept.
  Seven controls of the planned row added to `browser_renderer_mutations.py`
  are killed by it and each survived the agreement before it.
- **Held**: the page's own code against the general code, row by row
  (`renderer-agreement.test.mjs`), every field, capture, text, refusal and
  row kept the same: of the generated lists and templates, 3,302 lists
  written of 5,780 (2,478 refused, most inside a block that binds a name),
  3,429 rows made by them and 2,232 kept, 7,891 renders refused by both; of
  lists whose rows compute a value, under a name of one field or of two as
  the compiler names one, 1,368 written and 2,612 rows made with their
  function, 546 and 166 without; of the compiler's own templates, 79 lists
  written of 95, 185 rows made and 110 kept. The packaging
  (`optimize-assets.test.mjs`, 42): the lists written and refused, the
  build's reading, the walk over a tree of nodes, what a build holds of
  either code, and a build with no list of its own packed from the runtime
  as it was. The browser suites on the builds as served and built to
  report, three engines (306 tests, 258 and 48): the large list's rows by
  its own code and walk, a table's row by the general walk, a list in a
  case's arm kept by the general code with each kind of hole, a class its
  rows compute from the page and a button with two handlers. Run whole by
  `just e14-device-runtime` on `2d296cc8` (`t32/device-runtime-evidence-2.txt`):
  `own_rows_mutations.py` 38 of 38 killed, `device_runtime_mutations.py`
  210 of 210.
- **What the controls' first whole run found** (35 of 38 killed): a guard in
  the reading of a value of the page (`head !== B`) that no path reaches, as
  one from the row is read before it, removed; the agreement read a row's
  text before its captures, and the text makes the row's scope, so captures
  read in no scope of the row's went unseen, now asked first, as a press can
  come before the text is read; and the hook that gives the page's own code
  the functions a row computes with, whose absence the code's fallback hid,
  computing the same through the general `read`. The fallback now reads the
  path's fields, as the general code does where it holds no function, and a
  control of it was added, held by computed names of the compiler's own
  form (`row.#t.P~ui~5`), whose first field an item may hold a record at.
- **What the device runtime's controls found** (their first whole run on
  this step, 207 of 210, `t32/device-runtime-evidence.txt`): three of the
  general code survived, which the rows suite had held through the large
  list and the events fixture, whose lists are now their page's own code:
  an element's handlers each a hole, what a row's computed value reads
  noted as its markup's, and no row kept. The one list the general code
  still keeps there, the case's arm's, now computes its rows' class from
  the page and holds a button with two handlers, and two tests hold it
  (`2d296cc8`): a press that changes one row's class leaves the other's
  class, title, boolean attribute and node as they were, and, built to
  report, writes one of its two rows. Each of the three fails the suite now
  (7, 1 and 1 of 34 tests, `t32/red-general-rows.txt`).
- **Bytes** (`t32/pack_pristine.sh`, each app's one compiled build packed
  with each commit's runtime and packaging as git holds them,
  `t32/bytes-vs-fb18e636.txt`, `t32/bytes-vs-train31.txt`; Brotli 11):

  | packed | before this step (`fb18e636`) | this step (`64e4b028`), less that | this step, less train-31 (`faf07898`) |
  |---|---|---|---|
  | the large list's page alone (its document's script) | 42,864 (14,989) | **+700 (+175)** | +740 (+218) |
  | the large list's device runtime | 38,208 (13,422) | +723 (+257) | +765 (+314) |
  | TodoMVC's page alone | 44,892 (15,325) | **byte for byte** | +40 (+12) |
  | TodoMVC's device runtime | 39,382 (13,668) | **byte for byte** | +42 (+5) |
  | a server's runtime, either app | 66,711 (21,971) | **byte for byte** | +42 (+20) |

  What train-31 differs by beyond this step is the fix of a row that read
  `NaN` (`01fc394b`), in every runtime.
- **First press, profiled** (`t32/prof_first32-after.log`, Chromium, nine
  rounds, the build the visits below measured): "create 1,000 rows" 9.48 ms
  of JavaScript cold, Svelte's 9.09 (from 12.40); with the processor slowed
  four times 43.10, Svelte's 46.52 (from 48.71, where Svelte's was 41.36: the
  two runs' Svelte apart by more than this build is from it). "Swap two
  rows" 1.80 (from 1.88) and 7.89 (from 8.56):
  what a swap spends is placing the rows (`rowsPlaced`, `staying`,
  `placesOf`), which this step does not touch.
- **The builds visited in turn** (`t32/ab-t32.txt`, `ab_rows.mjs`, train-31
  against this, eleven rounds a cell, this less train-31, quartiles). The
  build measured differs from `64e4b028` in one expression, which the
  controls' first whole run changed (above): a value a row computes, where no
  function is given, read then through the general code's `read` in a scope
  made for it, now its path's fields directly (+87 B, −81 B with Brotli, in
  the large list's page alone, its packed runtime otherwise byte for byte
  the build measured); a branch the large list takes only before its
  computed module is loaded.

  | | create | append | replace | update | every press |
  |---|---|---|---|---|---|
  | Chromium, cold | −3.5 (−6.1..1.3) | **+6.4 (3.5..8.6)** | −0.3 (−4.5..2.2) | 0.1 | +2.6 (−4.1..4.0) |
  | Chromium, warm | 0.3 (−2.4..2.0) | **−4.5 (−7.7..−3.7)** | 1.6 (−0.6..2.5) | −0.1 | **−7.9 (−11.8..−5.7)** |
  | Slow 4G, cold | **−15.2 (−23.2..−3.2)** | **+25.8 (20.1..28.7)** | −5.5 (−7.2..13.8) | **−8.0 (−10.2..−0.7)** | −4.6 (−16.4..23.6) |
  | Slow 4G, warm | −3.8 (−9.4..5.0) | **−44.6 (−48.5..−18.6)** | 14.3 (−8.8..32.4) | **−2.6 (−9.8..−0.5)** | −28.2 (−65.0..2.9) |
  | Firefox, cold | **−5.0 (−6.0..−2.0)** | 0.0 (−2.0..1.0) | **−4.0 (−7.0..−1.0)** | 0.0 | **−6.0 (−11.0..−3.0)** |
  | Firefox, warm | 0.0 (−4.0..4.0) | 0.0 (−4.0..1.0) | −4.0 (−5.0..0.0) | 0.0 | −7.0 (−9.0..10.0) |

  Past its spread too: Chromium's warm "clear all rows" −4.9 (−8.1..−2.8)
  and Slow 4G's warm swap −1.5 (−4.2..−0.3), quicker; Slow 4G's warm "clear
  all rows" +2.8 (1.0..5.4), slower. Firefox's cold select, remove and clear,
  +1.0 each, a millisecond its clock reads whole, have spreads from 0.0.
  Every other cell is within its spread.
- **The cold append, slower, is a collection moved into it**: its change
  reaches the document sooner in every round (6.1 to 6.4 ms against 6.3 to
  7.0) and its frame later (3.7 ms, 2.3..5.3, `t32/ab-append-detail.txt`).
  The head-to-head's own visit traced (`t32/h2h-traced-append.txt`, six
  rounds a build) has this build's append hold V8's incremental marking and
  a mark-compact before its frame in four of six, train-31's a scavenge
  alone; its own work in the press is less (`t32/prof-append.log`: 7.80 ms
  of JavaScript to 8.12, 0.68 of collection to 1.47). Where a visit's major
  collection falls moves with what it allocated (ADR-0432's result): every
  press of a cold visit is within its spread in Chromium and under Slow 4G,
  and quicker in Firefox.

## Consequences

- A planned list of a device's page is kept and made by code its build
  wrote, held to the general code by the agreement, and the general code to
  the server's.
- The large list's page alone is 175 B heavier with Brotli (+700 decoded);
  a page none of whose lists is written is sent what it was. The byte rows
  Svelte wins are ADR-0434's later steps' (a block's markup, the general
  renderer folded away).
- A cold visit's append is slower to its frame in Chromium, its press doing
  less: a major collection falls in it (NEXT).
- A mutant that changes what the packaging writes in place of the general
  code (`OWN_EDITS`) changes it only where a list is written: the runtime as
  written is the general code, and a build with no list of its own is packed
  from it.
- The general code that keeps and makes a planned row is held in a browser
  only by a list the packaging does not write: the rows suite keeps one
  (the events fixture's case's arm) with each kind of hole, a value its rows
  compute from the page and an element with two handlers. A feature of
  planned rows that list lacks is held there by the agreement alone.
