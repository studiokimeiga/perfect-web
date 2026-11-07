# ADR-XXXX: the first press finds its code: boot preloads, and a production build warms

Status: proposed, 2026-11-06, on branch `perf/first-press-20261106` (PR #76,
a draft), stacked on PR #75 (`perf/production-assets-20261104`, itself stacked on #68's
head-to-head). Proposed under the owner's standing order of 2026-10-25
("deciding by best judgement without asking") and the owner's instruction of
2026-11-06 to continue the performance work against Next.js, SvelteKit,
React and Svelte without asking. Not landed; not numbered (PARALLEL.md: the
integrator numbers ADRs). Milestone: E14 (ADR-0392's "Next", items 1 and 5).

## Amendment: a host that keeps sessions warms, 2026-11-06

**Why.** Decision 2 kept a host that admits from a kept source from warming
"until ADR-0389's repair", and question 5 asked whether the repair should land
first. It had already landed: master took it on 2026-11-06 in PR #67, which
added ADR-0389's sections of 2026-11-02, "Overlapping admissions of one
session are each answered" and its review. This branch was cut at
`45e0c1a6`, before that.

**What changes.** The change is on branch `perf/kept-source-warms-20261106`,
on top of #76 with master merged in at `ac3ea31c`:
- The handler table says `#warm|1` wherever the build warms, on a host that
  keeps sessions too. `Principals::admits_from_a_kept_source`, which only
  this rule read, is removed, and so is its test.
- The merge had two conflicts, both PR #63's exemption of the runtime's three
  files, and resolved them to master's side, which removed it. Those files
  are admitted as every request is. The GET fallback serves them with the
  request's whole path, its query naming the delivery, as the exemption did.
  `scripts/session_overlap_mutations.py`'s control that restores the
  exemption is rewritten for this branch's `serve_file`; as master wrote it,
  it would no longer build.

**Held by.**
- `tests::session_overlap::a_host_that_keeps_sessions_warms_and_its_warm_up_is_each_answered`:
  - A warming host whose sessions are kept says `#warm|1`.
  - Six requests of one session, sent together, are each answered. Five
    are what a warming page has under way: the runtime, its WebAssembly, the
    handler table, the renderer and a handler module. The sixth is a second
    tab's page.
  - Every admission read meets the others before any ends. Six is as many as
    a browser sends to one origin over HTTP/1.1 at a time.
- Its PostgreSQL twin, `on_postgres_…`, with a press in place of the page.
- `scripts/session_overlap_mutations.py`:
  - It holds both tests in its frozen inventory, now 24 tests.
  - It has a control for the rule restored: a host that keeps sessions does
    not warm.

**Unchanged.** Admission, delivery's inspection and a command's fence are
ADR-0389's. A warm-up asks only for the build's files and the handler
modules the decision authorised, and admits each as before.

## Context

ADR-0392 measured where Pleris loses to the mainstream stacks, and its
"Next" puts the first interaction first. On PR #75's head (`5c53870d`,
minified and compressed assets), re-measured on the development PC on
2026-11-06 (TodoMVC, Chromium, five runs a cell), three of its findings
were still the largest losses:

- **Finding 18, interactive last.** Under Lighthouse's Slow 4G, TodoMVC
  was interactive at 3,176 ms (React 1,759, Svelte 1,335). The boot was a
  chain of round trips: the runtime was asked for when the parser reached
  the end of the body; the resume decision's Wasm once the runtime ran; the
  build's handler table (`/pw-handlers`) once that Wasm had compiled. Each
  is one 562.5 ms request under Slow 4G.
- **Finding 17, the first press downloads its code.** TodoMVC's first Add
  fetched two handler modules, the page's computed module and the 406 KB
  renderer (92 KB Brotli): 2,383 ms to its frame under Slow 4G (React 65,
  Svelte 49); the first toggle fetched its handler, 633 ms (50; 40).
- **Finding 22, every interaction a frame behind**, on the loopback too:
  toggle 29 ms (React 14, Svelte 12), add 40 ms warm (React 12, Svelte
  13). A press's first `import()` of its handler resolves in a later task:
  after the network where the module is not there, and still a task later
  where it was only preloaded (traced in Chromium on 2026-11-06; that trace
  was not retained), and the browser paints a frame in between, so the
  change shows in the frame after next.

