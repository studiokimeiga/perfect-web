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
//! are what the database serves, less what it keeps for its superusers and
//! what others hold at the first test's start, a test's budget each, and
//! at least one; a test past them waits.

use std::cell::RefCell;
use std::sync::{Condvar, Mutex, OnceLock};

/// **The connections one test holds at its busiest**: two hosts' pools,
/// their listeners, the test's own clients. Nine at most, measured one test
/// at a time over the feed's, the store's, second-host's and the tables'
/// tests (2026-11-16); kept above it.
pub(crate) const BUDGET: usize = 12;

struct Places {
    held: Mutex<usize>,
    freed: Condvar,
}

static PLACES: Places = Places {
    held: Mutex::new(0),
    freed: Condvar::new(),
};
static LIMIT: OnceLock<usize> = OnceLock::new();

thread_local! {
    /// This thread's place, given back as the thread ends.
    static HELD: RefCell<Option<Place>> = const { RefCell::new(None) };
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
/// serves, less those it keeps for its superusers and those held now, a
/// test's budget each; at least one.
pub(crate) fn limit_of(url: &str) -> usize {
    let free = postgres::Client::connect(url, postgres::NoTls)
        .ok()
        .and_then(|mut c| {
            c.query_one(
                "SELECT current_setting('max_connections')::int \
                 - current_setting('superuser_reserved_connections')::int \
                 - (SELECT count(*)::int FROM pg_stat_activity WHERE backend_type = 'client backend')",
                &[],
            )
            .ok()
        })
        .map(|row| row.get::<_, i32>(0))
        .unwrap_or(0);
    (usize::try_from(free).unwrap_or(0) / BUDGET).max(1)
}

/// **A place among the tests that hold the database**, waited for while
/// every place is held.
pub(crate) fn place(url: &str) -> Place {
    let limit = *LIMIT.get_or_init(|| limit_of(url));
    let mut held = PLACES.held.lock().unwrap_or_else(|p| p.into_inner());
    while *held >= limit {
        held = PLACES.freed.wait(held).unwrap_or_else(|p| p.into_inner());
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
        }
    });
}

/// Whether this thread holds a place.
fn holds() -> bool {
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
    let threads = limit_of(&url) * 3;
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

/// A thread takes one place however many layers it opens; the first test
/// above holds that a place is given back as its thread ends (its later
/// threads could not start otherwise).
#[test]
fn a_thread_takes_one_place_however_many_layers_it_opens() {
    let Some(url) = super::tables_pg::database() else {
        return;
    };
    let inside = std::thread::spawn(move || {
        let before = holds();
        hold(&url);
        let after = holds();
        hold(&url);
        (before, after, holds())
    })
    .join()
    .expect("the thread ends");
    assert_eq!(inside, (false, true, true));
}
