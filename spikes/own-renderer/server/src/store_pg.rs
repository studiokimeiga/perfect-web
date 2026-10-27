//! **The store's data in PostgreSQL** (track `store-pg`), beside the
//! in-memory layer (`store.rs`), which stays the default. A deployment names
//! a database with `PW_STORE_DATABASE_URL`; the host opens this layer on it,
//! holds it to what the store's `source` states, and serves the same
//! program. The feed's layer on PostgreSQL (ADR-0246) is its model.
//!
//! - **Its own schema** (`pw_store`, or a test's), which its connections
//!   search alone: its tables, its `outbox`, its migration table
//!   (`pw_store_migrations`) and its advisory lock are its own, and no name
//!   it writes can reach the feed's.
//! - **A command is one transaction**, opened at [`DataLayer::begin`] and
//!   serializable whatever the connection's default. Its writes, and what it
//!   handed the outbox, commit together or not at all (ADR-0208).
//! - **What is delivered is read back from the outbox** once the
//!   transaction committed, and consumed. The host records it in its
//!   materializer as the in-memory layer's commit is recorded, so a
//!   session's cart reaches its open pages as it does in memory.
//! - **Its operations are `store.rs`'s**, over [`Rows`] read and written
//!   here: the two layers cannot answer differently.
//!
//! [`DataLayer::begin`]: crate::data::DataLayer::begin

use super::*;
use crate::data::{Dropped, Handed, Ops, Provided};
use crate::feed_pg::{Isolation, delivered, ident, json_of, pg};
use crate::store::{AddressRow, Estimate, Faults, Item, Order, Rows, StoreRow, With, Written};
use postgres::{Client, NoTls};
use std::collections::BTreeSet;
use std::sync::atomic::Ordering;

/// The migrations, in the order they apply, by the name each is recorded by.
const MIGRATIONS: &[(&str, &str)] = &[
    (
        "0001_schema",
        include_str!("../migrations/store/0001_schema.sql"),
    ),
    (
        "0002_seed",
        include_str!("../migrations/store/0002_seed.sql"),
    ),
    // Track `store-accounts`: a cart and an order are their owner's.
    (
        "0003_owners",
        include_str!("../migrations/store/0003_owners.sql"),
    ),
    // And where a store delivers, and a reader's addresses.
    (
        "0004_addresses",
        include_str!("../migrations/store/0004_addresses.sql"),
    ),
    // ADR-0325: an interaction's answer, committed with its writes.
    (
        "0005_interactions",
        include_str!("../migrations/store/0005_interactions.sql"),
    ),
    // TRACK SEAM (store-search): each store's cuisine, which a search reads.
    (
        "0010_search",
        include_str!("../migrations/store/0010_search.sql"),
    ),
    // And the cuisine, the store's own column (ADR-0308, amended).
    (
        "0011_store_cuisine",
        include_str!("../migrations/store/0011_store_cuisine.sql"),
    ),
    // TRACK SEAM (store-hours): each store's zone, cut-off and hours.
    (
        "0012_store_hours",
        include_str!("../migrations/store/0012_store_hours.sql"),
    ),
    // TRACK SEAM (order-total): the fees, the codes, the choices, and the
    // orders as charged.
    (
        "0013_order_total",
        include_str!("../migrations/store/0013_order_total.sql"),
    ),
    // NEXT 6c: the payment an order awaits.
    (
        "0014_order_payments",
        include_str!("../migrations/store/0014_order_payments.sql"),
    ),
    // The scheduled-commands track: the runs a command schedules.
    (
        "0015_schedules",
        include_str!("../migrations/store/0015_schedules.sql"),
    ),
    // TRACK SEAM (scheduled-orders): the time chosen at checkout, and an
    // order's time.
    (
        "0016_scheduled_orders",
        include_str!("../migrations/store/0016_scheduled_orders.sql"),
    ),
    // TRACK SEAM (reviews): a store's ratings, each through an order.
    (
        "0017_ratings",
        include_str!("../migrations/store/0017_ratings.sql"),
    ),
    // Track `second-host` (ADR-0369): the outbox a log every host reads.
    (
        "0018_outbox_log",
        include_str!("../migrations/store/0018_outbox_log.sql"),
    ),
    // Track `second-host` (ADR-0369): sessions and sign-ins kept.
    (
        "0019_sessions",
        include_str!("../migrations/store/0019_sessions.sql"),
    ),
    (
        "0020_scheduled_commit",
        include_str!("../migrations/store/0020_scheduled_commit.sql"),
    ),
    (
        "0021_session_epoch",
        include_str!("../migrations/store/0021_session_epoch.sql"),
    ),
];

/// **The schema a deployment's store is in**, unless it names another.
pub(crate) const SCHEMA: &str = "pw_store";

/// How many connections one layer holds at most: a reader past it waits for
/// one to be given back.
const CONNECTIONS: usize = 8;

/// **Connections, kept open between uses**, each searching the store's
/// schema alone, and no more than [`CONNECTIONS`] at once.
struct Pool {
    url: String,
    schema: String,
    /// The idle connections, and how many are out.
    idle: Mutex<(Vec<Client>, usize)>,
    given_back: std::sync::Condvar,
}

impl Pool {
    fn connect(&self) -> Result<Client, String> {
        let mut c = Client::connect(&self.url, NoTls).map_err(pg)?;
        c.batch_execute(&format!("SET search_path TO {}", ident(&self.schema)))
            .map_err(pg)?;
        Ok(c)
    }

    fn take(self: &Arc<Self>) -> Result<Conn, String> {
        let mut idle = self.idle.lock().expect("idle connections");
        loop {
            while let Some(c) = idle.0.pop() {
                if !c.is_closed() {
                    idle.1 += 1;
                    return Ok(Conn::of(self, c));
                }
            }
            if idle.1 < CONNECTIONS {
                idle.1 += 1;
                drop(idle);
                return match self.connect() {
                    Ok(c) => Ok(Conn::of(self, c)),
                    Err(why) => {
                        self.gone();
                        Err(why)
                    }
                };
            }
            idle = self.given_back.wait(idle).expect("idle connections");
        }
    }

    /// A connection given back, or `None` for one that is not kept.
    fn give(&self, c: Option<Client>) {
        let mut idle = self.idle.lock().expect("idle connections");
        idle.1 -= 1;
        if let Some(c) = c.filter(|c| !c.is_closed()) {
            idle.0.push(c);
        }
        self.given_back.notify_one();
    }

    fn gone(&self) {
        self.give(None);
    }
}

/// **A connection taken from the pool**, given back when dropped; one that
/// is [`Conn::discard`]ed is closed instead.
struct Conn {
    pool: Arc<Pool>,
    client: Option<Client>,
}

