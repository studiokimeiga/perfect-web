//! **Connections, bounded, each preparing a statement once** (ADR-0298's
//! bounded pool; ADR-0354's render seats).
//!
//! A [`Pool`] holds at most its bound of connections out at once: a taker
//! past it waits, for the pool's wait at most, and then fails saying so,
//! never opening one more. A connection is opened when first used, not when
//! its place is taken, so a render's seat is taken where it begins and costs
//! nothing until it reads (ADR-0354, decision 4). Each connection prepares a
//! statement the first time it runs it and keeps it (`Statement`), so every
//! later call is one round trip; both are counted ([`Counters`]).

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use postgres::{Client, NoTls, Row, Statement};

use crate::Error;
use crate::values::{Param, refs};

/// **What the adapter has asked of its database**: each statement it ran
/// (one round trip each: a prepared statement's call, or a simple query
/// such as `BEGIN`), and each statement it prepared (a round trip the first
/// time a connection runs a statement). ADR-0336's count, kept always: an
/// atomic add is nothing beside a round trip.
#[derive(Debug, Default)]
pub(crate) struct Counters {
    pub statements: AtomicU64,
    pub prepared: AtomicU64,
}

/// One connection and the statements it has prepared, by their text.
pub(crate) struct Conn {
    client: Client,
    prepared: HashMap<String, Statement>,
}

impl Conn {
    fn connect(url: &str) -> Result<Conn, Error> {
        let client = Client::connect(url, NoTls).map_err(crate::database)?;
        Ok(Conn {
            client,
            prepared: HashMap::new(),
        })
    }

    /// **`sql`, prepared once on this connection, run with `params`**: one
    /// statement.
    pub(crate) fn query(
        &mut self,
        counters: &Counters,
        sql: &str,
        params: &[Param],
    ) -> Result<Vec<Row>, postgres::Error> {
        let statement = match self.prepared.get(sql) {
            Some(statement) => statement.clone(),
            None => {
                counters.prepared.fetch_add(1, Ordering::Relaxed);
                let statement = self.client.prepare(sql)?;
                self.prepared.insert(sql.to_string(), statement.clone());
                statement
            }
        };
        counters.statements.fetch_add(1, Ordering::Relaxed);
        self.client.query(&statement, &refs(params))
    }

    /// `sql` as a simple query (`BEGIN`, `COMMIT`, a migration's DDL): one
    /// statement.
    pub(crate) fn batch(&mut self, counters: &Counters, sql: &str) -> Result<(), postgres::Error> {
        counters.statements.fetch_add(1, Ordering::Relaxed);
        self.client.batch_execute(sql)
    }

    pub(crate) fn is_closed(&self) -> bool {
        self.client.is_closed()
    }
}

/// The connections idle, and how many places are taken.
struct Places {
    idle: Vec<Conn>,
    taken: usize,
    /// The most places taken at once, so far.
    most: usize,
    /// Takers waiting for a place now.
    waiting: usize,
}

/// **A bounded pool of connections** to one database.
pub(crate) struct Pool {
    url: String,
    bound: usize,
    wait: Duration,
    what: &'static str,
    places: Mutex<Places>,
    freed: Condvar,
}

impl Pool {
    pub(crate) fn new(url: &str, bound: usize, wait: Duration, what: &'static str) -> Arc<Pool> {
        Arc::new(Pool {
            url: url.to_string(),
            bound,
            wait,
            what,
            places: Mutex::new(Places {
                idle: Vec::new(),
                taken: 0,
                most: 0,
                waiting: 0,
            }),
            freed: Condvar::new(),
        })
    }

    /// **A place in the pool**, once one is free: waited for while every
    /// place is taken, for the pool's wait at most, then refused, by name.
    /// Its connection is opened at its first use.
    pub(crate) fn take(self: &Arc<Self>) -> Result<Lease, Error> {
        let deadline = Instant::now() + self.wait;
        let mut places = self.places.lock().map_err(|_| Error::StoragePoisoned)?;
        while places.taken >= self.bound {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(Error::Unavailable(format!(
                    "waited {:?} for one of the {} {}",
                    self.wait, self.bound, self.what
                )));
            }
            places.waiting += 1;
            let waited = self.freed.wait_timeout(places, left);
            places = match waited {
                Ok((places, _)) => places,
                Err(_) => return Err(Error::StoragePoisoned),
            };
            places.waiting -= 1;
        }
        places.taken += 1;
        places.most = places.most.max(places.taken);
        Ok(Lease {
            pool: Arc::clone(self),
            conn: None,
        })
    }

    fn give_back(&self, conn: Option<Conn>) {
        let mut places = match self.places.lock() {
            Ok(places) => places,
            Err(poisoned) => poisoned.into_inner(),
        };
        places.taken -= 1;
        if let Some(conn) = conn.filter(|c| !c.is_closed()) {
            places.idle.push(conn);
        }
        drop(places);
        self.freed.notify_one();
    }

    /// (places taken now, the most taken at once, takers waiting now).
    pub(crate) fn census(&self) -> (usize, usize, usize) {
        match self.places.lock() {
            Ok(p) => (p.taken, p.most, p.waiting),
            Err(p) => {
                let p = p.into_inner();
                (p.taken, p.most, p.waiting)
            }
        }
    }
}

/// **A place in a pool**, and its connection once used; given back when
/// dropped, its connection kept idle unless it was [`Lease::discard`]ed or
/// closed.
pub(crate) struct Lease {
    pool: Arc<Pool>,
    conn: Option<Conn>,
}

impl Lease {
    /// Its connection: an idle one of the pool's, or a new one.
    pub(crate) fn conn(&mut self) -> Result<&mut Conn, Error> {
        if self.conn.is_none() {
            let idle = {
                let mut places = self
                    .pool
                    .places
                    .lock()
                    .map_err(|_| Error::StoragePoisoned)?;
                let mut found = None;
                while let Some(c) = places.idle.pop() {
                    if !c.is_closed() {
                        found = Some(c);
                        break;
                    }
                }
                found
            };
            self.conn = Some(match idle {
                Some(c) => c,
                None => Conn::connect(&self.pool.url)?,
            });
        }
        self.conn.as_mut().ok_or(Error::StoragePoisoned)
    }

    /// Whether it has opened its connection.
    pub(crate) fn connected(&self) -> bool {
        self.conn.is_some()
    }

    /// Its connection closed, not kept: one whose transaction it could not
    /// end, or whose state is unknown.
    pub(crate) fn discard(&mut self) {
        self.conn = None;
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        self.pool.give_back(self.conn.take());
    }
}
