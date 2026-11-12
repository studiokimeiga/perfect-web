# ADR-0432: A list's rows are made as they changed, and a planned row is of one shape

Status: accepted 2026-11-11, to land with train-27. Proposed 2026-11-11, on
branch `perf/train-27-20261111`, under the owner's delegation of 2026-10-02
and order of 2026-10-25 ("deciding by best judgement without asking"), and the
order of 2026-11-09 that every row Svelte wins be a defect until it is fixed
or recorded. Milestone: E14. Builds on ADR-0417, ADR-0420, ADR-0427 and
ADR-0430.

## Context

The large list's presses are most of the head-to-head's losses at `87446c10`:
"create 1,000 rows", "append 1,000 rows" and "replace all rows" in every
engine, a few milliseconds behind Svelte to their frame, and under Chromium's
Slow 4G, whose processor is four times slower, "update every 10th row" and
"swap two rows" too.

Each press was profiled in Chromium, Pleris's large list packed for
production with its names kept and Svelte's as the head-to-head serves it:
ten presses each after one that warms the engine, each clicked in the page and
waited for there, so that nothing of Playwright's runs while it is measured.
Pleris's JavaScript was twice Svelte's to make a thousand rows (7.16 ms a
press against 3.64), and it allocated half as much again (2,842 KiB against
1,875). Three of the causes were the runtime's alone:

- **Every change looked for and placed every row** (`listMade`). A list made
  from none, rows added at its end, or a list replaced whole were each given
  the whole of the general way: a map of the old keys and a look-up of each
  new one (`placesOf`), a set of those that stay, a pass for those changed, a
  filter for the new, `every`, `indexOf` and `lastIndexOf`, the longest run
  that need not move (`staying`), and a walk back over every row; none of it
  finds anything in those three.
