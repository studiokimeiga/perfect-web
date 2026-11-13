# ADR-0439: A list's own code finds its rows where they were, and its rows are placed by what it found

Status: accepted 2026-11-13 (train-34, branch `perf/train-34-20261113`), on
train-33; the next step of ADR-0434's rows (NEXT's fifth item after
train-33, train-32's fourth: no page's own code placed rows). Under the
owner's delegation of 2026-10-02 and order of 2026-10-25 ("deciding by best
judgement without asking"), and the order of 2026-11-09 that every row Svelte
wins be a defect until it is fixed or recorded. Milestone: E14. Builds on
ADR-0428, ADR-0432, ADR-0434, ADR-0437 and ADR-0438.

## Context

The record at `8096c4e7` (`docs/evidence/E14/head-to-head.txt`, before
trains 31 to 33) had Svelte ahead in the large list's swap and remove in
Firefox warm (8.0 ms against 6.0, 7.0 against 5.0), and in Chromium under Slow
4G in the swap and the clear cold (33 against 27, 22 against 19) and the swap
warm (30 against 21). Train-33 left the swap and the remove within 0.6 ms of
train-32's.

**Where the first press's time went** (`t34/prof-base.txt`, the cold visit's
first press of each kind as the head-to-head makes it, Chromium's sampling
profiler, eleven rounds, train-33's code against Svelte's build, at a load of
3 to 4):

| ms of JavaScript + collection | train-33 | Svelte |
|---|---|---|
| swap two rows | 2.42 + 0.99 | 1.14 + 0 |
| slowed four times | 8.23 + 1.94 | 5.61 + 0 |
| remove a row | 1.81 + 0 | 1.09 + 0 |
| slowed four times | 5.34 + 0 | 5.60 + 0 |
| clear all rows (`t34/prof-clear-text.txt`) | 4.09 + 0 | 4.77 + 0.03 |
| slowed four times | 13.47 + 0 | 15.74 + 0.01 |

Of the swap, placing the rows (`rowsPlaced` 0.36 ms, `staying` 0.26,
`placesOf` 0.18, `listMade` 0.16, their callbacks 0.08) and keeping them
(`ownKept`'s pass, its callback 0.25, `computedHold` 0.12): each a pass over
the thousand rows. Every row's key was computed, a map of a thousand keys made
at each render to keep them by and another to place them by, a set of the
rows that stay, a filter of the new ones, a longest run over a thousand
places, and a pass that looked each row up by its key, to move two. A press on
a page just loaded runs that code unoptimized (ADR-0432), and its garbage
(565 KiB in the swap, `t34/alloc.txt`) was scavenged in the press. The remove
spent the same passes on a row gone. The clear's JavaScript was not where it
trailed: the browser taking the rows away was (`replaceChildren`, 3.48 ms;
Svelte's `textContent`, 3.74), and the ways of doing so measured alike in every
engine (`t34/clear-forms.txt`).

## Decision

1. **A list's own code finds a row where it was** (the keep loop the
   packaging writes for each list its page's own code keeps, ADR-0437,
   `own-rows.mjs`): a row whose item is the very one at its place in the
   list's last render, or as far from the list's end (a row gone or added
   before it), is that render's row, its key not computed. It is kept where
   nothing its rows can read of the page has moved since that render
   (decision 2), and held as the general code holds it otherwise. Any other
   row is looked for by its key among the last render's rows (`byKey`, kept
   from render to render), at its place there as that render wrote it (its
   `at`), or, where a render refused since wrote over it, looked for there.
   A key is taken where its row of the last render is (`taken`) or a row of
   it was made (`made`): a key missing or taken twice is refused at the row
   the general code refuses it at. What the loop keeps changes once every
   row is found, so a render refused keeps the last one's.
2. **What a list's rows read of the page is said with its code** (`reads`):
   each name a hole reads, whole or as a path's head, and each a value its row
   computes reads; none said (`null`) where a handler's captures read the
   page, which a press reads in the scope the row was made in, so that each
   such row is held as the general code holds it. A row kept at a render read
   what the page held then, so a row whose item is the very one is the row
   kept where none of those names has moved.
