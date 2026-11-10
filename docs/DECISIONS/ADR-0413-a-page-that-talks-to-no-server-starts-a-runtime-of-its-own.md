# ADR-0413: a page that talks to no server starts a runtime of its own

Status: accepted 2026-11-10, landed with train-16. Proposed 2026-11-09, on
branch `perf/device-runtime-20261109`, under the owner's delegation of
2026-10-02 and order of 2026-10-25 ("deciding by best judgement without
asking"), and the order of 2026-11-09 to work the measured gaps with Svelte
largest first. Milestone: E14. Builds on ADR-0409, whose compiler marks the
pages this applies to, and on the page bundle's.

## Context

After the renderer's and ADR-0412s, what a TodoMVC page sends on its
first load is its document, its stylesheets, 2 KB of its own code, the runtime at
21.3 KB as Brotli sends it, and `pw-resume.wasm` at 6.6 KB and a request of its
own. Svelte's whole page is 17 KB.

Most of that runtime and all of that module are for a page TodoMVC is not. The
compiler has already marked it: no reader decides its document, no handler sends
anything, and it has no layout. Such a page sends no command, keeps no
subscription, reads no stream, speculates about nothing and never navigates in
place. Its only decision is whether each handler its document names is one this
build compiled, which `pw-resume.wasm` answers by hashing what the table already
holds as text.

## Decision

1. **A second runtime, from the same source.** The packaging replaces
   `const PW_SERVER = true;` and lets esbuild shake the tree: every path only a
   server's page reaches is guarded at its call site and falls away, and
   `pw-runtime-device.mjs` is written, minified and compressed, beside the
   runtime. A build says so in `delivery.json`.
2. **The host starts it for a page no reader decides and with no layout**
   (`runs_on_device`): that page's one document starts the device's runtime,
   preloads it, and asks for no decision module. The policy allows each runtime
   a build can start, by its hash. Every other page is as it was.
3. **Such a page makes the same decision, in a few lines of the runtime.**
   `deviceDecider` reads the same table and the same manifest, and answers with
   the same code and recovery. It keeps `pw-resume`'s order: the manifest's
   bytes, its scheme, its ABI, the handler, the scope, the document. Nothing is
   trusted that was refused before: a document cached from another build still
   names handlers this one does not have, and that is what this refuses.
4. **The two are held to each other in every run of the suite.**
   `decision-agreement.test.mjs` asks the real `pw-resume.wasm` and those lines the
   same questions, over twelve tables and every single and pairwise change to
   nine fields, and a seeded walk, and fails on any difference. The first
   version was wrong: it ignored the scheme and the ABI and compared a build the
   module does not. It attached a manifest the module refuses, and this test
   said so on its first run.
5. **The browser suite can test what is deployed.** A production build says in
   `delivery.json` which runtime it packed, so the host map takes it as a build
   of the runtime under test, where before a minified runtime was simply
   "another" one. `just e14-device-runtime` runs TodoMVC's whole suite against a
   production build, in three engines.

A development build has no second runtime, and its pages are as they were.

## Why not the other ways

- **No decision at all on such a page.** It would attach whatever a cached
  document names, which is the failure E7V exists to refuse.
- **One runtime, the server's half loaded on demand.** Every page with a server
  would ask for one more file before its first press, and most pages have one.
- **A runtime per app, from what its pages use.** A build would have as many
  runtimes as it has combinations, each cached alone. Two cover what a page can
  be: it talks to a server, or it does not.

## Result

`just e14-head-to-head` at `3b0e7c4f`, with the rest of the size train; the parts are not measured apart. `pw-runtime-device.mjs` is 14.5 KB as Brotli sends it where the runtime is 21.3 KB, and TodoMVC's first load receives 22,623 bytes, from 45,913 at `7f328c3d` (Svelte 20,821, React 70,085). Compressed alike it is 20,684 against Svelte's 16,992, which is 16,506 of script against about 14,800 and a document that holds the page and its stylesheets. Under Slow 4G it is interactive in 1,269 ms, from 1,373 (Svelte 1,306, React 1,688). TodoMVC's whole specification, 60 tests in three engines, passes on the production build.

## Consequences

- A build deploys two runtimes. Each page is sent one.
- A path added to the runtime that only a server's page reaches must be guarded
  by `PW_SERVER`, or it ships to pages that cannot use it. The packaging's test
  names what must not be in the device's runtime, and fails for each that is.
  Test hooks count: two of them, `__pwTestApply` and `__pwTestBatch`, held all of
  the frame-applying code in the device's runtime until they were guarded.
- There are two implementations of the decision. That is the cost of not
  fetching the module, and the test above is what pays it.
- A server's page still fetches `pw-resume.wasm`. Item 3 shows it could stop,
  which would take 6.6 KB, a request and `'wasm-unsafe-eval'` off every page; that
  is a decision of its own, for after this lands.
