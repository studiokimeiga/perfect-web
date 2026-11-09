# ADR-XXXX: a page no reader decides is one document

Status: proposed 2026-11-09, on branch `perf/prerender-device-pages-20261109`,
under the owner's delegation of 2026-10-02 and order of 2026-10-25 ("deciding
by best judgement without asking"), on the owner's choice of 2026-11-09
("Prerender first"). Milestone: E14 (ADR-0392's "Next", item 8).

## Context

The head-to-head at `58b0b2ca` (`docs/evidence/E14/head-to-head.txt`) measured
what it costs that a page which is the same for everyone is served as if it
were one person's.

- **TodoMVC's document names no reader.** Its 10,974 bytes were byte-identical
  in all seven cold runs, each a new browser context and so a new session; its
  resume manifest's scope is `public`.
- **It is served as a session's.** It is rendered again for each request and
  sent `private, no-store` and uncompressed, with no validator, and with a
  session's cookie where the request had none.
- **What that costs against Svelte:**
  - a repeat visit receives 11,645 bytes against 759. The report prints this and
    judges no row of it, so neither its losses nor pleris.dev count it;
  - a first visit receives 8,834 bytes more than the document's Brotli size;
  - three of the report's losses are this one fact: a document served at 0.59 ms
    at the median against 0.27 from a static host, 1.50 against 0.78 at the 99th
    percentile, and 0.38 ms of CPU a request against 0.18.
- **The rules that keep it private are about a session's answers.** ADR-0184's
  is "a response to a session's request is kept by no cache", because of what a
  session's page holds, and ADR-0398 kept documents out of its packaging because
  they are personalized. Neither speaks of a document that holds nothing of
  anyone's.

## Decision

1. **The compiler says which pages no reader decides.** It is derived, and
   nothing declares it: a page's plan says `"prerender": true` where
   - it has a `route` (an address of its own), and no parameter, path or query;
   - it binds no query, has no stream, and asks no predicate of its reader;
   - its scope is `public`;
   - and **no handler on it sends a command**: in its markup, its layout's, its
     blocks' and any view it reaches.

   The first three say the document is the same for every reader: what remains
   to render it is its signals' first values, which are the build's. The last
   says nothing on it ever asks the server anything, so it has no use for a
   session. A value the device keeps (ADR-0366) does not change this: a document
   already holds only its declared default, hidden until the browser has read
   the device. `pw build` says which pages it marked.

2. **The host renders each one once, before it listens**, in the public
   partition, with the deployment's origin and policy already settled. A page
   that does not render stops the host, as a handler that was not compiled does:
   a build that cannot show a page did not finish. One process, one rendering,
   one set of bytes: no file on disk can be from another build, and there is no
   second renderer.

3. **It is sent as a file of the build is, and asks nothing of its reader.** The
   same document for everyone:
   - `cache-control: public, no-cache`: any cache may keep it, and asks before
     each use;
   - a strong `etag` for each representation, `vary: accept-encoding`, and a 304
     with no body for a request that holds it;
   - Brotli at quality 11 and gzip at level 9, made once at start, identity
     where a request accepts neither;
   - its `content-security-policy`, `pw-build` and `pw-delivery`, as every
     document's;
   - **it names no session**: no cookie is set, so a device-only app never
     has one, and a request with none reads no session.

   It takes the place of rendering and nothing else. It is answered where a
   routed page is, so a host route keeps its precedence over a page's, and a
   request that brings a cookie is admitted as every request is (ADR-0389,
   whose `every_request_is_admitted_and_a_failed_read_refuses_it` this does
   not change: skipping it would answer a document whose runtime and modules
   are refused in the same outage). Only the address the page declares is
   served so: `/page/<path>` renders as it did.

4. **A page the browser's back-forward cache brings back reads the device
   again.** `no-store` kept such a page out of that cache in most engines. A page
   that says `no-cache` may be kept, frozen, and another tab's writes are never
   told to a frozen page: so on `pageshow` with `persisted` the runtime reads what
   the device keeps, as it reads it after a `clear()`.

5. **The head-to-head holds it to this.** TodoMVC's Pleris target declares
   `compresses: "all"`, so a document sent uncompressed is refused as any other
   file would be, and the store stays `root-assets`.

The `brotli` crate, 9.1.1 (<https://crates.io/crates/brotli/9.1.1>, BSD-3-Clause
AND MIT, both already allowed; pure Rust; Dropbox's), is new, for the host alone:
checked on crates.io on 2026-11-09 with `cargo info`. gzip is `miniz_oxide`, which
the host has. A document's gzip is 21% larger than its Brotli (TodoMVC: 2,591 B
against 2,140).

## Why not

- **Write the file at build time.** It would need the host's document writer at
  build, and a check that the file is this build's: a new way to be wrong, and
  the deployment's origin is not known then.
- **Keep the first render.** A cache filled by the first request races, and it
  turns a build's failure into one visitor's.
- **Declare it**, `prerender` on a page. A declaration can be wrong, and the
  compiler can already prove the answer.
- **Allow commands on such a page.** Its first command would be its first
  request with no session, a path nothing has exercised. A page that sends
  commands stays as it was until something needs otherwise.

## What it does not do

- It does not make a device-only app servable by a plain static host. That wants
  file names a static host keeps without being told (`?b=` is no content hash to
  a host's default policy), the handler table as a file, and the policy in a
  `<meta>`. It is its own decision. Rendering the document once is its first step.
- It does not compress a personalized document (ADR-0398).
- It does not touch the WebAssembly, which is most of TodoMVC's first load.
- A public page that binds a query is not prerendered: its document changes
  with its data.

## Result

To be recorded from `just e14-head-to-head` before this is accepted.

## Held by

`just e14-prerender` records each of these
(`docs/evidence/E14/prerender.txt`).

- **Which pages are marked**: `compiler/pw-core/tests/prerender.rs`. Each case
  changes one thing of a page that is marked: no address, an address that gives
  it a value (a path's and a query's), a query (the page's and its layout's), a
  stream, each scope that is someone's, and a handler that sends a command on
  the page, in a block's arm, in a row, in a view, in a view that contains
  itself and in a layout. Their controls stay marked: a press that changes only
  a signal in each of those places, and a value the device keeps.
- **What the host sends**: `tests::prerender::` and
  `tests::todomvc::todomvc_is_one_document`, over real connections.
  - Every reader the same bytes, and no cookie.
  - The document is what a request would have rendered, but for each row's
    token, which is keyed by whose the document is (a page with no rows is the
    same bytes).
  - Each representation decodes to the document; Brotli, then gzip, then none,
    as the request accepts; a validator each; a 304 for the one held, with no
    length; a weak tag and another representation's miss.
  - Its policy is the rendered document's. Its build and delivery are said.
  - A `HEAD`; a page not marked as it was; a host route keeping its place; a
    request with a session admitted and refused as every request is; a page
    that does not render stopping the host.
- **In three engines**: `e2e/todomvc.spec.mjs` runs through this path, and two
  tests name it: the one document, kept and asked for again, and a page brought
  back from the back-forward cache reading what was written while it was away.
  Playwright cannot freeze a page, so the second gives the page what a frozen
  one is given: the write and no event, then `pageshow` with `persisted`.
- **The controls**: `scripts/prerender_mutations.py` (the mark and the host,
  strict, every kill at an assertion) and one in
  `scripts/device_store_mutations.py` (a page brought back reads nothing).
