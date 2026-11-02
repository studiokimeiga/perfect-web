//! **One render's identical calls made once, in one snapshot** (NEXT 4c's
//! first step, track `render-merge`; W9's design note, its second case):
//! the host's render merge served, on the store and on the feed.
//!
//! - **Each identical call once**: a session's documents read again after a
//!   commit are one render, so the cart every page's layout shows is read
//!   once, and the feed's timeline once for two of its documents; the
//!   control, each store page's reach by its own store, read for each.
//! - **One snapshot**: another writer's commit landing between two
//!   bindings' reads is seen by neither, on the store's page in memory and
//!   on PostgreSQL (the layer `PW_STORE_TEST_LAYER` says), and on the feed's
//!   page on PostgreSQL; the control, the same two bindings read in two
//!   renders, which straddle it. **And no other render's**: a render reads a
//!   key another render has in flight in its own snapshot, never joining it
//!   (the integrator's ruling 3).
//! - **Each layer's snapshot** (`DataLayer::snapshot`, `reads_in`): the
//!   store's and the feed's, in memory and on PostgreSQL; on PostgreSQL, a
//!   render's read that fails ends its transaction.
//! - **Bounded seats, every one given back** (the integrator's ruling 2): on
//!   PostgreSQL a render reads in one of the layer's seats, as many as its
//!   deployment says (`feed_pg::RenderSeats`, `feed_pg::RENDER_CONNECTIONS`
//!   where it says nothing: the integrator's answer of 2026-10-17), its own
//!   pool, never the commands'. A deployment's seats and seat wait are read
//!   or refused, and bound its renders as it says. A render past the bound
//!   waits for a seat, and one that waits past its
//!   limit fails saying so; twenty-four renders at once are never more than
//!   the bound seated, nor connections open; and with every seat held by a
//!   change's render and the subscriber table held by a reader waiting for a
//!   seat, the renders end before their telling waits for the table, on the
//!   store (a drain) and on the feed (a session's documents told).

use super::*;
use crate::data::{DataLayer, Ops};
use crate::feed_pg::RENDER_CONNECTIONS;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

fn text(v: &str) -> Val {
    Val::String(v.to_string())
}

/// The store, served on the layer the server's tests serve it on.
fn store() -> Served {
    served_from_patches_in("examples", |app| app.to_string(), &[])
}

/// Each data-layer operation's calls so far (`/metrics`).
fn calls(s: &Server) -> BTreeMap<String, u64> {
    s.calls.lock().expect("calls").clone()
}

/// The calls made since `before`, by operation.
fn since(s: &Server, before: &BTreeMap<String, u64>) -> BTreeMap<String, u64> {
    calls(s)
        .into_iter()
        .filter_map(|(op, n)| {
            let made = n - before.get(&op).copied().unwrap_or_default();
            (made > 0).then_some((op, made))
        })
        .collect()
}

/// **What a cart's lines cost together**, as the cart shows them.
fn cart_subtotal(cart: &Val) -> i64 {
    let Some(Val::List(lines)) = val_at(cart, &["lines"]) else {
        panic!("no cart: {cart:?}");
    };
    lines
        .iter()
        .map(
            |l| match (val_at(l, &["quantity"]), val_at(l, &["unit-price"])) {
                (Some(Val::S64(quantity)), Some(Val::S64(price))) => quantity * price,
                other => panic!("no line: {other:?}"),
            },
        )
        .sum()
}

/// **The subtotal the checkout charges**, as the cart page shows it.
fn checkout_subtotal(checkout: &Val) -> i64 {
    match val_at(checkout, &["charges", "subtotal"]) {
        Some(Val::S64(n)) => *n,
        other => panic!("no subtotal: {other:?} in {checkout:?}"),
    }
}

/// **A session's documents read again after a commit are one render, and
/// each identical call of it is made once**: the cart every page's layout
/// shows, which the drain reads and each of three documents reads, is read
/// once. The control: each store page's reach, one operation called with
/// two stores, is read for each.
#[test]
fn a_commits_documents_are_read_in_one_render_each_identical_call_once() {
    let s = store();
    s.command(
        "store.page.add_to_cart",
        "a",
        &add_shown("espresso", 1),
        false,
    )
    .expect("added");
    for path in ["/stores/47", "/stores/48", "/cart"] {
        fetched_as(&s, path, Some("a"));
    }
    let before = calls(&s);
    s.command("store.page.add_to_cart", "a", &add_shown("drip", 1), false)
        .expect("added");
    let made = since(&s, &before);
    assert_eq!(
        made.get("store:data/user-carts#current"),
        Some(&1),
        "the cart, once for the drain and three documents: {made:?}"
    );
    assert_eq!(
        made.get("store:data/coverage#for-store"),
        Some(&2),
        "each store's reach, by its own arguments: {made:?}"
    );
}