impl Conn {
    fn of(pool: &Arc<Pool>, c: Client) -> Conn {
        Conn {
            pool: pool.clone(),
            client: Some(c),
        }
    }

    fn client(&mut self) -> &mut Client {
        self.client.as_mut().expect("a connection taken")
    }

    /// Closed, not given back: a transaction it could not end.
    fn discard(mut self) {
        self.client = None;
    }
}

impl Drop for Conn {
    fn drop(&mut self) {
        self.pool.give(self.client.take());
    }
}

/// **Open a command's transaction** on `c`, as `isolation` says: the one
/// place a command's transaction begins, which [`StorePg::provides`]
/// measures.
fn begin_command(c: &mut Client, isolation: Isolation) -> Result<(), postgres::Error> {
    match isolation {
        Isolation::Serializable => c.batch_execute("BEGIN ISOLATION LEVEL SERIALIZABLE"),
        Isolation::ConnectionDefault => c.batch_execute("BEGIN"),
    }
}

/// **Apply each migration not yet applied**, in order, in one transaction,
/// under a lock no other host migrating the store can share. The table that
/// records them, and the lock, are the store's, never the feed's.
fn migrate(c: &mut Client) -> Result<(), String> {
    let mut tx = c.transaction().map_err(pg)?;
    tx.batch_execute(
        "SELECT pg_advisory_xact_lock(hashtext('pw_store_migrations'));
         CREATE TABLE IF NOT EXISTS pw_store_migrations (
             version    text PRIMARY KEY,
             applied_at timestamptz NOT NULL DEFAULT now()
         );",
    )
    .map_err(pg)?;
    for (version, sql) in MIGRATIONS {
        let applied = tx
            .query_opt(
                "SELECT 1 FROM pw_store_migrations WHERE version = $1",
                &[version],
            )
            .map_err(pg)?
            .is_some();
        if applied {
            continue;
        }
        tx.batch_execute(sql)
            .map_err(|e| format!("migration {version}: {}", pg(e)))?;
        tx.execute(
            "INSERT INTO pw_store_migrations (version) VALUES ($1)",
            &[version],
        )
        .map_err(pg)?;
    }
    tx.commit().map_err(pg)
}

/// **The store's rows, on a connection**, in whatever transaction it is in.
struct PgRows<'c>(Counted<'c>);

// TRACK SEAM (store-search): every statement the store's rows run, counted
// for a test (as Django's `assertNumQueries` does), on the thread a read or a
// command runs on: what tells a read that grows with the data from one that
// does not.
#[cfg(test)]
thread_local! {
    pub(crate) static STATEMENTS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// **A store's row**, as `stores` and `stores_by_ids` select it.
fn store_row(r: &postgres::Row) -> StoreRow {
    StoreRow {
        id: r.get(0),
        name: r.get(1),
        description: r.get(2),
        opens_minute: r.get(3),
        closes_minute: r.get(4),
        zone: crate::places::Zone {
            lat_e6: r.get(5),
            lon_e6: r.get(6),
            radius_m: r.get(7),
        },
        cuisine: r.get(8),
    }
}

/// **The statements a layer's reads and commands ran, whatever thread ran
/// them** (NEXT 4b): what a page's or a command's count is read from, each
/// call's statements (`STATEMENTS`, counted on its own thread) added once
/// it returns. Nothing in a build that is not a test's.
#[derive(Clone, Default)]
pub(crate) struct Tally {
    #[cfg(test)]
    total: Arc<std::sync::atomic::AtomicU64>,
    /// Each operation's calls and statements, by its name: what a page's or
    /// a command's count is made of (NEXT 4c's measured baseline).
    #[cfg(test)]
    by_op: Arc<Mutex<BTreeMap<String, (u64, u64)>>>,
    /// Each command's transaction's statements, its operations' and its
    /// commit's, in the order the commands began.
    #[cfg(test)]
    transactions: Arc<Mutex<Vec<u64>>>,
}

impl Tally {
    fn around<T>(&self, f: impl FnOnce() -> T) -> T {
        self.around_in(None, f)
    }

    /// [`around`](Tally::around), the statements also the command's
    /// transaction `txn`'s, where it is one.
    fn around_in<T>(&self, txn: Option<usize>, f: impl FnOnce() -> T) -> T {
        #[cfg(not(test))]
        let _ = txn;
        #[cfg(test)]
        let before = STATEMENTS.with(|n| n.get());
        let out = f();
        #[cfg(test)]
        {
            let ran = STATEMENTS.with(|n| n.get()) - before;
            self.total
                .fetch_add(ran, std::sync::atomic::Ordering::SeqCst);
            if let Some(t) = txn {
                self.transactions.lock().expect("transactions")[t] += ran;
            }
        }
        out
    }

    /// A command's transaction begun: its place among the tally's.
    fn begin(&self) -> Option<usize> {
        #[cfg(test)]
        {
            let mut t = self.transactions.lock().expect("transactions");
            t.push(0);
            Some(t.len() - 1)
        }
        #[cfg(not(test))]
        None
    }

    #[cfg(test)]
    pub(crate) fn transactions(&self) -> Vec<u64> {
        self.transactions.lock().expect("transactions").clone()
    }

    /// `ops`, each call's statements recorded by the operation's name: a
    /// test's. Elsewhere `ops` as they are.
    fn by_name(&self, ops: Ops, within: &'static str) -> Ops {
        #[cfg(not(test))]
        let _ = within;
        #[cfg(test)]
        let ops = ops
            .into_iter()
            .map(|(name, f)| {
                let (by_op, key) = (self.by_op.clone(), format!("{within} {name}"));
                let counted: crate::HostFn = Arc::new(move |args: &[Val]| {
                    let before = STATEMENTS.with(|n| n.get());
                    let out = f(args);
                    let ran = STATEMENTS.with(|n| n.get()) - before;
                    let mut by_op = by_op.lock().expect("by operation");
                    let entry = by_op.entry(key.clone()).or_default();
                    entry.0 += 1;
                    entry.1 += ran;
                    out
                });
                (name, counted)
            })
            .collect();
        ops
    }

    #[cfg(test)]
    pub(crate) fn get(&self) -> u64 {
        self.total.load(std::sync::atomic::Ordering::SeqCst)
    }

    #[cfg(test)]
    pub(crate) fn by_op(&self) -> BTreeMap<String, (u64, u64)> {
        self.by_op.lock().expect("by operation").clone()
    }
}

/// **A connection whose every statement is counted** (`STATEMENTS`): what
/// `PgRows` and the search's own statements (`search.rs`) run through.
pub(crate) struct Counted<'c>(pub(crate) &'c mut Client);

