//! **An order placed for a time its reader chooses** (track
//! `scheduled-orders`, the integrator's design of 2026-10-14): the times
//! offered by the cart's stores' hours, compared with the platform's clock
//! (ADR-0304), in each store's zone (ADR-0328); a time held to them when it is
//! chosen and again when the order is placed; and an order for one paid now
//! and sent to its stores when its preparation is due (ADR-0334). To a store
//! an order for a time is an order for as soon as possible placed when it is
//! told, so a window is offered only where every store takes an order then,
//! by the rule that holds one now (the integrator's ruling of 2026-10-15). On
//! the layer the server's tests run on: memory, or PostgreSQL under
//! `PW_STORE_TEST_LAYER=postgres`. Each rule's test has its control. The
//! stores' zone is San Francisco's; 2027-03-01 is a Monday, before the clocks
//! go forward on the 14th. The estimate is 25 to 35 minutes where a test sets
//! none, so its stores are told 40 minutes before a window, and a store open
//! 07:00 to 19:00 with a 20-minute cut-off takes an order from 07:00 to 18:40.

use super::store_hours::{at, clock_to, daily, every_day, page, set_hours};
use super::store_payments::{pay, paying, paying_with};
use super::*;

const ADD: &str = "store.page.add_to_cart";
const PLACE: &str = "store.page.place_order";
const CHOOSE: &str = "store.page.choose_time";
const SEND: &str = "store.page.send_to_store";
const SWEEP: &str = "store.page.expire_order";

const MINUTE: i64 = 60_000;

fn served() -> Served {
    let s = served_from_patches_in("examples", |app| app.to_string(), &[]);
    no_delay(&s, None);
    s
}

/// The estimator session `a`'s reader reaches, answering at once (its delay,
/// charter 15.5's 400 ms, is a test control, not data: every page of times
/// asks it once a store), failing as `fail` says.
fn no_delay(s: &Server, fail: Option<&str>) {
    s.store
        .faults()
        .estimators
        .lock()
        .expect("estimators")
        .insert(
            "a".to_string(),
            Estimator {
                delay_ms: 0,
                fail: fail.map(str::to_string),
                ..Estimator::default()
            },
        );
}

/// **The estimate session `a`'s reader is given**, as the layer holds it:
/// its least and most minutes.
fn estimating(s: &Server, least: i64, most: i64) {
    s.untold(
        "a",
        "store:host/estimates#set",
        &[
            // The session by its handle, as the host keys it (track
            // `data-path`).
            Val::String(crate::identity::session_handle("a")),
            Val::S64(least),
            Val::Option(Some(Box::new(Val::S64(most)))),
        ],
    );
}

/// **Why an order for as soon as possible would be refused now**, as the
/// cart says it, or nothing: the rule `place_order` holds the hours to.
fn cart_hours(s: &Server) -> String {
    match s
        .query("store.hours.CartHours", "a", &[Val::String(s.owner("a"))])
        .expect("the cart's hours")
    {
        Val::String(said) => said,
        other => panic!("the cart's hours are {other:?}"),
    }
}

/// **A time chosen** by session `a`'s reader, as the page of times presses
/// it: `None` for as soon as possible.
fn choose(s: &Server, when: Option<i64>, interaction: &str) -> Answered {
    let when = Val::Option(when.map(|at| Box::new(Val::S64(at))));
    s.command_answered(CHOOSE, "a", &[when], Some(interaction))
        .expect("answered")
}

/// **The reader's cart placed**, as the cart's page presses it.
fn place(s: &Server, interaction: &str) -> Answered {
    s.command_answered(PLACE, "a", &[s.shown_total("a")], Some(interaction))
        .expect("answered")
}

/// **Every window offered to session `a`'s reader now**, earliest first, as
/// the page of times shows them day by day: each window's start and its
/// words with its day.
fn offered(s: &Server) -> Vec<(i64, String)> {
    let owner = s.owner("a");
    let times = |day: Option<&str>| {
        let day = Val::Option(day.map(|d| Box::new(Val::String(d.to_string()))));
        s.query(
            "store.page.TimesOffered",
            "a",
            &[Val::String(owner.clone()), day],
        )
        .expect("the times offered")
    };
    let first = times(None);
    let Some(Val::List(days)) = val_at(&first, &["days"]) else {
        panic!("no days: {first:?}")
    };
    let mut out = Vec::new();
    for day in days {
        let Some(Val::String(key)) = val_at(day, &["key"]) else {
            panic!("a day without its key: {day:?}")
        };
        let shown = times(Some(key));
        let Some(Val::List(windows)) = val_at(&shown, &["windows"]) else {
            panic!("no windows: {shown:?}")
        };
        for w in windows {
            match (val_at(w, &["start"]), val_at(w, &["whole"])) {
                (Some(Val::S64(start)), Some(Val::String(whole))) => {
                    out.push((*start, whole.clone()))
                }
                other => panic!("a window is {other:?}"),
            }
        }
    }
    out
}

/// What the checkout says of when, for session `a`'s reader.
fn when_said(s: &Server) -> String {
    let checkout = s
        .query("store.page.Checkout", "a", &[Val::String(s.owner("a"))])
        .expect("the checkout");
    match val_at(&checkout, &["when-said"]) {
        Some(Val::String(said)) => said.clone(),
        other => panic!("the checkout's when is {other:?}"),
    }
}

/// **The reader's order as the layer reads it**: its case, and a scheduled
/// one's time and when it is sent.
fn order(s: &Server) -> Option<(String, Option<(i64, i64)>)> {
    let owner = s.owner("a");
    match s.layer_read("a", "store:data/user-orders#current", &owner) {
        Val::Result(Ok(Some(v))) => match *v {
            Val::Option(None) => None,
            Val::Option(Some(case)) => match *case {
                Val::Variant(case, Some(time)) if case == "scheduled" => {
                    match (val_at(&time, &["at"]), val_at(&time, &["sends"])) {
                        (Some(Val::S64(at)), Some(Val::S64(sends))) => {
                            Some((case, Some((*at, *sends))))
                        }
                        other => panic!("a scheduled order's time is {other:?}"),
                    }
                }
                Val::Variant(case, _) => Some((case, None)),
                other => panic!("user-orders#current answered {other:?}"),
            },
            other => panic!("user-orders#current answered {other:?}"),
        },
        other => panic!("user-orders#current answered {other:?}"),
    }
}

