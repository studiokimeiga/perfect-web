# ADR-XXXX: the browser renders with a page of JavaScript, held to the server's renderer

Status: proposed 2026-11-09, on branch `perf/browser-renderer-20261109`, under
the owner's delegation of 2026-10-02 and order of 2026-10-25 ("deciding by best
judgement without asking"), and the order of 2026-11-09 to work the measured
gaps with Svelte largest first. Milestone: E14 (ADR-0392's "Next"). Amends
ADR-0130's step 1 and ADR-0172, which built the browser's renderer as
`runtime/pw-render` compiled to WebAssembly; retires what ADR-0382 and ADR-0399
built around loading and warming that module.

## Context

The head-to-head at `58b0b2ca` measured where Pleris trails Svelte. TodoMVC
receives 141,362 bytes against 20,821, and 96,888 of the 120,541-byte gap is
WebAssembly. `pw-render.wasm` alone was 77,240 bytes as sent, and every page
that renders a block a signal decides downloads it.

The module was a renderer in name and an allocator, a JSON library and a
formatter in bytes. Of its 233 KB of code, `serde_json` was 43 KB, `core`'s
sort and formatting 40 KB, `dlmalloc` 8 KB, and the renderer's own decoder of
the template IR 11 KB. A week's work at that (`wip` commit `b0fdcb4f`,
`no_std`, its own JSON, its own heap) took it to 54 KB as sent, and the rest
is the long tail of a generic renderer's monomorphizations.

The reason for the module was stated where it was built: a second
implementation of a function that escapes is "two things that can disagree, and
the disagreement is silent". That is a true thing to be afraid of. It was
answered by not having a second implementation, at a price that had not then
been measured against anything.

Everything the module does a browser already does, for nothing: it has strings,
maps, a JSON parser and a garbage collector. And each render paid for the
boundary: every request serialized the part and every value to JSON, copied it
into the module's memory, parsed it there, decoded the IR from that, and
rendered. A loop's new row serialized all the page's values again.

## Decision

1. **The browser's renderer is a section of the runtime**
   (`spikes/own-renderer/public/pw-runtime.mjs`, between its markers): the
   request the runtime makes of it is the five `runtime/pw-render` answered,
   `part`, `instance`, `changes`, `attribute` and `captures`, called with the
   page's values as they are and the IR as the page already parsed it.
2. **The disagreement is not silent.** `renderer-agreement.test.mjs`, in every
   run of the suite (`scripts/tests/test_renderer_agreement.py`), asks both the
   same requests, the server's through `src/bin/oracle.rs` in
   `runtime/pw-render-oracle`, and fails on any difference in output or in
   refusal:
   - the compiler's own templates (the store's, kiokun's), with adversarial
     values for every path they read;
   - generated templates of every part kind and context, values of every form,
     and every request about each;
   - every Unicode scalar value through each escaping, and every code point up
     to U+2FFF inside and around a scheme, in two spellings of each reference;
   - each mutation in `scripts/browser_renderer_mutations.py` is killed by it.
3. **What the two may differ in is written down**, and the test holds them to
   everything else:
   - A token: the server's is a keyed hash, under a key a page does not hold,
     and already differed. The browser's is a counter for each instance and
     key, so two are never one by chance, and it is compared by which tokens
     are the same.
   - A number that is no integer, or an integer-valued number outside what an
     `Int` holds, is written as JavaScript writes it. The language shows no
     such number (`compiler/pw-core/tests/template_text.rs` refuses a `Float`
     in a text hole, an attribute and a condition), and `Int` is checked into
     64 bits, so one only passes through what a handler captures.
   - A value holding an unpaired surrogate, which a Rust string cannot hold, is
     rendered as the browser holds it, where the module refused the block.
   - A `$graph` that is no tree is refused where it is read. The server's
     reader refuses the whole request, so a malformed one nobody reads differs.
     It takes a server bug to write one, and it cannot change an escaping.
4. **What existed only because the renderer was a download goes.** A page could
   fail to load it, load it late, or have it trap, so the runtime held an owner
   for it, retired it, and raced a press against its load. There is no such
   failure now, and no such code: `renderModule`, `retireRenderer`,
   `RetiredRenderer`, the ABI, `warm.json`, `renderer-fault.mjs`. `pw-render`'s
   `decode.rs`, a hand-written reader of the IR that existed to keep `serde` out
   of the module, goes with it: nothing reads the IR but serde now.

`runtime/pw-render-wasm` becomes `runtime/pw-render-oracle`, native only.

## Found on the way

Two ways to crash the server, both found by holding the two renderers to each
other, and both fixed here with tests of their own:

- **An address panicked the renderer.** `escape::decode_entities` cut a
  twelve-byte window with a string slice, and panicked where the twelfth byte
  fell inside a character: `&aaaaaaaaaaé;` in any URL-valued attribute, on a
  server's worker, from a value a user wrote. The window is searched as bytes
  now. The agreement test reached it on its first run.
- **A list that came to repeat a key panicked its diff.** `each_changes` walks
  the new items into place, and a key held twice asked it to insert past the
  end. A render refuses a repeated key and the diff did not, so a commit that
  made two keys equal on an open page panicked its thread, while
  `refresh_menu` held a lock that 106 sites expect not to be poisoned. It is
  refused now, as a render refuses it. The review of this work found it, and
  623 of 4,000 random list changes reached it.

The browser's window is counted in characters, not bytes: a body holding a wide
character is no number, and what is no number leaves an `&` at the front of what
is compared, so where its window ends decides nothing.

## Review

An independent review traced the renderer against the Rust and against what the
runtime passes it. No input was found for which the JavaScript writes what the
Rust would have refused or escaped, through replacement patterns, regular
expressions' state, prototype keys, sort order, or integer forms. It found the
list's panic above, that the generator never changed a nested list's keys (it
does now, and `listChanged` has controls), ten mutation anchors the change had
moved, and two small regressions: `revertLists` decoded every pending value on a
page with no list to revert, and a speculation module that arrived after its
press had given up was published. All are repaired.

## Why not the other ways

- **Keep shrinking the WebAssembly.** It reached 54 KB as sent, and the
  remainder is the renderer's own generic code. A renderer in a language with
  its own strings and maps has to ship them.
- **Generate each block's code at build time, as Svelte does.** It would cost
  a page per template, and it is a third thing that must agree with the
  renderer. An interpreter of the IR that a page already carries costs the same
  on every page and has one thing to hold to the server.
- **Render on the server.** A device-only page has no server to ask.

## Result

To be recorded from `just e14-head-to-head` before this is accepted.

## Consequences

- A page that renders nothing in the browser pays for the renderer: about 3 KB
  as sent, in a runtime of about 30. It saves the request and every line that
  managed it, and a page with no signal has no runtime at all (gate 7a).
- The renderer is two implementations, held together by a test rather than by
  construction. A new part kind or context must be added to both, and
  `renderer-agreement.test.mjs`'s generator, or it will not be exercised by
  it: the runtime's `default` branches refuse what they do not know, and the
  test of the compiler's own templates reads whatever the compiler writes.
