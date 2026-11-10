# ADR-0410: the decision runs on what it needs

Status: accepted 2026-11-10, landed with train-16. Proposed 2026-11-09, on
branch `perf/resume-wasm-size-20261109`, under the owner's delegation of
2026-10-02 and order of 2026-10-25, and of 2026-11-09 ("continue until its
literally perfect"). Milestone: E14 (ADR-0392's first-load losses).

## Context

Every interactive page downloads `pw-resume.wasm` before a handler can attach.
It decides whether a document's handlers may run on this build, one
implementation with the server's (ADR-0132).

- It was 52,300 bytes, 19,552 as sent (Brotli 11): more than the whole 8,692-byte
  gap between the store's first load and SvelteKit's (39,157 against 30,465).
- Most of it was not the decision. Read from the module's own symbols: `dlmalloc`
  (7 KB), a stable and an unstable sort for lists that hold one item (6.6 KB),
  `std`'s panic and formatting machinery, UTF-8 repair, and paths and messages for
  panics nobody can read.

## Decision

1. **The browser's build is `no_std`** (`pw-resume-wasm`, on `wasm32` only: its
   native tests and every native build are as they were).
   - A heap that only grows. A decision's values are its own and are given back
     together when it is answered, so a page that decides for many handlers uses
     no more than it needs for one.
   - A panic stops the module. The runtime already treats a decision that does
     not answer as a decision not made.
   - One thread, so a lock is a cell.
2. **`pw-resume` is `no_std` everywhere**, using `alloc`, and writes its hashes'
   text by hand where the browser's build reaches them: no `format!`, and a
   dependency set kept sorted as it is built rather than sorted after. The text
   each identity hashes is unchanged, and `tests/identity.rs` states it by hand: a
   build whose identities differed would refuse every open tab.
3. **What the page wrote is read as UTF-8 or as nothing.** `TextEncoder` writes
   the manifest, so anything else is not one: it is read as empty, and refused
   as any malformed manifest is. Until now it was repaired and read.

The decision's logic does not change, and neither does its ABI.

## Result

`pw-resume.wasm` is 16,145 bytes, 6,620 as sent (`e2e/runtime-size.spec.mjs`,
`EVIDENCE decision-wasm-bytes`), a 66% cut, on every interactive page. No new
dependency.

## Held by

- `runtime/pw-resume/tests/identity.rs`: each hash's text as written out by hand,
  FNV-1a's published values, leading zeroes, a set's order and repeats, and numbers
  of more than one digit.
- `pw-resume-wasm`'s and `pw-resume`'s existing tests, unchanged, and every browser
  suite that resumes a handler.
- `e2e/runtime-size.spec.mjs`: its size as sent is bounded, and 20,000 decisions
  grow its memory by nothing.
- `scripts/refusal_codes_mutations.py`: a decision that keeps what it allocated,
  and a sort put back.