/// **The host's clock, moved `ms` on**, as `/bench/clock` moves it: a run
/// due a duration after its command's commit is due that long after now.
fn later(s: &Server, ms: i64) {
    s.query_clock.0.advance(u64::try_from(ms).expect("forward"));
}

/// Every run the host keeps, earliest first.
fn runs(s: &Server) -> Vec<pw_materialize::ScheduledRun> {
    s.runs().all_runs().expect("the runs read")
}

/// The answer a refusal is, as the page reads it.
fn refused(case: serde_json::Value) -> Option<serde_json::Value> {
    Some(serde_json::json!({ "$case": "err", "value": case }))
}

/// **Open the day round, as the stores are first**: every quarter hour from
/// the first more than an hour ahead to the last a week ahead, said in the
/// store's zone. The control: a quarter of an hour on, the first is gone,
/// within its hour, and one more is a week ahead.
#[test]
fn the_seeds_hours_offer_every_quarter_hour_from_an_hour_to_a_week_ahead() {
    let s = served();
    clock_to(&s, at(2027, 3, 1, 10, 7));
    s.command(ADD, "a", &add_shown("espresso", 1), false)
        .expect("added");
    let times = offered(&s);
    assert_eq!(times.len(), 668, "{:?}", &times[..3]);
    assert_eq!(
        times[0],
        (at(2027, 3, 1, 11, 15), "Today, 11:15 am".to_string())
    );
    assert!(times.windows(2).all(|w| w[1].0 - w[0].0 == 15 * MINUTE));
    assert!(times.contains(&(at(2027, 3, 2, 0, 0), "Tomorrow, 12:00 am".to_string())));
    assert!(times.contains(&(at(2027, 3, 4, 12, 0), "Thursday, 12:00 pm".to_string())));
    assert_eq!(
        times.last().cloned(),
        Some((at(2027, 3, 8, 10, 0), "Next Monday, 10:00 am".to_string()))
    );
    clock_to(&s, at(2027, 3, 1, 10, 15));
    let later = offered(&s);
    assert_eq!(later[0].0, at(2027, 3, 1, 11, 30), "11:15 is an hour away");
    assert_eq!(later.last().map(|t| t.0), Some(at(2027, 3, 8, 10, 15)));
}

/// **A page of times is told when the first window's hour comes**: the
/// windows are compared with the clock (ADR-0304), so the page is held until
/// an hour before the first, and read again then.
#[test]
fn the_page_of_times_is_held_until_the_first_windows_hour() {
    let s = served();
    clock_to(&s, at(2027, 3, 1, 10, 7));
    s.command(ADD, "a", &add_shown("espresso", 1), false)
        .expect("added");
    let (html, _, _, _) = s
        .serve_document_settled("a", "store.page.TimePage~ui", &Params::new(), &[])
        .expect("served");
    assert!(visible(&html).contains("11:15 am"), "{html}");
    let doc = latest(&s.pending.lock().expect("pending"), "a");
    assert_eq!(s.timers.at(&doc), Some(at(2027, 3, 1, 10, 15)));
}

/// **A store open 07:00 to 19:00 is offered the windows its stores are told
/// at while it takes an order**, 40 minutes before each: from 07:00 to 18:40,
/// its last order (a 20-minute cut-off), so the windows from 07:45 to 19:15
/// each day: 07:30, whose preparation begins before it opens, is not offered,
/// nor 19:30, whose preparation begins past its last order; and 19:15, after
/// it closes, is, as an order for as soon as possible at 18:35 is taken. At
/// 18:00 today the one window left is 19:15.
#[test]
fn a_stores_hours_offer_the_windows_it_is_open_at() {
    let s = served();
    set_hours(&s, "47", &daily(7 * 60, 19 * 60));
    clock_to(&s, at(2027, 3, 1, 18, 0));
    s.command(ADD, "a", &add_shown("espresso", 1), false)
        .expect("added");
    let times = offered(&s);
    let starts: Vec<i64> = times.iter().map(|t| t.0).collect();
    assert_eq!(
        times[0],
        (at(2027, 3, 1, 19, 15), "Today, 7:15 pm".to_string()),
        "after it closes, a window its store is told for while it is open"
    );
    assert_eq!(
        times[1],
        (at(2027, 3, 2, 7, 45), "Tomorrow, 7:45 am".to_string())
    );
    assert!(starts.contains(&at(2027, 3, 2, 19, 15)));
    for not in [
        at(2027, 3, 1, 19, 30),
        at(2027, 3, 2, 7, 30),
        at(2027, 3, 2, 19, 30),
        at(2027, 3, 3, 7, 0),
    ] {
        assert!(!starts.contains(&not), "{not}");
    }
    assert_eq!(
        times.last().cloned(),
        Some((at(2027, 3, 8, 18, 0), "Next Monday, 6:00 pm".to_string()))
    );
    assert_eq!(times.len(), 1 + 6 * 47 + 42);
}

/// **Hours past midnight offer the windows across it**: open 22:00 to 02:00
/// with its last order at 01:40, a night's windows are 22:45 to 02:15, the
/// second half the next day's; 22:30 and 02:30 are not.
#[test]
fn hours_past_midnight_offer_windows_across_it() {
    let s = served();
    set_hours(&s, "48", &daily(22 * 60, 2 * 60));
    clock_to(&s, at(2027, 3, 1, 12, 0));
    s.command(ADD, "a", &add_shown("drip", 1), false)
        .expect("added");
    let times = offered(&s);
    for (open, said) in [
        (at(2027, 3, 1, 22, 45), "Today, 10:45 pm"),
        (at(2027, 3, 1, 23, 45), "Today, 11:45 pm"),
        (at(2027, 3, 2, 1, 45), "Tomorrow, 1:45 am"),
        (at(2027, 3, 2, 2, 15), "Tomorrow, 2:15 am"),
    ] {
        assert!(times.contains(&(open, said.to_string())), "{said}");
    }
    let starts: Vec<i64> = times.iter().map(|t| t.0).collect();
    for closed in [at(2027, 3, 2, 2, 30), at(2027, 3, 1, 22, 30)] {
        assert!(!starts.contains(&closed), "{closed}");
    }
    assert_eq!(times.len(), 7 * 15, "fifteen a night, a week of nights");
}

