# ADR-0303: a page is shown in its layout, which the pages that name it share

Status: proposed by the integrator, 2026-10-09, under the owner's delegation
of 2026-10-02. The second of three steps to the parts two pages share kept in
place (NEXT, the DoorDash customer app's item 1): the build is named (ADR-0300), a page names its layout (this), a navigation keeps the
layout (the third). Builds on ADR-0136 (a view is written where it is used),
ADR-0144 (signals), ADR-0172 (a page's audience), ADR-0183 and ADR-0186 (a
page's title and description), ADR-0280 (a navigation after a commit).
Date: 2026-10-09. Milestone: E14.

## Context

- **The owner's Next.js bug** (ADR-0280's brief): save, go to a page, and see
  it as it was before the save. ADR-0280 made a navigation a document load,
  read after the commit: fresh, and nothing kept. Keeping the parts two pages
  share needs those parts to be the same thing on both pages first.
- **What others do** (read 2026-10-09):
  - Next.js 16.4, `layout.js`: a layout wraps the segments below it as
    `children`; "Layouts do not rerender" on navigation; they "cannot pass
    data to their children", and cannot read the search params or the
    pathname, "which would otherwise become stale"; a root layout holds
    `<html>` and `<body>`; across two root layouts a navigation is a full
    load. A layout's data is not read again on navigation: the owner's bug
    at the layout's level.
  - SvelteKit, `+layout.svelte` with `{@render children()}`: a layout's
    `load` runs again when a `params` or `url` property it read changed,
    when `parent()` ran again, on `invalidate(url)` or `depends`, or on
    `refreshAll()`; running again "does not cause the component to be
    recreated".
  - React Router 8.4, framework mode: a parent route renders its child in
    `<Outlet/>`, and "route loaders are automatically revalidated after all
    navigations and form submissions".
  - Astro: a layout is a component holding `<slot />`.

  So a layout is kept, not mounted again, everywhere; whether its data is
  read again differs, and where it is not, it goes stale.
- **Here, a view used in a page is lowered in place** (ADR-0136): its parts
  numbered in each page's template, in document order. A header written as a
  view used by four pages is four different sets of parts, with nothing to
  say they are one. A view holds no binding of its own (ADR-0136's Not
  claimed), so a header's data was each page's to bind.
- **ADR-0203's instance** is a template of its own, rendered in a frame. A
  frame holds no signal, stream, computed value or block of its own yet.

## Decision

1. **`layout Name { bindings; view { … <slot /> … } }`**, a declaration as a
   page is (`UiDecl`), without a route: its bindings are queries keyed by the
   request's reader (`current_session()`, `current_user()`) or by its own
   signals; its signals, its handlers and the views it uses are as a page's.
   - **`<slot />`** is where the page is shown: exactly one, in the
     layout's own view, at its top (in no block, loop, match or stream),
     with no attributes and nothing inside (PW5044); a layout with none is
     refused (PW5045), and so is one anywhere else, a page's or a view's
     (PW5044). HTML's own `slot` element belongs to a shadow tree, which no
     program here declares; `<slot />` is Astro's, Vue's and Svelte 4's word
     for this place.
   - **A layout is given nothing**: it declares no parameters (PW0353).
     What it shows is read from the reader, never from the page, so two
     pages that name it show it the same, and a navigation between them can
     keep it.
   - **Not a page's clauses**: no `route`, `cache`, `placement`,
     `not_found_on`, `redirect_on` and no `layout` of its own; the policy
     table refuses each (PW5105). No `<title>` and no `<meta
     name="description">`: the page states its own (PW5030, PW5034).
2. **A page names its layout**: `layout StoreLayout`, a clause beside
   `route`, resolved in the page's file as a view's tag is. It names a
   layout, once (PW0352). The word is a clause, and a declaration, only
   where a name follows it on its line: `layout` is also an effect family's
   (`layout.measure`), and a body may begin with one.
3. **Composed into the page, its parts numbered first.** A page's template
   is its layout's markup with the page's own in the slot's place, between
   two markers, `<!--pw-slot-->` and `<!--/pw-slot-->`:
   - the layout's markup is lowered first, then the page's, then the page's
     title and description (ADR-0183, ADR-0186). The layout's parts and
     elements are numbered before the page's, so they are the same on every
     page that names it, after the slot as before it;
   - its bindings and signals are the page's, under names no source writes,
     `cart~StoreLayout` (`~` is in no identifier; a view's renaming,
     `open~2`, ends in a number); its handlers are its own, one module
     whichever page holds them (ADR-0136's rule for a view); its computed
     parts are named by the layout, not the page; its `provide` reaches its
     own views, and a page's reaches none of them, as a page gives its
     layout nothing;
   - **a page's speculation covers its layout's bindings** (ADR-0122): a
     command's optimistic transition moves every binding that shows its
     entry by the same key, the layout's count with the page's, so the two
     never show two values while the command is out (Found 3);
   - the page's plan records it: its path, its own markup's schema (its
     slot empty), and how many parts and elements it numbers. The third
     step keeps the layout across a navigation between two pages that
     record the same.
4. **What a page's document holds is its layout's too.**
   - **A page declares at least its layout's audience** (PW5046): what its
     resume manifest may hold (ADR-0172) must take what the layout's
     handlers capture, which are checked against the layout's own. A
     `session` layout's page is a `session` page or narrower.
   - **Its privacy label joins its layout's**, what the layout declares and
     reads: an unlabelled layout that reads a session's cart makes a page
     that declares `cache shared` a session's value in a shared cache
     (PW5001), as a read of the page's own would.
   - **The graph has the page read what its layout reads** (ADR-0123): a
     page that reads no cart of its own, shown in a layout that does, is
     told of the cart's change.
5. **Read again with every document, and once.** Nothing of a layout is
   kept across documents: each document reads its layout's bindings when it
   is served, after the commit before it (ADR-0280), and a live document is
   told their changes as its own (ADR-0219). The layout is kept in place by
   the third step; its data is never kept. **A query bound with one key by
   both the page and its layout is read once for the document** (Found 2),
   and both show that one value.
6. **The store's four pages are shown in `StoreLayout`**: a header with the
   way home and the session's cart, counted.

## Found

1. **Two sibling elements written on one line at the top of a view do not
   parse**: `<h1>a</h1><p>b</p>` is PW0009, and `<h1>a</h1><hr />` too; inside
   an element they do. The template region ends after its first element at
   the top, and the next `<` is read as a comparison. Found writing this
   ADR's refusals; a grammar fix of its own, ADR-0243's amendment of
   2026-10-10 (merged after this ADR).
2. **A page and its layout that bind one query read it twice.** The store's
   page binds `Cart(current_session())`, and so does its layout: the host
   read the cart twice for each document, which the host's count of reads
   found (`a_shared_query_is_run_once_for_its_readers_and_a_private_one_for_each`,
   4 where 2 were). Two reads of one entry for one document could also show
   two values of it, a write committed between them. The host now reads each
   query with one key once for its document, and both bindings are given
   that value. Next.js leaves this to a request cache (`React.cache`,
   `fetch`'s memoization), which a program must remember to use.
3. **A speculation found one binding per entry.** ADR-0122's module took the
   first of a page's bindings that showed the command's entry, from the
   page's own body. The layout's count would have waited for the server's
   patch while the page's moved at once, two counts for a round trip; and a
   page with no binding of its own beside its layout's would have speculated
   on nothing. Every binding that shows the entry is now transitioned, the
   layout's among them, its key read where the layout writes it; a layout's
   binding never matches a key from the page's parameter, which a layout is
   not given.

## Alternatives

- **A layout of its own template, rendered around the page in a frame**
  (ADR-0203's machinery, the draft's first shape). Every part of every page
  with a layout would move into a frame and change its address; a frame
  holds no signal, stream, computed value or block of its own yet; and the
  host would read, render and subscribe two plans for each document.
  Composition gives the layout the same parts on every page by numbering
  them first, with nothing new for the host or the browser to hold.
- **Composition numbered in document order**, as a view's is: the layout's
  parts after the slot would be numbered after the page's, and differ on
  every page.
- **A layout's data kept across navigations** (Next.js): the owner's bug.
- **A layout given the page's parameters** (Next.js's `params`): two pages
  of one layout would show it differently, and a navigation between them
  could not keep it whole. Left for a need.
- **`{children}` or `{@render children()}`**: `<slot />` is the word most
  people who have written a layout know.

## Acceptance

Recorded by `just e14-layouts` in `docs/evidence/E14/layouts.txt`:

- **`compiler/pw-core/tests/layouts.rs`, 13 tests**, each with its control:
  - a page shown in its layout, the layout's markup the same before and
    after the slot on two pages, its parts numbered first, the page's title
    last; a page naming none as before;
  - the plan: the layout's bindings first, under `~` names, read by its
    parts; the layout recorded, the same for two pages, its schema another
    where its markup is;
  - a layout's signal, the handler that sets it and the block it decides,
    one handler module whichever page holds it;
  - a computed part named by the layout; a layout's `provide` reaching its
    views, and a page's reaching none;
  - the refusals: PW0352 (nothing, a view, two layouts), PW0353, PW5044 (in
    a block, a stream, with attributes or content, twice, in a page, in a
    view), PW5045, PW5105 (a route, cache, placement or layout in a layout;
    a layout named by a view), PW5030 (a layout's title), PW5046;
  - the shared cache: an unlabelled layout reading a session's cart makes
    the page's `cache shared` PW5001;
  - the graph: a page reads what its layout reads;
  - the clause and the declaration only where a name follows `layout`;
  - the store's page speculates on its layout's count with its own.
- **The host**: the store's four pages served in `StoreLayout`, its header
  the same on each and the page in its slot; the order's page, which reads
  no cart, told the cart's count through its layout; a query bound by page
  and layout read once (the host's read count, 2 for two readers).
- **`spikes/own-renderer/e2e/layouts.spec.mjs`, in Chromium, Firefox and
  WebKit**: every page in its layout, the header before the slot and the
  page's `<main>` in it, the header's markup the same on each; the count
  told on the page that changed it and on another open beside it; moved with
  the page's before the server answers; and served without a script.
- **`scripts/layouts_mutations.py`**: 23 mutants, on the composition, the
  plan, the checks, the privacy join, the graph, the speculation, the clause
  and the host's single read.

## Not claimed

- **Keeping the layout across a navigation**: the third step.
- **Nested layouts** (a layout shown in a layout), **a layout's own title
  template** ("%s · Store"), **a layout given the page's parameters**, and
  **more than one slot**.
