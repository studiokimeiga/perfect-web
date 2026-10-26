//! **A store's opening hours, served** (track `store-hours`, W9): whether a
//! store is open, read by comparing the platform's clock (ADR-0304), shown on
//! its page, the home page, a search and the cart, and an order refused while
//! a store of the cart is closed or past its last order. On the layer the
//! server's tests run on: memory, or PostgreSQL under
//! `PW_STORE_TEST_LAYER=postgres`. Each test moves the host's clock to a day
//! ahead, in San Francisco's zone, the stores'.

use super::*;
use crate::hours::{Hours, Period, Special};

const ADD: &str = "store.page.add_to_cart";
const PLACE: &str = "store.page.place_order";

fn served() -> Served {
    served_from_patches_in("examples", |app| app.to_string(), &[])
}

/// San Francisco's zone, from the host's database.
fn sf() -> std::sync::Arc<pw_time::Zone> {
    pw_time::Database::system()
        .zone("America/Los_Angeles")
        .expect("the zone")
}

/// The instant, in milliseconds, of a wall-clock minute in San Francisco.
pub(super) fn at(y: i64, m: u32, d: u32, hour: i64, minute: i64) -> i64 {
    sf().instant(pw_time::days(y, m, d), (hour * 60 + minute) * 60) * 1000
}

/// The host's clock moved forward to `to`.
pub(super) fn clock_to(s: &Server, to: i64) {
    let now = s.now();
    assert!(to >= now, "a test's clock moves forward");
    s.query_clock
        .0
        .advance(u64::try_from(to - now).expect("forward"));
}

/// Every day `opens` to `closes`, in minutes.
pub(super) fn daily(opens: i64, closes: i64) -> Hours {
    Hours {
        zone: "America/Los_Angeles".to_string(),
        last_order_minutes: 20,
        weekly: (1..=7)
            .map(|weekday| Period {
                weekday,
                opens,
                closes,
            })
            .collect(),
        special: Vec::new(),
    }
}

/// A store's hours set, written with the `StoreChanged` that drops what read
/// them.
pub(super) fn set_hours(s: &Server, store: &str, hours: &Hours) {
    s.store_write(
        "",
        "store:host/hours#set",
        &[
            Val::String(store.to_string()),
            crate::hours::hours_val(hours),
        ],
        &[(
            "Events.StoreChanged".to_string(),
            vec![Val::String(store.to_string())],
        )],
        |delivered| {
            s.invalidate_queries("", &[], delivered);
        },
    )
    .expect("set");
}

