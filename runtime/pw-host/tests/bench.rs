//! **E10 gate item 4: what a compiled command costs, against baselines.**
//!
//! Three things through one host API, `pw_host::engine::Prepared::call_within`,
//! which is what the development server does per request: admission's
//! `Granted`, a linker built from it, a fresh store and instance, the call, the
//! drop.
//!
//! ```text
//! the compiled command    docs/evidence/E10/store.page.add_to_cart.wasm
//! a hand-written guest    spikes/wasmtime-component/guest-minimal (Rust, no_std)
//! the native operation    the data layer's HostFn, called directly: the floor
//! ```
//!
//! Every test here is ignored in an ordinary run: a timing taken by a debug
//! build under `cargo test --workspace` measures the build. `just e10-bench`
//! runs them in release, alone, with `--include-ignored`.

#![cfg(feature = "engine")]

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::Instant;

use pw_host::engine::HostFn;
use pw_host::*;
use wasmtime::component::Val;

fn root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Per-call microseconds for each of `rounds` rounds of `calls` calls, after a
/// warm-up round. Printed with the median, min and max.
fn measure(label: &str, rounds: usize, calls: usize, mut f: impl FnMut(usize)) -> f64 {
    for i in 0..calls {
        f(i);
    }
    let mut per_call: Vec<f64> = (0..rounds)
        .map(|_| {
            let started = Instant::now();
            for i in 0..calls {
                f(i);
            }
            started.elapsed().as_secs_f64() * 1e6 / calls as f64
        })
        .collect();
    per_call.sort_by(f64::total_cmp);
    let median = per_call[per_call.len() / 2];
    println!(
        "bench: {label}: median {median:.3} µs per call (min {:.3}, max {:.3}; {rounds} rounds x {calls} calls)",
        per_call[0],
        per_call[per_call.len() - 1]
    );
    median
}

fn limits() -> Limits {
    Limits {
        fuel: Some(10_000_000),
        memory_bytes: Some(16 * 1024 * 1024),
        table_elements: Some(1_000),
    }
}

/// The item a press adds: a `MenuItem` as its record takes it, its fields
/// in the order the type declares them. Until 2026-10-11 an id, the command's
/// parameter before ADR-0178 made it the item the page showed.
fn menu_item(i: usize) -> Val {
    let text = |s: &str| Val::String(s.into());
    Val::Record(vec![
        ("id".into(), Val::String(format!("item-{i}"))),
        ("store-id".into(), text("store_47")),
        ("name".into(), text("Cortado")),
        ("description".into(), text("")),
        ("price".into(), Val::S64(450)),
        ("available".into(), Val::Bool(true)),
        (
            "category".into(),
            Val::Record(vec![
                ("id".into(), text("coffee")),
                ("name".into(), text("Coffee")),
            ]),
        ),
    ])
}

fn node(grants: &[&str]) -> Topology {
    Topology {
        nodes: vec![Node {
            name: "origin-1".into(),
            world: "origin".into(),
            grants: grants
                .iter()
                .map(|g| g.to_string())
                .collect::<BTreeSet<_>>(),
        }],
    }
}

#[test]
#[ignore = "a benchmark; `just e10-bench` runs it in release"]
fn the_compiled_command_through_the_host() {
    let bytes = std::fs::read(root().join("docs/evidence/E10/store.page.add_to_cart.wasm"))
        .expect("run `just e10-component`");
    let contract = ComponentContract::from_json(
        &std::fs::read_to_string(root().join("docs/evidence/E8/component-contracts.json"))
            .expect("contracts"),
    )
    .expect("parse")
    .into_iter()
    .find(|c| c.component_id == "store.page.add_to_cart")
    .expect("add_to_cart");
    let actual = engine::imports_of(&bytes).expect("imports");
    // The node grants what the command's contract requires, read from it:
    // until 2026-10-11 the two it required when this was written, and the
    // command, which reads the menu (ADR-0178) and hands the outbox its
    // event (ADR-0246) since, was refused admission, unseen, the test being
    // run only by `just e10-bench` and `just e8-host`.
    let grants: Vec<String> = contract
        .required_capabilities
        .iter()
        .map(Capability::name)
        .collect();
    let grants: Vec<&str> = grants.iter().map(String::as_str).collect();
    let admission = admit(&contract, &node(&grants), "origin-1", &actual);
    let granted = Granted::from(&admission, &BTreeMap::new()).expect("admitted");
    let export = contract.exports[0].component.clone().expect("located");
    let cart = Val::Result(Ok(Some(Box::new(Val::Record(vec![(
        "lines".into(),
        // A line as `CartLine` declares it, its name since ADR-0172.
        Val::List(vec![Val::Record(vec![
            ("item-id".into(), Val::String("cortado".into())),
            ("name".into(), Val::String("Cortado".into())),
            ("quantity".into(), Val::S64(2)),
            ("unit-price".into(), Val::S64(450)),
        ])]),
    )])))));
    let add_result = cart.clone();
    // The reader's user, and their cart (ADR-0318); the store that reaches
    // their chosen address (ADR-0319). Until 2026-10-12 the benchmark
    // answered the session's cart, which the command no longer imports, and
    // `just e8-host` failed every nightly from W8's merge.
    let read: HostFn = Arc::new(|_| Ok(vec![Val::String("user-7".into())]));
    let add: HostFn = Arc::new(move |_| Ok(vec![add_result.clone()]));
    let done: HostFn = Arc::new(|_| Ok(Vec::new()));
    let available: HostFn = Arc::new(|_| Ok(vec![Val::Bool(true)]));
    let reaches: HostFn = Arc::new(|_| Ok(vec![Val::Bool(true)]));
    let ops = BTreeMap::from([
        ("pw:host/principal#read".to_string(), read),
        ("store:data/user-carts#add".to_string(), add.clone()),
        ("store:data/menus#is-available".to_string(), available),
        ("store:data/coverage#reaches".to_string(), reaches),
        ("pw:host/outbox#user-cart-changed".to_string(), done.clone()),
        ("pw:host/invalidations#store-page-cart".to_string(), done),
    ]);
    // Each import the contract declares, answered here: one the benchmark
    // does not answer is said by name rather than refused by the engine.
    for import in &contract.imports {
        assert!(
            ops.contains_key(&import.key()),
            "the benchmark answers no `{}`: add it here",
            import.key()
        );
    }

    let started = Instant::now();
    let prepared = engine::Prepared::compile(&bytes).expect("compiles");
    println!(
        "bench: compile store.page.add_to_cart ({} bytes): {:.1} ms",
        bytes.len(),
        started.elapsed().as_secs_f64() * 1e3
    );
    measure("store.page.add_to_cart through the host", 7, 2_000, |i| {
        let out = prepared
            .call_authorized_within(
                &contract,
                &granted,
                &limits(),
                &ops,
                &[&export.interface, &export.function],
                &[menu_item(i), Val::S64(1)],
                |predicate, _arguments| match predicate {
                    "SignedIn" => Ok(true),
                    other => Err(format!("benchmark deployment does not define `{other}`")),
                },
            )
            .expect("runs");
        assert_eq!(out, vec![cart.clone()]);
    });

    // The floor: the operation the command exists to reach, called directly.
    let args = [
        Val::String("session-7".into()),
        Val::String("cortado".into()),
        Val::S64(1),
    ];
    measure("the data layer's add, called natively", 7, 200_000, |_| {
        let out = add(&args).expect("runs");
        std::hint::black_box(out);
    });
}

