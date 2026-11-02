//! spike: wasmtime-component — the capability host.
//!
//! Charter §14 Milestone 0 task 10: "compile and run one minimal typed component
//! with one host-provided capability."
//!
//! Four checks, each printing a machine-greppable `check:<name>=<pass|fail>`
//! line so run.sh can turn them into gate evidence:
//!
//!   1. granted   — the component runs when the host provides `stores`.
//!   2. denied    — the SAME component fails to instantiate when the host does
//!                  not provide `stores`. This is the charter §10.2 claim
//!                  ("a component without an imported capability cannot use it")
//!                  tested rather than asserted.
//!   3. fuel      — execution is interruptible by a fuel budget (§14 M8 task 9).
//!   4. ambient   — the component has NO ambient WASI: no filesystem, clock,
//!                  environment, or network, because none was added to the linker.

use anyhow::{Context, Result};
use wasmtime::component::{Component, HasSelf, Linker};
use wasmtime::{Config, Engine, Store, StoreLimits, StoreLimitsBuilder};

wasmtime::component::bindgen!({
    path: "../wit",
    world: "public-store-query",
});

use perfect_web::store::stores::{Host as StoresHost, Store as WitStore};

/// Everything the host is willing to expose. Anything not reachable from here is
/// not reachable from the guest — the deny-by-default posture charter §14 M8
/// task 4 requires.
struct HostState {
    /// Deterministic fixture data. No clock, no randomness, no I/O.
    stores: Vec<(String, String)>,
    /// Counts capability invocations, so the test can prove the host was
    /// actually called rather than the guest fabricating an answer.
    reads: u32,
    limits: StoreLimits,
}

impl HostState {
    fn new() -> Self {
        Self {
            stores: vec![
                ("store_47".into(), "Blue Bottle".into()),
                ("store_48".into(), "Sightglass".into()),
            ],
            reads: 0,
            // Charter §14 M8 task 9: per-component resource limits.
            // NOTE: a single component instantiation creates several *core* wasm
            // instances, so `instances(1)` rejects even a valid component with
            // "instance count too high at 2". Measured, not guessed.
            limits: StoreLimitsBuilder::new()
                .memory_size(16 << 20) // 16 MiB
                .instances(16)
                .memories(4)
                .build(),
        }
    }
}

impl StoresHost for HostState {
    fn read(&mut self, id: String) -> Option<WitStore> {
        self.reads += 1;
        self.stores
            .iter()
            .find(|(sid, _)| *sid == id)
            .map(|(sid, name)| WitStore {
                id: sid.clone(),
                name: name.clone(),
            })
    }
}

fn engine(fuel: bool) -> Result<Engine> {
    let mut config = Config::new();
    config.wasm_component_model(true);
    if fuel {
        config.consume_fuel(true);
    }
    // NOTE: wasmtime 47 has its own `wasmtime::Error`, not `anyhow::Error`, so
    // `anyhow::Context` does not apply to it. Convert explicitly.
    Engine::new(&config).map_err(|e| anyhow::anyhow!("creating wasmtime engine: {e}"))
}

fn report(name: &str, pass: bool, detail: &str) -> bool {
    println!("check:{name}={}", if pass { "pass" } else { "fail" });
    println!("  {detail}");
    pass
}

// --- 1. capability GRANTED --------------------------------------------------

fn check_granted(wasm: &[u8]) -> Result<bool> {
    let engine = engine(false)?;
    let component = Component::from_binary(&engine, wasm)?;
    let mut linker: Linker<HostState> = Linker::new(&engine);

    // The single capability grant. Removing this line is check 2.
    perfect_web::store::stores::add_to_linker::<_, HasSelf<_>>(&mut linker, |s| s)?;

    let mut store = Store::new(&engine, HostState::new());
    store.limiter(|s| &mut s.limits);

    let bindings = PublicStoreQuery::instantiate(&mut store, &component, &linker)?;

    let hit = bindings.call_lookup(&mut store, "store_47")?;
    let miss = bindings.call_lookup(&mut store, "store_99")?;
    let reads = store.data().reads;

    let pass = hit == "found:store_47:Blue Bottle" && miss == "missing:store_99" && reads == 2;
    Ok(report(
        "granted",
        pass,
        &format!("lookup(store_47)={hit:?} lookup(store_99)={miss:?} host_reads={reads}"),
    ))
}

// --- 2. capability DENIED ---------------------------------------------------

fn check_denied(wasm: &[u8]) -> Result<bool> {
    let engine = engine(false)?;
    let component = Component::from_binary(&engine, wasm)?;

    // A linker with NOTHING in it. The component's `stores` import is unsatisfied.
    let linker: Linker<HostState> = Linker::new(&engine);

    let mut store = Store::new(&engine, HostState::new());
    store.limiter(|s| &mut s.limits);

    match PublicStoreQuery::instantiate(&mut store, &component, &linker) {
        Ok(_) => Ok(report(
            "denied",
            false,
            "SECURITY PROBLEM: component instantiated with an unsatisfied import",
        )),
        Err(e) => {
            // Charter §14 M8 gate: "Undeclared capabilities fail before or at
            // component instantiation with clear diagnostics."
            let msg = format!("{e:#}");
            let names_the_import = msg.contains("stores") || msg.contains("perfect-web:store");
            Ok(report(
                "denied",
                true,
                &format!(
                    "instantiation refused (diagnostic names the missing import: {names_the_import})\n  error: {}",
                    msg.lines().take(3).collect::<Vec<_>>().join(" | ")
                ),
            ))
        }
    }
}

