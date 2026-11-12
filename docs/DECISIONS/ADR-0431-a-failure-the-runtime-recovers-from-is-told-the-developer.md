# ADR-0431: A failure the runtime recovers from is told the developer

Status: accepted 2026-11-11, to land with train-26. Proposed 2026-11-11, on
branch `prod/train-26-20261111`, under the owner's delegation of 2026-10-02
and order of 2026-10-25 ("deciding by best judgement without asking"), and the
order of 2026-11-09 that there be no reason to use anything else. Milestone:
E14. Builds on ADR-0302, ADR-0382, ADR-0400 and ADR-0430.

## Context

The runtime catches what fails so that the page goes on. A press whose
handler throws, whose module does not load, or whose command the host
answers with an error is told the reader where it was pressed ("This did not
work. Try again.", ADR-0302). A boot that fails leaves the page to be read.
The page's one module that does not load leaves each press its own module;
a speculation's, the press sent unspeculated (ADR-0382). A rollback that
cannot render, a soft navigation that cannot be finished in place, a next
runtime that does not load: each recovers.

- **Each was told no developer.** The runtime wrote each to its log, which no
  console shows, and which a production page no longer holds (ADR-0430). A
  developer whose handler threw saw nothing: not in the console, and not in
  an error monitor, which hears the window's errors.
- **A Svelte page's handler that throws is an error nothing caught**: the
  console shows it, with its stack, and the window's `error` event carries it
  to whatever listens.
- **One failure the browser told already**: computed values that do not load
  are thrown on, and nothing catches them.

## Decision

1. **Told as the platform tells an error nothing caught**: `reportError`,
   the HTML standard's, which gives the console the error and its stack and
   the window's `error` event and `onerror` the error itself, in each engine
   the suites run (checked on a page with an origin: Chromium, Firefox and
   WebKit). At seven places:
   - a press that failed, for anything but a refusal or no answer;
   - the page's one module that did not load, once, since a module that
     failed is not asked for again;
   - a speculation's module that did not load, the press sent unspeculated;
   - a rollback that could not render, beside the press's own failure;
   - a navigation that could not be finished in place, and a next runtime
     that did not load, each before the page is read again;
   - a boot that failed, the page left to be read.
2. **A condition the runtime names is the reader's alone**: a refusal, the
   host's answer (`refused:<predicate>`); no answer, the network's
   (`unreachable`); a store that is full, the device's (`device-full`). Each
   is told the reader by name, and is no failure of the program. Nor is an
   abort the runtime made, or a module warmed before it is needed: where that
   module is needed and does not load, its press fails, and is told.
3. **What the browser tells already is told once**: computed values that do
   not load are thrown on, as before.
4. **In every build.** It is what the page does, not what it reports to its
   tests (ADR-0430).

## Why not the other ways

- **`console.error` with the runtime's own words**: words in every page, and
  an error monitor hears no console. `reportError` gives both the error
  itself, as an uncaught one.
- **Throwing it on**: the page would stop where it recovers now.
- **Telling the reader's conditions too**: a refusal is an answer, and a
  monitor that counts every one hides what failed.
- **A code and a page for each failure, as a refusal has** (ADR-0400): the
  error already says what failed, in the program's words or the platform's;
  a refusal's code names what the decision found, which no error carries.

## Result

Seven calls, each as written: TodoMVC's one script 46,138 bytes, 15,403 with
Brotli, from 46,090 and 15,360; the large list's 43,808 and 14,962, from
43,760 and 14,950; a server's page's runtime, which holds all seven, 66,638
and 21,904, from 66,522 and 21,811.

The head-to-head at `87446c10`, with train-25, from `afb99fe3`, train-25
alone: TodoMVC's first load as sent 19,975, from 19,908 (Svelte's 20,821),
the row Pleris's as it was; decoded 63,226, from 63,178 (57,354); the large
list's 16,539 and 48,447, from 16,507 and 48,399 (13,242 and 35,039); the
store's 27,468 as sent, from 27,375 (SvelteKit's 35,023). Pleris is behind
in 52 rows and best in 86, from 56 and 90: ten timing rows crossed, seven of
them toward Pleris, none of them a failure's.

Each place is held by a test of the spec that drives its failure, in three
engines, on the demos and the feed as developed and on TodoMVC as served for
a boot: the developer is told once where the press failed, and not where a
refusal or no answer was the reader's.

## Consequences

- Tests: `lazy-handler.spec.mjs`, `argument-invariants.spec.mjs`,
  `identity.spec.mjs` (a refusal and no answer told no developer),
  `computed-modules.spec.mjs`, `page-bundle.spec.mjs`,
  `press-modules.spec.mjs`, `feed.spec.mjs` (a rollback and its press, two),
  `soft-navigation.spec.mjs` (two new) and `device-runtime.spec.mjs` (a
  boot that fails, new). Controls: ten in
  `scripts/failures_told_mutations.py`; `just e14-failures-told` records
  both. One anchor of `scripts/press_modules_mutations.py` names the
  speculation's load with its report.
- A spec that hears the page's errors (`rows`, `nested-blocks`,
  `kiokun-site`, `wit-unique`) now hears a failure the runtime recovers
  from too.
