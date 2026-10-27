//! **A read is a bounded number of statements on PostgreSQL** (NEXT 4b, W9,
//! the integrator's finding of 2026-10-12): every operation of the store's
//! data layer, and every page and command of the store, counted (as Django's
//! `assertNumQueries` and Rails' `assert_queries_count` count them) at the
//! seed and with fifty stores more, each count pinned and equal at both.
//! Each operation's count is "operation X makes k statements per call": the
//! shape a page's or a command's budget composes from (NEXT 4c), and the
//! conformance a layer's adapter is held to. On PostgreSQL alone, which
//! counts its statements (`store_pg.rs`' `Counted`); skipped without
//! `PW_STORE_DATABASE_URL`.

use super::*;

/// The reader the operations act for, a user's handle.
const ADA: &str = "ada";

fn database() -> Option<String> {
    match std::env::var("PW_STORE_DATABASE_URL") {
        Ok(url) if !url.is_empty() => Some(url),
        _ => {
            eprintln!("skipped: PW_STORE_DATABASE_URL is not set");
            None
        }
    }
}

/// **Fifty stores more**, each as a seeded one is: a category, two items,
/// open the day round, San Francisco's fees. Listed before the seeded ones,
/// so a loop over the stores that stops at the store it wants still meets
/// every one of them.
const FIFTY_MORE: &str = "
    INSERT INTO stores (id, position, name, description, cuisine, opens_minute,
                        closes_minute, lat_e6, lon_e6, radius_m)
    SELECT 'more-' || n, -n, 'More ' || n, 'One more store.', 'Coffee', 420, 1140,
           37780000 + n, -122400000 - n, 3000
    FROM generate_series(1, 50) AS n;
    INSERT INTO categories (store, id, name)
    SELECT 'more-' || n, 'coffee', 'Coffee' FROM generate_series(1, 50) AS n;
    INSERT INTO menu_items (store, id, position, name, description, price, category,
                            served, available)
    SELECT 'more-' || n, 'more-' || n || '-' || k, k, 'Coffee ' || k, 'A coffee.', 300 + k,
           'coffee', 'hot', true
    FROM generate_series(1, 50) AS n, generate_series(1, 2) AS k;
    INSERT INTO store_hours (store, weekday, opens_minute, closes_minute)
    SELECT 'more-' || n, d, 0, 0 FROM generate_series(1, 50) AS n, generate_series(1, 7) AS d;
    INSERT INTO store_fees
    SELECT 'more-' || n, 299, 150000, 300, 1200, 1000, 250, 86250, false, false
    FROM generate_series(1, 50) AS n;
";

/// The reader's state every operation is read against: a cart with a line
/// from each seeded store, and an address chosen.
fn readers_state(c: &mut postgres::Client) {
    let place = crate::places::PLACES[0].id;
    c.execute(
        "INSERT INTO cart_lines (owner, position, item, name, quantity, price) VALUES \
         ($1, 0, 'espresso', 'Espresso', 2, 350), ($1, 1, 'drip', 'Drip Coffee', 1, 300)",
        &[&ADA],
    )
    .expect("a cart");
    c.execute(
        "INSERT INTO addresses (owner, id, position, label, place, chosen) \
         VALUES ($1, 'addr-1', 0, 'Home', $2, true)",
        &[&ADA, &place],
    )
    .expect("an address");
    // TRACK SEAM (reviews): another reader, whose order from store 47 was
    // delivered, so a rating is read and written whole.
    c.batch_execute(&format!(
        "INSERT INTO orders (owner, status) VALUES ('{BEA}', 'delivered');
         INSERT INTO order_parts (order_id, position, store, subtotal, discount, funded_by,
                                  delivery, service, small_order, taxable, tax_rate, tax)
         SELECT id, 0, '47', 350, 0, NULL, 299, 300, 250, 350, 86250, 30
         FROM orders WHERE owner = '{BEA}';"
    ))
    .expect("a delivered order");
}

/// TRACK SEAM (reviews): the reader who rates, her order delivered.
const BEA: &str = "bea";

/// Every store's id, as a list a set read takes.
fn all_stores(c: &mut postgres::Client) -> Val {
    Val::List(
        c.query("SELECT id FROM stores ORDER BY position", &[])
            .expect("the stores")
            .iter()
            .map(|r| Val::String(r.get(0)))
            .collect(),
    )
}

fn s(v: &str) -> Val {
    Val::String(v.to_string())
}