/// **A window is offered where its store takes an order when it is told, on
/// the instant** (the integrator's ruling of 2026-10-15): open 07:00 to 19:00
/// with its last order at 18:30, told 15 minutes before a window (the
/// estimate's most, 10, and five). 07:15 is told as the store opens and is
/// offered; 07:00, told before, is not. 18:30 is told at 18:15 and is offered;
/// 18:45 is told as the last order passes, and is not. And the cart says, at
/// each of those instants, what it says of an order for as soon as possible:
/// taken at 07:00 and 18:15, refused at 06:45 and at 18:30.
#[test]
fn a_window_is_offered_where_its_store_takes_an_order_when_it_is_told() {
    let s = served();
    estimating(&s, 10, 10);
    let mut hours = daily(7 * 60, 19 * 60);
    hours.last_order_minutes = 30;
    set_hours(&s, "47", &hours);
    clock_to(&s, at(2027, 3, 1, 20, 0));
    s.command(ADD, "a", &add_shown("espresso", 1), false)
        .expect("added");
    let starts: Vec<i64> = offered(&s).iter().map(|t| t.0).collect();
    let tuesday: Vec<i64> = starts
        .iter()
        .copied()
        .filter(|t| (at(2027, 3, 2, 0, 0)..at(2027, 3, 3, 0, 0)).contains(t))
        .collect();
    assert_eq!(tuesday.first(), Some(&at(2027, 3, 2, 7, 15)));
    assert_eq!(tuesday.last(), Some(&at(2027, 3, 2, 18, 30)));
    assert_eq!(tuesday.len(), 46);
    for (window, taken) in [
        (at(2027, 3, 2, 7, 0), false),
        (at(2027, 3, 2, 7, 15), true),
        (at(2027, 3, 2, 18, 30), true),
        (at(2027, 3, 2, 18, 45), false),
    ] {
        assert_eq!(starts.contains(&window), taken, "{window}");
        clock_to(&s, window - 15 * MINUTE);
        assert_eq!(
            cart_hours(&s).is_empty(),
            taken,
            "{window}: {}",
            cart_hours(&s)
        );
    }
}

/// **One rule for both**: a store open 07:00 to 11:00 and 14:00 to 19:00 with
/// a cut-off of 25 minutes, its last orders between two quarter hours (10:35,
/// 18:35). Of each quarter hour of a day, a window is offered exactly where
/// the cart says an order for as soon as possible is taken when its stores
/// are told, 40 minutes before: the windows 07:45 to 11:00 and 14:45 to
/// 19:00.
#[test]
fn a_window_is_offered_exactly_where_an_order_for_as_soon_as_possible_is_taken_when_told() {
    let s = served();
    let mut hours = every_day(&[(7 * 60, 11 * 60), (14 * 60, 19 * 60)]);
    hours.last_order_minutes = 25;
    set_hours(&s, "47", &hours);
    clock_to(&s, at(2027, 3, 1, 20, 0));
    s.command(ADD, "a", &add_shown("espresso", 1), false)
        .expect("added");
    let starts: Vec<i64> = offered(&s).iter().map(|t| t.0).collect();
    let tuesday = at(2027, 3, 2, 0, 0);
    let mut taken = 0;
    for quarter in 0..96 {
        let window = tuesday + quarter * 15 * MINUTE;
        clock_to(&s, window - 40 * MINUTE);
        let would = cart_hours(&s).is_empty();
        assert_eq!(
            starts.contains(&window),
            would,
            "{window}: {}",
            cart_hours(&s)
        );
        taken += usize::from(would);
    }
    assert_eq!(taken, 14 + 18);
}

/// **The windows wait for the estimate**: a store open 07:00 to 19:00 (its
/// last order at 18:40) is told the estimate's most and five minutes before a
/// window. No estimate set, 40 minutes: the windows 07:45 to 19:15. An
/// estimate of 20 to 50, 55 minutes: 08:00 to 19:30. An estimator that
/// cannot answer counts for nothing beyond the five: 07:15 to 18:30.
#[test]
fn the_estimate_moves_the_windows() {
    let s = served();
    set_hours(&s, "47", &daily(7 * 60, 19 * 60));
    clock_to(&s, at(2027, 3, 1, 20, 0));
    s.command(ADD, "a", &add_shown("espresso", 1), false)
        .expect("added");
    let tuesday = |s: &Server| {
        let day: Vec<i64> = offered(s)
            .iter()
            .map(|w| w.0)
            .filter(|w| (at(2027, 3, 2, 0, 0)..at(2027, 3, 3, 0, 0)).contains(w))
            .collect();
        (day[0], day[day.len() - 1])
    };
    assert_eq!(tuesday(&s), (at(2027, 3, 2, 7, 45), at(2027, 3, 2, 19, 15)));
    estimating(&s, 20, 50);
    assert_eq!(tuesday(&s), (at(2027, 3, 2, 8, 0), at(2027, 3, 2, 19, 30)));
    no_delay(&s, Some("declared"));
    assert_eq!(tuesday(&s), (at(2027, 3, 2, 7, 15), at(2027, 3, 2, 18, 30)));
}

/// **A time is held to the estimate as it is when the order is placed**: a
/// store open 07:00 to 19:00; 08:00 tomorrow is offered under an estimate of
/// 20 to 50 (its stores told at 07:05) and chosen; the estimate grows to 60
/// to 90 (told at 06:25, before the store opens), and the order is refused by
/// name. The control: the estimate back, the same time is placed, scheduled
/// to be sent at 07:05.
#[test]
fn a_time_is_held_to_the_estimate_as_it_is_when_the_order_is_placed() {
    let s = served();
    set_hours(&s, "47", &daily(7 * 60, 19 * 60));
    clock_to(&s, at(2027, 3, 1, 20, 0));
    s.command(ADD, "a", &add_shown("espresso", 1), false)
        .expect("added");
    estimating(&s, 20, 50);
    let eight = at(2027, 3, 2, 8, 0);
    assert!(choose(&s, Some(eight), "eight").committed);
    estimating(&s, 60, 90);
    let late = place(&s, "late");
    assert!(!late.committed);
    assert_eq!(
        late.result,
        refused(serde_json::json!({ "$case": "not-offered", "value": eight })),
    );
    assert_eq!(order(&s), None, "nothing placed");
    estimating(&s, 20, 50);
    let placed = place(&s, "again");
    assert!(placed.committed, "{:?}", placed.result);
    assert_eq!(
        order(&s),
        Some(("scheduled".into(), Some((eight, eight - 55 * MINUTE))))
    );
}

