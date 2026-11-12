//! **A clock run is admitted as the reader who scheduled it** (L2,
//! ADR-0395; ADR-0407's amendment, after PR #91's `runs_for`): the command
//! the clock runs reads and writes the owned rows of the reader whose
//! command scheduled it, as that reader, and no one else's, after their
//! browser signed out, a run made stale by a reschedule writing nothing, and
//! never replayed once it completed; on memory and on PostgreSQL. Ported
//! from the integration's witness (`4f80e543`, on `fix/row-data-integration`),
//! its program trimmed to what the witness reads.
use super::tables::Asserted;
use super::*;

const ROWS: &str = r#"module GuestCarts
import capability.{ UserId }
type Cart = Cart { owner: UserId, text: String }
table GuestCarts
    row     Cart
    key     owner
    owned   by owner
source CartData
    holds        GuestCarts
    transactions serializable
    reads        strong
    changes      none
    sets         one_call
"#;

const APP: &str = r#"module owned.cart
import capability.{ User, UserId, id_of }
import context.{ current_user }
import GuestCarts.{ Cart }
import GuestCarts
opaque type InteractionId = String
event CartChanged(owner: UserId)
user query CartOf(reader: User<UserId>) -> String
    freshness      0.seconds
    consistency    read_your_writes
    cache          private
    key            reader
    invalidates_on CartChanged(reader)
    concurrency    one_per_key
    on_key_change  cancel
    timeout        2.seconds
{
    match GuestCarts.get(reader) { Some(cart) => cart.text, None => "EMPTY" }
}
command keep(text: String) -> ()
    requires      Anyone
    idempotent_by InteractionId
    transaction   serializable
    emits         CartChanged(id_of(current_user()))
{
    GuestCarts.put(current_user(), Cart { owner: id_of(current_user()), text: text })
}
user page CartPage() {
    route     "/"
    placement origin
    cache     private
    let cart = query CartOf(current_user())
    view { <title>Carts</title><main><p id="cart">{cart}</p></main> }
}
"#;

const CLOCK: &str = r#"module owned.clock
import context.{ current_user }
import capability.{ id_of }
import clock.{ minutes }
import GuestCarts.{ Cart }
import GuestCarts
import owned.cart.{ CartChanged }
opaque type InteractionId = String
command remind() -> Result<Int, String>
    invoked_by    Clock
    idempotent_by InteractionId
    emits         CartChanged(id_of(current_user()))
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

fn page(server: &Server, session: &str) -> String {
    fetched_as(server, "/", Some(session))
        .into_iter()
        .map(|(_, s)| s)
        .collect()
}

