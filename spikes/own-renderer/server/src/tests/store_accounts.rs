//! **A cart and an order are a user's** (track `store-accounts`, ADR-0318;
//! docs/PARALLEL.md, W8's plan, milestone 1): the store's readers signed in
//! through a provider of the test's own (`tests/sign_in.rs`'s), their carts
//! and orders keyed by the reader's handle, a guest's cart joining its user's
//! at sign-in, and an order read by its user alone.
//!
//! Each runs on the layer the server's tests serve the store on: in memory,
//! or on PostgreSQL where `PW_STORE_TEST_LAYER=postgres` names one
//! (`store::layer_for_tests`), so each holds on both.

use super::sign_in::{
    REDIRECT, TestProvider, exchanged, get, location, set_cookie, signed_in, text,
};
use super::*;

/// **The canonical store, its readers signed in through `provider`**: the
/// accounts model, where a session no sign-in opened is a guest, its user
/// its session's own (ADR-0270).
fn store_with(provider: Arc<TestProvider>) -> Served {
    let s = served_from_patches_in("examples", |app| app.to_string(), &[]);
    s.identity.use_provider(provider, REDIRECT);
    s
}

/// What the page `page` shows `session`, as text.
fn shown_to(s: &Server, session: &str, page: &str) -> String {
    let (html, ..) = s
        .serve_document_settled(session, page, &Params::new(), &[])
        .expect("served");
    text(&html)
}

/// **Two users' carts never share a line** (charter §15.6 test 12, "User
/// A's cart can never be observed by User B"): each adds to their own, reads
/// their own, and their pages show their own; a user's second session holds
/// the same cart; and a guest's is its own, empty.
#[test]
fn two_users_carts_never_share_a_line() {
    let provider = Arc::new(TestProvider::default());
    let s = store_with(provider.clone());
    let ada = signed_in(&s, &provider, "came-ada", "ada");
    let ben = signed_in(&s, &provider, "came-ben", "ben");
    s.command(ADD, &ada, &add_shown("espresso", 2), false)
        .expect("ada adds");
    s.command(ADD, &ben, &add_shown("cortado", 1), false)
        .expect("ben adds");
    assert_eq!(s.cart_lines(&ada), [("espresso".to_string(), 2)]);
    assert_eq!(s.cart_lines(&ben), [("cortado".to_string(), 1)]);
    // The cart is the user's: ada's second session holds it, and adds to it.
    let ada_again = signed_in(&s, &provider, "came-ada-2", "ada");
    assert_eq!(s.cart_lines(&ada_again), [("espresso".to_string(), 2)]);
    s.command(ADD, &ada_again, &add_shown("espresso", 1), false)
        .expect("ada adds again");
    assert_eq!(s.cart_lines(&ada), [("espresso".to_string(), 3)]);
    assert_eq!(s.cart_lines(&ben), [("cortado".to_string(), 1)]);
    // By page: each shows its own lines and not the other's.
    let (mine, theirs) = (shown_to(&s, &ada, CART_PAGE), shown_to(&s, &ben, CART_PAGE));
    assert!(
        mine.contains("Espresso") && !mine.contains("Cortado"),
        "{mine}"
    );
    assert!(
        theirs.contains("Cortado") && !theirs.contains("Espresso"),
        "{theirs}"
    );
    // A guest, signed in as no one, holds a cart of its own, empty.
    assert!(s.cart_lines("a-guest").is_empty());
    assert!(shown_to(&s, "a-guest", CART_PAGE).contains("Your cart is empty."));
    // And the layer answers a user's cart for that user's handle alone.
    let read = |user: &str| {
        lines_of(
            match &s.layer_read(&ada, "store:data/user-carts#current", user) {
                Val::Result(Ok(Some(cart))) => cart,
                other => panic!("user-carts#current answered {other:?}"),
            },
        )
    };
    assert_eq!(read("ada"), [("espresso".to_string(), 3)]);
    assert_eq!(read("ben"), [("cortado".to_string(), 1)]);
}

