# ADR-0244: Wasmtime 48.0.5, and a host that enables only what it runs

Status: accepted under the owner's delegation of 2026-10-02. The owner
approved the downloads in this session, and switching the three proposals
off (relayed 2026-10-07). Date: 2026-10-07. Milestones: E8, E14. A
correction: master's CI has been red since 2026-10-05.

## Context

- **Every push to master since 2026-10-05 failed CI** (`ebed28c` through
  `745a972`). Both build-and-test jobs passed; the "licenses and advisories"
  job (`just audit`) failed on three RustSec advisories against the pinned
  Wasmtime 48.0.3:
  - RUSTSEC-2026-0325 (GHSA-cfhf-m2cr-62wj): mis-typed tag imports can
    corrupt the GC heap;
  - RUSTSEC-2026-0326 (GHSA-hw8m-q44c-ggrf): GC values live across a
    `try_call` may go unrooted, corrupting the GC heap;
  - RUSTSEC-2026-0327 (GHSA-32h6-97mm-8q3c, CVSS 9.3): an async-lifted
    callback's result count is unvalidated, a native stack buffer overflow.

  Nothing said so: STATUS recorded the last commit `just ci` passed on,
  locally, and nothing read CI's state.
- **The same job's second half was never reached.** `just audit` runs
  cargo-deny, then the Node audit. With cargo-deny green, the Node audit
  failed on three npm advisories: `braces` (GHSA-vfj7-8cjw-p6xm),
  `source-map-js` (GHSA-68fv-2mgg-jv7q) and `compression`
  (GHSA-vc2v-76pw-4v95).