/// **Each operation's arguments**, by its name: what a program passes it.
/// An operation not named here fails the test: every one is counted.
fn arguments(op: &str, stores: &Val) -> Option<Vec<Val>> {
    let charged = crate::checkout::charged_val(&crate::checkout::Charged {
        parts: Vec::new(),
        tip: 0,
        total: 1000,
        promo: None,
    });
    let op = op.strip_prefix("store:")?;
    Some(match op {
        "data/stores#get"
        | "data/menus#for-store"
        | "data/menus#sections"
        | "data/notices#current"
        | "data/kitchen#prep-minutes"
        | "data/recommendations#for-store"
        | "data/hours#for-store" => vec![s("47")],
        "data/stores#list" | "data/places#list" => vec![],
        "data/stores#nowhere" => vec![s("47")],
        "data/menus#is-available" => vec![s("espresso")],
        "data/menus#in-category" => vec![s("48"), s("hot")],
        "data/carts#current"
        | "data/orders#current"
        | "data/orders#placed"
        | "data/estimates#current"
        | "data/carts#clear"
        | "data/orders#place" => vec![s("s")],
        "data/user-carts#current"
        | "data/user-orders#current"
        | "data/user-orders#placed"
        | "data/user-orders#charged"
        | "data/user-orders#awaited"
        | "data/addresses#list"
        | "data/hours#for-cart"
        | "data/checkout#parts"
        | "data/checkout#chosen"
        | "data/user-carts#clear" => vec![s(ADA)],
        "data/user-estimates#for-store" | "data/coverage#for-store" | "data/coverage#reaches" => {
            vec![s("47"), s(ADA)]
        }
        "data/user-estimates#for-stores" => vec![stores.clone(), s(ADA)],
        "data/hours#for-stores" | "data/fees#for-stores" => vec![stores.clone()],
        "data/promos#find" => vec![s(ADA), s("WELCOME10")],
        "data/search#find" => vec![s("coffee")],
        "data/search#find-for" => vec![s("coffee"), s(ADA)],
        "data/carts#add" => vec![s("s"), s("espresso"), Val::S64(1)],
        "data/carts#increase" | "data/carts#decrease" | "data/carts#remove" => {
            vec![s("s"), s("espresso")]
        }
        "data/user-carts#add" => vec![s(ADA), s("espresso"), Val::S64(1)],
        "data/user-carts#increase" | "data/user-carts#decrease" | "data/user-carts#remove" => {
            vec![s(ADA), s("espresso")]
        }
        "data/addresses#add" => vec![s(ADA), s("Work"), s(crate::places::PLACES[1].id)],
        "data/addresses#rename" => vec![s(ADA), s("addr-1"), s("House")],
        "data/addresses#remove" | "data/addresses#choose" => vec![s(ADA), s("addr-1")],
        "data/checkout#set-tip" => vec![s(ADA), Val::Variant("fifteen".into(), None)],
        "data/checkout#set-promo" => vec![s(ADA), Val::Option(None)],
        // Track `scheduled-orders`: the time chosen, none; an order sent; and
        // an order awaiting its payment is given its time, none.
        "data/checkout#set-time" => vec![s(ADA), Val::Option(None)],
        "data/user-orders#send" => vec![s(ADA), s("ref-1")],
        "data/user-orders#await-payment" => {
            vec![s(ADA), charged, Val::Option(None), Val::Option(None)]
        }
        "data/user-orders#place" => vec![s(ADA), charged, Val::Option(None)],
        "data/user-orders#paying" => vec![
            s(ADA),
            s("ref-1"),
            Val::Record(vec![
                ("id".into(), s("pay-1")),
                ("page".into(), s("https://pay")),
            ]),
        ],
        "data/user-orders#settle" => {
            vec![s(ADA), s("ref-1"), Val::Variant("succeeded".into(), None)]
        }
        "host/carts#join" => vec![s("guest-1"), s(ADA)],
        "host/estimates#set" => vec![s("s"), Val::S64(20), Val::Option(None)],
        "host/kitchen#set-prep" => vec![s("47"), Val::S64(15)],
        "host/menus#change" => vec![s("47"), s("remove"), s("cortado")],
        "host/menus#replace" => vec![s("47"), Val::S64(3)],
        "host/menus#stock" => vec![s("espresso"), Val::Bool(false)],
        "host/notices#post" => vec![s("47"), s("Closed early.")],
        "host/orders#set-status" => vec![s(ADA), Val::Option(None)],
        "host/recommendations#curate" => vec![Val::Option(None)],
        "host/fees#set" => vec![
            s("47"),
            crate::checkout::fees_val(&crate::checkout::seed_fees()),
        ],
        "host/hours#set" => vec![s("47"), crate::hours::hours_val(&crate::hours::seed())],
        "host/promos#set" => vec![crate::checkout::promo_val(
            &crate::checkout::seed_promos()[0],
        )],
        // TRACK SEAM (reviews): every store's ratings at once, a store's
        // reviews, and the reader's order rated.
        "data/ratings#for-stores" => vec![stores.clone()],
        "data/ratings#reviews" => vec![s("47")],
        "data/ratings#for-order" => vec![s(BEA)],
        "data/ratings#rate" => vec![s(BEA), s("47"), Val::S64(4), s("Fine.")],
        _ => return None,
    })
}

/// **Each operation's statements per call**, at one size of the data: every
/// read, each in its own read transaction, and every write, each in a
/// command's transaction that is then dropped, so nothing it writes stays.
fn counts(layer: &crate::store_pg::StorePg, stores: &Val) -> BTreeMap<String, u64> {
    use crate::data::DataLayer;
    let mut out = BTreeMap::new();
    let counted = |name: &str, op: &HostFn| -> u64 {
        let args = arguments(name, stores).unwrap_or_else(|| panic!("`{name}` is not counted"));
        crate::store_pg::STATEMENTS.with(|n| n.set(0));
        // An operation may answer a refusal: what it ran is still counted.
        let _ = op(&args);
        crate::store_pg::STATEMENTS.with(|n| n.get())
    };
    for (name, op) in layer.reads("s", None) {
        let n = counted(&name, &op);
        out.insert(name, n);
    }
    let names: Vec<String> = layer.begin("s").ops().into_keys().collect();
    for name in names {
        if out.contains_key(&name) {
            continue;
        }
        let staged = layer.begin("s");
        let ops = staged.ops();
        let n = counted(&name, &ops[&name]);
        drop(staged);
        out.insert(name, n);
    }
    out
}

/// **Every operation makes as many statements per call with fifty stores
/// more as at the seed**, and as many as pinned here: one call is a bounded
/// number of statements, whatever the number of stores, and a set read
/// (`hours#for-stores`, `fees#for-stores`) whatever the number of keys.
#[test]
fn each_operation_is_a_bounded_number_of_statements_per_call() {
    let Some(url) = database() else { return };
    let layer = crate::store_pg::StorePg::for_test(&url).expect("the layer");
    let mut c = layer.sql();
    readers_state(&mut c);
    let seed = counts(&layer, &all_stores(&mut c));
    c.batch_execute(FIFTY_MORE).expect("fifty stores more");
    let more = counts(&layer, &all_stores(&mut c));
    let table: Vec<String> = seed
        .iter()
        .map(|(op, n)| format!("{op}: {n} at the seed, {} with fifty more", more[op]))
        .collect();
    eprintln!("statements per call:\n  {}", table.join("\n  "));
    assert_eq!(seed, more, "equal at both sizes");
    let pinned: BTreeMap<&str, u64> = PINNED.iter().copied().collect();
    let found: BTreeMap<&str, u64> = seed.iter().map(|(k, v)| (k.as_str(), *v)).collect();
    assert_eq!(found, pinned, "as pinned");
}