/// **A render merges the layer's calls, never the platform's**: two queries
/// of one render that compare the clock each note what the clock holds them
/// until (ADR-0304), which a call answered again would not.
#[test]
fn a_renders_queries_each_note_what_the_clock_holds_them_until() {
    let s = store();
    let _render = crate::render_merge::begin(&*s.data, s.queries.epoch());
    let status = || {
        clock::noting(|| {
            s.answer("store.hours.StoreStatus", "a", &[text("47")])
                .expect("answered")
        })
        .1
    };
    let (one, two) = (status(), status());
    assert!(one.is_some(), "the status compares the clock");
    assert_eq!(one, two, "each query notes it");
}

/// The reader's cart page, served, and its document: a line of store 47's,
/// one espresso.
fn cart_page(s: &Server) -> u64 {
    cart_page_of(s, "a")
}

/// `session`'s cart page, served, and its document: one espresso.
fn cart_page_of(s: &Server, session: &str) -> u64 {
    s.command(
        "store.page.add_to_cart",
        session,
        &add_shown("espresso", 1),
        false,
    )
    .expect("added");
    let (_, doc, _, _) = s
        .serve_document_settled(session, "store.page.CartPage~ui", &Params::new(), &[])
        .expect("served");
    doc
}

/// **Two bindings of one render read one snapshot**: another writer's
/// commit landing after the render read the cart, five espressos where
/// there was one, is not what its checkout prices either; the next render
/// reads it in both.
#[test]
fn two_bindings_of_one_render_read_one_snapshot() {
    let s = store();
    let doc = cart_page(&s);
    let owner = s.owner("a");
    *s.between_bindings.lock().expect("between") = Some(Box::new(move |s: &Server| {
        s.store
            .line_around(&owner, 0, 5)
            .expect("another writer's commit");
    }));
    let read = s.bindings("a", doc).expect("read");
    assert_eq!(lines_of(&read["cart"]), [("espresso".to_string(), 1)]);
    assert_eq!(
        checkout_subtotal(&read["checkout"]),
        cart_subtotal(&read["cart"]),
        "the checkout prices the cart the page shows"
    );
    let read = s.bindings("a", doc).expect("read");
    assert_eq!(lines_of(&read["cart"]), [("espresso".to_string(), 5)]);
    assert_eq!(
        checkout_subtotal(&read["checkout"]),
        cart_subtotal(&read["cart"])
    );
}

/// **The control: two renders straddle a commit between them.** The same
/// two bindings, each read in a render of its own, the commit between: the
/// checkout prices five espressos, and the cart showed one.
#[test]
fn two_renders_straddle_a_commit_between_them() {
    let s = store();
    let doc = cart_page(&s);
    let one = |binding: &str| {
        s.bindings_where("a", Some(doc), |b| b == binding, &Keys::Shown)
            .expect("read")
            .remove(binding)
            .expect("the binding")
    };
    let cart = one("cart");
    s.store
        .line_around(&s.owner("a"), 0, 5)
        .expect("another writer's commit");
    let checkout = one("checkout");
    assert_eq!(lines_of(&cart), [("espresso".to_string(), 1)]);
    assert_eq!(checkout_subtotal(&checkout), 5 * cart_subtotal(&cart));
}

/// **The feed's documents read again after a commit are one render**: a
/// session's two timelines, read again after its post, read its timeline
/// once. Each document's alone, it was read twice.
#[test]
fn a_feed_sessions_documents_are_read_in_one_render_each_identical_call_once() {
    let s = served_feed();
    for _ in 0..2 {
        s.serve_document_settled("a", "feed.app.Home~ui", &Params::new(), &[])
            .expect("served");
    }
    let before = calls(&s);
    let answered = s
        .command_answered("feed.app.post", "a", &[text("One render.")], Some("i-1"))
        .expect("runs");
    assert!(answered.committed, "{:?}", answered.result);
    let made = since(&s, &before);
    assert_eq!(
        made.get("feed:data/posts#timeline"),
        Some(&1),
        "one timeline for two documents: {made:?}"
    );
}

fn feed_database() -> Option<String> {
    match std::env::var("PW_FEED_DATABASE_URL") {
        Ok(url) if !url.is_empty() => Some(url),
        _ => {
            eprintln!("skipped: PW_FEED_DATABASE_URL is not set");
            None
        }
    }
}

fn store_database() -> Option<String> {
    match std::env::var("PW_STORE_DATABASE_URL") {
        Ok(url) if !url.is_empty() => Some(url),
        _ => {
            eprintln!("skipped: PW_STORE_DATABASE_URL is not set");
            None
        }
    }
}

/// A notification for the guest `a` reads as, as another writer commits it.
/// Another writer's commit: a notification for session `a`'s reader, its
/// guest, whose id is its session's digest (track `data-path`).
fn notified() -> String {
    format!(
        "INSERT INTO notifications (recipient, actor, act) VALUES ('{}', 'u-grace', 'followed')",
        crate::identity::guest_user("a")
    )
}