/// **A user's cart changed in one session reaches their other sessions'
/// open pages** (ADR-0270's listener rule: `UserCartChanged(reader)` binds
/// the user's id), live, as a laptop and a phone; and **another user's open
/// page is sent no frame at all** (telling by principal): until it, a
/// version with nothing in it, another user's activity sent to every reader.
#[test]
fn a_users_cart_reaches_each_of_their_sessions_and_no_one_elses() {
    let provider = Arc::new(TestProvider::default());
    let s = store_with(provider.clone());
    let ada = signed_in(&s, &provider, "came-ada", "ada");
    let ada_again = signed_in(&s, &provider, "came-ada-2", "ada");
    let ben = signed_in(&s, &provider, "came-ben", "ben");
    for session in [&ada, &ada_again, &ben] {
        shown_to(&s, session, CART_PAGE);
    }
    let docs = {
        let pending = s.pending.lock().expect("pending");
        [&ada_again, &ben].map(|session| latest(&pending, session))
    };
    let before = docs.clone().map(|doc| sets_of(&s, &doc).len());
    let frames = |doc: &Doc| s.pending.lock().expect("pending")[doc].frames.len();
    let bens_frames = frames(&docs[1]);
    s.command(ADD, &ada, &add_shown("espresso", 1), false)
        .expect("ada adds");
    s.tell_waiting();
    let told: Vec<String> = sets_of(&s, &docs[0])[before[0]..]
        .iter()
        .map(written)
        .collect();
    // The line's total, $3.50, and the count, 1: what ada's add changed.
    assert!(
        told.iter().any(|w| w.contains("$3.50")),
        "ada's other session is told: {told:?}"
    );
    assert_eq!(
        sets_of(&s, &docs[1]).len(),
        before[1],
        "ben is sent no patch set"
    );
    assert_eq!(frames(&docs[1]), bens_frames, "ben is sent no frame at all");
}

/// **A shared entry still reaches every reader** (ADR-0219), a private one
/// keyed by a user that user's sessions alone (telling by principal): store
/// 48's menu dropped reaches every reader of `Menu`, its own page's reader
/// and another's, where ada's cart dropped reaches ada's other session and
/// not ben's.
#[test]
fn a_shared_entry_still_reaches_every_reader() {
    let provider = Arc::new(TestProvider::default());
    let s = store_with(provider.clone());
    let ada = signed_in(&s, &provider, "came-ada", "ada");
    let ada_again = signed_in(&s, &provider, "came-ada-2", "ada");
    let ben = signed_in(&s, &provider, "came-ben", "ben");
    for session in [&ada, &ada_again, &ben] {
        s.serve_store_document(session, STORE_ID).expect("served");
    }
    let menu = s.invalidate_queries(
        &ada,
        &[(
            "store.page.Menu".to_string(),
            vec![Some(Val::String("48".into()))],
        )],
        &[],
    );
    assert_eq!(
        menu,
        [Reached {
            resource: "store.page.Menu".to_string(),
            user: None
        }],
        "every reader's"
    );
    let mut told = s.others_reading(&ada, &menu);
    told.sort();
    let mut every = vec![ada_again.clone(), ben.clone()];
    every.sort();
    assert_eq!(told, every, "every other reader of the menu");
    let cart = s.invalidate_queries(
        &ada,
        &[(
            "store.page.Cart".to_string(),
            vec![Some(Val::String("ada".into()))],
        )],
        &[],
    );
    assert_eq!(
        cart,
        [Reached {
            resource: "store.page.Cart".to_string(),
            user: Some("ada".to_string())
        }],
        "ada's"
    );
    assert_eq!(
        s.others_reading(&ada, &cart),
        [ada_again],
        "ada's other session alone"
    );
}

