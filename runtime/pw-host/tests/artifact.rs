//! The audit, against real components.
//!
//! Architect ruling, 2026-08-07: *"final artifact audit […] adversarial guests
//! attempting undeclared interfaces"*.
//!
//! Everything in `admission.rs` is decided on data, and it must be — a
//! capability decision that needed a Wasm engine to be checked is a decision
//! nobody runs. But the data has to come from somewhere, and the point of an
//! ARTIFACT audit is that the artifact can disagree with the source. So this
//! file reads the imports out of two real components built by
//! `just spike-wasmtime`, and puts them through the same `admit`.
//!
//! # The adversarial guest is not synthetic
//!
//! It is the ordinary one. `spikes/wasmtime-component/guest` declares one
//! import in its WIT world and demands fifteen, because Rust `std` on
//! `wasm32-wasip2` injects fourteen `wasi:*` interfaces during runtime
//! initialization — measured in `docs/evidence/E0/spike-wasmtime-component.txt`
//! and recorded as that spike's finding F-2.
//!
//! Nobody wrote that guest to attack anything. That is what makes it the right
//! adversary: undeclared authority arrives by *default*, from the toolchain,
//! without a line of application code asking for it, and a host that trusted
//! the source would have granted all fourteen.
//!
//! Requires the `engine` feature and the built components:
//!
//! ```text
//! just spike-wasmtime      # builds the guests
//! just e8-host             # runs this, with --include-ignored
//! ```
//!
//! Each test that reads a guest is `#[ignore]`d, with that reason. Since E10-I
//! the development server runs compiled commands, so it depends on `pw-host`
//! with `engine`, and Cargo unifies that feature into every
//! `cargo test --workspace`. That made this file part of `just ci` on a
//! checkout where nothing had built the guests, and on 2026-09-25 CI failed
//! on exactly that (run 36095701735). An ignored test is listed as ignored,
//! with its reason; a `cfg` would make it vanish from the output.

#![cfg(feature = "engine")]

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use pw_host::*;

fn guest(dir: &str, file: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../spikes/wasmtime-component")
        .join(dir)
        .join("target/wasm32-wasip2/release")
        .join(file)
}