/// How many notifications a list binding shows.
fn listed(notes: &Val) -> usize {
    match notes {
        Val::List(items) => items.len(),
        other => panic!("no list: {other:?}"),
    }
}

/// **On PostgreSQL, the feed's render reads one snapshot**: a notification
/// committed after the render read the reader's list is not counted unread
/// either; the next render reads it in both.
#[test]
fn on_postgres_a_feed_renders_bindings_read_one_snapshot() {
    let Some(url) = feed_database() else { return };
    let s = super::feed_pg::served_on(&url, "render_merge_one");
    let (_, doc, _, _) = s
        .serve_document_settled("a", "feed.app.NotificationsPage~ui", &Params::new(), &[])
        .expect("served");
    *s.between_bindings.lock().expect("between") = Some(Box::new(|s: &Server| {
        s.data
            .execute_for_test(&notified())
            .expect("another writer's commit");
    }));
    let read = s.bindings("a", doc).expect("read");
    assert_eq!(
        (listed(&read["notes"]), &read["unread"]),
        (0, &Val::S64(0)),
        "neither sees it"
    );
    let read = s.bindings("a", doc).expect("read");
    assert_eq!((listed(&read["notes"]), &read["unread"]), (1, &Val::S64(1)));
}

/// **The control, on PostgreSQL: the feed's two renders straddle a commit
/// between them**, the list read before it and the count after.
#[test]
fn on_postgres_a_feeds_two_renders_straddle_a_commit_between_them() {
    let Some(url) = feed_database() else { return };
    let s = super::feed_pg::served_on(&url, "render_merge_two");
    let (_, doc, _, _) = s
        .serve_document_settled("a", "feed.app.NotificationsPage~ui", &Params::new(), &[])
        .expect("served");
    let one = |binding: &str| {
        s.bindings_where("a", Some(doc), |b| b == binding, &Keys::Shown)
            .expect("read")
            .remove(binding)
            .expect("the binding")
    };
    let notes = one("notes");
    s.data
        .execute_for_test(&notified())
        .expect("another writer's commit");
    let unread = one("unread");
    assert_eq!((listed(&notes), unread), (0, Val::S64(1)));
}

/// The first line's quantity of `ada`'s cart, as `ops` read it.
fn ada_quantity(ops: Ops) -> i64 {
    match &ops["store:data/user-carts#current"](&[text("ada")]).expect("read")[..] {
        [Val::Result(Ok(Some(cart)))] => lines_of(cart)[0].1,
        other => panic!("no cart: {other:?}"),
    }
}

/// `ada`'s espresso added, in a command of the memory store's, committed.
fn ada_adds(layer: &crate::store::StoreData) {
    let mut staging = DataLayer::begin(layer, "s");
    staging.ops()["store:data/user-carts#add"](&[text("ada"), text("espresso"), Val::S64(1)])
        .expect("added");
    staging.publish();
}

/// **The store's snapshot in memory** is the store as the render first read
/// it: a commit after is read by a read of its own, not by the render's.
#[test]
fn the_memory_stores_snapshot_is_the_store_as_a_render_first_read_it() {
    let layer = crate::store::StoreData::new();
    ada_adds(&layer);
    let snapshot = layer.snapshot().expect("a render's snapshot");
    assert_eq!(ada_quantity(layer.reads_in(&snapshot, "s", None)), 1);
    ada_adds(&layer);
    assert_eq!(
        ada_quantity(layer.reads_in(&snapshot, "s", None)),
        1,
        "the render's"
    );
    assert_eq!(ada_quantity(layer.reads("s", None)), 2, "a read's own");
}

/// The store on PostgreSQL, `ada` with one espresso in her cart.
fn store_on_postgres(url: &str) -> (crate::store_pg::StorePg, postgres::Client) {
    let layer = crate::store_pg::StorePg::for_test(url).expect("the layer");
    let mut c = layer.sql();
    c.execute(
        "INSERT INTO cart_lines (owner, position, item, name, quantity, price) \
         VALUES ('ada', 0, 'espresso', 'Espresso', 1, 350)",
        &[],
    )
    .expect("a cart");
    (layer, c)
}

/// **The store's snapshot on PostgreSQL** is a render's read transaction,
/// repeatable read: another writer's commit after its first read is read by
/// a read of its own, not by the render's.
#[test]
fn on_postgres_the_stores_snapshot_is_a_renders_read_transaction() {
    let Some(url) = store_database() else { return };
    let (layer, mut c) = store_on_postgres(&url);
    let snapshot = layer.snapshot().expect("a render's snapshot");
    assert_eq!(ada_quantity(layer.reads_in(&snapshot, "s", None)), 1);
    c.execute(
        "UPDATE cart_lines SET quantity = 5 WHERE owner = 'ada'",
        &[],
    )
    .expect("another writer's commit");
    assert_eq!(
        ada_quantity(layer.reads_in(&snapshot, "s", None)),
        1,
        "the render's"
    );
    assert_eq!(ada_quantity(layer.reads("s", None)), 5, "a read's own");
}