/// **A guest's cart follows them in** (docs/PARALLEL.md, Q2): at sign-in each
/// of the guest's lines is added to the user's cart, one item's quantities
/// summed, in one transaction, the guest's cart emptied; the sign-in is
/// answered after it, so the page the browser asks for next shows it. Run
/// again, a join finds nothing; a session already signed in that signs in
/// again joins nothing of its user's to the next.
#[test]
fn a_guests_cart_follows_them_in() {
    let provider = Arc::new(TestProvider::default());
    let s = store_with(provider.clone());
    // Ada already holds an espresso, from another device.
    let earlier = signed_in(&s, &provider, "came-earlier", "ada");
    s.command(ADD, &earlier, &add_shown("espresso", 1), false)
        .expect("ada adds");
    // A guest adds two espressos and a cortado, and signs in as ada.
    s.command(ADD, "guest-1", &add_shown("espresso", 2), false)
        .expect("the guest adds");
    s.command(ADD, "guest-1", &add_shown("cortado", 1), false)
        .expect("the guest adds");
    let ada = signed_in(&s, &provider, "guest-1", "ada");
    assert_eq!(
        s.cart_lines(&ada),
        [("espresso".to_string(), 3), ("cortado".to_string(), 1)],
        "each line added, one item's quantities summed"
    );
    assert_eq!(
        s.cart_lines(&earlier),
        s.cart_lines(&ada),
        "one user's cart"
    );
    // The guest's cart is emptied, in the same transaction.
    assert!(
        s.cart_lines("guest-1").is_empty(),
        "the guest's cart is emptied"
    );
    let page = shown_to(&s, &ada, CART_PAGE);
    assert!(
        page.contains("Espresso") && page.contains("Cortado"),
        "{page}"
    );
    // Run twice, the second finds nothing to join.
    s.joined(&identity::SignedIn {
        guest: "guest-1",
        session: &ada,
        principal: &identity::Principal {
            user: "ada".to_string(),
            handle: "@ada".to_string(),
            name: "Ada".to_string(),
            issuer: "https://id.example.test".to_string(),
        },
    });
    assert_eq!(
        s.cart_lines(&ada),
        [("espresso".to_string(), 3), ("cortado".to_string(), 1)]
    );
    // Ada, signed in, signs in again as ben: what she holds stays hers.
    let ben = signed_in(&s, &provider, &ada, "ben");
    assert!(s.cart_lines(&ben).is_empty(), "ben is joined ada's cart");
    assert_eq!(s.cart_lines(&earlier).len(), 2, "ada keeps hers");
}

/// **A session already signed in that signs in as someone else joins
/// nothing to them**, on a program that keys its cart by the session (the
/// benchmark's copy of the store), where the session's cart is what the
/// reader signed in held: the identity tells the deployment of a guest's
/// sign-in alone, and a signed-in session is no guest.
#[test]
fn a_signed_in_sessions_cart_is_not_joined_to_the_next_user() {
    let provider = Arc::new(TestProvider::default());
    let s = served_from_patches(|app| app.to_string(), &[]);
    s.identity.use_provider(provider.clone(), REDIRECT);
    // This program's cart requires a reader signed in: ada's session's.
    let ada = signed_in(&s, &provider, "came-ada", "ada");
    s.command(ADD, &ada, &add("espresso", 1), false)
        .expect("ada adds");
    assert_eq!(s.cart_lines(&ada), [("espresso".to_string(), 1)]);
    // Ada, signed in, signs in again as ben: what her session held stays.
    let ben = signed_in(&s, &provider, &ada, "ben");
    assert!(
        s.cart_lines(&ben).is_empty(),
        "ben is joined ada's session's cart"
    );
    assert_eq!(s.cart_lines(&ada), [("espresso".to_string(), 1)]);
}

/// **On a program that keys its cart by the session, a session's own change
/// reaches no other session** (the benchmark's copy of the store): an entry
/// pinned to a session is that session's alone, though its key names no
/// user. The canonical store's cart is a user's, and telling by principal
/// holds it there; this is the rule for a session's own entries, which the
/// canonical store no longer reads.
#[test]
fn a_session_keyed_carts_change_reaches_no_other_session() {
    let provider = Arc::new(TestProvider::default());
    let s = served_from_patches(|app| app.to_string(), &[]);
    s.identity.use_provider(provider.clone(), REDIRECT);
    let ada = signed_in(&s, &provider, "came-ada", "ada");
    let ben = signed_in(&s, &provider, "came-ben", "ben");
    for session in [&ada, &ben] {
        s.serve_document_settled(
            session,
            "store.page.StorePage~ui",
            &store_params(STORE_ID),
            &[],
        )
        .expect("served");
    }
    s.tell_waiting();
    let (mine, theirs) = {
        let pending = s.pending.lock().expect("pending");
        (latest(&pending, &ada), latest(&pending, &ben))
    };
    let frames = |doc: &Doc| s.pending.lock().expect("pending")[doc].frames.len();
    let (mine_before, theirs_before) = (frames(&mine), frames(&theirs));
    s.command(ADD, &ada, &add("espresso", 1), false)
        .expect("ada adds");
    s.tell_waiting();
    assert!(
        frames(&mine) > mine_before,
        "the session's own page is told"
    );
    assert_eq!(frames(&theirs), theirs_before, "another session's is not");
}