/// **What each operation makes per call**, on PostgreSQL: the store's
/// adapter's conformance (NEXT 4c).
// A write's statements grow with what it writes (a line, a period, an
// item), never with the number of stores: `user-orders#await-payment` writes
// the cart's two lines, `hours#set` a week's seven periods, `menus#change`
// the menu's items. `places#list` is the fixed table, no statement.
const PINNED: &[(&str, u64)] = &[
    ("store:data/addresses#add", 4),
    ("store:data/addresses#choose", 3),
    ("store:data/addresses#list", 1),
    ("store:data/addresses#remove", 2),
    ("store:data/addresses#rename", 3),
    ("store:data/carts#add", 4),
    ("store:data/carts#clear", 2),
    ("store:data/carts#current", 1),
    ("store:data/carts#decrease", 2),
    ("store:data/carts#remove", 2),
    ("store:data/checkout#chosen", 1),
    ("store:data/checkout#parts", 2),
    ("store:data/checkout#set-promo", 2),
    // Track `scheduled-orders`: the time chosen, read and written as the
    // tip is.
    ("store:data/checkout#set-time", 2),
    ("store:data/checkout#set-tip", 2),
    ("store:data/coverage#for-store", 2),
    ("store:data/coverage#reaches", 2),
    ("store:data/estimates#current", 1),
    ("store:data/fees#for-stores", 1),
    ("store:data/hours#for-cart", 6),
    ("store:data/hours#for-store", 3),
    ("store:data/hours#for-stores", 3),
    ("store:data/kitchen#prep-minutes", 1),
    ("store:data/menus#for-store", 2),
    ("store:data/menus#in-category", 2),
    ("store:data/menus#is-available", 1),
    ("store:data/menus#sections", 2),
    ("store:data/notices#current", 1),
    ("store:data/orders#current", 1),
    ("store:data/orders#place", 1),
    ("store:data/orders#placed", 1),
    ("store:data/places#list", 0),
    ("store:data/promos#find", 1),
    // TRACK SEAM (reviews): each one statement, `#rate` its order read again
    // and its row written.
    ("store:data/ratings#for-order", 1),
    ("store:data/ratings#for-stores", 1),
    ("store:data/ratings#rate", 2),
    ("store:data/ratings#reviews", 1),
    ("store:data/recommendations#for-store", 3),
    ("store:data/search#find", 2),
    ("store:data/search#find-for", 4),
    ("store:data/stores#get", 1),
    ("store:data/stores#list", 1),
    ("store:data/user-carts#add", 4),
    ("store:data/user-carts#clear", 2),
    ("store:data/user-carts#current", 1),
    ("store:data/user-carts#decrease", 4),
    ("store:data/user-carts#remove", 3),
    ("store:data/user-estimates#for-store", 3),
    ("store:data/user-estimates#for-stores", 3),
    ("store:data/user-orders#await-payment", 14),
    ("store:data/user-orders#awaited", 2),
    ("store:data/user-orders#charged", 1),
    ("store:data/user-orders#current", 1),
    ("store:data/user-orders#paying", 2),
    ("store:data/user-orders#place", 11),
    ("store:data/user-orders#placed", 1),
    // Track `scheduled-orders`: the reader's order read, where it is not
    // there (this reader has none); with an order, its lines and its time
    // too, and its status written.
    ("store:data/user-orders#send", 1),
    ("store:data/user-orders#settle", 2),
    ("store:host/carts#join", 1),
    ("store:host/estimates#set", 1),
    ("store:host/fees#set", 1),
    ("store:host/hours#set", 13),
    ("store:host/kitchen#set-prep", 1),
    ("store:host/menus#change", 6),
    ("store:host/menus#replace", 1),
    ("store:host/menus#stock", 1),
    ("store:host/notices#post", 1),
    ("store:host/orders#set-status", 2),
    ("store:host/promos#set", 1),
    ("store:host/recommendations#curate", 1),
];

/// **The reader's open pages, asking** (ADR-0297): each document of `session`
/// counts as asked for from now for an hour, as an open page's stream asks
/// without pause. A commit tells the documents that asked within `LIVE`
/// (three seconds of the machine's own clock) and passes the rest by, so a page
/// opened a few slow steps before the commit was not read again, and the
/// statements counted for "the open pages read again" were the machine's speed:
/// 42 on a quiet one, 36 or 31 on a loaded one. An hour ahead is a time no
/// step reaches.
fn open_pages_ask(s: &Server, session: &str) {
    let asked = std::time::Instant::now() + std::time::Duration::from_secs(3600);
    let mut open = s.pending.lock().expect("pending");
    for doc in documents_of(&open, session) {
        if let Some(waiting) = open.get_mut(&doc) {
            waiting.seen = asked;
        }
    }
}

/// **What a page or a command ran**, each in turn, on a store served on
/// PostgreSQL, with fifty stores more where `more` says: the reader adds a
/// line from each seeded store, saves an address, chooses a tip, and reads
/// every page; then places the order.
fn steps(more: bool) -> Vec<(String, u64)> {
    let s = served_from_patches_in("examples", |app| app.to_string(), &[]);
    if more {
        s.data
            .execute_for_test(FIFTY_MORE)
            .expect("fifty stores more");
    }
    let mut out = Vec::new();
    let mut step = |name: &str, run: &dyn Fn()| {
        let before = s.data.statements().expect("a layer that counts");
        run();
        out.push((
            name.to_string(),
            s.data.statements().expect("counted") - before,
        ));
    };
    step("add_to_cart (47)", &|| {
        s.command(
            "store.page.add_to_cart",
            "a",
            &add_shown("espresso", 1),
            false,
        )
        .expect("added")
    });
    step("add_to_cart (48)", &|| {
        s.command("store.page.add_to_cart", "a", &add_shown("drip", 1), false)
            .expect("added")
    });
    step("add_address", &|| {
        s.command(
            "store.page.add_address",
            "a",
            &[s_val("Home"), s_val(crate::places::PLACES[0].id)],
            false,
        )
        .expect("saved")
    });
    step("choose_tip", &|| {
        s.command(
            "store.page.choose_tip",
            "a",
            &[Val::Variant("fifteen".into(), None)],
            false,
        )
        .expect("chosen")
    });
    // Track `scheduled-orders`: as soon as possible chosen, which this
    // scenario's order is for; and the page of times read below.
    step("choose_time (as soon as possible)", &|| {
        s.command("store.page.choose_time", "a", &[Val::Option(None)], false)
            .expect("chosen")
    });
    for path in [
        "/",
        "/stores/47",
        "/cart",
        "/search?q=coffee",
        "/stores/47/address",
        "/cart/when",
    ] {
        step(path, &|| {
            fetched_as(&s, path, Some("a"));
        });
        open_pages_ask(&s, "a");
    }
    // `place_order`, pinned apart (the integrator's ruling on NEXT 4b): its
    // transaction's operations, its commit (what it hands the outbox, its
    // interaction's record, the outbox read back), and the session's
    // queries read again after its commit.
    let total = s.shown_total("a");
    let (txns, all, by_op) = (
        s.data.statements_by_transaction().len(),
        s.data.statements().expect("counted"),
        s.data.statements_by_op(),
    );
    open_pages_ask(&s, "a");
    s.command_answered(
        "store.page.place_order",
        "a",
        std::slice::from_ref(&total),
        None,
    )
    .expect("answered");
    let ran = s.data.statements_by_transaction()[txns..].to_vec();
    let after = s.data.statements().expect("counted") - all;
    // One transaction: this server takes no payments, so `place_order`
    // places the order at once, `user-orders#settle` in its own (ADR-0332).
    let [own] = ran[..] else {
        panic!("place_order's one transaction: {ran:?}")
    };
    let (mut in_ops, mut carts_again) = (0, 0);
    for (op, (calls, statements)) in s.data.statements_by_op() {
        let (c0, r0) = by_op.get(&op).copied().unwrap_or_default();
        if calls > c0 {
            // What its statements are made of, by operation (NEXT 4c's
            // baseline).
            eprintln!(
                "place_order: {op}: {} calls, {} statements",
                calls - c0,
                statements - r0
            );
            if op.starts_with("command ") {
                in_ops += statements - r0;
            }
            // The cart every page's layout shows, read again after the
            // commit (NEXT 4c's render merge).
            if op == "read store:data/user-carts#current" {
                carts_again = calls - c0;
            }
        }
    }
    out.push((
        "place_order, taking no payments: its operations".to_string(),
        in_ops,
    ));
    out.push((
        "place_order, taking no payments: its commit".to_string(),
        own - in_ops,
    ));
    out.push((
        "place_order, taking no payments: its queries read again".to_string(),
        after - own,
    ));
    out.push((
        "place_order, taking no payments: user-carts#current called again".to_string(),
        carts_again,
    ));
    let mut step = |name: &str, run: &dyn Fn()| {
        let before = s.data.statements().expect("a layer that counts");
        run();
        out.push((
            name.to_string(),
            s.data.statements().expect("counted") - before,
        ));
    };
    step("/order", &|| {
        fetched_as(&s, "/order", Some("a"));
    });
    open_pages_ask(&s, "a");
    // TRACK SEAM (reviews): the order delivered by the kitchen's route, no
    // reader's step; then store 47 rated, and its rate page read.
    let delivered = posted(&s, "/bench/order?status=delivered", "a", "", "");
    assert!(delivered.starts_with("HTTP/1.1 200"), "{delivered}");
    // `rate_order` apart, as `place_order` is: its transaction (its
    // operations and its commit), and the reader's open pages read again
    // after it, which its cart's event regenerates (ADR-0218's session
    // entry): every page the scenario left open.
    let (txns, all, by_op) = (
        s.data.statements_by_transaction().len(),
        s.data.statements().expect("counted"),
        s.data.statements_by_op(),
    );
    open_pages_ask(&s, "a");
    let rated = s
        .command_answered(
            "store.rate.rate_order",
            "a",
            &[s_val("47"), Val::S64(4), s_val("Fine.")],
            None,
        )
        .expect("answered");
    assert!(rated.committed, "{rated:?}");
    let ran = s.data.statements_by_transaction()[txns..].to_vec();
    let after = s.data.statements().expect("counted") - all;
    let [own] = ran[..] else {
        panic!("rate_order's one transaction: {ran:?}")
    };
    // What its statements are made of, by operation, as `place_order`'s
    // (NEXT 4c's render merge), and the two reads every open page of the
    // reader's makes with one key: the cart its layout shows, and its order.
    let (mut carts_again, mut orders_again) = (0, 0);
    for (op, (calls, statements)) in s.data.statements_by_op() {
        let (c0, r0) = by_op.get(&op).copied().unwrap_or_default();
        if calls > c0 {
            eprintln!(
                "rate_order: {op}: {} calls, {} statements",
                calls - c0,
                statements - r0
            );
            match op.as_str() {
                "read store:data/user-carts#current" => carts_again = calls - c0,
                "read store:data/user-orders#current" => orders_again = calls - c0,
                _ => {}
            }
        }
    }
    out.push(("rate_order: its transaction".to_string(), own));
    out.push((
        "rate_order: its reader's open pages read again".to_string(),
        after - own,
    ));
    out.push((
        "rate_order: user-carts#current called again".to_string(),
        carts_again,
    ));
    out.push((
        "rate_order: user-orders#current called again".to_string(),
        orders_again,
    ));
    let mut step = |name: &str, run: &dyn Fn()| {
        let before = s.data.statements().expect("a layer that counts");
        run();
        out.push((
            name.to_string(),
            s.data.statements().expect("counted") - before,
        ));
    };
    step("/order/rate/47", &|| {
        fetched_as(&s, "/order/rate/47", Some("a"));
    });
    out
}