/// **On PostgreSQL, a render's read that fails ends its transaction**, which
/// it aborted: the render's next read opens another, and is answered.
#[test]
fn on_postgres_a_renders_failed_read_ends_its_transaction() {
    let Some(url) = store_database() else { return };
    let (layer, mut c) = store_on_postgres(&url);
    let snapshot = layer.snapshot().expect("a render's snapshot");
    let read = |op: &str| layer.reads_in(&snapshot, "s", None)[op](&[text("47")]);
    read("store:data/stores#get").expect("the render's first read");
    c.batch_execute("ALTER TABLE notices RENAME TO notices_away")
        .expect("renamed");
    let failed = read("store:data/notices#current");
    c.batch_execute("ALTER TABLE notices_away RENAME TO notices")
        .expect("renamed back");
    assert!(failed.is_err(), "a read that fails: {failed:?}");
    read("store:data/kitchen#prep-minutes").expect("the render's next read");
}

/// One read of store 47 in a render of its own, on `layer`.
fn read_in_a_render(layer: &crate::store_pg::StorePg) -> Result<Vec<Val>, String> {
    let snapshot = layer.snapshot().expect("a render's snapshot");
    layer.reads_in(&snapshot, "s", None)["store:data/stores#get"](&[text("47")])
}

/// **On PostgreSQL, a render reads in a seat of the renders' own, never a
/// connection of the commands'**: with every connection the commands'
/// bound allows held, a render reads. The control: a single read, which
/// takes one of the commands' connections, waits, and reads once one is
/// given back.
#[test]
fn on_postgres_a_render_reads_in_a_seat_with_the_commands_bound_held() {
    let Some(url) = store_database() else { return };
    let (layer, _c) = store_on_postgres(&url);
    let (tx, rx) = std::sync::mpsc::channel();
    let held = layer.hold_the_bound();
    std::thread::scope(|scope| {
        let layer = &layer;
        scope.spawn(move || tx.send(read_in_a_render(layer)));
        let read = rx.recv_timeout(Duration::from_secs(20));
        drop(held);
        assert!(
            matches!(read, Ok(Ok(_))),
            "a render's read, the commands' bound held: {read:?}"
        );
    });
    let (tx, rx) = std::sync::mpsc::channel();
    let held = layer.hold_the_bound();
    std::thread::scope(|scope| {
        let layer = &layer;
        scope.spawn(move || {
            tx.send(layer.reads("s", None)["store:data/stores#get"](&[text(
                "47",
            )]))
        });
        let early = rx.recv_timeout(Duration::from_millis(500));
        drop(held);
        let late = rx.recv_timeout(Duration::from_secs(20));
        assert!(early.is_err(), "a single read waits: {early:?}");
        assert!(matches!(late, Ok(Ok(_))), "and then reads: {late:?}");
    });
}

/// **On PostgreSQL, a render past the bound waits for a seat**: with every
/// seat held, a render's read waits, and reads once one is given back.
#[test]
fn on_postgres_a_render_past_the_bound_waits_for_a_seat() {
    let Some(url) = store_database() else { return };
    let (layer, _c) = store_on_postgres(&url);
    let mut seated: Vec<_> = (0..RENDER_CONNECTIONS)
        .map(|_| layer.snapshot().expect("a seat"))
        .collect();
    let renders = layer.renders_for_test().expect("the database's renders");
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::scope(|scope| {
        let layer = &layer;
        scope.spawn(move || tx.send(read_in_a_render(layer)));
        // The render found every seat taken and waits for one: the seats say
        // so (a pause of 500 ms and "it has not read yet" was a silence a
        // slow read makes too).
        wait_until(|| renders.waiting() >= 1, "the render waiting for a seat");
        let early = rx.try_recv();
        seated.pop();
        let late = rx.recv_timeout(Duration::from_secs(20));
        assert!(early.is_err(), "a render past the bound waits: {early:?}");
        assert!(
            matches!(late, Ok(Ok(_))),
            "and reads once a seat is given back: {late:?}"
        );
    });
}

/// **On PostgreSQL, a render that waits past its limit for a seat fails,
/// saying so**: never a read outside the render's snapshot, nor a seat past
/// the bound.
#[test]
fn on_postgres_a_render_waiting_past_its_limit_fails_saying_so() {
    let Some(url) = store_database() else { return };
    let (layer, _c) = store_on_postgres(&url);
    let renders = layer.renders_for_test().expect("the database's renders");
    renders.waiting_at_most(Duration::from_millis(300));
    let _seated: Vec<_> = (0..RENDER_CONNECTIONS)
        .map(|_| layer.snapshot().expect("a seat"))
        .collect();
    let why = read_in_a_render(&layer).expect_err("no seat");
    assert!(why.contains("waited"), "{why}");
    assert_eq!(renders.taken(), RENDER_CONNECTIONS, "none past the bound");
}