/// **A join past what a line can hold is refused, and the sign-in still
/// completes** (Q2): the guest's lines stay the guest's, the user's cart is as
/// it was, and the browser is signed in.
#[test]
fn a_join_past_a_bigint_is_refused_and_the_sign_in_completes() {
    let provider = Arc::new(TestProvider::default());
    let s = store_with(provider.clone());
    let earlier = signed_in(&s, &provider, "came-earlier", "ada");
    s.command(ADD, &earlier, &add_shown("espresso", 1), false)
        .expect("ada adds");
    s.command(ADD, "guest-2", &add_shown("espresso", 1), false)
        .expect("the guest adds");
    // Around the program, as a database's row can be: the guest's line at
    // the most a bigint holds.
    s.store
        .line_around(&s.owner("guest-2"), 0, i64::MAX)
        .expect("written around the program");
    let started = get(&s, "/sign-in", "pw-session=guest-2");
    let (state, _) = set_cookie(&started, "pw-sign-in").expect("the sign-in's cookie");
    let back = provider.authenticate(&location(&started), "ada", "Ada");
    let answer = get(
        &s,
        &format!("/sign-in/callback?{back}"),
        &format!("pw-session=guest-2; pw-sign-in={state}"),
    );
    assert!(
        answer.starts_with("HTTP/1.1 303"),
        "the sign-in completes: {answer}"
    );
    let ada = set_cookie(&answer, "pw-session").expect("a session").0;
    assert_eq!(
        s.cart_lines(&ada),
        [("espresso".to_string(), 1)],
        "nothing moved"
    );
    let guest = s.owner("guest-2");
    let left = lines_of(
        match &s.layer_read(&ada, "store:data/user-carts#current", &guest) {
            Val::Result(Ok(Some(cart))) => cart,
            other => panic!("user-carts#current answered {other:?}"),
        },
    );
    assert_eq!(
        left,
        [("espresso".to_string(), i64::MAX)],
        "the guest's rows stay"
    );
}

/// **An order is no other user's to read** (as ADR-0274's notifications
/// are): placed from ada's cart, it is ada's in each of her sessions, and
/// ben's page, ben's read of the layer for himself, and a guest's say none.
#[test]
fn an_order_is_no_other_users_to_read() {
    let provider = Arc::new(TestProvider::default());
    let s = store_with(provider.clone());
    let ada = signed_in(&s, &provider, "came-ada", "ada");
    let ben = signed_in(&s, &provider, "came-ben", "ben");
    s.command(ADD, &ada, &add_shown("espresso", 1), false)
        .expect("ada adds");
    s.command(PLACE, &ada, &[s.shown_total(&ada)], false)
        .expect("ada places her order");
    assert_eq!(
        s.order_of(&ada),
        Some(("placed".to_string(), vec![("espresso".to_string(), 1)]))
    );
    let ada_again = signed_in(&s, &provider, "came-ada-2", "ada");
    assert_eq!(
        s.order_of(&ada_again).map(|o| o.0).as_deref(),
        Some("placed")
    );
    assert_eq!(s.order_of(&ben), None);
    assert_eq!(s.order_of("a-guest"), None);
    assert!(shown_to(&s, &ada, ORDER_PAGE).contains("Placed: the store has your order."));
    for other in [ben.as_str(), "a-guest"] {
        let page = shown_to(&s, other, ORDER_PAGE);
        assert!(page.contains("You have no order yet."), "{other}: {page}");
    }
    // The cart it was placed from is empty, in each of ada's sessions.
    assert!(s.cart_lines(&ada_again).is_empty());
}

