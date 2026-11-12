//! **E10: resumable handlers compile to JavaScript modules** (`backend::js`).
//!
//! Charter §14 M10 task 2 puts "generated modern JavaScript modules for
//! handlers" first. What a module reads of its captures is exactly what the
//! template IR tells the renderer to serialize, and a module and its event
//! part name one handler.
//!
//! Since 2026-09-25 a handler's body computes (ADR-0058): it is lowered as a
//! query's is, and its commands are awaited in order. Each module here is run
//! under Node against a context that records the commands it sends, so what
//! is held is what the browser would send, not the text that sends it.
//! Everything outside the supported set is refused by name.

use std::collections::BTreeMap;

use pw_core::backend::js::{self, HandlerModule};
use pw_core::backend::wasm::Encoding;
use pw_core::check::Unit;
use pw_core::hir::Hir;
use pw_core::lower::lower_file;
use pw_core::resolve::Workspace;
use pw_core::signatures::Signatures;
use pw_syntax::parse_tree;

fn store_units() -> Vec<Unit> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut paths = Vec::new();
    for d in [
        "packages/pw-std",
        "packages/pw-platform-web",
        "examples/lib",
        "examples/store",
    ] {
        let mut ps: Vec<std::path::PathBuf> = std::fs::read_dir(root.join(d))
            .unwrap_or_else(|e| panic!("{d}: {e}"))
            .map(|e| e.expect("entry").path())
            .filter(|p| p.extension().is_some_and(|x| x == "pw"))
            .collect();
        ps.sort();
        paths.extend(ps);
    }
    paths.push(root.join("examples/domain.pw"));
    paths
        .into_iter()
        .map(|p| {
            let src = std::fs::read_to_string(&p).expect("read");
            Unit {
                path: p.display().to_string(),
                hir: lower_file(&src, &parse_tree(&src).green),
                src,
            }
        })
        .collect()
}

/// Every compiled module, by the behaviour's name; a refusal fails the test.
fn modules(units: &[Unit]) -> BTreeMap<String, HandlerModule> {
    js::compile(units)
        .expect("the program checks")
        .into_iter()
        .map(|c| match c.module {
            Encoding::Encoded(m) => (m.name.clone(), m),
            other => panic!("a handler in `{}` was not compiled: {other}", c.declaration),
        })
        .collect()
}

/// `(name, handler identity, capture paths)` for every event part in the
/// template IR, built with the identities the resume artifacts derived, as
/// `pw emit-template` builds it, and read back from its JSON, as the runtime
/// and the renderer read it.
fn event_parts(units: &[Unit]) -> Vec<(String, String, Vec<String>)> {
    let hirs: Vec<&Hir> = units.iter().map(|u| &u.hir).collect();
    let ws = Workspace::build(&hirs);
    let sigs = Signatures::build(&ws, &hirs);
    let mut handlers = pw_core::template_ir::Handlers::new();
    for (unit, u) in units.iter().enumerate() {
        for (decl, lambda, m, _) in pw_core::resume_artifacts::located(
            &u.src,
            &u.hir,
            &sigs,
            pw_core::resume_artifacts::BUILD,
        ) {
            handlers.insert((unit, decl, lambda), (m.handler, m.capture_paths));
        }
    }
    let ir = serde_json::to_value(pw_core::template_ir::build_with(&hirs, &sigs, &handlers))
        .expect("the IR serializes");
    let mut out = Vec::new();
    let mut stack = vec![&ir];
    while let Some(v) = stack.pop() {
        match v {
            serde_json::Value::Object(o) => {
                if o.contains_key("event") && o.contains_key("handler") {
                    let text = |k: &str| o[k].as_str().unwrap_or_default().to_string();
                    let captures = o
                        .get("captures")
                        .and_then(|c| c.as_array())
                        .map(|c| c.iter().map(|p| p.as_str().unwrap().to_string()).collect())
                        .unwrap_or_default();
                    out.push((text("name"), text("handler"), captures));
                }
                stack.extend(o.values());
            }
            serde_json::Value::Array(a) => stack.extend(a),
            _ => {}
        }
    }
    out.sort();
    out
}

// --- the store ------------------------------------------------------------

