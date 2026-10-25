# ADR-XXXX: the head-to-head: the same apps in Pleris and in the mainstream stacks, measured, losses first

Status: proposed by track `head-to-head` under the owner's delegation of
2026-10-02 and standing order of 2026-10-25 ("deciding by best judgement
without asking"), on the integrator's ruling of 2026-10-25 (build the
head-to-head as a reproducible instrument, not a one-off). Date: 2026-10-25.
Milestone: E14 (the proof roadmap; [ADR-0342](ADR-0342-the-claim-measured.md)
§4, the protocol this implements).

## Context

The owner, 2026-10-20: "best" is a belief until measured head to head. The
coordinator added: make the same feature change as a pull request in Pleris
and in a React/Next.js counterpart, and compare lines, files, languages
touched and what a reviewer must check by hand. ADR-0342 §4 wrote the
protocol (apps, metrics and instruments, fairness, losses) and asked for the
downloads it needed; nothing had been measured ("any head-to-head number,
none of which is measured yet").

The integrator's ruling of 2026-10-25 set the scope: TodoMVC (Pleris's
against TodoMVC's own React and Svelte apps, each held to the same behaviour
checks), then one data-backed slice (the store's cart) against ADR-0120's
Next.js and SvelteKit baselines; bytes, first render and interactive,
server cost under load, source size, the museum's bug classes, the same
feature change in each stack; production builds, the same machine and
shaping, warm caches stated, Pleris's losses reported first; a recipe,
mutation and integrity controls, and this ADR.

## Decision

### 1. The apps, and who wrote each

| app | stack | the code | written by |
|---|---|---|---|
| TodoMVC | Pleris | `examples/todomvc/app.pw`, built by `spikes/own-renderer/todomvc.sh`, served by the Pleris host's **release** build | this project (track `todomvc`, ADR-0367) |
| TodoMVC | React 19.2.5 | `benchmarks/headtohead/todomvc/react`, its committed webpack/Terser production build | TodoMVC's maintainers (tastejs/todomvc@ff43b02e) |
| TodoMVC | Svelte 5.55.5 | `benchmarks/headtohead/todomvc/svelte`, its committed Vite production build | TodoMVC's maintainers (tastejs/todomvc@ff43b02e) |
| the store | Pleris | `benchmarks/baselines/pleris` (ADR-0156), `dist-baseline`, the host's release build | this project |
| the store | Next.js 16.4.1 | `benchmarks/baselines/next-react`, `next build` + `next start` | this project (ADR-0120) |
| the store | SvelteKit 2.70.3 | `benchmarks/baselines/sveltekit`, `vite build` + adapter-node | this project (ADR-0120) |

TodoMVC's apps are never rewritten here (ADR-0342 §4): their builds are
measured as their repository commits them, each file held to its upstream
blob. Neither framework's TodoMVC has a server: any static host serves
them, so the head-to-head's own static host does (`static-host.mjs`), with one
policy for both that a production static host or CDN applies: Brotli 11 and
gzip 9 precompressed by `Accept-Encoding`, a strong `ETag` and `304`,
`immutable` for a file named by its content's hash and `no-cache` for any
other, keep-alive. Pleris's pages are served by its own host, the only one
it has, as it is.

### 2. The instrument (`benchmarks/headtohead/`), each metric with its instrument

| metric | instrument | rule |
|---|---|---|
| bytes, first load and each interaction | `measure.mjs`, Playwright 1.58.0 | every response's bytes decoded, what Brotli 11 and gzip 9 make of them (as a static host sends them, the same for every stack), and the bytes the browser received as each host sent them: Chromium's DevTools count per phase (headers, bodies, streams and aborted reads; a cache hit none); TodoMVC's common files (`base.js`, `learn.json`) shown and left out of the "framework and app" total for every stack alike |
| first render | the Paint Timing API's first contentful paint, in Chromium, Firefox and WebKit | from navigation start |
| interactive | each stack's own signal, observed by an init script: a client-rendered app when its field is in the document (React and Svelte attach handlers as they render); Pleris's `data-pw-ready`; Next.js's hydrated App Router writing `history.state.__NA`; SvelteKit's router starting after `_hydrate` (`history.scrollRestoration`) | checked every run: the interactions that follow must take effect in the same document |
| each interaction | the input event's own time stamp to the change in the document and to the frame after it | TodoMVC: add, complete, filter; the store: Add |
| cold and warm | cold: a new browser context; warm: the same context after one whole visit, its page storage cleared, cookies kept | stated beside every number |
| network shaping | Chromium alone (Playwright offers Firefox and WebKit none): Lighthouse's Slow 4G, 150 ms, 1.6 Mbit/s down, 750 kbit/s up, CPU x4 | the same profile for every stack in a block |
| runs | seven a cell, every stack visited once a round in an order shuffled from the seed and the round (printed) | median, quartiles, extremes; a time differs only where the quartiles do not overlap |
| server cost | `load.mjs`: an open fixed load, 100 requests/s for 10 s after 50 warm-up requests, of the page's document (the store with 20 sessions' cookies); latency from when each was due | CPU (user + system) of the server's process tree per request and resident memory before, at peak and after, from /proc |
| open pages | 20 pages of the app in one Chromium, loaded and left alone for 10 s; then a press in the last | the requests they made, the server's CPU a second, the press's time to its change |
| source size | `scripts/head_to_head_loc.py` | non-blank, non-comment lines by each language's own comments; app, the code it needs in its server in another language, build and configuration apart; lockfiles and builds listed and not counted |
| the same feature change | `feature-change.mjs` over the benchmark's twelve tasks | each reference patch's lines, files and languages; each stack's own checker (`pw check`, `tsc --noEmit`, `svelte-check`) on the reference (must be clean) and on the task's unsafe version, applied in place and taken back with every file's time |
| the museum's classes | `museum-classes.mjs`, from `e14-museum`'s record | which exhibits' bugs each framework was shown to let ship; Pleris's verdict on each |
| behaviour | TodoMVC's official Cypress suite (the compatibility copy Pleris is held to, under each app's own name) and `behaviour.spec.mjs` (one test per `app-spec.md` behaviour) in three engines | judged against written expectations: each stack passes all but its listed gaps, each traced to its source line; anything else fails |

