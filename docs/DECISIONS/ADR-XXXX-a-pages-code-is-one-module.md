# ADR-XXXX: a page's code is one module

Status: proposed 2026-11-09, on branch `perf/page-bundle-20261109`, under the
owner's delegation of 2026-10-02 and order of 2026-10-25 ("deciding by best
judgement without asking"), and the order of 2026-11-09 to work the measured
gaps with Svelte largest first. Milestone: E14 (ADR-0392's "Next"). Amends
ADR-0399, which has a production page warm each piece of its code by itself.

## Context

The head-to-head at `58b0b2ca` has TodoMVC make 19 requests against Svelte's 5,
and 12 of them are scripts: the runtime, a module of computed values, and one
module for each of ten handlers. E7-L made each handler a module of its own so
that a page downloads no code until a press needs it, and so that a handler the
decision refuses is never fetched. ADR-0399 then had a production page warm them
all as soon as it is idle, and from then on a production page fetches every one
of them on every first visit, each in a request and a compression of its own.

In TodoMVC those eleven files are 6,743 bytes as Brotli sends them, 300 to 900
bytes each, before each response's headers. Most of what is in them is the same
few helper functions, and a compressor sees none of that across files.

## Decision

1. **A production build puts each page's code in one module**:
   `build/bundles/<page>.mjs`, written by the packaging from the page's handlers
   (through every block, and each view the page reaches), its computed values
   and its speculations, by esbuild, minified. The packaging says so in
   `delivery.json`. A handler a page names with no module is a build that did
   not finish, and fails it.
2. **The handler table names it**, `#bundle|<page>|/bundle/<page>.mjs`, and the
   host serves only a module its packaging wrote, at the delivery's terms: kept
   a year where it is asked for at the build's delivery, which digests it.
3. **The runtime asks for it where it would have asked for any piece of it**:
   the first need of a handler, the computed values or the speculations loads it,
   and warming asks for each as before, each finding the one module. Nothing else
   about a press changes.
4. **E7-L's order holds.** A refused handler is never fetched: a page where the
   decision refuses any handler on it does not ask for the bundle at all, and
   loads each of the others alone, as it did.
5. **Each module stays, unchanged, at its own address.** A bundle that fails to
   load is not asked for again, and each piece is asked for alone from there,
   within the same press.

A development build has no bundle, and its pages load each module as they did.

## Why not the other ways

- **One bundle of the whole app.** A page would download every other page's code
  on its first visit.
- **Runtime and page code in one file.** A second page would download the
  runtime again, where now its first visit has it cached.
- **Preload each module.** Each is still a request, and still compressed alone.

## Result

`just e14-head-to-head` at `3b0e7c4f`, with the rest of the size train; the parts are not measured apart. TodoMVC's first load makes 4 requests, from 18 at `7f328c3d` (Svelte 5, React 5), and its page's code is one module of 2,032 bytes as Brotli sends it, from 6,743 in eleven files. The store's first load makes 4, from 7 (SvelteKit 9, Next.js 6).

## Consequences

- A press before the page is idle downloads the page's code together, 2 KB in
  TodoMVC, rather than its own handler's alone. In a production build it would
  all be downloaded moments later anyway.
- Two copies of each handler's code are in the deployment, one in the bundle and
  one beside it, so that a page whose decision refuses one, or whose bundle
  fails, has somewhere to get the rest.
