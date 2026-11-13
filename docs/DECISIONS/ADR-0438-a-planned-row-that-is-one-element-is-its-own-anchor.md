# ADR-0438: a planned row that is one element is its own anchor

Status: accepted 2026-11-12 (train-33, branch `perf/train-33-20261112`), on
train-32; the next step of ADR-0434's rows, NEXT's first item after
train-32. Accepted with three presses of the head-to-head's sequenced visit
recorded, slower to their frames where their own work is not (Firefox's warm
"update every 10th row", Chromium's warm "clear all rows" and "select a
row"; Consequences). Under the owner's delegation of 2026-10-02 and order of
2026-10-25, and the order of 2026-11-09 that every row Svelte wins be a
defect until it is fixed or recorded. Milestone: E14. Builds on ADR-0427,
ADR-0428, ADR-0432, ADR-0435 and ADR-0437.

## Context

A keyed list's row is written, by the host and by the device alike, as its
markup between two comments, `<!--pw:s6@T-->` the row `<!--pw:e6@T-->` (T
its token): what the page's index, the morph, the moves and the rows a plan
makes (ADR-0427) find a row by. For the large list's `<tr>` that is two of
every three nodes a thousand rows insert: `createComment` and two of every
three appends to the fragment, about 0.7 ms of 6.6 in Chromium, and about 2
ms of layout a thousand rows in Firefox and 1 to 2 in WebKit (a probe of
2026-11-11, NEXT). ADR-0437 found a third cost: V8's initial global
allocation limit counts the DOM's and layout's heap, and with a leaner heap
the page's first mark-compact started inside the cold "append 1,000 rows";
it named fewer DOM nodes a row as the remedy.

Two decisions put this off. ADR-0427: the anchors cost 2 ms of script and
nothing to the frame, and "a row found by its key in the document, by a
morph, needs them". ADR-0432 and ADR-0437: the anchors are a format both
renderers write and read, a decision of its own.

What reads a row's anchors, on a page that talks to no server: the index
(`buildIndex`, which passes over a row's inside where presses are heard at
the document); the rows the document shows (`rowsShown`); a row's nodes
gone, put and moved (`rowGone`, `rowPut`, `rowsPlaced`); the rows parsed
(`rowsParsed`) and morphed (`rowsMorphed`); the holes the plan finds in a
row it did not make (`holesFound`); and the morph of a whole block, which
matches a row by its key (`rowsOf`, `morphChildren`). On a page with a
server, the server's patches, the speculation (ADR-0172) and the index's
frames, which address what is inside a row.

## Decision

1. **The packaging says which lists' rows are their own anchors**, in the
   build's template IR (`row_anchor: "element"` on the list,
   `optimize-assets.mjs`'s `rowAnchors`), which the host renders the page's
   document from and the page's runtime reads: each list a block keeps
   whose rows a plan makes (ADR-0427), of a build whose pages that talk to
   no server show no view, so that their presses are heard at their
   documents and nothing addresses what is inside a row, and whose row is
   one element as HTML's parser reads it wherever it is written
   (`own-rows.mjs`'s `rowElement`):
   - its markup one element, nothing beside it, not even a space;
   - each tag ended in turn, no comment, no `</` that ends nothing;
   - its element one of those a row may be its own anchor as (`ENDED_BY`),
     and none inside it that would end it there, as HTML's "in body", "in
     row" and "in cell" end an open element where another starts: an
     `<li>` in an `<li>`, a `<div>`, an `<li>`, a `<dd>` or a heading in a
     `<p>`, an `<a>` in an `<a>`, a table's part in a `<tr>` or a `<td>`;
   - nothing a parser does not keep as written (`<template>`, `<textarea>`,
     `<svg>`, `<body>` and the rest);
   - a table's row holding its cells alone, as what else is in it a parser
     moves out.
   Each list it does not say so of is said, by its page and part, with why
   (`pw packaging: list N of PAGE keeps its rows' comments: WHY`), and keeps
   its comments. The compiler writes no `row_anchor`; a development build is
   not packed, and its rows keep their comments.
2. **Both renderers write such a row's key on its element**, `data-pw-row`,
   right after the element's name as HTML's tokenizer ends it (a space of
   its kinds, a `/`, a `>`, or the markup before a hole): `<tr
   data-pw-row="6@T" data-pw="6" class="">`. The server's renderer
   (`pw-render`'s `RowAnchor` and `row_written`) and the browser's
   (`rowOwn`, in each of the three places it writes a keyed row: a block's
   rows, a row's text, an instance; the element's name cut once a list)
   refuse alike a row whose list says this and whose markup begins with no
   element.
3. **A row a plan makes is its element, cloned alone**: no fragment, no
   comment, and its key under a symbol of the runtime's own (`ROW_KEY`, as
   ADR-0435's `ROW_OF`), not an attribute. The plan's clone is the row's
   element and each hole's path is from it (`planOf`, the first step of
   each, to the one element, gone), so the general walk and the walk the
   packaging writes (`walkSource`) are the code they were: the large list's
   five holes in seven steps where they took eight.
4. **Each reader reads both forms**: a row's key is its element's symbol or
   its `data-pw-row` (`rowKeyOf`); a row its own anchor is `[its element,
   its element]` where one between comments is `[its start, its end]`, so
   that what is gone, put and moved is unchanged; the morph keeps such a row
   as its element, by its key; the index passes over it whole; the holes of
   such a row the plan did not make are found by the plan as parsed, from a
   stand-in for an anchor before the element (`holesFound`).
5. **The list's own two anchors stay**, also where it is its parent's only
   content: measured, no difference (below), and they are what the index
   addresses the list by.
6. **What a build holds is what it reaches** (ADR-0428, ADR-0437): the lines
   only a row its own anchor needs are marked (`// element rows:` ..
   `// :element rows`), fifteen runs, and cut from the device's runtime of a
   build none of whose lists is so, and from a server's runtime. The runtime
   outside them is train-32's, so such a build is minified to the bytes it
   was: TodoMVC's document and runtime, and a server's runtime, are byte for
   byte train-32's.

## Why not the other ways

- **One writer for every keyed row** (`rowWritten`, this step's first
  form): the renderer's three places made one, 89 B less in a server's
  runtime; but every build's esbuild names moved with it, and TodoMVC's
  document was 79 B heavier with Brotli for a change none of its rows makes.
  A saving in the server's runtime is a step of its own.
- **An attribute set on each row a plan makes** (`setAttribute`): one form
  in the document and in text, but 0.2 ms a thousand rows more than the
  symbol in Chromium (`anchor_forms.mjs`), and an attribute's storage in
  each row; nothing reads the key of a row a plan made but the runtime. In
  the builds visited in turn (below) no press quicker, Firefox's warm create
  a millisecond slower (0.0..2.0).
- **The fragment cloned, its first child the row**: 1 ms a thousand rows
  more in Firefox, 0.2 in Chromium, the fragment garbage.
- **Each renderer deciding by itself**: the rule in Rust and in JavaScript,
  two readings of HTML's parser to drift, and the rule's bytes in every
  runtime; decided once, by the packaging, and carried by the IR, each
  renderer only obeys it, and the agreement holds them to each other.
- **The host left writing comments**: the document's text would not be the
  device's, and one row would have two forms a morph must match.
- **Every keyed row**, a server's page's too: its patches, its speculation
  and its index's frames address what is inside a row by its comments.
- **A row with a space beside its element** (a `<ul>`'s `<li>`, as the
  compiler writes a list there): three nodes a row, which keep their
  comments; what the compiler writes between elements is the compiler's.
- **The list's own anchors dropped** where the list is its parent's only
  content: measured, no difference at a thousand rows.

## Result

- **The forms, before the code** (`t33/anchor_forms.mjs`,
  `t33/anchor-forms.txt`): 1,000 of the large list's rows made as the
  runtime makes them, put in a table's body, each form interleaved, medians
  of 21 after five rounds that warm the engine, at a load of 1.9 to 2.5; the
  script alone, the style and layout it asks for alone, and the whole to the
  next frame, ms:

  | create | Chromium | Firefox | WebKit |
  |---|---|---|---|
  | two comments, the fragment cloned (as before) | 2.60 / 18.4 / 25.1 | 7 / 20 / 27 | 3 / 39 / 43 |
  | the element cloned, its key an attribute | 2.20 / 18.0 / 24.3 | 5 / 19 / 26 | 3 / 39 / 42 |
  | the element cloned, its key under a symbol | 2.00 / 18.5 / 24.6 | 5 / 20 / 26 | 3 / 38 / 42 |
  | the fragment cloned, its key under a symbol | 2.20 / 18.8 / 25.1 | 6 / 20 / 26 | 3 / 39 / 42 |
  | the element cloned, its key nowhere | 1.90 / 18.5 / 24.5 | 5 / 19 / 25 | 2 / 39 / 42 |
  | the symbol, the list's own two anchors gone | 2.00 / 18.4 / 24.3 | 6 / 19 / 26 | 2 / 39 / 42 |

  The comments are 0.6 ms of script a thousand rows in Chromium and 2 in
  Firefox; the layout after is the same in each engine to its clock; the
  list's own two anchors move nothing (replacing: 5.9 against 6.2 ms of
  script in Chromium, the rest level).
- **The builds visited in turn** (`t33/ab_rows.mjs`, `t33/measure_final.sh`,
  `t33/ab-final.txt`; train-32 at `120e6487` against this train's code at
  `b0b3662c`, whose builds the later commits pack byte for byte, eleven
  rounds a cell, this less train-32, the median of each
  round's pair and its quartiles, ms to the frame; each cell begun at a
  one-minute load under 4, 3.4 to 4.0):

  | | create | update | select | swap | remove | append | replace | clear | every press |
  |---|---|---|---|---|---|---|---|---|---|
  | Chromium, cold | −0.9 (−2.8..0.2) | −0.1 | **−0.3 (−0.5..−0.1)** | −0.3 | −0.1 | **−4.1 (−6.9..−1.6)** | −0.8 (−3.6..1.4) | 0.1 | **−6.0 (−10.6..−2.1)** |
  | Chromium, warm | −0.9 (−1.9..0.3) | 0.1 | **+0.5 (0.3..0.7)** | −0.2 | **−0.6 (−0.6..−0.2)** | −0.9 (−1.7..0.7) | −2.1 (−4.0..0.4) | **+2.6 (1.5..6.0)** | −2.8 (−5.9..4.2) |
  | Firefox, cold | **−3.0 (−4.0..−1.0)** | −1.0 | −1.0 | 0.0 | −1.0 | **−6.0 (−12.0..−5.0)** | −6.0 (−11.0..0.0) | **−1.0 (−3.0..−1.0)** | **−21.0 (−27.0..−12.0)** |
  | Firefox, warm | −2.0 (−2.0..0.0) | **+4.0 (2.0..5.0)** | 0.0 | 0.0 | 0.0 | −2.0 (−5.0..1.0) | **−4.0 (−8.0..−2.0)** | −1.0 | **−6.0 (−15.0..−3.0)** |
  | Slow 4G, cold | **−0.8 (−10.3..−0.4)** | 0.3 | −1.4 (−2.0..0.2) | **−4.1 (−10.1..−3.5)** | **−2.8 (−3.9..−0.2)** | −4.8 (−18.3..2.6) | −9.9 (−17.1..4.2) | **−2.7 (−4.0..−2.2)** | **−16.7 (−52.2..−10.4)** |
  | Slow 4G, warm | **−5.7 (−9.9..−2.8)** | 0.9 (−0.3..2.1) | 0.1 | **−4.1 (−5.2..−2.6)** | −0.1 | **−10.9 (−11.8..−4.9)** | **−10.1 (−11.0..−3.8)** | −1.0 | **−28.2 (−35.6..−16.5)** |

  Bold: past its spread. A second look, 21 rounds a cell
  (`t33/ab-final-more.txt`): Firefox warm, update +4.0 (3.0..4.0), append
  −3.0 (−4.0..−2.0), replace −5.0 (−6.0..−3.0), clear −1.0 (−1.0..−1.0),
  every press −6.0 (−7.0..−3.0); Chromium warm, create −1.9 (−2.7..−1.2),
  select +0.6 (0.3..0.9), append −3.4 (−7.2..−1.7), replace −2.4
  (−4.6..−1.3), clear +2.3 (1.9..3.8), every press −4.4 (−10.3..−3.1). The
  code first measured (`c453988f`, below) against this, no press past its
  spread: Chromium cold (21 rounds) every press −2.7 (−5.2..0.8), Chromium
  warm and Firefox warm (eleven) −1.4 and −2.0.
- **What the three slower presses spend: no more of their own work, a later
  frame.** Each press split (`t33/press_wait.mjs`, `t33/press-wait.txt`,
  21 rounds a cell): from its click to the change (its script), the style
  and layout the change asks for, forced at once, the wait to the frame's
  callbacks, and from them to a task after the frame (its paint). In every
  engine, condition and press the script is less where rows are made (0.9
  to 4.5 ms) or gone at once (0.3 to 0.7), within 0.2 ms elsewhere, and the
  style and layout and the paint the same or less to their spreads.
  Firefox's warm update: 2.06, 4.08, 4.68 and 0.78 ms against 2.10, 4.10,
  2.80 and 0.78; what it adds is the wait for Firefox's next frame. Chromium's warm visit traced
  (`t33/phase_probe.mjs`, `t33/phases.txt`, the head-to-head's own visit,
  seven rounds a build): its clear's main thread busy 5.01 ms against 5.20
  (its script 3.29 against 3.74, a scavenge of 1.9 ms among it), and 9.70
  waiting for the frame against 1.97; its select busy 3.83 against 4.04,
  12.79 waiting against 11.56. A press of the sequenced visit begins where
  the one before it left the engine's frame interval, and a time to the
  frame carries up to a frame of waiting (NEXT, after trains 27 and 28,
  item 3). **With each click at a point of the frame interval drawn for its
  round** (`t33/press_random.mjs`, `t33/press-random.txt`, the same points
  for each build, 41 rounds warm and 21 cold), the whole from the click to
  a task after the frame, this less train-32:

  | | create | update | select | swap | remove | append | replace | clear |
  |---|---|---|---|---|---|---|---|---|
  | Firefox, warm | **−2.42** | −0.34 (−3.68..0.98) | −0.14 | −0.28 | 0.00 | **−2.42** | **−3.96** | **−0.90 (−1.32..−0.58)** |
  | Firefox, cold | **−2.26** | −0.28 (−6.60..4.20) | −0.08 | −0.34 | −0.16 | **−2.76** | **−4.66** | **−0.70** |
  | Chromium, warm | −1.20 | 0.10 (−0.80..0.90) | 0.80 (−1.70..1.80) | −0.40 | −0.10 | −2.30 (−3.20..0.30) | **−2.40** | −0.70 (−1.10..0.10) |
  | Chromium, cold | −1.90 | 0.10 | −0.70 | 0.00 | **−0.10** | −0.90 | **−2.80** | −0.40 |

  No press is slower past its spread. Profiled four times slowed, warm
  (`t33/prof-update-select.txt`, 15 rounds, names kept), the update's
  JavaScript is 6.06 ms against 6.00 and the select's 6.95 against 7.67.
- **The key's form and the comments, in the builds visited in turn**
  (`t33/ab-forms.txt` and `t33/ab-factors.txt`, built by
  `t33/build_variants.sh` and `t33/build_factors.sh` from the code first
  measured, `c453988f`, 21 rounds a cell): the key an attribute each row a
  plan makes is given, against the symbol, moves no press past its spread
  in Chromium cold and warm or Firefox cold, and Firefox's warm create +1.0
  (0.0..2.0); the key nowhere, the same as the attribute against train-32.
  What Firefox's warm update follows, against train-32: this code with no
  list said (its rows between comments, their fragment cloned) 0.0
  (−1.0..1.0); each row's element cloned alone between two comments +1.0
  (−1.0..2.0), and its append, replace and every press no quicker either;
  the fragment cloned with no comment +4.0 (2.0..5.0), with this step's
  gains (append −6.0, replace −5.0, clear −2.0, every press −9.0): the rows
  having no comment between them, as Svelte's have none.
- **The first press, profiled** (`t33/profile_first.mjs`,
  `t33/prof-final.txt` and `t33/probes-final.txt`, Chromium, names kept,
  train-32, this, and Svelte's build; a load of 2.3 to 3.0):

  | ms of JavaScript + collection | train-32 | this | Svelte |
  |---|---|---|---|
  | create 1,000 rows, cold (15 rounds) | 9.78 + 1.85 | **8.80** + 2.76 | 9.11 + 2.54 |
  | slowed four times (15) | 42.39 + 3.71 | **36.93** + 5.12 | 38.10 + 3.85 |
  | append 1,000 rows, cold (9) | 6.82 + 0.65 | **5.72** + 0.98 | 5.64 + 1.40 |
  | slowed four times (9) | 28.28 + 1.96 | **23.07** + 2.41 | 24.11 + 3.09 |

  Slowed (nine rounds), the create's `createComment` (0.64 ms) and its
  fragment's `append` (2.65) gone, `cloneNode` 6.78 to 5.82 and
  `insertBefore` of the rows into the document 2.35 to 1.52, `appendChild`
  1.23 in their place. The collection is an average: the first round's
  major collection (28.8 and 41.0 ms slowed) is in each build's; its median
  a round, cold, 0.9 ms, 3.6 here and Svelte's 1.8, a scavenge of 2 ms or
  more falling in this create in 9 rounds of 15 and in train-32's in 5
  (JavaScript and collection 11.56 ms against 11.63, Svelte's 11.65).
- **NEXT's check** (`t33/mem_probe.mjs`, `t33/gc-final.txt`, the
  head-to-head's own cold visit traced, five rounds a build, at a load of
  2.9 to 3.2): five scavenges before the append in both; their average
  survival 39.7 to 40.0 % (train-32) and 40.1 to 40.2 % (this); the global
  allocation limit at the last of them 48.8 MB (48.2..49.7) and **51.5 MB
  (50.9..51.7)**. The first mark-compact still starts inside the append in
  every round, at the global allocation limit, 22.9 to 25.2 ms after its
  click and 24.2 to 25.2 here: the comments were two nodes of a row's
  twelve, and what the DOM keeps of a thousand rows is not what moves V8's
  limit far enough. Its marking and finalizing in the press is the same
  (`t33/gc_presses.py`: 4.11 ms against 4.13), and the append reaches its
  frame sooner (41.1 ms against 43.8, the medians of the five traced
  rounds).
- **Bytes** (`t33/bytes.sh`, `t33/bytes-final.txt`, each build packed for
  production as the head-to-head's, its document as its host sends it;
  Brotli 11):

  | packed | train-32 (`120e6487`) | this, less that |
  |---|---|---|
  | the large list's document | 48,203 (16,209) | +1,232 (+354) |
  | the large list's page alone (its document's script) | 43,564 (15,164) | **+1,209 (+472)** |
  | the large list's device runtime | 38,931 (13,679) | +1,217 (+453) |
  | TodoMVC's document | 61,980 (19,066) | **byte for byte** |
  | TodoMVC's page alone | 44,892 (15,325) | **byte for byte** |
  | TodoMVC's device runtime | 39,382 (13,668) | **byte for byte** |
  | a server's runtime, either app | 66,711 (21,971) | **byte for byte** |

  Of the large list's, decoded, the browser's writing of such a row
  (`rowOwn`) is 346 B, the index's, the morph's and the readers' 5 to 90
  each, the plan 140, the holes found 60 (`t33/region_costs.mjs`, each run
  cut alone).
  The code first measured (`c453988f`) wrote every keyed row through one
  writer and its walk from a row's first node: 157 B less in the large
  list's page alone (55 with Brotli), TodoMVC's document 79 B heavier with
  Brotli and a server's runtime 89 B lighter; its presses measured as this
  code's (above).
- **Held**: the packaging (`optimize-assets.test.mjs`, 47): the rule's
  verdict on each shape it takes and each it refuses by name, what a build
  says and holds of a row its own anchor (a page with a view, a page none
  of whose rows is one, a page its server renders, alone and beside a
  device's page), the walk from a row's element, and the fifteen marked
  runs cut to the runtime as it was. The agreement
  (`renderer-agreement.test.mjs`, 27): the server's renderer, the browser's
  general code and the page's own code held to each other row by row over
  generated templates, half their keyed lists said their rows' own anchors,
  and 4,000 generated rows of one element or of none (7,325 planned rows,
  1,821 of them their own anchors, 22,799 holes; of the compiler's own
  templates 126 and 42), every way a tag's name ends, and each refusal. The
  server's renderer (`pw-render`'s `row_anchors.rs`, 6). The browser suites
  on the builds as served and built to report, three engines
  (`t33/gates_browser.sh`, `t33/browser-gates.txt`, at `b0b3662c`): 276
  and 51 tests, and the pages that render through the browser's renderer,
  120: the large list's thousand rows a thousand elements between the list's
  two anchors across its presses, the document's rows their keys, a morph
  keeping rows by their keys, rows a morph kept set by the plan, rows whose
  plan a parser cannot make parsed and morphed, a block shown again, and the
  rule held to each engine's parser over 3,000 shapes. Of the builds the
  suites serve, only the large list's (one list) and its fixture's (two) say
  a row its own anchor. The controls run whole, three slots each
  (`t33/mutations.sh`, `t33/mutations.txt`, `t33/mutations2.sh`,
  `t33/mutations2.txt`): `own_rows_mutations.py` 61 of 61 at `4af3dd8e`,
  `browser_renderer_mutations.py` 100 of 100 at `b37e1d90`,
  `device_runtime_mutations.py` 225 of 225 at `4af3dd8e`, each of the 5,794
  mutants' anchors matching once. The first whole run of the own code's, at
  `b37e1d90`, killed 60 of 61: the packaging reading a page its server
  renders as the device's survived, as no test built a page of each kind;
  one does now (`4af3dd8e`), and fails with that mutant. Fallow's new-only
  audit against master passes, the ceilings of the functions this step
  raised or added at their scores plus one (`b37e1d90`).

## Consequences

- A planned row of one element is one node: the large list's thousand
  rows are a thousand nodes, not three thousand, in the document the host
  writes and in what the device makes, and no `createComment` is made.
- **The three slower presses** (decided under the owner's delegation of
  2026-10-02). *The cost*: in the head-to-head's sequenced visit,
  Firefox's warm "update every 10th row" +4.0 ms (3.0..4.0), Chromium's
  warm "clear all rows" +2.3 to +2.6 and "select a row" +0.5 to +0.6. *The
  mechanism*: each press's own work is the same or less, its script,
  style, layout and paint, and what it adds is time waiting for the
  engine's next frame, as where the press begins in the engine's frame
  interval follows where the presses before it left it; Firefox's update
  follows its rows having no comment between them. *Why it is accepted*:
  with each click at a point of the frame interval drawn for its round, no
  press is slower, and every press of each visit is quicker in sum past
  its spread: −21.0 ms in Firefox cold and −6.0 warm, −6.0 in Chromium cold
  and −4.4 warm (21 rounds), −16.7 and −28.2 under Slow 4G. A record
  of the head-to-head may still show these presses a frame's wait apart
  from train-32's; a reader's click falls where it falls.
- The large list's page alone is 472 B heavier with Brotli (+1,209
  decoded); a build none of whose rows is its own anchor is sent the bytes
  it was, TodoMVC's and every server's. The byte rows Svelte wins are
  ADR-0434's later steps' (a block's markup, the general renderer folded
  away).
- **The cold append's mark-compact** (ADR-0437, Consequences) is still
  inside the press: the comments were not enough of what V8's limit
  counts. It stays recorded; NEXT says what would move it.
- A list's row with a space beside its element, as the compiler writes a
  `<ul>`'s, keeps its comments: what the compiler writes between elements is
  the compiler's (NEXT).
- A row that is no one element (a text, several roots, a block) keeps its
  comments, and every reader still reads them.
