# ADR-XXXX: every page decides in its runtime

Status: proposed 2026-11-09, on branch `perf/decision-in-js-20261109`, under the
owner's delegation of 2026-10-02 and order of 2026-10-25 ("deciding by best
judgement without asking"), and the order of 2026-11-09 to work the measured
gaps with Svelte largest first. Milestone: E14. Follows the device runtime's
ADR, whose third decision this generalizes, and amends the decision's ADRs
(E7V, ADR-0132, ADR-0300, ADR-0400) as to where a page's decision runs.

## Context

Every page that attaches a handler fetched `pw-resume.wasm`: 16 KB, 6.6 KB as
Brotli sends it, a request of its own and a compile before its first press, and
its policy allows `'wasm-unsafe-eval'` for no other reason. The renderer's ADR
took 77 KB of WebAssembly out of a page by holding a page of JavaScript to the
Rust renderer on every run of the suite.

The device runtime's ADR did the same for the decision, for pages that talk to
no server, and found the decision is few lines: what the module hashes is what
the table holds as text. It was held to the real module over every single and
pairwise change to a manifest's nine fields and twelve tables, including the
scopes only a page with a server has. There is nothing left about a page with a
server that the module answers and those lines do not.

## Decision

1. **Every page decides with the runtime's `decider`**, from the build's table
   and the document's manifest, and answers with the same code and recovery.
2. **No build holds `pw-resume.wasm`, no head asks for it, and no policy allows
   WebAssembly.** `script-src` is `'self'` and the hash of the script that
   starts the runtime.
3. **`pw-resume` stays the specification, and its WebAssembly the oracle.**
   `decision-agreement.test.mjs` builds the crate as the module pages used to
   fetch, asks it and those lines the same questions, and fails on any
   difference. A change to the decision is made in Rust, where its tests and its
   property tests are, and then in those lines, or the suite is red.

The decision's rule is unchanged: it fails closed. A table that cannot be read
knows no handler, and each is refused.

## Why not the other ways

- **Keep the module on pages with a server.** Every one of those pages would pay
  6.6 KB, a request, a compile and a wider policy for an answer two lines of
  JavaScript give. Its one advantage was that the proof of attachment,
  `Authorised`, is checked by Rust's types; in a browser it is the page's
  JavaScript that attaches, on the code it is told, either way.
- **Compile the crate to JavaScript.** A hash-based identity and a heap would
  ship to compare two strings.

## Result

To be recorded from `just e14-head-to-head` before this is accepted.

## Consequences

- Two implementations of the decision, as there are of the renderer, and for the
  same reason. The test above is what makes that safe.
- A page's first press no longer waits on a module's compile.
- `pw-resume-wasm` is built only by the tests that use it. Its size no longer
  reaches a reader, so ADR-0400's bounds on it are not measured again.
