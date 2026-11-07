//! **A command runs when the clock passes its time** (the scheduled-commands
//! track). The store's sweep: an order no one pays is taken back thirty
//! minutes on, on behalf of the reader who placed it, and a payment settled
//! for good, or the reader's own cancel, cancels it. And the host's runs,
//! through a pair of commands a test adds to the store ([`SCHEDULING`]). On
//! the layer the server's tests run on: memory, or PostgreSQL under
//! `PW_STORE_TEST_LAYER=postgres`. Each rule's test has its control.

use super::store_payments::{pay, paying, paying_with, placed_awaiting, status_of};
use super::*;

const CANCEL: &str = "store.page.cancel_order";
const SWEEP: &str = "store.page.expire_order";
const LATER: &str = "store.page.add_later";
const IN_FIVE: &str = "store.page.add_in_five";

/// Thirty minutes, as the store's sweep waits, in milliseconds.
const THIRTY_MINUTES: u64 = 30 * 60 * 1000;

/// Five minutes, as [`SCHEDULING`]'s run waits.
const FIVE_MINUTES: u64 = 5 * 60 * 1000;

/// **Commands a test adds to the store**: `add_in_five`, which schedules a
/// line of an item added to the reader's cart five minutes on, then, where
/// told to refuse, answers a declared error; `add_later`, which the clock
/// runs then; and `sweep_in_five`, which schedules the store's sweep of an
/// order five minutes on.
pub(super) const SCHEDULING: &str = r#"
// A test's: a line of `item` added to the reader's cart, by the clock.
command add_later(item: MenuItemId)
    -> Result<UserCart, CartError>
    invoked_by    Clock
    idempotent_by InteractionId
    transaction   serializable
    invalidates   Cart(current_user())
    emits         UserCartChanged(current_user())
{
    UserCarts.add(current_user(), item, PositiveInt(1))
}

// A test's: `add_later(item)` scheduled five minutes on; then, where
// `refuse`, a declared error, which commits nothing.
command add_in_five(item: MenuItemId, refuse: Bool)
    -> Result<Int, CartError>
    requires      Anyone
    idempotent_by InteractionId
{
    schedule add_later(item) after minutes(5)
    if refuse {
        return Err(CartError.ItemUnavailable(item))
    }
    Ok(1)
}

// A test's: the store's sweep of `order`, scheduled five minutes on.
command sweep_in_five(order: String)
    -> Result<Int, CartError>
    requires      Anyone
    idempotent_by InteractionId
{
    schedule expire_order(order) after minutes(5)
    Ok(1)
}
"#;

/// The store's `app.pw`, with [`SCHEDULING`]'s commands.
pub(super) fn with_scheduling(app: &str) -> String {
    format!("{app}{SCHEDULING}")
}

/// The store with [`SCHEDULING`]'s commands.
fn scheduling() -> Served {
    served_from_patches_in("examples", with_scheduling, &[])
}

/// `add_in_five(item, refuse)`, pressed by the reader of session `a`.
pub(super) fn in_five(s: &Server, item: &str, refuse: bool, interaction: &str) -> Answered {
    s.command_answered(
        IN_FIVE,
        "a",
        &[Val::String(item.into()), Val::Bool(refuse)],
        Some(interaction),
    )
    .expect("answered")
}

/// **The host's clock, moved `ms` on**, as `/bench/clock` moves it.
fn later(s: &Server, ms: u64) {
    s.query_clock.0.advance(ms);
}

/// Every run the host keeps, pending or failed for good, earliest first.
fn runs(s: &Server) -> Vec<pw_materialize::ScheduledRun> {
    s.runs().all_runs().expect("the runs read")
}

/// **An order no one pays is taken back thirty minutes on** (ADR-0332's
/// sweep), on behalf of the reader who placed it: its payment canceled at
/// the provider, which takes no card for it after, and the order canceled,
/// its lines back in the cart. The control: a minute short of it, the
/// order awaits.
#[test]
fn an_order_no_one_pays_is_taken_back_thirty_minutes_on() {
    paying(|s, _| {
        let page = placed_awaiting(s);
        let placed = s.now();
        let pending = runs(s);
        assert_eq!(pending.len(), 1, "{pending:?}");
        assert_eq!(pending[0].command, SWEEP);
        assert_eq!(pending[0].user, s.identity.principals().user_of("a"));
        let after = pending[0].due_ms - placed;
        assert!(
            (1_799_000..=1_800_000).contains(&after),
            "due thirty minutes after the commit: {after} ms after"
        );
        later(s, THIRTY_MINUTES - 60_000);
        assert_eq!(s.run_due().0, 0, "a minute before");
        assert_eq!(
            status_of(s, "a"),
            Some(("awaiting-payment".to_string(), Some(page.clone())))
        );
        later(s, 60_000);
        assert_eq!(s.run_due().0, 1, "at thirty minutes");
        assert_eq!(status_of(s, "a"), Some(("canceled".to_string(), None)));
        assert_eq!(
            s.cart_lines("a"),
            vec![("espresso".to_string(), 2), ("cortado".to_string(), 1)]
        );
        assert!(runs(s).is_empty(), "answered, and removed");
        let paid = pay(&page, "4242424242424242");
        assert!(paid.starts_with("HTTP/1.1 409"), "{paid}");
    });
}