/// **A cart for as soon as possible asks no estimate**: with the estimator
/// down (a call that fails, not one that refuses), the checkout is shown and
/// the order placed. The control: with a time chosen the checkout asks the
/// estimate its windows wait for, and is unavailable while it is down.
#[test]
fn a_cart_for_as_soon_as_possible_asks_no_estimate() {
    let s = served();
    clock_to(&s, at(2027, 3, 1, 10, 7));
    s.command(ADD, "a", &add_shown("espresso", 1), false)
        .expect("added");
    no_delay(&s, Some("down"));
    assert_eq!(when_said(&s), "As soon as possible");
    let soon = place(&s, "soon");
    assert!(soon.committed, "{:?}", soon.result);

    let s = served();
    clock_to(&s, at(2027, 3, 1, 10, 7));
    s.command(ADD, "a", &add_shown("espresso", 1), false)
        .expect("added");
    assert!(choose(&s, Some(at(2027, 3, 1, 12, 0)), "noon").committed);
    no_delay(&s, Some("down"));
    let owner = s.owner("a");
    assert!(
        s.query("store.page.Checkout", "a", &[Val::String(owner)])
            .is_err(),
        "a time chosen asks the estimate"
    );
    no_delay(&s, None);
    assert_eq!(when_said(&s), "Scheduled for Mon 12:00 pm");
}

/// The cart's two stores: Blue Bottle (47) open 07:00 to 19:00 in San
/// Francisco, Harbor Coffee (48) 09:00 to 17:00 in New York, which is 06:00
/// to 14:00 in San Francisco.
fn two_zones(s: &Server) {
    set_hours(s, "47", &daily(7 * 60, 19 * 60));
    let mut new_york = daily(9 * 60, 17 * 60);
    new_york.zone = "America/New_York".to_string();
    set_hours(s, "48", &new_york);
    clock_to(s, at(2027, 3, 1, 20, 0));
}

/// **A cart of two stores in two zones is offered the windows both take an
/// order for when they are told**, each by its hours in its own zone: from
/// 07:00 to 13:40 in San Francisco (Harbor Coffee's last order, 16:40 in New
/// York), so the windows 07:45 to 14:15, said in the zone of the store the
/// cart lists first. The control: the other store listed first says the same
/// windows in New York's.
#[test]
fn a_cart_across_two_zones_is_offered_the_windows_both_are_open_at() {
    let s = served();
    two_zones(&s);
    s.command(ADD, "a", &add_shown("espresso", 1), false)
        .expect("added");
    s.command(ADD, "a", &add_shown("drip", 1), false)
        .expect("added");
    let times = offered(&s);
    assert_eq!(
        times[0],
        (at(2027, 3, 2, 7, 45), "Tomorrow, 7:45 am".to_string())
    );
    assert!(times.contains(&(at(2027, 3, 2, 14, 15), "Tomorrow, 2:15 pm".to_string())));
    let starts: Vec<i64> = times.iter().map(|t| t.0).collect();
    for closed in [at(2027, 3, 2, 14, 30), at(2027, 3, 2, 7, 30)] {
        assert!(!starts.contains(&closed), "{closed}");
    }
    assert_eq!(times.len(), 7 * 27);
    s.command("store.page.clear_cart", "a", &[], false)
        .expect("cleared");
    s.command(ADD, "a", &add_shown("drip", 1), false)
        .expect("added");
    s.command(ADD, "a", &add_shown("espresso", 1), false)
        .expect("added");
    let said_in_new_york = offered(&s);
    assert_eq!(
        said_in_new_york[0],
        (at(2027, 3, 2, 7, 45), "Tomorrow, 10:45 am".to_string())
    );
    assert_eq!(said_in_new_york.len(), times.len());
}

/// **A time is held to the times offered when it is chosen**: one within the
/// hour, one between two quarter hours, one more than a week ahead, one whose
/// store is told before it opens and one past its last order are each refused
/// by name, and nothing is chosen. The control: an offered one is chosen, said
/// at checkout, and as soon as possible chosen again after.
#[test]
fn a_time_not_offered_is_refused_when_chosen() {
    let s = served();
    set_hours(&s, "47", &daily(7 * 60, 19 * 60));
    clock_to(&s, at(2027, 3, 1, 10, 7));
    s.command(ADD, "a", &add_shown("espresso", 1), false)
        .expect("added");
    assert_eq!(when_said(&s), "As soon as possible");
    for (i, not) in [
        at(2027, 3, 1, 11, 0),
        at(2027, 3, 1, 12, 7),
        at(2027, 3, 8, 10, 15),
        at(2027, 3, 2, 7, 30),
        at(2027, 3, 1, 19, 30),
    ]
    .into_iter()
    .enumerate()
    {
        let answered = choose(&s, Some(not), &format!("not-{i}"));
        assert!(!answered.committed, "{not}");
        assert_eq!(
            answered.result,
            refused(serde_json::json!({ "$case": "not-offered", "value": not })),
        );
    }
    assert_eq!(when_said(&s), "As soon as possible", "nothing chosen");
    let chosen = choose(&s, Some(at(2027, 3, 1, 12, 0)), "offered");
    assert!(chosen.committed, "{:?}", chosen.result);
    assert_eq!(when_said(&s), "Scheduled for Mon 12:00 pm");
    assert!(
        page(&s, "a", "/cart").contains("Scheduled for Mon 12:00 pm Choose a time"),
        "the cart says it"
    );
    let soon = choose(&s, None, "soon");
    assert!(soon.committed, "{:?}", soon.result);
    assert_eq!(when_said(&s), "As soon as possible");
}