- **The host had each proposal on, for nothing.** `pw-host` takes
  `wasmtime` with its default features and made each engine with
  `Config::new()` and the component model. Wasmtime 48's default features
  include `gc` and `component-model-async`, and `wasm_gc` and
  `wasm_exceptions` are on by default
  ([`Config`](https://docs.rs/wasmtime/48.0.3/wasmtime/struct.Config.html),
  [features](https://docs.rs/crate/wasmtime/48.0.3/features)). The test
  that says so: Wasmtime's default engine loads a component declaring a GC
  struct or an exception tag. Pleris's components use none of them, and the
  host loads only what its own compiler emits, so an attacker would first
  have to replace a build artifact.
- **This machine's Wasmtime CLI was 47.0.3.** ADR-0116 moved the pin to
  48.0.3 on 2026-10-01, and `just bootstrap` was not run here after it; CI
  bootstraps on each run.

## Research

- **The advisories**
  ([0325](https://rustsec.org/advisories/RUSTSEC-2026-0325.html),
  [0326](https://rustsec.org/advisories/RUSTSEC-2026-0326.html),
  [0327](https://rustsec.org/advisories/RUSTSEC-2026-0327.html)), issued
  2026-10-02: each is patched in `>=48.0.4, <49.0.0` and `>=49.0.2`.
  GHSA-cfhf-m2cr-62wj: exceptions are "on-by-default starting in Wasmtime
  47.0.0". GHSA-32h6-97mm-8q3c's workaround is to disable the
  `component-model-async` feature.
- **The releases**
  ([v48.0.5](https://github.com/bytecodealliance/wasmtime/releases/tag/v48.0.5)):
  48.0.4 fixed eight advisories, these three among them. The other five are
  in Wasmtime's WASI implementations, which no crate here uses: the
  workspace declares `wasmtime-wasi`, and nothing depends on it. 48.0.5
  republished 48.0.4's artifacts, which its CI had failed to publish. On crates.io
  `wasmtime 48.0.5` was published 2026-10-02 and is not yanked. Its release
  assets carry SHA-256 digests on GitHub, from which the CLI is pinned, as
  ADR-0116 pinned 48.0.3's.
- **The Canonical ABI has no GC option yet**: "Currently wasm memory is
  always 32-bit or 64-bit linear memory, but soon GC memory will be added"
  ([CanonicalABI.md](https://github.com/WebAssembly/component-model/blob/main/design/mvp/CanonicalABI.md),
  its GC ABI option is issue #525).
- **The npm advisories**: `source-map-js` is fixed in 1.2.2 and
  `compression` in 1.8.2, each inside the range its dependent declares;
  `braces` has no fixed release (3.0.3 is the latest).

## Decision

1. **Wasmtime 48.0.5**, every live pin: the workspace and its lockfile,
   `pw-host`, `pw-conformance`'s tests, the standalone spike and its
   lockfile, and the bootstrapped CLI on macOS arm64, Linux x86_64 and Linux
   arm64. Cargo moved the release's own crates with it (`cranelift-*`,
   `pulley-*`, `wasmtime-internal-*`, and Wasmtime's `wasmparser` family,
   0.254.0 to 0.254.2); the compiler's own `wasm-tools` crates (0.257.1) are
   untouched. 49 is a major release, and its own decision.
2. **One configuration for every engine the host makes**,
   `engine::engine_config()`: the component model on, and the proposals
   Pleris's components do not use off, so a component that uses one is
   refused when it loads. The conformance tests use it too. Each comes back
   only when the compiler emits code that uses it, checked against that
   release's advisories:
   - **the component model's async** (`wasm_component_model_async`).
     Revisit when ADR-0008's move to WASI 0.3 (`wasm32-wasip3`) comes, for
     concurrent or streamed host calls inside a query. The likeliest to be
     wanted.
   - **GC** (`wasm_gc`). Revisit if the browser's Wasm (E10-T2) or the
     boxed recursive types (ADR-0202) would be smaller or faster on GC
     references than on linear memory and regions. The Canonical ABI
     carries no GC type across a component's boundary yet.
   - **Exceptions** (`wasm_exceptions`). Revisit only if a lowering needs a
     non-local exit that `Result` and a trap cannot express. Resumable
     effect handlers would need stack switching, not exceptions.
3. **The Node audit**: `source-map-js` 1.2.2 and `compression` 1.8.2, in
   their ranges; `braces` accepted in `tools/node-audit-allow.txt` with its
   reason and revisit condition, as `esbuild` is. It reaches the repository
   only through the Marko spike's build globbing.
4. **No advisory is ignored in `deny.toml`.**

## Acceptance

- **`just audit`**: "advisories ok, bans ok, licenses ok, sources ok", and
  the Node audit OK.
- **`runtime/pw-host/tests/engine_proposals.rs`, 5 tests**: a component
  using none of the three loads; one declaring a GC struct, an exception
  tag or an async stream is refused, and the import audit refuses each too;
  Wasmtime's default engine loads the GC and exception components, which is
  what the host did before this.
- **This machine's CLI is 48.0.5**, by `just bootstrap`, which verified the
  archive against the pinned digest.
- **The workspace, 2,159 tests; the browser suite, 782 in three engines**,
  every compiled component run through the restricted engine.
- **CI's three jobs pass** on the push that carries this.
- **The mutation scripts that run the host or the conformance tests are not
  run whole here.** 21 scripts, 146 mutants of them in the scripts that
  plant into `pw-host`; none has an anchor within 30 lines of this change,
  whose test edits change only how an engine is made. They run on the
  push, in ADR-0245's verification.

## Not claimed

- **49.x.**
- **Reading CI's state.** That it went unread for two days is why the
  heavy verification moves to GitHub Actions next, where a red job is the
  first thing read.

## Amended 2026-11-17: the braces and esbuild allowances go

The Node audit accepted `braces` (GHSA-vfj7-8cjw-p6xm) here, beside
`esbuild` (GHSA-g7r4-m6w7-qqqr). Neither is reported any more: GitHub
withdrew the braces advisory on 2026-10-23 (`gh api
/advisories/GHSA-vfj7-8cjw-p6xm`), and the lockfile resolves only esbuild
0.28.2, past the 0.28.1 that patches the other. An allowance for what the
audit no longer reports hides nothing and says what is not so, so both leave
`tools/node-audit-allow.txt`; were either reported again, the audit fails and
says so. `bash scripts/audit-node.sh`: OK, 8 accepted and documented, the 8
it reports.