impl Counted<'_> {
    fn counted() {
        Self::ran(1);
    }

    /// **`n` statements run on a connection that is not counted's**, by code
    /// another layer shares (`feed_pg::record`, `feed_pg::delivered`),
    /// counted where they are run.
    pub(crate) fn ran(n: u64) {
        #[cfg(not(test))]
        let _ = n;
        #[cfg(test)]
        STATEMENTS.with(|c| c.set(c.get() + n));
    }

    pub(crate) fn query<T: ?Sized + postgres::ToStatement>(
        &mut self,
        query: &T,
        params: &[&(dyn postgres::types::ToSql + Sync)],
    ) -> Result<Vec<postgres::Row>, postgres::Error> {
        Self::counted();
        self.0.query(query, params)
    }

    pub(crate) fn query_one<T: ?Sized + postgres::ToStatement>(
        &mut self,
        query: &T,
        params: &[&(dyn postgres::types::ToSql + Sync)],
    ) -> Result<postgres::Row, postgres::Error> {
        Self::counted();
        self.0.query_one(query, params)
    }

    pub(crate) fn query_opt<T: ?Sized + postgres::ToStatement>(
        &mut self,
        query: &T,
        params: &[&(dyn postgres::types::ToSql + Sync)],
    ) -> Result<Option<postgres::Row>, postgres::Error> {
        Self::counted();
        self.0.query_opt(query, params)
    }

    pub(crate) fn execute<T: ?Sized + postgres::ToStatement>(
        &mut self,
        query: &T,
        params: &[&(dyn postgres::types::ToSql + Sync)],
    ) -> Result<u64, postgres::Error> {
        Self::counted();
        self.0.execute(query, params)
    }
}

fn line_of(r: &postgres::Row) -> Line {
    Line {
        item: r.get(0),
        name: r.get(1),
        quantity: r.get(2),
        price: r.get(3),
    }
}

