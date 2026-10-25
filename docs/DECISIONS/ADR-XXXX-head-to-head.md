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
downloaded its main script again from a host that lets a cache keep it
(judged in Chromium, where a cache's answer can be told: correction 4); a
fixed load with an unanswered request; a failed page load, fixed load or
open-pages measurement, each kept in the report as the failure it was.
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

`PORT=13441 just e14-head-to-head`, 2026-10-25 (the whole recipe 18:21-18:46Z,
its measurement 18:28-18:46Z), at `d6fdb217`, on the PC: an Intel Core
i9-13900K, 32 logical CPUs, 31.2 GiB, Ubuntu 22.04.3 on WSL2 (kernel
6.18.54.1); the one-minute load average 30.3 when the measurement began (the
recipe's own builds and checks just before it, and other sessions' work) and
1.4 when it ended; Chromium 145.0.7632.6, Firefox 146.0.1, WebKit 26.0;
seven runs a cell; every rule held (`docs/evidence/E14/head-to-head.txt`,
which prints the 58 rows where Pleris loses and the 31 where it is best
before any table). The mutation and integrity controls: 29 of 29 mutants
killed; 4 of 4 integrity controls caught.

Times are medians [first-third quartiles] in milliseconds from navigation
start; bytes are Chromium's, which every engine's decoded bodies equal.

**TodoMVC** (Chromium's bytes; times by engine and condition):

| metric | Pleris | React | Svelte |
|---|---|---|---|
| first load: bytes received, as each host sent them | 234,001 | 70,085 | 20,821 |
| first load: bytes as a static host sends them (Brotli 11), app only | 67,051 | 67,161 | 16,992 |
| first load: requests | 7 | 5 | 5 |
| "add": bytes received | 395,444 | 0 | 0 |
| "toggle": bytes received | 1,723 | 0 | 0 |
| "filter": bytes received | 0 | 0 | 0 |
| warm load: bytes received | 233,910 | 1,338 | 759 |
| first contentful paint, ms (chromium, cold) | 48 [40-50] | 60 [48-68] | 52 [40-60] |
| interactive, ms (chromium, cold) | 58 [57-59] | 84 [77-124] | 41 [39-50] |
| "add" to its frame, ms (chromium, cold) | 43 [42-46] | 17 [13-21] | 14 [13-17] |
| first contentful paint, ms (chromium, cold, slow-4g) | 460 [448-470] | 392 [390-396] | 396 [392-406] |
| interactive, ms (chromium, cold, slow-4g) | 1,958 [1,946-1,966] | 921 [912-939] | 508 [498-532] |
| "add" to its frame, ms (chromium, cold, slow-4g) | 2,506 [2,489-2,510] | 66 [62-69] | 67 [65-71] |
| first contentful paint, ms (chromium, warm, slow-4g) | 420 [418-424] | 352 [348-354] | 208 [200-214] |
| interactive, ms (chromium, warm, slow-4g) | 1,901 [1,898-1,909] | 406 [401-413] | 358 [356-366] |
| "add" to its frame, ms (chromium, warm, slow-4g) | 2,414 [2,412-2,421] | 33 [31-35] | 25 [25-29] |
| first contentful paint, ms (firefox, cold) | 46 [40-56] | 87 [84-117] | 105 [92-111] |
| interactive, ms (firefox, cold) | 151 [123-167] | 109 [107-154] | 95 [91-100] |
| "add" to its frame, ms (firefox, cold) | 48 [47-53] | 15 [14-18] | 14 [14-15] |
| first contentful paint, ms (webkit, cold) | 73 [66-76] | 64 [55-67] | 40 [37-42] |
| interactive, ms (webkit, cold) | 125 [117-130] | 94 [84-104] | 57 [53-61] |
| "add" to its frame, ms (webkit, cold) | 179 [161-190] | 36 [33-42] | 35 [32-38] |
| server CPU per document request, ms | 0.27 | 0.61 | 0.61 |
| server resident memory at peak, MiB | 32 | 90 | 89 |
| latency under 100 requests/s, p50 / p99 ms | 2.0 / 3.7 | 0.9 / 4.4 | 0.9 / 2.0 |
| 20 idle pages: requests a page a minute; server CPU ms/s | 0.0; 0.0 | 0.0; 0.0 | 0.0; 0.0 |
| a press with 19 other pages open, ms to its change | 94 | 155 | 59 |

**The store** (ADR-0120's three stores, one contract):

| metric | Pleris | Next.js | SvelteKit |
|---|---|---|---|
| first load: bytes received, as each host sent them | 216,653 | 140,210 | 35,007 |
| first load: bytes as a static host sends them (Brotli 11), app only | 63,189 | 117,107 | 30,442 |
| first load: requests | 4 | 6 | 9 |
| "add": bytes received | 4,314 | 2,386 | 388 |
| warm load: bytes received | 216,615 | 2,897 | 2,500 |
| first contentful paint, ms (chromium, cold) | 40 [34-44] | 72 [60-92] | 52 [38-56] |
| interactive, ms (chromium, cold) | 66 [57-89] | 179 [140-207] | 76 [71-97] |
| "add" to its frame, ms (chromium, cold) | 28 [27-29] | 60 [43-61] | 33 [27-43] |
| first contentful paint, ms (chromium, cold, slow-4g) | 232 [230-242] | 224 [222-226] | 224 [212-228] |
| interactive, ms (chromium, cold, slow-4g) | 1,736 [1,731-1,744] | 1,307 [1,281-1,322] | 596 [579-613] |
| "add" to its frame, ms (chromium, cold, slow-4g) | 369 [368-371] | 259 [254-269] | 388 [380-395] |
| first contentful paint, ms (chromium, warm, slow-4g) | 204 [202-206] | 212 [204-212] | 212 [208-214] |
| interactive, ms (chromium, warm, slow-4g) | 1,705 [1,698-1,710] | 366 [352-376] | 295 [288-296] |
| "add" to its frame, ms (chromium, warm, slow-4g) | 360 [355-365] | 225 [224-226] | 369 [367-372] |
| first contentful paint, ms (firefox, cold) | 42 [39-45] | 53 [41-60] | 38 [36-42] |
| interactive, ms (firefox, cold) | 96 [95-103] | 140 [114-150] | 95 [91-100] |
| "add" to its frame, ms (firefox, cold) | 33 [33-34] | 39 [38-42] | 37 [35-44] |
| first contentful paint, ms (webkit, cold) | 20 [17-20] | 26 [26-34] | 20 [19-22] |
| interactive, ms (webkit, cold) | 66 [64-68] | 120 [113-123] | 64 [64-67] |
| "add" to its frame, ms (webkit, cold) | 24 [22-25] | 40 [40-50] | 29 [27-31] |
| server CPU per document request, ms | 0.33 | 9.69 | 2.24 |
| server resident memory at peak, MiB | 36 | 157 | 118 |
| latency under 100 requests/s, p50 / p99 ms | 2.1 / 3.9 | 7.9 / 27.3 | 2.2 / 4.1 |
| 20 idle pages: requests a page a minute; server CPU ms/s | 9.0; 10.0 | 0.0; 0.0 | 0.0; 0.0 |
| a press with 19 other pages open, ms to its change | 10,024 | 53 | 56 |

Behaviour, source, the same change and the museum:

| | Pleris | React / Next.js | Svelte / SvelteKit |
|---|---|---|---|
| TodoMVC's specification, three engines | **60/60** | 51/60 (3 gaps) | 48/60 (4 gaps) |
| TodoMVC's official Cypress suite | **29/29** | 28/29 (no persistence) | 28/29 (no persistence) |
| TodoMVC source: lines, files, languages | **189, 1, 1** | 278, 11, 4 | 201, 8, 4 |
| store source: lines, files, languages | 503 + 2,107 of Rust in the server, 21, 2 | **151, 6, 2** | 134, 6, 3 |
| the twelve changes: lines, files, languages | **140, 13, 1** | 323, 20, 2 | 235, 19, 2 |
| unsafe versions the checker refuses | **9/12** | 0/12 | 0/12 |
| the museum's exhibits let ship | **0/5** | 3/5 shown | 2/5 shown |

## Findings

Corrections first: the instrument's own, each caught by its rules or its
controls before a number was reported.

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
   protocol's count, a cache's answer counting 0 (a warm Next.js load
   received 2,897 bytes where the first count said ~140 KB), and the warm rule
   is held in Chromium alone: the dry run refused 35 Firefox and WebKit warm
   runs for "downloading again" what they had not.
5. **The first minified rule misjudged Next.js's Turbopack chunks** (a
   bytes-a-line heuristic fooled by CSS in template literals); the rule is
   now what esbuild's minifier still takes off a file: under 2% for every
   framework's output, 20-64% for files no minifier touched.
6. **The mutation controls found a test gap**: no test had a higher median
   whose quartiles overlap, so "a higher median is a loss" survived; the
   test has it now, and the mutant is killed.

Then the framework's, each a finding for the coordinator to route, losses
first:

7. **Pleris's first interaction downloads its renderer.** In TodoMVC, the
   first add fetches `pw-render.wasm` (386,022 bytes, sent uncompressed), two
   handler modules and the page's computed module: 395,444 bytes where
   React and Svelte fetch nothing. Under Slow 4G the first add takes
   2,506 ms to its frame (React 66, Svelte 67).
   *Route*: the runtime (preload the renderer once the page is interactive,
   serve it compressed, or render a keyed row without it).
8. **Pleris's host compresses nothing, keeps no connection and lets no cache
   keep a build file** (`write_response`: `connection: close`; `serve_file`:
   no `ETag`, `Last-Modified` or `Cache-Control`). Its first load is sent
   whole (234,001 bytes for TodoMVC, where the same files
   Brotli-compressed would be 67,051), and a warm load downloads
   everything again (216,615 bytes on the store page, Next.js
   2,897, SvelteKit 2,500). *Route*: a production host
   (precompressed Brotli, content-hashed immutable names or validators,
   keep-alive or HTTP/2): the charter's deployment story, which has none.
9. **The runtime is not minified**: `pw-runtime.mjs` is 154,052 bytes;
   esbuild 0.28.2's minifier makes it 55,576 (64% less). ADR-0188 left
   minifying to the owner; the owner's standing order of 2026-10-25 delegates
   it. *Route*: a minify step in the build (esbuild is in the workspace).
10. **With several pages of one Pleris app open, a press waits for a held
    connection.** Each store page holds its update stream for two seconds at
    a time; over HTTP/1.1 a browser opens six connections to a host, so with
    more pages than that, the next request waits for a stream to end. With 20
    pages open the last page's Add took 10,024 ms to change its count
    (Next.js 53, SvelteKit 56); a probe found 55 ms
    with one page, 1,855 ms for the command with eight. *Route*: one
    subscription per origin shared by its pages (a `BroadcastChannel` or
    `SharedWorker` leader), or HTTP/2 in the host.
11. **An idle page costs its server.** The stream is answered by a thread
    that wakes every 25 ms for two seconds under the server's `pending` lock
    (`stream_open`, 80 passes), then the page asks again: 20 idle store pages
    made 30 requests in 10 s and cost 10 ms of CPU a
    second; the frameworks' pages ask nothing. (They also push nothing: the
    live update is a feature Pleris has and they do not; its cost is
    reported, not its value.) *Route*: wake held streams on a write, not on a
    timer.
12. **The store's data layer is Rust, not Pleris.** Its operations are
    `host "store:data/..."` declarations, implemented in the development
    server's `store.rs` (2,107 lines of code, shared by every store
    example); the Next.js and SvelteKit stores keep theirs in 50 lines of
    TypeScript each. A feature that needs new server data (the brief's
    "favorite stores") cannot be written in Pleris alone today: its pull
    request would touch Rust in the server. Even the tutorial's guestbook
    reads the feed's Rust data layer. *Route*: D2 (ADR-0338's declared
    tables, served), being integrated now.
