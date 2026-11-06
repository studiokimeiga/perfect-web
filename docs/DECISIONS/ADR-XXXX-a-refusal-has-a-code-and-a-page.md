# ADR-XXXX: a refusal has a code and a page, and the browser's decision carries no words

Status: proposed, 2026-11-06, on branch `feat/refusal-codes-20261106` (a
draft PR), stacked on PR #76 (`perf/first-press-20261106`). Proposed under
the owner's instruction of 2026-11-06, which asked whether Pleris should name
an error by a code that a website explains, as React does, and to do
whatever is best. Not landed; not numbered (PARALLEL.md: the integrator
numbers ADRs). Milestone: E14.

## Context

- **The browser's decision carried words no page read.** `pw-resume`'s
  `decide` wrote a causal trace for every step: "handler identity found",
  "privacy scope is equal or narrower". `pw-resume-wasm` exported the last
  refusal's trace (`last_trace_ptr`, `last_trace_len`). The runtime imports
  `alloc`, `decide_manifest`, `last_recovery`, `know` and `know_document`,
  and never the trace. Its words were 5,651 B of the decision's 57,951 B,
  and 1,732 B of the 21,284 B a page downloads with Brotli.
- **A refusal was told nowhere a developer looks.** The runtime wrote it
  only to `window.__pw.log`, as `refused 5: code 4 recovery reload`. A
  developer whose button did nothing saw nothing in the console. The
  number 4 was explained only in a doc comment of `pw-resume-wasm`.
- **Gate 7b had 29 B left.** It bounds the runtime and the decision together
  at 65,536 B as sent (ADR-0188). After the first press's code-health split
  (PR #76, `d962441b`) they came to 65,507 B.
- **The compiler's diagnostics already have codes and pages.** `pw check`
  prints `[PW0606]`, and `pleris.dev/e/PW0606` explains it (PR #77).
  React's production builds send no message text: they name an error by a
  number and a URL to its message (`react.dev/errors/418`). That is the
  precedent the owner named.

## Decision

1. **The browser's decision compiles no words.**
   - `pw-resume` gains a `trace` feature, on by default.
   - Each step of the trace is now a closure, called only where the feature
     is on. With it off, no step's words or formatting reach the binary.
   - `pw-resume-wasm` depends on `pw-resume` without the feature, and no
     longer exports `last_trace_ptr` or `last_trace_len`.
   - Natively, with the default features, the trace is as it was: in the
     compiler, the tests and the fuzzer.
2. **A refusal's code is named `PWR` and the decision's own number:** from
   PWR02 (hash scheme) to PWR10 (mixed build). The number is the one
   `decide_manifest` already returns, so the console and the decision agree
   with no table between them.
3. **The console tells a developer.**
   - On each refusal the runtime writes `console.warn("Pleris: handler 5 not
     attached: PWR04, https://pleris.dev/e/PWR04, recovery reload")`.
   - The `__pw.log` line beside it is unchanged.
   - A document with no resume manifest gives the decision nothing to read,
     and the console says `no decision`.
   - The words avoid "Refused to", which the page-policy spec reads as the
     browser's own policy report.
4. **The guide explains each code**, in `docs/guide/errors.md` ("When the
   browser refuses a handler"). pleris.dev will give each code its page at
   `/e/PWR04` from that table, as it gives the compiler's codes theirs, once
   this and PR #77 have both landed.

## Alternatives

- **Keep the trace in development builds only.** The development build is
  the one gate 7b measures and the one a developer debugs. Even there, no
  page could read the trace: nothing imported it. A developer reads the
  code's page instead, and the words remain where something reads them:
  the native decision's tests and fuzzer.
- **Format the identities without `format!`.** That means hex and decimal
  written out by hand in `Hash::of`, `HandlerId::derive` and
  `SchemaHash::of_fields`. Measured, it saves 53 B raw and costs 113 B with
  Brotli. A panic brings the formatting machinery in regardless. Rejected.
- **React's scheme for every message of the runtime**, each `log` line a
  number in production. The log is the suite's oracle and the runtime's
  own record. That would be a far larger change, and it is not this one.
- **Put the codes in the compiler's registry**, as `PW9xxx`. The registry is
  the compiler's, and `docs/guide/diagnostics.md`'s column "What the program
  must hold to" does not describe a refusal at run time.

## Measured

Built with `cargo build -p pw-resume-wasm --target wasm32-unknown-unknown
--profile browser`, and compressed with Brotli at quality 11, as
`runtime-size.spec.mjs` computes it, on the development PC on 2026-11-06:

| The decision's WebAssembly | Bytes | Brotli |
|---|---:|---:|
| Before, at `14a3a7f6` | 57,951 | 21,284 |
| Without the trace's words | 52,628 | 19,589 |
| Without its exports too (this ADR) | 52,300 | 19,552 |
| With identities formatted by hand (rejected) | 52,575 | 19,702 |

As the store page downloads them (`runtime-size.spec.mjs`'s gate 7b):

| | Runtime | Decision | Gate 7b |
|---|---:|---:|---:|
| Before, at `14a3a7f6` | 44,223 | 21,284 | 65,507 of 65,536 |
| This ADR | 44,395 | 19,552 | 63,947 of 65,536 |

The console's line costs the runtime 172 B. The decision saves 1,732 B, so
the gate's margin grows from 29 B to 1,589 B.

## Not claimed

- **The codes' pages on pleris.dev.** The site builds them once this and
  PR #77 have both landed. Until then a code's link reaches the site's page
  for an unknown code ("No such code").
- **The decision's words elsewhere.** The native decision keeps them.
- **The decision's other text**, such as a panic's message. `Refusal`'s
  `Display` is unused in the browser, so the linker drops it there.

## Evidence

`docs/evidence/E14/refusal-codes.txt`, from `just e14-refusal-codes`:
- the decision's tests with its trace and without;
- the browser tests in Chromium;
- the mutation controls (`scripts/refusal_codes_mutations.py`).

## Questions for the integrator

1. Is `PWR` the family's right name? It keeps `PW`, as the compiler's codes
   do, with `R` for the runtime.
2. Should a production build warn in the console? React's do, with the code
   and its URL. A refusal on a live site means its documents and its build
   disagree, which the site's developer wants to know.
