# ADR-0428: what a device's page holds is what its build reaches

Status: accepted 2026-11-11, landed with train-23. Proposed 2026-11-11, on
branch `perf/train-23-20261111`, under the owner's delegation of 2026-10-02
and order of 2026-10-25 ("deciding by best judgement without asking"), and
the order of 2026-11-09 that every row of the head-to-head Svelte wins is a
defect until it is fixed or recorded.
Milestone: E14. Builds on ADR-0205, ADR-0302, ADR-0409, ADR-0412, ADR-0413
and ADR-0426.

## Context

At `71987e54` TodoMVC sends 21,203 bytes on a first visit to Svelte's 20,821,
and the large list 17,819 to 13,242. Each is one document now (ADR-0426),
whose one script is the device's runtime and the page's module.

Chromium's coverage of that script over a load, every action TodoMVC has and
a reload (`coverage_todo.mjs`) finds 22,041 of its 68,447 bytes never run, a
third. Most is the runtime's for what a page may do and this one did not.
Three kinds of it no TodoMVC can reach, whatever a person does:

- **A command's refusal and its unreachable server.** `Refused` and
  `Unreachable` are thrown by `command()` alone, which posts to a server; a
  page that talks to no server sends no command. `acted` named both classes
  where it tells a press's failure, so the device's runtime held them.
- **A graph's reading** (`$graph`, ADR-0205): a value of a type that contains
  itself is written as its nodes, and read back by `opened` and `built`.
  Neither app has such a type.
- **The handlers' own helpers.** Each of TodoMVC's eight handler modules holds
  the helpers it calls (js_pure's `prelude`: `trap`, `exact`, `some`,
  `none`, `WHITE` and `trim`), and its page's module holds each copy:
  esbuild bundles declarations of distinct modules apart, `trap` to `trap8`.

And the head-to-head's bytes count each response's head (`report.mjs`,
"headers included"): TodoMVC's document sends 830 bytes of head with a body
of 20,373, its validator 64 hexadecimal digits among them.

## Decision

1. **A device's runtime holds no command's failure.** `acted` tells of a
   refusal and of an unreachable server only under `PW_SERVER`, the folded
   flag a server's runtime has true and a device's false (ADR-0413), so the
   device's runtime holds neither class.
2. **It reads a graph only where its build writes one.** `PW_GRAPHS` is a
   feature the packaging folds as the others (`deviceFeatures`): true where
   any file of the build holds `$graph`. Every file is read, whatever its
   kind: a page's first value, a handler, a computed module, and a component
   the host runs as it renders, whose value for the document is written as
   its nodes. What the packaging itself writes from the rest and the runtime,
   which holds the word, is not read (`alone/`, `bundles/`, `.br`, `.gz`), so
   a build packed again is packed alike. Where it is false, `opened` returns
   what it is given, and `built` and its cache are no part of the runtime:
   each is an expression the fold makes `null`, as the runtime's other folded
   parts are, since esbuild drops no function declared inside another.
3. **What a page's handlers write alike is in its module once.** Bundling a
   page's module (ADR-0412), each handler's prelude, what precedes its
   `name`, is read as the compiler writes it, a paragraph a declaration, or
   one for two where one is the other's alone (`WHITE` and `trim`). A
   paragraph that two or more of the page's handlers hold word for word is a
   shared module's, `pw:shared`, every name it declares exported, which each
   of them imports instead; each handler's own file is as it was compiled.
   What is shared means in each handler what it meant there:
   - a handler's names are those it declares at its prelude's margin, as
     js_pure writes every prelude (at the margin only a comment, the close of
     what a line above opened, or a `function` or `const` of one name); a
     handler whose prelude is in another form shares nothing;
   - a shared paragraph says no name a handler declares that is not shared
     too, every word of it counted, a comment's and a string's as well;
   - a handler imports what it holds of what is shared only where it holds
     each paragraph what it imports names, and otherwise nothing: a handler
     whose `trap` is its own keeps the `int` that reads it;
   - a name two shared paragraphs would declare otherwise is shared by
     neither, a function compiled beside one handler (`callee0`,
     `closure0`) never is, and a paragraph fewer than two handlers import is
     each one's.