// --- 3. resource limits -----------------------------------------------------

fn check_fuel(wasm: &[u8]) -> Result<bool> {
    let engine = engine(true)?;
    let component = Component::from_binary(&engine, wasm)?;
    let mut linker: Linker<HostState> = Linker::new(&engine);
    perfect_web::store::stores::add_to_linker::<_, HasSelf<_>>(&mut linker, |s| s)?;

    let mut store = Store::new(&engine, HostState::new());
    store.limiter(|s| &mut s.limits);
    // Deliberately far too little fuel to complete a call.
    store.set_fuel(1_000)?;

    // Fuel may run out during instantiation OR during the call — both are valid
    // demonstrations that the budget is enforced, so neither is treated as an
    // error of the check itself.
    let outcome = PublicStoreQuery::instantiate(&mut store, &component, &linker)
        .map_err(|e| format!("{e:#}"))
        .and_then(|b| {
            b.call_lookup(&mut store, "store_47")
                .map_err(|e| format!("{e:#}"))
        });

    match outcome {
        Ok(v) => Ok(report(
            "fuel",
            false,
            &format!("expected a fuel trap, but the call completed with {v:?}"),
        )),
        Err(msg) => {
            let pass = msg.to_lowercase().contains("fuel");
            Ok(report(
                "fuel",
                pass,
                &format!("trapped as expected: {}", msg.lines().next().unwrap_or("")),
            ))
        }
    }
}

// --- 4. no ambient authority ------------------------------------------------

fn check_no_ambient_authority(wasm: &[u8]) -> Result<bool> {
    let engine = engine(false)?;
    let component = Component::from_binary(&engine, wasm)?;

    // Enumerate what the component actually asks the host for. Anything beyond
    // the single declared capability would mean the toolchain injected ambient
    // authority behind our back.
    let mut imports: Vec<String> = Vec::new();
    let ty = component.component_type();
    for (name, _item) in ty.imports(&engine) {
        imports.push(name.to_string());
    }
    imports.sort();

    let only_declared = imports
        .iter()
        .all(|i| i.contains("perfect-web:store/stores"));

    Ok(report(
        "ambient",
        only_declared,
        &format!("component imports = {imports:?}"),
    ))
}

/// Run one check, converting a hard error into a reported failure. A spike that
/// aborts on the first surprise measures nothing.
fn guarded(name: &str, f: impl FnOnce() -> Result<bool>) -> bool {
    match f() {
        Ok(v) => v,
        Err(e) => report(
            name,
            false,
            &format!("check could not run: {}", format!("{e:#}").lines().next().unwrap_or("")),
        ),
    }
}

fn run_component(path: &str, imports_only: bool) -> Result<bool> {
    let wasm = std::fs::read(path).with_context(|| format!("reading component {path}"))?;

    println!("===================================================================");
    println!("component: {path}");
    println!("bytes:     {}", wasm.len());
    if imports_only {
        println!("mode:      OBSERVE — import surface only, not a gate");
    }
    println!("===================================================================");

    // Always measured. For the observe-only component this is the whole point.
    let ambient = guarded("ambient", || check_no_ambient_authority(&wasm));
    println!();
    if imports_only {
        return Ok(true);
    }

    let mut all = ambient;
    all &= guarded("granted", || check_granted(&wasm));
    println!();
    all &= guarded("denied", || check_denied(&wasm));
    println!();
    all &= guarded("fuel", || check_fuel(&wasm));
    println!();
    Ok(all)
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        anyhow::bail!(
            "usage: spike-wasmtime-host <component.wasm> [observe:<component.wasm> ...]\n\
             \n\
             A bare path is gated: every check must pass.\n\
             An `observe:` path is measured for its import surface only."
        );
    }

    println!("wasmtime crate: 48.0.8");
    println!();

    let mut any_failed = false;
    for a in &args {
        let (path, imports_only) = match a.strip_prefix("observe:") {
            Some(p) => (p, true),
            None => (a.as_str(), false),
        };
        // A component that cannot satisfy its imports is a *result*, not a crash.
        match run_component(path, imports_only) {
            Ok(true) => {}
            Ok(false) => any_failed = true,
            Err(e) => {
                println!("component {path}: could not be evaluated: {e:#}");
                any_failed = true;
            }
        }
    }

    println!(
        "result: {}",
        if any_failed {
            "SOME GATED CHECKS FAILED (see per-component output above)"
        } else {
            "ALL GATED CHECKS PASSED"
        }
    );
    if any_failed {
        std::process::exit(1);
    }
    Ok(())
}