/// **An order swept is told to its reader's open page**: the run is the
/// host's own, made by no connection, and the page open on the order is
/// told it was taken back as it is told of the reader's own cancel. The
/// provider's word that the payment is canceled is held back: heard, its
/// own connection would tell the page.
#[test]
fn an_order_swept_is_told_to_its_readers_open_page() {
    paying(|s, origin| {
        placed_awaiting(s);
        s.serve_document_settled("a", "store.page.OrderPage~ui", &Params::new(), &[])
            .expect("served");
        let doc = latest(&s.pending.lock().expect("pending"), "a");
        origin.hold();
        later(s, THIRTY_MINUTES);
        assert_eq!(s.run_due().0, 1);
        assert!(
            sets_of(s, &doc)
                .iter()
                .any(|set| { written(set).contains("Canceled: its items are back in your cart.") }),
            "{:?}",
            sets_of(s, &doc)
        );
    });
}

/// **A payment settled for good cancels its order's sweep; one declined
/// keeps it**: a decline is no outcome for the order, which still awaits a
/// card, and is still taken back if none comes. Paid, the order stands past
/// thirty minutes.
#[test]
fn a_paid_order_is_not_swept_and_a_declined_card_keeps_its_sweep() {
    paying(|s, _| {
        let page = placed_awaiting(s);
        let declined = pay(&page, "4000000000000002");
        assert!(declined.contains("Your card was declined."), "{declined}");
        assert_eq!(runs(s).len(), 1, "declined: its sweep stays");
        let paid = pay(&page, "4242424242424242");
        assert!(paid.contains("Paid."), "{paid}");
        assert!(runs(s).is_empty(), "paid: its sweep cancelled");
        later(s, THIRTY_MINUTES);
        assert_eq!(s.run_due().0, 0);
        assert_eq!(status_of(s, "a"), Some(("placed".to_string(), None)));
    });
}

/// **An order its reader takes back is not swept**: the cancel cancels its
/// sweep, in the same commit. The provider's word that the payment is
/// canceled would cancel it too, by `settle_payment`, once heard; it is held
/// back here, so what is seen is the cancel's own. Until 2026-10-14 it was
/// not, and the test held whichever came first: a mutant that took the
/// cancel's away survived one run of two. The control: before it, the
/// sweep is pending.
#[test]
fn an_order_taken_back_by_its_reader_is_not_swept() {
    paying(|s, origin| {
        placed_awaiting(s);
        assert_eq!(runs(s).len(), 1, "the control: its sweep");
        origin.hold();
        let taken = s
            .command_answered(CANCEL, "a", &[], Some("cancel-1"))
            .expect("answered");
        assert!(taken.committed, "{:?}", taken.result);
        assert!(runs(s).is_empty(), "{:?}", runs(s));
    });
}

/// **A sweep takes back only the order it was scheduled for**: one for
/// another order finds the reader's order not that one, and it stands. The
/// control: its own sweep, at thirty minutes, takes it back.
#[test]
fn a_sweep_takes_back_only_its_own_order() {
    paying_with(with_scheduling, |s, _| {
        let page = placed_awaiting(s);
        let swept = s
            .command_answered(
                "store.page.sweep_in_five",
                "a",
                &[Val::String("o-another".into())],
                Some("i-1"),
            )
            .expect("answered");
        assert!(swept.committed, "{:?}", swept.result);
        later(s, FIVE_MINUTES);
        assert_eq!(s.run_due().0, 1, "another order's sweep");
        assert_eq!(
            status_of(s, "a"),
            Some(("awaiting-payment".to_string(), Some(page)))
        );
        later(s, THIRTY_MINUTES);
        assert_eq!(s.run_due().0, 1, "its own");
        assert_eq!(status_of(s, "a"), Some(("canceled".to_string(), None)));
    });
}

/// **A reader's press of a command the clock runs is a command that is not
/// there**: the sweep, pressed, takes nothing back. The control: the same
/// route takes a command a reader presses.
#[test]
fn a_readers_press_of_the_sweep_is_not_a_command() {
    paying(|s, _| {
        let page = placed_awaiting(s);
        let forged = posted(
            s,
            "/command/store.page.expire_order",
            "a",
            "forged-1",
            r#"["o-0000000000000000"]"#,
        );
        assert!(forged.starts_with("HTTP/1.1 404"), "{forged}");
        assert_eq!(
            status_of(s, "a"),
            Some(("awaiting-payment".to_string(), Some(page)))
        );
        let control = posted(s, "/command/store.page.clear_cart", "a", "control-1", "[]");
        assert!(control.contains("\"committed\":true"), "{control}");
    });
}