/// The store's Add, POSTed as `session` as the runtime sends it, naming the
/// page it was pressed on (`pw-document`) where `document` says.
fn pressed_on(s: &Server, session: &str, document: Option<u64>, interaction: &str) -> String {
    let body = serde_json::json!([shown("cortado"), 1]).to_string();
    let named = document
        .map(|d| format!("pw-document: {d}\r\n"))
        .unwrap_or_default();
    exchanged(
        s,
        &format!(
            "POST /command/{ADD} HTTP/1.1\r\nHost: t\r\nCookie: pw-session={session}\r\n\
             Sec-Fetch-Site: same-origin\r\npw-interaction: {interaction}\r\n{named}\
             content-type: application/json\r\ncontent-length: {}\r\n\r\n{body}",
            body.len()
        ),
    )
}

/// **A command is answered for the reader its page was shown to** (Q3, a
/// rule of the platform): a tab whose reader signed in in another, so that
/// the browser now sends another session, is refused at its next press,
/// before the command runs, and told so; one whose page the host no longer
/// holds is told the page is out of date. A press from the page's own
/// session runs, and a request that names no page is answered as before.
#[test]
fn a_command_is_answered_for_the_reader_its_page_was_shown_to() {
    let provider = Arc::new(TestProvider::default());
    let s = store_with(provider.clone());
    // A guest's page, pressed from its own session: it runs.
    s.serve_document_settled(
        "came-ada",
        "store.page.StorePage~ui",
        &store_params(STORE_ID),
        &[],
    )
    .expect("served");
    let guests = latest(&s.pending.lock().expect("pending"), "came-ada").1;
    let answer = pressed_on(&s, "came-ada", Some(guests), "i-1");
    assert!(answer.contains("\"committed\":true"), "{answer}");
    // Signed in in another tab: the browser sends the new session, and the
    // guest's page is not its page.
    let ada = signed_in(&s, &provider, "came-ada", "ada");
    let held = s.cart_lines(&ada);
    let refused = pressed_on(&s, &ada, Some(guests), "i-2");
    assert!(refused.starts_with("HTTP/1.1 403"), "{refused}");
    assert!(
        refused.contains("\"refused\":\"another-reader\"")
            && refused.contains(
                "\"says\":\"You signed in or out in another tab. Reload this page to go on.\""
            ),
        "{refused}"
    );
    assert_eq!(s.cart_lines(&ada), held, "and nothing ran");
    // A page the host does not hold: never served, or forgotten.
    let gone = pressed_on(&s, &ada, Some(9_999_999), "i-3");
    assert!(gone.starts_with("HTTP/1.1 403"), "{gone}");
    assert!(
        gone.contains("\"refused\":\"out-of-date\"")
            && gone.contains("\"says\":\"This page is out of date. Reload it to go on.\""),
        "{gone}"
    );
    assert_eq!(s.cart_lines(&ada), held, "and nothing ran");
    // A request from no page, or from a page the host did not number, as
    // before.
    for (document, interaction) in [(None, "i-4"), (Some(0), "i-5")] {
        let answer = pressed_on(&s, &ada, document, interaction);
        assert!(
            answer.contains("\"committed\":true"),
            "{document:?}: {answer}"
        );
    }
    // Ada's own page, read after the sign-in, runs.
    s.serve_document_settled(
        &ada,
        "store.page.StorePage~ui",
        &store_params(STORE_ID),
        &[],
    )
    .expect("served");
    let hers = latest(&s.pending.lock().expect("pending"), &ada).1;
    let answer = pressed_on(&s, &ada, Some(hers), "i-6");
    assert!(answer.contains("\"committed\":true"), "{answer}");
}

