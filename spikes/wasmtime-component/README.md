# spike: wasmtime-component

**Charter reference:** §14 Milestone 0 task 10 — *"compile and run one minimal
typed component with one host-provided capability. If WASI 0.3 bindings are not
yet reliable in the tested toolchain, record that and use the stable supported
path."*

## Question

Is the capability boundary described in charter §10.2 (*"a component without an
imported capability cannot use it"*) **real and enforced**, or merely a
convention? And which WASI version does the current toolchain actually deliver?

## Run it

```bash
just spike-wasmtime
```

Evidence: `docs/evidence/E0/spike-wasmtime-component.txt`.

Pinned: `wasmtime` 48.0.8, `wit-bindgen` 0.60.0, Rust 1.97.1, target
`wasm32-wasip2`.

## Layout

```text
wit/store.wit          the typed contract: one capability interface + two worlds
guest/                 std Rust guest       -> 43,837 byte component
guest-minimal/         no_std Rust guest    -> 5,276 byte component
host/                  Rust capability host + 4 checks
```

## Result — the boundary is real

All four gated checks pass against the `no_std` component:

| check | result |
|---|---|
| `ambient` | imports are **exactly** `["perfect-web:store/stores@0.1.0"]` |
| `granted` | `lookup("store_47") == "found:store_47:Blue Bottle"`, host called twice |
| `denied` | instantiation **refused** when the capability is withheld |
| `fuel` | execution traps on a 1,000-unit fuel budget |

The denial diagnostic names the missing capability precisely:

```text
component imports instance `perfect-web:store/stores@0.1.0`, but a matching
implementation was not found in the linker: instance export `read` has the wrong
type: function implementation is missing
```

That satisfies the Milestone 8 gate wording — *"Undeclared capabilities fail
before or at component instantiation with clear diagnostics"* — four milestones
early, for one capability.

## Findings

**F-1 — WASI 0.2, not 0.3, is what the toolchain delivers.** Rust's
`wasm32-wasip2` target emits imports at **`@0.2.9`**. WASI 0.3 was not reachable
through the stable Rust target plus `wit-bindgen` 0.60 in this test. Charter
§10.3 anticipated exactly this and instructs using the stable 0.2 path behind an
adapter; that is now a measured decision rather than an assumption. Recorded as
**ADR-0008**.

**F-2 — `wasm32-wasip2` + Rust `std` injects 14 ambient WASI imports the WIT
world never declared.** The std guest's WIT world declares **one** import. The
compiled component demands **fifteen**:

```text
perfect-web:store/stores@0.1.0        <- the one that was declared
wasi:cli/environment                  ┐
wasi:cli/exit                         │
wasi:cli/std{in,out,err}              │  none of these appear
wasi:cli/terminal-{input,output,...}  ├─ anywhere in store.wit
wasi:clocks/monotonic-clock           │
wasi:io/{error,poll,streams}          ┘
```

They come from Rust's std runtime initialization. This is **ambient authority
arriving through the language runtime**, precisely what charter §14 M8 task 4
requires denying by default. A host that refuses them cannot instantiate the
component at all — safe, but useless.

**F-3 — `no_std` removes the ambient surface completely, at 12% of the size.**
The `no_std` guest imports **only** the declared capability, and is 5,276 bytes
against 43,837 (**8.3× smaller**). Cost: a hand-written `#[global_allocator]`
(a ~25-line bump allocator), a `#[panic_handler]`, and a hand-written
`cabi_realloc` export — `wit-bindgen`'s `realloc` feature supplies one but pulls
in `std`.
*Consequence for Milestone 8:* the compiler's generated components should target
a `no_std`-equivalent runtime, and the build must **assert** the import list
against the declared WIT world rather than trusting it. The `ambient` check in
`host/src/main.rs` is the prototype of that assertion.

**F-4 — component instantiation creates several core wasm instances.**
`StoreLimitsBuilder::new().instances(1)` rejects a perfectly valid component with
`resource limit exceeded: instance count too high at 2`. Milestone 8's per-
component limits must be calibrated against measurements, not intuition.

**F-5 — `wasmtime` 47 has its own `wasmtime::Error`, not `anyhow::Error`.**
`anyhow::Context` does not apply to it. Minor, but it will recur throughout the
Milestone 8 host.

**F-6 — the wasmtime 47 CLI has no `component wit` subcommand.** Import/export
introspection was done through the Rust API (`Component::component_type()` →
`.imports()`). If a CLI path is wanted later, `wasm-tools` is the separate tool
to pin.

## Limitations

- **One capability, one direction.** No resources/handles, no async, no streams.
  Charter §7.6 structured concurrency is untouched.
- The guest is Rust, not Koka. Charter §14 M8 task 7 explicitly permits this, but
  it means the Koka→Component path remains **completely unvalidated**.
- The `denied` check withholds a *declared* import. It does not yet demonstrate
  the more interesting Milestone 8 case: a component that *declares* a capability
  it should not have (`edge component requests payment secret`) being rejected by
  **policy** at build time. `wit/store.wit` defines `payments-secret` and a
  `privileged-store-query` world for that future test; no guest implements it yet.
- Memory limits are configured but not independently exercised; only fuel is.
- No epoch-interruption test (the charter lists "fuel or epoch interruption";
  fuel was sufficient to answer the Milestone 0 question).
- Single-threaded, single-instance. No concurrency or pooling-allocator behavior.
