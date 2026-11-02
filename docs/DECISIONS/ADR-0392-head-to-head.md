# ADR-0392: the head-to-head: the same apps in Pleris and in the mainstream stacks, measured, losses first

Status: accepted, 2026-11-02. Proposed by track `head-to-head` on
2026-10-25 under the owner's delegation of 2026-10-02 and standing order of
2026-10-25 ("deciding by best judgement without asking"), on the
integrator's ruling of 2026-10-25 (build the head-to-head as a reproducible
instrument, not a one-off); reviewed by a separate session, whose
corrections changed the instrument and every number below ("The
independent review"), and landed. Milestone: E14 (the proof roadmap;
[ADR-0342](ADR-0342-the-claim-measured.md) §4, the protocol this
implements).

## Where Pleris loses

From the evidence of record (`docs/evidence/E14/head-to-head.txt`, which
lists 61 rows where Pleris loses before any other), first and plainly:

- **It sends too many bytes, and sends them again.** TodoMVC's first load
  receives 237,218 bytes in Pleris, React's 70,085 and Svelte's 20,821: the
  build does not minify (`pw-runtime.mjs` is 157,269 bytes; esbuild makes it
  56,873) and the host compresses nothing (Brotli would send 67,828, now
  more than React's 67,161). A warm visit downloads it all again (237,127
  bytes; React 1,338, Svelte 759): the host lets no cache keep a file. The
  store's first load is 219,870 bytes (Next.js 140,211, SvelteKit 35,020),
  its warm load 219,832 (2,898; 2,499).
- **Its first press downloads its renderer.** TodoMVC's first add fetches
  395,444 bytes (`pw-render.wasm`, 386,022 of them, uncompressed); React and
  Svelte fetch nothing. Under Slow 4G that add takes 3,889 ms to its frame
  (React 44, Svelte 40).
- **It is interactive last under Slow 4G**: TodoMVC at 4,101 ms (React
  1,711, Svelte 1,307), the store at 3,501 (Next.js 2,096, SvelteKit
  1,752), and warm 4,081 and 3,462 (the frameworks' 1,204 and 1,169; 723
  and 671).
- **Every interaction is slower, in every engine, on the loopback too**:
  TodoMVC's add reaches its frame in 32 ms in Chromium (React 11, Svelte
  9.9), 31 in Firefox (9.0, 8.0) and 113 in WebKit (25, 22); its toggle a
  frame behind React's in each.
- **Open tabs and idle visitors cost it.** With 19 other tabs of the store
  open in one browser, a press takes 10,084 ms to its change (Next.js 54,
  SvelteKit 46): each tab holds a live stream, and a browser opens six
  connections to a host. Each idle store page asks its server again every
  two seconds, and twenty idle visitors cost the host 16.0 ms of CPU a
  second (the Node servers' 2.00).
- **Its store's data layer is Rust**: 294 lines of Pleris in 7 files, and
  the 2,110 lines of `store.rs` in the server they call, against Next.js's
  151 lines and SvelteKit's 134. TodoMVC in Pleris needs an application
  server, where React's and Svelte's are static files any host serves (and
  a plain Node static host answers faster: 0.42 ms at p50 to Pleris's
  1.18).
- **Smaller losses**: 7 requests on TodoMVC's first load (5); the store's
  Add fetches 4,314 bytes (2,390; 701); two of the twelve changes are larger
  in Pleris (T03, 43 lines, Next.js 25, SvelteKit 29; T11, 23, SvelteKit
  22); TodoMVC's build and configuration lines (76; Svelte's 44); three of
  the twelve unsafe changes pass its checker (T02, T07, T09).

Where it is best: TodoMVC's behaviour (60 of 60 specification checks in
three engines, 29 of 29 of TodoMVC's own Cypress suite; React 51 and 28,
Svelte 48 and 28); the twelve changes (140 lines in 13 files and one
language, against 323 in 20 and 235 in 19, two languages each), its checker
catching 9 of their 12 unsafe versions, the frameworks' none; the museum
(it lets none of the five exhibits ship; Next.js was shown letting three,
SvelteKit two); TodoMVC's source (189 lines, one file, one language); the
store server's cost (0.32 ms of CPU a request and 45.2 MiB at its peak under
load, against Next.js's 7.28 ms and 148 MiB and SvelteKit's 1.68 ms and
107 MiB); and
some first and largest paints, since its server renders the page.

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
| TodoMVC | Pleris | `examples/todomvc/app.pw`, built by `spikes/own-renderer/todomvc.sh` (its Wasm in the `browser` profile: release, `opt-level = "z"`, LTO, stripped), served by the Pleris host's **release** build | this project (track `todomvc`, ADR-0367) |
| TodoMVC | React 19.2.5 | `benchmarks/headtohead/todomvc/react`, its committed webpack/Terser production build (the bundle says `version:"19.2.5"`) | TodoMVC's maintainers (tastejs/todomvc@ff43b02e) |
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
other, keep-alive. That host **stands in** for any static host: its cost is
no stack's, so Pleris's host doing better than it is never a win, and doing
worse is a loss all the same (`report.mjs`). Pleris's pages are served by
its own host, the only one it has, as it is.

### 2. The instrument (`benchmarks/headtohead/`), each metric with its instrument