- **A planned row had a getter of its own** (ADR-0427's `get text()`, an
  object literal's). V8 makes such an object one of slow properties, a
  dictionary of its own, and Node 22's V8 says so (`%HasFastProperties`):
  about 920 bytes a row in Chromium, a third of what making a thousand
  allocated, and each of the row's fields looked up by name wherever it is
  read, which a press does for every row it keeps (`holdsStill`).
- **Each key was noted twice**: in the map of rows a render makes and in a set
  of its own (`rowKey`'s), only to refuse a repeated key.

## Decision

1. **Each row where it was, and any new ones after them**: where the old rows
   are the first of the new, in their order (an update, a list made from
   none, rows added at its end), those there are changed in place and the new
   ones go in after them as one fragment. No row is looked for, placed or
   moved.
2. **No row stays**: where none of the old keys is among the new (a list
   replaced or cleared), every row goes at once (`rowsAllGone`, as before),
   and the new ones go in as in 1.
3. **Otherwise the rows are placed** as before (`rowsPlaced`, cut out of
   `listMade`): those gone taken out, those changed set, the new put in, and
   as few moved as leave the rest in their order.
4. **A planned row is a plain object of plain fields**, its text `null`
   until something first reads it (a morph or a parse), which the renderer's
   `textOf` then writes and keeps; every reader of a row's text asks
   `textOf`. V8 then makes each planned row by copying the one object it
   keeps for that literal: fast properties, one shape, in code optimized or
   not.
5. **The map of rows a render makes refuses a repeated key**; the server's
   way of writing a list, which makes no such map, notes its keys as before.
6. **A list's change says which it was** (`rows in order: …`, `rows replaced:
   …`, `rows placed: …`), a line of the log a build that reports keeps
   (ADR-0430), which `rows.spec.mjs` reads for each press.

## Why not the other ways

- **The getter on a prototype the literal names** (`{ __proto__: P, … }`),
  this decision's first form: one shape, and in optimized code each row half
  the bytes; but V8 makes such a literal an empty object, sets its prototype
  by a call into the engine, and adds each field after, into a store that
  outgrows the object. A press is made once on a page just loaded, in code
  not yet optimized, and there making a thousand such rows was 5.03 ms to
  0.81 (`plannedRowOf`'s own time, twelve first presses of a cold visit
  each, profiled on 2026-11-11): the interleaved comparison of the two
  builds had a warm "create 1,000 rows" 3.0 ms slower to its frame, and
  7.1 ms under Slow 4G.
- **A class for the planned row**: one shape too, but its constructor's
  twelve assignments are 116 bytes more, minified, than a literal, and a
  call each.
- **Finding the three cases inside the general way**: it is its passes that
  cost; asked first, each case costs one pass over the rows it keeps.
- **A row that is one element as its own anchor**: its two comments are two
  of every three nodes a thousand rows insert, and about 0.7 ms of making
  them in Chromium; but the host writes them, the page's index and
  `rowsShown` read them, and taking them away is a change to both renderers,
  a decision of its own (NEXT).
- **An `Int` as a number where it fits**: a handler's arithmetic is in
  `BigInt`, and the large list's own code allocates about a quarter of what
  making a thousand rows does (an operation's result each, an `Option` for
  each `List.get`); that is the compiler's, a decision of its own (NEXT).

## Result

The two builds of the large list, `c0bc24fe`'s and this decision's
(`b3f0beed`), packed for production and visited in turn by the head-to-head's
own visit (`measureOnce`), eleven rounds a cell in Chromium on 2026-11-11, each
press timed from its input event to the frame after its change; this
decision's less the other's, the median of each round's pair (quartiles):

| | every press of a visit | "create 1,000 rows" | "append 1,000 rows" |
|---|---|---|---|
| cold | −5.2 ms (−7.6..−0.1) | −0.7 (−2.4..1.2) | −0.8 (−6.4..0.0) |
| cold, Slow 4G | −14.9 ms (−24.6..−7.3) | −7.1 (−10.3..−3.4) | 2.9 (−6.9..4.5) |
| warm | 1.5 ms (−4.0..5.6) | 2.6 (1.4..4.0) | 3.5 (0.9..4.3) |
| warm, Slow 4G | 22.0 ms (−19.0..46.1) | 5.7 (3.1..11.2) | 17.4 (8.2..32.1) |

Cold under Slow 4G, "update every 10th row" was 4.8 ms quicker (−8.1..−3.0)
and "replace all rows" 2.9 (−6.5..−1.6); of the presses not in the table,
the quartiles of each other cell held zero. Firefox was compared with decision 4's
first form (below), eleven rounds of each condition, and no press moved by
more than its quartiles; this form was compared in Chromium alone.

A warm visit's create and append are slower to their frame where this
decision's JavaScript is less. A first press profiled, twelve rounds of each
build with its names kept, cold: the press's JavaScript 12.79 ms to 12.47,
`plannedRowOf`'s own 0.80 to 0.52, `listMade`'s 0.66 to 0.22; warm, 9.48 to
9.13, but 0.10 ms of collection to 1.31. A warm visit loads the page in the
renderer the visit before it used, whose young generation V8 sized by what
that visit allocated, and the base allocated more. With the young generation
set to 16 MB for both (`--js-flags=--min-semi-space-size=16`), ten rounds:
create's JavaScript 9.94 ms to 8.46 and append's 9.18 to 7.82, the collection
in the base's append (3.53 ms) and in this decision's create (1.42): the
same work, less of it, collected in another press.

The large list's document, as its host sends it: 15,919 bytes of Brotli to
16,030, and 48,447 decoded to 48,636; the device's runtime 37,990 bytes
minified to 38,173. TodoMVC's document 19,187 bytes of Brotli to 19,198, and
63,226 decoded to 63,396. The three ways and `textOf`, which each reader of a
row's text asks, are the bytes.

The head-to-head is train-28's too (ADR-0433's result).

Each way a list changes is read in `rows.spec.mjs` from the log of a build
that reports, in Chromium, Firefox and WebKit: rows made, added and updated
in order, a replace and a clear at once, a swap and a remove placed. Seven
mutants of `e14-device-runtime` stand for the new ways (rows added after
those kept put in before them, or not at all, or all the rows; a row kept
taken for one out of place; rows kept compared past those that are not; rows
in order looked for and placed; a list replaced looked for and placed), and
five are re-anchored, the planned row's unwritten text on `textOf`: 210 of 210
killed, run on this PC at `b3f0beed`. `e14-browser-renderer`'s key that is not
noted is its eighty-second: 82 of 82. The audit's ceilings: `listMade`'s CRAP
871 to 463, its cognitive complexity 36 to 26 and cyclomatic 29 to 22;
`rowsPlaced` (73) and `rowsNew` (31) new; the renderer's 241 to 273, for the
emit loop that notes each key.

## Consequences

- The large list's presses do less, and none of them is yet as quick as
  Svelte's; what is left is NEXT's, after trains 27 and 28.
- **A profile of optimized code does not say what the record will show.**
  The prototype form of decision 4 was quicker in ten presses after one that
  warmed the engine, and slower in the one press each visit makes; it was
  found by visiting both builds in turn, round after round, which the
  record's seven runs of each stack could not tell from noise.
- Where a collection falls in a warm visit moves with what the visit before
  it allocated: a change that allocates less can move one into a press, and
  its worth is the whole visit's.
- Every way a list can change is told in its log, so a press that takes the
  general way where a quick one was meant fails a test, not a measurement.