/// **A time no longer offered is refused when the order is placed**, by
/// name: chosen at 10:07 for 12:00, it is an hour away at 11:00, and the
/// checkout says so; the times offered now begin at 12:15. The control: at
/// 10:59 it is placed, scheduled for 12:00.
#[test]
fn a_time_no_longer_offered_is_refused_when_the_order_is_placed() {
    let s = served();
    clock_to(&s, at(2027, 3, 1, 10, 7));
    s.command(ADD, "a", &add_shown("espresso", 1), false)
        .expect("added");
    let noon = at(2027, 3, 1, 12, 0);
    assert!(choose(&s, Some(noon), "noon").committed);
    clock_to(&s, at(2027, 3, 1, 11, 0));
    let late = place(&s, "late");
    assert!(!late.committed);
    assert_eq!(
        late.result,
        refused(serde_json::json!({ "$case": "not-offered", "value": noon })),
    );
    assert_eq!(
        when_said(&s),
        "Mon 12:00 pm is no longer offered: choose another time."
    );
    assert_eq!(offered(&s)[0].0, at(2027, 3, 1, 12, 15));
    assert_eq!(order(&s), None, "nothing placed");

    let s = served();
    clock_to(&s, at(2027, 3, 1, 10, 7));
    s.command(ADD, "a", &add_shown("espresso", 1), false)
        .expect("added");
    assert!(choose(&s, Some(noon), "noon").committed);
    clock_to(&s, at(2027, 3, 1, 10, 59));
    let placed = place(&s, "in-time");
    assert!(placed.committed, "{:?}", placed.result);
    assert_eq!(order(&s).map(|(case, _)| case), Some("scheduled".into()));
}

/// **A time a store's hours no longer cover is refused**: chosen for 18:00,
/// the store's hours then end at 17:00, its stores told at 17:20. The control:
/// the same time placed while the hours covered it (above).
#[test]
fn a_time_the_hours_no_longer_cover_is_refused() {
    let s = served();
    clock_to(&s, at(2027, 3, 1, 10, 7));
    s.command(ADD, "a", &add_shown("espresso", 1), false)
        .expect("added");
    let six = at(2027, 3, 1, 18, 0);
    assert!(choose(&s, Some(six), "six").committed);
    set_hours(&s, "47", &daily(7 * 60, 17 * 60));
    let refused_now = place(&s, "place");
    assert_eq!(
        refused_now.result,
        refused(serde_json::json!({ "$case": "not-offered", "value": six })),
    );
}

/// **An order for a time is taken while its store is closed now**, for a
/// time it is open: at 20:00, the store closed until 07:00, as soon as
/// possible is refused `Closed`, and tomorrow at 09:00 is placed, scheduled,
/// paid at once where the deployment takes no payments.
#[test]
fn an_order_for_a_time_is_taken_while_its_store_is_closed() {
    let s = served();
    set_hours(&s, "47", &daily(7 * 60, 19 * 60));
    clock_to(&s, at(2027, 3, 1, 20, 0));
    s.command(ADD, "a", &add_shown("espresso", 1), false)
        .expect("added");
    let soon = place(&s, "soon");
    assert_eq!(
        soon.result,
        refused(serde_json::json!({ "$case": "closed",
            "value": { "$case": "closed-now", "value": { "$case": "some",
                "value": { "day": { "$case": "tomorrow" }, "at": 420 } } } })),
    );
    let nine = at(2027, 3, 2, 9, 0);
    assert!(choose(&s, Some(nine), "nine").committed);
    let placed = place(&s, "nine");
    assert!(placed.committed, "{:?}", placed.result);
    let (case, time) = order(&s).expect("an order");
    assert_eq!(case, "scheduled");
    assert_eq!(time.map(|t| t.0), Some(nine));
    assert!(s.cart_lines("a").is_empty(), "the cart is the order's");
    assert_eq!(when_said(&s), "As soon as possible", "the choice is spent");
}