| metric | instrument | rule |
|---|---|---|
| bytes, first load and each interaction | `measure.mjs`, Playwright 1.58.0 | every response's bytes decoded, what Brotli 11 and gzip 9 make of them (as a static host sends them, the same for every stack), and the bytes the browser received as each host sent them: Chromium's DevTools count per phase (headers, bodies, streams and aborted reads; a cache hit none); TodoMVC's common files (`base.js`, `learn.json`) shown and left out of the "framework and app" total for every stack alike |
| paint | the Paint Timing API's first contentful paint, in Chromium, Firefox and WebKit; the largest contentful paint and the layout shift where an engine reports them | from navigation start; an engine that does not report one is "not reported", never 0; a layout shift differs by a hundredth |
| interactive | each stack's own signal, observed by an init script: a client-rendered app when its field is in the document (React and Svelte attach handlers as they render); Pleris's `data-pw-ready`; Next.js's hydrated App Router writing `history.state.__NA`; SvelteKit's router starting after `_hydrate` (`history.scrollRestoration`) | checked every run: the interactions that follow must take effect in the same document |
| each interaction | the input event's own time stamp to the change in the document and to the frame after it | TodoMVC: add, complete, filter; the store: Add |
| cold and warm | cold: a new browser context; warm: the same context after one whole visit, its page storage cleared, cookies kept | stated beside every number |
| network shaping | Chromium alone (Playwright offers Firefox and WebKit none): **Lighthouse's Slow 4G as Lighthouse applies it through the DevTools protocol**: 562.5 ms a request (its 150 ms round trip x 3.75), 1.6 Mbit/s x 0.9 down, 750 kbit/s x 0.9 up, CPU x4 | the same profile for every stack in a block; the report prints the profile the run sent |
| runs | seven a page cell, every stack visited once a round in an order shuffled from the seed and the round (printed), after the machine settles (up to 120 s for its one-minute load to fall under a quarter of its CPUs); five rounds of each server measurement, shuffled the same way | median, quartiles, extremes; a time differs only where the quartiles are a millisecond apart, the engines' timer resolution; a server's number only where they are apart by what its instrument tells apart (the CPU clock's tick over a run's requests, a hundredth of a millisecond of Node's clock, a request over an idle window, a press's 10 ms polling) |
| server cost | `load.mjs`, five rounds: an open fixed load, 100 requests/s for 10 s after 50 warm-up requests, of the page's document (the store with 20 sessions' cookies); latency from when each was due | CPU (user + system) of the server's process tree per request and resident memory at peak, from /proc |
| idle visitors | 20 pages of the app, **each in a browser context of its own** (as separate people's browsers), loaded and left alone for 10 s, five rounds | the requests they made a page a minute, the server's CPU a second |
| a press with tabs open | 20 tabs of the app in **one** browser context (one person's: its six connections to a host over HTTP/1.1), left 4 s, then a press in the last, five rounds | the press's time to its change |
| source size | `scripts/head_to_head_loc.py` | non-blank, non-comment lines by each language's own comments; app (for the Pleris store, the modules its pages import), the code it needs in its server in another language, build and configuration apart; lockfiles, builds and modules no page imports listed and not counted |
| the same feature change | `feature-change.mjs` over the benchmark's twelve tasks | each reference patch's lines, files and languages; each stack's own checker (`pw check`, `tsc --noEmit`, `svelte-check`) on the reference (must be clean) and on the task's unsafe version, applied in place and taken back with every file's time; a refusal, or a warning the unsafe version draws and its reference does not, is the checker's catch |
| the museum's classes | `museum-classes.mjs`, from `e14-museum`'s record | which exhibits' bugs each framework was shown to let ship; Pleris's verdict on each |
| behaviour | TodoMVC's official Cypress suite (the compatibility copy Pleris is held to, under each app's own name) and `behaviour.spec.mjs` (one test per `app-spec.md` behaviour) in three engines | judged against written expectations: each stack passes all but its listed gaps, each traced to its source line; anything else fails |

### 3. Fairness, and what is not equal

- Production builds only: Pleris's host built `--release` and its Wasm in
  the `browser` profile; `next build` and `vite build`; TodoMVC's committed
  production builds. Pleris's build does not minify and its host does not
  compress or let a cache keep its files: that is what Pleris ships, so it
  is measured, and reported as losses; no stack is tuned, and no stack is
  given what it does not have.
- The same machine, browsers, shaping, run counts and order rule for every
  stack, the servers' measurements included; the machine, its load average
  and the profile sent are printed with every run.
- Not equal, and stated: React's and Svelte's TodoMVC keep todos in memory
  only, Pleris's in `localStorage` (TodoMVC's specification asks for it);
  Pleris's store page keeps a live update stream, the frameworks' push
  nothing; React's and Svelte's servers are the instrument's stand-in
  static host, Pleris's is the framework's own.