impl Rows for PgRows<'_> {
    // TRACK SEAM (store-search): a search, matched by the database (`search.rs`).
    fn search(&mut self, terms: &[String]) -> Result<Vec<crate::search::Matched>, String> {
        crate::search::on_postgres(&mut self.0, terms)
    }

    // TRACK SEAM (store-hours): a store's hours, read and replaced (`hours.rs`).
    fn hours(&mut self, store: &str) -> Result<Option<crate::hours::Hours>, String> {
        crate::hours::pg_hours(&mut self.0, store)
    }
    fn set_hours(&mut self, store: &str, hours: &crate::hours::Hours) -> Result<(), String> {
        crate::hours::pg_set_hours(&mut self.0, store, hours)
    }

    // TRACK SEAM (order-total): the checkout's rows (`checkout.rs`).
    fn fees(&mut self, stores: &[String]) -> Result<Vec<(String, crate::checkout::Fees)>, String> {
        crate::checkout::pg_fees(&mut self.0, stores)
    }
    fn set_fees(&mut self, store: &str, fees: &crate::checkout::Fees) -> Result<(), String> {
        crate::checkout::pg_set_fees(&mut self.0, store, fees)
    }
    fn promo(
        &mut self,
        owner: &str,
        code: &str,
    ) -> Result<Option<(crate::checkout::Promo, bool)>, String> {
        crate::checkout::pg_promo(&mut self.0, owner, code)
    }
    fn set_promo(&mut self, promo: &crate::checkout::Promo) -> Result<(), String> {
        crate::checkout::pg_set_promo(&mut self.0, promo)
    }
    fn use_promo(&mut self, owner: &str, code: &str) -> Result<bool, String> {
        crate::checkout::pg_use_promo(&mut self.0, owner, code)
    }
    fn release_promo(&mut self, owner: &str, code: &str) -> Result<(), String> {
        crate::checkout::pg_release_promo(&mut self.0, owner, code)
    }
    fn chosen(&mut self, owner: &str) -> Result<crate::checkout::Chosen, String> {
        crate::checkout::pg_chosen(&mut self.0, owner)
    }
    fn set_chosen(&mut self, owner: &str, chosen: &crate::checkout::Chosen) -> Result<(), String> {
        crate::checkout::pg_set_chosen(&mut self.0, owner, chosen)
    }
    fn charge(&mut self, owner: &str, charged: &crate::checkout::Charged) -> Result<(), String> {
        crate::checkout::pg_charge(&mut self.0, owner, charged)
    }
    fn charged(&mut self, owner: &str) -> Result<Option<crate::checkout::Charged>, String> {
        crate::checkout::pg_charged(&mut self.0, owner)
    }

    // TRACK SEAM (reviews): a store's ratings (`reviews.rs`).
    fn ratings_of(&mut self, stores: &[String]) -> Result<Vec<(String, i64, i64)>, String> {
        crate::reviews::pg_ratings_of(&mut self.0, stores)
    }
    fn reviews(&mut self, store: &str, most: usize) -> Result<Vec<(i64, i64, String)>, String> {
        crate::reviews::pg_reviews(&mut self.0, store, most)
    }
    fn to_rate(&mut self, owner: &str) -> Result<Option<crate::reviews::ToRate>, String> {
        crate::reviews::pg_to_rate(&mut self.0, owner)
    }
    fn rate(&mut self, rating: &crate::reviews::Rating) -> Result<bool, String> {
        crate::reviews::pg_rate(&mut self.0, rating)
    }
    // TRACK SEAM (scheduled-orders): an order's time (`scheduled_orders.rs`).
    fn order_time(
        &mut self,
        owner: &str,
    ) -> Result<Option<crate::scheduled_orders::OrderTime>, String> {
        crate::scheduled_orders::pg_order_time(&mut self.0, owner)
    }
    fn set_order_time(
        &mut self,
        owner: &str,
        time: &crate::scheduled_orders::OrderTime,
    ) -> Result<(), String> {
        crate::scheduled_orders::pg_set_order_time(&mut self.0, owner, time)
    }

    fn stores(&mut self) -> Result<Vec<StoreRow>, String> {
        let rows = self
            .0
            .query(
                "SELECT id, name, description, opens_minute, closes_minute, \
                        lat_e6, lon_e6, radius_m, cuisine \
                 FROM stores ORDER BY position",
                &[],
            )
            .map_err(pg)?;
        Ok(rows.iter().map(store_row).collect())
    }

    // TRACK SEAM (bounded-statements): rows read by set (NEXT 4b), one
    // statement each whatever the number of keys.
    fn stores_by_ids(&mut self, ids: &[String]) -> Result<Vec<StoreRow>, String> {
        let rows = self
            .0
            .query(
                "SELECT id, name, description, opens_minute, closes_minute, \
                        lat_e6, lon_e6, radius_m, cuisine \
                 FROM stores WHERE id = ANY($1) ORDER BY position",
                &[&ids],
            )
            .map_err(pg)?;
        Ok(rows.iter().map(store_row).collect())
    }
    fn stores_of_items(&mut self, items: &[String]) -> Result<BTreeMap<String, String>, String> {
        let rows = self
            .0
            .query(
                "SELECT DISTINCT ON (i.id) i.id, i.store \
                 FROM menu_items i JOIN stores s ON s.id = i.store \
                 WHERE i.id = ANY($1) ORDER BY i.id, s.position",
                &[&items],
            )
            .map_err(pg)?;
        Ok(rows.iter().map(|r| (r.get(0), r.get(1))).collect())
    }
    fn hours_of(
        &mut self,
        stores: &[String],
    ) -> Result<Vec<(String, crate::hours::Hours)>, String> {
        crate::hours::pg_hours_of(&mut self.0, stores)
    }

    fn menu(&mut self, store: &str) -> Result<Vec<Item>, String> {
        let rows = self
            .0
            .query(
                "SELECT i.id, i.name, i.description, i.price, i.category, c.name, i.served, \
                        i.available \
                 FROM menu_items i JOIN categories c ON c.store = i.store AND c.id = i.category \
                 WHERE i.store = $1 ORDER BY i.position",
                &[&store],
            )
            .map_err(pg)?;
        Ok(rows
            .iter()
            .map(|r| Item {
                id: r.get(0),
                name: r.get(1),
                description: r.get(2),
                price: r.get(3),
                category: r.get(4),
                category_name: r.get(5),
                served: r.get(6),
                available: r.get(7),
            })
            .collect())
    }

    fn set_menu(&mut self, store: &str, items: &[Item]) -> Result<(), String> {
        self.0
            .execute("DELETE FROM menu_items WHERE store = $1", &[&store])
            .map_err(pg)?;
        for (position, item) in items.iter().enumerate() {
            self.0
                .execute(
                    "INSERT INTO categories (store, id, name) VALUES ($1, $2, $3) \
                     ON CONFLICT DO NOTHING",
                    &[&store, &item.category, &item.category_name],
                )
                .map_err(pg)?;
            self.0
                .execute(
                    "INSERT INTO menu_items (store, id, position, name, description, price, \
                         category, served, available) \
                     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
                    &[
                        &store,
                        &item.id,
                        &(position as i32),
                        &item.name,
                        &item.description,
                        &item.price,
                        &item.category,
                        &item.served,
                        &item.available,
                    ],
                )
                .map_err(pg)?;
        }
        Ok(())
    }

    fn stock(&mut self, item: &str, available: bool) -> Result<(), String> {
        self.0
            .execute(
                "UPDATE menu_items SET available = $2 WHERE id = $1",
                &[&item, &available],
            )
            .map(|_| ())
            .map_err(pg)
    }

    fn catalog_item(&mut self, item: &str) -> Result<Option<(String, i64, bool)>, String> {
        let row = self
            .0
            .query_opt(
                "SELECT i.name, i.price, \
                        (SELECT bool_or(available) FROM menu_items WHERE id = $1) \
                 FROM menu_items i JOIN stores s ON s.id = i.store \
                 WHERE i.id = $1 ORDER BY s.position DESC LIMIT 1",
                &[&item],
            )
            .map_err(pg)?;
        Ok(row.map(|r| (r.get(0), r.get(1), r.get(2))))
    }

    fn cart(&mut self, owner: &str) -> Result<Lines, String> {
        let rows = self
            .0
            .query(
                "SELECT item, name, quantity, price FROM cart_lines \
                 WHERE owner = $1 ORDER BY position",
                &[&owner],
            )
            .map_err(pg)?;
        Ok(rows.iter().map(line_of).collect())
    }

    fn set_cart(&mut self, owner: &str, lines: &[Line]) -> Result<(), String> {
        self.0
            .execute("DELETE FROM cart_lines WHERE owner = $1", &[&owner])
            .map_err(pg)?;
        for (position, line) in lines.iter().enumerate() {
            self.0
                .execute(
                    "INSERT INTO cart_lines (owner, position, item, name, quantity, price) \
                     VALUES ($1, $2, $3, $4, $5, $6)",
                    &[
                        &owner,
                        &(position as i32),
                        &line.item,
                        &line.name,
                        &line.quantity,
                        &line.price,
                    ],
                )
                .map_err(pg)?;
        }
        Ok(())
    }

    fn order(&mut self, owner: &str) -> Result<Option<Order>, String> {
        let Some(order) = self
            .0
            .query_opt(
                "SELECT id, status FROM orders WHERE owner = $1 ORDER BY id DESC LIMIT 1",
                &[&owner],
            )
            .map_err(pg)?
        else {
            return Ok(None);
        };
        let id: i64 = order.get(0);
        let lines = self
            .0
            .query(
                "SELECT item, name, quantity, price FROM order_lines \
                 WHERE order_id = $1 ORDER BY position",
                &[&id],
            )
            .map_err(pg)?;
        Ok(Some(Order {
            // TRACK SEAM (reviews): its id.
            id,
            status: order.get(1),
            lines: lines.iter().map(line_of).collect(),
        }))
    }

    fn place(&mut self, owner: &str, lines: &[Line]) -> Result<(), String> {
        let id: i64 = self
            .0
            .query_one(
                "INSERT INTO orders (owner, status) VALUES ($1, 'placed') RETURNING id",
                &[&owner],
            )
            .map_err(pg)?
            .get(0);
        for (position, line) in lines.iter().enumerate() {
            self.0
                .execute(
                    "INSERT INTO order_lines (order_id, position, item, name, quantity, price) \
                     VALUES ($1, $2, $3, $4, $5, $6)",
                    &[
                        &id,
                        &(position as i32),
                        &line.item,
                        &line.name,
                        &line.quantity,
                        &line.price,
                    ],
                )
                .map_err(pg)?;
        }
        Ok(())
    }

    fn set_status(&mut self, owner: &str, status: Option<&str>) -> Result<(), String> {
        let Some(status) = status else {
            // NEXT 6c: and the payment it awaited, as the in-memory layer.
            self.0
                .execute("DELETE FROM order_payments WHERE owner = $1", &[&owner])
                .map_err(pg)?;
            return self
                .0
                .execute("DELETE FROM orders WHERE owner = $1", &[&owner])
                .map(|_| ())
                .map_err(pg);
        };
        let moved = self
            .0
            .execute(
                "UPDATE orders SET status = $2, committed_in = pg_current_xact_id() \
                 WHERE id = (SELECT max(id) FROM orders WHERE owner = $1)",
                &[&owner, &status],
            )
            .map_err(pg)?;
        if moved == 0 {
            self.0
                .execute(
                    "INSERT INTO orders (owner, status) VALUES ($1, $2)",
                    &[&owner, &status],
                )
                .map_err(pg)?;
        }
        Ok(())
    }
    fn awaiting(&mut self, owner: &str) -> Result<Option<crate::payments::Awaiting>, String> {
        let row = self
            .0
            .query_opt(
                "SELECT reference, payment, page FROM order_payments WHERE owner = $1",
                &[&owner],
            )
            .map_err(pg)?;
        Ok(row.map(|r| crate::payments::Awaiting {
            reference: r.get(0),
            payment: r.get(1),
            page: r.get(2),
        }))
    }
    fn set_awaiting(
        &mut self,
        owner: &str,
        awaiting: Option<&crate::payments::Awaiting>,
    ) -> Result<(), String> {
        match awaiting {
            Some(a) => self.0.execute(
                "INSERT INTO order_payments (owner, reference, payment, page) \
                 VALUES ($1, $2, $3, $4) \
                 ON CONFLICT (owner) DO UPDATE SET reference = $2, payment = $3, page = $4",
                &[&owner, &a.reference, &a.payment, &a.page],
            ),
            None => self
                .0
                .execute("DELETE FROM order_payments WHERE owner = $1", &[&owner]),
        }
        .map(|_| ())
        .map_err(pg)
    }

    fn addresses(&mut self, owner: &str) -> Result<Vec<AddressRow>, String> {
        let rows = self
            .0
            .query(
                "SELECT id, label, place, chosen FROM addresses \
                 WHERE owner = $1 ORDER BY position",
                &[&owner],
            )
            .map_err(pg)?;
        Ok(rows
            .iter()
            .map(|r| AddressRow {
                id: r.get(0),
                label: r.get(1),
                place: r.get(2),
                chosen: r.get(3),
            })
            .collect())
    }

    fn set_addresses(&mut self, owner: &str, rows: &[AddressRow]) -> Result<(), String> {
        self.0
            .execute("DELETE FROM addresses WHERE owner = $1", &[&owner])
            .map_err(pg)?;
        for (position, a) in rows.iter().enumerate() {
            self.0
                .execute(
                    "INSERT INTO addresses (owner, id, position, label, place, chosen) \
                     VALUES ($1, $2, $3, $4, $5, $6)",
                    &[
                        &owner,
                        &a.id,
                        &(position as i32),
                        &a.label,
                        &a.place,
                        &a.chosen,
                    ],
                )
                .map_err(pg)?;
        }
        Ok(())
    }

    fn notice(&mut self, store: &str) -> Result<Option<String>, String> {
        let row = self
            .0
            .query_opt("SELECT text FROM notices WHERE store = $1", &[&store])
            .map_err(pg)?;
        Ok(row.map(|r| r.get(0)))
    }

    fn set_notice(&mut self, store: &str, text: &str) -> Result<(), String> {
        self.0
            .execute(
                "INSERT INTO notices (store, text) VALUES ($1, $2) \
                 ON CONFLICT (store) DO UPDATE SET text = EXCLUDED.text",
                &[&store, &text],
            )
            .map(|_| ())
            .map_err(pg)
    }

    fn prep(&mut self, store: &str) -> Result<Option<i64>, String> {
        let row = self
            .0
            .query_opt(
                "SELECT prep_minutes FROM kitchens WHERE store = $1",
                &[&store],
            )
            .map_err(pg)?;
        Ok(row.map(|r| r.get(0)))
    }

    fn set_prep(&mut self, store: &str, minutes: i64) -> Result<(), String> {
        self.0
            .execute(
                "INSERT INTO kitchens (store, prep_minutes) VALUES ($1, $2) \
                 ON CONFLICT (store) DO UPDATE SET prep_minutes = EXCLUDED.prep_minutes",
                &[&store, &minutes],
            )
            .map(|_| ())
            .map_err(pg)
    }

    fn curated(&mut self) -> Result<Option<Vec<(String, String)>>, String> {
        let curated: Option<String> = self
            .0
            .query_one("SELECT curated::text FROM recommender", &[])
            .map_err(pg)?
            .get(0);
        curated
            .map(|json| {
                serde_json::from_str(&json).map_err(|e| format!("the recommender's curation: {e}"))
            })
            .transpose()
    }

    fn curate(&mut self, items: Option<&[(String, String)]>) -> Result<(), String> {
        let json = items.map(|items| serde_json::to_string(items).expect("strings encode"));
        self.0
            .execute("UPDATE recommender SET curated = $1::text::jsonb", &[&json])
            .map(|_| ())
            .map_err(pg)
    }

    fn estimate(&mut self, session: &str) -> Result<Option<Estimate>, String> {
        let row = self
            .0
            .query_opt(
                "SELECT min_minutes, max_minutes FROM estimates WHERE session = $1",
                &[&session],
            )
            .map_err(pg)?;
        Ok(row.map(|r| (r.get(0), r.get(1))))
    }

    fn set_estimate(&mut self, session: &str, (min, max): Estimate) -> Result<(), String> {
        self.0
            .execute(
                "INSERT INTO estimates (session, min_minutes, max_minutes) VALUES ($1, $2, $3) \
                 ON CONFLICT (session) DO UPDATE \
                 SET min_minutes = EXCLUDED.min_minutes, max_minutes = EXCLUDED.max_minutes",
                &[&session, &min, &max],
            )
            .map(|_| ())
            .map_err(pg)
    }
}