fn s_val(v: &str) -> Val {
    Val::String(v.to_string())
}

/// **Every page and command of the store runs as many statements with fifty
/// stores more as at the seed**, and as many as pinned: what a reader's
/// visit costs the database does not grow with the stores it does not read.
#[test]
fn each_page_and_command_is_a_bounded_number_of_statements() {
    if std::env::var("PW_STORE_TEST_LAYER").as_deref() != Ok("postgres") {
        eprintln!("skipped: the store is not served on PostgreSQL");
        return;
    }
    let seed = steps(false);
    let more = steps(true);
    eprintln!("statements per page and command:\n  {seed:?}\n  {more:?}");
    assert_eq!(seed, more, "equal at both sizes");
    let pinned: Vec<(String, u64)> = PAGES.iter().map(|(k, v)| (k.to_string(), *v)).collect();
    assert_eq!(seed, pinned, "as pinned");
}

/// `choose_tip` emitting one event more (`UserOrderChanged`), for the
/// control below.
fn one_more_event(app: &str) -> String {
    let at = app.find("command choose_tip(").expect("choose_tip");
    let emits = "    emits         UserCartChanged(current_user())\n";
    let found = at + app[at..].find(emits).expect("its emits");
    format!(
        "{}    emits         UserCartChanged(current_user()), UserOrderChanged(current_user())\n{}",
        &app[..found],
        &app[found + emits.len()..]
    )
}

/// `choose_tip`'s transaction's statements, on a store built with `change`.
fn choose_tip_counted(change: fn(&str) -> String) -> u64 {
    let s = served_from_patches_in("examples", change, &[]);
    s.command(
        "store.page.choose_tip",
        "a",
        &[Val::Variant("fifteen".into(), None)],
        false,
    )
    .expect("chosen");
    *s.data
        .statements_by_transaction()
        .last()
        .expect("its transaction")
}

/// **A command's commit is counted with it** (the integrator's ruling on
/// NEXT 4b): what it hands the outbox is a statement each, so the same
/// command emitting one event more makes one statement more.
#[test]
fn a_commands_commit_is_counted_with_it() {
    if std::env::var("PW_STORE_TEST_LAYER").as_deref() != Ok("postgres") {
        eprintln!("skipped: the store is not served on PostgreSQL");
        return;
    }
    let plain = choose_tip_counted(|app| app.to_string());
    let more = choose_tip_counted(one_more_event);
    assert_eq!(more, plain + 1, "one event more, one statement more");
}