/// Read a component's imports, failing loudly.
///
/// Never a default of "no imports": that passes every audit, so a failure to
/// read must not be able to look like a clean component.
fn imports(path: PathBuf) -> Vec<String> {
    let bytes = std::fs::read(&path)
        .unwrap_or_else(|e| panic!("{}: {e}\nrun `just spike-wasmtime` first", path.display()));
    pw_host::engine::imports_of(&bytes).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// The contract the minimal guest's WIT world corresponds to: one capability.
fn contract(imports: Vec<Import>) -> ComponentContract {
    ComponentContract {
        component_id: "spike.Store".into(),
        capability_mapping: pw_host::CAPABILITY_MAPPING,
        abi_schema: "abi".into(),
        required_capabilities: vec![Capability {
            family: "store".into(),
            operation: "read".into(),
            argument: None,
        }],
        allowed_placements: vec!["origin".into()],
        imports,
        exports: vec![Export {
            name: "lookup".into(),
            kind: "query".into(),
            binding: BindingSupport::default(),
            // Its place in the component: a function the world exports
            // directly. Since 2026-10-01 a call reaches only an export whose
            // place the contract names (`authorization_for`), and this one
            // named none: the granted call below was refused, and no run saw
            // it, every test here being ignored where the guests are not
            // built, as no verification runner had them (ADR-0026, amended
            // 2026-10-11).
            component: Some(ComponentExport {
                interface: String::new(),
                function: "lookup".into(),
                authorization: vec![],
                idempotent_by: None,
                invoked_by: None,
                runs_for: vec![],
                bounded: vec![],
            }),
        }],
    }
}

fn topology() -> Topology {
    Topology {
        nodes: vec![Node {
            name: "origin-1".into(),
            world: "origin".into(),
            grants: BTreeSet::from(["store.read".to_string()]),
        }],
    }
}

#[test]
#[ignore = "reads the guests `just spike-wasmtime` builds; `just e8-host` runs it"]
fn the_declared_component_imports_exactly_what_its_world_says() {
    let actual = imports(guest("guest-minimal", "spike_wasmtime_guest_minimal.wasm"));
    println!("minimal guest imports: {actual:?}");
    assert!(
        actual
            .iter()
            .all(|i| i.starts_with("perfect-web:store/stores")),
        "the no_std guest declares one interface and demands only that: {actual:?}"
    );

    // And the audit admits it, because the contract allows what it asks for.
    let allowed: Vec<Import> = actual
        .iter()
        .map(|i| {
            let (interface, name) = i.split_once('#').unwrap_or((i.as_str(), "use"));
            Import {
                interface: interface.to_string(),
                name: name.to_string(),
                capability: "store.read".into(),
                kind: ImportKind::HostCapability,
                bounded: Vec::new(),
                event: None,
                invalidates: None,
                every: Vec::new(),
                reads: None,
                schedules: None,
            }
        })
        .collect();
    let c = contract(allowed);
    let a = admit(&c, &topology(), "origin-1", &actual);
    assert!(a.is_admitted(), "refused: {:?}", a.refusals());
}

#[test]
#[ignore = "reads the guests `just spike-wasmtime` builds; `just e8-host` runs it"]
fn the_std_component_is_refused_for_authority_nobody_asked_for() {
    // The adversarial case, and it is the ordinary build. Fourteen `wasi:*`
    // interfaces the WIT world never mentions, injected by std's runtime
    // initialization.
    let actual = imports(guest("guest", "spike_wasmtime_guest.wasm"));
    println!("std guest imports {} interfaces", actual.len());
    assert!(
        actual.len() > 1,
        "if std stopped injecting imports this test measures nothing: {actual:?}"
    );

    // The allowed set is the OTHER BUILD OF THE SAME WORLD.
    //
    // Both guests declare `perfect-web:store/stores` and nothing else, so what
    // the no_std build imports is exactly what this world legitimately needs —
    // including the resource types an interface brings with it, which a
    // hand-written allow-list would have got wrong and did.
    //
    // Same world, same contract, two builds. Everything the std build asks for
    // beyond the other one is authority the world never mentioned.
    let declared = imports(guest("guest-minimal", "spike_wasmtime_guest_minimal.wasm"));
    let c = contract(
        declared
            .iter()
            .map(|i| {
                let (interface, name) = i.split_once('#').unwrap_or((i.as_str(), "use"));
                Import {
                    interface: interface.to_string(),
                    name: name.to_string(),
                    capability: "store.read".into(),
                    kind: ImportKind::HostCapability,
                    bounded: Vec::new(),
                    event: None,
                    invalidates: None,
                    every: Vec::new(),
                    reads: None,
                    schedules: None,
                }
            })
            .collect(),
    );
    let a = admit(&c, &topology(), "origin-1", &actual);

    let Some(Refusal::UndeclaredImport { imports }) = a.refusals().first() else {
        panic!("expected an import refusal, got {:?}", a.refusals());
    };
    println!("undeclared: {imports:?}");
    assert!(
        imports.iter().any(|i| i.starts_with("wasi:")),
        "the refusal names the ambient WASI interfaces"
    );
    assert!(
        !imports.iter().any(|i| i.starts_with("perfect-web:store")),
        "and does not name the interface the world DID declare"
    );
    assert!(
        imports.len() >= 14,
        "every undeclared interface is named, not just the first: {imports:?}"
    );
}

#[test]
fn a_component_that_cannot_be_read_is_not_treated_as_importing_nothing() {
    // The failure mode that would make every test above vacuous. `imports_of`
    // must error rather than return an empty list, because an empty list
    // passes every audit.
    let err = pw_host::engine::imports_of(b"not a wasm component")
        .expect_err("garbage is not a component");
    assert!(!err.is_empty());
}

// --- typed linking, and the engine's own refusal -----------------------------

/// **A component instantiates with exactly what it was granted, and not
/// otherwise.**
///
/// E8 gate item: *"typed linking only from a `Granted`. A component whose
/// contract omits an import fails to instantiate, with the engine's own
/// diagnostic rather than ours."*
///
/// Both halves against a real component and a real engine, because the whole
/// claim is about what wasmtime does — a data-level check would be a second
/// implementation of instantiation's own rule, and the two would agree until a
/// component imported something the check did not model.
#[test]
#[ignore = "reads the guests `just spike-wasmtime` builds; `just e8-host` runs it"]
fn a_granted_component_instantiates_and_an_ungranted_one_does_not() {
    let path = guest("guest-minimal", "spike_wasmtime_guest_minimal.wasm");
    let bytes = std::fs::read(&path)
        .unwrap_or_else(|e| panic!("{}: {e}\nrun `just spike-wasmtime` first", path.display()));

    let actual = imports(path);
    let allowed: Vec<Import> = actual
        .iter()
        .map(|i| {
            let (interface, name) = i.split_once('#').unwrap_or((i.as_str(), "use"));
            Import {
                interface: interface.to_string(),
                name: name.to_string(),
                capability: "store.read".into(),
                kind: ImportKind::HostCapability,
                bounded: Vec::new(),
                event: None,
                invalidates: None,
                every: Vec::new(),
                reads: None,
                schedules: None,
            }
        })
        .collect();
    let c = contract(allowed);

    // Admitted, so it holds `store.read`, so `linkable` covers its one import.
    let admission = admit(&c, &topology(), "origin-1", &actual);
    let granted = Granted::from(&admission, &BTreeMap::new()).expect("admitted");
    let linked = pw_host::engine::instantiate(&bytes, &c, &granted)
        .unwrap_or_else(|e| panic!("a granted component must instantiate: {e}"));
    println!("linked: {linked:?}");
    assert!(!linked.is_empty(), "and something was actually linked");

    // **The refusal.** Same artifact, same node, a contract that does not
    // require `store.read` — so nothing is granted, so nothing is linked, so
    // the import is unresolvable. The message is wasmtime's.
    let mut ungranted = c.clone();
    ungranted.required_capabilities.clear();
    let admission = admit(&ungranted, &topology(), "origin-1", &actual);
    let granted = Granted::from(&admission, &BTreeMap::new()).expect("placement still admits it");
    assert_eq!(granted.count(), 0, "no capability, no handle");
    let err = pw_host::engine::instantiate(&bytes, &ungranted, &granted)
        .expect_err("an unlinked import must not instantiate");
    println!("refused by the engine: {err}");
    assert!(
        err.contains("perfect-web:store/stores"),
        "and the engine names the import it could not resolve: {err}"
    );
}

/// A refused admission cannot be instantiated at all.
///
/// The direction that matters most: there is no `Granted` to link from, so
/// there is nothing to call `instantiate` with. Stated as a test because
/// "cannot" is a claim about the API's shape, and an `Option` that some caller
/// unwraps with a default would break it silently.
#[test]
#[ignore = "reads the guests `just spike-wasmtime` builds; `just e8-host` runs it"]
fn a_refused_admission_yields_no_granted_to_link_from() {
    let path = guest("guest-minimal", "spike_wasmtime_guest_minimal.wasm");
    let actual = imports(path);
    let allowed: Vec<Import> = actual
        .iter()
        .map(|i| {
            let (interface, name) = i.split_once('#').unwrap_or((i.as_str(), "use"));
            Import {
                interface: interface.to_string(),
                name: name.to_string(),
                capability: "store.read".into(),
                kind: ImportKind::HostCapability,
                bounded: Vec::new(),
                event: None,
                invalidates: None,
                every: Vec::new(),
                reads: None,
                schedules: None,
            }
        })
        .collect();
    let c = contract(allowed);

    // A node in the wrong world. Placement refuses, and there is no override.
    let browser = Topology {
        nodes: vec![Node {
            name: "laptop".into(),
            world: "browser".into(),
            grants: BTreeSet::from(["store.read".to_string()]),
        }],
    };
    let admission = admit(&c, &browser, "laptop", &actual);
    assert!(!admission.is_admitted());
    assert!(
        Granted::from(&admission, &BTreeMap::new()).is_none(),
        "a refusal yields no capability set, and therefore no linker"
    );
}

// --- resource limits, per instance and from policy ---------------------------

/// **A budget the deployment declares, enforced by the engine.**
///
/// E8 gate item: *"fuel and memory limits per instance, driven by policy rather
/// than a constant."* E0's `check:fuel` proved wasmtime enforces a fuel budget.
/// What a spike cannot say is where the number comes from — and a limit
/// compiled into the host is one nobody can raise for a component that
/// legitimately needs more.
#[test]
#[ignore = "reads the guests `just spike-wasmtime` builds; `just e8-host` runs it"]
fn an_instance_runs_within_the_budget_its_deployment_declares() {
    let path = guest("guest-minimal", "spike_wasmtime_guest_minimal.wasm");
    let bytes = std::fs::read(&path)
        .unwrap_or_else(|e| panic!("{}: {e}\nrun `just spike-wasmtime` first", path.display()));
    let actual = imports(path);
    let allowed: Vec<Import> = actual
        .iter()
        .map(|i| {
            let (interface, name) = i.split_once('#').unwrap_or((i.as_str(), "use"));
            Import {
                interface: interface.to_string(),
                name: name.to_string(),
                capability: "store.read".into(),
                kind: ImportKind::HostCapability,
                bounded: Vec::new(),
                event: None,
                invalidates: None,
                every: Vec::new(),
                reads: None,
                schedules: None,
            }
        })
        .collect();
    let c = contract(allowed);
    let admission = admit(&c, &topology(), "origin-1", &actual);
    let granted = Granted::from(&admission, &BTreeMap::new()).expect("admitted");

    // A generous budget: it instantiates, and the cost is REPORTED rather than
    // assumed. A host that could not say what an instance spent could not set
    // the next budget from evidence.
    let generous = Limits {
        fuel: Some(10_000_000),
        memory_bytes: Some(64 * 1024 * 1024),
        table_elements: Some(10_000),
    };
    let out = pw_host::engine::instantiate_within(&bytes, &c, &granted, &generous)
        .unwrap_or_else(|e| panic!("a generous budget must not refuse: {e}"));
    let used = out.fuel_used.expect("a fuel budget reports what it spent");
    println!("instantiation spent {used} fuel");
    assert!(used > 0, "instantiation is not free");

    // A budget below what instantiation costs. The trap is the engine's, and
    // the number came from the measurement above rather than from a guess —
    // which is the whole difference between a policy and a constant.
    let starved = Limits {
        fuel: Some(1),
        ..generous.clone()
    };
    let err = pw_host::engine::instantiate_within(&bytes, &c, &granted, &starved)
        .expect_err("a starved instance must not run");
    println!("refused for fuel: {err}");
    assert!(
        err.to_lowercase().contains("fuel"),
        "and the engine says why: {err}"
    );

    // The control that keeps both halves meaningful: unbounded is a real
    // answer and not the only one that works.
    let unbounded = pw_host::engine::instantiate_within(&bytes, &c, &granted, &Limits::unbounded())
        .expect("unbounded instantiates");
    assert_eq!(
        unbounded.fuel_used, None,
        "nothing is metered when nothing is bounded"
    );
    assert!(!Limits::unbounded().is_bounded());
    assert!(generous.is_bounded());
}

/// A memory ceiling is refused growth, not a crashed host.
///
/// The direction charter §7.10 asks for at every boundary: the guest sees an
/// allocation failure it can handle. A host that aborted would turn one
/// component's appetite into everyone's outage, which is the thing limits exist
/// to prevent.
#[test]
#[ignore = "reads the guests `just spike-wasmtime` builds; `just e8-host` runs it"]
fn a_memory_ceiling_denies_growth_rather_than_aborting() {
    let path = guest("guest-minimal", "spike_wasmtime_guest_minimal.wasm");
    let bytes = std::fs::read(&path)
        .unwrap_or_else(|e| panic!("{}: {e}\nrun `just spike-wasmtime` first", path.display()));
    let actual = imports(path);
    let allowed: Vec<Import> = actual
        .iter()
        .map(|i| {
            let (interface, name) = i.split_once('#').unwrap_or((i.as_str(), "use"));
            Import {
                interface: interface.to_string(),
                name: name.to_string(),
                capability: "store.read".into(),
                kind: ImportKind::HostCapability,
                bounded: Vec::new(),
                event: None,
                invalidates: None,
                every: Vec::new(),
                reads: None,
                schedules: None,
            }
        })
        .collect();
    let c = contract(allowed);
    let admission = admit(&c, &topology(), "origin-1", &actual);
    let granted = Granted::from(&admission, &BTreeMap::new()).expect("admitted");

    // One page. The guest's own memory does not fit, so growth is denied and
    // instantiation fails — reported, with the engine's words.
    let cramped = Limits {
        fuel: None,
        memory_bytes: Some(1),
        table_elements: None,
    };
    let err = pw_host::engine::instantiate_within(&bytes, &c, &granted, &cramped)
        .expect_err("one byte of memory is not enough for anything");
    println!("refused for memory: {err}");

    // And the discriminating half: the same component under a real ceiling runs.
    // Without this, the assertion above would pass for a limiter that denied
    // every allocation.
    let roomy = Limits {
        fuel: None,
        memory_bytes: Some(64 * 1024 * 1024),
        table_elements: None,
    };
    pw_host::engine::instantiate_within(&bytes, &c, &granted, &roomy)
        .expect("64 MiB is enough for the minimal guest");
}

// --- the positive control: authority USED, not only refused ------------------

/// **A granted component calls the host interface it was permitted, and gets
/// the host's answer.**
///
/// Architect ruling, 2026-08-08:
///
/// > If you don't yet have a positive guest that actually exercises one granted
/// > host interface, I would add that as the last E8 control. A tiny
/// > handwritten Rust/WIT fixture is appropriate because it's testing the host,
/// > not pretending to be the Pleris backend.
///
/// The spike's minimal guest is exactly that fixture: its WIT world declares
/// one import, its `lookup` calls `stores::read`, and it returns a string
/// derived from what came back. Everything else in E8 shows authority being
/// refused or linked; this shows it being **used**, and a capability system
/// that has only ever been observed saying no has not been observed working.
#[test]
#[ignore = "reads the guests `just spike-wasmtime` builds; `just e8-host` runs it"]
fn a_granted_guest_calls_the_host_and_receives_its_answer() {
    use wasmtime::component::Val;

    let path = guest("guest-minimal", "spike_wasmtime_guest_minimal.wasm");
    let bytes = std::fs::read(&path)
        .unwrap_or_else(|e| panic!("{}: {e}\nrun `just spike-wasmtime` first", path.display()));
    let actual = imports(path);
    let allowed: Vec<Import> = actual
        .iter()
        .map(|i| {
            let (interface, name) = i.split_once('#').unwrap_or((i.as_str(), "use"));
            Import {
                interface: interface.to_string(),
                name: name.to_string(),
                capability: "store.read".into(),
                kind: ImportKind::HostCapability,
                bounded: Vec::new(),
                event: None,
                invalidates: None,
                every: Vec::new(),
                reads: None,
                schedules: None,
            }
        })
        .collect();
    let c = contract(allowed);
    let admission = admit(&c, &topology(), "origin-1", &actual);
    let granted = Granted::from(&admission, &BTreeMap::new()).expect("admitted");

    // The host's own data, behind the capability. The guest never receives the
    // map — only the answer to the call it made.
    let read = actual
        .iter()
        .find(|i| i.ends_with("#read"))
        .expect("the guest imports `read`")
        .clone();
    // The host's implementation of `read`: it answers from its own data, keyed
    // by what the guest asked for.
    let lookup: pw_host::engine::HostFn = std::sync::Arc::new(|args: &[Val]| {
        let asked = match args.first() {
            Some(Val::String(s)) => s.clone(),
            _ => return Err("`read` takes the store id".to_string()),
        };
        Ok(vec![Val::Option(Some(Box::new(Val::Record(vec![
            ("id".to_string(), Val::String(asked)),
            ("name".to_string(), Val::String("Corner Store".to_string())),
        ]))))])
    });
    let answers = BTreeMap::from([(read, lookup)]);

    let out = pw_host::engine::call_within(
        &bytes,
        &c,
        &granted,
        &Limits {
            fuel: Some(50_000_000),
            memory_bytes: Some(64 * 1024 * 1024),
            table_elements: Some(10_000),
        },
        &answers,
        &["lookup"],
        &[Val::String("store_47".into())],
    )
    .unwrap_or_else(|e| panic!("a granted guest must be able to call: {e}"));

    let Some(Val::String(s)) = out.first() else {
        panic!("expected a string, got {out:?}");
    };
    println!("the guest returned: {s}");
    assert_eq!(
        s, "found:store_47:Corner Store",
        "the value came THROUGH the host: the id is the guest's argument and \
         the name is the host's data"
    );

    // **The same call, ungranted.** Nothing is linked, so the guest cannot be
    // instantiated to make it — the capability is what made the call possible,
    // not the code being present.
    let mut ungranted = c.clone();
    ungranted.required_capabilities.clear();
    let admission = admit(&ungranted, &topology(), "origin-1", &actual);
    let granted = Granted::from(&admission, &BTreeMap::new()).expect("placement admits it");
    let err = pw_host::engine::call_within(
        &bytes,
        &ungranted,
        &granted,
        &Limits::unbounded(),
        &answers,
        &["lookup"],
        &[Val::String("store_47".into())],
    )
    .expect_err("an ungranted guest cannot call what it was not linked");
    println!("ungranted call refused: {err}");
    assert!(err.contains("perfect-web:store/stores"), "{err}");
}