/// The witness, on whichever layer `server` serves its tables from.
fn owned_clock_contract(server: &Server) {
    server.identity.use_provider(
        Arc::new(super::sign_in::TestProvider::default()),
        super::sign_in::REDIRECT,
    );
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
                "owned.cart.keep",
                session,
                &[serde_json::json!(text)],
                Some(id),
            )
            .asserted("an owned cart kept");
        assert!(kept.committed, "{kept:?}");
    }
    let schedule = || {
        let answer = server
            .command_answer("owned.clock.later", "alice-session", &[], None)
            .asserted("scheduled");
        assert!(answer.committed, "{answer:?}");
    };
    schedule();
    let old = schedules::select(server.runs(), server.now() + 31 * 60 * 1000, 16)
        .asserted("the first schedule's run");
    assert_eq!(old.len(), 1);
    // The run is the scheduling reader's, and the clock runs it as them.
    assert_eq!(old[0].user, "alice");
    schedule();
    let current = server.runs().all_runs().asserted("rescheduled");
    assert_eq!(current.len(), 1);
    assert_ne!(old[0].id, current[0].id);
    server.run_scheduled(&old[0]);
    assert!(
        page(server, "alice-session").contains("ALICE_BEFORE"),
        "a stale run wrote"
    );
    assert_eq!(server.runs().all_runs().asserted("still pending"), current);
    // No request runs a command the clock invokes.
    let browser = super::sign_in::exchanged(
        server,
        "POST /command/owned.clock.remind HTTP/1.1\r\nHost: t\r\nCookie: pw-session=alice-session\r\nSec-Fetch-Site: same-origin\r\nContent-Length: 2\r\n\r\n[]",
    );
    assert!(browser.starts_with("HTTP/1.1 404"), "{browser}");
    // Her browser signs out; the run is still hers.
    let out = super::sign_in::exchanged(
        server,
        "POST /sign-out HTTP/1.1\r\nHost: t\r\nCookie: pw-session=alice-session\r\nSec-Fetch-Site: same-origin\r\nContent-Length: 0\r\n\r\n",
    );
    assert!(out.starts_with("HTTP/1.1 303"), "{out}");
    assert!(server.identity.principals().of("alice-session").is_none());
    let due = schedules::select(server.runs(), server.now() + 31 * 60 * 1000, 16)
        .asserted("the current run");
    assert_eq!(due.len(), 1);
    server.query_clock.0.advance(31 * 60 * 1000);
    assert_eq!(server.run_due().0, 1);
    assert!(server.runs().all_runs().asserted("completed").is_empty());
    server.run_scheduled(&due[0]);
    assert_eq!(server.run_due().0, 0, "a completed run is not run again");
    // Another session of the scheduling reader's sees what the run wrote,
    // and bob's cart is his alone.
    server
        .identity
        .signed_in_for_test("alice-new", "alice", "Alice");
    let alice = page(server, "alice-new");
    assert!(alice.contains("ALICE_REMIND"), "{alice}");
    assert!(!alice.contains("BOB_PRIVATE"), "{alice}");
    let bob = page(server, "bob-session");
    assert!(bob.contains("BOB_PRIVATE"), "{bob}");
    assert!(!bob.contains("ALICE_REMIND"), "{bob}");
}

#[test]
fn the_clock_reads_and_writes_the_scheduling_owners_rows_after_sign_out_in_memory() {
    let (_dir, out) = built();
    let server = Server::from_build(out.clone(), out).asserted("the generic memory server");
    owned_clock_contract(&server);
}

#[test]
fn the_clock_reads_and_writes_the_scheduling_owners_rows_after_sign_out_on_postgresql() {
    let Some(url) = super::tables_pg::database() else {
        return;
    };
    let schema = super::tables_pg::Schema::new(&url, "owned_clock");
    let (_dir, out) = built();
    let server = super::tables_pg::serve(&out, &schema);
    // Each event the outbox is written, kept with its transaction: a row
    // delivered is forgotten (ADR-0397).
    super::tables_pg::audit_outbox(&schema);
    owned_clock_contract(&server);
    assert_eq!(
        schema.texts(
            "SELECT convert_from(owner, 'UTF8') || ':' || convert_from(text, 'UTF8') \
             FROM \"GuestCarts\" ORDER BY owner"
        ),
        ["alice:ALICE_REMIND", "bob:BOB_PRIVATE"]
    );
    assert_eq!(
        schema.count("pw_interactions"),
        "3",
        "two browser writes and one clock completion"
    );
    let clock = schema.texts(
        "SELECT xmin::text FROM \"GuestCarts\" WHERE convert_from(owner, 'UTF8') = 'alice' \
         UNION ALL SELECT xmin::text FROM pw_interactions WHERE sent = '[]'",
    );
    assert_eq!(clock.len(), 2, "{clock:?}");
    assert!(
        clock.iter().all(|xmin| xmin == &clock[0]),
        "the clock's row and its receipt share a transaction: {clock:?}"
    );
    let events = schema.texts("SELECT xid FROM outbox_seen");
    assert!(
        events.contains(&clock[0]),
        "the clock's event is written in its row's transaction: {events:?}, {clock:?}"
    );
}
