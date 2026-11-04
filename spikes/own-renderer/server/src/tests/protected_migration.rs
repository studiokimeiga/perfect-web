//! Generic authority seams required before the canonical feed/store port.
use super::tables::Asserted;
use super::*;

const ROWS: &str = r#"module GuestCarts
import capability.{ UserId }
type Cart = Cart { owner: UserId, text: String }
table GuestCarts
    row Cart
    key owner
    owned by owner
source CartData
    holds GuestCarts
    transactions serializable
    reads strong
    changes none
    sets one_call
"#;

const APP: &str = r#"module protected.cart
import capability.{ User, UserId, id_of }
import context.{ current_user, account_of }
import GuestCarts.{ Cart }
import GuestCarts
opaque type InteractionId = String
event CartChanged(owner: UserId)
predicate SignedIn
    says "Sign in first."
user query CartOf(reader: User<UserId>) -> String
    freshness 0.seconds
    consistency read_your_writes
    cache private
    key reader
    invalidates_on CartChanged(reader)
    concurrency one_per_key
    on_key_change cancel
    timeout 2.seconds
{
    match GuestCarts.get(reader) { Some(cart) => cart.text, None => "EMPTY" }
}
user query AccountOf(reader: User<UserId>) -> Bool
    freshness 0.seconds
    consistency read_your_writes
    cache private
    key reader
    invalidates_on CartChanged(reader)
    concurrency one_per_key
    on_key_change cancel
    timeout 2.seconds
{
    match account_of(reader) { Some(_) => true, None => false }
}
command keep(text: String) -> ()
    idempotent_by InteractionId
    transaction serializable
    emits CartChanged(id_of(current_user()))
{
    GuestCarts.put(current_user(), Cart { owner: id_of(current_user()), text: text })
}
command forge(owner: UserId, text: String) -> ()
    transaction serializable
    emits CartChanged(owner)
{
    GuestCarts.put(current_user(), Cart { owner: owner, text: text })
}
command clear() -> ()
    transaction serializable
    emits CartChanged(id_of(current_user()))
{
    GuestCarts.delete(current_user())
}
command account_only(text: String) -> ()
    requires SignedIn
    transaction serializable
    emits CartChanged(id_of(current_user()))
{
    GuestCarts.put(current_user(), Cart { owner: id_of(current_user()), text: text })
}
user page CartPage() {
    route "/"
    placement origin
    cache private
    let cart = query CartOf(current_user())
    let account = query AccountOf(current_user())
    view { <title>Guest carts</title><main><p id="cart">{cart}</p>{#if account}<p>ACCOUNT</p>{:else}<p>ANONYMOUS</p>{/if}</main> }
}
"#;

const CLOCK: &str = r#"module protected.clock
import context.{ current_user }
import capability.{ id_of }
import clock.{ minutes }
import GuestCarts.{ Cart }
import GuestCarts
import protected.cart.{ CartChanged }
opaque type InteractionId = String
command remind() -> Result<Int, String>
    invoked_by Clock
    idempotent_by InteractionId
    emits CartChanged(id_of(current_user()))
{
    match GuestCarts.get(current_user()) {
        Some(cart) => {
            if cart.text == "ALICE_BEFORE" {
                GuestCarts.put(current_user(), Cart { owner: id_of(current_user()), text: "ALICE_REMIND" })
                Ok(1)
            } else { Err("unexpected cart") }
        },
        None => Err("missing cart"),
    }
}
command later() -> () {
    schedule remind() after minutes(30)
}
"#;

fn built() -> (tempfile::TempDir, std::path::PathBuf) {
    super::tables::built(
        &[],
        &[
            ("GuestCarts.pw", ROWS),
            ("cart.pw", APP),
            ("clock.pw", CLOCK),
        ],
    )
}

fn accounts(server: &Server) {
    server.identity.use_provider(
        Arc::new(super::sign_in::TestProvider::default()),
        super::sign_in::REDIRECT,
    );
}

fn page(server: &Server, session: &str) -> String {
    fetched_as(server, "/", Some(session))
        .into_iter()
        .map(|(_, s)| s)
        .collect()
}

fn table_calls(server: &Server) -> Option<u64> {
    server
        .data
        .table_calls_for_test()
        .or_else(|| server.data.statements())
}

fn guest_cart_contract(server: &Server) {
    accounts(server);
    let first = page(server, "guest-a");
    assert!(
        first.starts_with("HTTP/1.1 200") && first.contains("EMPTY") && first.contains("ANONYMOUS"),
        "{first}"
    );
    let kept = server
        .command_answer(
            "protected.cart.keep",
            "guest-a",
            &[serde_json::json!("A_SECRET")],
            Some("keep-a"),
        )
        .asserted("guest keep answered");
    assert!(kept.committed, "{kept:?}");
    assert!(page(server, "guest-a").contains("A_SECRET"));
    let other = page(server, "guest-b");
    assert!(
        other.contains("EMPTY") && !other.contains("A_SECRET"),
        "{other}"
    );
    let calls = table_calls(server);
    assert!(calls.is_some(), "the adapter counts table accesses");
    let forged = server
        .command_answer(
            "protected.cart.forge",
            "guest-b",
            &[
                serde_json::json!(identity::guest_user("guest-a")),
                serde_json::json!("FORGED"),
            ],
            None,
        )
        .asserted("forgery answered");
    assert!(!forged.committed, "{forged:?}");
    assert_eq!(
        table_calls(server),
        calls,
        "forged ownership refused before adapter access"
    );
    let signed = server
        .command_answer(
            "protected.cart.account_only",
            "guest-a",
            &[serde_json::json!("SIGNED")],
            None,
        )
        .asserted("signed-only answered");
    assert!(!signed.committed && signed.refused.is_some(), "{signed:?}");
    assert_eq!(table_calls(server), calls, "SignedIn remains false");
    let after = page(server, "guest-a");
    assert!(
        after.contains("A_SECRET") && !after.contains("FORGED") && after.contains("ANONYMOUS"),
        "{after}"
    );
    let cleared = server
        .command_answer("protected.cart.clear", "guest-a", &[], None)
        .asserted("guest clear answered");
    assert!(cleared.committed, "{cleared:?}");
    assert!(page(server, "guest-a").contains("EMPTY"));
}

#[test]
fn accounts_mode_guests_use_their_own_rows_in_memory() {
    let (_dir, out) = built();
    let server = Server::from_build(out.clone(), out).asserted("generic memory server");
    guest_cart_contract(&server);
}

#[test]
fn accounts_mode_guests_use_their_own_rows_on_postgresql() {
    let Some(url) = super::tables_pg::database() else {
        return;
    };
    let schema = super::tables_pg::Schema::new(&url, "guest_carts");
    let (_dir, out) = built();
    let server = super::tables_pg::serve(&out, &schema);
    assert!(
        server.data.tables().is_some() && server.data.tail().is_some(),
        "generic PostgreSQL adapter active"
    );
    guest_cart_contract(&server);
    let wrote = server
        .command_answer(
            "protected.cart.keep",
            "guest-b",
            &[serde_json::json!("PERSISTED_GUEST")],
            Some("persist-b"),
        )
        .asserted("persist answered");
    assert!(wrote.committed, "{wrote:?}");
    assert_eq!(
        schema.texts("SELECT convert_from(text, 'UTF8') FROM \"GuestCarts\""),
        ["PERSISTED_GUEST"]
    );
    drop(server);
    let reopened = super::tables_pg::serve(&out, &schema);
    accounts(&reopened);
    assert!(page(&reopened, "guest-b").contains("PERSISTED_GUEST"));
    assert!(!page(&reopened, "guest-a").contains("PERSISTED_GUEST"));
}

/// The new fallback requires this Principals instance's explicit immutable
/// browser binding. Neither a cache miss nor another thread/session's binding
/// supplies it. Existing account and on-behalf callers keep their own path.
#[test]
fn anonymous_row_authority_is_explicit_scoped_and_immutable() {
    use identity::ReaderIdentity;
    use pw_host::tables::Caller;
    let (_dir, out) = built();
    let server = Server::from_build(out.clone(), out).asserted("generic server");
    accounts(&server);
    let principals = server.identity.principals();
    assert_eq!(principals.anonymous_row_user("a"), None);
    assert_eq!(server.invocation_caller("a"), Caller::Nobody);
    let actor = principals
        .command_actor("a")
        .asserted("anonymous actor captured");
    assert!(actor.reader == ReaderIdentity::Anonymous);
    let other = identity::Principals::default();
    principals.with_reader("a", &actor.reader, || {
        let guest = identity::guest_user("a");
        assert_eq!(server.invocation_caller("a"), Caller::User(guest.clone()));
        assert_eq!(principals.anonymous_row_user("b"), None);
        assert_eq!(other.anonymous_row_user("a"), None);
        let worker = std::thread::spawn({
            let principals = principals.clone();
            let reader = actor.reader.clone();
            move || {
                assert_eq!(principals.anonymous_row_user("a"), None);
                principals.with_reader("a", &reader, || principals.anonymous_row_user("a"))
            }
        })
        .join()
        .unwrap();
        assert_eq!(worker, Some(guest.clone()));
        server
            .identity
            .signed_in_for_test("a", "account-a", "Account A");
        assert_eq!(server.invocation_caller("a"), Caller::User(guest));
        assert!(
            principals.of("a").is_none(),
            "anonymous binding remains anonymous after cache changes"
        );
        assert!(
            actor.ensure_current().is_err(),
            "stale anonymous delivery is fenced"
        );
    });
    assert_eq!(
        server.invocation_caller("a"),
        Caller::User("account-a".into())
    );
    for session in ["", "on-behalf:clock:test"] {
        principals.with_reader(session, &ReaderIdentity::Anonymous, || {
            assert_eq!(principals.anonymous_row_user(session), None);
            assert_eq!(server.invocation_caller(session), Caller::Nobody);
        });
    }
}

fn stale_guest_command(server: &Server, opening_host: &Server) {
    accounts(server);
    accounts(opening_host);
    let (entered, reached) = std::sync::mpsc::channel();
    let (release, go) = std::sync::mpsc::channel::<()>();
    *server.before_command_authority.lock().unwrap() = Some(Box::new(move |_| {
        let _ = entered.send(());
        let _ = go.recv_timeout(std::time::Duration::from_secs(20));
    }));
    let result = std::thread::scope(|scope| {
        let command = scope.spawn(|| {
            server.command_answer(
                "protected.cart.keep",
                "guest-a",
                &[serde_json::json!("STALE")],
                Some("stale-guest"),
            )
        });
        let held = reached
            .recv_timeout(std::time::Duration::from_secs(20))
            .is_ok();
        opening_host
            .identity
            .signed_in_for_test("guest-a", "now-account", "Account");
        let _ = release.send(());
        let answer = command.join();
        assert!(held, "captured anonymous actor reached authority seam");
        answer
    });
    assert!(
        matches!(&result, Ok(Err(_))) || matches!(&result, Ok(Ok(a)) if !a.committed),
        "{result:?}"
    );
    let neighbor = server
        .command_answer(
            "protected.cart.keep",
            "guest-b",
            &[serde_json::json!("NEIGHBOR")],
            Some("neighbor"),
        )
        .asserted("current guest answered");
    assert!(neighbor.committed, "{neighbor:?}");
}

#[test]
fn a_guest_became_an_account_before_authority_keeps_no_memory_write() {
    let (_dir, out) = built();
    let server = Server::from_build(out.clone(), out).asserted("generic memory server");
    stale_guest_command(&server, &server);
    // Inspect storage independently of delivery authority: guest-a now reads
    // its account partition, which would hide a stale guest write.
    let contract = server
        .contracts
        .iter()
        .find(|c| c.component_id == "protected.cart.CartOf")
        .unwrap();
    let granted = server
        .authorise(&contract.component_id)
        .asserted("query grant");
    let reads = server
        .data
        .table_reads(contract, &granted, None)
        .asserted("adapter reads");
    let guest = reads["pw:tables/guest-carts#get"](&[Val::String(identity::guest_user("guest-a"))])
        .asserted("original guest partition");
    assert!(
        matches!(guest.as_slice(), [Val::Option(None)]),
        "stale guest row: {guest:?}"
    );
    let key = identity::interaction_key("guest-a", "protected.cart.keep", "stale-guest");
    assert_eq!(
        server
            .recalled_interaction(&key)
            .asserted("stale receipt inspected"),
        None
    );
    let current = page(&server, "guest-a");
    assert!(
        current.contains("EMPTY") && !current.contains("STALE"),
        "{current}"
    );
}

#[test]
fn a_guest_became_an_account_on_another_host_keeps_no_postgresql_write() {
    let Some(url) = super::tables_pg::database() else {
        return;
    };
    let schema = super::tables_pg::Schema::new(&url, "stale_guest");
    let (_dir, out) = built();
    let a = super::tables_pg::serve(&out, &schema);
    let b = super::tables_pg::serve(&out, &schema);
    stale_guest_command(&a, &b);
    assert_eq!(schema.count("\"GuestCarts\""), "1");
    assert_eq!(
        schema.texts("SELECT convert_from(text, 'UTF8') FROM \"GuestCarts\""),
        ["NEIGHBOR"]
    );
    assert_eq!(schema.count("pw_interactions"), "1");
    assert_eq!(schema.count("pw_outbox"), "1");
}

fn owned_clock_contract(server: &Server) {
    accounts(server);
    server
        .identity
        .signed_in_for_test("alice-session", "alice", "Alice");
    server
        .identity
        .signed_in_for_test("bob-session", "bob", "Bob");
    for (session, text, id) in [
        ("alice-session", "ALICE_BEFORE", "a"),
        ("bob-session", "BOB_PRIVATE", "b"),
    ] {
        let kept = server
            .command_answer(
                "protected.cart.keep",
                session,
                &[serde_json::json!(text)],
                Some(id),
            )
            .asserted("owned cart kept");
        assert!(kept.committed, "{kept:?}");
    }
    let schedule = || {
        let answer = server
            .command_answer("protected.clock.later", "alice-session", &[], None)
            .asserted("scheduled");
        assert!(answer.committed, "{answer:?}");
    };
    schedule();
    let old = schedules::select(server.runs(), server.now() + 31 * 60 * 1000, 16)
        .asserted("selected old generation");
    assert_eq!(old.len(), 1);
    assert_eq!(old[0].user, "alice");
    schedule();
    let current = server.runs().all_runs().asserted("rescheduled");
    assert_eq!(current.len(), 1);
    assert_ne!(old[0].id, current[0].id);
    server.run_scheduled(&old[0]);
    assert!(
        page(server, "alice-session").contains("ALICE_BEFORE"),
        "stale generation wrote nothing"
    );
    assert_eq!(server.runs().all_runs().asserted("still pending"), current);
    let browser = super::sign_in::exchanged(
        server,
        "POST /command/protected.clock.remind HTTP/1.1\r\nHost: t\r\nCookie: pw-session=alice-session\r\nSec-Fetch-Site: same-origin\r\nContent-Length: 2\r\n\r\n[]",
    );
    assert!(browser.starts_with("HTTP/1.1 404"), "{browser}");
    let out = super::sign_in::exchanged(
        server,
        "POST /sign-out HTTP/1.1\r\nHost: t\r\nCookie: pw-session=alice-session\r\nSec-Fetch-Site: same-origin\r\nContent-Length: 0\r\n\r\n",
    );
    assert!(out.starts_with("HTTP/1.1 303"), "{out}");
    assert!(server.identity.principals().of("alice-session").is_none());
    let due = schedules::select(server.runs(), server.now() + 31 * 60 * 1000, 16)
        .asserted("current candidate");
    assert_eq!(due.len(), 1);
    server.query_clock.0.advance(31 * 60 * 1000);
    assert_eq!(server.run_due().0, 1);
    assert!(server.runs().all_runs().asserted("completed").is_empty());
    server.run_scheduled(&due[0]);
    assert_eq!(server.run_due().0, 0, "completed generation cannot replay");
    // A different session of the scheduled owner sees the result after the
    // originating browser signed out. No guest identity stands in for it.
    server
        .identity
        .signed_in_for_test("alice-new", "alice", "Alice");
    let alice = page(server, "alice-new");
    assert!(
        alice.contains("ALICE_REMIND") && !alice.contains("BOB_PRIVATE"),
        "{alice}"
    );
    let bob = page(server, "bob-session");
    assert!(
        bob.contains("BOB_PRIVATE") && !bob.contains("ALICE_REMIND"),
        "{bob}"
    );
}

#[test]
fn the_clock_reads_and_writes_the_scheduling_owners_rows_after_logout_in_memory() {
    let (_dir, out) = built();
    let server = Server::from_build(out.clone(), out).asserted("generic memory server");
    owned_clock_contract(&server);
}

#[test]
fn the_clock_reads_and_writes_the_scheduling_owners_rows_after_logout_on_postgresql() {
    let Some(url) = super::tables_pg::database() else {
        return;
    };
    let schema = super::tables_pg::Schema::new(&url, "owned_clock");
    let (_dir, out) = built();
    let server = super::tables_pg::serve(&out, &schema);
    owned_clock_contract(&server);
    assert_eq!(schema.texts("SELECT convert_from(owner, 'UTF8') || ':' || convert_from(text, 'UTF8') FROM \"GuestCarts\" ORDER BY owner"), ["alice:ALICE_REMIND", "bob:BOB_PRIVATE"]);
    assert_eq!(
        schema.count("pw_interactions"),
        "3",
        "two browser writes and one clock completion"
    );
    let clock = schema.texts("SELECT xmin::text FROM \"GuestCarts\" WHERE convert_from(owner,'UTF8')='alice' UNION ALL SELECT xmin::text FROM pw_interactions WHERE sent = '[]' UNION ALL SELECT xmin::text FROM (SELECT xmin FROM pw_outbox ORDER BY id DESC LIMIT 1) last");
    assert_eq!(clock.len(), 3, "{clock:?}");
    assert!(
        clock.iter().all(|xmin| xmin == &clock[0]),
        "clock row/receipt/event share a transaction: {clock:?}"
    );
}