/// **A deployment states its renders' seats, or its host does not start**
/// (the integrator's answer of 2026-10-17): `PW_RENDER_SEATS` and
/// `PW_RENDER_SEAT_WAIT`, each its default where unset or empty, read where
/// given, and refused, naming the setting, where it is no count of one at
/// least.
#[test]
fn a_deployment_states_its_renders_seats_or_its_host_is_refused() {
    use crate::feed_pg::RenderSeats;
    let default = RenderSeats::default();
    assert_eq!(default.seats, RENDER_CONNECTIONS, "where it says nothing");
    assert_eq!(RenderSeats::stated(None, None), Ok(default));
    assert_eq!(RenderSeats::stated(Some(""), Some("")), Ok(default));
    assert_eq!(
        RenderSeats::stated(Some("3"), Some("2")),
        Ok(RenderSeats {
            seats: 3,
            wait: Duration::from_secs(2)
        })
    );
    assert_eq!(
        RenderSeats::stated(Some("12"), None),
        Ok(RenderSeats {
            seats: 12,
            ..default
        })
    );
    assert_eq!(
        RenderSeats::stated(None, Some("45")),
        Ok(RenderSeats {
            wait: Duration::from_secs(45),
            ..default
        })
    );
    for (seats, wait, named) in [
        (Some("0"), None, "PW_RENDER_SEATS=0 "),
        (Some("eight"), None, "PW_RENDER_SEATS=eight "),
        (Some("-1"), None, "PW_RENDER_SEATS=-1 "),
        (None, Some("0"), "PW_RENDER_SEAT_WAIT=0 "),
        (None, Some("1.5"), "PW_RENDER_SEAT_WAIT=1.5 "),
        (None, Some("30s"), "PW_RENDER_SEAT_WAIT=30s "),
    ] {
        let why = RenderSeats::stated(seats, wait).expect_err("refused");
        assert!(why.contains(named), "{why}");
    }
}

/// **On PostgreSQL, a layer seats as many renders as its deployment says,
/// and a render waits for a seat as long as it says**: two seats, both
/// taken, and a third render refused after the deployment's half second, not
/// a test host's five seconds, saying it waited for one of the two.
#[test]
fn on_postgres_a_deployments_seats_bound_its_renders_and_their_wait() {
    let Some(url) = store_database() else { return };
    let seats = crate::feed_pg::RenderSeats {
        seats: 2,
        wait: Duration::from_millis(500),
    };
    let layer = crate::store_pg::StorePg::for_test_seated(
        &url,
        crate::feed_pg::Isolation::Serializable,
        seats,
    )
    .expect("the layer");
    let renders = layer.renders_for_test().expect("the database's renders");
    let _seated: Vec<_> = (0..2).map(|_| layer.snapshot().expect("a seat")).collect();
    assert_eq!(renders.taken(), 2, "the deployment's two");
    let began = Instant::now();
    let why = read_in_a_render(&layer).expect_err("no third seat");
    let waited = began.elapsed();
    assert!(why.contains("one of the 2 connections"), "{why}");
    assert!(
        waited >= Duration::from_millis(500),
        "the deployment's wait: {waited:?}"
    );
    assert!(
        waited < Duration::from_secs(3),
        "the deployment's wait, not a test host's five seconds: {waited:?}"
    );
    assert_eq!(renders.taken(), 2, "none past the deployment's bound");
}

/// The store served on PostgreSQL, where its tests serve it there: in
/// memory a render holds no seat.
fn store_on_its_database() -> Option<Served> {
    if std::env::var("PW_STORE_TEST_LAYER").as_deref() != Ok("postgres") {
        eprintln!("skipped: the store is not served on PostgreSQL");
        return None;
    }
    Some(store())
}

/// Waits for `done`, twenty seconds at most, or fails saying for `what`.
fn wait_until(done: impl Fn() -> bool, what: &str) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !done() {
        assert!(Instant::now() < deadline, "never: {what}");
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// **On PostgreSQL, the bound holds under many renders at once**:
/// twenty-four renders of the reader's cart page, each held at its reads
/// until the bound's eight were seated together, all read the page, never
/// more than eight seated at once, nor more connections open for them.
#[test]
fn on_postgres_the_renders_bound_holds_under_many_renders_at_once() {
    let Some(s) = store_on_its_database() else {
        return;
    };
    let doc = cart_page(&s);
    let renders = s.data.renders_for_test().expect("the database's renders");
    let seated = renders.clone();
    *s.while_reading.lock().expect("while reading") = Some(Arc::new(move |_: &Server| {
        let deadline = Instant::now() + Duration::from_secs(10);
        while seated.most().0 < RENDER_CONNECTIONS && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(1));
        }
    }));
    let read: Vec<Result<Vec<(String, i64)>, String>> = std::thread::scope(|scope| {
        let s = &s;
        let each: Vec<_> = (0..3 * RENDER_CONNECTIONS)
            .map(|_| scope.spawn(move || s.bindings("a", doc).map(|b| lines_of(&b["cart"]))))
            .collect();
        each.into_iter()
            .map(|r| r.join().expect("a render"))
            .collect()
    });
    *s.while_reading.lock().expect("while reading") = None;
    let espresso = vec![("espresso".to_string(), 1)];
    assert!(
        read.iter().all(|r| r.as_ref() == Ok(&espresso)),
        "every render read the page: {read:?}"
    );
    let (seats, open) = renders.most();
    assert_eq!(seats, RENDER_CONNECTIONS, "the bound, reached and held");
    assert!(open <= RENDER_CONNECTIONS, "{open} connections open");
}