/// **The store's data in one PostgreSQL schema.**
pub(crate) struct StorePg {
    queue_authority: pw_materialize::QueueAuthority,
    pool: Arc<Pool>,
    /// TRACK SEAM (render-merge): the renders' connections, as the feed's
    /// (NEXT 4c): with the pool's, no more than `CONNECTIONS` + the
    /// deployment's render seats (`feed_pg::RenderSeats`) open.
    renders: Arc<crate::feed_pg::Renders>,
    isolation: Isolation,
    /// One command of this host at a time, from its first write to its
    /// commit, as the in-memory layer's lock is: a host never conflicts with
    /// itself. A serializable transaction is what holds against any other
    /// writer of the database, and a commit it refuses keeps nothing.
    writes: crate::data::Writes,
    faults: Faults,
    /// The statements its reads and commands ran (NEXT 4b).
    tally: Tally,
    /// **This host's reader of the outbox** (ADR-0369): every other host's
    /// commits, from its cursor; and its id, which its own rows carry.
    tail: crate::hosts::Tail,
    /// **Its sessions and sign-ins, kept where the hosts share them**
    /// (ADR-0369).
    sessions: Arc<crate::hosts::SessionsPg>,
    /// **This host's instance on the database** (ADR-0369), which its page
    /// numbers carry.
    instance: u64,
    /// A test's schema, dropped with the layer.
    #[cfg(test)]
    owned: bool,
}