4. **A validator is 22 characters**: the first 128 bits of its
   representation's SHA-256, in base64url (RFC 4648 §5), where it was the
   whole digest in hexadecimal. Two representations of one resource share one
   by chance once in 2^128; a validator need tell apart only what one
   resource sends (RFC 9110 §8.8.3).

## Why not the other ways

- **The compiler writing one module of helpers for a build**, which each
  handler imports: exact by construction, as the compiler knows what it
  wrote. But every reader of a handler's module would change with it: the
  host's tests that run one, a page that loads its handlers one by one, which
  would ask for one module more, and E10's evidence of every handler. Sharing
  in the page's bundle changes what the bundle holds alone, and checks the
  form it relies on.
- **Reading any JavaScript for its names**: a second declarator
  (`let a = 0, b = 1`) or a pattern is a name no line's start shows. The
  compiler's form is what is read, and a prelude in another is refused, not
  guessed at.
- **One inline style for the document's three**, a hash fewer for each: the
  rule `<noscript>` holds must come after the one it undoes, and the page's
  stylesheet after the platform's; moving either changes the cascade of a
  page with no script. Not taken.
- **The policy's hash of a style the document does not hold**: the store's
  data layer's (ADR-0220) is in every document's policy, 54 bytes, though a
  prerendered page holds no such style. A server's page that navigates in
  place keeps its first document's policy, so every server's document says
  one policy; a device's page never navigates in place and could say only
  what it holds, but its document rendered for a request must then say the
  same (`it_says_what_a_document_says_of_itself`). NEXT.
- **`pw-build` and `pw-delivery` on a device's page**: its runtime reads
  neither, a server's page navigating in place reads both, and loads a page
  whole where they differ. Dropping them is a navigation's question, with its
  own test. NEXT. The keep-alive pair stays (ADR-0401).

## Result

Packed again from train-21's builds (Brotli 11): TodoMVC's one script
16,627 bytes to 16,054, the large list's 16,126 to 15,526; TodoMVC's page
module 2,086 to 1,967 and the large list's 2,006 to 1,763. Each document's
validator 42 bytes shorter.

The head-to-head at `06408e2d`, against the record at `71987e54`
(Svelte's in brackets): of its rows Pleris is behind in 50 and ahead in 79,
from 60 and 84.

- **Bytes as sent**: TodoMVC's first load 20,668, from 21,203 (20,821): the
  row is Pleris's. The large list's 17,117, from 17,819 (13,242). The
  store's 28,811, from 28,846 (SvelteKit's 35,022), its runtime's validator.
- **Bytes decoded**: TodoMVC 65,758 from 68,898 (57,354); the large list
  50,278 from 54,522 (35,039).
- **As a static host compresses them** (Brotli 11): TodoMVC 19,830 from
  20,430 (16,992); the large list 16,497 from 17,155 (12,567). What is left
  is the runtime's.
- The other rows that crossed moved by one to three milliseconds, either way,
  on presses this decision does not touch.

## Consequences

- A device's runtime holds `PW_GRAPHS`, folded where the build writes no
  graph; a server's keeps every reading.
- A page's module holds `pw:shared` where its handlers write anything alike;
  each handler's file is as the compiler wrote it, and the host serves it as
  before.
- Tests: `optimize-assets.test.mjs` (what a device's runtime holds of a
  server's failures and of graphs; what a page's handlers share, in a page's
  module and as `sharedPreludes` reads them), `tests::prerender::` (the
  validator); `e2e/shared-output.spec.mjs` reads the validator's form.
- Controls, each killed: seventeen in `device_runtime_mutations.py`'s
  packaging suite and one in `prerender_mutations.py`. The cache of read
  graphs folds with `built` and has no control: no word of its own is left.