3. **What it found it says** in the list it renders: the rows it found them
   among (`from`), each row's place there, -1 for a key none had (`places`),
   which of those rows it holds and how many (`taken`, `matched`) and, where
   it holds some, those it does not (`gone`), whether the places of the rows
   that stay increase (`sorted`), the rows made again under their keys
   (`remade`), how many it looked for by their keys (`keyed`), and, where the
   list has as many rows as it had and holds each, those out of their
   places while they are four at most (`displaced`; else none).
4. **The runtime places such a list by what it found** (`rowsFound`, in the
   runtime's own lines, ADR-0437), where it found them among the rows the
   document shows: those gone taken out, at once where none stays; those made
   again set where they differ. Where the rows that stay are in order, only
   the new ones are looked for, between the first and the last of them.
   Where they are not, and a few are out of their places, every other row
   where it was (`displaced`), the rows to move are chosen among those few
   (`fewMoved`): each choice of the ones kept where they are, in order, moves
   the rest and each row in its place between where a kept one was and where
   it is, and is given up once it moves as many as the fewest yet; those of
   the fewest move, each before the row after it, the last first. Otherwise
   the rows in their places at either end are left as they are, and between
   them as few move as leave the rest in order (`stayingIn`: a longest run,
   which a place past its last adds to at once). Either way as few move as a
   longest run in order leaves out. The new ones go in, those side by side as
   one. A row is looked up by its key only to be moved, or as what one moved
   or put in goes before. A row moved is moved whole where the engine can, or
   where it holds what has focus, as `moveNodes` moves it; else put where it
   goes, nothing of it read (`rowMoved`: where the engine moves no node whole,
   `moveNodes` reads the scroll of what has focus, off its layout). Rows its
   own code found among other rows than the document shows (a block shown
   again, a render refused after the list was kept) are found by their keys
   (`foundByKeys`) and placed alike. Each says what it did as the general
   placing says it.
5. **What a build holds is what it reaches** (ADR-0428, ADR-0437): the
   general placing is marked (`// general rows:` .. `// :general rows`, four
   runs) and cut from a build every list of which is its page's own, which
   places each by what its own code found; a build with a list of the general
   code's holds both; one with no list of its own holds the general placing
   alone, every own line cut, and is minified to the bytes it was: TodoMVC's
   document and runtime and a server's runtime byte for byte.
6. **The interactive route's activation is bounded at 72 KiB** (ADR-0188's
   gate item 7b, amended; `e2e/runtime-size.spec.mjs`): the development host
   sends the runtime as it is written, its comments in it, the found rows'
   placing beside the general placing that a build with a list of the
   general code's still holds; 67,331 B with Brotli, past ADR-0188's 64 KiB
   (train-33's 65,188). A bound, not a target, as before: the control that
   grows the script by 48 KB that does not compress fails it still
   (`runtime_size_mutations.py`).

## Why not the other ways

- **Rows matched from both ends** (Vue 2's, train-29, `2b8a117b`, NEXT after
  trains 27 and 28, item 8): the placing alone, a millisecond quicker in
  Firefox and nothing in Chromium unthrottled, every row still kept by its
  key and placed by a map; 300 B more with Brotli in every runtime.
- **The keeping and the placing as one pass of the page's own code**, writing
  the document's moves: the renderer's rows, which the agreement holds to the
  general code's and the server's, would no longer be what the runtime
  places. What was found is said instead, held to the rows' keys, and the
  runtime's one placing reads it.
- **The rows at their places kept, the rest moved, no longest run**: a row
  moved five places would move six rows. A run is found only between the
  first row out of its place and the last, a place past its last added to it
  at once: a long move's is one pass of the rows between.
- **The rows out of their places moved, whatever they are**, for a swap's
  two: a row moved three places, four rows out of their places, would move
  four where one does; a choice of those kept is weighed instead, among four
  at most. More than four is what the longest run is for, whose pass over the
  rows between is what a few out of place, far apart, would otherwise cost.
- **The rows gone looked for only past the rows found in order** (`93396c91`):
  measured nothing (`t34/press-split-few.txt`, the remove's script −0.02 ms
  (−0.10..0.14) in Firefox cold, −0.04 (−0.18..0.04) warm, −0.10
  (−0.30..0.00) in Chromium cold), for its bytes; the keep loop scans the last
  render's rows for them, as before (`da3cf7f6`).
- **`textContent` to clear the list**: no quicker than `replaceChildren` in
  any engine (`t34/clear-forms.txt`), nor in the page (`t34/prof-clear-text.txt`).
- **`insertBefore` for every row moved**: `moveBefore` costs Chromium and
  Firefox no more (`t34/move-forms.txt`), and keeps what a moved row holds
  (focus, a frame, an animation) as the runtime's moves have.
- **The runtime's documentation shortened to fit 64 KiB**: 1,795 B of
  Brotli, some seven thousand bytes of comments, other steps' as much as this
  one's; the bound reads what the development host sends, and a production
  build sends 15,007 B of runtime with Brotli (the large list's device
  runtime) or 21,971 (a server's).
- **One placing in every build**: the general placing replaced in TodoMVC's
  runtime too would move its bytes and its esbuild names, for no list of its
  own; the cut keeps a build none of whose lists is its own as it was.
- **The general placing kept beside the found rows' placing**: code a build
  whose every list is its own never runs: 1,041 B of the large list's page
  alone, 418 with Brotli (`t34/placing-bytes-final.txt`).

## Result

Measured at `da3cf7f6` against train-33's code (`e5baa112`, 8906b707 packed
the same way) and Svelte's build, on the head-to-head's own visit; each cell
begun at a one-minute load under 4 (0.2 to 3.7); `t34/` is this train's
scratchpad, its tools and logs.

- **The first press, profiled** (`t34/profile_first.mjs`,
  `t34/prof-final.txt`, Chromium, the cold visit's first press of each kind,
  eleven rounds, names kept):

  | ms of JavaScript + collection | train-33 | this | Svelte |
  |---|---|---|---|
  | swap two rows | 1.95 + 0.64 | **0.99** + 0 | 1.02 + 0 |
  | slowed four times | 8.26 + 1.66 | **4.43** + 0 | 5.38 + 0 |
  | remove a row | 1.39 + 0 | **0.88** + 0.01 | 1.38 + 0 |
  | slowed four times | 5.78 + 0.22 | **4.11** + 0 | 5.81 + 0 |
  | clear all rows | 3.16 + 0 | 3.17 + 0 | 3.47 + 0 |
  | slowed four times | 13.41 + 0 | 13.37 + 0 | 14.43 + 0.24 |

  The swap's keeping is one pass of the rows by their items, no key
  computed; its placing reads two places and moves two rows. Of the other
  presses: create 9.76 + 2.53 against 10.75 + 2.34 (Svelte 10.54 + 3.12),
  44.09 + 3.50 slowed against 41.22 + 2.73 (49.54 + 10.31); update 1.41 +
  0.01 against 1.95 + 1.22 (0.89); select 2.16 + 0.93 against 2.13 + 0.06
  (1.24), slowed 11.29 + 3.72 against 9.84 + 0.09 (7.91 + 0.19), a scavenge
  that fell in the update now falling in the select of this probe's visits;
  append 6.90 + 1.00 against 6.05 + 0.80 (6.29 + 1.17), slowed 31.73 + 2.52
  against 25.79 + 2.50 (28.19 + 2.63); replace 11.06 against 10.34 (14.11 +
  1.23).
- **Allocation** (`t34/alloc_probe.mjs`, `t34/alloc-final.txt`, the cold
  visit, three rounds, KiB a press, the harness's own `next` among them):
  swap 224 against 569 (Svelte 287), remove 2,071 against 2,394 (2,054),
  clear 307 against 371 (533); no press allocates more.
- **The cold visit's collections** (`t34/mem_probe_any.mjs`,
  `t34/gc-final.txt`, five rounds a build): the scavenges fall in the same
  presses as train-33's (create 2, update, select, swap and replace one each,
  none in remove, append or clear), Svelte's none in its swap and one in its
  remove and its append; the global allocation limit at the last scavenge
  before the append 50.8 MB (46.0..53.3) against 50.3 (46.7..50.9); the first
  mark-compact starts inside the append in every round of each build, 26.4 to
  30.0 ms after its click (train-33's 25.5 to 28.3, Svelte's 54.6 to 76.8).
  The swap's scavenge, 1.23 ms, still falls in it: the presses before it fill
  the young generation, whatever the swap allocates.
