# Technology matrix

Charter §14 Milestone 0 task 4 specifies the columns: project, useful idea,
maturity, license, extension points, known limitations,
reuse/fork/tape/build decision, deletion or migration condition.

**Verification status.** Rows marked **measured** were exercised by a Milestone 0
spike on this machine; the evidence file is named. Rows marked *read* come from
primary documentation only and have **not** been run here — charter §3.3 forbids
presenting the two as equivalent. Versions verified against upstream release
APIs and the npm registry on **2026-08-05**; see `version-verification.md`.

Strategy vocabulary is charter §4: **reuse permanently** · **tape together
temporarily** · **fork only after evidence** · **build from scratch**.

---

## Measured in Milestone 0

| project | useful idea | maturity | license | extension points | known limitations (measured) | decision | deletion / migration condition |
|---|---|---|---|---|---|---|---|
| **Koka 3.2.3** | inferred effect rows, handlers, ADTs, `maybe`, Perceus | research language, actively released (v3.2.3, 2026-03-18) | Apache-2.0 | `--target=js` ES-module output; `.kki` interface files carry signatures **with effect rows**, spans, ctor tags | Exhaustiveness enforced **only** for `exn`-free functions — a partial match in an `exn` function compiles and fails at runtime (F-8). Single-field `value struct`s **erased** to their payload, so `Money<USD>` is a bare int at the JS boundary (F-4). `Nothing` and `Nil` are both `null`, runtime-indistinguishable (F-7). `is_*` predicates are discriminators, not validators (F-5). No generic `Result<T,E>` (F-3). `.kki` format is internal/unstable. Async on the JS target untested. | **tape** (ADR-0001) | Milestone 9 checker accepts the accepted corpus and rejects the rejected corpus with differential agreement |
| **Marko 6.3.32** | streaming, resumability, zero-JS static output, interaction-lazy code | stable, `latest` on npm, very actively released | MIT | `@marko/run` 0.11.8 + `@marko/vite` + adapters; template compilation | "Zero JS" is exact for a fully static route (0 script tags) but the **streamed** route carries 849 B of *inline* patch shim (F-2). Client runtime splits into a shared 3,745 B chunk + ~176 B per route (F-6). Syntax traps: `--` is a text line not a comment; a top-level `>` in an attribute silently truncates the tag (F-8). | **tape** (ADR-0002) | Milestone 7 renderer passes the renderer-independent golden suite |
| **Wasmtime 48.0.7** | typed component interfaces, host-granted capabilities, fuel/epoch limits | stable | Apache-2.0 WITH LLVM-exception | `wasmtime::component::bindgen!`, `Linker`, `StoreLimits`, `set_fuel` | CLI has no `component wit` subcommand — introspection is API-only (F-6). Its `wasmtime::Error` is not `anyhow::Error` (F-5). One component instantiation creates several core instances, so naive `instances(1)` limits reject valid components (F-4). | **reuse** (ADR-0006) | a required capability cannot be expressed in WIT |
| **WIT / Component Model / WASI 0.2** | capability-secure typed component boundaries | WASI 0.2 stable; imports resolve at `@0.2.9` | Apache-2.0 WITH LLVM-exception | WIT worlds, `wit-bindgen` 0.60.0 | **WASI 0.3 not reachable** through the stable Rust target (ADR-0008). A `std` Rust guest imports **15** WASI instances when its world declares **one** — ambient authority from the language runtime, removable only via `no_std` (F-2, F-3). | **reuse** (ADR-0006, ADR-0008) | Rust ships a stable `wasm32-wasip3` target |
| **Rust 1.97.1** | ADTs, `Option`/`Result`, exhaustive enums, affine resources, strong diagnostics | stable (2026-07-14) | MIT OR Apache-2.0 | rustup, `rust-toolchain.toml`, `wasm32-wasip2` | Ownership syntax must **not** leak into `pw` application code (charter §5). Lossless-tree design for `pw fmt` still open. | **reuse + build on** (ADR-0003) | a dependency MSRV exceeds the pin |
| **rustls 0.23.46 (ring 0.17.14)** | TLS 1.3 and 1.2 for the host's outbound calls, its certificates verified by rustls-webpki against the platform's roots (rustls-native-certs 0.8.4) | stable, very actively maintained; 0.23.46 fixes four advisories | Apache-2.0 OR ISC OR MIT (ring: Apache-2.0 AND ISC; rustls-webpki, untrusted: ISC) | `ClientConfig::builder_with_provider`, `ClientConnection`, `rustls::Stream`; a `CryptoProvider` | The ring provider offers no post-quantum key exchange (X25519, P-256, P-384 alone). No revocation is checked (no CRL or OCSP). The roots are the machine's, read at start. | **reuse** (ADR-0387, outbound-https) | a provider that needs what ring lacks (post-quantum key exchange): aws-lc-rs, a ruling of its own |
| **annotate-snippets 0.12.16** | rustc-grade diagnostic rendering | stable, used by rustc | MIT OR Apache-2.0 | `AnnotationKind::Primary` / `::Context`, `Renderer::plain()` | Column mapping untested against non-ASCII / wide characters. | **reuse** (ADR-0009) | it cannot express a needed diagnostic shape |
| **Node 22.21.1 / pnpm 10.15.1 / Vite 8.2.0** | dev host and bundling for the Marko path | Node 22 is maintenance LTS (24 is active) | MIT | adapters, plugins | Node 22 rather than current LTS 24 (assumption A-002). Node is a temporary host per charter §10.1. | **tape** | Milestone 8: the Rust host owns privileged I/O |