13. **TodoMVC in Pleris needs an application server for every visit**: its
    document is rendered per request, while its todos are the device's; the
    frameworks' TodoMVCs are static files any CDN serves. *Route*: prerender a
    page whose reads are all the device's at build time.
14. **The checker leaves three of the twelve changes to a reviewer**: T02
    (asks of a slow source not shared), T07 (an answer for a category the
    customer moved past shown) and T09 (a kept answer older than the
    freshness asked for) check clean in Pleris as in the frameworks; each is
    caught by its hidden test only. *Route*: rules for shared reads, stale
    answers and freshness bounds.
15. **More requests on a first load**: TodoMVC in Pleris makes 7 (three
    stylesheets, because the page's policy refuses the official sheet's
    `data:` images and `todomvc-styles.mjs` moves them to files), React and
    Svelte 5. *Route*: one stylesheet per page from the build.

And where Pleris is best, in the same terms:

16. **Behaviour**: TodoMVC in Pleris passes all 60 of the
    specification's checks in the three engines and 29 of 29 of TodoMVC's
    own Cypress suite; React 51 of 60 and 28 of 29, Svelte 48 of 60 and 28
    of 29. React's gaps: Escape does not leave an edit (`input.jsx`), the
    counter reads "1 item left!" with no `<strong>` (`footer.jsx`), no
    persistence. Svelte's: an edit is saved untrimmed and an edit of spaces is
    kept (`Item.svelte`), no persistence, a page opened at `#/completed`
    ignores it (`router.js`). Both apps are the official ones, as TodoMVC
    ships them.