- **The builds visited in turn** (`t34/ab_three.mjs`, `t34/ab-final.txt`,
  eleven rounds a cell, each round's pair, the median and its quartiles, ms
  to the frame; bold where the quartiles leave zero out), this less train-33:

  | | swap | remove | clear | every press |
  |---|---|---|---|---|
  | Chromium, cold | +0.1 (−0.3..0.5) | −0.1 (−0.3..0.2) | 0.0 (−0.2..0.1) | +1.5 (−2.9..4.2) |
  | Chromium, warm | −0.1 (−0.4..0.2) | −0.2 (−0.7..0.1) | **+0.2 (0.2..0.5)** | −0.3 (−1.4..3.0) |
  | Firefox, cold | −2.0 (−2.0..0.0) | −1.0 (−1.0..0.0) | 0.0 (0.0..1.0) | −4.0 (−6.0..0.0) |
  | Firefox, warm | **−1.0 (−2.0..−1.0)** | **−1.0 (−1.0..−1.0)** | 0.0 (−1.0..2.0) | **−8.0 (−12.0..−6.0)** |
  | Slow 4G, cold | **−4.5 (−5.7..−2.9)** | −1.4 (−3.7..0.8) | −0.6 (−1.8..0.8) | −1.0 (−5.0..25.2) |
  | Slow 4G, warm | **−3.3 (−4.6..−1.8)** | **−2.8 (−4.1..−1.4)** | **−4.4 (−7.2..−2.3)** | −6.6 (−23.5..11.2) |

  and less Svelte's:

  | | swap | remove | clear | every press |
  |---|---|---|---|---|
  | Chromium, cold | +0.1 (−0.3..0.4) | −0.2 (−0.4..0.5) | −0.1 (−0.6..0.1) | **+1.1 (0.2..5.5)** |
  | Chromium, warm | 0.0 (−0.2..0.1) | 0.0 (−0.2..0.5) | +0.2 (0.0..0.3) | **−3.2 (−4.3..−1.0)** |
  | Firefox, cold | 0.0 (−1.0..1.0) | 0.0 (0.0..1.0) | **−2.0 (−3.0..−1.0)** | 0.0 (−2.0..3.0) |
  | Firefox, warm | 0.0 (−1.0..1.0) | 0.0 (−1.0..1.0) | 0.0 (−1.0..1.0) | 0.0 (−5.0..2.0) |
  | Slow 4G, cold | **−1.3 (−2.5..−0.6)** | −0.6 (−1.6..0.5) | −0.2 (−0.3..0.5) | **+17.8 (7.8..23.7)** |
  | Slow 4G, warm | −0.1 (−1.2..0.8) | 0.0 (−1.2..5.2) | **−1.7 (−3.4..−1.3)** | −6.2 (−16.3..13.0) |

  Of the other presses, past their spread: against train-33, Firefox's warm
  update −4.0 (−5.0..−3.0) and replace −2.0 (−3.0..−1.0), and the cold create
  under Slow 4G +12.3 (2.3..16.1); against Svelte, the presses where it was
  ahead before this step: Chromium's cold append +4.1 (3.5..6.0) (ADR-0437's
  mark-compact), Firefox's update +2.0 (1.0..3.0) cold and select +2.0 cold
  and warm, and under Slow 4G the cold create +2.6 (0.7..10.4) and append
  +16.7 (13.0..29.4), the warm update +4.0, select +0.7 and replace +13.9
  (0.7..19.6). **A second look, 21 rounds a cell**: under Slow 4G cold
  (`t34/ab-more-slow-cold.txt`), this less train-33 and less Svelte, the swap
  **−4.8 (−6.0..−3.5)** and **−2.3 (−4.4..−1.5)** (train-33's was behind
  Svelte's, +1.9 (0.5..4.4)), the remove **−2.7 (−5.0..−0.3)** and −4.5
  (−6.1..0.7), the clear −1.0 (−2.3..0.2) and +0.5 (−1.4..1.1), the create
  −5.0 (−12.9..5.5) and −3.4 (−13.2..2.4), the append +4.8 (−2.4..8.5) and
  **+22.9 (7.0..28.5)** (train-33's +17.6 (8.4..27.8)), every press −20.2
  (−29.1..4.1) and +2.0 (−16.9..18.6); against `bf4bde68`
  (`t34/ab-create-slow.txt`) the create −0.3 (−7.2..6.6) and the swap −0.8
  (−2.1..0.4); Chromium warm (`t34/ab-more-chromium-warm.txt`),
  the clear +0.2 (−0.1..0.2) against train-33 and −0.2 (−0.2..0.0) against
  Svelte, the swap +0.2 (−0.1..0.4) and 0.0 (−0.2..0.5), the remove −0.1
  (−0.3..0.4) and 0.0 (−0.2..0.2), every press −0.1 (−2.9..3.3) and −2.7
  (−7.0..0.3).
- **Each press split** (`t34/press_split.mjs`, `t34/press-split-final.txt`,
  21 rounds a cell, each click at a point of the frame interval drawn for its
  round): from the click to the change (its script), the style and layout it
  asks for, forced, the wait to the frame's callbacks, and from them to a
  task after the frame. This less Svelte, the script and the whole:

  | | swap | remove | clear |
  |---|---|---|---|
  | Firefox, cold | −0.04 (−0.10..0.04), +0.04 (−0.32..0.92) | **+0.22 (0.14..0.28)**, +0.12 (0.00..1.10) | −0.38 (−0.54..0.04), −0.18 (−0.48..0.34) |
  | Firefox, warm | 0.00 (−0.06..0.14), −0.04 (−0.40..0.24) | **+0.14 (0.10..0.22)**, +0.14 (−0.82..0.42) | −0.08 (−0.32..0.18), −0.12 (−0.58..0.48) |
  | Chromium, cold | **−0.10 (−0.20..−0.10)**, 0.00 (−0.30..0.30) | **−0.30 (−0.40..−0.20)**, −0.10 (−0.60..0.50) | −0.10 (−0.50..0.00), +0.10 (−0.30..1.90) |
  | Chromium, warm | **−0.10 (−0.10..−0.10)**, 0.00 (−1.10..0.60) | −0.10 (−0.30..0.00), 0.00 (−1.40..0.50) | **−0.60 (−1.70..−0.30)**, −0.60 (−1.90..0.50) |
  | Chromium, cold, slowed four times | −0.50 (−1.00..0.00), −0.60 (−1.80..0.90) | **−1.80 (−2.10..−1.20)**, −1.90 (−3.00..0.50) | −0.60 (−1.20..0.20), −0.50 (−1.20..0.50) |

  The style and layout, the wait and the paint of these presses are
  Svelte's to their spreads in every cell. Against train-33 the swap's
  script is 1.62 ms less in Firefox cold, 1.14 warm, 0.90 and 0.60 in
  Chromium, 3.80 slowed; the remove's 0.90, 1.08, 0.40, 0.50 and 1.60; the
  clear's the same. Of the other presses' script, against train-33: the
  update −0.90 and −0.20 in Firefox, −0.30 and −0.40 in Chromium; the
  create +0.46 (0.06..0.82) in Firefox cold and −2.42 warm; the append
  +0.20 (0.10..0.60) in Chromium cold and +1.60 (0.20..4.30) slowed, −1.90
  in Firefox warm; the replace **+1.48 (1.06..2.18) in Firefox cold**, +0.42
  (−0.02..0.88) warm, +0.10 (0.00..0.60) in Chromium cold (Consequences).
- **The few rows' placing and the skips, each alone** (`t34/press-split-few.txt`,
  `93396c91` against `bf4bde68`, 21 rounds): the swap's script −0.20 ms
  (−0.40..−0.06) in Firefox cold, −0.16 (−0.34..−0.10) warm, −0.20
  (−0.30..−0.10) in Chromium cold; the remove's −0.02 (−0.10..0.14), −0.04
  (−0.18..0.04) and −0.10 (−0.30..0.00), the skips' (Why not). What else the
  swap and the remove spend in Firefox (`t34/press-split-exp.txt`, variants of
  `bf4bde68`): the handler's check of each `Int` it writes, left out, −0.14
  (−0.20..−0.04) and −0.12 (−0.22..−0.06) cold, −0.10 and −0.12 warm; `same`
  depth-first, −0.04 and 0.00. Rows a keyed array pushed in order rather
  than written at their places into one made with its length
  (`t34/press-split-pushed.txt`): nothing in any press.
- **`moveBefore` against `insertBefore`** (`t34/move_forms2.mjs`,
  `t34/move-forms.txt`, two rows of a thousand swapped): the style and
  layout 2.30 ms against 2.40 in Chromium, 9.70 against 10.50 slowed four
  times, 2.32 against 2.34 in Firefox; to the frame the same.
- **Bytes** (`t34/bytes.sh`, `t34/bytes-final.txt`, each build packed for
  production as the head-to-head's, its document as its host sends it;
  Brotli 11):

  | packed | train-33 (`e5baa112`) | this, less that |
  |---|---|---|
  | the large list's document | 49,435 (16,563) | +2,032 (+863) |
  | the large list's page alone (its document's script) | 44,773 (15,636) | **+2,032 (+769)** |
  | the large list's device runtime | 40,148 (14,132) | +2,074 (+875) |
  | TodoMVC's document | 61,980 (19,066) | **byte for byte** |
  | TodoMVC's page alone | 44,892 (15,325) | **byte for byte** |
  | TodoMVC's device runtime | 39,382 (13,668) | **byte for byte** |
  | a server's runtime, either app | 66,711 (21,971) | **byte for byte** |

  Of the large list's page alone, `bf4bde68`'s was 46,180 (16,257): the few
  rows' placing and what the keep loop says of them are 625 B of it (148
  with Brotli). The general placing kept beside the found rows' would add
  1,041 B (418 with Brotli, `t34/placing_bytes.mjs`,
  `t34/placing-bytes-final.txt`).