/// **A command that answers an error schedules nothing**: the run it staged
/// goes with its writes. The control: the same command answering `Ok`
/// commits its run, though it writes nothing else.
#[test]
fn a_command_that_answers_an_error_schedules_nothing() {
    let s = scheduling();
    let refused = in_five(&s, "espresso", true, "i-1");
    assert!(!refused.committed, "{:?}", refused.result);
    assert!(runs(&s).is_empty(), "{:?}", runs(&s));
    let scheduled = in_five(&s, "espresso", false, "i-2");
    assert!(scheduled.committed, "{:?}", scheduled.result);
    let pending = runs(&s);
    assert_eq!(pending.len(), 1, "{pending:?}");
    assert_eq!(pending[0].command, LATER);
    assert_eq!(pending[0].args, r#"["espresso"]"#);
}

/// **A run runs once the clock passes its time, and not before, on behalf
/// of the user whose command scheduled it**: the line is added to that
/// reader's cart, and the run is removed, so it runs once.
#[test]
fn a_run_runs_at_its_time_on_its_schedulers_behalf_and_once() {
    let s = scheduling();
    assert!(in_five(&s, "espresso", false, "i-1").committed);
    later(&s, FIVE_MINUTES - 1000);
    assert_eq!(s.run_due().0, 0, "a second before its time");
    assert!(s.cart_lines("a").is_empty());
    later(&s, 1000);
    assert_eq!(s.run_due().0, 1);
    assert_eq!(s.cart_lines("a"), vec![("espresso".to_string(), 1)]);
    assert!(runs(&s).is_empty());
    later(&s, FIVE_MINUTES);
    assert_eq!(s.run_due().0, 0, "run once");
    assert_eq!(s.cart_lines("a"), vec![("espresso".to_string(), 1)]);
}

/// **Scheduled again, a run moves**: one run is pending for a command and
/// its arguments, at the time the later scheduling says. The control:
/// another item's is another run.
#[test]
fn a_run_scheduled_again_moves() {
    let s = scheduling();
    assert!(in_five(&s, "espresso", false, "i-1").committed);
    let first = runs(&s)[0].due_ms;
    later(&s, 60_000);
    assert!(in_five(&s, "espresso", false, "i-2").committed);
    let pending = runs(&s);
    assert_eq!(pending.len(), 1, "{pending:?}");
    assert!(pending[0].due_ms >= first + 60_000, "{pending:?}");
    assert!(in_five(&s, "cortado", false, "i-3").committed);
    assert_eq!(runs(&s).len(), 2, "{:?}", runs(&s));
}

/// **A run refused runs again after 2 s, doubling, and after five runs
/// again is kept, failed, and said**: here its command may not run, its
/// events having nowhere to be kept (no `outbox.write`, ADR-0208). Kept, it
/// is not run again, and the host's `/metrics` counts it.
#[test]
fn a_run_refused_runs_again_and_is_then_kept_failed() {
    let mut s = scheduling();
    assert!(in_five(&s, "espresso", false, "i-1").committed);
    s.topology.nodes[0].grants.retain(|g| g != "outbox.write");
    later(&s, FIVE_MINUTES);
    for failures in 1..=5 {
        let before = s.now();
        assert_eq!(s.run_due().0, 1, "run {failures}");
        let run = runs(&s).remove(0);
        assert_eq!(run.attempts, failures);
        assert_eq!(run.failed, None);
        let wait = run.due_ms - before;
        let expected = crate::schedules::backoff_ms(failures);
        assert!(
            (expected..expected + 1000).contains(&wait),
            "after run {failures}, {wait} ms, not {expected}"
        );
        later(&s, u64::try_from(run.due_ms - s.now()).unwrap_or(0));
    }
    assert_eq!(s.run_due().0, 1, "the sixth run");
    let kept = runs(&s);
    assert_eq!(kept.len(), 1, "{kept:?}");
    assert_eq!(kept[0].attempts, 6);
    assert!(
        kept[0]
            .failed
            .as_deref()
            .is_some_and(|why| why.contains("may not run")),
        "{kept:?}"
    );
    later(&s, THIRTY_MINUTES);
    assert_eq!(s.run_due().0, 0, "failed for good: not run again");
    assert!(s.cart_lines("a").is_empty());
    let metrics = get_as(&s, "a", "/metrics");
    for counted in [r#""failed":1"#, r#""retried":5"#, r#""ran":0"#] {
        assert!(metrics.contains(counted), "{counted}: {metrics}");
    }
}