/// **What a module sends.** It runs under Node with `captures` as the
/// document would give them, and a context that records each command and
/// each signal set, and answers each command `Ok`, as the runtime's context
/// does for a commit (ADR-0157). `sent` is each command, in order; `set` is
/// each signal written, with its value as JSON carries it; `trap` is the
/// message it stopped with, if it stopped.
fn run(m: &HandlerModule, captures: &str) -> serde_json::Value {
    run_answered(m, captures, "", r#"{"$case":"ok"}"#)
}

/// [`run`], with the context refusing the command `refuse`, as the browser
/// runtime's does on a non-2xx answer: its promise rejects.
fn run_refusing(m: &HandlerModule, captures: &str, refuse: &str) -> serde_json::Value {
    run_answered(m, captures, refuse, r#"{"$case":"ok"}"#)
}

/// [`run`], each command answered `answer`, the server's `result` as the
/// runtime's context hands it over (ADR-0157).
fn run_answered(
    m: &HandlerModule,
    captures: &str,
    refuse: &str,
    answer: &str,
) -> serde_json::Value {
    // One directory each run: two tests may run one module at once, and one's
    // cleanup would take the other's files.
    static RUNS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "pw-handler-{}-{}-{}",
        std::process::id(),
        m.identity,
        RUNS.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&dir).expect("temp");
    std::fs::write(dir.join("handler.mjs"), &m.source).expect("write");
    std::fs::write(
        dir.join("run.mjs"),
        "import { run } from \"./handler.mjs\";\n\
         const sent = [];\n\
         const set = [];\n\
         const context = {\n  captures: JSON.parse(process.argv[2]),\n  \
         command: async (id, args) => {\n    sent.push([id, args]);\n    \
         await new Promise((r) => setTimeout(r, 1));\n    \
         if (id === process.argv[3]) throw new Error(\"refused\");\n    \
         return JSON.parse(process.argv[4]);\n  },\n  \
         set: (name, value) => set.push([name, value]),\n};\n\
         try {\n  await run(context);\n  console.log(JSON.stringify({ sent, set }));\n} catch (e) {\n  \
         console.log(JSON.stringify({ sent, set, trap: String(e.message) }));\n}\n",
    )
    .expect("write");
    let out = std::process::Command::new("node")
        .arg("run.mjs")
        .arg(captures)
        .arg(refuse)
        .arg(answer)
        .current_dir(&dir)
        .output()
        .expect("node runs: a handler's module is tested under Node");
    std::fs::remove_dir_all(&dir).ok();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).expect("one line of JSON")
}

fn sent(v: &serde_json::Value) -> &serde_json::Value {
    &v["sent"]
}

/// How the store's commands are sent again (ADR-0173), as each declares:
/// `retry transport_only(max = 2, jitter = true)`.
const RESEND: &str = "], { retry: { max: 2, jitter: true } });";

