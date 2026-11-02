//! **E10-I step 8: a Pleris-compiled command, running in the E8 host.**
//!
//! The component is `docs/evidence/E10/store.page.add_to_cart.wasm`, produced
//! by `just e10-component` from `examples/store/app.pw` and held to the
//! compiler's current output by `pw-core`'s `evidence_is_current`. The contract
//! is the compiler's, from `docs/evidence/E8/component-contracts.json`. The
//! host reads both as data — it links no compiler, which is ADR-0020's
//! boundary: the compiler decides what authority the code needs, the host
//! decides whether it exists.
//!
//! What runs is the compiled body of
//!
//! ```text
//! command add_to_cart(item: MenuItem, quantity: PositiveInt) -> Result<Cart, CartError>
//! {
//!     if !Menus.is_available(item.id) {
//!         return Err(CartError.ItemUnavailable(item.id))
//!     }
//!     Carts.add(current_session(), item.id, quantity)
//! }
//! ```
//!
//! The item is the one the page showed (ADR-0172), a record; the command
//! writes by its id.
//!
//! The availability read is ADR-0157's: an item sold out since the page was
//! rendered is refused by name, before anything is written.
//!
//! and nothing else: no Rust closure computes its result.

#![cfg(feature = "engine")]

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use pw_host::engine::HostFn;
use pw_host::*;
use wasmtime::component::Val;

fn root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn component() -> Vec<u8> {
    let path = root().join("docs/evidence/E10/store.page.add_to_cart.wasm");
    std::fs::read(&path)
        .unwrap_or_else(|e| panic!("{}: {e} — run `just e10-component`", path.display()))
}

fn contract() -> ComponentContract {
    let path = root().join("docs/evidence/E8/component-contracts.json");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{}: {e} — run `just e8-contracts`", path.display()));
    ComponentContract::from_json(&text)
        .expect("the compiler's contracts parse")
        .into_iter()
        .find(|c| c.component_id == "store.page.add_to_cart")
        .expect("the contract for add_to_cart")
}