/// **What each page and command runs**, on PostgreSQL, in the order
/// [`steps`] runs them.
// A command's count is its transaction's, its commit's statements with it
// (what it hands the outbox, its interaction's record, the outbox read
// back). `place_order`'s three apart, where the deployment takes no
// payments: `user-orders#settle` (7) is among its operations, the order
// placed at once (ADR-0332).
//
// NEXT 4c's render merge (track `render-merge`): the queries read again
// after `place_order`'s commit are one render, the drain's read of the cart
// and the session's six documents, so an operation called with the same
// arguments by several of them is called once there: `user-carts#current`,
// which every page's layout reads, once where seven times (its row below),
// `hours#for-cart` (6 statements) once where twice, and `checkout#chosen`
// once where twice (the cart page and the page of times), and 32 statements
// became 19. And the reader's eight open pages read again after
// `rate_order` are one render: `user-carts#current` once where eight times,
// `user-orders#current` (2 statements) once where twice (their rows below),
// `hours#for-cart` once where twice and `checkout#chosen` once where twice,
// and 53 statements became 37. Measured with the merge and without it (its
// kept answers never given, a temporary edit, reverted) on train-11,
// 2026-10-19; on master before scheduled-orders, 21 -> 16 and 42 -> 34
// (2026-10-17). No other count moved: `hours#for-stores` and
// `ratings#for-stores`, called three times after `rate_order`, are called
// with other stores each time.
// Track `second-host` (ADR-0369): and, where a commit hands the outbox
// anything, the outbox's lock and its notify, one statement before its
// rows take their ids (`hosts::ordered`), so every host reads them in
// commit order: each command here one more than before, each page the
// same.
const PAGES: &[(&str, u64)] = &[
    ("add_to_cart (47)", 11),
    ("add_to_cart (48)", 12),
    ("add_address", 7),
    ("choose_tip", 7),
    ("choose_time (as soon as possible)", 7),
    // TRACK SEAM (reviews): each list's ratings one read more, the store's
    // page two (its ratings and its reviews), the order page one.
    ("/", 6),
    ("/stores/47", 18),
    ("/cart", 16),
    ("/search?q=coffee", 9),
    ("/stores/47/address", 2),
    // Track `scheduled-orders`: the page of times, its cart's stores' hours
    // (6), the time chosen (1), and its layout's cart (1); and the windows
    // wait for all stores' estimates in one set read (ADR-0348): the
    // address, store rows and estimate, 3 total regardless of key count.
    // cart's own (16) asks none: a cart for as soon as possible has no lead.
    ("/cart/when", 11),
    // Track `scheduled-orders`: 33 and 2 more, the time chosen
    // (`checkout#chosen`) and, where the order is settled, its time
    // (`user-orders#settle` 8, was 7).
    ("place_order, taking no payments: its operations", 35),
    ("place_order, taking no payments: its commit", 8),
    // Track `scheduled-orders`: 21 and 11 more without the render merge,
    // the checkout's stores' hours (3), and the open page of times read
    // again: its time chosen (1), its stores' hours (6) and its layout's
    // cart (1); with it, 16 and 3 more (the merge's paragraph above).
    (
        "place_order, taking no payments: its queries read again",
        19,
    ),
    (
        "place_order, taking no payments: user-carts#current called again",
        1,
    ),
    ("/order", 6),
    // TRACK SEAM (reviews): `rate_order`'s transaction (`MayRate`'s read,
    // `#rate`'s read and its row, and its commit); the reader's open pages
    // read again after it, including scheduling's extra cart-hours read
    // (3) and the times page with an empty cart (8): 42 + 11 = 53 without
    // the render merge, 37 with it (the merge's paragraph above). The rate
    // page reads its store and the layout's cart separately.
    ("rate_order: its transaction", 9),
    ("rate_order: its reader's open pages read again", 37),
    ("rate_order: user-carts#current called again", 1),
    ("rate_order: user-orders#current called again", 1),
    ("/order/rate/47", 2),
];