---

## Read only — not yet exercised here

These informed the design but **no code was run** in Milestone 0. Treat every
claim as upstream's, not ours.

| project | useful idea to borrow | maturity | license | extension points | what we must not assume | decision | condition |
|---|---|---|---|---|---|---|---|
| **Elm** | pure view; commands/subscriptions as distinct primitives; decoders | stable, slow-moving | BSD-3-Clause | none needed — conceptual donor | that its full-stack/deployment model is complete | **build** (borrow the shape) | n/a |
| **Gleam** | small readable syntax, friendly inference, explicit failure | stable | Apache-2.0 | conceptual donor | that BEAM/JS targets or its effect model solve this project | **build** (borrow the shape) | n/a |
| **Effekt** | scoped capabilities, effect safety | research | MIT | conceptual donor | that it is the runtime or web stack to ship | **build** | revisit if Koka's effect model proves insufficient |
| **Unison** | content-addressed code; abilities | young | MIT | conceptual donor | that its deployment model should be copied | **build** (content-addressed *handlers* only, §8.5) | n/a |
| **Roc** | platform-owned capabilities, narrow application API | pre-1.0 | UPL-1.0 | conceptual donor | that compiler maturity or effect system fits now | **build** | n/a |
| **Links** | one language split across browser/server/database; typed RPC | research | **GPL** | **research reference only** | — charter §3.6: **do not copy implementation code into a permissive core** | **study only** | never vendored |
| **Ur/Web** | compile-time web-safety guarantee checklist | research | MIT | conceptual donor | that its implementation should be reused | **study only** | n/a |
| **Skip** | effects tied to safe memoization and incremental invalidation | dormant | MIT | conceptual donor | that it supplies UI or server platform | **study only** | informs Milestone 6 |
| **Jane Street Incremental** | a stable dependency DAG, cutoffs, stabilization, incremental recomputation of arbitrary derived values | mature, in production at Jane Street | MIT | OCaml library; `Incr.Var`, `Incr.map`, `Incr.observe`, `Incr.stabilize` | OCaml-only. Charter §5 warns against assuming "its OCaml implementation should become the permanent cross-target runtime". | **study only** — borrow the DAG/cutoff/stabilize semantics into the own resource graph (M4/M6) | never vendored; it is a design donor, not a dependency |
| **Bonsai / Bonsai_web** | purely functional state machines, static computation DAG, lifecycle/scoping, strong UI expect tests | mature (opam v0.16/v0.17) | MIT | OCaml/opam | Charter §5 warns against assuming "its virtual-DOM diff/patch loop, Js_of_ocaml target, or lifecycle APIs prevent forced layout or provide SSR/resumption/placement" — and the layout spike confirms a vdom diff does nothing about forced synchronous layout | **study only** — not the permanent renderer | never vendored |
| **Svelte / SvelteKit** | SFC ergonomics, scoped CSS, compiled targeted updates, typed remote functions | stable | MIT | benchmark baseline (§18.1) | that JS semantics, generic `$effect`, hydration and manual placement are ideal | **baseline** | Milestone 3 benchmark |
| **Qwik** | serialized resumption, interaction-lazy code | stable | MIT | conceptual donor | that arbitrary closure serialization is automatically safe | **study** | Milestone 7 informs |
| **Phoenix LiveView** | HTML-first server state, pushed diffs | stable | MIT | conceptual donor | that constant network dependence suits every interaction | **study only** | n/a |
| **Convex** | reactive queries with tracked dependencies | commercial | FSL/Apache mix | conceptual donor | that the storage platform should be mandatory | **study only** | n/a |
| **Materialize / differential dataflow** | incremental view maintenance | mature | Apache-2.0 / BSL | conceptual donor | that a distributed dataflow engine belongs in a first prototype | **study only** | possible Milestone 6+ |
| **Servo** | modular Rust browser engine, embedding | experimental | MPL-2.0 | embedding API | that it can replace production browsers | **defer** — no fork before profiling (§11.4) | Milestone 13, only after measurement |
| **WICG declarative partial updates** | native document range patches, out-of-order streaming | **proposal** | W3C | none yet | that a proposal is standardized | **watch** | Milestone 12/13 |
| **TC39 Signals** | low-level reactive primitives | **proposal** | — | none yet | that a JS proposal is a complete UI model | **watch** | n/a |
| **Lima** | reproducible arm64 Linux VMs on macOS | stable, v2.2.0 | Apache-2.0 | YAML VM definitions, `vz`/virtiofs | charter VM sizing assumes 64 GB; this host has 16 GiB (A-001) | **reuse** | Milestone 11 |
| **Playwright 1.62.1** | Chromium/WebKit/Firefox automation | stable | Apache-2.0 | test runner | automated a11y checks cannot prove accessibility (§17.4) | **reuse** | Milestone 3 |
| **SQLite / PostgreSQL** | ordinary transactions, transactional outbox | mature | public domain / PostgreSQL | SQL, adapters | that we should build a database (§2 forbids it) | **reuse** (ADR-0005) | PostgreSQL at Milestone 11 |