/// What a page shows `session`, as text.
pub(super) fn page(s: &Server, session: &str, path: &str) -> String {
    let whole: String = fetched_as(s, path, Some(session))
        .into_iter()
        .map(|(_, c)| c)
        .collect();
    visible(&whole)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// **A page is told when the store closes** (ADR-0304): the store's page
/// read at 18:59:30, open until 19:00 and held to it; nothing is due a second
/// before, and past it the page is sent "Closed".
#[test]
fn a_store_page_is_told_at_the_closing_instant() {
    let s = served();
    set_hours(&s, "47", &daily(7 * 60, 19 * 60));
    let closing = at(2027, 3, 1, 19, 0);
    clock_to(&s, closing - 30_000);
    let (html, _, _, _) = s
        .serve_document_settled("a", "store.page.StorePage~ui", &store_params("47"), &[])
        .expect("served");
    assert!(visible(&html).contains("Open until 7:00 pm"), "{html}");
    let doc = latest(&s.pending.lock().expect("pending"), "a");
    assert_eq!(s.timers.at(&doc), Some(closing), "held until 19:00");
    let before = sets_of(&s, &doc).len();
    s.query_clock.0.advance(29_000);
    assert_eq!(s.tell_due().0, 0, "18:59:59: nothing is due");
    s.query_clock.0.advance(2_000);
    assert_eq!(s.tell_due().0, 1, "19:00:01: the page is due");
    let told = format!("{:?}", &sets_of(&s, &doc)[before..]);
    assert!(
        told.contains("Closed · opens tomorrow at 7:00 am"),
        "{told}"
    );
}

/// **An order is refused past the last order, and while closed**: the
/// cut-off 20 minutes before 19:00. Adding to a cart while closed is
/// allowed: that is browsing.
#[test]
fn an_order_is_refused_past_the_last_order_and_while_closed() {
    let s = served();
    set_hours(&s, "47", &daily(7 * 60, 19 * 60));
    clock_to(&s, at(2027, 3, 1, 18, 39));
    s.command(ADD, "a", &add_shown("espresso", 1), false)
        .expect("added");
    let placed = s
        .command_answered(PLACE, "a", &[s.shown_total("a")], None)
        .expect("answered");
    assert!(placed.committed, "18:39: before the last order");
    s.command(ADD, "a", &add_shown("espresso", 1), false)
        .expect("added");
    clock_to(&s, at(2027, 3, 1, 18, 41));
    let refused = s
        .command_answered(PLACE, "a", &[s.shown_total("a")], None)
        .expect("answered");
    assert!(!refused.committed, "18:41: past the last order");
    // Data the page words (the integrator's ruling of 2026-10-12): past
    // the last order, which was at 18:40, minute 1120.
    assert_eq!(
        refused.result,
        Some(
            serde_json::json!({ "$case": "err", "value": { "$case": "closed",
            "value": { "$case": "past-last-order", "value": 1120 } } })
        ),
    );
    clock_to(&s, at(2027, 3, 1, 20, 0));
    s.command(ADD, "a", &add_shown("cortado", 1), false)
        .expect("added while closed: browsing");
    let refused = s
        .command_answered(PLACE, "a", &[s.shown_total("a")], None)
        .expect("answered");
    assert!(!refused.committed);
    // Closed, and it opens tomorrow at 07:00, minute 420.
    assert_eq!(
        refused.result,
        Some(
            serde_json::json!({ "$case": "err", "value": { "$case": "closed",
            "value": { "$case": "closed-now", "value": { "$case": "some",
                "value": { "day": { "$case": "tomorrow" }, "at": 420 } } } } })
        ),
    );
    // And the cart says so before the press.
    assert!(
        page(&s, "a", "/cart").contains("Blue Bottle is closed: it opens tomorrow at 7:00 am."),
        "the cart"
    );
}

/// **Hours that run past midnight**: 22:00 to 02:00, open at 01:00 from the
/// day before, closed at 03:00 until 22:00.
#[test]
fn hours_run_past_midnight() {
    let s = served();
    set_hours(&s, "48", &daily(22 * 60, 2 * 60));
    clock_to(&s, at(2027, 3, 2, 1, 0));
    assert!(
        page(&s, "a", "/stores/48").contains("Open until 2:00 am"),
        "01:00"
    );
    clock_to(&s, at(2027, 3, 2, 3, 0));
    assert!(
        page(&s, "b", "/stores/48").contains("Closed · opens today at 10:00 pm"),
        "03:00"
    );
}

/// **A special date closed**: the weekday's hours give way to it.
#[test]
fn a_special_date_closes_the_store() {
    let s = served();
    let mut hours = daily(7 * 60, 19 * 60);
    hours.special.push(Special {
        date: pw_time::days(2027, 3, 3),
        closed: true,
        opens: 0,
        closes: 0,
    });
    set_hours(&s, "47", &hours);
    clock_to(&s, at(2027, 3, 3, 10, 0));
    assert!(
        page(&s, "a", "/stores/47").contains("Closed · opens tomorrow at 7:00 am"),
        "the special date"
    );
    clock_to(&s, at(2027, 3, 4, 10, 0));
    assert!(
        page(&s, "b", "/stores/47").contains("Open until 7:00 pm"),
        "the next day"
    );
}

/// **A day the clocks go forward** (2027-03-14 in San Francisco, 02:00 to
/// 03:00): a store open 01:00 to 03:00 closes at 03:00 PDT, half an hour
/// after 01:30 PST; one that opens at 02:30, a time the day skips, opens at
/// 03:30 PDT, as RFC 5545 §3.3.5 reads a skipped time.
#[test]
fn a_day_the_clocks_go_forward() {
    let s = served();
    set_hours(&s, "47", &daily(60, 3 * 60));
    let now = at(2027, 3, 14, 1, 30);
    clock_to(&s, now);
    let (html, _, _, _) = s
        .serve_document_settled("a", "store.page.StorePage~ui", &store_params("47"), &[])
        .expect("served");
    assert!(visible(&html).contains("Open until 3:00 am"), "{html}");
    let doc = latest(&s.pending.lock().expect("pending"), "a");
    let closing = at(2027, 3, 14, 3, 0);
    assert_eq!(s.timers.at(&doc), Some(closing));
    assert_eq!(closing - now, 30 * 60_000, "the hour from 02:00 is skipped");
    set_hours(&s, "48", &daily(2 * 60 + 30, 5 * 60));
    clock_to(&s, at(2027, 3, 14, 3, 15));
    assert!(
        page(&s, "b", "/stores/48").contains("Closed · opens today at 2:30 am"),
        "03:15 PDT, before 02:30 read as 03:30"
    );
    clock_to(&s, at(2027, 3, 14, 3, 31));
    assert!(
        page(&s, "c", "/stores/48").contains("Open until 5:00 am"),
        "03:31 PDT"
    );
}

/// **The home page and a search show each store's hours**, and a store open
/// every day round says so. The clock stands still between the two reads,
/// so the home page's stores, kept 30 seconds, are read again only because
/// the change to the hours dropped them.
#[test]
fn the_home_page_and_a_search_show_each_stores_hours() {
    let s = served();
    clock_to(&s, at(2027, 3, 1, 20, 0));
    let home = page(&s, "a", "/");
    assert_eq!(home.matches("Open 24 hours").count(), 2, "{home}");
    set_hours(&s, "48", &daily(7 * 60, 19 * 60));
    let home = page(&s, "b", "/");
    assert!(
        home.contains("Harbor Coffee Café, Bakery Closed · opens tomorrow at 7:00 am"),
        "{home}"
    );
    assert!(home.contains("Blue Bottle Coffee Open 24 hours"), "{home}");
    let found = page(&s, "b", "/search?q=coffee");
    assert!(
        found.contains("Harbor Coffee Café, Bakery Closed · opens tomorrow at 7:00 am"),
        "{found}"
    );
}

/// **A set of hours the store's rules refuse changes nothing**: a weekday
/// past 7, a minute past 1440, a zone the database does not have.
#[test]
fn hours_a_store_cannot_have_are_refused() {
    let s = served();
    for bad in [
        Hours {
            weekly: vec![Period {
                weekday: 8,
                opens: 0,
                closes: 60,
            }],
            ..daily(0, 0)
        },
        Hours {
            weekly: vec![Period {
                weekday: 1,
                opens: 0,
                closes: 1441,
            }],
            ..daily(0, 0)
        },
        Hours {
            zone: "Mars/Olympus_Mons".to_string(),
            ..daily(0, 0)
        },
    ] {
        let answer = s.store_write(
            "",
            "store:host/hours#set",
            &[Val::String("47".into()), crate::hours::hours_val(&bad)],
            &[],
            |_| {},
        );
        assert!(answer.is_err(), "{bad:?}: {answer:?}");
    }
    assert_eq!(
        page(&s, "a", "/").matches("Open 24 hours").count(),
        2,
        "unchanged"
    );
}

/// **The stores' first hours are alike on both layers**: `hours.rs`'s seed
/// in memory, `0012_store_hours.sql` on PostgreSQL; and a set of hours is
/// read back as it was written, its special date's day too.
#[test]
fn the_hours_read_back_alike_on_this_layer() {
    let s = served();
    let ok = |h: &Hours| Val::Result(Ok(Some(Box::new(crate::hours::hours_val(h)))));
    for store in ["47", "48"] {
        assert_eq!(
            s.layer_read("a", "store:data/hours#for-store", store),
            ok(&crate::hours::seed()),
            "{store}"
        );
    }
    let mut hours = daily(6 * 60 + 30, 2 * 60);
    hours.last_order_minutes = 45;
    hours.special.push(Special {
        date: pw_time::days(2027, 12, 25),
        closed: true,
        opens: 0,
        closes: 0,
    });
    hours.special.push(Special {
        date: pw_time::days(2027, 12, 31),
        closed: false,
        opens: 8 * 60,
        closes: 14 * 60,
    });
    set_hours(&s, "48", &hours);
    assert_eq!(
        s.layer_read("a", "store:data/hours#for-store", "48"),
        ok(&hours)
    );
    assert!(
        matches!(
            s.layer_read("a", "store:data/hours#for-store", "999"),
            Val::Result(Err(_))
        ),
        "a store not held"
    );
}

/// **A cut-off across midnight, and a store that never closes**: open 18:00
/// to 00:10, the last order at 23:50 the day before its closing; open the
/// day round, an order at 23:50 is taken, the days joined into one opening.
#[test]
fn a_cut_off_across_midnight_and_a_store_that_never_closes() {
    let s = served();
    set_hours(&s, "47", &daily(18 * 60, 10));
    clock_to(&s, at(2027, 3, 1, 23, 49));
    s.command(ADD, "a", &add_shown("espresso", 1), false)
        .expect("added");
    let placed = s
        .command_answered(PLACE, "a", &[s.shown_total("a")], None)
        .expect("answered");
    assert!(placed.committed, "23:49: before the last order");
    s.command(ADD, "a", &add_shown("espresso", 1), false)
        .expect("added");
    clock_to(&s, at(2027, 3, 1, 23, 55));
    let refused = s
        .command_answered(PLACE, "a", &[s.shown_total("a")], None)
        .expect("answered");
    assert!(!refused.committed, "23:55: past the last order, 23:50");
    // Store 48 is open the day round, as it is first.
    s.command(ADD, "b", &add_shown("drip", 1), false)
        .expect("added");
    let placed = s
        .command_answered(PLACE, "b", &[s.shown_total("b")], None)
        .expect("answered");
    assert!(placed.committed, "23:55 at a store that never closes");
}

/// Hours of `periods`, each `(weekday, opens, closes)` in minutes.
fn weekly(periods: &[(i64, i64, i64)]) -> Hours {
    Hours {
        weekly: periods
            .iter()
            .map(|&(weekday, opens, closes)| Period {
                weekday,
                opens,
                closes,
            })
            .collect(),
        ..daily(0, 0)
    }
}

/// Every day's `periods`, in the order given.
pub(super) fn every_day(periods: &[(i64, i64)]) -> Hours {
    let each: Vec<(i64, i64, i64)> = (1..=7)
        .flat_map(|d| periods.iter().map(move |&(o, c)| (d, o, c)))
        .collect();
    weekly(&each)
}

/// **A day's periods are taken by opening, whatever order a set lists them
/// in** (the integrator's rulings of 2026-10-12): dinner, 17:00 to 22:00,
/// listed and written before lunch, 11:00 to 14:00; at 09:00 the store opens
/// at 11:00, and, the control, at 15:00 at 17:00. The layer hands them
/// ordered: memory as it holds a set, PostgreSQL by its `ORDER BY`.
#[test]
fn a_days_periods_are_taken_by_opening_whatever_the_order_listed() {
    let s = served();
    set_hours(
        &s,
        "47",
        &every_day(&[(17 * 60, 22 * 60), (11 * 60, 14 * 60)]),
    );
    clock_to(&s, at(2027, 3, 1, 9, 0));
    let morning = page(&s, "a", "/stores/47");
    assert!(
        morning.contains("Closed · opens today at 11:00 am"),
        "09:00: {morning}"
    );
    clock_to(&s, at(2027, 3, 1, 15, 0));
    let afternoon = page(&s, "b", "/stores/47");
    assert!(
        afternoon.contains("Closed · opens today at 5:00 pm"),
        "15:00: {afternoon}"
    );
}

/// **Overlapping periods are open as their union** (the integrator's ruling
/// of 2026-10-12): 11:00 to 15:00 and 14:00 to 22:00; at 14:30 the store is
/// open until 22:00, and an order at 14:45 is taken, not refused at the
/// first period's last order; the control, at 22:30 it is closed and an
/// order refused.
#[test]
fn overlapping_periods_are_open_as_their_union() {
    let s = served();
    set_hours(
        &s,
        "47",
        &every_day(&[(11 * 60, 15 * 60), (14 * 60, 22 * 60)]),
    );
    clock_to(&s, at(2027, 3, 1, 14, 30));
    let open = page(&s, "a", "/stores/47");
    assert!(open.contains("Open until 10:00 pm"), "14:30: {open}");
    clock_to(&s, at(2027, 3, 1, 14, 45));
    s.command(ADD, "a", &add_shown("espresso", 1), false)
        .expect("added");
    let placed = s
        .command_answered(PLACE, "a", &[s.shown_total("a")], None)
        .expect("answered");
    assert!(placed.committed, "14:45: open until 22:00");
    clock_to(&s, at(2027, 3, 1, 22, 30));
    s.command(ADD, "a", &add_shown("espresso", 1), false)
        .expect("added");
    let refused = s
        .command_answered(PLACE, "a", &[s.shown_total("a")], None)
        .expect("answered");
    assert!(!refused.committed, "22:30: closed");
}

/// **A period past midnight that overlaps the next day's first is one
/// span**: Monday 18:00 to 02:00 and Tuesday 01:00 to 10:00; at Tuesday
/// 01:30 the store is open until 10:00 am, and, the control, at 10:30 it is
/// closed until Monday's 18:00.
#[test]
fn a_period_past_midnight_overlapping_the_next_days_is_one_span() {
    let s = served();
    set_hours(&s, "47", &weekly(&[(1, 18 * 60, 2 * 60), (2, 60, 10 * 60)]));
    clock_to(&s, at(2027, 3, 2, 1, 30));
    let night = page(&s, "a", "/stores/47");
    assert!(
        night.contains("Open until 10:00 am"),
        "Tuesday 01:30: {night}"
    );
    clock_to(&s, at(2027, 3, 2, 10, 30));
    let morning = page(&s, "b", "/stores/47");
    assert!(
        morning.contains("Closed · opens Monday at 6:00 pm"),
        "Tuesday 10:30: {morning}"
    );
}

/// **A set reads back as the layer holds it, on either layer**: its weekly
/// periods by weekday and opening, its special dates by date, the last of a
/// period or a date listed twice kept, as PostgreSQL's upsert keeps it (the
/// integrator's ruling of 2026-10-12).
#[test]
fn a_set_reads_back_ordered_the_last_of_each_kept() {
    let s = served();
    let special = |date: i64, closed: bool, opens: i64, closes: i64| Special {
        date,
        closed,
        opens,
        closes,
    };
    let (christmas, new_year) = (pw_time::days(2027, 12, 25), pw_time::days(2027, 12, 31));
    let mut written = weekly(&[
        (2, 17 * 60, 22 * 60),
        (1, 17 * 60, 21 * 60),
        (1, 11 * 60, 14 * 60),
        (1, 17 * 60, 23 * 60),
    ]);
    written.special = vec![
        special(new_year, false, 8 * 60, 14 * 60),
        special(christmas, false, 9 * 60, 12 * 60),
        special(christmas, true, 0, 0),
    ];
    set_hours(&s, "48", &written);
    let mut held = weekly(&[
        (1, 11 * 60, 14 * 60),
        (1, 17 * 60, 23 * 60),
        (2, 17 * 60, 22 * 60),
    ]);
    held.special = vec![
        special(christmas, true, 0, 0),
        special(new_year, false, 8 * 60, 14 * 60),
    ];
    assert_eq!(
        s.layer_read("a", "store:data/hours#for-store", "48"),
        Val::Result(Ok(Some(Box::new(crate::hours::hours_val(&held))))),
    );
}

/// The legacy set contract retains request order and duplicates. Every
/// occurrence carries the full value; a prior occurrence cannot consume it.
/// The same test runs on the configured memory/PostgreSQL DataLayer.
#[test]
fn a_repeated_store_in_a_set_read_keeps_all_of_its_hours() {
    let layer = crate::store::layer_for_tests();
    let first = Hours {
        zone: "America/Los_Angeles".into(),
        last_order_minutes: 13,
        weekly: vec![
            Period {
                weekday: 1,
                opens: 420,
                closes: 720,
            },
            Period {
                weekday: 1,
                opens: 780,
                closes: 1140,
            },
        ],
        special: vec![
            Special {
                date: pw_time::days(2027, 3, 3),
                closed: true,
                opens: 0,
                closes: 0,
            },
            Special {
                date: pw_time::days(2027, 3, 4),
                closed: false,
                opens: 600,
                closes: 900,
            },
        ],
    };
    let second = Hours {
        last_order_minutes: 7,
        weekly: vec![Period {
            weekday: 2,
            opens: 540,
            closes: 1020,
        }],
        special: vec![Special {
            date: pw_time::days(2027, 3, 5),
            closed: false,
            opens: 660,
            closes: 840,
        }],
        ..first.clone()
    };
    let mut staged = layer.begin("hours-conformance");
    let writes = staged.ops();
    for (store, hours) in [("47", &first), ("48", &second)] {
        assert_eq!(
            writes["store:host/hours#set"](&[
                Val::String(store.into()),
                crate::hours::hours_val(hours),
            ])
            .expect("the real host write runs"),
            vec![Val::Result(Ok(None))],
            "the fixture's hours are accepted for {store}",
        );
    }
    staged
        .commit(&[], &[], None, &[], None)
        .expect("the fixture commits");
    staged.publish();
    drop(staged);
    let reads = layer.reads("hours-conformance", None);
    let result_for = |store: &str| {
        let hours = match store {
            "47" => &first,
            "48" => &second,
            _ => return None,
        };
        Some(Val::Record(vec![
            ("store".into(), Val::String(store.into())),
            ("name".into(), Val::String(String::new())),
            ("hours".into(), crate::hours::hours_val(hours)),
        ]))
    };
    for size in [3, 17] {
        let requests: Vec<_> = (0..size)
            .map(|i| ["48", "47", "missing", "47", "48"][i % 5])
            .chain(["47", "missing", "48", "47"])
            .collect();
        let expected: Vec<_> = requests.iter().filter_map(|s| result_for(s)).collect();
        let args = [Val::List(
            requests.iter().map(|s| Val::String((*s).into())).collect(),
        )];
        assert_eq!(
            reads["store:data/hours#for-stores"](&args).expect("the real set read runs"),
            vec![Val::List(expected)],
            "request order, misses, duplicates and full periods for {size} initial keys",
        );
    }
}