17. **The same change**: over the twelve tasks, Pleris's pull requests change
    140 lines in 13 files, one language; Next.js's 323 lines in 20 files, two
    (TSX, TypeScript); SvelteKit's 235 in 19, two (Svelte, TypeScript).
    Pleris's checker refuses 9 of the 12 unsafe versions (loading and error
    states, authorization and privacy, the new order state, accessibility,
    retries); the frameworks' checkers refuse none of them. One task is
    larger in Pleris: T03, 43 lines against 25 and 29.
18. **The museum**: Pleris refuses all five exhibits; Next.js was shown
    letting three ship (G01, H07, J01) and SvelteKit two (F01, J08).
19. **Server cost per request**: the Pleris host's 0.33 ms of CPU and
    36 MiB at peak for the store page, against Next.js's 9.69 ms and
    157 MiB and SvelteKit's 2.24 ms and 118 MiB.
20. **Source**: TodoMVC in Pleris is 189 lines in one file and one language
    (persistence included), React's 278 in 11 files and four languages,
    Svelte's 201 in 8 and four. The store is the other way round: 503 lines
    of Pleris in 21 files, and the Rust above, against 151 and 134.
21. **On a fast network the bytes cost Pleris little**: unshaped, its store
    page is interactive in 66 ms in Chromium (Next.js 179, SvelteKit 76) and
    its Add reaches its frame soonest in Firefox (33 ms; 39, 37) and WebKit
    (24 ms; 40, 29), with the fewest requests (4; 6, 9); TodoMVC's first paint
    in Firefox is 46 ms (87, 105). Server rendering paints early, and its
    host answers fast; what it sends is what loses on a slow network.