#[test]
#[ignore = "a benchmark, and it reads the guest `just spike-wasmtime` builds"]
fn a_hand_written_rust_guest_through_the_host() {
    let path = root().join(
        "spikes/wasmtime-component/guest-minimal/target/wasm32-wasip2/release/spike_wasmtime_guest_minimal.wasm",
    );
    let bytes = std::fs::read(&path).expect("run `just spike-wasmtime`");
    let actual = engine::imports_of(&bytes).expect("imports");
    let contract = ComponentContract {
        component_id: "spike.Store".into(),
        capability_mapping: CAPABILITY_MAPPING,
        abi_schema: "abi".into(),
        required_capabilities: vec![Capability {
            family: "store".into(),
            operation: "read".into(),
            argument: None,
        }],
        allowed_placements: vec!["origin".into()],
        imports: actual
            .iter()
            .map(|i| {
                let (interface, name) = i.split_once('#').unwrap_or((i.as_str(), "use"));
                Import {
                    event: None,
                    invalidates: None,
                    every: Vec::new(),
                    reads: None,
                    schedules: None,
                    interface: interface.into(),
                    name: name.into(),
                    capability: "store.read".into(),
                    kind: ImportKind::HostCapability,
                    bounded: Vec::new(),
                }
            })
            .collect(),
        exports: vec![Export {
            name: "lookup".into(),
            kind: "query".into(),
            binding: BindingSupport::default(),
            // The guest's world exports `lookup` directly: no interface.
            component: Some(ComponentExport {
                interface: String::new(),
                function: "lookup".into(),
                authorization: Vec::new(),
                idempotent_by: None,
                invoked_by: None,
                runs_for: Vec::new(),
                bounded: Vec::new(),
            }),
        }],
    };
    let admission = admit(&contract, &node(&["store.read"]), "origin-1", &actual);
    let granted = Granted::from(&admission, &BTreeMap::new()).expect("admitted");
    let read = actual
        .iter()
        .find(|i| i.ends_with("#read"))
        .expect("read")
        .clone();
    let lookup: HostFn = Arc::new(|args: &[Val]| {
        let Some(Val::String(id)) = args.first() else {
            return Err("read takes an id".into());
        };
        Ok(vec![Val::Option(Some(Box::new(Val::Record(vec![
            ("id".into(), Val::String(id.clone())),
            ("name".into(), Val::String("Corner Store".into())),
        ]))))])
    });
    let ops = BTreeMap::from([(read, lookup)]);

    let started = Instant::now();
    let prepared = engine::Prepared::compile(&bytes).expect("compiles");
    println!(
        "bench: compile the Rust no_std guest ({} bytes): {:.1} ms",
        bytes.len(),
        started.elapsed().as_secs_f64() * 1e3
    );
    measure(
        "the Rust no_std guest's lookup through the host",
        7,
        2_000,
        |i| {
            let out = prepared
                .call_within(
                    &contract,
                    &granted,
                    &limits(),
                    &ops,
                    &["lookup"],
                    &[Val::String(format!("store_{i}"))],
                )
                .expect("runs");
            assert!(matches!(out.first(), Some(Val::String(s)) if s.starts_with("found:")));
        },
    );
}
