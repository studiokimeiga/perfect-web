# Unconfigured Wasmtime disk cache does not join the runtime graph

2026-10-26. Current exact-head audit reports four distinct RustSec advisories: RUSTSEC-2026-0391 (zstd),0398/0403 (zstd-safe),0410 (zstd-sys). This blocks further integration; no advisory is ignored.

The crates.io API and installed normalized upstream manifests show Wasmtime48.0.7 (the current48 release),49.0.4 and50.0.0 all constrain internal-cache to zstd^0.13.0. A patch/minor lock update cannot select corrected zstd0.14.3. Config::new has no disk cache configured, and no workspace Rust source configures CacheConfig. ADR0317's compiled-component cache is a different in-process implementation and remains enabled/unchanged.

Decision before configuration edit: explicitly preserve every48.0.7 default feature except unused `cache`, disable default-features across every direct consumer so additive Cargo feature unification cannot restore it, and update each affected lock. This avoids a custom vendored dependency fork or an unrelated engine major upgrade. It does not remove an implemented persistent-cache capability: Pleris does not configure one. Future disk-cache integration requires a reviewed upstream fix and renewed audit/tests.

Primary checks: crates.io/api/v1/crates/{wasmtime,wasmtime-internal-cache,zstd,zstd-safe,zstd-sys}; registry wasmtime48.0.7 Cargo.toml defaults and internal-cache dependency; bytecodealliance/wasmtime Config/cache implementation; RustSec advisory database used by exact CI40336676059 (licenses/advisories job121620007829). Independent read-only review: existing engine_config/COMPILED path and all consumers inspected before edit.

Validation pending: both lock graphs omit all zstd/internal-cache packages, cargo-deny/current audit, host engine/compiled-component tests, excluded component spike, preflight, independent final diff review and exact published-head CI/quality/verify. No runtime source behavior or passing gate is asserted here.

Local candidate validation: both actual `cargo metadata --locked` resolution graphs omit zstd/zstd-safe/zstd-sys/wasmtime-internal-cache and omit Wasmtime cache while retaining the runtime/component/compiler/GC/async features. Both locks only remove packages (21/25), adding/upgrading none. `just preflight` passes. Host library17, compile-once1, proposal5 and artifact1 tests pass;7 guest-dependent artifact cases remain ignored and receive no passing credit. Excluded spike host `cargo check --locked` passes. `just audit` cannot run locally because cargo-deny is absent; current hosted audit is mandatory, pending, and must not be waived. Exact-commit tests and independent final diff review remain required.


## Canonical exact-source hosted evidence

Source109f459ec3df8b66b48a84f55867c034d9747ca7 passed WIP CI40339069136, including current advisories, then complete manual verify40341658053 (both affected recipes and all3browser jobs). `just evidence-fetch 40341658053` canonically fetched `docs/evidence/E8/artifact-audit.txt` and `docs/evidence/E14/compiled-once.txt`, preserving the actual source/run identity and updating only those2recipe timings. No restamped synthetic or prior-source receipt is used.

The artifact audit ran105host tests with0ignored, including all8real-guest artifact cases. The compiled-once recipe ran its focused/cache/proposal and complete conformance suites, then the complete dev-server memory suite:771passed,9ignored,0failed,1.71GiB peak within its bound. Ignored tests receive no passing credit. Its2legacy controls fail their native tests; this is not relabeled as a strict assertion-provenance campaign. The normal built component's ambient/granted/denied/fuel checks passed locally as separately recorded.

Independent final source review accepted109f's exact feature/lock/source delta. The evidence-only descendant still requires its own WIPCI, independent final metadata review, publication, and all exact published-head CI/quality/CodeQL/Fallow/census/planned verification before landing. No complete-framework or persistent-cache capability is claimed.