fn origin(grants: &[&str]) -> Topology {
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

/// The contract's export, where the compiler says it is in the component.
fn export(c: &ComponentContract) -> [String; 2] {
    let e = c.exports[0]
        .component
        .clone()
        .expect("the contract says where its export is in the component");
    [e.interface, e.function]
}

/// A cart the data layer returns: one line, as the WIT's record fields name
/// them.
fn cart(item: &str, quantity: i64) -> Val {
    Val::Record(vec![(
        "lines".into(),
        Val::List(vec![Val::Record(vec![
            ("item-id".into(), Val::String(item.into())),
            ("name".into(), Val::String("Cortado".into())),
            ("quantity".into(), Val::S64(quantity)),
            ("unit-price".into(), Val::S64(450)),
        ])]),
    )])
}

/// An item as the page showed it (ADR-0172), as the WIT's record fields name
/// them: what `add_to_cart` is called with.
fn item(id: &str) -> Val {
    Val::Record(vec![
        ("id".into(), Val::String(id.into())),
        ("store-id".into(), Val::String("47".into())),
        ("name".into(), Val::String("Cortado".into())),
        ("description".into(), Val::String("Short.".into())),
        ("price".into(), Val::S64(375)),
        ("available".into(), Val::Bool(true)),
        (
            "category".into(),
            Val::Record(vec![
                ("id".into(), Val::String("coffee".into())),
                ("name".into(), Val::String("Coffee".into())),
            ]),
        ),
    ])
}

/// What the host saw of each call, so a test can assert the COMPONENT made it.
type Calls = Arc<Mutex<Vec<(String, Vec<Val>)>>>;

/// Each predicate the deployment was asked, and what it was given.
type Asked = Arc<Mutex<Vec<(String, Vec<Val>)>>>;

/// The deployment's operations: the platform's session, and a data layer in
/// which every item can be ordered.
fn host(calls: &Calls, add_result: Val) -> BTreeMap<String, HostFn> {
    stocked(calls, add_result, true)
}

/// [`host`], with whether an item can be ordered answered `available`.
fn stocked(calls: &Calls, add_result: Val, available: bool) -> BTreeMap<String, HostFn> {
    let stock_calls = calls.clone();
    let stock: HostFn = Arc::new(move |args: &[Val]| {
        stock_calls
            .lock()
            .unwrap()
            .push(("store:data/menus#is-available".into(), args.to_vec()));
        Ok(vec![Val::Bool(available)])
    });
    let session_calls = calls.clone();
    let read: HostFn = Arc::new(move |args: &[Val]| {
        session_calls
            .lock()
            .unwrap()
            .push(("pw:host/principal#read".into(), args.to_vec()));
        Ok(vec![Val::String("session-7".into())])
    });
    let add_calls = calls.clone();
    let add: HostFn = Arc::new(move |args: &[Val]| {
        add_calls
            .lock()
            .unwrap()
            .push(("store:data/user-carts#add".into(), args.to_vec()));
        Ok(vec![add_result.clone()])
    });
    // The outbox, which the command hands its event to (ADR-0208).
    let outbox_calls = calls.clone();
    let outbox: HostFn = Arc::new(move |args: &[Val]| {
        outbox_calls
            .lock()
            .unwrap()
            .push(("pw:host/outbox#user-cart-changed".into(), args.to_vec()));
        Ok(Vec::new())
    });
    // And the invalidations, which it hands its cart's entry to (ADR-0209).
    let entry_calls = calls.clone();
    let entry: HostFn = Arc::new(move |args: &[Val]| {
        entry_calls.lock().unwrap().push((
            "pw:host/invalidations#store-page-cart".into(),
            args.to_vec(),
        ));
        Ok(Vec::new())
    });
    // Whether the item's store reaches the reader's chosen address (track
    // `store-accounts`): it does.
    let reach_calls = calls.clone();
    let reaches: HostFn = Arc::new(move |args: &[Val]| {
        reach_calls
            .lock()
            .unwrap()
            .push(("store:data/coverage#reaches".into(), args.to_vec()));
        Ok(vec![Val::Bool(true)])
    });
    BTreeMap::from([
        ("store:data/coverage#reaches".to_string(), reaches),
        ("store:data/menus#is-available".to_string(), stock),
        ("pw:host/principal#read".to_string(), read),
        ("pw:host/invalidations#store-page-cart".to_string(), entry),
        ("pw:host/outbox#user-cart-changed".to_string(), outbox),
        ("store:data/user-carts#add".to_string(), add),
    ])
}

fn admitted(c: &ComponentContract, bytes: &[u8], grants: &[&str]) -> Result<Granted, String> {
    let actual = engine::imports_of(bytes)?;
    let admission = admit(c, &origin(grants), "origin-1", &actual);
    Granted::from(&admission, &BTreeMap::new()).ok_or_else(|| format!("{admission:?}"))
}

/// What the command needs granted: the menu read, the cart write, the
/// reader's addresses (track `store-accounts`) and the session.
const ALL: [&str; 5] = [
    "database.read<Addresses>",
    "database.read<Menus>",
    "database.write<Carts>",
    "outbox.write",
    "session.read",
];

fn limits() -> Limits {
    Limits {
        fuel: Some(10_000_000),
        memory_bytes: Some(16 * 1024 * 1024),
        table_elements: Some(1_000),
    }
}

fn approve_store_authorization(predicate: &str, _arguments: &[&Val]) -> Result<bool, String> {
    match predicate {
        "SignedIn" => Ok(true),
        other => Err(format!("test deployment does not define `{other}`")),
    }
}

fn authorized_call(
    bytes: &[u8],
    contract: &ComponentContract,
    granted: &Granted,
    limits: &Limits,
    host: &BTreeMap<String, HostFn>,
    export: &[&str],
    args: &[Val],
) -> Result<Vec<Val>, String> {
    engine::call_authorized_within(
        bytes,
        contract,
        granted,
        limits,
        host,
        export,
        args,
        approve_store_authorization,
    )
}

#[test]
fn the_artifact_imports_exactly_what_its_contract_allows() {
    let actual = engine::imports_of(&component()).expect("the component reads");
    println!("compiled add_to_cart imports: {actual:?}");
    assert_eq!(
        actual,
        [
            "pw:host/invalidations#store-page-cart",
            "pw:host/outbox#user-cart-changed",
            "pw:host/principal#read",
            "store:data/coverage#reaches",
            "store:data/menus#is-available",
            "store:data/user-carts#add",
        ]
    );
    let c = contract();
    assert!(
        matches!(
            admit(&c, &origin(&ALL), "origin-1", &actual),
            Admission::Admit { .. }
        ),
        "the host admits the compiled artifact against the compiler's contract"
    );
}

#[test]
fn an_export_not_declared_by_the_contract_cannot_be_invoked() {
    let c = contract();
    let bytes = component();
    let granted = admitted(&c, &bytes, &ALL).expect("admitted");
    let calls: Calls = Arc::default();
    let err = engine::call_authorized_within(
        &bytes,
        &c,
        &granted,
        &limits(),
        &host(&calls, Val::Result(Ok(Some(Box::new(cart("cortado", 1)))))),
        &["pw:app/not-the-contract@0.1.0", "add-to-cart"],
        &[item("cortado"), Val::S64(1)],
        approve_store_authorization,
    )
    .expect_err("an undeclared export path must fail closed");
    assert!(err.contains("not an export declared"), "{err}");
    assert!(calls.lock().unwrap().is_empty(), "nothing ran");
}

#[test]
fn requires_cannot_be_bypassed_by_the_raw_call_api() {
    // Track `store-accounts` (ADR-0318): anyone fills a cart, so the store's
    // `add_to_cart` requires no one signed in. What a command requires is its
    // contract's, which the host evaluates: given one here, the raw call
    // cannot pass it.
    let mut c = contract();
    c.exports
        .iter_mut()
        .find_map(|e| e.component.as_mut())
        .expect("the command's export")
        .authorization
        .push(
            serde_json::from_value(serde_json::json!({ "predicate": "SignedIn", "arguments": [] }))
                .expect("an authorization"),
        );
    let bytes = component();
    let granted = admitted(&c, &bytes, &ALL).expect("admitted");
    let calls: Calls = Arc::default();
    let [interface, function] = export(&c);
    let err = engine::call_within(
        &bytes,
        &c,
        &granted,
        &limits(),
        &host(&calls, Val::Result(Ok(Some(Box::new(cart("cortado", 1)))))),
        &[&interface, &function],
        &[item("cortado"), Val::S64(1)],
    )
    .expect_err("a requires clause must be evaluated before the body can run");
    assert!(err.contains("authorization precondition"), "{err}");
    assert!(
        calls.lock().unwrap().is_empty(),
        "nothing ran before authorization"
    );
}

/// **A world-level export is reachable when declared, and refused when not.**
///
/// `run` finds an export by a path of any length, and a function the world
/// exports directly has a one-segment path. The authorization lookup took only
/// two segments, so such an export was refused as a malformed path whatever
/// the contract said, and the hand-written baseline guest of `just e10-bench`
/// could not be called (found 2026-10-02 closing E10).
#[test]
fn a_world_level_export_is_found_by_its_declaration() {
    let allow = |_: &str, _: &[&Val]| Ok(true);
    let mut c = contract();
    let [_, function] = export(&c);

    // The store command lives in an interface: its bare name is not it.
    let err = engine::authorize_export(&c, &[&function], &[], allow)
        .expect_err("a bare name does not name an interface export");
    assert!(err.contains("is not an export declared"), "{err}");

    // Declared at the world level, the same bare name is found.
    let declared = c
        .exports
        .iter_mut()
        .find_map(|e| e.component.as_mut())
        .expect("the command's export");
    declared.interface = String::new();
    declared.authorization.clear();
    engine::authorize_export(&c, &[&function], &[], allow).expect("declared at the world level");

    // A path of any other shape is malformed.
    let err = engine::authorize_export(&c, &["a", "b", &function], &[], allow)
        .expect_err("three segments name nothing");
    assert!(err.contains("an export path is"), "{err}");
}

#[test]
fn the_compiled_command_runs_through_the_host() {
    let c = contract();
    let bytes = component();
    let granted = admitted(&c, &bytes, &ALL).expect("admitted");
    let calls: Calls = Arc::default();
    let returned = Val::Result(Ok(Some(Box::new(cart("cortado", 2)))));
    let [interface, function] = export(&c);

    let out = authorized_call(
        &bytes,
        &c,
        &granted,
        &limits(),
        &host(&calls, returned.clone()),
        &[&interface, &function],
        &[item("cortado"), Val::S64(2)],
    )
    .unwrap_or_else(|e| panic!("the compiled command must run: {e}"));

    let seen = calls.lock().unwrap().clone();
    println!("the host saw: {seen:?}");
    println!("the command returned: {out:?}");

    // The COMPONENT read the session for its event's key and handed the event
    // to the outbox (ADR-0208), asked whether the item can be ordered, read
    // the reader and asked whether the item's store reaches their address
    // (track `store-accounts`), read the reader, then called the data layer
    // with it and with its own arguments, in that order.
    assert_eq!(seen.len(), 9, "{seen:?}");
    let session = || ("pw:host/principal#read".to_string(), vec![]);
    let given = |op: &str| (op.to_string(), vec![Val::String("session-7".into())]);
    assert_eq!(seen[0], session());
    assert_eq!(seen[1], given("pw:host/invalidations#store-page-cart"));
    assert_eq!(seen[2], session());
    assert_eq!(seen[3], given("pw:host/outbox#user-cart-changed"));
    assert_eq!(
        seen[4],
        (
            "store:data/menus#is-available".to_string(),
            vec![Val::String("cortado".into())]
        )
    );
    assert_eq!(seen[5], session());
    assert_eq!(
        seen[6],
        (
            "store:data/coverage#reaches".to_string(),
            vec![Val::String("47".into()), Val::String("session-7".into())]
        )
    );
    assert_eq!(seen[7], session());
    assert_eq!(
        seen[8],
        (
            "store:data/user-carts#add".to_string(),
            vec![
                Val::String("session-7".into()),
                Val::String("cortado".into()),
                Val::S64(2),
            ]
        ),
        "the session came from the host; the item and quantity from the caller"
    );
    // And the data layer's answer is the command's answer, lifted back out of
    // the component's memory by the engine.
    assert_eq!(out, vec![returned]);
}

/// **An item that cannot be ordered is refused by name** (ADR-0157), and
/// nothing is written: the session is not read and the data layer's `add`
/// is not called.
#[test]
fn an_item_that_cannot_be_ordered_is_refused_before_anything_is_written() {
    let c = contract();
    let bytes = component();
    let granted = admitted(&c, &bytes, &ALL).expect("admitted");
    let calls: Calls = Arc::default();
    let [interface, function] = export(&c);
    let out = authorized_call(
        &bytes,
        &c,
        &granted,
        &limits(),
        &stocked(
            &calls,
            Val::Result(Ok(Some(Box::new(cart("cortado", 1))))),
            false,
        ),
        &[&interface, &function],
        &[item("cortado"), Val::S64(1)],
    )
    .expect("the call completes; the command's result is the refusal");
    println!("sold out: {out:?}");
    assert_eq!(
        out,
        vec![Val::Result(Err(Some(Box::new(Val::Variant(
            "item-unavailable".into(),
            Some(Box::new(Val::String("cortado".into()))),
        )))))]
    );
    // Its entry and its event were handed over first (ADR-0208, ADR-0209),
    // and a refusal commits them no more than it commits a write.
    assert_eq!(
        *calls.lock().unwrap(),
        [
            ("pw:host/principal#read".to_string(), vec![]),
            (
                "pw:host/invalidations#store-page-cart".to_string(),
                vec![Val::String("session-7".into())]
            ),
            ("pw:host/principal#read".to_string(), vec![]),
            (
                "pw:host/outbox#user-cart-changed".to_string(),
                vec![Val::String("session-7".into())]
            ),
            (
                "store:data/menus#is-available".to_string(),
                vec![Val::String("cortado".into())]
            )
        ],
        "only the availability was read, and nothing written"
    );
}

#[test]
fn a_data_layer_failure_is_the_commands_failure() {
    let c = contract();
    let bytes = component();
    let granted = admitted(&c, &bytes, &ALL).expect("admitted");
    let calls: Calls = Arc::default();
    let failed = Val::Result(Err(Some(Box::new(Val::Variant(
        "cart-expired".into(),
        None,
    )))));
    let [interface, function] = export(&c);
    let out = authorized_call(
        &bytes,
        &c,
        &granted,
        &limits(),
        &host(&calls, failed.clone()),
        &[&interface, &function],
        &[item("cortado"), Val::S64(1)],
    )
    .expect("the call completes; the command's result is a failure");
    println!("a failing data layer: {out:?}");
    assert_eq!(out, vec![failed]);
}

#[test]
fn a_node_without_the_write_capability_does_not_admit_it() {
    let c = contract();
    let bytes = component();
    // Everything else granted, so the refusal is the write's alone.
    let err = admitted(&c, &bytes, &["database.read<Menus>", "session.read"]).expect_err("refused");
    println!("refused without database.write<Carts>: {err}");
    assert!(err.contains("database.write<Carts>"), "{err}");
}

#[test]
fn an_ungranted_operation_is_refused_by_the_engine() {
    // The contract without the write: the host links only what is granted, so
    // `user-carts#add` has no definition and the ENGINE refuses to instantiate —
    // in its own words, naming the interface.
    let mut c = contract();
    c.required_capabilities
        .retain(|cap| cap.name() != "database.write<Carts>");
    let bytes = component();
    let admission = admit(&c, &origin(&ALL), "origin-1", &[]);
    let granted = Granted::from(&admission, &BTreeMap::new()).expect("admitted as edited");
    let calls: Calls = Arc::default();
    let [interface, function] = export(&c);
    let err = authorized_call(
        &bytes,
        &c,
        &granted,
        &limits(),
        &host(&calls, Val::Bool(false)),
        &[&interface, &function],
        &[item("cortado"), Val::S64(1)],
    )
    .expect_err("the write is not linked");
    println!("ungranted write refused by the engine: {err}");
    assert!(err.contains("store:data/user-carts"), "{err}");
    assert!(calls.lock().unwrap().is_empty(), "nothing ran");
}

#[test]
fn a_granted_operation_the_host_does_not_implement_is_refused() {
    let c = contract();
    let bytes = component();
    let granted = admitted(&c, &bytes, &ALL).expect("admitted");
    let calls: Calls = Arc::default();
    let mut ops = host(&calls, Val::Bool(false));
    ops.remove("store:data/user-carts#add");
    let [interface, function] = export(&c);
    let err = authorized_call(
        &bytes,
        &c,
        &granted,
        &limits(),
        &ops,
        &[&interface, &function],
        &[item("cortado"), Val::S64(1)],
    )
    .expect_err("granted is not implemented");
    assert!(err.contains("implements nothing"), "{err}");
}

#[test]
fn the_instance_runs_within_its_fuel() {
    let c = contract();
    let bytes = component();
    let granted = admitted(&c, &bytes, &ALL).expect("admitted");
    let calls: Calls = Arc::default();
    let [interface, function] = export(&c);
    let starved = Limits {
        fuel: Some(1),
        memory_bytes: None,
        table_elements: None,
    };
    let err = authorized_call(
        &bytes,
        &c,
        &granted,
        &starved,
        &host(&calls, Val::Result(Ok(Some(Box::new(cart("x", 1)))))),
        &[&interface, &function],
        &[item("cortado"), Val::S64(1)],
    )
    .expect_err("one unit of fuel runs out");
    println!("starved of fuel: {err}");
    assert!(err.to_lowercase().contains("fuel"), "{err}");
}

// --- E10: arguments that arrive as JSON, from a compiled handler -----------

/// **A browser's arguments are typed by the component's own parameters.**
///
/// The compiled handler for `add_to_cart(item, PositiveInt(1))` runs in the
/// user's browser and sends the item as the page showed it, and `1`
/// (ADR-0172). The host converts them by the parameter types the ARTIFACT
/// declares, a record and an `s64`, and refuses anything else before the
/// component runs.
#[test]
fn json_arguments_are_typed_by_the_export_they_are_for() {
    let c = contract();
    let prepared = engine::Prepared::compile(&component()).expect("compiles");
    let [interface, function] = export(&c);
    let at = [interface.as_str(), function.as_str()];
    let sent = |price: serde_json::Value| {
        serde_json::json!({
            "id": "cortado",
            "store_id": "47",
            "name": "Cortado",
            "description": "Short.",
            "price": price,
            "available": true,
            "category": { "id": "coffee", "name": "Coffee" },
        })
    };
    let cortado = sent(serde_json::json!(375));

    let args = prepared
        .arguments(&at, &[cortado.clone(), serde_json::json!(2)])
        .expect("well-typed");
    assert_eq!(args, vec![item("cortado"), Val::S64(2)]);
    // A field the record does not declare is not passed in.
    let mut more = cortado.clone();
    more["popular"] = serde_json::json!(true);
    assert_eq!(
        prepared
            .arguments(&at, &[more, serde_json::json!(2)])
            .expect("well-typed"),
        args
    );

    let mut nameless = cortado.clone();
    nameless.as_object_mut().unwrap().remove("name");
    for (json, why) in [
        (vec![cortado.clone()], "takes 2 argument(s); 1 were sent"),
        (
            vec![cortado.clone(), serde_json::json!(1), serde_json::json!(1)],
            "takes 2 argument(s); 3 were sent",
        ),
        (
            vec![serde_json::json!("cortado"), serde_json::json!(1)],
            "expected an object",
        ),
        (
            vec![nameless, serde_json::json!(1)],
            "the record has no field `name`",
        ),
        (
            vec![sent(serde_json::json!(1.5)), serde_json::json!(1)],
            "arg0`).price: expected an integer",
        ),
        (
            // An amount is its cents (ADR-0327), not a record of them.
            vec![
                sent(serde_json::json!({ "minor_units": 375 })),
                serde_json::json!(1),
            ],
            "arg0`).price: expected an integer",
        ),
        (
            vec![cortado.clone(), serde_json::json!(1.5)],
            "expected an integer",
        ),
        (
            vec![cortado.clone(), serde_json::json!(true)],
            "expected an integer",
        ),
        (
            vec![cortado.clone(), serde_json::json!(null)],
            "expected an integer",
        ),
        (
            vec![cortado.clone(), serde_json::json!(u64::MAX)],
            "is outside",
        ),
    ] {
        let err = prepared.arguments(&at, &json).expect_err("refused");
        println!("{json:?}: {err}");
        assert!(err.contains(why), "{json:?}: {err}");
    }

    // The extremes of `s64` are the type's, not JavaScript's: the host takes
    // the whole range the parameter declares.
    let edge = prepared
        .arguments(&at, &[cortado, serde_json::json!(i64::MIN)])
        .expect("in range");
    assert_eq!(edge[1], Val::S64(i64::MIN));
}

/// **A quantity that is no `PositiveInt` is refused as the arguments are
/// decoded** (ADR-0179): the contract states that argument 2 holds `value
/// >= 1`, and the host holds a browser's request to it before the component
/// runs. `arguments` alone types by the ABI, and takes any `s64`.
#[test]
fn a_quantity_that_is_no_positive_int_is_refused_as_the_arguments_are_decoded() {
    let c = contract();
    let prepared = engine::Prepared::compile(&component()).expect("compiles");
    let located = c.exports[0].component.clone().expect("located");
    let cortado = serde_json::json!({
        "id": "cortado",
        "store_id": "47",
        "name": "Cortado",
        "description": "Short.",
        "price": 375,
        "available": true,
        "category": { "id": "coffee", "name": "Coffee" },
    });
    for forged in [0, -3, i64::MIN] {
        let json = [cortado.clone(), serde_json::json!(forged)];
        let why = prepared
            .arguments_for(&located, &json)
            .expect_err("refused before the component runs");
        assert_eq!(
            why,
            format!(
                "argument refused: value 2 is {forged}, and `domain.PositiveInt` holds `value >= 1`"
            )
        );
        // The ABI alone takes it: the invariant is the contract's.
        let [interface, function] = export(&c);
        assert!(prepared.arguments(&[&interface, &function], &json).is_ok());
    }
    for held in [1, 2, i64::MAX] {
        let args = prepared
            .arguments_for(&located, &[cortado.clone(), serde_json::json!(held)])
            .expect("a count");
        assert_eq!(args[1], Val::S64(held));
    }
}

/// **An argument that breaks its invariant never reaches the component,
/// whichever call carries it** (ADR-0346, argument invariants): the
/// contract's checks of an export's arguments are the component's boundary,
/// not one decoder's. A host that builds its values itself (a page's address,
/// a stored run, a test) calls the same `call_*` as one that decoded a
/// browser's JSON, and a quantity of 0 for a `PositiveInt` is refused before
/// the component runs: no operation is called, so the data layer never sees
/// it, and no `requires` predicate is asked about it. Until the track, only
/// [`engine::Prepared::arguments_for`] held them, and a value built otherwise
/// ran the command (track `reviews`' finding).
#[test]
fn an_argument_that_breaks_its_invariant_never_reaches_the_component() {
    let c = contract();
    let bytes = component();
    let granted = admitted(&c, &bytes, &ALL).expect("admitted");
    let prepared = engine::Prepared::compile(&bytes).expect("compiles");
    let [interface, function] = export(&c);
    let at = [interface.as_str(), function.as_str()];
    let answered = Val::Result(Ok(Some(Box::new(cart("cortado", 1)))));
    // The command as one that requires a predicate of its quantity, as
    // `OwnsPost(post)` reads its post: a predicate reads through the data
    // layer, so it is asked only of a value that holds its type.
    let mut guarded = c.clone();
    guarded
        .exports
        .iter_mut()
        .find_map(|e| e.component.as_mut())
        .expect("the command's export")
        .authorization
        .push(AuthorizationRequirement {
            predicate: "MayAdd".into(),
            arguments: vec![1],
            component: None,
        });
    let asked: Asked = Arc::default();
    let evaluate = |predicate: &str, read: &[&Val]| {
        asked.lock().unwrap().push((
            predicate.to_string(),
            read.iter().map(|v| (*v).clone()).collect(),
        ));
        Ok(true)
    };
    for forged in [0, -3, i64::MIN] {
        let args = [item("cortado"), Val::S64(forged)];
        let said = format!(
            "argument refused: value 2 is {forged}, and `domain.PositiveInt` holds `value >= 1`"
        );
        // Each way in: the free function and the compiled component's, with
        // its `requires` evaluated; and the unauthorized call and the
        // measured one a benchmark makes.
        let calls: Calls = Arc::default();
        let why = engine::call_authorized_within(
            &bytes,
            &guarded,
            &granted,
            &limits(),
            &host(&calls, answered.clone()),
            &at,
            &args,
            evaluate,
        )
        .expect_err("refused before the component runs");
        assert_eq!(why, said);
        let why = prepared
            .call_authorized_within(
                &guarded,
                &granted,
                &limits(),
                &host(&calls, answered.clone()),
                &at,
                &args,
                evaluate,
            )
            .expect_err("refused before the component runs");
        assert_eq!(why, said);
        let why = prepared
            .call_within(
                &c,
                &granted,
                &limits(),
                &host(&calls, answered.clone()),
                &at,
                &args,
            )
            .expect_err("refused");
        assert_eq!(why, said);
        let why = prepared
            .call_measured(
                &c,
                &granted,
                &limits(),
                &host(&calls, answered.clone()),
                &at,
                &args,
            )
            .expect_err("refused");
        assert_eq!(why, said);
        assert!(
            calls.lock().unwrap().is_empty(),
            "no operation was called: {:?}",
            calls.lock().unwrap()
        );
        assert!(
            asked.lock().unwrap().is_empty(),
            "no predicate read it: {:?}",
            asked.lock().unwrap()
        );
    }
    // The control: 1 runs each way, and reaches the data layer each time;
    // the predicate is asked of it, by each authorized call.
    let calls: Calls = Arc::default();
    let count = [item("cortado"), Val::S64(1)];
    let ops = host(&calls, answered.clone());
    engine::call_authorized_within(
        &bytes,
        &guarded,
        &granted,
        &limits(),
        &ops,
        &at,
        &count,
        evaluate,
    )
    .expect("a count runs");
    prepared
        .call_authorized_within(&guarded, &granted, &limits(), &ops, &at, &count, evaluate)
        .expect("a count runs");
    prepared
        .call_within(&c, &granted, &limits(), &ops, &at, &count)
        .expect("a count runs");
    prepared
        .call_measured(&c, &granted, &limits(), &ops, &at, &count)
        .expect("a count runs");
    let added = calls
        .lock()
        .unwrap()
        .iter()
        .filter(|(op, _)| op == "store:data/user-carts#add")
        .count();
    assert_eq!(added, 4, "{:?}", calls.lock().unwrap());
    assert_eq!(
        *asked.lock().unwrap(),
        vec![("MayAdd".to_string(), vec![Val::S64(1)]); 2]
    );
}

/// **A data layer's answer that breaks an invariant is the command's
/// failure** (ADR-0179): a cart line of 0, answered by `carts#add`, is never
/// a value the component reads. The contract states where: `ok.lines.*.
/// quantity`.
#[test]
fn a_data_layer_answer_that_breaks_an_invariant_is_the_commands_failure() {
    let c = contract();
    let bytes = component();
    let granted = admitted(&c, &bytes, &ALL).expect("admitted");
    let [interface, function] = export(&c);
    let run = |quantity: i64| {
        let calls: Calls = Arc::default();
        let answered = Val::Result(Ok(Some(Box::new(cart("cortado", quantity)))));
        authorized_call(
            &bytes,
            &c,
            &granted,
            &limits(),
            &host(&calls, answered),
            &[&interface, &function],
            &[item("cortado"), Val::S64(1)],
        )
    };
    let why = run(0).expect_err("a line of 0 is no cart");
    assert!(
        why.contains(
            "`store:data/user-carts#add` answered what breaks an invariant: value 1's \
                      `ok.lines[0].quantity` is 0, and `domain.PositiveInt` holds `value >= 1`"
        ),
        "{why}"
    );
    assert!(run(1).is_ok());
}

#[test]
fn arguments_for_an_export_the_component_does_not_have_are_refused() {
    let c = contract();
    let prepared = engine::Prepared::compile(&component()).expect("compiles");
    let [interface, _] = export(&c);
    let err = prepared
        .arguments(&[&interface, "remove-from-cart"], &[])
        .expect_err("no such function");
    assert!(err.contains("exports no function"), "{err}");
    let err = prepared
        .arguments(&["pw:app/nothing", "add-to-cart"], &[])
        .expect_err("no such instance");
    assert!(err.contains("exports no instance"), "{err}");
}

// --- E10 gate item 3: sustained load --------------------------------------

/// The process's resident set, in KiB, as `ps` reports it on macOS and Linux.
fn resident_kib() -> u64 {
    let out = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .expect("ps runs");
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse()
        .expect("ps prints a number")
}

/// **20,000 calls leave the host's memory where 1,000 left it.**
///
/// Each call admits the component, builds a linker from that call's `Granted`,
/// instantiates it in a fresh store with its own limits, runs it and drops it:
/// what the development server does per request. A leak anywhere in that path
/// (a store kept alive, a linker cached per grant, an allocation in the
/// invocation region that the post-return never resets) grows with the number
/// of calls. The bound is 16 MiB over 19,000 calls, under a kilobyte a call,
/// and the measured growth is printed beside it.
#[test]
fn sustained_calls_leave_memory_flat() {
    let c = contract();
    let bytes = component();
    let prepared = engine::Prepared::compile(&bytes).expect("compiles");
    let granted = admitted(&c, &bytes, &ALL).expect("admitted");
    let [interface, function] = export(&c);
    let returned = Val::Result(Ok(Some(Box::new(cart("cortado", 2)))));
    let calls: Calls = Arc::default();
    let ops = host(&calls, returned.clone());
    let call = |n: i64| {
        let out = prepared
            .call_authorized_within(
                &c,
                &granted,
                &limits(),
                &ops,
                &[&interface, &function],
                // A count from 1: a `PositiveInt` of 0 is refused before the
                // component runs (ADR-0346, argument invariants), and this
                // measures the calls that run.
                &[item(&format!("item-{n}")), Val::S64(n + 1)],
                approve_store_authorization,
            )
            .expect("runs");
        assert_eq!(out, vec![returned.clone()]);
    };

    for n in 0..1_000 {
        call(n);
    }
    calls.lock().unwrap().clear();
    let warm = resident_kib();
    let started = std::time::Instant::now();
    for n in 0..19_000 {
        call(n);
        if n % 1_000 == 0 {
            calls.lock().unwrap().clear();
        }
    }
    let elapsed = started.elapsed();
    let after = resident_kib();
    let grown = after.saturating_sub(warm);
    println!(
        "sustained: 19000 calls after 1000 warm-up -> resident {warm} KiB -> {after} KiB \
         (+{grown} KiB), {:.1} µs per call",
        elapsed.as_secs_f64() * 1e6 / 19_000.0
    );
    assert!(grown < 16 * 1024, "grew {grown} KiB over 19,000 calls");
}
