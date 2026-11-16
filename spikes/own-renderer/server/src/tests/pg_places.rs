//! **How many tests hold the PostgreSQL database at once** (NEXT after L3,
//! item 10). A test's hosts open their connections as they need them and
//! keep them, so the server's tests, run at one thread per core, exhausted a
//! default database (100 connections): at 32 threads, seven of the feed's,
//! the store's and second-host's tests failed `53300: sorry, too many
//! clients already` (2026-11-16), where CI's four cores never reach it.
//!
//! Each test takes a place when it first opens a layer on the database
//! (`FeedPg::open_with`, `StorePg::open_with`, `TablesPg::from_build`), one
//! per thread however many layers it opens, and gives it back when its
//! thread ends: libtest runs each test on a thread of its own. The places
//! are what the database serves, less what it keeps reserved, what others
//! hold at the first test's start and a few more for what no place covers
//! (a test's fixture before its first layer, a backend still exiting), a
//! test's budget each, and at least one; a test past them waits, ten
//! minutes at most, and then fails saying why.
//!
//! What it does not bound: a test that opens a layer on another thread than
//! its own (a place belongs to a thread, so such a layer is counted apart,
//! or not at all once that thread ends: open each on the test's own thread),
//! and other processes on the same database (a second `cargo test` sizes its
//! places on what the first holds then).

use std::cell::RefCell;
use std::sync::{Condvar, Mutex, OnceLock};

/// **The connections one test holds at its busiest**: two hosts' pools,
/// their listeners, the test's own clients. Ten at most, measured one test
/// at a time over every server test but this module's, with the store on
/// PostgreSQL and all three databases set (2026-11-16, ASSUMPTIONS); kept
/// above it.
pub(crate) const BUDGET: usize = 12;

struct Places {
    held: Mutex<usize>,
    freed: Condvar,
}

static PLACES: Places = Places {
    held: Mutex::new(0),
    freed: Condvar::new(),
};

/// What no place covers: a test's fixture connections before its first
/// layer, and backends still exiting after a place is given back.
const HEADROOM: i32 = 6;

/// How long a test waits for a place before it fails saying why.
const PATIENCE: std::time::Duration = std::time::Duration::from_secs(600);
static LIMIT: OnceLock<usize> = OnceLock::new();