/// **The transactions after exactly `completed` payment outcomes finish**.
/// `Payments::heard` records each only after its real settlement command
/// returns. Equal counters for an interval do not prove that completion.
fn after_payments(s: &Server, completed: usize) -> Vec<u64> {
    let payments = s.payments.get().expect("payments");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let done = payments.settled.lock().expect("settled").len();
        assert!(
            done <= completed,
            "expected exactly {completed} completed outcomes, got {done}"
        );
        if done == completed {
            return s.data.statements_by_transaction();
        }
        assert!(
            std::time::Instant::now() < deadline,
            "expected {completed} completed payment outcomes, got {done}; transactions: {:?}",
            s.data.statements_by_transaction()
        );
        POLLING.with(|polling| {
            if let Some(polled) = &*polling.borrow() {
                polled();
            }
        });
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

std::thread_local! {
    /// What this thread's test does each time `after_payments` finds fewer
    /// payment outcomes than it waits for, before it waits again.
    static POLLING: std::cell::RefCell<Option<Box<dyn Fn()>>> =
        const { std::cell::RefCell::new(None) };
}

/// **A test's: `polled` is called on this thread each time `after_payments`
/// finds fewer outcomes than it waits for**, until the guard is dropped. The
/// waiting is counted in the helper's own polls, which a slow machine
/// stretches along with the work it waits for: no stretch of time is
/// guessed.
fn tell_on_poll(polled: impl Fn() + 'static) -> PollingTold {
    POLLING.with(|polling| *polling.borrow_mut() = Some(Box::new(polled)));
    PollingTold
}

struct PollingTold;

impl Drop for PollingTold {
    fn drop(&mut self) {
        POLLING.with(|polling| *polling.borrow_mut() = None);
    }
}

/// **Scenario (b), a deployment that takes payments** (ADR-0332, the
/// production path): `place_order` leaves the order awaiting its payment,
/// no settle among its operations; the provider's word, a card paid on its
/// page, runs `settle_payment` in a transaction of its own. Each
/// transaction's statements, with fifty stores more where `more` says.
fn awaiting(more: bool) -> Vec<(String, u64)> {
    let mut out = Vec::new();
    super::store_payments::paying(|s, _| {
        if more {
            s.data
                .execute_for_test(FIFTY_MORE)
                .expect("fifty stores more");
        }
        s.command(
            "store.page.add_to_cart",
            "a",
            &add_shown("espresso", 1),
            false,
        )
        .expect("added");
        s.command("store.page.add_to_cart", "a", &add_shown("drip", 1), false)
            .expect("added");
        let total = s.shown_total("a");
        let before = s.data.statements_by_transaction().len();
        s.command_answered(
            "store.page.place_order",
            "a",
            std::slice::from_ref(&total),
            // A payment is begun by an interaction (ADR-0332).
            Some("place-1"),
        )
        .expect("answered");
        let Some((status, Some(page))) = super::store_payments::status_of(s, "a") else {
            panic!("an order awaiting its payment")
        };
        assert_eq!(status, "awaiting-payment");
        let placed = s.data.statements_by_transaction()[before..].to_vec();
        let paid = super::store_payments::pay(&page, "4242424242424242");
        assert!(paid.contains("Paid."), "{paid}");
        let settled = s.data.statements_by_transaction()[before + placed.len()..].to_vec();
        let ([own], [settle]) = (&placed[..], &settled[..]) else {
            panic!("place_order's transaction, then settle_payment's: {placed:?} {settled:?}")
        };
        out.push((
            "place_order, taking payments: its transaction".to_string(),
            *own,
        ));
        out.push((
            "settle_payment, on the provider's word: its transaction".to_string(),
            *settle,
        ));
        // An order awaiting, taken back by its reader: `cancel_order`'s
        // transaction (ADR-0332's step eight).
        s.command(
            "store.page.add_to_cart",
            "a",
            &add_shown("espresso", 1),
            false,
        )
        .expect("added");
        let total = s.shown_total("a");
        s.command_answered(
            "store.page.place_order",
            "a",
            std::slice::from_ref(&total),
            Some("place-2"),
        )
        .expect("answered");
        let before = after_payments(s, 1).len();
        let canceled = s
            .command_answered("store.page.cancel_order", "a", &[], Some("cancel-1"))
            .expect("answered");
        assert!(canceled.committed, "{:?}", canceled.result);
        let ran = after_payments(s, 2)[before..].to_vec();
        // Its own, and `settle_payment` on the provider's word of the
        // cancel, which the provider tells the host as it answers the
        // cancel (ADR-0332's step eight).
        let [cancel, told] = ran[..] else {
            panic!("cancel_order's transaction, then settle_payment's: {ran:?}")
        };
        out.push(("cancel_order: its transaction".to_string(), cancel));
        out.push((
            "settle_payment, on the provider's word of the cancel: its transaction".to_string(),
            told,
        ));
        // An order no one pays, swept thirty minutes on (ADR-0334): the clock
        // moved, then the run that is due, `expire_order`'s transaction.
        let total = s.shown_total("a");
        s.command_answered(
            "store.page.place_order",
            "a",
            std::slice::from_ref(&total),
            Some("place-3"),
        )
        .expect("answered");
        let before = after_payments(s, 2).len();
        s.query_clock.0.advance(30 * 60 * 1000);
        assert_eq!(s.run_due().0, 1, "the sweep, run");
        let ran = after_payments(s, 3)[before..].to_vec();
        // Its own, and `settle_payment` on the provider's word of the cancel
        // the sweep asks of it.
        let [expire, told] = ran[..] else {
            panic!("expire_order's transaction, then settle_payment's: {ran:?}")
        };
        out.push((
            "expire_order, run thirty minutes on: its transaction".to_string(),
            expire,
        ));
        out.push((
            "settle_payment, on the provider's word of the sweep's cancel: its transaction"
                .to_string(),
            told,
        ));
        // Track `scheduled-orders`: the swept order's lines, back in the
        // cart, ordered for a time, a quarter hour past the first offered;
        // paid on the provider's page; and sent to its stores when its
        // preparation is due, the window less the estimate's most (35
        // minutes, no address chosen) and five more.
        let window = ((s.now() + 3_600_000) / 900_000 + 2) * 900_000;
        let before = after_payments(s, 3).len();
        let chosen = s
            .command_answered(
                "store.page.choose_time",
                "a",
                &[Val::Option(Some(Box::new(Val::S64(window))))],
                Some("time-1"),
            )
            .expect("answered");
        assert!(chosen.committed, "{:?}", chosen.result);
        let total = s.shown_total("a");
        s.command_answered(
            "store.page.place_order",
            "a",
            std::slice::from_ref(&total),
            Some("place-4"),
        )
        .expect("answered");
        let ran = after_payments(s, 3)[before..].to_vec();
        let [choose, place] = ran[..] else {
            panic!("choose_time's transaction, then place_order's: {ran:?}")
        };
        out.push((
            "choose_time, for a time: its transaction".to_string(),
            choose,
        ));
        out.push((
            "place_order, for a time, taking payments: its transaction".to_string(),
            place,
        ));
        let Some((status, Some(page))) = super::store_payments::status_of(s, "a") else {
            panic!("an order awaiting its payment")
        };
        assert_eq!(status, "awaiting-payment");
        let before = after_payments(s, 3).len();
        let paid = super::store_payments::pay(&page, "4242424242424242");
        assert!(paid.contains("Paid."), "{paid}");
        let ran = after_payments(s, 4)[before..].to_vec();
        let [settle] = ran[..] else {
            panic!("settle_payment's transaction: {ran:?}")
        };
        out.push((
            "settle_payment, on the provider's word, for a time: its transaction".to_string(),
            settle,
        ));
        let sends = window - 40 * 60_000;
        s.query_clock
            .0
            .advance(u64::try_from(sends - s.now()).expect("ahead") + 1000);
        let before = after_payments(s, 4).len();
        assert_eq!(s.run_due().0, 1, "the send, run");
        let ran = after_payments(s, 4)[before..].to_vec();
        let [send] = ran[..] else {
            panic!("send_to_store's transaction: {ran:?}")
        };
        out.push((
            "send_to_store, run when its preparation is due: its transaction".to_string(),
            send,
        ));
    });
    out
}

/// **Where the deployment takes payments, `place_order` and the
/// `settle_payment` the provider's word runs are each a bounded number of
/// statements**, equal at the seed and with fifty stores more, and pinned.
#[test]
fn placing_and_settling_where_payments_are_taken_is_bounded() {
    if std::env::var("PW_STORE_TEST_LAYER").as_deref() != Ok("postgres") {
        eprintln!("skipped: the store is not served on PostgreSQL");
        return;
    }
    let seed = awaiting(false);
    let more = awaiting(true);
    eprintln!("statements where payments are taken:\n  {seed:?}\n  {more:?}");
    assert_eq!(seed, more, "equal at both sizes");
    let pinned: Vec<(String, u64)> = AWAITING.iter().map(|(k, v)| (k.to_string(), *v)).collect();
    assert_eq!(seed, pinned, "as pinned");
}

/// Scenario (b)'s transactions, each with its commit.
// Since ADR-0334 a command's commit writes its scheduled runs too: a delete
// for each, and an insert for each run due. `place_order` schedules its
// sweep (2), `settle_payment` and `cancel_order` cancel it (1).
// Track `scheduled-orders`: `place_order` reads the time chosen (1 more),
// and `settle_payment` the order's time where it settles one paid (1 more).
// Track `second-host` (ADR-0369): and each commit's lock and notify on the
// outbox (1), as [`PAGES`]' commands.
// ADR-0388: each Clock transaction also consumes its exact queue generation:
// a locked current-row read and a conditional delete (2). Browser and Payments
// transactions keep their previous budgets.
const AWAITING: &[(&str, u64)] = &[
    ("place_order, taking payments: its transaction", 39),
    (
        "settle_payment, on the provider's word: its transaction",
        19,
    ),
    ("cancel_order: its transaction", 26),
    (
        "settle_payment, on the provider's word of the cancel: its transaction",
        15,
    ),
    ("expire_order, run thirty minutes on: its transaction", 29),
    (
        "settle_payment, on the provider's word of the sweep's cancel: its transaction",
        15,
    ),
    // Track `scheduled-orders`: an order for a time. Its time chosen, held
    // to the windows its cart's stores take an order at when they are told:
    // their hours and the cart's one store's estimate (2: no address is
    // chosen here, so none to read; 3 where one is); placed, the estimate
    // read and the time written with it; paid, its send scheduled in the
    // commit (a delete and an insert); and sent when its preparation is due.
    ("choose_time, for a time: its transaction", 14),
    (
        "place_order, for a time, taking payments: its transaction",
        41,
    ),
    (
        "settle_payment, on the provider's word, for a time: its transaction",
        22,
    ),
    (
        "send_to_store, run when its preparation is due: its transaction",
        12,
    ),
];

/// How many polls of `after_payments` a held event must outlast: sixty,
/// ten milliseconds apart, twice the 300 ms of equal counters that once
/// stood for completion.
const POLLS: usize = 60;

/// What the counting thread tells the test, in the order it happens.
enum Counting {
    /// `after_payments` found the payment outcome missing, and waits again.
    Polled,
    /// It returned this count.
    Counted(Vec<u64>),
}

/// A provider may delay its webhook after answering the cancel. Unchanged
/// counters in that interval do not prove the provider's transaction finished.
#[test]
fn a_pending_real_payment_webhook_is_not_a_complete_statement_count() {
    if std::env::var("PW_STORE_TEST_LAYER").as_deref() != Ok("postgres") {
        eprintln!("skipped: the store is not served on PostgreSQL");
        return;
    }
    super::store_payments::paying(|s, provider| {
        s.command(
            "store.page.add_to_cart",
            "a",
            &add_shown("espresso", 1),
            false,
        )
        .expect("added");
        let total = s.shown_total("a");
        let placed = s
            .command_answered("store.page.place_order", "a", &[total], Some("held-place"))
            .expect("placed");
        assert!(placed.committed, "{:?}", placed.result);
        provider.hold();
        let before = s.data.statements_by_transaction().len();
        let canceled = s
            .command_answered("store.page.cancel_order", "a", &[], Some("held-cancel"))
            .expect("answered");
        assert!(canceled.committed, "{:?}", canceled.result);

        std::thread::scope(|scope| {
            let (told, heard) = std::sync::mpsc::channel();
            scope.spawn(move || {
                let polls = told.clone();
                let _polling = tell_on_poll(move || {
                    let _ = polls.send(Counting::Polled);
                });
                let counted = after_payments(s, 1)[before..].to_vec();
                let _ = told.send(Counting::Counted(counted));
            });
            // The held event keeps the count waiting: `POLLS` of its polls
            // pass, or it returns. The wait is counted in the count's own
            // polls, so a slow machine stretches it with the count; and a
            // count that stops polling, to return on counters that are
            // equal, is heard returning where it should have polled.
            let (mut polled, mut early) = (0, None);
            while polled < POLLS && early.is_none() {
                match heard
                    .recv_timeout(MUST_HAPPEN)
                    .expect("the counting thread neither polled nor returned")
                {
                    Counting::Polled => polled += 1,
                    Counting::Counted(counted) => early = Some(counted),
                }
            }
            // Always release and drain before asserting: the real signed HTTP
            // event and compiled settlement run even if counting returned early.
            provider.release_reversed();
            let returned_early = early.is_some();
            let finished = early.unwrap_or_else(|| {
                loop {
                    let counting = heard
                        .recv_timeout(MUST_HAPPEN)
                        .expect("the count after actual settlement");
                    if let Counting::Counted(counted) = counting {
                        break counted;
                    }
                }
            });
            eprintln!("delayed webhook: returned_early={returned_early}; completed={finished:?}");
            assert!(
                !returned_early,
                "counting returned before the provider released its event"
            );
            // Each with its commit's lock and notify on the outbox (track
            // `second-host`, ADR-0369), as `AWAITING`'s.
            assert_eq!(finished, [26, 15], "the exact completed transactions");
        });
    });
}

/// The longest a test waits for something that must happen. Only a hang
/// reaches it: what happens is read the moment it does.
const MUST_HAPPEN: std::time::Duration = std::time::Duration::from_secs(10);

/// **What a write met at the layer's write guard**, in the order it met it.
enum Met<T> {
    /// It found the guard held, and waits for it: it has written nothing.
    Waits,
    /// It returned.
    Returned(T),
}

/// **A write on a thread of its own, and whether the layer's write guard held
/// it back** (ADR-0352). The guard says so itself (`Writes::tell_on_wait`),
/// before the write waits for it; a write that skips the guard says nothing
/// and returns. So the first thing heard is the verdict, at any machine's
/// speed: a stretch of time could only say that a write had not returned
/// *yet*, and a slow write outlasts any stretch.
struct Watched<T> {
    heard: std::sync::mpsc::Receiver<Met<T>>,
    first: Option<Met<T>>,
}

impl<T: Send + 'static> Watched<T> {
    fn spawn<'scope, 'env>(
        scope: &'scope std::thread::Scope<'scope, 'env>,
        write: impl FnOnce() -> T + Send + 'scope,
    ) -> Watched<T> {
        let (told, heard) = std::sync::mpsc::channel();
        scope.spawn(move || {
            let waiting = told.clone();
            let _tells = crate::data::Writes::tell_on_wait(move || {
                let _ = waiting.send(Met::Waits);
            });
            let _ = told.send(Met::Returned(write()));
        });
        Watched { heard, first: None }
    }

    fn next(&self, what: &str) -> Met<T> {
        self.heard.recv_timeout(MUST_HAPPEN).expect(what)
    }

    /// **Whether the write found the guard held**, known as soon as it has
    /// either waited for the guard or returned without.
    fn waited(&mut self) -> bool {
        let first = self.next("the write neither waited for the guard nor returned");
        let waited = matches!(first, Met::Waits);
        self.first = Some(first);
        waited
    }

    /// **What the write returned**, once the guard it waits for is let go.
    fn returned(mut self) -> T {
        let mut met = match self.first.take() {
            Some(met) => met,
            None => self.next("the write neither waited for the guard nor returned"),
        };
        if matches!(met, Met::Waits) {
            met = self.next("the write after the guard was let go");
        }
        match met {
            Met::Returned(returned) => returned,
            Met::Waits => panic!("the write waited for the guard twice"),
        }
    }
}