---

## License summary

Everything in the **implementation** path is permissive and compatible with the
intended dual MIT/Apache-2.0 core:

```text
Rust toolchain            MIT OR Apache-2.0
annotate-snippets 0.12.16 MIT OR Apache-2.0
Koka 3.2.3                Apache-2.0        (build-time tool, not linked)
Wasmtime 48.0.7           Apache-2.0 WITH LLVM-exception
wit-bindgen 0.60.0        Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT
rustls 0.23.46            Apache-2.0 OR ISC OR MIT
ring 0.17.14              Apache-2.0 AND ISC
rustls-webpki 0.103.16    ISC
untrusted 0.9.0           ISC
additional Rust transitive BSD-2-Clause / BSD-3-Clause / Zlib
marko 6.3.32              MIT
@marko/run 0.11.8         MIT
@marko/vite 6.1.9         MIT
vite 8.2.0                MIT
Playwright 1.62.1         Apache-2.0
Lima 2.2.0                Apache-2.0
```

**One GPL exposure, deliberately contained:** **Links** is GPL. Charter §3.6
permits it as a *research reference and test-corpus inspiration only*. No Links
code is or may be vendored, copied, or adapted into this repository.

Automated Rust license and vulnerability checking runs in CI through `cargo-deny`.
The allowlist remains explicit and permissive-only; advisories remain deny-by-default.