thread_local! {
    /// This thread's place, given back as the thread ends.
    static HELD: RefCell<Option<Place>> = const { RefCell::new(None) };
    /// How many places this thread has taken: one, however many layers.
    static TAKEN: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// A place, given back when it is dropped.
pub(crate) struct Place(());

impl Drop for Place {
    fn drop(&mut self) {
        let mut held = PLACES.held.lock().unwrap_or_else(|p| p.into_inner());
        *held = held.saturating_sub(1);
        PLACES.freed.notify_one();
    }
}

/// **How many places the database at `url` has**: the connections it
/// serves, less those it keeps reserved (for superusers, and since
/// PostgreSQL 16 for roles granted them), those held now (a session another
/// role holds counts too, though its kind is not shown to us) and the
/// headroom, a test's budget each; at least one. A database that cannot be
/// asked is an error, never one quiet place.
pub(crate) fn limit_of(url: &str) -> Result<usize, String> {
    let asked =
        |e: postgres::Error| format!("the test database cannot be asked its connections: {e}");
    let mut client = postgres::Client::connect(url, postgres::NoTls).map_err(asked)?;
    let free: i32 = client
        .query_one(
            "SELECT current_setting('max_connections')::int \
             - current_setting('superuser_reserved_connections')::int \
             - coalesce(current_setting('reserved_connections', true)::int, 0) \
             - (SELECT count(*)::int FROM pg_stat_activity \
                WHERE backend_type = 'client backend' OR backend_type IS NULL)",
            &[],
        )
        .map_err(asked)?
        .get(0);
    let usable = usize::try_from(free - HEADROOM).unwrap_or(0);
    if usable < BUDGET {
        eprintln!(
            "pw tests: the test database serves {usable} connections more, less than one \
             test's {BUDGET}: its tests run one at a time, and may still be refused (53300)"
        );
    }
    Ok((usable / BUDGET).max(1))
}

/// **A place among the tests that hold the database**, waited for while
/// every place is held, for [`PATIENCE`] at most.
pub(crate) fn place(url: &str) -> Place {
    let limit = *LIMIT.get_or_init(|| match limit_of(url) {
        Ok(limit) => {
            eprintln!(
                "pw tests: {limit} at once hold the test database, {BUDGET} connections each"
            );
            limit
        }
        Err(why) => panic!("{why}"),
    });
    let mut held = PLACES.held.lock().unwrap_or_else(|p| p.into_inner());
    let since = std::time::Instant::now();
    while *held >= limit {
        let waited = since.elapsed();
        assert!(
            waited < PATIENCE,
            "waited {waited:?} for a place on the test database, {} of {limit} held: a test \
             opened a layer on a thread that never ends, or holds a place while it waits on \
             another thread that needs one",
            *held
        );
        held = PLACES
            .freed
            .wait_timeout(held, PATIENCE.saturating_sub(waited))
            .unwrap_or_else(|p| p.into_inner())
            .0;
    }
    *held += 1;
    Place(())
}

/// **This thread's place, taken where it holds none**: held until the
/// thread ends, whatever layers it opens meanwhile.
pub(crate) fn hold(url: &str) {
    HELD.with(|held| {
        if held.borrow().is_none() {
            let taken = place(url);
            *held.borrow_mut() = Some(taken);
            TAKEN.with(|n| n.set(n.get() + 1));
        }
    });
}

/// Whether this thread holds a place.
pub(crate) fn holds() -> bool {
    HELD.with(|held| held.borrow().is_some())
}

/// **As many tests at once as the database serves, and the rest wait**:
/// three times as many threads as there are places each take one and open
/// a test's budget of connections at once. With the places, none is
/// refused; without them (every thread at once) the database would be asked
/// for three times what it serves, and refuse with `53300`.
#[test]
fn tests_beyond_the_databases_connections_wait_for_a_place() {
    let Some(url) = super::tables_pg::database() else {
        return;
    };
    let threads = limit_of(&url).expect("the test database") * 3;
    let failures: Vec<String> = (0..threads)
        .map(|_| {
            let url = url.clone();
            std::thread::spawn(move || {
                hold(&url);
                let mut open = Vec::new();
                for _ in 0..BUDGET {
                    match postgres::Client::connect(&url, postgres::NoTls) {
                        Ok(client) => open.push(client),
                        Err(e) => return Some(format!("{e}")),
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(200));
                None
            })
        })
        .collect::<Vec<_>>()
        .into_iter()
        .filter_map(|t| t.join().unwrap_or(Some("a thread panicked".into())))
        .collect();
    assert_eq!(
        failures,
        Vec::<String>::new(),
        "{threads} threads, each a test's budget"
    );
}

/// **A thread takes one place however many layers it opens**; the first
/// test above holds that a place is given back as its thread ends (its later
/// threads could not start otherwise).
#[test]
fn a_thread_takes_one_place_however_many_layers_it_opens() {
    let Some(url) = super::tables_pg::database() else {
        return;
    };
    let inside = std::thread::spawn(move || {
        let before = holds();
        hold(&url);
        hold(&url);
        hold(&url);
        (before, holds(), TAKEN.with(std::cell::Cell::get))
    })
    .join()
    .expect("the thread ends");
    assert_eq!(inside, (false, true, 1));
}

/// **The feed's and the store's layers take their thread's place** (the
/// review of 2026-11-16: no test held the hooks), each opened on a thread of
/// its own; the tables' is held in `tests::tables_pg`.
#[test]
fn the_feeds_and_the_stores_layers_take_their_threads_place() {
    let Some(url) = super::tables_pg::database() else {
        return;
    };
    let feed = {
        let url = url.clone();
        std::thread::spawn(move || {
            let before = holds();
            let on = super::feed_pg::served_on(&url, "pg_place");
            let held = holds();
            drop(on);
            (before, held)
        })
        .join()
        .expect("the feed's thread")
    };
    let store = std::thread::spawn(move || {
        let before = holds();
        let layer = crate::store_pg::StorePg::for_test(&url).expect("the store's layer");
        let held = holds();
        drop(layer);
        (before, held)
    })
    .join()
    .expect("the store's thread");
    assert_eq!((feed, store), ((false, true), (false, true)));
}