/// **An order is sent to its stores when its preparation is due, and not
/// before**: the window less the estimate's most, 35 minutes (25 to 35, no
/// address chosen), and five more. Paid at the provider, it is scheduled and
/// its sweep cancelled; its run is due at 11:20 for 12:00, the reader's; at
/// 11:19:59 nothing is due, and at 11:20 it is placed, and the reader's open
/// order page is told, once.
#[test]
fn an_order_is_sent_to_its_stores_when_its_preparation_is_due() {
    paying(|s, _| {
        clock_to(s, at(2027, 3, 1, 10, 7));
        s.command(ADD, "a", &add_shown("espresso", 1), false)
            .expect("added");
        let noon = at(2027, 3, 1, 12, 0);
        assert!(choose(s, Some(noon), "noon").committed);
        assert!(place(s, "place").committed);
        let Some((case, None)) = order(s) else {
            panic!("{:?}", order(s))
        };
        assert_eq!(case, "awaiting-payment");
        assert_eq!(
            runs(s)
                .iter()
                .map(|r| r.command.as_str())
                .collect::<Vec<_>>(),
            [SWEEP],
            "told nothing until paid"
        );
        let (html, _, _, _) = s
            .serve_document_settled("a", "store.page.OrderPage~ui", &Params::new(), &[])
            .expect("served");
        let doc = latest(&s.pending.lock().expect("pending"), "a");
        let page_url = html
            .split("src=\"")
            .nth(1)
            .and_then(|rest| rest.split('"').next())
            .expect("the provider's page")
            .to_string();
        let paid = pay(&page_url, "4242424242424242");
        assert!(paid.contains("Paid."), "{paid}");
        let sends = noon - 40 * MINUTE;
        assert_eq!(order(s), Some(("scheduled".into(), Some((noon, sends)))));
        let pending = runs(s);
        assert_eq!(pending.len(), 1, "{pending:?}");
        assert_eq!(pending[0].command, SEND);
        assert_eq!(pending[0].due_ms, sends);
        assert_eq!(pending[0].user, s.identity.principals().user_of("a"));
        // The open page is told it is scheduled once the webhook's
        // connection is answered (ADR-0219), on that connection: waited for
        // here, so that telling is done before the clock moves.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !sets_of(s, &doc)
            .iter()
            .any(|set| visible(&written(set)).contains("Scheduled for Mon 12:00 pm."))
        {
            assert!(
                std::time::Instant::now() < deadline,
                "the open page was not told: {:?}",
                sets_of(s, &doc)
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        clock_to(s, sends - 1000);
        assert_eq!(s.run_due().0, 0, "a second before");
        assert_eq!(order(s).map(|(case, _)| case), Some("scheduled".into()));
        clock_to(s, sends);
        assert_eq!(s.run_due().0, 1, "due");
        assert_eq!(order(s), Some(("placed".into(), None)));
        assert!(runs(s).is_empty(), "run once");
        // The run's commit is told by whichever thread takes it from the
        // queue (`tell_waiting`): this one at `run_due`'s end, or a
        // connection's that was answered meanwhile (the provider's webhook,
        // `handle`, ADR-0219), which then tells it a moment after `run_due`
        // returns. Waited for, as the scheduling's telling is above (PR #67's
        // verify, at 3fe00518, read it before the connection's thread had).
        let placed = |s: &Server| {
            sets_of(s, &doc)
                .iter()
                .map(written)
                .filter(|set| set.contains("Placed: the store has your order."))
                .count()
        };
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while placed(s) == 0 {
            assert!(
                std::time::Instant::now() < deadline,
                "the open page was not told it was placed: {:?}",
                sets_of(s, &doc)
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let told: Vec<String> = sets_of(s, &doc).iter().map(written).collect();
        assert_eq!(
            told.iter()
                .filter(|set| set.contains("Placed: the store has your order."))
                .count(),
            1,
            "{told:?}"
        );
        clock_to(s, noon);
        assert_eq!(s.run_due().0, 0, "nothing more");
    });
}

/// **The store is told the window less the estimate's most and five
/// minutes**: the reader's estimate 20 to 50 minutes sends the order 55
/// minutes before its window, where the deployment takes no payments. The
/// control: no estimate set, 40 minutes (above).
#[test]
fn an_order_is_sent_its_estimates_most_and_five_minutes_before() {
    let s = served();
    clock_to(&s, at(2027, 3, 1, 10, 7));
    s.untold(
        "a",
        "store:host/estimates#set",
        &[
            Val::String(crate::identity::session_handle("a")),
            Val::S64(20),
            Val::Option(Some(Box::new(Val::S64(50)))),
        ],
    );
    s.command(ADD, "a", &add_shown("espresso", 1), false)
        .expect("added");
    let noon = at(2027, 3, 1, 12, 0);
    assert!(choose(&s, Some(noon), "noon").committed);
    assert!(place(&s, "place").committed);
    let sends = noon - 55 * MINUTE;
    assert_eq!(order(&s), Some(("scheduled".into(), Some((noon, sends)))));
    assert_eq!(runs(&s).first().map(|r| r.due_ms), Some(sends));
}

/// **An order for a time awaiting its payment is swept as any is**
/// (ADR-0334): unpaid thirty minutes on, it is taken back, and nothing was
/// scheduled to send it. The control: one paid is sent (above); and the
/// reader's next order for a time, paid, is scheduled for its own time, the
/// one taken back's replaced.
#[test]
fn an_order_for_a_time_awaiting_its_payment_is_swept() {
    paying(|s, _| {
        clock_to(s, at(2027, 3, 1, 10, 7));
        s.command(ADD, "a", &add_shown("espresso", 1), false)
            .expect("added");
        assert!(choose(s, Some(at(2027, 3, 1, 15, 0)), "three").committed);
        assert!(place(s, "place").committed);
        later(s, 30 * MINUTE);
        assert_eq!(s.run_due().0, 1, "the sweep");
        assert_eq!(order(s), Some(("canceled".into(), None)));
        assert!(runs(s).is_empty(), "{:?}", runs(s));
        assert_eq!(s.cart_lines("a"), vec![("espresso".to_string(), 1)]);
        let four = at(2027, 3, 1, 16, 0);
        assert!(choose(s, Some(four), "four").committed);
        assert!(place(s, "again").committed);
        let (html, _, _, _) = s
            .serve_document_settled("a", "store.page.OrderPage~ui", &Params::new(), &[])
            .expect("served");
        let page_url = html
            .split("src=\"")
            .nth(1)
            .and_then(|rest| rest.split('"').next())
            .expect("the provider's page")
            .to_string();
        assert!(pay(&page_url, "4242424242424242").contains("Paid."));
        assert_eq!(
            order(s),
            Some(("scheduled".into(), Some((four, four - 40 * MINUTE))))
        );
    });
}

/// **A second order waits for a scheduled one to be sent**: the layers hold
/// one current order a reader, which a second would replace before it is
/// sent. Refused by name; the control, once sent, another is placed.
#[test]
fn a_second_order_waits_for_a_scheduled_one_to_be_sent() {
    let s = served();
    clock_to(&s, at(2027, 3, 1, 10, 7));
    s.command(ADD, "a", &add_shown("espresso", 1), false)
        .expect("added");
    let noon = at(2027, 3, 1, 12, 0);
    assert!(choose(&s, Some(noon), "noon").committed);
    assert!(place(&s, "first").committed);
    s.command(ADD, "a", &add_shown("cortado", 1), false)
        .expect("added");
    let second = place(&s, "second");
    assert!(!second.committed);
    assert_eq!(
        second.result,
        refused(serde_json::json!({ "$case": "scheduled" }))
    );
    assert_eq!(s.cart_lines("a"), vec![("cortado".to_string(), 1)]);
    assert!(
        page(&s, "a", "/order").contains("Scheduled for Mon 12:00 pm."),
        "the first stands"
    );
    clock_to(&s, noon - 40 * MINUTE);
    assert_eq!(s.run_due().0, 1);
    let third = place(&s, "third");
    assert!(third.committed, "{:?}", third.result);
    // For as soon as possible, it is placed: the first order's time, kept
    // in the layer, is that order's alone.
    assert_eq!(order(&s), Some(("placed".into(), None)));
}

/// **A cart of two stores is sent by the longer of their estimates**: the
/// reader's address at Union Square, where Blue Bottle (47) is and Harbor
/// Coffee (48) is a courier's minutes away. The cart lists 48's line first,
/// so the last store's estimate, 47's, is the shorter.
#[test]
fn a_cart_of_two_stores_is_sent_by_its_longest_estimate() {
    let s = served();
    clock_to(&s, at(2027, 3, 1, 10, 7));
    let union = crate::places::place("union-square").expect("a place");
    s.command(
        "store.page.add_address",
        "a",
        &[Val::String("Home".into()), Val::String(union.id.into())],
        false,
    )
    .expect("saved");
    let travel = |store: &str| crate::places::travel_minutes(&crate::store::zone_of(store), union);
    assert!(travel("48") > travel("47"), "48 is farther");
    s.command(ADD, "a", &add_shown("drip", 1), false)
        .expect("added");
    s.command(ADD, "a", &add_shown("espresso", 1), false)
        .expect("added");
    let noon = at(2027, 3, 1, 12, 0);
    assert!(choose(&s, Some(noon), "noon").committed);
    assert!(place(&s, "place").committed);
    let sends = noon - (35 + travel("48") + 5) * MINUTE;
    assert_eq!(order(&s), Some(("scheduled".into(), Some((noon, sends)))));
}

/// **A send run before its order is paid places nothing**: the order for a
/// time awaits its payment, and a send of it finds it not scheduled, and
/// leaves it awaiting. The control: paid, its own send places it (above).
#[test]
fn a_send_before_its_order_is_paid_places_nothing() {
    paying_with(
        |app| format!("{app}{SEND_IN_FIVE}"),
        |s, _| {
            clock_to(s, at(2027, 3, 1, 10, 7));
            s.command(ADD, "a", &add_shown("espresso", 1), false)
                .expect("added");
            assert!(choose(s, Some(at(2027, 3, 1, 15, 0)), "three").committed);
            assert!(place(s, "place").committed);
            let awaited = s.layer_read("a", "store:data/user-orders#awaited", &s.owner("a"));
            let reference = match &awaited {
                Val::Option(Some(a)) => match val_at(a, &["order"]) {
                    Some(Val::String(order)) => order.clone(),
                    other => panic!("an awaited order's reference is {other:?}"),
                },
                other => panic!("user-orders#awaited answered {other:?}"),
            };
            let early = s
                .command_answered(
                    "store.page.send_in_five",
                    "a",
                    &[Val::String(reference)],
                    Some("early"),
                )
                .expect("answered");
            assert!(early.committed, "{:?}", early.result);
            later(s, 5 * MINUTE);
            assert_eq!(s.run_due().0, 1, "the send, run");
            assert_eq!(
                order(s).map(|(case, _)| case),
                Some("awaiting-payment".into())
            );
        },
    );
}

/// A test's: the store's send of `order`, scheduled five minutes on.
const SEND_IN_FIVE: &str = r#"
// A test's: `send_to_store(order)` scheduled five minutes on.
command send_in_five(order: String)
    -> Result<Int, CartError>
    requires      Anyone
    idempotent_by InteractionId
{
    schedule send_to_store(order) after minutes(5)
    Ok(1)
}
"#;

/// **A send changes only the order it was scheduled for**: one for another
/// order's reference leaves the reader's scheduled order as it is. The
/// control: its own, when due, places it.
#[test]
fn a_send_places_only_its_own_order() {
    let s = served_from_patches_in("examples", |app| format!("{app}{SEND_IN_FIVE}"), &[]);
    clock_to(&s, at(2027, 3, 1, 10, 7));
    s.command(ADD, "a", &add_shown("espresso", 1), false)
        .expect("added");
    let noon = at(2027, 3, 1, 12, 0);
    assert!(choose(&s, Some(noon), "noon").committed);
    assert!(place(&s, "place").committed);
    let other = s
        .command_answered(
            "store.page.send_in_five",
            "a",
            &[Val::String("o-another".into())],
            Some("other"),
        )
        .expect("answered");
    assert!(other.committed, "{:?}", other.result);
    later(&s, 5 * MINUTE);
    assert_eq!(s.run_due().0, 1, "another order's send");
    assert_eq!(order(&s).map(|(case, _)| case), Some("scheduled".into()));
    clock_to(&s, noon - 40 * MINUTE);
    assert_eq!(s.run_due().0, 1, "its own");
    assert_eq!(order(&s).map(|(case, _)| case), Some("placed".into()));
}

/// **A reader's press of the send is a command that is not there**: what the
/// clock runs, pressed, places nothing. The control: the same route takes a
/// command a reader presses.
#[test]
fn a_readers_press_of_the_send_is_not_a_command() {
    let s = served();
    clock_to(&s, at(2027, 3, 1, 10, 7));
    s.command(ADD, "a", &add_shown("espresso", 1), false)
        .expect("added");
    assert!(choose(&s, Some(at(2027, 3, 1, 12, 0)), "noon").committed);
    assert!(place(&s, "place").committed);
    let forged = posted(
        &s,
        "/command/store.page.send_to_store",
        "a",
        "forged-1",
        r#"["o-0000000000000000"]"#,
    );
    assert!(forged.starts_with("HTTP/1.1 404"), "{forged}");
    assert_eq!(order(&s).map(|(case, _)| case), Some("scheduled".into()));
    let control = posted(&s, "/command/store.page.clear_cart", "a", "control-1", "[]");
    assert!(control.contains("\"committed\":true"), "{control}");
}

/// **The time chosen is the reader's, kept with the cart** in the layer, as
/// their tip is, and read back as it was set; as soon as possible is none.
#[test]
fn the_time_chosen_is_kept_with_the_readers_choices() {
    let s = served();
    clock_to(&s, at(2027, 3, 1, 10, 7));
    s.command(ADD, "a", &add_shown("espresso", 1), false)
        .expect("added");
    let chosen = |s: &Server| {
        let read = s.layer_read("a", "store:data/checkout#chosen", &s.owner("a"));
        val_at(&read, &["when"]).cloned()
    };
    assert_eq!(chosen(&s), Some(Val::Option(None)));
    let noon = at(2027, 3, 1, 12, 0);
    assert!(choose(&s, Some(noon), "noon").committed);
    assert_eq!(
        chosen(&s),
        Some(Val::Option(Some(Box::new(Val::S64(noon)))))
    );
    assert!(
        s.command(
            "store.page.choose_tip",
            "a",
            &[Val::Variant("fifteen".into(), None)],
            false
        )
        .is_ok()
    );
    assert_eq!(
        chosen(&s),
        Some(Val::Option(Some(Box::new(Val::S64(noon))))),
        "a tip keeps it"
    );
    assert!(choose(&s, None, "soon").committed);
    assert_eq!(chosen(&s), Some(Val::Option(None)));
}

/// **The page of times chooses**: its first window pressed as a browser
/// presses it, through the route a page's press reaches; the cart says the
/// time. Its control: as soon as possible pressed after.
#[test]
fn the_page_of_times_lists_a_days_windows_and_its_days() {
    let s = served();
    set_hours(&s, "47", &daily(7 * 60, 19 * 60));
    clock_to(&s, at(2027, 3, 1, 20, 0));
    s.command(ADD, "a", &add_shown("espresso", 1), false)
        .expect("added");
    let shown = page(&s, "a", "/cart/when");
    assert!(
        shown.contains("Tomorrow Wednesday Thursday Friday Saturday Sunday Next Monday"),
        "{shown}"
    );
    assert!(shown.contains("Tomorrow 7:45 am 8:00 am"), "{shown}");
    let friday = (pw_time::days(2027, 3, 5)).to_string();
    let shown = page(&s, "a", &format!("/cart/when?day={friday}"));
    assert!(shown.contains("Friday 7:45 am"), "{shown}");
    assert!(!shown.contains("Tomorrow 7:45 am"), "{shown}");
    let pressed = posted(
        &s,
        "/command/store.page.choose_time",
        "a",
        "pressed-1",
        &format!(r#"[{{"$case":"some","value":{}}}]"#, at(2027, 3, 5, 7, 45)),
    );
    assert!(pressed.contains("\"committed\":true"), "{pressed}");
    assert!(
        page(&s, "a", "/cart").contains("Scheduled for Fri 7:45 am"),
        "the cart says it"
    );
}

/// **An order for a time is placed where the deployment takes payments,
/// awaiting its payment as any is** (ADR-0332), the payment for its whole
/// total; it is the cart's lines, and the cart is empty after. Paid, the
/// order page says when, and that its total was charged.
#[test]
fn an_order_for_a_time_is_paid_now_and_its_page_says_when() {
    paying_with(
        |app| app.to_string(),
        |s, _| {
            clock_to(s, at(2027, 3, 1, 10, 7));
            s.command(ADD, "a", &add_shown("espresso", 2), false)
                .expect("added");
            assert!(choose(s, Some(at(2027, 3, 2, 18, 30)), "evening").committed);
            assert!(place(s, "place").committed);
            assert!(s.cart_lines("a").is_empty());
            let (html, _, _, _) = s
                .serve_document_settled("a", "store.page.OrderPage~ui", &Params::new(), &[])
                .expect("served");
            assert!(visible(&html).contains("Awaiting your payment"), "{html}");
            let page_url = html
                .split("src=\"")
                .nth(1)
                .and_then(|rest| rest.split('"').next())
                .expect("the provider's page")
                .to_string();
            assert!(pay(&page_url, "4242424242424242").contains("Paid."));
            let shown = page(s, "a", "/order");
            assert!(shown.contains("Scheduled for Tue 6:30 pm."), "{shown}");
            assert!(shown.contains("Total charged: $"), "{shown}");
        },
    );
}

/// ADR-0348: the set operation preserves each keyed result, including its
/// refusal, in request order. No per-element reads hide below the seam:
/// bounded_statements holds its total at three at both database sizes.
#[test]
fn estimates_read_together_preserve_each_stores_result() {
    let s = served();
    estimating(&s, 21, 43);
    s.command(
        "store.page.add_address",
        "a",
        &[
            Val::String("Home".into()),
            Val::String("union-square".into()),
        ],
        false,
    )
    .expect("saved");
    let owner = s.owner("a");
    fn without_time(value: &Val) -> String {
        let mut value = value.clone();
        if let Val::Result(Ok(Some(estimate))) = &mut value
            && let Val::Record(fields) = &mut **estimate
        {
            fields.retain(|(key, _)| key != "generated-at");
        }
        format!("{value:?}")
    }
    for failure in [None, Some("declared")] {
        no_delay(&s, failure);
        let ops = s.data.reads("a", None);
        let keys = ["48", "47", "unknown", "48"];
        let args = [
            Val::List(keys.iter().map(|k| Val::String((*k).into())).collect()),
            Val::String(owner.clone()),
        ];
        let answer = ops["store:data/user-estimates#for-stores"](&args).expect("set read");
        let [Val::List(values)] = answer.as_slice() else {
            panic!("{answer:?}")
        };
        assert_eq!(values.len(), keys.len());
        for (key, value) in keys.iter().zip(values) {
            assert!(matches!(val_at(value, &["store"]), Some(Val::String(id)) if id == key));
            let keyed = ops["store:data/user-estimates#for-store"](&[
                Val::String((*key).into()),
                Val::String(owner.clone()),
            ])
            .expect("keyed read");
            let estimate = val_at(value, &["estimate"]).expect("keyed result");
            assert_eq!(without_time(estimate), without_time(&keyed[0]), "{key}");
        }
    }
    no_delay(&s, Some("host"));
    let ops = s.data.reads("a", None);
    let args = [
        Val::List(vec![Val::String("47".into())]),
        Val::String(owner.clone()),
    ];
    assert!(ops["store:data/user-estimates#for-stores"](&args).is_err());
    let empty = [Val::List(Vec::new()), Val::String(owner.clone())];
    let answer =
        ops["store:data/user-estimates#for-stores"](&empty).expect("empty reads no estimator");
    assert!(matches!(answer.as_slice(), [Val::List(values)] if values.is_empty()));
    assert!(
        ops["store:data/user-estimates#for-stores"](&[
            Val::List(vec![Val::S64(1)]),
            Val::String(owner)
        ])
        .is_err()
    );
}