E7-L's ruling (2026-08-07) put a handler's bytes on the wire at its first
press and nowhere before, and `lazy-handler.spec.mjs` holds that "nothing is
prefetched without a policy that says so". The renderer and the decision
are one implementation with the server's (ADR-0130, ADR-0132); this ADR
keeps that, and changes only when the browser asks for them.

## Decision

1. **A document asks for what its runtime boots with from its head**
   (`BOOT`, last in the head, after its stylesheets): the runtime as a
   `modulepreload`, and the decision's Wasm and the handler table as
   `preload as="fetch" crossorigin`, which the runtime's own `fetch()`es
   (CORS, same-origin credentials) are answered by. The runtime asks for the
   table beside the Wasm rather than after it. Every booting page asks for
   all three, so nothing here is speculative, and none is a handler's code.
2. **A production build warms; a development build does not.** The
   production packaging (`optimize-assets.mjs`, PR #75's opt-in step) writes
   `delivery.json` (`{"warm": true}`) beside the assets; the server reads it
   (`warms`) and says so on the handler table, `#warm|1`, the build's word
   like the rest of the table and never the document's; the decision ignores
   `#` lines. A page whose build warms, once it is interactive, loaded and
   the browser idle, loads, by the loaders a press uses: the renderer where
   the page renders a block or a row; its computed and speculation modules;
   then each handler the decision **authorised**, those bound to an element
   now first. This amends E7-L for a warming build only: a handler's code
   stays off the page's critical path, and the decision is still asked
   first, so a refused handler is never fetched. A development build, as
   every build before it, fetches a handler at its first press.

   A host that admits from a kept source (accounts on, and sessions kept
   where every host reads them, ADR-0369) warms as any other: see the
   amendment of 2026-11-06, above. Until then it did not, because there, of
   two admissions of one session that overlapped, one was answered 503 and
   a command `session_changed` (ADR-0389, amended 2026-11-02). A warm-up's
   requests overlap by design. ADR-0389's overlap repair answers each.
3. **Loaded, not merely fetched.** Warming uses the press's loaders
   (`loadHandler`, `computedValues`, `speculations`, `renderModule`), so a
   module is fetched and evaluated before the press: a compiled handler's
   module defines its `run` and runs nothing. A first version preloaded
   modules (`modulepreload`) and the trace showed the evaluation hop above;
   loading removes it, and one path, the press's, serves both.
4. **A warm load that fails is forgotten as a press's is** (ADR-0382): the
   press asks under its next specifier. A press never waits for warming to
   begin: it asks its loader as before, and finds the module loading or
   loaded.

   It waits for that load within a deadline of its own, counted from the
   press. A load has no deadline of its own, since a warm-up and later
   presses share it. (The review: a press made late in a warm-up's wait had
   only what was left of the warm-up's.)
5. **The renderer's Wasm reads the IR without serde's derived visitors**
   (`pw_render::decode`): the same JSON into the same types, held to the
   derives by a differential test over the compiler's IR, every part kind
   and every one-step mutation of them (`tests/decode.rs`). The derives stay
   for every native reader, so the renderer remains one implementation; the
   Wasm falls from 406,192 to 262,527 bytes (Brotli 92 KB to 76 KB).

   Two of serde's leniencies are deliberately not copied: it also reads a
   struct written as a list of its fields, and an enum's unit variant
   written as a one-key map. The compiler writes neither, and this refuses
   both; a test holds each (the review found the second).
6. **A link to one of a page's fragment forms is followed in its press**:
   a primary, unmodified, unclaimed click on an `<a href>` to this page at a
   declared form sets the address with `pushState` (no entry where it is the
   address already) and the signal follows in the click's task, as React's
   router does; anything else stays the browser's, and back and forward still
   arrive as `hashchange`.
7. **A production build's compiled modules are compressed**: its handlers',
   computed values' and speculations' modules get Brotli and gzip siblings
   (`compressModules`), never rewritten (their bytes are the compiler's,
   their URL the identity's), and the server sends them, as PR #75 sends
   the runtime, in the representation a request accepts, with a strong
   validator per representation, `public, no-cache` and no session. A
   development build has no siblings, and its modules go as they are.
8. **A held subscription waits on the subscriber table, woken by a push**
   (ADR-0392's finding 21: each held stream asked the table every 25 ms,
   and twenty idle visitors cost the host 16.0 ms of CPU a second, the Node
   servers' 2.00). The table carries a condition each push of frames (all
   six sites) and each forced reconnect wakes; a held stream or long poll
   waits on it, for 250 ms at most at a time (`WAKE_AT_LEAST`, which also
   bounds how late it sees a subscriber forgotten while idle), through the
   same two-second and one-second holds as before. A frame is written as
   it is pushed rather than at the next 25 ms pass. Tests hold both: an idle
   stream looks at its subscriber at most twelve times in its hold, where
   it looked eighty; and with its slice lengthened past its hold, a frame
   pushed through the server's own path is written within a second of its
   push, which only the wake can do.

   A cut (ADR-0175) is looked for again under the table before each wait,
   and its wake is made holding the table, so none is missed between the
   look and the wait. A test now holds the cut's wake too; before the
   review, deleting that wake passed every test.
9. **A subscription asks at low priority** (finding 20: with nineteen other
   tabs of the store open, a press took 10 s, Next.js's 54 ms). Over
   HTTP/1.1 a browser shares six connections to a host among its pages and
   queues the waiting requests by priority; each page's subscription holds
   one nearly always. Both adapters' requests now carry `priority: "low"`,
   so a command, at a fetch's default priority, takes the next connection
   free ahead of every other page's subscription. A subscription asked a
   moment later loses nothing: it asks from its cursor. Not the whole
   repair: a connection still frees only when some subscription's hold
   ends, which one origin's shared subscription or HTTP/2 would remove.

   The priority is shown in Chromium only (`transport.spec.mjs`). A browser
   that ignores the hint sends the subscription as before, and finding 20
   stands there.
10. **A production build's files are named at one version a cache keeps**
    (ADR-0392's "Next" 2; the store's visit after its first was interactive
    under Slow 4G at 1,186 ms, the frameworks' at 690 and 643, every file
    asked for again). The **delivery id** is a digest of the build's name and
    of every file beside its documents; a production build's documents name
    the runtime, the decision's Wasm, the handler table and each file their
    markup names at it (`?b=<id>`), and the runtime, reading the id from its
    own address, asks for the renderer and those files at it too. A file or
    the table asked for at the id the host serves is kept a year
    (`immutable`); at another id (a document of an earlier build, in the
    browser's history) the host's current file is sent as before,
    revalidated before each use (`public, no-cache`). The page's policy allows the versioned
    script by its own hash. The compiled modules a page loads (handlers,
    computed values, speculations) are digested into the id too, by their
    names in the build, and asked for at it (`/handler/<identity>.mjs?b=…`;
    a retry's `attempt` after it). A development build is versioned not at
    all: its documents and every address are byte for byte as before.
    Measured (`ver5`, before the modules were versioned): TodoMVC's visit
    after its first is interactive under Slow 4G at 650 ms (React 1,202,
    Svelte 1,168); the store's at 640 ms (1,186 before; Next.js 686,
    SvelteKit 639).

    Two amendments from the review, below:
    - **Only tags are named at the id.** A reader's text is sent as
      written, since text escapes no `"`.
    - **A soft navigation stays within one delivery.** A versioned document
      says its delivery (`pw-delivery`), and a page that navigates in place
      holds the next document to it, as it does to its build (ADR-0300). A
      document of another delivery is loaded whole, so a runtime never
      meets another delivery's Wasm in the cache.
11. **A press's own paths are run once, on nothing, when the page is idle.**
    With its code warmed, TodoMVC's first add still took 8 ms unthrottled
    and 33-35 ms at 4x CPU from its key to the change in the document, its
    second about 1 ms: a browser compiles a function the first time it is
    called, and the trace showed 521 of the renderer's WebAssembly functions
    compiled inside the press (21-22 ms at 4x CPU) and 75 JavaScript ones.
    Once a production build's page is idle, its runtime asks the renderer
    to run each of its requests once on a sample (`warm`, its sample in
    `runtime/pw-render-wasm/src/warm.json`: every part a block can hold, in
    each escaping context, at values of each wire form), then renders the
    page's first block at the values it shows, which computes every value
    the page derives, and morphs it into a copy in a document of its own,
    which is inert: nothing imported into it loads or runs. Both are thrown
    away. One block calls the functions any block's change calls, so one
    serves. Two kinds of block are passed over:
    - a block holding a `<dialog>`, since a morph closes and refocuses one;
    - a block larger than 400 elements (a page's main list), whose copy and
      morph would cost more than the change they prepare.

    Each step waits for the browser's next idle period, and none runs once
    the runtime has ended. A runtime that hands its page over lets its
    renderer go (the review).

    The tests:
    - a unit test fails when the IR gains a kind of part the sample lacks,
      or the sample stops rendering;
    - `warm.spec.mjs` holds that the document is not changed by a single
      mutation, that focus does not move, and that a mark set on the live
      block survives the warm-up, which a morph of the live block would take
      away. (The review: rendered at the values it shows, a morph of the
      live block changed nothing either, so without the mark that test
      could not fail.) Measured with
    `first-add.mjs` (a page left idle after its load, two rounds of eight
    runs, medians): key to the change 32.5-34.9 → 7.2-8.3 ms at 4x CPU and
    7.8-8.0 → 1.6-1.8 ms unthrottled; to the frame after it 60-70 → 36-45 ms
    and 16.6-17.0 → 10.3-10.6 ms; no WebAssembly compiled in the press. In
    the head-to-head (`wp-*`), TodoMVC's first add under Slow 4G went from
    51 ms (`warm4`) to 31, React's 33 and Svelte's 34 in the same run; at
    loopback, cold, from 16-18 to 8.0-11 (React 8.5-8.9, Svelte 7.5-8.8).
    What it costs: the renderer's `warm()` takes 5.2 ms of an idle page's
    time unthrottled and 23.7 ms at 4x CPU, the compiling the press no
    longer does; the sample adds 990 bytes to the renderer as sent (Brotli).

## Measured (the development PC, 2026-11-06)

i9-13900K, 32 CPUs, WSL2, Chromium 145, Playwright 1.58.0; TodoMVC; five runs
a cell; A is PR #75's head (`1d82a2b2`, its source `5c53870d`), B this
branch; each side built and served by its own release host. Medians.

| | A: PR #75 | B: decisions 1-4 | B: and 5-7 | React | Svelte |
|---|---:|---:|---:|---:|---:|
| interactive, Slow 4G | 3,176 ms | **1,414 ms** | **1,413 ms** | 1,682-1,703 | 1,302 |
| first add, Slow 4G | 2,383 ms | 96 ms | **51 ms** | 35-41 | 35-38 |
| first toggle, Slow 4G | 633 ms | 23 ms | **25 ms** | 24-30 | 21-26 |
| filter, loopback, cold | 29 ms | 31 ms | **14 ms** | 13-15 | 29-30 |
| toggle, loopback, cold | 29 ms | **15 ms** | **15 ms** | 13 | 14 |
| add, loopback, cold | 40 ms | 16 ms | 18 ms | 10 | 8.4-9.9 |
| add, loopback, warm visit | 37 ms | 6.6 ms | 12 ms | 7-13 | 5-8 |
| first paint, Slow 4G | 1,244 ms | 1,244 ms | 1,256 ms | 1,200-1,204 | 1,204 |
| first load received | 52,836 B | 175,377 B | **140,029 B** | 70,085 | 20,821 |
| first load requests | 7 | 19 | 19 | 5 | 5 |

(B "decisions 1-4" is the `warm3` record, "and 5-7" `warm4`: the decoder,
the fragment's links and compressed compiled modules. React's and Svelte's
ranges are their medians across the two records. The loopback adds differ
within their spreads: 16 [13-17] and 18 [17-27].)

The raw runs and commands are in
[`evidence/E14/first-press-2026-11-06/`](../evidence/E14/first-press-2026-11-06/README.md).
Every run held `validate.mjs`'s rules.

## What it costs, plainly

- **Bytes.** A page that warms downloads, after it is interactive, what its
  presses would have: TodoMVC's first load is 141,822 B received (`wp`),
  twice React's and nearly seven times Svelte's, 77 KB of it the renderer's
  Wasm as sent. Under Slow 4G the warming takes about 1.7 s over HTTP/1.1's
  six connections. A visitor who never presses pays for it.
- **Idle time.** Decision 11 spends 5.2 ms of an idle page's time
  unthrottled (23.7 ms at 4x CPU, in one task) compiling what a first
  change would have; a press made during it waits for it, and then runs
  that much faster.
- **First paint.** The boot preloads share the first round trip's bandwidth
  with the stylesheets: about 20 ms later under Slow 4G (1,244 to 1,264 ms
  in one A/B, the same in the next); the page is interactive 1.76 s sooner.
- **The filter** still lands a frame behind React's (31 ms, React 15,
  Svelte 30): it follows the fragment's `hashchange`, which a browser
  dispatches in a task of its own.

## Not claimed

Field performance is not claimed. The head-to-head's records here are
Chromium's; Firefox's TodoMVC run is indicative only (the instrument
refuses a Firefox-only record, see the evidence's README), and WebKit could
not open a page on the PC in this session, for any test, changed or not.
The host is still the development host: no keep-alive, no HTTP/2 (decision
10 gave it immutable caching of a production build's files). The specs the
changes reach passed in Chromium and Firefox (the evidence's README lists
each run). The full `just e14-head-to-head`, the full browser suites in
three engines and exact-head CI remain the gates (AGENTS.md); local runs do
not replace them.

The review's fixes (below) are held by tests and mutation controls, not by
measurement: the numbers above were taken before them. Also not claimed:
- **A host that keeps sessions, in a browser.** It warms since the
  amendment of 2026-11-06. Its host's tests, in this process and on
  PostgreSQL, hold that. No browser test runs a warming build on hosts that
  keep sessions: the second host's spec runs development builds.
- **A file replaced in a running host's `dist`.** It is sent `immutable`
  under the id the host computed when it started, until the host restarts.
- **Every push site's wake.** Two wakes are held by tests: a frame pushed,
  and a cut. A lost wake at the other four sites would delay a frame by
  one slice at most.
- **The edges of a fragment link:**
  - a form that is also an element's id is not scrolled to;
  - a `<base target>` is not read;
  - the page's own `hashchange` listeners do not hear a link followed in
    its press.
- **The warm-up's 400-element bound**, which is a judgement, not a
  measurement.

## The independent review (2026-11-06)

AGENTS.md asks for a separate reviewer before a pull request. Two reviewers
read the branch's diff, read-only, one the server and delivery and one the
runtime and the renderer; their findings were hypotheses until reproduced.
None was critical. Each fix has its own commit and a test that fails
without it (built and served as the suite serves it), except where noted:

| Finding | Disposition |
|---|---|
| A versioned build named files inside a reader's text | Fixed: tags only (`d30a5810`) |
| A soft navigation across a redeploy paired a new runtime with old, kept Wasm | Fixed: `pw-delivery` (`874068b7`) |
| A press inherited a warm-up's deadline | Fixed: a deadline per press (`72edb56d`) |
| "The page is as it was" could not fail | Fixed: a mark a live morph removes (`85faaa9a`) |
| Overlapping session admissions, made routine | Guarded: no warming where admission reads a kept source (`b6e38417`); question 5. Lifted by the amendment of 2026-11-06: ADR-0389's overlap repair (PR #67) answers each |
| A cut's wake could be lost, and none was tested | Fixed and tested (`906fdbaf`); one look for a cut a pass, so that its control sees the look go (`21863b22`) |
| The cache policy, untested through its routes | Tested (`bf07fe9e`) |
| `warmPaths` outlived its runtime, ran outside idle, and had no bound | Fixed; read, not observed, by a test (`d8e1b639`) |
| A fragment link's `pushState` during a browser load | Not reproduced in Chromium or Firefox; unchanged (`d8e1b639`) |
| `fragment.spec`'s new assertion could not fail | Fixed (`caa44bd3`) |
| The decoder's second leniency | Documented and tested (`eafdd9da`) |
| A file replaced in place is sent `immutable` | Not changed; not claimed |
| This ADR's "kept by no cache", a lost executable bit, a stale comment | Fixed (`bf07fe9e`, `bfe02214`) |

The pre-push check found one more, outside the review. The renderer's
contract test had failed since `1f021e1d`, because its harness lacked the
runtime's new `delivered` and `attempted`. It is fixed in `3b1136e3`.

At the last fix, `21863b22`, the whole check passed (the browser suite:
1,061 of 1,074, 13 skipped by their own conditions). Every mutation script
the fixes reach was run whole, and killed every mutant: 119 of 119 in eight
scripts. The evidence's README, under "The review's fixes", lists each red
and green.

## Next

1. **The decision's Wasm is the largest file on the boot's round trip**
   (21 KB Brotli of 37 KB with the runtime; Svelte's TodoMVC is 21 KB in
   all), and what is left of the gap to Svelte's interactive time under
   Slow 4G is those bytes. Part of its weight is its decision traces, built
   with `format!` (Rust's formatting machinery) and logged by the runtime;
   how much was not measured here. Shrinking them changes what a refused
   handler says, so it is the owner's call (question 3).
2. **A live page's idle requests** (the store: 29.7 a minute, where the
   frameworks' pages hear no change and ask nothing): a stream is held two
   seconds and opened again at once, and ADR-0297's `LIVE` (3 s) and the
   cursor's acknowledgement by the next request both rest on that. Holding
   a stream for tens of seconds needs an explicit liveness signal in their
   place: an ADR of its own.
3. Fewer warm requests and one connection's latency: a host that keeps
   connections alive or speaks HTTP/2 (ADR-0392's "production host").
4. Compiled templates for the browser, if the renderer stays TodoMVC's
   largest first-load cost (77 KB Brotli, warmed): an ADR of their own,
   since they are a second implementation of what ADR-0130 keeps as one.

## Questions for the integrator

1. Should a development build warm too? Warming changes what a press
   waits for, and the browser suites run development builds; this ADR keeps
   them lazy so E7-L's tests keep their meaning, and holds warming to
   `warm.spec.mjs`. With every base page's build warming (a `delivery.json`
   put beside it for the run), 15 tests of `press-modules.spec.mjs` and
   `keyed-list.spec.mjs` fail in Chromium and Firefox, the same 15 with and
   without decision 11: each installs its fault after the page is ready, or
   counts the runtime's log lines, and expects the press to be the first to
   load. Under warming they would install their faults before the load; the
   case they could not see, a warm load that never answers, is held by
   `warm.spec.mjs` (the press fails at its deadline and the next one loads).
2. Should the head-to-head also time a press made at the moment a page
   says it is interactive, before its network is quiet? A warming page's
   first press then waits for what is still arriving.
3. Is the decision's trace text part of what the browser's build must
   carry? A build of `pw-resume` for the browser that kept each verdict and
   its code, and not the words, would be smaller on the boot's round trip;
   the runtime would log the code alone, and the native decision would keep
   its words.
4. May a page's policy allow `data:` images (ADR-0333 allows `'self'`
   only)? TodoMVC's first add and first toggle each fetch one drawing of the
   checkbox, which React's and Svelte's sheets carry inline; under Slow 4G
   (562.5 ms a request) the checkbox would be drawn that long after the
   change, which the instrument, timing the change's frame, does not see. The
   alternative within the policy is for the page to preload both drawings,
   two more requests in its first load.
5. Should ADR-0389's repair land before warming builds serve hosts that
   keep sessions? **Answered** by the amendment of 2026-11-06: the repair had
   landed on master (PR #67). With master merged in, such a host warms, and
   its warm-up's overlapping requests are each answered.