- ADR-0342 §4's protocol, where this departs from it: its "event" entries
  (Event Timing, INP's source) are not read: the instrument times each
  interaction from its input event to the frame after its change, in all
  three engines, which Event Timing is not; its accessibility row
  (Playwright's snapshot and role assertions, axe-core on the owner's word)
  is not measured; its store "defects" (ADR-0120's contract and the tasks'
  hidden tests per stack) are cited per task from the harness's evidence,
  not counted here.

### 4. Integrity

`validate.mjs` refuses to report a run that breaks any rule: a cell of fewer
than five runs or of a different count, the servers' measurements alike; a
served file that is not the build named (a vendored file not upstream's
blob, a Pleris build of changed sources, a framework build older than its
sources or a page naming another build); an unminified script or style from
a stack whose build minifies; an uncompressed answer of 1 KiB or more from a
host that compresses; a shaped run whose document came faster than the
profile's round trip; an interaction that loaded a new document or did not
complete; a warm run that downloaded its main script again from a host that
lets a cache keep it (judged in Chromium, where a cache's answer can be
told: correction 4); a fixed load with an unanswered request; a failed page
load, fixed load, idle window or press, each kept in the report as the
failure it was. `scripts/head_to_head_mutations.py` takes each rule out in
turn, and each piece of the instrument, its judges and its counter (41
mutants, 11 of them the review's, each killed by an assertion of its witness
test), and runs four integrity controls on the real instrument (a run count
of one; React on an unminified print of its build; Svelte on a build not
upstream's; Pleris on a build older than its source), each of which it must
refuse by the rule that names it.

### 5. The recipe, recorded on the development machine

`just e14-head-to-head` (`just/head-to-head.just`) writes
`docs/evidence/E14/head-to-head.txt`: the versions and its `host:`, then
where Pleris loses (every row of every table in which another stack does
better, the measured rows and the source, feature-change, behaviour and
museum rows alike), where it is best, each section whole, the measurement's
tables, and the controls. Ports: `PORT` + 220..249. It is a measurement of
the machine it runs on, so it is `LOCAL_ONLY` (`scripts/ci_plan.py`;
ADR-0281's decision 5): recorded on the development machine, never fetched
from a runner, whose two shared cores would measure their own contention.
The instrument's 29 node tests and 18 Python tests run in `just ci`
(`scripts/tests/test_head_to_head.py`).

## Downloads made

- **TodoMVC's own React and Svelte apps**: <https://github.com/tastejs/todomvc>
  at `ff43b02e59dfa604386bb382034b2cd07c2bcd8a` (2026-05-03; `master` was still
  there on 2026-10-25, and it is the commit the Cypress suite was vendored
  from), `examples/react` (26 files, 2,011,587 bytes) and `examples/svelte`
  (17 files, 123,152 bytes), MIT (`license.md`), the stylesheets carrying
  `todomvc-app-css` (CC BY 4.0). Fetched by a blob-filtered sparse git fetch
  into an empty directory; vendored as `benchmarks/headtohead/todomvc`, 41
  files and the licence, every one its upstream blob (`upstream.json`), the
  React build's two source maps (1,375,884 bytes, read only by developer
  tools) left out and recorded. Approved by the owner's standing order of
  2026-10-25, after ADR-0342's request.
- **No package**: the instrument runs on `@playwright/test` 1.58.0 and
  `esbuild` 0.28.2, both already in the lockfile (the harness's and the
  SvelteKit baseline's); `benchmarks/headtohead` joins the pnpm workspace with
  those two (nine lockfile lines). The review downloaded nothing; it read
  Lighthouse's and DevTools' published source (correction 8).

## Results

`PORT=13441 just e14-head-to-head` at `dbd37a8c` (master's `27c1ffba`
merged), 2026-11-02, its measurement 06:05-06:45Z, on the PC: an Intel Core
i9-13900K, 32 logical CPUs, 31.2 GiB, Ubuntu 22.04.3 on WSL2 (kernel
6.18.54.1), its `host:` line naming `docs/environment/pc-wsl.md`. Other
sessions were working: the instrument waited its 120 s for the one-minute
load average to fall under 8 (a quarter of the CPUs) and measured from 10.2
(it had been 21.2, the recipe's own builds and another session's test
binary among it); it was 10.46 at the end (five-minute 12.96). Chromium
145.0.7632.6, Firefox 146.0.1, WebKit 26.0; seven runs a page cell, five
rounds of each server measurement; every rule held; 61 rows where Pleris
loses, 32 where it is best, 58 neither. The controls: 41 of 41 mutants
killed; 4 of 4 integrity controls caught.

A second record at the same commit (12:05-12:44Z, the load reaching 26 as
other sessions' test suites ran) held every rule too and agreed on every
row but six near their spreads' edges: in it, the store's first and
largest paint in Chromium cold and TodoMVC's in Firefox warm were within
the spread, not wins; the store's interactive in Chromium warm within the
spread, not a loss; and TodoMVC's first paint in WebKit cold a loss, and its
filter's loss in WebKit warm rather than cold. The first, under the lower
load, is the record; those rows are the instrument's least stable. The
track's four records before the review (`d6fdb217` to `acd3f4c8`) were made
by the instrument the review corrected: their bytes and unshaped times are
of the same order, their Slow 4G and server rows are not comparable.

Times are medians [first-third quartiles] in milliseconds from navigation
start; servers' numbers medians [quartiles] of five rounds; bytes are
Chromium's, which every engine's decoded bodies equal. A row marked * is the
instrument's stand-in static host, not React's or Svelte's cost.

**TodoMVC** (Chromium's bytes; times by engine and condition):

| metric | Pleris | React | Svelte |
|---|---|---|---|
| first load: bytes received, as each host sent them | 237,218 | 70,085 | 20,821 |
| first load: bytes as a static host sends them (Brotli 11), app only | 67,828 | 67,161 | 16,992 |
| first load: requests | 7 | 5 | 5 |
| "add": bytes received | 395,444 | 0 | 0 |
| "toggle": bytes received | 1,723 | 0 | 0 |
| warm load: bytes received | 237,127 | 1,338 | 759 |
| first contentful paint (chromium, cold) | 36 [32-42] | 40 [32-42] | 36 [34-38] |
| largest contentful paint (chromium, cold) | 36 [32-42] | 68 [60-74] | 36 [36-40] |
| interactive (chromium, cold) | 40 [37-48] | 49 [47-62] | 23 [21-30] |
| "add" to its frame (chromium, cold) | 32 [27-34] | 11 [9.4-11] | 9.9 [8.5-11] |
| "toggle" to its frame (chromium, cold) | 31 [30-31] | 14 [13-14] | 14 [13-14] |
| first contentful paint (chromium, cold, slow-4g) | 1,248 [1,246-1,254] | 1,204 [1,204-1,224] | 1,204 [1,202-1,210] |
| largest contentful paint (chromium, cold, slow-4g) | 1,248 [1,246-1,254] | 1,720 [1,700-1,760] | 1,324 [1,314-1,326] |
| interactive (chromium, cold, slow-4g) | 4,101 [4,099-4,112] | 1,711 [1,689-1,748] | 1,307 [1,302-1,313] |
| "add" to its frame (chromium, cold, slow-4g) | 3,889 [3,875-3,905] | 44 [36-53] | 40 [38-42] |
| "toggle" to its frame (chromium, cold, slow-4g) | 611 [603-625] | 28 [22-34] | 26 [21-29] |
| interactive (chromium, warm, slow-4g) | 4,081 [4,070-4,085] | 1,204 [1,203-1,216] | 1,169 [1,169-1,172] |
| "add" to its frame (chromium, warm, slow-4g) | 3,834 [3,829-3,841] | 17 [16-20] | 16 [13-19] |
| first contentful paint (firefox, cold) | 27 [25-35] | 56 [50-73] | 64 [58-65] |
| interactive (firefox, cold) | 71 [69-78] | 69 [63-91] | 53 [50-57] |
| "add" to its frame (firefox, cold) | 31 [30-38] | 9.0 [8.5-10] | 8.0 [7.0-9.5] |
| first contentful paint (webkit, cold) | 35 [30-39] | 36 [34-41] | 28 [25-31] |
| largest contentful paint (webkit, cold) | 171 [163-175] | 223 [218-235] | 211 [202-222] |
| interactive (webkit, cold) | 68 [64-73] | 56 [51-59] | 38 [35-43] |
| "add" to its frame (webkit, cold) | 113 [108-129] | 25 [22-26] | 22 [21-23] |
| layout shift (chromium, cold; Firefox and WebKit report none) | 0.000 | 0.006 | 0.000 [0.000-0.003] |
| server CPU per document request, ms | 0.19 [0.17-0.19] | 0.39 [0.37-0.45]* | 0.38 [0.38-0.46]* |
| server resident memory at peak under load, MiB | 30.5 | 70.9 [69.3-71.3]* | 70.7 [69.2-70.9]* |
| latency under 100 requests/s, p50 / p99, ms | 1.18 / 2.87 | 0.42 / 1.97* | 0.40 / 1.81* |
| 20 idle visitors: requests a page a minute; server CPU ms/s | 0.00; 0.00 | 0.00; 1.00* | 0.00; 1.00* |
| a press with 19 other tabs open in one browser, ms to its change | 75 [67-76] | 34 [33-42] | 32 [30-47] |

**The store** (ADR-0120's three stores, one contract):

| metric | Pleris | Next.js | SvelteKit |
|---|---|---|---|
| first load: bytes received, as each host sent them | 219,870 | 140,211 | 35,020 |
| first load: bytes as a static host sends them (Brotli 11) | 63,981 | 117,109 | 30,454 |
| first load: requests | 4 | 6 | 9 |
| "add": bytes received | 4,314 | 2,390 | 701 |
| warm load: bytes received | 219,832 | 2,898 | 2,499 |
| first contentful paint (chromium, cold) | 32 [28-32] | 52 [38-54] | 36 [34-38] |
| interactive (chromium, cold) | 45 [42-54] | 95 [84-104] | 46 [43-47] |
| "add" to its frame (chromium, cold) | 28 [28-29] | 29 [28-30] | 30 [28-30] |
| interactive (chromium, warm) | 30 [29-31] | 49 [36-57] | 23 [19-27] |
| first contentful paint (chromium, cold, slow-4g) | 616 [612-618] | 624 [624-630] | 620 [616-628] |
| interactive (chromium, cold, slow-4g) | 3,501 [3,471-3,505] | 2,096 [2,090-2,201] | 1,752 [1,749-1,759] |
| "add" to its frame (chromium, cold, slow-4g) | 1,168 [1,167-1,180] | 643 [635-651] | 1,200 [1,186-1,207] |
| interactive (chromium, warm, slow-4g) | 3,462 [3,459-3,469] | 723 [700-747] | 671 [657-687] |
| "add" to its frame (chromium, warm, slow-4g) | 1,167 [1,166-1,173] | 620 [619-623] | 1,184 [1,178-1,185] |
| first contentful paint (firefox, cold) | 35 [31-42] | 51 [43-60] | 38 [33-44] |
| interactive (firefox, cold) | 82 [76-101] | 126 [115-169] | 115 [87-128] |
| "add" to its frame (firefox, cold) | 34 [33-41] | 34 [33-40] | 33 [33-35] |
| first contentful paint (webkit, cold) | 17 [14-18] | 22 [21-29] | 17 [15-23] |
| interactive (webkit, cold) | 55 [50-64] | 108 [84-116] | 60 [46-68] |
| interactive (webkit, warm) | 41 [39-47] | 34 [31-39] | 34 [32-35] |
| "add" to its frame (webkit, cold) | 23 [18-47] | 28 [27-41] | 28 [26-28] |
| server CPU per document request, ms | 0.32 [0.26-0.41] | 7.28 [7.16-7.99] | 1.68 [1.58-1.94] |
| server resident memory at peak under load, MiB | 45.2 [40.4-49.9] | 148 [148-151] | 107 [107-109] |
| latency under 100 requests/s, p50, ms | 1.56 [1.48-1.79] | 5.40 [4.87-5.95] | 1.43 [1.35-1.46] |
| latency under 100 requests/s, p99, ms | 3.31 [3.29-3.50] | 25.8 [25.7-27.6] | 3.26 [3.21-3.54] |
| 20 idle visitors: requests a page a minute | 30.0 [29.7-30.0] | 0.00 | 0.00 |
| 20 idle visitors: server CPU, ms a second | 16.0 [15.0-20.0] | 2.00 [2.00-3.00] | 2.00 [2.00-2.00] |
| a press with 19 other tabs open in one browser, ms to its change | 10,084 [10,034-10,258] | 54 [51-56] | 46 [45-47] |

Behaviour, source, the same change and the museum:

| | Pleris | React / Next.js | Svelte / SvelteKit |
|---|---|---|---|
| TodoMVC's specification, three engines | **60/60** | 51/60 (3 gaps) | 48/60 (4 gaps) |
| TodoMVC's official Cypress suite | **29/29** | 28/29 (no persistence) | 28/29 (no persistence) |
| TodoMVC source: lines, files, languages | **189, 1, 1** | 278, 11, 4 | 201, 8, 4 |
| TodoMVC build and configuration lines | 76 | 145 | **44** |
| store source: lines, files, languages | 294 + 2,110 of Rust in the server, 7, 2 | 151, 6, 2 | **134**, 6, 3 |
| store build and configuration lines | **9** | 65 | 44 |
| the twelve changes: lines, files, languages | **140, 13, 1** | 323, 20, 2 | 235, 19, 2 |
| unsafe versions the checker refuses or warns of | **9/12** | 0/12 | 0/12 |
| the museum's exhibits let ship | **0/5** | 3/5 shown | 2/5 shown |

## Findings

Corrections first: the instrument's own, each caught by its rules or its
controls before a number was reported (1-7, the track's; their numbers are
from its development runs, not the record), then the independent review's
(8-16), each fixed with a test and a mutant the test kills.

1. **Chromium's shaping holds a response's body, not its first byte.** Under
   Slow 4G the document's `responseStart` stayed at ~1 ms while its
   `responseEnd` was ~157 ms; the first shaping rule (first byte at least the
   round trip) refused every shaped run. The rule reads the document's last
   byte.
2. **SvelteKit writes its router state before it hydrates.** The first
   "interactive" signal for SvelteKit (`history.state["sveltekit:history"]`)
   is written in `start()` before `_hydrate`, which would have flattered it;
   read in `@sveltejs/kit` 2.70.3's `client.js`. The signal is its router
   starting, after hydration (`_start_router` sets
   `history.scrollRestoration`).
3. **WebKit keeps no Secure cookie from `http://127.0.0.1`, and SvelteKit
   sets its session cookie Secure on any host but `localhost`** (its
   `cookie.js`: `secure: url.hostname === 'localhost' && url.protocol ===
   'http:' ? false : true`). On 127.0.0.1 the SvelteKit store kept no cart
   in WebKit and its Add never moved the count; every stack is measured at
   `localhost` since. ADR-0120's contract runs in Chromium alone, which
   accepts the cookie, so it never saw this.
4. **Byte counts of cached responses**: Playwright reports a cached
   response's sizes as negative in Chromium, and as its whole original size in
   Firefox and WebKit (a probe: the static host saw no request for Svelte's
   immutable script on a warm visit, while those two engines reported 14,802
   and 16,368 bytes received); Chromium's `responseReceived` carries a cached
   response's original size too. Bytes received are Chromium's DevTools
   protocol's count, a cache's answer counting 0, and the warm rule is held
   in Chromium alone: the dry run refused 35 Firefox and WebKit warm runs
   for "downloading again" what they had not.
5. **The first minified rule misjudged Next.js's Turbopack chunks** (a
   bytes-a-line heuristic fooled by CSS in template literals); the rule is
   now what esbuild's minifier still takes off a file: under 2% for every
   framework's output, 20-64% for files no minifier touched.
6. **The mutation controls found a test gap**: no test had a higher median
   whose quartiles overlap, so "a higher median is a loss" survived; the
   test has it now, and the mutant is killed. And a run called three stores'
   Add, each 31 ms at the median, a win for Pleris by a fraction of a
   millisecond, below Firefox's and WebKit's timer resolution: a time now
   differs only where the quartiles are at least a millisecond apart.
7. **The dry run's evidence said "+ uncommitted changes"**: master's
   `Cargo.lock` at `cc47a64b` was not in cargo's own order, so the unlocked
   `cargo run` in `todomvc.sh` rewrote it and every recipe building through
   such a script recorded its commit as changed. This branch committed
   cargo's order (`0a4ae365`); master's lock is in it since D2's merge
   (`4345726a`).
8. **Slow 4G cost a request a quarter of its round trip.** The profile sent
   Lighthouse's packet-level figures (150 ms, 1.6 Mbit/s, 750 kbit/s) to
   `Network.emulateNetworkConditions`, which delays each request; for that
   throttle Lighthouse itself sends `requestLatencyMs`, 562.5 ms (150 x
   3.75), and the throughputs x 0.9 (Lantern's `mobileSlow4G` in
   devtools-frontend's `front_end/models/trace/lantern/simulation/Constants.ts`;
   Lighthouse's `enableNetworkThrottling` in `core/lib/emulation.js`; read
   2026-11-02). Each request cost a quarter of the round trip the profile
   names, which flattered most the stack making the most requests in
   sequence, Pleris: its first add under Slow 4G was 2,402 ms in the track's
   record, 3,889 in this one. The report also printed "latency 150 ms" from
   a constant of its own; it prints the profile the run sent now.
9. **The servers' numbers were single runs.** One fixed load, one idle
   window and one press a stack, in a fixed order, judged by value: the
   store's p50 latency was a "win" by 0.1 ms (1.1 against 1.2) and
   TodoMVC's press a loss from one sample. Each is five rounds in shuffled
   orders now, a median with its quartiles, judged by the spread rule at
   what its instrument tells apart, and held to a rule of its own
   (`validateServer`).
10. **Twenty idle pages shared one browser's six connections.** No more than
    six of their streams could be held at once, so the "8.7 requests a page
    a minute" and "2.0 ms of CPU a second" were the six connections'
    ceiling (29 requests in 10 s: six streams of two seconds each), not
    twenty visitors'. Each idle visitor has a browser context of its own
    now: 30.0 requests a page a minute and 16.0 ms of CPU a second. One
    person's tabs sharing their connections is the press's question,
    measured apart.
11. **TodoMVC's server rows "won" against the instrument's own static
    host.** React's and Svelte's TodoMVCs are static files; the Node host
    that serves them here stands in for any static host or CDN, and its
    0.3 ms and 88 MiB were not React's or Svelte's cost. Pleris's host doing
    better than the stand-in is no win now; doing worse is still a loss.
12. **The store's Pleris lines counted fourteen modules no page imports**
    (the corpus's fixtures: a legacy widget's, a vendor SDK's, telemetry's,
    maps', payments' and receipts' declarations, and others): 503 lines in
    21 files, where the store's pages import 294 in 7, which check clean
    alone.
13. **Claims not in the evidence.** The ADR's 0.21, 0.33 and 0.28 ms, 0.25,
    7.43 and 1.77 ms of CPU and 142.3 and 10.8 ms at p99 were finer than the
    evidence printed (0.2, 0.3, 0.3; 0.3, 7.4, 1.8; 142, 11); finding 11's
    "55 ms with one page, 1,855 ms with eight" was a probe nothing recorded;
    "one task is larger in Pleris: T03" missed T11 (23 lines against
    SvelteKit's 22); the store's source, a loss, was reported among the
    wins; and the loopback was called a place where Pleris's bytes cost
    little while each of TodoMVC's interactions lost there in every engine.
    The report prints servers' numbers to the precision this ADR cites, and
    this ADR cites nothing its evidence does not print.
14. **Largest contentful paint and layout shift were recorded and not
    reported**, and an engine that reports no layout shift would have read 0
    (Firefox and WebKit report none). Both are reported now, an engine's
    silence as "not reported", a layout shift judged at a hundredth.
15. **The checkers' warnings were not read**: an unsafe change `svelte-check`
    warned of would have counted as accepted. A warning an unsafe change
    draws and its reference does not counts as the checker's catch now;
    none of the twelve draws one.
16. **The recipe is a measurement of its machine** (ADR-0281's decision 5):
    planned on CI, a runner's two shared cores would have measured their own
    contention, and its file would have replaced the development machine's
    this ADR reads. It is `LOCAL_ONLY` now, with a `host:` line; its
    instrument's tests run in `just ci`.

Then the framework's, each a finding for the coordinator to route, losses
first:

17. **Pleris's first interaction downloads its renderer.** In TodoMVC the
    first add fetches `pw-render.wasm` (386,022 bytes, sent uncompressed),
    two handler modules, the page's computed module and an image: 395,444
    bytes where React and Svelte fetch nothing. Under Slow 4G the first add
    takes 3,889 ms to its frame (React 44, Svelte 40), and the first toggle,
    which fetches a handler module, 611 (28, 26). *Route*: the runtime
    (preload the renderer once the page is interactive, serve it
    compressed, or render a keyed row without it).
18. **Pleris's host compresses nothing, keeps no connection and lets no
    cache keep a build file** (`write_response`: `connection: close`;
    `serve_file`: no `ETag`, `Last-Modified` or `Cache-Control`). Its first
    load is sent whole (237,218 bytes for TodoMVC, where Brotli would send
    67,828), a warm load downloads everything again (237,127; the store's
    219,832 against 2,898 and 2,499), and under Slow 4G it is interactive
    last: TodoMVC at 4,101 ms (1,711; 1,307), the store at 3,501 (2,096;
    1,752), warm at 4,081 and 3,462. *Route*: a production host
    (precompressed Brotli, content-hashed immutable names or validators,
    keep-alive or HTTP/2): the charter's deployment story, which has none.
19. **The runtime is not minified**: `pw-runtime.mjs` is 157,269 bytes;
    esbuild 0.28.2's minifier makes it 56,873 (64% less). It grew 3,217
    bytes on `master` since the track's record, and TodoMVC's Brotli bytes
    are now above React's (67,828; 67,161). ADR-0188 left minifying to the
    owner; the owner's standing order of 2026-10-25 delegates it. *Route*: a
    minify step in the build (esbuild is in the workspace).
20. **With several tabs of one Pleris app open, a press waits for a held
    connection.** Each store page holds its update stream for two seconds
    at a time; over HTTP/1.1 a browser opens six connections to a host, so
    with more tabs than that, the next request waits for a stream to end.
    With 19 other tabs open the last tab's Add took 10,084 ms [10,034-10,258]
    to change its count (Next.js 54, SvelteKit 46). *Route*: one
    subscription per origin shared by its tabs (a `BroadcastChannel` or
    `SharedWorker` leader), or HTTP/2 in the host.
21. **An idle page costs its server.** The stream is answered by a thread
    that wakes every 25 ms for two seconds under the server's `pending` lock
    (`stream_open`, 80 passes), then the page asks again: each idle store
    page makes 30.0 requests a minute, and twenty idle visitors cost the
    host 16.0 ms of CPU a second [15.0-20.0] (Next.js's and SvelteKit's 2.00,
    their runtimes' own timers). (The frameworks' pages also push nothing:
    the live update is a feature Pleris has and they do not; its cost is
    reported, not its value.) *Route*: wake held streams on a write, not on
    a timer.
22. **Every interaction is slower, in every engine, on the loopback too.**
    TodoMVC's add reaches its frame in 32 ms [27-34] in Chromium (React 11,
    Svelte 9.9), 31 in Firefox (9.0, 8.0) and 113 in WebKit (25, 22), and
    warm, its renderer fetched again from the loopback, 27, 29 and 82; its
    toggle and filter land a frame behind React's (Chromium 31 against 14).
    The store's Add, a round trip in every stack, is within the spread.
    *Route*: find where each interaction's time goes (a handler module
    fetched at its first use; the renderer's compilation, in WebKit four
    times the others' whole add) before choosing a repair.
23. **The store's data layer is Rust, not Pleris.** Its operations are
    `host "store:data/..."` declarations, implemented in the development
    server's `store.rs` (2,110 lines of code, shared by every store example
    and task, counted whole); the Next.js and SvelteKit stores keep theirs
    in 50 lines of TypeScript each. A feature that needs new server data
    (the brief's "favorite stores") cannot be written in Pleris alone today:
    its pull request would touch Rust in the server. *Route*: D2 (ADR-0338's
    declared tables): its read path landed (#53); a page served with its
    tables, and their writes, are what the store needs next.
24. **TodoMVC in Pleris needs an application server for every visit**: its
    document is rendered per request, while its todos are the device's;
    the frameworks' TodoMVCs are static files any host serves, and the
    instrument's plain Node static host already answers them faster (p50
    0.42 and 0.40 ms against 1.18, p99 1.97 and 1.81 against 2.87).
    *Route*: prerender a page whose reads are all the device's at build
    time.
25. **The checker leaves three of the twelve changes to a reviewer**: T02
    (asks of a slow source not shared), T07 (an answer for a category the
    customer moved past shown) and T09 (a kept answer older than the
    freshness asked for) check clean in Pleris as in the frameworks; each is
    caught by its hidden test only. *Route*: rules for shared reads, stale
    answers and freshness bounds.
26. **The rest of the losses**: TodoMVC's first load makes 7 requests
    (three stylesheets, because the page's policy refuses the official
    sheet's `data:` images and `todomvc-styles.mjs` moves them to files;
    React and Svelte 5); the store's Add fetches 4,314 bytes (a handler and
    a speculation module before its command; Next.js 2,390, SvelteKit 701);
    T03's change is 43 lines (Next.js 25, SvelteKit 29) and T11's 23
    (SvelteKit 22); TodoMVC's build and configuration are 76 lines (Svelte
    44); the store is interactive later than SvelteKit warm in Chromium (30
    against 23) and than both in WebKit warm (41 against 34); SvelteKit's
    p50 under load is below Pleris's (1.43 against 1.56). *Route*: one
    stylesheet per page from the build; the rest are the first items' or
    the store's own.

And where Pleris is best, in the same terms:

27. **Behaviour**: TodoMVC in Pleris passes all 60 of the specification's
    checks in the three engines and 29 of 29 of TodoMVC's own Cypress suite;
    React 51 of 60 and 28 of 29, Svelte 48 of 60 and 28 of 29. React's gaps:
    Escape does not leave an edit (`input.jsx`), the counter reads "1 item
    left!" with no `<strong>` (`footer.jsx`), no persistence. Svelte's: an
    edit is saved untrimmed and an edit of spaces is kept (`Item.svelte`),
    no persistence, a page opened at `#/completed` ignores it
    (`router.js`). Both apps are the official ones, as TodoMVC ships them.
28. **The same change**: over the twelve tasks, Pleris's pull requests
    change 140 lines in 13 files, one language; Next.js's 323 lines in 20
    files, two (TSX, TypeScript); SvelteKit's 235 in 19, two (Svelte,
    TypeScript). Pleris's checker refuses 9 of the 12 unsafe versions
    (loading and error states, authorization and privacy, the new order
    state, accessibility, retries); the frameworks' checkers refuse none,
    and warn of none.
29. **The museum**: Pleris refuses all five exhibits; Next.js was shown
    letting three ship (G01, H07, J01) and SvelteKit two (F01, J08).
30. **The store server's cost**: 0.32 ms of CPU a request and 45.2 MiB at
    its peak under load, against Next.js's 7.28 ms and 148 MiB and
    SvelteKit's 1.68 ms and 107 MiB; at p99 under the load 3.31 ms against
    Next.js's 25.8 (SvelteKit's 3.26 within the spread).
31. **Source**: TodoMVC in Pleris is 189 lines in one file and one language
    (persistence included), React's 278 in 11 files and four languages,
    Svelte's 201 in 8 and four; the store's build and configuration are 9
    lines (65; 44).
32. **Paint, where the server renders**: TodoMVC's first paint in Firefox
    is 27 ms cold (56; 64); its largest contentful paint is earlier than
    React's in Chromium (36 against 68) and than both under Slow 4G (1,248;
    1,720, 1,324) and in WebKit (171; 223, 211); the store's first paint in
    Chromium cold is 32 (52; 36) with the fewest requests (4; 6, 9). Server
    rendering paints early; what Pleris then sends and runs is what loses
    (findings 17-22). (The second record had four of these paint wins
    within the spread.)

## The independent review, 2026-11-02

A session separate from the track's author (AGENTS.md's fourth rule) read
the instrument, its rules, the evidence and this ADR against each other for
fairness and truth: each stack production-built and idiomatic; the same
machine, shaping, cache state and run counts on every side; medians with
spread; no metric computed on the wrong build; the downloads recorded; every
claim traceable to the evidence; the losses first and plain. Its findings
were hypotheses until reproduced: each of corrections 8-16 was, by a primary
source, the code or a run, and each is fixed with a test and a mutant the
test kills. It then merged `master` (`27c1ffba`), numbered this ADR, and
recorded the evidence again, twice, at `dbd37a8c` (Results).

Confirmed, and unchanged:

- Each stack's build is its production build: React's bundle names
  `version:"19.2.5"`; Pleris's Wasm is the `browser` profile (release,
  `opt-level = "z"`, LTO, stripped); the frameworks' stores are built by
  the recipe before they are served, and refused when older than a source.