/// A consumed sweep's late conditional retry shares the payment transaction's
/// write guard. The real Clock driver consumes its queue row atomically; no
/// separate cleanup is fabricated after that commit (ADR-0388).
#[test]
fn sweep_cleanup_and_payment_commit_share_the_same_write_guard() {
    if std::env::var("PW_STORE_TEST_LAYER").as_deref() != Ok("postgres") {
        return;
    }
    super::store_payments::paying(|s, provider| {
        s.command(
            "store.page.add_to_cart",
            "a",
            &add_shown("espresso", 1),
            false,
        )
        .expect("added");
        let total = s.shown_total("a");
        assert!(
            s.command_answered(
                "store.page.place_order",
                "a",
                &[total],
                Some("overlap-place")
            )
            .expect("placed")
            .committed
        );
        provider.hold();
        s.query_clock.0.advance(30 * 60 * 1000);
        let runs = crate::schedules::select(s.runs(), s.now(), 16)
            .expect("the actual adapter-bound due sweep");
        let [run] = runs.as_slice() else {
            panic!("one due sweep");
        };
        let before = s.data.statements_by_transaction().len();
        assert!(!s.run_scheduled(run), "the real Clock driver was not blocked");
        assert_eq!(
            s.counted_runs.ran.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "the actual Clock action completed"
        );
        assert!(
            s.runs().all_runs().expect("the consumed sweep").is_empty(),
            "the Clock transaction already consumed its queue generation"
        );
        assert_eq!(
            super::store_payments::status_of(s, "a"),
            Some(("canceled".to_string(), None)),
            "the real sweep changed its order before the late retry"
        );

        let (at_commit, arrived) = std::sync::mpsc::channel();
        let (resume, paused) = std::sync::mpsc::channel();
        *s.before_commit.lock().expect("before commit") = Some(Box::new(move |_| {
            at_commit
                .send(())
                .expect("the real settlement reached commit");
            paused
                .recv_timeout(std::time::Duration::from_secs(10))
                .expect("release settlement");
        }));
        std::thread::scope(|scope| {
            let provider_request = scope.spawn(|| provider.release_reversed());
            arrived
                .recv_timeout(std::time::Duration::from_secs(10))
                .expect("the real signed webhook reached the commit barrier");
            // An older dispatcher may still attempt a retry of this consumed
            // generation. Its production conditional update must meet the same
            // guard, then report no current row without resurrecting the sweep.
            let mut retry = Watched::spawn(scope, || {
                let failures = run.attempts.saturating_add(1);
                let due = s.now().saturating_add(crate::schedules::backoff_ms(failures));
                s.runs().retry_current(run, failures, due, None)
            });
            let retry_waited = retry.waited();
            let while_waiting = s.runs().all_runs().expect("runs during the late retry");
            // Release and drain every real operation before failure assertions.
            resume.send(()).expect("release settlement");
            let retried = retry.returned().expect("the conditional late retry");
            provider_request
                .join()
                .expect("the actual HTTP webhook completed");
            let finished = s.data.statements_by_transaction()[before..].to_vec();
            let completed = s
                .payments
                .get()
                .expect("payments")
                .settled
                .lock()
                .expect("settled")
                .len();
            eprintln!(
                "forced overlap: retry_waited={retry_waited}; retried={retried}; completed_markers={completed}; transactions={finished:?}"
            );
            assert!(
                retry_waited,
                "conditional retry crossed the active serializable payment transaction"
            );
            assert!(!retried, "a consumed queue generation cannot be retried");
            assert!(
                while_waiting.is_empty(),
                "the consumed sweep remains absent during the payment transaction"
            );
            assert_eq!(
                completed, 1,
                "the payment's actual command completed successfully"
            );
            // Each with its commit's lock and notify on the outbox (track
            // `second-host`, ADR-0369), as `AWAITING`'s.
            assert_eq!(
                finished,
                [29, 15],
                "the exact completed clock and payment transactions"
            );
            assert_eq!(
                super::store_payments::status_of(s, "a"),
                Some(("canceled".to_string(), None))
            );
            assert!(s.runs().all_runs().expect("runs").is_empty());
        });
    });
}