/// **A press the host already answered is answered from what it kept,
/// though its page went stale between the two attempts** (the integrator's
/// finding of 2026-10-11), restricted by ADR-0389: cart adoption after sign-in
/// does not grant the new cookie its guest predecessor's receipt. The old
/// operation still runs once; a same-reader stale-page retry remains valid. The
/// same where the host has since forgotten the page. A press the host never
/// answered is still refused.
#[test]
fn a_retry_of_a_press_that_committed_is_answered_from_what_was_kept() {
    let provider = Arc::new(TestProvider::default());
    let s = store_with(provider.clone());
    let drop_after = |session: &str| {
        s.connection_faults
            .lock()
            .expect("connection faults")
            .entry(session.to_string())
            .or_default()
            .drop_command = Some(DropAt::After);
    };
    s.serve_document_settled(
        "came-ada",
        "store.page.StorePage~ui",
        &store_params(STORE_ID),
        &[],
    )
    .expect("served");
    let guests = latest(&s.pending.lock().expect("pending"), "came-ada").1;
    drop_after("came-ada");
    assert_eq!(
        pressed_on(&s, "came-ada", Some(guests), "i-1"),
        "",
        "the answer is lost"
    );
    // Signed in in another tab before the retry: the guest's line joins.
    let ada = signed_in(&s, &provider, "came-ada", "ada");
    assert_eq!(s.cart_lines(&ada), [("cortado".to_string(), 1)]);
    let retried = pressed_on(&s, &ada, Some(guests), "i-1");
    assert!(
        retried.starts_with("HTTP/1.1 403") && !retried.contains("\"result\""),
        "cart adoption cannot grant another cookie the old receipt: {retried}"
    );
    assert_eq!(
        s.cart_lines(&ada),
        [("cortado".to_string(), 1)],
        "and nothing ran again"
    );
    // A press on that page the host never answered is refused.
    let fresh = pressed_on(&s, &ada, Some(guests), "i-2");
    assert!(fresh.starts_with("HTTP/1.1 403"), "{fresh}");

    // Ada's own page, its answer lost, then forgotten by the host.
    s.serve_document_settled(
        &ada,
        "store.page.StorePage~ui",
        &store_params(STORE_ID),
        &[],
    )
    .expect("served");
    let doc = latest(&s.pending.lock().expect("pending"), &ada);
    drop_after(&ada);
    assert_eq!(pressed_on(&s, &ada, Some(doc.1), "i-3"), "");
    assert_eq!(s.cart_lines(&ada), [("cortado".to_string(), 2)]);
    s.pages.lock().expect("pages").remove(&doc);
    let retried = pressed_on(&s, &ada, Some(doc.1), "i-3");
    assert!(
        retried.starts_with("HTTP/1.1 202") && retried.contains("\"committed\":true"),
        "the retry is told it committed: {retried}"
    );
    assert_eq!(s.cart_lines(&ada), [("cortado".to_string(), 2)], "once");
    let fresh = pressed_on(&s, &ada, Some(doc.1), "i-4");
    assert!(
        fresh.contains("\"refused\":\"out-of-date\""),
        "a press never answered is refused: {fresh}"
    );
}

/// **The stale tab's rule comes before the order's wait** (the integrator's
/// ordering of 2026-10-11: a kept answer, the stale check, the order's wait,
/// the command): a press on a page another reader's, naming a command
/// before it that never comes (`pw-after`, ADR-0305), is refused at once as
/// the page's, not held for the order and answered early.
#[test]
fn a_stale_press_is_refused_before_the_order_waits() {
    let provider = Arc::new(TestProvider::default());
    let s = store_with(provider.clone());
    s.serve_document_settled(
        "came-ada",
        "store.page.StorePage~ui",
        &store_params(STORE_ID),
        &[],
    )
    .expect("served");
    let guests = latest(&s.pending.lock().expect("pending"), "came-ada").1;
    let ada = signed_in(&s, &provider, "came-ada", "ada");
    let body = serde_json::json!([shown("cortado"), 1]).to_string();
    let started = std::time::Instant::now();
    let answer = exchanged(
        &s,
        &format!(
            "POST /command/{ADD} HTTP/1.1\r\nHost: t\r\nCookie: pw-session={ada}\r\n\
             Sec-Fetch-Site: same-origin\r\npw-interaction: i-9\r\npw-after: never-sent\r\n\
             pw-document: {guests}\r\ncontent-type: application/json\r\n\
             content-length: {}\r\n\r\n{body}",
            body.len()
        ),
    );
    assert!(
        answer.starts_with("HTTP/1.1 403") && answer.contains("\"refused\":\"another-reader\""),
        "{answer}"
    );
    assert!(
        started.elapsed() < crate::order::WAIT,
        "not held for the order: {:?}",
        started.elapsed()
    );
}