#[test]
fn the_stores_handlers_compile_to_the_calls_their_bodies_make() {
    let modules = modules(&store_units());
    assert_eq!(
        modules.keys().collect::<Vec<_>>(),
        [
            // The address page's label, bound (ADR-0142), and its handlers
            // (track `store-accounts`): a rename and a save each test the
            // label first (`address_label`), one name here for two modules.
            "",
            "add_to_cart",
            "address_label",
            // The cart's code applied (track `order-total`).
            "apply_promo",
            // The order page takes back an order awaiting its payment (NEXT
            // 6c, step eight).
            "cancel_order",
            "choose_address",
            // The page of times' two (track `scheduled-orders`): as soon as
            // possible and a window, two modules of one name.
            "choose_time",
            // Its tip: none, 15, 18 or 20 percent, four modules of one name;
            // and an amount, typed, which the handler reads as dollars first
            // (`dollars_typed`) (track `order-total`).
            "choose_tip",
            "clear_cart",
            "decrease_in_cart",
            "dollars_typed",
            "increase_in_cart",
            // The cart's own page places it as an order (ADR-0193), passing
            // the total it shows (track `order-total`).
            "place_order",
            "remove_address",
            "remove_from_cart",
            // The cart's code taken off (track `order-total`).
            "remove_promo",
            // The rate page's send, named for the call it tests first, as
            // `address_label` is (track `reviews`).
            "stars_of"
        ]
    );

    // on:press={resumable(captures = { item }) => match add_to_cart(item, PositiveInt(1)) {
    //     Ok(_) => notice = "",
    //     Err(CartError.ItemUnavailable(_)) => notice = "That item just sold out.",
    //     Err(_) => notice = "That item could not be added.",
    // }}
    let add = &modules["add_to_cart"];
    println!("{}", add.source);
    assert_eq!(add.commands, ["store.page.add_to_cart"]);
    // Sent again where no answer came, as `retry transport_only(max = 2,
    // jitter = true)` says (ADR-0173).
    // The item is sent as it is, its one `Int` checked (ADR-0433).
    assert!(
        add.source.contains(
            "await context.command(\"store.page.add_to_cart\", \
             [((w) => { exact(w[\"price\"]); return w; })(v0), exact(v2)]"
        ) && add.source.contains(RESEND),
        "the item and the quantity, then how it is sent again"
    );
    assert!(
        add.source.contains(&format!(
            "export const name = \"add_to_cart\";\n\
             export const handler = \"{id}\";\n\
             export async function run(context) {{\n\
             \x20 const v0 = context.captures[\"item\"];\n",
            id = add.identity
        )),
        "the captured item, read as it is: its form is the wire's, its `Int` a \
         number (ADR-0433), whole, as the page showed it (ADR-0172), with whether \
         it is available (ADR-0178), its store and its category (ADR-0181)"
    );
    let item = r#"{"item":{"id":"espresso","store_id":"47","name":"Espresso","description":"Short.","price":350,"available":true,"category":{"id":"coffee","name":"Coffee"}}}"#;
    // Answered `Ok`, without the cart (ADR-0157): the notice is cleared. The
    // item is sent as the page showed it, and `PositiveInt(1)` as its
    // representation.
    let ok = run(add, item);
    assert_eq!(
        sent(&ok),
        &serde_json::json!([[
            "store.page.add_to_cart",
            [
                {
                    "id": "espresso",
                    "store_id": "47",
                    "name": "Espresso",
                    "description": "Short.",
                    "price": 350,
                    "available": true,
                    "category": { "id": "coffee", "name": "Coffee" }
                },
                1
            ]
        ]])
    );
    assert_eq!(ok["set"], serde_json::json!([["notice", ""]]));
    // Refused by name: the handler reads the declared error, and says so.
    let refused = run_answered(
        add,
        item,
        "",
        r#"{"$case":"err","value":{"$case":"item-unavailable","value":"espresso"}}"#,
    );
    assert_eq!(
        refused["set"],
        serde_json::json!([["notice", "That item just sold out."]])
    );
    assert!(refused.get("trap").is_none(), "{refused}");
    // Any other refusal is shown too (ADR-0159).
    let expired = run_answered(
        add,
        item,
        "",
        r#"{"$case":"err","value":{"$case":"cart-expired"}}"#,
    );
    assert_eq!(
        expired["set"],
        serde_json::json!([["notice", "That item could not be added."]])
    );
    // Control: an answer that is no declared case traps, rather than being
    // read as one of them.
    let wrong = run_answered(
        add,
        item,
        "",
        r#"{"$case":"err","value":{"$case":"sold-out"}}"#,
    );
    assert_eq!(wrong["trap"], "trap: no such case sold-out");
    assert_eq!(wrong["set"], serde_json::json!([]));

    // on:press={resumable() => match clear_cart() {
    //     Ok(_) => notice = "",
    //     Err(_) => notice = "The cart could not be cleared.",
    // }}
    let clear = &modules["clear_cart"];
    println!("{}", clear.source);
    assert_eq!(clear.commands, ["store.page.clear_cart"]);
    let cleared = run(clear, "{}");
    assert_eq!(
        sent(&cleared),
        &serde_json::json!([["store.page.clear_cart", []]])
    );
    assert_eq!(cleared["set"], serde_json::json!([["notice", ""]]));
    let refused = run_answered(
        clear,
        "{}",
        "",
        r#"{"$case":"err","value":{"$case":"cart-expired"}}"#,
    );
    assert_eq!(
        refused["set"],
        serde_json::json!([["notice", "The cart could not be cleared."]])
    );

    // A line's −, + and Remove (ADR-0172), e.g.
    // on:press={resumable(captures = { line }) => match increase_in_cart(line.item_id) {
    //     Ok(_) => notice = "",
    //     Err(CartError.ItemUnavailable(_)) => notice = "That item just sold out.",
    //     Err(_) => notice = "That line could not be changed.",
    // }}
    // Each sends the line's item id, and says when it is refused.
    let line = r#"{"line":{"item_id":"espresso"}}"#;
    for (name, notice) in [
        ("decrease_in_cart", "That line could not be changed."),
        ("increase_in_cart", "That line could not be changed."),
        ("remove_from_cart", "That line could not be removed."),
    ] {
        let m = &modules[name];
        let command = format!("store.page.{name}");
        assert_eq!(m.commands, [command.as_str()]);
        assert!(
            m.source.contains(RESEND),
            "{name} sends as its command declares"
        );
        let ok = run(m, line);
        assert_eq!(sent(&ok), &serde_json::json!([[command, ["espresso"]]]));
        assert_eq!(ok["set"], serde_json::json!([["notice", ""]]), "{name}");
        let refused = run_answered(
            m,
            line,
            "",
            r#"{"$case":"err","value":{"$case":"cart-expired"}}"#,
        );
        assert_eq!(
            refused["set"],
            serde_json::json!([["notice", notice]]),
            "{name}"
        );
    }
    // An item that sold out since it was added is said so on +, as on Add.
    let sold_out = run_answered(
        &modules["increase_in_cart"],
        line,
        "",
        r#"{"$case":"err","value":{"$case":"item-unavailable","value":"espresso"}}"#,
    );
    assert_eq!(
        sold_out["set"],
        serde_json::json!([["notice", "That item just sold out."]])
    );
}

