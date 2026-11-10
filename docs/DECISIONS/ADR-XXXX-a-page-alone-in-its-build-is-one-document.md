# ADR-XXXX: a page alone in its build is one document

Status: proposed 2026-11-10, on branch `perf/train-20-20261110`, under the
owner's delegation of 2026-10-02 and order of 2026-10-25 ("deciding by best
judgement without asking"), and the order of 2026-11-09 that every row of the
head-to-head Svelte wins is a defect until it is fixed or recorded. Milestone:
E14. Builds on ADR-0409, ADR-0412, ADR-0413 and ADR-0419; amends ADR-0132's
table, for this page alone.

## Context

The large list's first load is four requests, at `7ac4f20c`: its document
(1,361 bytes as Brotli sends it), the device's runtime (13,312), the page's
code in one module (2,347) and the handler table (364). Svelte's is two, a
document and a script; TodoMVC's is four in Pleris and five in each other
stack. The runtime waits for the table before it decides which handlers it
may attach (ADR-0132), and the page's module before it renders what the
device holds, so each is on the way to the page being ready to use.

Merging the runtime and the module into one file saves 42 bytes of Brotli
(15,617 together, 15,659 apart): what the two files cost is their requests
and the round trip they wait on, not their bytes.

## Decision

1. **A build of one page has that page's code as one module**
   (`build/alone/<page>.mjs`, a directory of its own, which no host reads
   for a page's module): the device's runtime, with the page's
   one module (ADR-0412) imported into it and its flags as the device's
   (ADR-0419), and `PW_ALONE` true. The packaging writes it where the build
   has exactly one page, the page has its module, and the build has a device
   runtime; for any other build it writes none.
2. **The host sends that page's document with all of it inside**, where the
   page runs on the device (ADR-0413) and is prerendered (ADR-0409): the
   module as the document's one script, `<script type="module">`, which its
   policy allows by its hash; the build's handler table beside it, as the
   host answers it at `/pw-handlers`, in a data block
   (`<script type="application/json" id="pw-handlers">`, its `<` escaped as
   ADR-0097 escapes the page's parts); and no link that fetches anything as
   it boots. Loading the page is one request.
3. **The table is of the runtime it is read by** (ADR-0132's rule, kept):
   the decision reads a table from the server that served the runtime, and
   never from a document, as a cached document from another build may name
   handlers this one lacks. Here the runtime, the page's code and the table
   are all in one document, written by one host from one build: a document
   cached from another build carries that build's runtime and its own table
   with it. A table that cannot be read is empty, and every handler is
   refused, the closed side, as before.
4. **What the parser would not read as one script is not put in one.** The
   module's text is refused where it holds `</script` or `<script`, in any
   case: the first ends the element, and the second, after a `<!--`, hides
   its end: in the script data double escaped state a `</script>` returns
   the tokenizer to the escaped state, where in the escaped state it ends the
   element (the HTML Standard, 13.2.5, the tokenizer,
   html.spec.whatwg.org/multipage/parsing.html, read 2026-11-10). A `<!--`
   alone ends nothing, and the runtime
   writes the anchors around a row as comments. esbuild escapes `</script`
   in what it writes for a browser, "in case you inline esbuild's output
   directly into an HTML file" (esbuild.github.io/api, Platform, read
   2026-11-10). A refused module's page is delivered as before, its runtime
   and its module fetched.
5. **Only a page alone in its build.** A build of several pages keeps one
   runtime file, which a browser keeps for each page of it after the first;
   every page carrying its own copy would send it again for each.

## Why not the other ways

- **One file, fetched by its address**: two requests, and a round trip after
  the document's, for 42 bytes.
- **The table in the runtime's file, written by the packaging**: a second
  writer of the table, in JavaScript beside the host's. The host is its one
  writer, here as at `/pw-handlers`.
- **Each device page carrying its runtime**: a site of several pages sends
  the runtime once for each, where a cache now keeps one.

## Result

The large list's build held its fixture of presses (`examples/rows/events.pw`,
a corpus fixture the head-to-head does not count) as a second page, so it was
no page alone in its build: the fixture is a build of its own now
(`row-events.sh`, its own hosts), and the large list's build is the app the
head-to-head measures. A host refused a build that computes nothing on a
host, which has no `components` directory; it serves one whose contracts
export pages alone, and refuses one that lost the components its contracts
name, as before.

An A/B on this PC, the head-to-head's driver at this branch and at train-19's
commit (`7ac4f20c`) in one state of the machine (`--apps rows,todomvc
--engines chromium --conditions cold --runs 7 --stacks pleris,svelte
--no-load`), Svelte's in brackets:

- **Requests on a first visit**: the large list 1, from 4 (2); TodoMVC 1,
  from 4 (5).
- **Ready to use**, cold, ms: the large list 13, from 22 (12); TodoMVC 13,
  from 29 (33). Under Slow 4G: the large list 686, from 1,279 (1,235);
  TodoMVC 721, from 1,291 (1,302).
- **Bytes as sent**: the large list 17,380, from 19,387 (13,242); TodoMVC
  21,260, from 22,843 (20,821). Both still behind: what is sent is the
  runtime's, and a page's own generated code is NEXT's. The policy measured
  here allowed the two scripts this page does not hold; it allows its own
  alone now, 106 bytes fewer.

The head-to-head's figures at this branch's commit go here when it has run.

## Consequences

- A page alone in its build asks for nothing as it boots, and a visit after
  the first revalidates its document alone (ADR-0409's validators).
- The device's runtime holds `PW_ALONE`, false and folded away in every file
  but the alone module; the server's runtime drops it too.
- `e2e/device-runtime.spec.mjs` reads TodoMVC's production build, which is a
  page alone in its build: it holds the one request, and changes the table
  in the document where it changed `/pw-handlers` before. A build of several
  device pages keeps the files it fetched, held by the host's tests.