/// **No render holds its seat waiting for the subscriber table** (the
/// integrator's ruling 2): each of `changes`' sessions' commands tells its
/// open page, read in a render held at its reads until every seat is taken;
/// a reader holding the table, as a document's last attempt does
/// (ADR-0151), then waits for a seat. Released, the commands' renders end
/// before their telling takes the table, the reader is seated and reads,
/// and every command is answered. Were a render to hold its seat through
/// its telling, it would wait for the table the reader holds, and the
/// reader for a seat until its limit, five seconds here, refused it. Then
/// the same, the table taken before the changes.
fn no_render_holds_its_seat_waiting_for_the_table(
    s: &Server,
    changes: &[(String, u64)],
    change: &(dyn Fn(&Server, &str) -> Result<(), String> + Sync),
) {
    assert_eq!(changes.len(), RENDER_CONNECTIONS, "a seat each");
    let renders = s.data.renders_for_test().expect("the database's renders");
    renders.waiting_at_most(Duration::from_secs(5));
    let released = Arc::new(AtomicBool::new(false));
    let held = released.clone();
    *s.while_reading.lock().expect("while reading") = Some(Arc::new(move |_: &Server| {
        let deadline = Instant::now() + Duration::from_secs(20);
        while !held.load(Ordering::SeqCst) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(1));
        }
    }));
    let (session, doc) = changes[0].clone();
    let (answered, reader) = std::thread::scope(|scope| {
        let each: Vec<_> = changes
            .iter()
            .map(|(session, _)| scope.spawn(move || change(s, session)))
            .collect();
        wait_until(
            || renders.taken() == RENDER_CONNECTIONS,
            "every seat taken by a change's render",
        );
        let reader = scope.spawn(move || {
            let _table = s.pending.lock().expect("pending");
            s.bindings(&session, doc).map(|_| ())
        });
        wait_until(
            || renders.waiting() == 1,
            "the reader waiting for a seat, the table held",
        );
        released.store(true, Ordering::SeqCst);
        let reader = reader.join().expect("the reader");
        let answered: Vec<_> = each
            .into_iter()
            .map(|c| c.join().expect("a change"))
            .collect();
        (answered, reader)
    });
    *s.while_reading.lock().expect("while reading") = None;
    assert_eq!(reader, Ok(()), "the reader, seated once the renders ended");
    assert!(answered.iter().all(Result::is_ok), "{answered:?}");
    // And the table taken first: each change waits for it where its
    // documents are taken, holding no seat, and the reader holding it is
    // seated at once. Were a render to begin before its documents are taken,
    // every change would hold a seat waiting for the table, and the reader
    // would wait for one until its limit refused it.
    let (session, doc) = changes[1].clone();
    let (answered, reader) = std::thread::scope(|scope| {
        let table = s.pending.lock().expect("pending");
        let each: Vec<_> = changes
            .iter()
            .map(|(session, _)| scope.spawn(move || change(s, session)))
            .collect();
        let deadline = Instant::now() + Duration::from_secs(2);
        while renders.taken() < RENDER_CONNECTIONS && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(1));
        }
        let reader = s.bindings(&session, doc).map(|_| ());
        drop(table);
        let answered: Vec<_> = each
            .into_iter()
            .map(|c| c.join().expect("a change"))
            .collect();
        (answered, reader)
    });
    assert_eq!(reader, Ok(()), "the reader holding the table, seated");
    assert!(answered.iter().all(Result::is_ok), "{answered:?}");
}

/// [`no_render_holds_its_seat_waiting_for_the_table`], on the store: each
/// change a command whose drain tells the session's cart page.
#[test]
fn on_postgres_a_drains_render_never_holds_its_seat_waiting_for_the_table() {
    let Some(s) = store_on_its_database() else {
        return;
    };
    let changes: Vec<(String, u64)> = (0..RENDER_CONNECTIONS)
        .map(|i| {
            let session = format!("d{i}");
            let doc = cart_page_of(&s, &session);
            (session, doc)
        })
        .collect();
    no_render_holds_its_seat_waiting_for_the_table(&s, &changes, &|s, session| {
        s.command(
            "store.page.add_to_cart",
            session,
            &add_shown("drip", 1),
            false,
        )
    });
}

