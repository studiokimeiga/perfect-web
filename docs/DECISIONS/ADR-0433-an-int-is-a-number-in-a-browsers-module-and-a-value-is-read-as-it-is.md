# ADR-0433: An Int is a number in a browser's module, as on its wire, and a value is read as it is

Status: accepted 2026-11-11, to land with train-28. Proposed 2026-11-11, on
branch `perf/train-28-20261111`, under the owner's delegation of 2026-10-02
and order of 2026-10-25 ("deciding by best judgement without asking"), and the
order of 2026-11-09 that every row Svelte wins be a defect until it is fixed
or recorded. Milestone: E14. Supersedes ADR-0044's rule that an `Int` is a
`BigInt` in a JavaScript module; ADR-0039's semantics, ADR-0058's and
ADR-0130's wire are unchanged. Builds on ADR-0432.

## Context

A signal holds its value in the form JSON carries it (ADR-0130): an `Int` a
number, exact within ±2^53, a record an object by its field names, a case its
name and its payload. A module the compiler writes for the browser, a handler
or a page's computed values, held an `Int` as a `BigInt` (ADR-0044), and so
read every value it was given into its own form and wrote every value it gave
back into the wire's. Each was a copy of the whole value. The large list's
"update every 10th row", compiled:

```js
let s = n.get("rows").map((e) => ((o) => ({ id: BigInt(o.id), label: o.label }))(e)).map(...);
n.set("rows", s.map((e) => ((o) => ({ id: v(o.id), label: o.label }))(e)));
```

a thousand records made to read the list and a thousand more to write it, so
"append 1,000 rows" made three thousand. Every row the handler left alone came
back a new object, so the runtime, which keeps a row whose item is the same
(`holdsStill`, ADR-0424), compared each one field by field (`same`: the keys of
both, a pair for each) where `===` would have ended it. And every `+`, `-` and
`*` was a `BigInt`'s and a 64-bit check (`BigInt.asIntN`), every `%` a
`BigInt`'s, every length a `BigInt` made from a number: an allocation each.

The head-to-head makes each press once on a page just loaded (ADR-0432's
consequences), in code V8 has not yet optimized, where an operation on a
`BigInt` is a call into the engine and an operation on two small numbers is
not.

## Decision

1. **An `Int` is a number within ±2^53 of zero, and a `BigInt` past it**, 64
   bits at most as before: one form for each value, so that two equal ones are
   `===`, and for every `Int` the wire carries, the wire's own form. No `Int`
   is -0.
2. **Each operation keeps that form, and is exact.** On two numbers, `+`, `-`
   and `*` are a number's where the result is a safe integer (the nearest
   double to a true result past 2^53 - 1 is at least 2^53, so the test tells
   them apart), and made again as a `BigInt`, checked into 64 bits and given
   back in its form, otherwise (`add`, `sub`, `mul`, `big`). `/` on two
   numbers is `(a - a % b) / b`, a multiple of `b` no larger than `a` divided
   by `b`, so exact, then Euclidean as before; `%` is a number's `%`, then
   Euclidean (`div`, `rem`). A comparison is JavaScript's, which compares a
   number and a `BigInt` by value. A literal is a number where it can be; a
   length, a place in a list and a code point are numbers.
3. **A value the page holds is read as it is**: where a type's form in a
   module is its form on the wire (every type but one that holds `Unit` or a
   type that contains itself), a signal's value, a capture, an event and a
   computed value's inputs are given to the module themselves. Nothing is
   copied, and a row a handler leaves alone is the very object it read.
4. **A value given to the wire is given as it is, its `Int`s checked**: a
   loop over what holds an `Int`, `exact` on each, which traps on a `BigInt`,
   one past ±2^53, as before. Nothing is made.
5. **A command's answer is read case by case**, as before: it comes from the
   server as the call returns, and a case its type does not have traps where
   it arrives rather than being carried on.
6. **The differential test reads a result by the component's answer**
   (`pw-conformance/tests/javascript.rs`): an `Int` where the component
   answered one is a number within ±2^53 and not -0, or a `BigInt` past it,
   and any other form disagrees, so the test holds the form as well as the
   value.

## Why not the other ways

- **A `BigInt` always, and no copy where nothing changed**: a module could
  give back the value it read where it changed nothing, but it reads every
  `Int` into a `BigInt` to compute with it, so a row it reads is a copy
  whatever it does; and a `BigInt`'s operations are what a press on a page
  just loaded spends.
- **A number always, past 2^53 an error**: an `Int` is 64 bits (ADR-0039),
  and a product of two `Int`s within ±2^53 can be one past it, which the
  component computes exactly.
- **The check written as one walk over any value** (no `BigInt` anywhere in
  it): one helper for every type, but it reads each record's keys, an array
  for each, which is the allocation a row this decision removes.
- **Each operation written inline**, `(r = a + b, safe(r) ? r : slow(a, b))`:
  JavaScript throws on `+` of a number and a `BigInt`, so the operands' kinds
  are tested first either way, and a helper says it once.

## Result

The large list's handlers, compiled: "update every 10th row" reads the rows
as they are and gives back those it left alone, and "create 1,000 rows"
computes each label in numbers:

```js
let n = t.get("next_id"), s = add(n, 1e3), r = range(n, s).map((i) => { let _ = mul(i, 16807), z = rem(_, 2147483647), …
t.set("rows", ((i) => { for (let q of i) exact(q.id); return i; })(r));
```

