# ADR-0415: a page's stylesheets, small together, are part of its document

Status: accepted 2026-11-10, landed with train-16. Proposed 2026-11-09, on
branch `perf/inline-styles-20261109`, under the owner's delegation of 2026-10-02
and order of 2026-10-25 ("deciding by best judgement without asking"), and the
order of 2026-11-09 to work the measured gaps with Svelte largest first.
Milestone: E14. Builds on ADR-0409.

## Context

The head-to-head has Pleris trail on first contentful paint where the page's
stylesheets are what a first paint waits for: TodoMVC at 1,244 ms against 1,192
under Slow 4G, and 32 ms against Svelte's 20 in WebKit, and on requests, 7 against
5 even after the page's code became one module. Three of those seven are
stylesheets, 2.9 KB between them as Brotli sends them, each its own round trip
that a browser holds the first paint for.

A document ADR-0409 made is compressed once and revalidated: on a
repeat visit its bytes are not sent at all, so what is in it costs nothing then.

## Decision

1. **A page that is one document holds its stylesheets in its head**, in their
   order, in one `<style>`, where every stylesheet it names is a file of this
   build that means the same inside a document, and they come to at most 4 KB
   together as Brotli sends them. The links are gone, and with them the requests
   and the wait.
2. **Otherwise nothing moves.** One stylesheet that cannot, a stylesheet of
   another origin, a link that says more than its address (`media`), a `<style>`
   of the page's own in its body, or too much together: the page keeps every
   link it had. Half a page's stylesheets inlined would change their order.
3. **What cannot move is what would mean something else.** `@import` and
   `@namespace` must come first in a sheet of their own; a relative `url()` reads
   from the sheet's address and would read from the page's; and `</style` would
   end the element. An address from any page, `data:`, absolute or a fragment,
   moves.
4. **The page's policy allows exactly what it holds**, by that `<style>`'s hash:
   the policy of a prerendered document is its own.
5. **A build says so** (`inline` in `delivery.json`, written by the packaging). A
   development build, and a page rendered for each reader, keep their links. A
   document rendered per request is sent again on every visit, and would pay for
   what it holds each time.

## Why 4 KB

It is where a document still fits the first flight with room to spare. A new
connection's first window is about 14 KB; TodoMVC's document and its stylesheets
together are 5 KB. Above that a page's own document starts to wait on its
stylesheets instead. Astro inlines stylesheets under 4 KB by default, and
SvelteKit offers the same as an option; both count uncompressed bytes, which is
stricter for a page and looser for a build. This counts what is sent, which is
what a reader pays.

## Why not the other ways

- **One combined stylesheet.** Two requests fewer, and a first paint still waits
  for the third.
- **Every page, however rendered.** A document sent again on each visit would
  carry its stylesheets each time, where a linked sheet is kept a year.
- **Inline what fits and link the rest.** The order would change.

## Result

`just e14-head-to-head` at `3b0e7c4f`, with the rest of the size train; the parts are not measured apart. TodoMVC asks for no stylesheet, and its document is 3,959 bytes as Brotli sends it, from 2,109 and three more requests. Under Slow 4G its first paint is at 652 ms, from 1,244 at `7f328c3d` (Svelte 1,204, React 1,204), and its largest content at 652 ms, from 1,244 (Svelte 1,320, React 1,696). A production build's stylesheets are minified as well.

## Consequences

- A site of many prerendered pages sends these bytes once for each page a reader
  opens, rather than once for all of them. It is at most 4 KB a page, and a page
  whose sheets are larger is unaffected.
- A stylesheet's own file is still in the build, and still served, for a page
  that links it.