- **Held**: the agreement (`renderer-agreement.test.mjs`, 29): the page's
  own code held to the general code and the server's render after render,
  each list's very items reordered as presses do (swapped, gone, added,
  moved, made again or changed, written twice, reversed, shuffled, rotated,
  cleared, replaced), and what it says it found held to the rows' keys:
  17,586 lists found among their last render's rows, 1,692 out of order,
  1,028 of them a few out of their places, 2,002 with rows gone, 1,873 with
  rows made again, 7,519 with rows kept by their items; and 6,772 lists of
  the large list's kind, what they compute moving and not. The packaging
  (`optimize-assets.test.mjs`, 49): each list's `reads`, the general placing
  cut from a build every list of which is its own and kept where one is the
  general code's. The contract (`renderer-contract.test.mjs`, 10): a longest
  run over 1,000 seeded lists of places, the rows found by their keys over
  600, and `fewMoved` over 6,000 lists with two to four rows out of their
  places held to a longest run (2,402 keeping a row out of its place, 272
  moving one in its place), reading no more than a few of a thousand rows'
  places for a swap. The rows suite (`e2e/rows.spec.mjs`) on the builds as
  served and built to report, three engines, 81 and 60 tests: the fixture's
  reordered list (`examples/rows/events.pw`, its table rows and its list
  items its own code's, a third the general code's) walked by 80 seeded
  presses, each row the element it was; each press found by the own code
  (`window.__pw.found`); each moving the fewest rows; the list made short,
  each press placed among the few out of their places (`window.__pw.few`);
  the large list's swap moving two, placed so; a moved row holding focus
  keeping it; the general placing's log said alike. The controls, run whole,
  three slots each (`t34/evidence.sh`, `t34/evidence.txt`):
  CONTROLS
  Fallow's new-only audit against master passes, the ceilings of the
  functions this step raised or added at their scores plus one.