## What is claimed, and what is not

- Claimed: on the machine and at the commit the evidence names, the numbers
  its tables print, each from a recorded command, each run held to
  `validate.mjs`'s rules; the behaviour each stack's TodoMVC shows to the
  same checks; each stack's checker's verdict on the twelve tasks' changes;
  the museum's record as `e14-museum` made it.
- Not claimed: field performance (these are lab numbers on a loopback, and
  Chromium's shaping is a model); that two apps sample the apps people
  build; that the twelve tasks sample the changes people make (they were
  written for the benchmark, by this project, in every stack); that a stack
  the museum did not reproduce an exhibit in is safe from it; that the
  instrument's static host is any particular CDN; anything about Pleris
  behind a production server, which it does not have.

## Next

Pleris's work first, in the order the losses cost a user; each is measured
again by `just e14-head-to-head`, which says whether it moved:

1. The first interaction's renderer (finding 7): preload or ship
   `pw-render.wasm` compressed before the first press, or render a keyed row
   without it. The first add at Slow 4G should fall from seconds to the tens
   of milliseconds the frameworks take.
2. A production host (finding 8): precompressed Brotli, content-hashed
   immutable names or validators, keep-alive or HTTP/2. Warm loads should
   receive kilobytes, not the whole page.
3. Minify the runtime in the build (finding 9).
4. One live subscription per origin, or HTTP/2 (finding 10), and streams
   woken by writes, not a 25 ms timer (finding 11).
5. D2's declared tables served (finding 12); then a thirteenth task, "favorite
   stores" (a per-customer list the store page shows and a command changes:
   authorization, invalidation, loading and error states and types across the
   boundary in one change), the first whose Pleris pull request would need
   Rust today, added to `benchmarks/tasks` and so to this comparison.
6. Checker rules for shared reads, stale answers and freshness bounds
   (finding 14).
7. Prerender device-only pages (finding 13); one stylesheet per page
   (finding 15).
8. The instrument: let the machine settle before measuring (the recipe's
   own builds and checks left a one-minute load average of 30 when the
   measurement began: wait until it falls, and print the wait); ADR-0120's
   contract in all three engines (correction 3 would have been caught there);
   the feed's timeline as a third app; the same run on a GitHub runner beside
   the PC's, two machines' numbers.

## Questions for the integrator

1. `0a4ae365` moves `pw-tasks` after `pw-table-memory` in `Cargo.lock`,
   cargo's own order: master's order is rewritten by any unlocked cargo
   command, so every recipe that builds through `todomvc.sh` and the like
   records "+ uncommitted changes" on master's next verify. Land it with this
   track, or alone first?
2. `benchmarks/headtohead` joins the pnpm workspace with two packages the
   lockfile already holds (`@playwright/test` 1.58.0, `esbuild` 0.28.2), nine
   lockfile lines: acceptable, or should the instrument resolve them from the
   harness and the SvelteKit baseline instead?
3. The recipe took 25 minutes here (twelve tasks' checkers in three stacks,
   three Cypress runs, the specification in three engines, 336 measured page
   loads, two fixed loads, forty idle pages and the controls): is a verify
   shard of its own acceptable, or should it run nightly only?