The two builds of the large list, train-27's (`b3f0beed`) and this
decision's, packed for production and visited in turn by the head-to-head's
own visit (`measureOnce`), eleven rounds a cell on 2026-11-11, each press
timed from its input event to the frame after its change; this decision's
less train-27's, the median of each round's pair (quartiles):

| | every press of a visit | "create 1,000 rows" | "append 1,000 rows" |
|---|---|---|---|
| Chromium, cold | −9.0 ms (−9.6..−5.1) | 2.4 (−2.5..3.1) | −7.9 (−9.1..−6.9) |
| Chromium, cold, Slow 4G | −40.2 ms (−71.3..−21.8) | −1.8 (−15.6..7.6) | −35.6 (−47.4..−30.7) |
| Chromium, warm | 1.0 ms (−10.5..2.8) | −3.1 (−4.3..−2.7) | 4.7 (−4.1..6.9) |
| Chromium, warm, Slow 4G | −3.4 ms (−35.7..7.8) | −11.8 (−14.0..−6.0) | 26.5 (−17.9..35.9) |
| Firefox, cold | 3.0 ms (−3.0..3.0) | 0.0 (−2.0..2.0) | 0.0 (0.0..2.0) |
| Firefox, warm | −6.0 ms (−8.0..−1.0) | −3.0 (−5.0..−1.0) | 1.0 (0.0..2.0) |

Warm under Slow 4G, "update every 10th row" was 2.5 ms quicker (−4.8..−0.7),
"replace all rows" 5.5 (−10.9..−4.4) and "clear all rows" 1.7 (−2.3..−0.5);
in Firefox, cold, "update every 10th row" 1.0 (−2.0..−1.0). A warm append's
quartiles hold zero where its median is slower (below).

Trains 27 and 28 together, against `c0bc24fe`, the same way: every press of
a cold visit 9.5 ms quicker in Chromium (−14.4..−5.0) and 61.6 under Slow 4G
(−69.7..−50.3), "append 1,000 rows" 8.7 (−11.3..−3.9) and 38.6 (−45.0..−32.9);
in Firefox, warm, 4.0 (−8.0..1.0); a warm visit in Chromium 2.1 ms slower
(−5.4..5.0) and 25.6 under Slow 4G (−23.2..43.2), where a warm append's frame
came 3.7 ms later (3.1..10.8) and 38.3 under Slow 4G (30.9..55.4). Its rows
reach the document sooner in every round, its frame later in some: nine
rounds more of a warm append under Slow 4G, 29.6 to 33.9 ms from the click to
the document (the base's 34.4 to 40.7), and to the frame 148 to 224 (the
base's 155 to 188), the whole visit 12.3 ms quicker (−32.8..−7.8). In
Chromium's own trace of five rounds of each (warm, the CPU slowed four times
from the load on, without the network shaped), this build's append drew its
first frame at 154 to 158 ms to the base's 160 to 187, with no collection
over half a millisecond in it, to a scavenge of 3 to 5 ms in each of the
base's. What delays the frame in the slow rounds is not found (NEXT).

The handler bundle of the large list is 6,068 bytes, minified, from 6,194,
and its document, as its host sends it, 16,030 bytes of Brotli to 16,053 and
48,636 decoded to 48,521: the helpers that compute both ways are larger than
the copies they replace, and compress less well than copies that repeat.
TodoMVC's handlers, a decoder and an encoder for each list they read or
write, are its bundle's 7,432 bytes minified to 5,976 (36 `BigInt`
conversions to 2), and its document 19,198 bytes of Brotli to 19,059, and
63,396 decoded to 61,940.

The differential test, each `Int` held to its form: 97 queries and 19,400
calls (1,446 trapped in both), kiokun's shard rule's two queries and 400
calls, five queries over values held in place and five over trees, 1,000
calls each, and three over cases, 600: component and module agree. The
compiler's own tests, 1,885, four of them rewritten for what a module now
writes (the captured item read as it is, an `Int` literal a number, the
event sent as it is, a length a number); the compiled handlers and kiokun's
build regenerated (`just e10-handlers`, `just e10-kiokun`), kiokun in three
engines 21 of 21. Fourteen mutants re-anchored on the number form, two added
(`/` and `%` past 2^53, a way of their own now), and one taken out ("a
decoded Int stays a JSON number", now the design): run on this PC,
`e10-javascript` 19 of 19 killed, `e10-handlers-compute` 12 of 12, `e10-maps`
15 of 15, `e10-slices` 22 of 22 and `e14-optimistic` 5 of 5. "A value the page
holds is copied as it is read" stands for decision 3 and "an Int past 2^53 is
sent as it is" for decision 4. The fifteen other scripts with a mutant in the
backend mutate what this decision did not reach (a graph's nodes, a command's
arguments, a first value, a match), and run in the full verify.

The head-to-head of trains 27 and 28 is recorded at the commit their full
verify runs on, with that run (STATUS).

## Consequences

- A handler's or a computed value's work on a list is the work its body
  does: nothing is copied in or out, and the page finds each row it left
  alone by `===`.
- TodoMVC's handlers read and wrote their todos the same way (a decoder and
  an encoder for each of its lists, 36 `BigInt` conversions in its one
  bundle): each of its presses is a copy fewer each way.
- A value of a type that holds `Unit` or a type that contains itself is
  still read and written by its shape; a command's answer is read case by
  case.
- An `Int` past ±2^53 is a `BigInt`, exact, and never reaches the wire.