## Consequences

- A list's own code finds a row where it was by its item: a swap, a remove,
  an update or a select computes no key, makes no map, and places its rows by
  two numbers a row (its place, its key's taking), where train-33 made two
  maps of a thousand keys and four passes to place them.
- **Swap, remove and clear against Svelte.** To the frame, no cell has
  Svelte's swap, remove or clear ahead past its spread, in any engine, cold
  or warm, Slow 4G included; the swap under Slow 4G cold is quicker past it,
  and the clear in Firefox cold and Slow 4G warm. Each press's own work is
  Svelte's or less in Chromium, cold, warm and slowed, and in Firefox the
  swap's and the clear's; **Firefox's remove spends 0.22 ms of script more
  than Svelte's cold (0.14..0.28) and 0.14 warm (0.10..0.22)**, which its
  1 ms clock leaves within the frame (0.0 to the frame in both). Of it, the
  handler's check of each `Int` of the 999 rows it writes is 0.12 ms, rows it
  read as they are, each checked when it was written: the compiler's
  (ADR-0433), NEXT; the rest is the keep loop's pass over the 999 rows, each
  row's place written.
- **Trades, each recorded.** *Bytes*: the large list's page alone is 769 B
  heavier with Brotli than train-33's (+2,032 decoded), 148 of it the few
  rows' placing; a build none of whose lists is its own (TodoMVC's, every
  server's) is sent the bytes it was. *A number on every row*: each row a
  list's own code keeps holds its place at its render (`at`), written as the
  loop finds it. *What the loop says*: two arrays a render (`places`,
  `taken`) and, where a few rows are out of their places, the list of them.
  *The runtime as written*: 67,331 B with Brotli as the development host
  sends it, 2,143 more than train-33's, past ADR-0188's 64 KiB, now bounded
  at 72 KiB (decision 6).
  *Firefox's cold replace* spends 1.48 ms more script than train-33's
  (1.06..2.18), from this step's first commit on (`t34/press-split.txt`,
  `ef585c84`: +1.80), its warm one +0.42 (−0.02..0.88) and Chromium's +0.10:
  its keeping does no more than train-33's (a key, a lookup and a row made
  each), its record to the frame is not slower (−1.0, −2.0..2.0) and its
  script is 4.0 ms less than Svelte's; not traced (NEXT). *The append's
  script* in Chromium cold +0.20 ms (0.10..0.60), +1.60 (0.20..4.30) slowed,
  a key looked up and a row noted in the kept map for each new row, neither
  past its spread to the frame, and less than Svelte's. *The cold create
  under Slow 4G*, +12.3 ms (2.3..16.1) to the frame in eleven rounds, is
  −5.0 (−12.9..5.5) in 21 and +2.9 (−9.2..9.4) in 21 more, its script slowed
  four times +0.80 (−0.60..2.90): within the cell's spread.
- The presses Svelte was ahead in before this step are ahead still, and
  recorded: Chromium's cold append (ADR-0437), Firefox's update and select,
  and under Slow 4G the create, append, update, select and replace (NEXT).
- A row moved is put where it goes, nothing of it read, where the engine
  moves no node whole (WebKit) and what has focus is outside it: no row
  moved asks for the list's layout to read a scroll it does not hold. Not
  measured: the head-to-head visits Chromium and Firefox.
- A list shown again by a block, or kept by a render refused after it, is
  placed by its rows' keys, as the general placing placed it.