### 3. Fairness, and what is not equal

- Production builds only: Pleris's host built `--release`; `next build` and
  `vite build`; TodoMVC's committed production builds. Pleris's build does
  not minify and its host does not compress or let a cache keep its files:
  that is what Pleris ships, so it is measured, and reported as losses; no
  stack is tuned, and no stack is given what it does not have.
- The same machine, browsers, shaping, run count and order rule for every
  stack; the machine and its load average are printed with every run.
- Not equal, and stated: React's and Svelte's TodoMVC keep todos in memory
  only, Pleris's in `localStorage` (TodoMVC's specification asks for it);
  Pleris's store page keeps a live update stream, the frameworks' push
  nothing; React's and Svelte's servers are the instrument's static host (any
  static host would do), Pleris's is the framework's own.

### 4. Integrity

`validate.mjs` refuses to report a run that breaks any rule: a cell of fewer
than five runs or of a different count; a served file that is not the build
named (a vendored file not upstream's blob, a Pleris build of changed
sources, a framework build older than its sources or a page naming another
build); an unminified script or style from a stack whose build minifies; an
uncompressed answer of 1 KiB or more from a host that compresses; a shaped
run whose document came faster than the profile's round trip; an
interaction that loaded a new document or did not complete; a warm run that
downloaded its main script again from a host that lets a cache keep it.
`scripts/head_to_head_mutations.py` takes each rule out in turn, and each
piece of the instrument, its judges and its counter (29 mutants, each killed
by an assertion of its witness test), and runs four integrity controls on the
real instrument (a run count of one; React on an unminified print of its
build; Svelte on a build not upstream's; Pleris on a build older than its
source), each of which it must refuse by the rule that names it.

### 5. The recipe

`just e14-head-to-head` (`just/head-to-head.just`) writes
`docs/evidence/E14/head-to-head.txt`: the versions, then where Pleris loses
(every row of every table in which another stack does better, the measured
rows and the source, feature-change, behaviour and museum rows alike), where
it is best, each section whole, the measurement's tables, and the controls.
Ports: `PORT` + 220..249.

## Downloads made

- **TodoMVC's own React and Svelte apps**: <https://github.com/tastejs/todomvc>
  at `ff43b02e59dfa604386bb382034b2cd07c2bcd8a` (2026-05-03; `master` was still
  there on 2026-10-25, and it is the commit the Cypress suite was vendored
  from), `examples/react` (26 files, 2,011,587 bytes) and `examples/svelte`
  (17 files, 123,152 bytes), MIT (`license.md`), the stylesheets carrying
  `todomvc-app-css` (CC BY 4.0). Fetched by a blob-filtered sparse git fetch
  into an empty directory; vendored as `benchmarks/headtohead/todomvc`, 41
  files, every one its upstream blob (`upstream.json`), the React build's two
  source maps (1,375,884 bytes, read only by developer tools) left out and
  recorded. Approved by the owner's standing order of 2026-10-25, after
  ADR-0342's request.
- **No package**: the instrument runs on `@playwright/test` 1.58.0 and
  `esbuild` 0.28.2, both already in the lockfile (the harness's and the
  SvelteKit baseline's); `benchmarks/headtohead` joins the pnpm workspace with
  those two (nine lockfile lines).

## Results

## Findings

## What is claimed, and what is not

## Next

## Questions for the integrator