- The tasks' setups are symmetric: each stack's setup brings its baseline
  to the task's start (the Pleris setups take out a feature its baseline
  had, the frameworks' add the data sources theirs lacked), and no setup is
  counted for any stack.
- `svelte-check` warns of none of the twelve unsafe versions (compared by
  hand with each reference's warnings before the instrument read them, and
  by the instrument since); the Next.js baseline configures no linter, so
  its checker is `tsc`.
- The behaviour gaps are upstream's, at the lines cited; the spec's twenty
  tests are `app-spec.md`'s behaviours.
- The downloads' record: the source, commit, licences, sizes and every
  vendored file's blob (`upstream.json`, 41 app files and the licence; the
  two source maps left out, 1,375,884 bytes).

## What is claimed, and what is not

- Claimed: on the machine and at the commit the evidence names, the numbers
  its tables print, each from a recorded command, each run held to
  `validate.mjs`'s rules; the behaviour each stack's TodoMVC shows to the
  same checks; each stack's checker's verdict on the twelve tasks' changes;
  the museum's record as `e14-museum` made it.
- Not claimed: field performance (these are lab numbers on a loopback, and
  Chromium's shaping is a model); that two apps sample the apps people
  build; that the twelve tasks or the five exhibits sample the changes and
  bugs people make: this project wrote both, around the classes of mistake
  Pleris's checker was designed for, and three of the twelve are classes it
  misses; that a stack the museum did not reproduce an exhibit in is safe
  from it; that a linter a team adds (ESLint with `jsx-a11y`, not in the
  baselines) would catch none of them; anything about accessibility, which
  is not measured; that the instrument's static host is any particular CDN,
  or that its cost is React's or Svelte's; anything about Pleris behind a
  production server, which it does not have.

## Next

Pleris's work first, in the order the losses cost a user, each measured
again by `just e14-head-to-head`, which says whether it moved (NEXT's
"head-to-head" section holds them as items):

1. The first interaction's renderer (finding 17): preload or ship
   `pw-render.wasm` compressed before the first press, or render a keyed row
   without it. The first add under Slow 4G should fall from seconds to the
   tens of milliseconds the frameworks take.
2. A production host (finding 18): precompressed Brotli, content-hashed
   immutable names or validators, keep-alive or HTTP/2. Warm loads should
   receive kilobytes, not the whole page.
3. Minify the runtime in the build (finding 19).
4. One live subscription per origin, shared by its tabs, or HTTP/2
   (finding 20), and streams woken by writes, not a 25 ms timer (finding 21).
5. Where each interaction's time goes on the loopback (finding 22).
6. D2's declared tables served to pages, read and written (finding 23);
   then a thirteenth task, "favorite stores" (a per-customer list the store
   page shows and a command changes: authorization, invalidation, loading
   and error states and types across the boundary in one change), the
   first whose Pleris pull request would need Rust today, added to
   `benchmarks/tasks` and so to this comparison.
7. Checker rules for shared reads, stale answers and freshness bounds
   (finding 25).
8. Prerender device-only pages (finding 24); one stylesheet per page
   (finding 26).
9. The instrument: accessibility (Playwright's snapshot and roles now,
   axe-core when the owner approves its download); Event Timing beside its
   own interaction times; ADR-0120's contract in all three engines
   (correction 3 would have been caught there); the feed's timeline as a
   third app; a machine kept quiet for it.

## Questions for the integrator, and the answers

Answered at the landing (2026-11-02), under the owner's delegation:

1. *`benchmarks/headtohead` joins the pnpm workspace with two packages the
   lockfile already holds (`@playwright/test` 1.58.0, `esbuild` 0.28.2),
   nine lockfile lines: acceptable?* Yes: no new package and no download;
   in the workspace, `just audit` scans it like every other package.
2. *A verify shard of its own for a 25-minute recipe, or nightly only?*
   Neither: it is a measurement of the machine, so it is `LOCAL_ONLY`
   (ADR-0281's decision 5), recorded on the development machine in a quiet
   window and never fetched from a runner; its instrument's tests run in
   `just ci`. It is recorded again when a loss it names is worked on.