/// **A module and its event part are one handler.** The identity the runtime
/// fetches by, the name its manifest is looked up by, and the capture paths
/// the renderer serializes all agree with what the module was compiled as and
/// what it reads.
#[test]
fn a_module_and_its_event_part_name_one_handler() {
    let units = store_units();
    let modules = modules(&units);
    let parts = event_parts(&units);
    // The store's page's five, and its cart's page's five (ADR-0190,
    // ADR-0193), and the address page's five (track `store-accounts`): its
    // label's binding, an address chosen, renamed and removed, and one saved.
    // And the cart page's checkout (track `order-total`): the code's and the
    // typed tip's bindings, the code applied and taken off, four tips by
    // percent and one by amount. And the order page's (NEXT 6c, step
    // eight): an order awaiting its payment taken back. And the rate page's
    // (track `reviews`): the five stars chosen, the comment's binding, and the
    // rating sent, plus the page of times' two (track `scheduled-orders`).
    assert_eq!(parts.len(), 34, "{parts:?}");
    // By identity: two of the address page's handlers have one name (both
    // test the label first, `address_label`), and each is its own module.
    let _ = &modules;
    let by_identity: BTreeMap<String, HandlerModule> = js::compile(&units)
        .expect("the program checks")
        .into_iter()
        .map(|c| match c.module {
            Encoding::Encoded(m) => (m.identity.clone(), m),
            other => panic!("a handler in `{}` was not compiled: {other}", c.declaration),
        })
        .collect();
    for (name, identity, captures) in &parts {
        let m = by_identity
            .get(identity)
            .unwrap_or_else(|| panic!("{name}: no module {identity}"));
        assert_eq!(&m.name, name, "{name}: one name");
        for path in captures {
            let read: String = path.split('.').map(|s| format!("[\"{s}\"]")).collect();
            assert!(
                m.source.contains(&format!("context.captures{read}")),
                "{name} reads `{path}`, which the document carries"
            );
        }
    }
    let captures: BTreeMap<&str, &Vec<String>> =
        parts.iter().map(|(n, _, c)| (n.as_str(), c)).collect();
    assert_eq!(
        captures["add_to_cart"],
        &["item"],
        "the whole item: it is sent, and the line it makes shows its name and \
         price (ADR-0172)"
    );
    for name in ["decrease_in_cart", "increase_in_cart", "remove_from_cart"] {
        assert_eq!(
            captures[name],
            &["line.item_id"],
            "{name}: the path read, not the whole line"
        );
    }
    assert!(
        captures["clear_cart"].is_empty(),
        "reads nothing it captured"
    );
}

/// **Every command a handler calls is a component the contracts name.** The
/// handler calls by `contract::component_id`, and so do the contracts; a
/// second derivation that disagreed would send a press to a component no host
/// has a contract for.
#[test]
fn a_handler_calls_its_commands_by_the_contracts_own_ids() {
    let units = store_units();
    let hirs: Vec<&Hir> = units.iter().map(|u| &u.hir).collect();
    let ws = Workspace::build(&hirs);
    let sigs = Signatures::build(&ws, &hirs);
    let ids: Vec<String> = pw_core::contract::contracts(&hirs, &sigs, &ws)
        .into_iter()
        .map(|c| c.component_id)
        .collect();
    for m in modules(&units).values() {
        for c in &m.commands {
            assert!(ids.contains(c), "`{c}` has no contract");
        }
    }
}