/// [`no_render_holds_its_seat_waiting_for_the_table`], on the feed: each
/// change a post, whose commit tells the session's home page.
#[test]
fn on_postgres_a_feed_renders_never_hold_their_seats_waiting_for_the_table() {
    let Some(url) = feed_database() else { return };
    let s = super::feed_pg::served_on(&url, "render_merge_table");
    let changes: Vec<(String, u64)> = (0..RENDER_CONNECTIONS)
        .map(|i| {
            let session = format!("f{i}");
            let (_, doc, _, _) = s
                .serve_document_settled(&session, "feed.app.Home~ui", &Params::new(), &[])
                .expect("served");
            (session, doc)
        })
        .collect();
    // Each post an interaction of its own.
    let posts = std::sync::atomic::AtomicUsize::new(0);
    no_render_holds_its_seat_waiting_for_the_table(&s, &changes, &|s, session| {
        let n = posts.fetch_add(1, Ordering::SeqCst);
        let answered = s.command_answered(
            "feed.app.post",
            session,
            &[text("Told, the table held.")],
            Some(&format!("post-{session}-{n}")),
        )?;
        match answered.committed {
            true => Ok(()),
            false => Err(format!("{:?}", answered.result)),
        }
    });
}

/// A read held in flight once it has read, until `released` says: what
/// `Server::in_flight` holds, once, telling `flying` when it is.
fn held_in_flight(
    s: &Server,
    flying: std::sync::mpsc::Sender<()>,
    released: std::sync::mpsc::Receiver<()>,
) {
    let released = Mutex::new(released);
    *s.in_flight.lock().expect("in flight") = Some(Box::new(move |_: &Server| {
        flying.send(()).expect("told");
        let _ = released
            .lock()
            .expect("released")
            .recv_timeout(Duration::from_secs(20));
    }));
}

/// The reader's cart, as one render of `doc` reads it, alone.
fn cart_read(s: &Server, doc: u64) -> Result<Vec<(String, i64)>, String> {
    s.bindings_where("a", Some(doc), |b| b == "cart", &Keys::Shown)
        .map(|mut b| lines_of(&b.remove("cart").expect("the cart")))
        .map_err(|why| format!("{why:?}"))
}

/// **A render reads alone what a commit invalidated since it began** (the
/// integrator's ruling 3): a render reads the reader's cart, one espresso,
/// its read held in flight; the reader's own command adds a drip and
/// invalidates `Cart`, which stops that read; a second render, begun after,
/// reads the two lines, its read held in flight in its turn. The first,
/// released, reads again: alone, in its own snapshot, one espresso, without
/// waiting for the second's read, which it would take were it to read as
/// any reader does, newer than its snapshot.
#[test]
fn a_render_reads_alone_what_a_commit_invalidated_since_it_began() {
    let s = store();
    let doc = cart_page(&s);
    let (first_flying, first_in_flight) = std::sync::mpsc::channel();
    let (release_first, first_released) = std::sync::mpsc::channel::<()>();
    held_in_flight(&s, first_flying, first_released);
    let (first, second) = std::thread::scope(|scope| {
        let s = &s;
        let (tx, first) = std::sync::mpsc::channel();
        scope.spawn(move || tx.send(cart_read(s, doc)));
        first_in_flight
            .recv_timeout(Duration::from_secs(20))
            .expect("the first render's read in flight");
        s.command("store.page.add_to_cart", "a", &add_shown("drip", 1), false)
            .expect("the reader's command commits");
        let (second_flying, second_in_flight) = std::sync::mpsc::channel();
        let (release_second, second_released) = std::sync::mpsc::channel::<()>();
        held_in_flight(s, second_flying, second_released);
        let second = scope.spawn(move || cart_read(s, doc));
        let flying = second_in_flight.recv_timeout(Duration::from_secs(20));
        release_first.send(()).expect("released");
        let read = first.recv_timeout(Duration::from_secs(5));
        release_second.send(()).expect("released");
        flying.expect("the second render's read in flight");
        (read, second.join().expect("the second render"))
    });
    assert_eq!(
        first,
        Ok(Ok(vec![("espresso".to_string(), 1)])),
        "the first render's own snapshot, read alone, not waiting for the second's read"
    );
    let mut lines = second.expect("read");
    lines.sort();
    assert_eq!(
        lines,
        [("drip".to_string(), 1), ("espresso".to_string(), 1)],
        "the second's"
    );
}

/// Whether `doc` was told to read itself again.
fn told_to_reload(s: &Server, doc: &Doc) -> bool {
    s.pending.lock().expect("pending")[doc]
        .frames
        .iter()
        .any(|(_, f)| {
            matches!(
                f,
                StreamFrame::Recovery {
                    recovery: Recovery::Reload,
                    ..
                }
            )
        })
}