impl StorePg {
    /// **The layer on `url`**, its tables in `schema` (created if missing),
    /// which its connections search alone, migrated, a command's
    /// transaction opened as `isolation` says, and its renders seated as the
    /// deployment's `seats` say.
    pub(crate) fn open_with(
        url: &str,
        schema: &str,
        isolation: Isolation,
        seats: crate::feed_pg::RenderSeats,
    ) -> Result<StorePg, String> {
        let mut admin = Client::connect(url, NoTls).map_err(pg)?;
        admin
            .batch_execute(&format!("CREATE SCHEMA IF NOT EXISTS {}", ident(schema)))
            .map_err(pg)?;
        let pool = Arc::new(Pool {
            url: url.to_string(),
            schema: schema.to_string(),
            idle: Mutex::new((Vec::new(), 0)),
            given_back: std::sync::Condvar::new(),
        });
        let mut c = pool.take()?;
        migrate(c.client())?;
        // **The outbox is every host's** (ADR-0369): a host just opened
        // reads it from its highest id, and deletes nothing another host
        // has yet to read. Until then it deleted the outbox, a second host's
        // undelivered rows among it.
        let highest = crate::hosts::highest(c.client())?;
        let instance = crate::hosts::instance(c.client())?;
        drop(c);
        let host = crate::hosts::new_host();
        let tail = crate::hosts::Tail::at(url, crate::hosts::Outbox::of(schema), &host, highest);
        let sessions = Arc::new(crate::hosts::SessionsPg::new(url, schema));
        Ok(StorePg {
            queue_authority: pw_materialize::QueueAuthority::new(),
            pool,
            renders: crate::feed_pg::Renders::new(url, Some(schema), seats),
            isolation,
            writes: crate::data::Writes::new(),
            faults: Faults::new(),
            tally: Tally::default(),
            tail,
            sessions,
            instance,
            #[cfg(test)]
            owned: false,
        })
    }

    /// **A test's layer**: in a schema of its own, `pw_store_test_`, the
    /// process and a number, dropped with the layer. Never any other.
    #[cfg(test)]
    pub(crate) fn for_test(url: &str) -> Result<StorePg, String> {
        StorePg::for_test_with(url, Isolation::Serializable)
    }

    /// [`StorePg::for_test`], a command's transaction opened as `isolation`
    /// says.
    #[cfg(test)]
    pub(crate) fn for_test_with(url: &str, isolation: Isolation) -> Result<StorePg, String> {
        StorePg::for_test_seated(url, isolation, crate::feed_pg::RenderSeats::default())
    }

    /// [`StorePg::for_test_with`], its renders seated as `seats` say, as a
    /// deployment's are.
    #[cfg(test)]
    pub(crate) fn for_test_seated(
        url: &str,
        isolation: Isolation,
        seats: crate::feed_pg::RenderSeats,
    ) -> Result<StorePg, String> {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let schema = format!(
            "pw_store_test_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        );
        let mut layer = StorePg::open_with(url, &schema, isolation, seats)?;
        layer.owned = true;
        Ok(layer)
    }

    /// The test's schema's name.
    #[cfg(test)]
    pub(crate) fn schema(&self) -> &str {
        &self.pool.schema
    }

    /// **A connection in the layer's schema**, outside it, to look at what
    /// was committed and to arrange what a test needs there.
    #[cfg(test)]
    pub(crate) fn sql(&self) -> Client {
        self.pool.connect().expect("connected")
    }

    /// **Every connection the pool's bound allows, taken and held** (NEXT
    /// 4c's tests): what a single read and a command wait on, and a render,
    /// which reads in a seat of the renders' own, does not.
    #[cfg(test)]
    pub(crate) fn hold_the_bound(&self) -> Vec<Box<dyn std::any::Any + Send>> {
        (0..CONNECTIONS)
            .map(|_| {
                Box::new(self.pool.take().expect("a connection")) as Box<dyn std::any::Any + Send>
            })
            .collect()
    }
}

#[cfg(test)]
impl Drop for StorePg {
    fn drop(&mut self) {
        if !self.owned {
            return;
        }
        // Every connection closed first, so none holds what is dropped.
        self.pool.idle.lock().expect("idle connections").0.clear();
        if let Ok(mut c) = Client::connect(&self.pool.url, NoTls) {
            let _ = c.batch_execute(&format!(
                "SET lock_timeout = '10s'; DROP SCHEMA IF EXISTS {} CASCADE",
                ident(&self.pool.schema)
            ));
        }
    }
}

/// **A command's transaction** (ADR-0218, ADR-0246): its connection, in
/// the transaction `begin` opened, from the call to the commit.
pub(crate) struct Staging<'a> {
    queue_authority: &'a pw_materialize::QueueAuthority,
    _writes: std::sync::MutexGuard<'a, ()>,
    session: String,
    /// The connection, given back to the pool when the staging is dropped.
    conn: Arc<Mutex<Option<Conn>>>,
    written: Arc<Mutex<Written>>,
    /// Whether COMMIT was acknowledged. Drop attempts rollback otherwise;
    /// transport loss can still leave the authoritative outcome unknown.
    committed: bool,
    ops: Ops,
    /// The layer's tally, and this command's place in it (NEXT 4b).
    tally: Tally,
    txn: Option<usize>,
    /// The schema's outbox, which its rows are ordered in, and this host's
    /// id, which they carry (ADR-0369).
    outbox: &'a crate::hosts::Outbox,
    host: &'a str,
}

impl crate::data::Staged for Staging<'_> {
    fn projection_scope(&self) -> crate::data::ProjectionScope {
        crate::data::ProjectionScope::Session
    }
    fn ops(&self) -> Ops {
        self.ops.clone()
    }

    fn rows(&self) -> Option<Vec<(String, String)>> {
        // What the host's materializer records once the database committed:
        // the cart's total, as the in-memory layer's commit writes it.
        self.written.lock().expect("written").rows(&self.session)
    }

    fn publish(&mut self) {
        // Committed in `commit`: there is nothing left to make the layer's.
    }

    fn commit(
        &mut self,
        events: &[Handed],
        invalidated: &[Dropped],
        interaction: Option<&crate::data::Recorded<'_>>,
        scheduled: &[crate::schedules::Scheduled],
        completing: Option<crate::schedules::Completing<'_>>,
    ) -> Result<crate::data::Commit, String> {
        // The commit's statements are the command's (the integrator's ruling
        // on NEXT 4b): what it handed the outbox, its interaction's record,
        // the runs it scheduled or cancelled, and the outbox read back, each
        // bounded by what the command hands the platform, never by the
        // data's size.
        let (tally, txn) = (self.tally.clone(), self.txn);
        tally.around_in(txn, || {
            self.committed_with(events, invalidated, interaction, scheduled, completing)
        })
    }
}