#[test]
fn a_program_that_does_not_check_compiles_no_handlers() {
    // The store without the standard packages it imports: it does not check,
    // so nothing is compiled, although every handler in it would be.
    let units: Vec<Unit> = store_units()
        .into_iter()
        .filter(|u| !u.path.contains("packages/"))
        .collect();
    let err = js::compile(&units).expect_err("does not check");
    assert!(err.contains("does not check"), "{err}");
}

// --- what a handler computes, and the refusals --------------------------------

const SHOP: &str = "\
module shop.ui

opaque type Qty = Int
opaque type Sku = String
type Item = Item { id: Sku, name: String, stock: Int }
type Pack = Pack { n: Int }
type Size = Small | Large
type Crate = Crate { size: Size }
type Hours = Hours { opensMinute: Int }
type Tree = Leaf | Branch(Int, List<Tree>)
type Grove = Grove { tree: Tree }

command Buy(id: Sku, qty: Qty) -> Int { 0 }
command Rename(id: Sku, name: String) -> Int { 0 }
command Ship(p: Pack) -> Int { 0 }
command Order(item: Item) -> Int { 0 }
command Tag(names: List<String>, packs: List<Pack>) -> Int { 0 }
command Resent(id: Sku) -> Int
    idempotent_by Sku
    retry         bounded_exponential(max = 3)
{
    0
}
command Choose(s: Size) -> Int { 0 }
command Stack(c: Crate) -> Int { 0 }
command Open(h: Hours) -> Int { 0 }
command Plant(t: Tree) -> Int { 0 }
command Replant(g: Grove) -> Int { 0 }
query Look(id: Sku) -> Int { 0 }
fn helper(x: Int) -> Int { x }
fn colours(n: Int) -> List<String> { if n > 0 { colours(n - 1) } else { [\"red\", \"blue\"] } }

page Shop(items: List<Item>, sku: Sku) {
    view {
        <ul>
            {#each items as item (item.id)}
                <li>
                    <button on:press={resumable(captures = { item }) => HANDLER}>Go</button>
                </li>
            {/each}
        </ul>
    }
}
";

/// What the document carries for `SHOP`'s item.
const ITEM: &str = r#"{"item":{"id":"a","name":"nut","stock":5}}"#;

/// The one handler in `SHOP` with `HANDLER` replaced, compiled without the
/// check gate: these programs are about the backend, and what it compiles or
/// refuses must hold whatever produced the program.
fn shop(handler: &str) -> Encoding<HandlerModule> {
    let src = SHOP.replace("HANDLER", handler);
    let hir = lower_file(&src, &parse_tree(&src).green);
    let hirs = vec![&hir];
    let ws = Workspace::build(&hirs);
    let sigs = Signatures::build(&ws, &hirs);
    let mut all = js::handlers(&hirs, &[src.as_str()], &ws, &sigs);
    assert_eq!(all.len(), 1, "one handler in `{handler}`");
    all.remove(0).module
}

fn compiled(handler: &str) -> HandlerModule {
    match shop(handler) {
        Encoding::Encoded(m) => m,
        other => panic!("`{handler}` did not compile: {other}"),
    }
}

/// What the one handler in `SHOP` sends, run on `ITEM`.
fn sends(handler: &str) -> serde_json::Value {
    let m = compiled(handler);
    println!("{}", m.source);
    run(&m, ITEM)
}

fn refused(handler: &str, construct: &str, reason: &str) {
    match shop(handler) {
        Encoding::Unsupported {
            construct: c,
            reason: r,
        } => {
            println!("refused `{handler}`: {c}: {r}");
            assert_eq!(c, construct, "`{handler}`: {r}");
            assert!(r.contains(reason), "`{handler}`: {r}");
        }
        other => panic!("`{handler}` must be refused as {construct}, got {other:?}"),
    }
}

#[test]
fn an_opaque_constructor_sends_its_representation() {
    assert_eq!(
        sent(&sends("Buy(item.id, Qty(2))")),
        &serde_json::json!([["shop.ui.Buy", ["a", 2]]])
    );
}

#[test]
fn each_path_read_is_read_where_the_document_carries_it() {
    let m = compiled("Rename(item.id, item.name)");
    assert!(
        m.source.contains(r#"context.captures["item"]["id"]"#),
        "{}",
        m.source
    );
    assert!(
        m.source.contains(r#"context.captures["item"]["name"]"#),
        "{}",
        m.source
    );
    assert_eq!(
        sent(&run(&m, ITEM)),
        &serde_json::json!([["shop.ui.Rename", ["a", "nut"]]])
    );
    // A string literal sends its VALUE. Its token keeps the quotes, and a
    // module that sent the token would send `"new name"` with the quotes in it.
    assert_eq!(
        sent(&sends("Rename(item.id, \"new name\")")),
        &serde_json::json!([["shop.ui.Rename", ["a", "new name"]]])
    );
}

#[test]
fn a_string_with_escapes_sends_its_value() {
    // The decoded value (ADR-0049): the quotes are the value's, a line feed
    // is one, `\{` and `\}` are braces, and `\u{41}` is `A`.
    assert_eq!(
        sent(&sends("Rename(item.id, \"new \\\"name\\\"\\n\")")),
        &serde_json::json!([["shop.ui.Rename", ["a", "new \"name\"\n"]]])
    );
    assert_eq!(
        sent(&sends("Rename(item.id, \"\\{x\\} \\u{41}\")")),
        &serde_json::json!([["shop.ui.Rename", ["a", "{x} A"]]])
    );
}

/// **A handler computes what it sends** (ADR-0058). Refused until
/// 2026-09-25: an operator, and a call to the program's own function.
#[test]
fn a_handler_computes_what_it_sends() {
    assert_eq!(
        sent(&sends("Buy(item.id, Qty(1 + 1))")),
        &serde_json::json!([["shop.ui.Buy", ["a", 2]]])
    );
    // The captured `Int` is read as the number it is, and sent as one
    // (ADR-0433).
    assert_eq!(
        sent(&sends("Buy(item.id, Qty(helper(item.stock) * 2))")),
        &serde_json::json!([["shop.ui.Buy", ["a", 10]]])
    );
    assert_eq!(
        sent(&sends("Rename(item.id, \"{item.name}: {item.stock}\")")),
        &serde_json::json!([["shop.ui.Rename", ["a", "nut: 5"]]])
    );
}

/// **A list of literals is a value of the handler's module** (ADR-0427), as
/// a query's is: made once, its literals not written where it is read, and
/// sent as any list is.
#[test]
fn a_handlers_list_of_literals_is_a_value_of_its_module() {
    let m = compiled("Tag([\"red\", \"blue\"], [])");
    assert_eq!(
        m.source
            .matches("Object.freeze([\"red\", \"blue\"])")
            .count(),
        1,
        "{}",
        m.source
    );
    assert_eq!(m.source.matches("\"red\"").count(), 1, "{}", m.source);
    assert_eq!(
        sent(&run(&m, ITEM)),
        &serde_json::json!([["shop.ui.Tag", [["red", "blue"], []]]])
    );
    // In a function the handler calls, a value of the handler's module too:
    // recursive, so compiled beside the handler as well as inlined into it.
    let called = compiled("Tag(colours(1), [])");
    assert_eq!(
        called
            .source
            .matches("Object.freeze([\"red\", \"blue\"])")
            .count(),
        1,
        "{}",
        called.source
    );
    assert_eq!(
        sent(&run(&called, ITEM)),
        &serde_json::json!([["shop.ui.Tag", [["red", "blue"], []]]])
    );
}

#[test]
fn a_handler_branches_between_commands() {
    let m = compiled(
        "if item.stock > 3 { Buy(item.id, Qty(item.stock - 3)) } else { Rename(item.id, \"low\") }",
    );
    assert_eq!(m.commands, ["shop.ui.Buy", "shop.ui.Rename"]);
    assert_eq!(
        sent(&run(&m, ITEM)),
        &serde_json::json!([["shop.ui.Buy", ["a", 2]]])
    );
    assert_eq!(
        sent(&run(&m, r#"{"item":{"id":"b","name":"x","stock":2}}"#)),
        &serde_json::json!([["shop.ui.Rename", ["b", "low"]]])
    );
}

#[test]
fn a_handler_calls_several_commands_in_order() {
    assert_eq!(
        sent(&sends(
            "for q in [1, 2, 3] { Buy(item.id, Qty(q * item.stock)) }"
        )),
        &serde_json::json!([
            ["shop.ui.Buy", ["a", 5]],
            ["shop.ui.Buy", ["a", 10]],
            ["shop.ui.Buy", ["a", 15]]
        ])
    );
}

#[test]
fn an_int_a_javascript_number_cannot_carry_stops_the_handler_before_it_sends() {
    // 2^53 is exact; one past it is not, and nothing is sent.
    assert_eq!(
        sent(&sends("Buy(item.id, Qty(9007199254740992))")),
        &serde_json::json!([["shop.ui.Buy", ["a", 9007199254740992u64]]])
    );
    for handler in [
        "Buy(item.id, Qty(9007199254740993))",
        "Buy(item.id, Qty(item.stock * 9007199254740992))",
    ] {
        let out = sends(handler);
        assert_eq!(sent(&out), &serde_json::json!([]), "{handler}");
        assert!(
            out["trap"]
                .as_str()
                .unwrap_or_default()
                .contains("cannot carry exactly"),
            "{handler}: {out}"
        );
    }
    // An `Int` overflow traps as it does everywhere (ADR-0039).
    let out = sends("Buy(item.id, Qty(9223372036854775807 + item.stock))");
    assert_eq!(sent(&out), &serde_json::json!([]));
    assert!(
        out["trap"]
            .as_str()
            .unwrap_or_default()
            .contains("overflow"),
        "{out}"
    );
}

#[test]
fn a_handler_that_calls_no_command_is_refused() {
    // A handler may change a signal instead since ADR-0130; this one does
    // neither.
    refused(
        "helper(1)",
        "a handler that calls no command and changes no signal",
        "does neither",
    );
    refused(
        "Look(item.id)",
        "a query called from a handler",
        "answers on the server",
    );
}

#[test]
fn a_value_the_handler_did_not_capture_is_not_read() {
    // `sku` is the page's parameter. The document does not carry it for this
    // handler, so the handler would read nothing.
    assert!(
        !matches!(shop("Buy(sku, Qty(1))"), Encoding::Encoded(_)),
        "a value no capture carries compiled"
    );
}

/// **A command is sent again as its `retry` clause says** (ADR-0173): the
/// handler passes the policy where it sends the command, and the runtime,
/// which alone knows that no answer came, sends it again. A command with no
/// clause is passed none.
#[test]
fn a_command_is_sent_again_as_its_retry_clause_says() {
    let m = compiled("Resent(item.id)");
    assert!(
        m.source.contains(
            "await context.command(\"shop.ui.Resent\", [v0], \
             { retry: { max: 3, jitter: false } });"
        ),
        "{}",
        m.source
    );
    assert_eq!(
        sent(&run(&m, ITEM)),
        &serde_json::json!([["shop.ui.Resent", ["a"]]])
    );
    let once = compiled("Buy(item.id, Qty(1))");
    assert!(!once.source.contains("retry"), "{}", once.source);
}

/// **A record is sent field by field, and a list element by element**
/// (ADR-0172), each field by its name and each `Int` exact, as a command's
/// argument is. Refused until 2026-10-03, when `add_to_cart` came to take
/// the item the page showed.
#[test]
fn a_record_or_a_list_is_sent_as_json_carries_it() {
    assert_eq!(
        sent(&sends(
            "Tag([\"a\", item.name], [Pack { n: item.stock }, Pack { n: 2 }])"
        )),
        &serde_json::json!([["shop.ui.Tag", [["a", "nut"], [{ "n": 5 }, { "n": 2 }]]]])
    );
    assert_eq!(
        sent(&sends("Ship(Pack { n: item.stock })")),
        &serde_json::json!([["shop.ui.Ship", [{ "n": 5 }]]])
    );
    // A captured record, whole, its `Sku` as its representation.
    assert_eq!(
        sent(&sends("Order(item)")),
        &serde_json::json!([["shop.ui.Order", [{ "id": "a", "name": "nut", "stock": 5 }]]])
    );
    // An `Int` a JavaScript number cannot carry stops the handler before it
    // sends, in a field as anywhere.
    let out = sends("Ship(Pack { n: 9007199254740993 })");
    assert_eq!(sent(&out), &serde_json::json!([]));
    assert!(
        out["trap"]
            .as_str()
            .unwrap_or_default()
            .contains("cannot carry exactly"),
        "{out}"
    );
}

#[test]
fn a_command_parameter_a_browser_cannot_send_is_refused() {
    // A case of a type that contains itself (ADR-0331): a host reads none
    // from what a browser writes.
    refused(
        "Plant(Tree.Leaf)",
        "a command parameter a browser cannot send",
        "`Tree`",
    );
    // A record holding what a browser cannot send, the record refused by its
    // own name. Until 2026-10-13 no test held one once ADR-0331 sent cases,
    // and its mutant survived train-4's run.
    refused(
        "Replant(Grove { tree: Tree.Leaf })",
        "a command parameter a browser cannot send",
        "`Grove`",
    );
    // A record whose field's name would not come back from its WIT name:
    // `opens-minute` is found as `opens_minute`.
    refused(
        "Open(Hours { opensMinute: 1 })",
        "a command parameter a browser cannot send",
        "`Hours`",
    );
}

/// **A case, and a record that holds one, are sent as the browser holds
/// them** (ADR-0331): `{ $case, value }`, the case by its WIT name. Until
/// 2026-10-13 each was refused here, "a host reads none from what a browser
/// writes".
#[test]
fn a_case_and_a_record_holding_one_are_sent_as_the_browser_holds_them() {
    assert_eq!(
        sent(&sends("Choose(Size.Small)")),
        &serde_json::json!([["shop.ui.Choose", [{ "$case": "small" }]]])
    );
    assert_eq!(
        sent(&sends("Stack(Crate { size: Size.Large })")),
        &serde_json::json!([["shop.ui.Stack", [{ "size": { "$case": "large" } }]]])
    );
}

#[test]
fn a_command_inside_a_function_is_refused() {
    // Awaited inside a function value, a command would not be awaited by the
    // handler; and a function value's code does not receive the captures.
    refused(
        "{\n    let buy: fn(Sku, Int) -> Int = (id, q) => Buy(id, Qty(q))\n    buy(item.id, 1)\n}",
        "a command called inside a function",
        "`Buy`",
    );
    refused(
        "{\n    let stock: fn(Int) -> Int = q => q * item.stock\n    Buy(item.id, Qty(stock(1)))\n}",
        "a captured value read inside a function value",
        "`item`",
    );
}

#[test]
fn a_refused_command_stops_the_handler_before_the_next() {
    // Each command is awaited: the second is not sent when the first is
    // refused, and the handler fails where the runtime marks it.
    let m = compiled("{\n    Buy(item.id, Qty(1))\n    Rename(item.id, item.name)\n}");
    assert_eq!(
        sent(&run(&m, ITEM)),
        &serde_json::json!([["shop.ui.Buy", ["a", 1]], ["shop.ui.Rename", ["a", "nut"]]])
    );
    let out = run_refusing(&m, ITEM, "shop.ui.Buy");
    assert_eq!(sent(&out), &serde_json::json!([["shop.ui.Buy", ["a", 1]]]));
    assert_eq!(out["trap"], "refused");
}

/// **A handler that discards its command's answer with `_` sends it**
/// (ADR-0250, ruling 0099-a): `let _ = ..` binds nothing, and the backend
/// still computes what it is given. Named for its call, as the discard by
/// name is.
#[test]
fn a_handler_that_discards_its_answer_with_underscore_sends_it() {
    let m = compiled("{ let _ = Buy(item.id, Qty(2)) }");
    assert!(
        m.source.contains("export const name = \"Buy\";"),
        "{}",
        m.source
    );
    assert_eq!(
        sent(&run(&m, ITEM)),
        &serde_json::json!([["shop.ui.Buy", ["a", 2]]])
    );
}

/// **A handler that discards its command's answer by name compiles**
/// (ADR-0159): `let _ignored = ..`, ADR-0099's discard, as its last
/// statement. The block is the unit value. Until 2026-10-03 the backend
/// refused a block ending in a binding, so the one discard the checker
/// accepts could not be built. And it is named for its call, as a `match`
/// on it is.
#[test]
fn a_handler_that_discards_its_answer_by_name_sends_it() {
    let m = compiled("{ let _bought = Buy(item.id, Qty(2)) }");
    assert!(
        m.source.contains("export const name = \"Buy\";"),
        "{}",
        m.source
    );
    assert_eq!(
        sent(&run(&m, ITEM)),
        &serde_json::json!([["shop.ui.Buy", ["a", 2]]])
    );
    // Control: where a value is wanted, a block ending in a binding has
    // none to give.
    refused(
        "Buy(item.id, Qty(helper({ let y = 1 })))",
        "a block ending in a binding",
        "the unit value",
    );
}