/// The reader's next read of its cart fails, once (ADR-0174).
fn the_next_cart_read_fails(s: &Server) {
    s.store
        .faults()
        .cart_faults
        .lock()
        .expect("cart faults")
        .entry("a".into())
        .or_default()
        .fail_read = true;
}

/// **A document that could not be read is said so after its render ends**
/// (the integrator's ruling 2): the reader's cart page, told a change, its
/// cart's read failing, is told to read itself again, the table taken once
/// the render ended (a render takes it in no test).
#[test]
fn a_document_that_could_not_be_read_is_said_so_after_its_render() {
    let s = store();
    let doc = cart_page(&s);
    the_next_cart_read_fails(&s);
    s.clock.advance(1);
    let version = Version(s.clock.now());
    s.send_documents("a", &session_documents("a"), version, false);
    assert!(told_to_reload(&s, &("a".to_string(), doc)));
}

/// **A session whose count could not be read is said so after the drain's
/// render ends**: the reader's command commits, and its drain's read of the
/// cart fails; the reader's page is told to read itself again, the table
/// taken once the render ended.
#[test]
fn a_session_whose_count_could_not_be_read_is_said_so_after_its_render() {
    let s = store();
    let doc = cart_page(&s);
    the_next_cart_read_fails(&s);
    s.command("store.page.add_to_cart", "a", &add_shown("drip", 1), false)
        .expect("the command commits");
    assert!(told_to_reload(&s, &("a".to_string(), doc)));
}

/// **The feed's snapshot in memory** is the feed as the render first read
/// it: a post committed after is read by a read of its own, not by the
/// render's.
#[test]
fn the_memory_feeds_snapshot_is_the_feed_as_a_render_first_read_it() {
    let layer = crate::feed::FeedData::new();
    // The session as the program passes it, its handle (track `data-path`).
    let a = crate::identity::session_handle("a");
    let timeline = |ops: Ops| match &ops["feed:data/posts#timeline"](&[text(&a), Val::S64(20)])
        .expect("read")[..]
    {
        [Val::List(posts)] => posts.len(),
        other => panic!("no timeline: {other:?}"),
    };
    let snapshot = layer.snapshot().expect("a render's snapshot");
    let before = timeline(layer.reads_in(&snapshot, "a", None));
    {
        let mut staging = DataLayer::begin(&layer, "a");
        staging.ops()["feed:data/posts#publish"](&[
            text(&a),
            text("After the render's first read."),
        ])
        .expect("posted");
        staging.publish();
    }
    assert_eq!(
        timeline(layer.reads_in(&snapshot, "a", None)),
        before,
        "the render's"
    );
    assert_eq!(timeline(layer.reads("a", None)), before + 1, "a read's own");
}

/// **The feed's snapshot on PostgreSQL** is a render's read transaction: a
/// notification committed after its first read is counted by a read of its
/// own, not by the render's.
#[test]
fn on_postgres_the_feeds_snapshot_is_a_renders_read_transaction() {
    let Some(url) = feed_database() else { return };
    let s = super::feed_pg::served_on(&url, "render_merge_layer");
    let reader = crate::identity::guest_user("a");
    let unread = |ops: Ops| ops["feed:data/notifications#unread"](&[text(&reader)]).expect("read");
    let snapshot = s.data.snapshot().expect("a render's snapshot");
    assert_eq!(unread(s.data.reads_in(&snapshot, "a", None)), [Val::S64(0)]);
    s.data
        .execute_for_test(&notified())
        .expect("another writer's commit");
    assert_eq!(
        unread(s.data.reads_in(&snapshot, "a", None)),
        [Val::S64(0)],
        "the render's"
    );
    assert_eq!(
        unread(s.data.reads("a", None)),
        [Val::S64(1)],
        "a read's own"
    );
}

/// **On PostgreSQL, the feed's render read that fails ends its
/// transaction**, as the store's does: the render's next read opens another,
/// and is answered.
#[test]
fn on_postgres_a_feed_renders_failed_read_ends_its_transaction() {
    let Some(url) = feed_database() else { return };
    let s = super::feed_pg::served_on(&url, "render_merge_failed");
    let snapshot = s.data.snapshot().expect("a render's snapshot");
    let read = |op: &str, arg: &str| s.data.reads_in(&snapshot, "a", None)[op](&[text(arg)]);
    read("feed:data/posts#author", "p1").expect("the render's first read");
    s.data
        .execute_for_test("ALTER TABLE notifications RENAME TO notifications_away")
        .expect("renamed");
    let failed = read("feed:data/notifications#unread", "u-a");
    s.data
        .execute_for_test("ALTER TABLE notifications_away RENAME TO notifications")
        .expect("renamed back");
    assert!(failed.is_err(), "a read that fails: {failed:?}");
    read("feed:data/messages#unread", "u-a").expect("the render's next read");
}