impl Staging<'_> {
    fn committed_with(
        &mut self,
        events: &[Handed],
        invalidated: &[Dropped],
        interaction: Option<&crate::data::Recorded<'_>>,
        scheduled: &[crate::schedules::Scheduled],
        completing: Option<crate::schedules::Completing<'_>>,
    ) -> Result<crate::data::Commit, String> {
        let mut conn = self.conn.lock().expect("the command's connection");
        let c = conn
            .as_mut()
            .ok_or("the command's transaction did not open")?
            .client();
        if let Some(completion) = completing {
            if !completion.belongs_to(self.queue_authority) {
                return Err("scheduled completion belongs to another queue adapter".into());
            }
            match crate::schedules::pg::consume(c, completion.run, completion.now_ms, || {
                Counted::ran(1)
            })
            .map_err(pg)?
            {
                crate::schedules::pg::Fence::Consumed => {}
                crate::schedules::pg::Fence::NotCurrent => {
                    return Ok(crate::data::Commit::NotCurrent);
                }
                crate::schedules::pg::Fence::Refused => {
                    return Err("scheduled completion removed no current row".into());
                }
            }
        }
        // **The interaction's answer, with the writes** (ADR-0325).
        if let Some(r) = interaction {
            crate::feed_pg::record(c, r)?;
            // Its two: the expired records deleted, the answer inserted.
            Counted::ran(2);
        }
        // **And the runs it scheduled, or cancelled** (the
        // scheduled-commands track).
        crate::schedules::pg::write(c, scheduled).map_err(pg)?;
        // Its statements: a delete for each, and an insert for each run due.
        Counted::ran(
            scheduled
                .iter()
                .map(|s| 1 + u64::from(s.due_ms.is_some()))
                .sum(),
        );
        // What the command handed the outbox, in its transaction: an
        // invalidated entry's `_` as `null` (ADR-0256).
        let handed: Vec<_> = events
            .iter()
            .map(|(name, values)| {
                (
                    "event",
                    name,
                    values.iter().cloned().map(Some).collect::<Vec<_>>(),
                )
            })
            .chain(
                invalidated
                    .iter()
                    .map(|(name, values)| ("invalidation", name, values.clone())),
            )
            .collect();
        // **Last, and in commit order** (ADR-0369): the outbox's lock, held
        // to the commit's end, before its rows take their ids, so every host
        // reads them in the order the commits were made; and each host
        // listening woken at the commit. One statement.
        if !handed.is_empty() {
            crate::hosts::ordered(c, self.outbox).map_err(pg)?;
            Counted::ran(1);
        }
        let mut ids: Vec<i64> = Vec::new();
        for (kind, name, values) in handed {
            let args = json_of(name, &values)?;
            let row = Counted(c)
                .query_one(
                    "INSERT INTO outbox (kind, name, args, host) \
                     VALUES ($1, $2, $3::text::jsonb, $4) RETURNING id",
                    &[&kind, name, &args, &self.host],
                )
                .map_err(pg)?;
            ids.push(row.get(0));
        }
        // The writes and the outbox's rows, at once or not at all. A commit
        // the database refuses (a serialization failure, a deferred
        // constraint) has rolled back, and nothing is delivered.
        c.batch_execute("COMMIT").map_err(pg)?;
        self.committed = true;
        // The outbox's rows read back, one statement.
        Counted::ran(1);
        match delivered(c, &ids) {
            Ok(read) => Ok(crate::data::Commit::Applied(read)),
            // Committed, and not read back: what it handed is what
            // committed, and the other hosts read its rows from their
            // cursors; the command is not answered as refused.
            Err(why) => {
                eprintln!("pw dev server: the store's outbox was not read back: {why}");
                Ok(crate::data::Commit::Applied((
                    events.to_vec(),
                    invalidated.to_vec(),
                )))
            }
        }
    }
}

impl Drop for Staging<'_> {
    fn drop(&mut self) {
        let Some(mut conn) = self.conn.lock().expect("the command's connection").take() else {
            return;
        };
        // Roll back an unacknowledged transaction when possible. A transport
        // failure around COMMIT may have committed; queue/receipt state reconciles it.
        if !self.committed && conn.client().batch_execute("ROLLBACK").is_err() {
            conn.discard();
        }
    }
}

/// **Its runs, outside a command** (the scheduled-commands track): those
/// its `schedules` keeps, each statement on a connection of its own.
impl crate::schedules::Runs for StorePg {
    fn authority(&self) -> &pw_materialize::QueueAuthority {
        &self.queue_authority
    }
    fn current(
        &self,
        expected: &pw_materialize::ScheduledRun,
        now_ms: i64,
    ) -> Result<bool, String> {
        let mut conn = self.pool.take()?;
        crate::schedules::pg::current(conn.client(), expected, now_ms).map_err(pg)
    }
    fn retry_current(
        &self,
        expected: &pw_materialize::ScheduledRun,
        attempts: i64,
        due_ms: i64,
        failed: Option<&str>,
    ) -> Result<bool, String> {
        let _writes = self.writes.lock().unwrap_or_else(|e| e.into_inner());
        let mut conn = self.pool.take()?;
        crate::schedules::pg::retry_current(conn.client(), expected, attempts, due_ms, failed)
            .map_err(pg)
    }
    fn due_runs(
        &self,
        now_ms: i64,
        limit: usize,
    ) -> Result<Vec<pw_materialize::ScheduledRun>, String> {
        let mut conn = self.pool.take()?;
        crate::schedules::pg::due_runs(conn.client(), now_ms, limit).map_err(pg)
    }

    fn next_run(&self) -> Result<Option<i64>, String> {
        let mut conn = self.pool.take()?;
        crate::schedules::pg::next_run(conn.client()).map_err(pg)
    }

    #[cfg(test)]
    fn ran(&self, id: i64) -> Result<(), String> {
        let _writes = self.writes.lock().unwrap_or_else(|e| e.into_inner());
        let mut conn = self.pool.take()?;
        crate::schedules::pg::ran(conn.client(), id).map_err(pg)
    }

    #[cfg(test)]
    fn retry(
        &self,
        id: i64,
        attempts: i64,
        due_ms: i64,
        failed: Option<&str>,
    ) -> Result<(), String> {
        let _writes = self.writes.lock().unwrap_or_else(|e| e.into_inner());
        let mut conn = self.pool.take()?;
        crate::schedules::pg::retry(conn.client(), id, attempts, due_ms, failed).map_err(pg)
    }

    #[cfg(test)]
    fn all_runs(&self) -> Result<Vec<pw_materialize::ScheduledRun>, String> {
        let mut conn = self.pool.take()?;
        crate::schedules::pg::all_runs(conn.client()).map_err(pg)
    }
}

impl crate::data::DataLayer for StorePg {
    fn keeps_interactions(&self) -> bool {
        true
    }

    fn recalled(&self, key: &str, since_ms: u64) -> Result<Option<(String, String)>, String> {
        let mut conn = self.pool.take()?;
        crate::feed_pg::recalled(conn.client(), key, since_ms).map_err(pg)
    }

