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
    requires Anyone
    idempotent_by InteractionId
    transaction serializable
    emits CartChanged(id_of(current_user()))
{
    GuestCarts.put(current_user(), Cart { owner: id_of(current_user()), text: text })
}
command forge(owner: UserId, text: String) -> ()
    requires Anyone
    transaction serializable
    emits CartChanged(owner)
{
    GuestCarts.put(current_user(), Cart { owner: owner, text: text })
}
command clear() -> ()
    requires Anyone
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
command later() -> ()
    requires Anyone
{
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

const JOIN_ROWS: &str = r#"module JoinCarts
import capability.{ UserId }
type Line = Line { owner: UserId, quantity: Int }
table JoinCarts
    row Line
    key owner
    owned by owner
"#;

const JOIN: &str = r#"module protected.join
import capability.{ User, UserId, id_of }
import context.{ current_user, previous_user }
import JoinCarts.{ Line }
import JoinCarts
opaque type InteractionId = String
event QuantityChanged(owner: UserId)
user query Quantity(reader: User<UserId>) -> Int
    freshness 0.seconds
    consistency read_your_writes
    cache private
    key reader
    invalidates_on QuantityChanged(reader)
    concurrency one_per_key
    on_key_change cancel
    timeout 2.seconds
{
    match JoinCarts.get(reader) { Some(cart) => cart.quantity, None => 0 }
}
user query HasPrevious(reader: User<UserId>) -> Bool
    freshness 0.seconds
    consistency read_your_writes
    cache private
    key reader
    invalidates_on QuantityChanged(reader)
    concurrency one_per_key
    on_key_change cancel
    timeout 2.seconds
{
    match previous_user() { Some(_) => true, None => false }
}
command keep(quantity: Int) -> ()
    requires Anyone
    emits QuantityChanged(id_of(current_user()))
{
    JoinCarts.put(current_user(), Line { owner: id_of(current_user()), quantity: quantity })
}
command joined() -> ()
    invoked_by Identity
    idempotent_by InteractionId
    emits QuantityChanged(id_of(current_user()))
{
    match previous_user() {
        Some(previous) => match JoinCarts.get(previous) {
            Some(old) => {
                let current = match JoinCarts.get(current_user()) { Some(cart) => cart.quantity, None => 0 }
                JoinCarts.put(current_user(), Line { owner: id_of(current_user()), quantity: current + old.quantity })
                JoinCarts.delete(previous)
            },
            None => (),
        },
        None => (),
    }
}
user page JoinPage() {
    route "/join"
    placement origin
    cache private
    let quantity = query Quantity(current_user())
    view { <title>Cart quantity</title><main><p id="quantity">{quantity}</p></main> }
}
"#;

fn built_join(join: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let rows = ROWS
        .replace("module GuestCarts", "module GuestCarts\nimport JoinCarts")
        .replace("holds GuestCarts", "holds GuestCarts, JoinCarts");
    super::tables::built(
        &[],
        &[
            ("GuestCarts.pw", &rows),
            ("cart.pw", APP),
            ("JoinCarts.pw", JOIN_ROWS),
            ("join.pw", join),
        ],
    )
}

fn quantity(server: &Server, session: &str, user: &str) -> i64 {
    let principals = server.identity.principals();
    let reader = principals.admit(session).asserted("query reader admitted");
    let answer = principals
        .with_reader(session, &reader, || {
            server.answer(
                "protected.join.Quantity",
                session,
                &[Val::String(user.into())],
            )
        })
        .asserted("quantity answered");
    match answer {
        Val::S64(n) => n,
        other => panic!("quantity: {other:?}"),
    }
}

fn keep_quantity(server: &Server, session: &str, quantity: i64) {
    let answer = server
        .command_answer(
            "protected.join.keep",
            session,
            &[serde_json::json!(quantity)],
            None,
        )
        .asserted("quantity kept");
    assert!(answer.committed, "{answer:?}");
}

fn identity_join_contract(server: &Server, provider: &super::sign_in::TestProvider) -> String {
    server
        .identity
        .signed_in_for_test("target-seed", "target", "Target");
    server.identity.signed_in_for_test("bob-seed", "bob", "Bob");
    keep_quantity(server, "target-seed", 4);
    keep_quantity(server, "bob-seed", 11);
    keep_quantity(server, "joining-guest", 3);
    let session = super::sign_in::signed_in(server, provider, "joining-guest", "target");
    assert_eq!(
        quantity(server, &session, "target"),
        7,
        "compiled lifecycle merged guest and target"
    );
    assert_eq!(
        quantity(server, "target-seed", "target"),
        7,
        "shared target account"
    );
    assert_eq!(
        quantity(server, "bob-seed", "bob"),
        11,
        "another account unchanged"
    );
    assert_eq!(
        quantity(
            server,
            "joining-guest",
            &identity::guest_user("joining-guest")
        ),
        0,
        "old guest emptied"
    );
    let pressed = super::sign_in::exchanged(
        server,
        "POST /command/protected.join.joined HTTP/1.1\r\nHost: t\r\nCookie: pw-session=joining-guest\r\nSec-Fetch-Site: same-origin\r\nContent-Length: 2\r\n\r\n[]",
    );
    assert!(pressed.starts_with("HTTP/1.1 404"), "{pressed}");
    session
}

#[test]
fn a_verified_identity_transition_merges_owned_rows_in_memory() {
    let (_dir, out) = built_join(JOIN);
    let server = Server::from_build(out.clone(), out).asserted("generic server");
    let provider = Arc::new(super::sign_in::TestProvider::default());
    server
        .identity
        .use_provider(provider.clone(), super::sign_in::REDIRECT);
    identity_join_contract(&server, &provider);
}

#[test]
fn a_verified_identity_transition_merges_owned_rows_on_postgresql() {
    let Some(url) = super::tables_pg::database() else {
        return;
    };
    let schema = super::tables_pg::Schema::new(&url, "identity_join");
    let (_dir, out) = built_join(JOIN);
    let server = super::tables_pg::serve(&out, &schema);
    let provider = Arc::new(super::sign_in::TestProvider::default());
    server
        .identity
        .use_provider(provider.clone(), super::sign_in::REDIRECT);
    identity_join_contract(&server, &provider);
    assert_eq!(schema.texts("SELECT convert_from(owner,'UTF8') || ':' || quantity::text FROM \"JoinCarts\" ORDER BY owner"), ["bob:11", "target:7"]);
    assert_eq!(
        schema.count("pw_interactions"),
        "1",
        "the lifecycle's record"
    );
    let xmins = schema.texts("SELECT xmin::text FROM \"JoinCarts\" WHERE convert_from(owner,'UTF8')='target' UNION ALL SELECT xmin::text FROM pw_interactions UNION ALL SELECT xmin::text FROM (SELECT xmin FROM pw_outbox ORDER BY id DESC LIMIT 1) last");
    assert_eq!(xmins.len(), 3, "{xmins:?}");
    assert!(
        xmins.iter().all(|xmin| xmin == &xmins[0]),
        "merge row, receipt and event share one transaction: {xmins:?}"
    );
    drop(server);
    let reopened = super::tables_pg::serve(&out, &schema);
    reopened
        .identity
        .use_provider(provider, super::sign_in::REDIRECT);
    assert_eq!(quantity(&reopened, "target-seed", "target"), 7);
    assert_eq!(
        quantity(
            &reopened,
            "joining-guest",
            &identity::guest_user("joining-guest")
        ),
        0
    );
}

#[derive(Default)]
struct ExchangeProvider {
    provider: super::sign_in::TestProvider,
    during: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}

impl identity::Provider for ExchangeProvider {
    fn issuer(&self) -> String {
        identity::Provider::issuer(&self.provider)
    }
    fn client_id(&self) -> String {
        identity::Provider::client_id(&self.provider)
    }
    fn authorization_endpoint(&self) -> String {
        identity::Provider::authorization_endpoint(&self.provider)
    }
    fn exchange(
        &self,
        code: &str,
        verifier: &str,
        redirect: &str,
    ) -> Result<identity::Claims, String> {
        let claims = identity::Provider::exchange(&self.provider, code, verifier, redirect)?;
        if let Some(during) = self.during.lock().unwrap().take() {
            during();
        }
        Ok(claims)
    }
}

fn seed_join(server: &Server) {
    server
        .identity
        .signed_in_for_test("target-seed", "target", "Target");
    keep_quantity(server, "target-seed", 4);
    keep_quantity(server, "joining-guest", 3);
}

fn exchange_race(server: Arc<Server>, initial_account: bool) {
    let provider = Arc::new(ExchangeProvider::default());
    server
        .identity
        .use_provider(provider.clone(), super::sign_in::REDIRECT);
    seed_join(&server);
    server
        .identity
        .signed_in_for_test("alice-seed", "alice", "Alice");
    keep_quantity(&server, "alice-seed", 9);
    if initial_account {
        server
            .identity
            .signed_in_for_test("joining-guest", "alice", "Alice");
    }
    let weak = Arc::downgrade(&server);
    *provider.during.lock().unwrap() = Some(Box::new(move || {
        let server = weak.upgrade().unwrap();
        if initial_account {
            server.identity.signed_out_for_test("joining-guest");
        } else {
            server
                .identity
                .signed_in_for_test("joining-guest", "alice", "Alice");
        }
    }));
    let session = super::sign_in::signed_in(&server, &provider.provider, "joining-guest", "target");
    assert_eq!(
        quantity(&server, &session, "target"),
        if initial_account { 4 } else { 7 },
        "eligibility is frozen before exchange"
    );
    assert_eq!(
        quantity(&server, "alice-seed", "alice"),
        9,
        "source account rows never become guest rows"
    );
    assert_eq!(
        quantity(
            &server,
            "joining-guest",
            &identity::guest_user("joining-guest")
        ),
        if initial_account { 3 } else { 0 }
    );
    let outside = server.command_answer(
        "protected.join.joined",
        &session,
        &[],
        Some("identity-joined"),
    );
    assert!(
        outside.is_err(),
        "a lifecycle receipt cannot be read outside the one-use proof: {outside:?}"
    );
}

#[test]
fn identity_eligibility_does_not_follow_cache_changes_during_provider_exchange() {
    for initial_account in [false, true] {
        let (_dir, out) = built_join(JOIN);
        let server = Arc::new(Server::from_build(out.clone(), out).asserted("generic server"));
        exchange_race(server, initial_account);
    }
}

#[test]
fn identity_eligibility_is_frozen_with_postgresql_authority() {
    let Some(url) = super::tables_pg::database() else {
        return;
    };
    for initial_account in [false, true] {
        let schema = super::tables_pg::Schema::new(&url, "exchange_race");
        let (_dir, out) = built_join(JOIN);
        let server = Arc::new(super::tables_pg::serve(&out, &schema));
        exchange_race(server, initial_account);
        assert_eq!(
            schema.count("pw_interactions"),
            if initial_account { "0" } else { "1" }
        );
    }
}

fn a_failed_join_keeps_both_partitions(
    server: &Server,
    provider: &super::sign_in::TestProvider,
) -> String {
    let session = super::sign_in::signed_in(server, provider, "joining-guest", "target");
    assert_eq!(
        quantity(server, &session, "target"),
        4,
        "target unchanged on failed lifecycle"
    );
    assert_eq!(
        quantity(
            server,
            "joining-guest",
            &identity::guest_user("joining-guest")
        ),
        3,
        "old guest rows retained on failure"
    );
    assert!(
        server
            .command_answer(
                "protected.join.joined",
                &session,
                &[],
                Some("identity-joined")
            )
            .is_err(),
        "proof retired even on failure"
    );
    session
}

#[test]
fn a_memory_lifecycle_publication_refusal_keeps_guest_and_target_rows() {
    let (_dir, out) = built_join(JOIN);
    let server = Server::from_build(out.clone(), out).asserted("generic server");
    let provider = Arc::new(super::sign_in::TestProvider::default());
    server
        .identity
        .use_provider(provider.clone(), super::sign_in::REDIRECT);
    seed_join(&server);
    *server.before_command_authority.lock().unwrap() = Some(Box::new(|_| {
        crate::tables::FAIL_NEXT_PUBLISH.with(|fail| fail.set(true));
    }));
    let session = a_failed_join_keeps_both_partitions(&server, &provider);
    let key = identity::interaction_key(&session, "protected.join.joined", "identity-joined");
    assert_eq!(
        server
            .recalled_interaction(&key)
            .asserted("receipt inspected"),
        None
    );
}

#[test]
fn a_postgresql_lifecycle_commit_refusal_keeps_guest_target_and_records_atomic() {
    let Some(url) = super::tables_pg::database() else {
        return;
    };
    let schema = super::tables_pg::Schema::new(&url, "join_refused");
    let (_dir, out) = built_join(JOIN);
    let server = super::tables_pg::serve(&out, &schema);
    let provider = Arc::new(super::sign_in::TestProvider::default());
    server
        .identity
        .use_provider(provider.clone(), super::sign_in::REDIRECT);
    seed_join(&server);
    let events = schema.count("pw_outbox");
    schema.sql("CREATE FUNCTION refuse_join() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'fixture refuses commit'; END $$; CREATE CONSTRAINT TRIGGER refuse_join AFTER UPDATE ON \"JoinCarts\" DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION refuse_join()").asserted("deferred refusal installed");
    a_failed_join_keeps_both_partitions(&server, &provider);
    assert_eq!(schema.count("pw_interactions"), "0");
    assert_eq!(schema.count("pw_outbox"), events);
    assert_eq!(schema.count("\"JoinCarts\""), "2");
}

#[test]
fn an_identity_lifecycle_keeps_the_exact_new_session_epoch_fence() {
    let Some(url) = super::tables_pg::database() else {
        return;
    };
    let schema = Arc::new(super::tables_pg::Schema::new(&url, "join_epoch"));
    let (_dir, out) = built_join(JOIN);
    let server = super::tables_pg::serve(&out, &schema);
    let provider = Arc::new(super::sign_in::TestProvider::default());
    server
        .identity
        .use_provider(provider.clone(), super::sign_in::REDIRECT);
    seed_join(&server);
    let before = schema.count("pw_outbox");
    let changing = Arc::clone(&schema);
    *server.before_command_authority.lock().unwrap() = Some(Box::new(move |_| {
        // Replace only the callback's newly opened target generation; the
        // already open target-seed session remains a valid neighbor.
        let seed = identity::hex(&identity::kept_hash("target-seed"));
        changing.sql(&format!("UPDATE pw_sessions SET epoch=decode(repeat('ab',16),'hex') WHERE user_id='target' AND hash <> decode('{seed}','hex')")).asserted("new epoch replaced");
    }));
    let session = super::sign_in::signed_in(&server, &provider, "joining-guest", "target");
    assert_eq!(quantity(&server, "target-seed", "target"), 4);
    assert_eq!(
        quantity(
            &server,
            "joining-guest",
            &identity::guest_user("joining-guest")
        ),
        3
    );
    assert_eq!(schema.count("pw_interactions"), "0");
    assert_eq!(schema.count("pw_outbox"), before);
    assert!(
        server
            .command_answer(
                "protected.join.joined",
                &session,
                &[],
                Some("identity-joined")
            )
            .is_err()
    );
}

#[test]
fn a_table_free_identity_lifecycle_is_refused_at_startup() {
    let src = r#"module Lifecycle
opaque type InteractionId = String
command joined() -> ()
    invoked_by Identity
    idempotent_by InteractionId
{ () }
public page Home() {
    route "/"
    placement origin
    view { <title>Lifecycle</title><main>Ready</main> }
}
"#;
    let (_dir, out) = super::tables::built(&[], &[("Lifecycle.pw", src)]);
    let refused = Server::from_build(out.clone(), out).err();
    assert!(
        refused.as_ref().is_some_and(|why| why
            .contains("Identity lifecycle commands require the generic declared-table layer")),
        "{refused:?}"
    );
}

fn active_receipt_replay_contract(server: &Server) {
    let provider = Arc::new(super::sign_in::TestProvider::default());
    server
        .identity
        .use_provider(provider.clone(), super::sign_in::REDIRECT);
    seed_join(server);
    let observed = Arc::new(Mutex::new(None));
    let checked = Arc::clone(&observed);
    *server.after_identity_lifecycle.lock().unwrap() =
        Some(Box::new(move |server, component, session, _answer| {
            // Observe on the HTTP worker, assert on the test thread. An
            // unwind across thread::scope is no native-assertion control.
            let proof = server
                .identity
                .principals()
                .transition_previous(session, component)
                .is_some();
            let query = server.answer(
                "protected.join.HasPrevious",
                session,
                &[Val::String("target".into())],
            );
            let same = server.identity_answer(component, session);
            server.identity.signed_out_for_test(session);
            let calls = table_calls(server);
            let stale = server.identity_answer(component, session);
            let detail = format!("query={query:?}; same={same:?}; stale={stale:?}");
            *checked.lock().unwrap() = Some((
                proof,
                matches!(query, Ok(Val::Bool(false))),
                same.is_ok_and(|answer| answer.committed),
                stale.is_err(),
                table_calls(server) == calls,
                detail,
            ));
        }));
    super::sign_in::signed_in(server, &provider, "joining-guest", "target");
    let observed = observed.lock().unwrap().take();
    assert!(observed.is_some(), "active-proof replay seam ran");
    let (proof, query, same, stale, no_rows, detail) = observed.unwrap();
    assert!(proof, "proof still active at replay seam");
    assert!(
        query,
        "query cannot borrow outer lifecycle previous handle: {detail}"
    );
    assert!(
        same,
        "current active-proof receipt replay succeeds: {detail}"
    );
    assert!(
        stale,
        "stale target must not receive cached lifecycle success: {detail}"
    );
    assert!(no_rows, "receipt replay ran no row operations: {detail}");
    assert_eq!(
        quantity(server, "target-seed", "target"),
        7,
        "one merge only"
    );
}

#[test]
fn lifecycle_receipt_replay_keeps_target_authority_checks_in_memory() {
    let (_dir, out) = built_join(JOIN);
    let server = Server::from_build(out.clone(), out).asserted("generic server");
    active_receipt_replay_contract(&server);
}

#[test]
fn lifecycle_receipt_replay_keeps_target_authority_checks_on_postgresql() {
    let Some(url) = super::tables_pg::database() else {
        return;
    };
    let schema = super::tables_pg::Schema::new(&url, "join_replay");
    let (_dir, out) = built_join(JOIN);
    let server = super::tables_pg::serve(&out, &schema);
    active_receipt_replay_contract(&server);
    assert_eq!(schema.count("pw_interactions"), "1");
}

fn trapping_join() -> String {
    JOIN.replace("                JoinCarts.delete(previous)", "                let divisor = current - current\n                JoinCarts.put(current_user(), Line { owner: id_of(current_user()), quantity: old.quantity / divisor })\n                JoinCarts.delete(previous)")
}

#[test]
fn a_trapping_lifecycle_discards_its_staged_memory_merge() {
    let (_dir, out) = built_join(&trapping_join());
    let server = Server::from_build(out.clone(), out).asserted("generic server");
    let provider = Arc::new(super::sign_in::TestProvider::default());
    server
        .identity
        .use_provider(provider.clone(), super::sign_in::REDIRECT);
    seed_join(&server);
    a_failed_join_keeps_both_partitions(&server, &provider);
}

#[test]
fn a_trapping_lifecycle_discards_its_staged_postgresql_merge() {
    let Some(url) = super::tables_pg::database() else {
        return;
    };
    let schema = super::tables_pg::Schema::new(&url, "join_trap");
    let (_dir, out) = built_join(&trapping_join());
    let server = super::tables_pg::serve(&out, &schema);
    let provider = Arc::new(super::sign_in::TestProvider::default());
    server
        .identity
        .use_provider(provider.clone(), super::sign_in::REDIRECT);
    seed_join(&server);
    let events = schema.count("pw_outbox");
    a_failed_join_keeps_both_partitions(&server, &provider);
    assert_eq!(schema.count("pw_interactions"), "0");
    assert_eq!(schema.count("pw_outbox"), events);
}

#[test]
fn a_compiled_join_reaches_only_the_targets_private_page_on_another_generic_host() {
    let Some(url) = super::tables_pg::database() else {
        return;
    };
    let schema = super::tables_pg::Schema::new(&url, "join_delivery");
    let (_dir, out) = built_join(JOIN);
    let a = super::tables_pg::serve(&out, &schema);
    let b = super::tables_pg::serve(&out, &schema);
    let provider = Arc::new(super::sign_in::TestProvider::default());
    a.identity
        .use_provider(provider.clone(), super::sign_in::REDIRECT);
    b.identity
        .use_provider(provider.clone(), super::sign_in::REDIRECT);
    seed_join(&a);
    a.identity
        .signed_in_for_test("target-other", "target", "Target");
    a.identity.signed_in_for_test("bob-other", "bob", "Bob");
    keep_quantity(&a, "bob-other", 11);
    for session in ["target-other", "bob-other"] {
        b.identity
            .principals()
            .admit(session)
            .asserted("other host reader admitted");
        b.serve_document_settled(session, "protected.join.JoinPage~ui", &Params::new(), &[])
            .asserted("live private page");
    }
    b.hear();
    let target = super::latest(&b.pending.lock().unwrap(), "target-other");
    let bob = super::latest(&b.pending.lock().unwrap(), "bob-other");
    let target_before = sets_of(&b, &target).len();
    let bob_before = private_frames(&b, &bob);
    super::sign_in::signed_in(&a, &provider, "joining-guest", "target");
    assert_eq!(
        sets_of(&b, &target).len(),
        target_before,
        "no delivery before reading the outbox"
    );
    b.hear();
    let updated = sets_of(&b, &target);
    assert!(
        updated[target_before..]
            .iter()
            .any(|set| set.patches.iter().any(
                |patch| matches!(&patch.operation, PatchOp::ReplaceText { text } if text == "7")
            )),
        "target's actual page received merged quantity: {updated:?}"
    );
    assert_eq!(
        private_frames(&b, &bob),
        bob_before,
        "another account receives no frame"
    );
    let once = updated.len();
    b.hear();
    assert_eq!(
        sets_of(&b, &target).len(),
        once,
        "shared log delivered once"
    );
}

fn private_frames(server: &Server, doc: &Doc) -> Vec<String> {
    server
        .pending
        .lock()
        .unwrap()
        .get(doc)
        .map(|waiting| {
            waiting
                .frames
                .iter()
                .map(|(_, frame)| format!("{frame:?}"))
                .collect()
        })
        .unwrap_or_default()
}

fn revoked_after_lifecycle_commit(server: &Server) -> String {
    let provider = Arc::new(super::sign_in::TestProvider::default());
    server
        .identity
        .use_provider(provider.clone(), super::sign_in::REDIRECT);
    seed_join(server);
    let observed = Arc::new(Mutex::new(None));
    let checked = Arc::clone(&observed);
    *server.before_actor_inspection.lock().unwrap() = Some(Box::new(|server, _, session| {
        // The row/receipt transaction is over; revocation now changes
        // delivery authority without undoing the already committed merge.
        server.identity.signed_out_for_test(session);
    }));
    *server.after_identity_lifecycle.lock().unwrap() =
        Some(Box::new(move |server, component, session, answer| {
            let key = identity::interaction_key(session, component, "identity-joined");
            let receipt = server.recalled_interaction(&key);
            let committed_receipt = receipt.as_ref().is_ok_and(|r| {
                r.as_ref()
                    .is_some_and(|(_, kept)| Answered::from_kept(kept).is_ok_and(|a| a.committed))
            });
            *checked.lock().unwrap() = Some((
                answer.is_err(),
                committed_receipt,
                format!("answer={answer:?}; receipt={receipt:?}"),
            ));
        }));
    let session = super::sign_in::signed_in(server, &provider, "joining-guest", "target");
    let seen = observed.lock().unwrap().take();
    assert!(seen.is_some(), "post-commit observation ran");
    let (refused_delivery, receipt, detail) = seen.unwrap();
    assert!(
        refused_delivery,
        "revoked target receives no current success: {detail}"
    );
    assert!(
        receipt,
        "the successful transaction remains committed: {detail}"
    );
    assert_eq!(quantity(server, "target-seed", "target"), 7);
    assert_eq!(
        quantity(
            server,
            "joining-guest",
            &identity::guest_user("joining-guest")
        ),
        0
    );
    let reader = server
        .identity
        .principals()
        .admit(&session)
        .asserted("revoked target read");
    assert!(reader == identity::ReaderIdentity::Anonymous);
    session
}

#[test]
fn lifecycle_postcommit_revocation_preserves_committed_memory_merge_and_receipt() {
    let (_dir, out) = built_join(JOIN);
    let server = Server::from_build(out.clone(), out).asserted("generic server");
    revoked_after_lifecycle_commit(&server);
}

#[test]
fn lifecycle_postcommit_revocation_preserves_committed_postgresql_merge_and_receipt() {
    let Some(url) = super::tables_pg::database() else {
        return;
    };
    let schema = super::tables_pg::Schema::new(&url, "join_committed_revoked");
    let (_dir, out) = built_join(JOIN);
    let server = super::tables_pg::serve(&out, &schema);
    revoked_after_lifecycle_commit(&server);
    assert_eq!(schema.count("pw_interactions"), "1");
    assert_eq!(
        schema.texts(
            "SELECT quantity::text FROM \"JoinCarts\" WHERE convert_from(owner,'UTF8')='target'"
        ),
        ["7"]
    );
    assert_eq!(schema.count("\"JoinCarts\""), "1");
}