/// Each adapter's two scheduler writes must respect the same real command
/// transaction lifetime. A moved run keeps its new identity and due time.
///
/// **The write is held at the guard, not timed.** While a command's staging
/// holds the guard the write starts, and the test hears what it met first:
/// the guard held, which the guard itself reports, or nothing and a return.
/// No stretch of time decides it, so neither a slow machine nor a slow write
/// lets a write that skips the guard pass.
fn scheduled_transition_waits<T>(layer: &T, retry: bool)
where
    T: crate::data::DataLayer + crate::schedules::Runs,
{
    let scheduled = |due| crate::schedules::Scheduled {
        key: crate::schedules::key("test.expire", "[]"),
        command: "test.expire".to_string(),
        args: "[]".to_string(),
        user: "private-test-reader".to_string(),
        due_ms: Some(due),
    };
    let mut seed = layer.begin("private-test-session");
    seed.commit(&[], &[], None, &[scheduled(10)], None)
        .expect("the original run committed");
    drop(seed);
    let original = layer.all_runs().expect("the original run");
    let [original] = original.as_slice() else {
        panic!("one original run");
    };
    let old_id = original.id;
    std::thread::scope(|scope| {
        // This actual serializable transaction owns the adapter's guard.
        let mut staged = layer.begin("private-test-session");
        let mut write = Watched::spawn(scope, || {
            if retry {
                layer.retry(old_id, 1, 30, Some("private refusal control"))
            } else {
                layer.ran(old_id)
            }
        });
        let waited = write.waited();
        // Waiting at the guard, the write has changed nothing yet.
        let while_waiting = layer.all_runs().expect("the runs while the write waits");
        let committed = staged.commit(&[], &[], None, &[scheduled(20)], None);
        // Release every guard and drain before asserting, also on a refusal.
        drop(staged);
        let finished = write.returned();
        committed.expect("the moved run committed");
        finished.expect("the real scheduler write completed");
        assert!(
            waited,
            "scheduler transition retry={retry} crossed an active command transaction"
        );
        assert_eq!(
            while_waiting.as_slice(),
            std::slice::from_ref(original),
            "a write waiting at the guard has changed nothing"
        );
    });
    let moved = layer.all_runs().expect("the moved run");
    let [moved] = moved.as_slice() else {
        panic!("one moved run");
    };
    assert_ne!(moved.id, old_id, "a reschedule owns a new identity");
    assert_eq!(
        moved.due_ms, 20,
        "old cleanup/retry cannot change the new scheduling"
    );
    assert_eq!(moved.attempts, 0, "the moved run is a fresh attempt");
    assert_eq!(
        moved.failed, None,
        "the moved run is not the previous refusal"
    );
}

fn store_scheduled_transition(retry: bool) {
    let Some(url) = database() else { return };
    let layer = crate::store_pg::StorePg::for_test(&url).expect("private store schema");
    scheduled_transition_waits(&layer, retry);
}

fn feed_scheduled_transition(retry: bool) {
    let Some(url) = database() else { return };
    struct Schema {
        url: String,
        name: String,
    }
    impl Drop for Schema {
        fn drop(&mut self) {
            if let Ok(mut client) = postgres::Client::connect(&self.url, postgres::NoTls) {
                let _ = client.batch_execute(&format!(
                    "DROP SCHEMA IF EXISTS {} CASCADE",
                    crate::feed_pg::ident(&self.name)
                ));
            }
        }
    }
    let schema = Schema {
        url: url.clone(),
        name: format!(
            "pw_bounded_scheduler_{}_{}",
            std::process::id(),
            if retry { "retry" } else { "ran" }
        ),
    };
    // layer drops before schema, so no live transaction is removed.
    let layer =
        crate::feed_pg::FeedPg::open(&url, Some(&schema.name)).expect("private feed schema");
    scheduled_transition_waits(&layer, retry);
}

#[test]
fn scheduled_state_writes_store_ran_waits_for_a_command() {
    store_scheduled_transition(false);
}

#[test]
fn scheduled_state_writes_store_retry_waits_for_a_command() {
    store_scheduled_transition(true);
}

#[test]
fn scheduled_state_writes_feed_ran_waits_for_a_command() {
    feed_scheduled_transition(false);
}

#[test]
fn scheduled_state_writes_feed_retry_waits_for_a_command() {
    feed_scheduled_transition(true);
}