    #[cfg(test)]
    fn statements(&self) -> Option<u64> {
        Some(self.tally.get())
    }

    #[cfg(test)]
    fn statements_by_op(&self) -> BTreeMap<String, (u64, u64)> {
        self.tally.by_op()
    }

    #[cfg(test)]
    fn statements_by_transaction(&self) -> Vec<u64> {
        self.tally.transactions()
    }

    #[cfg(test)]
    fn execute_for_test(&self, sql: &str) -> Result<(), String> {
        self.sql().batch_execute(sql).map_err(pg)
    }

    fn runs(&self) -> Option<&dyn crate::schedules::Runs> {
        Some(self)
    }

    fn tail(&self) -> Option<&crate::hosts::Tail> {
        Some(&self.tail)
    }

    fn sessions(&self) -> Option<Arc<dyn crate::identity::Kept>> {
        Some(self.sessions.clone())
    }

    fn instance(&self) -> Option<u64> {
        Some(self.instance)
    }

    fn reads(&self, session: &str, stopped: Option<Stopped>) -> Ops {
        let (pool, tally) = (self.pool.clone(), self.tally.clone());
        let with: With = Arc::new(move |f| {
            let mut conn = pool.take()?;
            let c = conn.client();
            c.batch_execute("BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY")
                .map_err(pg)?;
            let out = tally.around(|| f(&mut PgRows(Counted(c))));
            if c.batch_execute("COMMIT").is_err() {
                conn.discard();
            }
            out
        });
        self.tally.by_name(
            crate::store::reads(with, session, &self.faults, stopped),
            "read",
        )
    }

    // TRACK SEAM (render-merge): a render's snapshot, its seat (waited for
    // while every seat is taken) and the read transaction it holds, and each
    // of its queries' reads in it, counted as a query's are (NEXT 4c).
    fn snapshot(&self) -> Option<crate::data::Snapshot> {
        Some(Box::new(Arc::new(self.renders.seat())))
    }

    fn reads_in(
        &self,
        snapshot: &crate::data::Snapshot,
        session: &str,
        stopped: Option<Stopped>,
    ) -> Ops {
        // Never another layer's: a render's snapshot is its layer's
        // (`render_merge`, by the layer's address).
        let Some(seat) = snapshot.downcast_ref::<Arc<crate::feed_pg::Seat>>() else {
            return self.reads(session, stopped);
        };
        let (seat, tally) = (seat.clone(), self.tally.clone());
        let with: With =
            Arc::new(move |f| seat.read(|c| tally.around(|| f(&mut PgRows(Counted(c))))));
        self.tally.by_name(
            crate::store::reads(with, session, &self.faults, stopped),
            "read",
        )
    }

    /// The renders' connections (NEXT 4c's tests).
    #[cfg(test)]
    fn renders_for_test(&self) -> Option<Arc<crate::feed_pg::Renders>> {
        Some(self.renders.clone())
    }

    fn begin<'a>(&'a self, session: &str) -> Box<dyn crate::data::Staged + 'a> {
        let writes = self.writes.lock().unwrap_or_else(|e| e.into_inner());
        let fail = self.faults.fail_next.swap(false, Ordering::SeqCst);
        // The transaction, open before the command runs: what it reads, it
        // reads in it. A connection or a `BEGIN` that fails leaves none, and
        // every operation of the command fails with why.
        let opened = self.pool.take().and_then(|mut conn| {
            match begin_command(conn.client(), self.isolation) {
                Ok(()) => Ok(conn),
                Err(e) => Err(pg(e)),
            }
        });
        let (conn, why) = match opened {
            Ok(c) => (Some(c), String::new()),
            Err(why) => (None, why),
        };
        let conn = Arc::new(Mutex::new(conn));
        let txn = self.tally.begin();
        let with: With = {
            let (conn, tally) = (conn.clone(), self.tally.clone());
            Arc::new(
                move |f| match conn.lock().expect("the command's connection").as_mut() {
                    Some(c) => tally.around_in(txn, || f(&mut PgRows(Counted(c.client())))),
                    None => Err(why.clone()),
                },
            )
        };
        let written: Arc<Mutex<Written>> = Arc::default();
        let ops = self.tally.by_name(
            crate::store::writes(with, session, &self.faults, fail, written.clone()),
            "command",
        );
        Box::new(Staging {
            queue_authority: &self.queue_authority,
            _writes: writes,
            session: session.to_string(),
            conn,
            written,
            committed: false,
            ops,
            tally: self.tally.clone(),
            txn,
            outbox: self.tail.outbox(),
            host: self.tail.host(),
        })
    }

    fn grants(&self) -> Vec<&'static str> {
        crate::store::grants().to_vec()
    }

    fn default_page(&self) -> Option<&'static str> {
        Some(crate::store::StoreData::default_page())
    }

    fn session_entry(&self) -> bool {
        true
    }

    fn style(&self) -> &'static str {
        crate::STYLE
    }

    /// **What the database provides, measured** (ADR-0246), as the feed's
    /// is: a transaction opened as a command's is, asked its isolation and
    /// whether it may write, and rolled back.
    fn provides(&self) -> Result<Provided, String> {
        let mut conn = self.pool.take()?;
        let c = conn.client();
        let measured = begin_command(c, self.isolation).and_then(|()| {
            let row = c.query_one(
                "SELECT current_setting('transaction_isolation'), \
                        current_setting('transaction_read_only')",
                &[],
            );
            // Whatever the query did, the transaction ends here.
            c.batch_execute("ROLLBACK")?;
            row
        });
        let row = measured.map_err(pg)?;
        drop(conn);
        let (isolation, read_only): (String, String) = (row.get(0), row.get(1));
        if read_only == "on" {
            return Err(
                "the store's database opens a command's transaction read only: no write commits"
                    .to_string(),
            );
        }
        let transactions = match isolation.as_str() {
            "serializable" => "serializable",
            "repeatable read" => "snapshot",
            _ => "read_committed",
        };
        Ok(Provided {
            transactions: transactions.to_string(),
            reads: BTreeSet::from(["strong".to_string()]),
            feed: false,
        })
    }
}

impl crate::store::StoreLayer for StorePg {
    fn faults(&self) -> &Faults {
        &self.faults
    }

    #[cfg(test)]
    fn line_around(&self, owner: &str, at: usize, quantity: i64) -> Result<(), String> {
        let changed = self
            .sql()
            .execute(
                "UPDATE cart_lines SET quantity = $3 WHERE owner = $1 AND position = $2",
                &[&owner, &(at as i32), &quantity],
            )
            .map_err(pg)?;
        if changed == 0 {
            return Err("no such line".to_string());
        }
        Ok(())
    }
}
