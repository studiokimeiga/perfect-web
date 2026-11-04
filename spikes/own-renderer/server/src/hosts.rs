//! **Hosts sharing one database** (ADR-0369, track `second-host`).
//!
//! Each host on a PostgreSQL layer tells its own pages of every commit,
//! whichever host made it:
//!
//! - **A commit's outbox rows are numbered in commit order.** Before it takes
//!   its ids, a command's transaction takes the outbox's lock, held to its
//!   end ([`ordered`]): a transaction that inserts later takes its ids later
//!   and commits later, so a row a reader sees means every smaller id is
//!   final, committed or rolled back for good. PostgreSQL's sequences are
//!   not that by themselves: a value "is not reclaimed for re-use if the
//!   calling transaction later aborts", and it is taken at the insert, not
//!   at the commit.
//! - **The outbox is a log, read by each host from its cursor** ([`Tail`]):
//!   the rows after it, in id order, those of another host's commits told
//!   to this host's pages, its own passed by (its committing thread told
//!   them at once). Rows are trimmed by age, never by a reader; a host whose
//!   cursor fell below what was trimmed tells its pages to reload.
//! - **`NOTIFY` only wakes the readers.** PostgreSQL delivers it "until and
//!   unless the transaction is committed", and only to a session listening
//!   then: a host whose connection dropped misses it, and reads the rows
//!   from its cursor anyway, at least every [`WAIT_MS`].
//!
//! The in-memory layers are one host by definition: their data is the
//! process's, and they have none of this.

use crate::data::Outboxed;
use crate::feed_pg::{ident, pg};
use postgres::{Client, NoTls};
use std::sync::Mutex;

/// **How long a reader waits for a notification before it reads anyway**:
/// what a notification lost to a dropped connection costs at most.
pub(crate) const WAIT_MS: u64 = 1000;

/// **How long a commit's rows are kept** for the hosts that have not read
/// them, on the database's clock.
pub(crate) const KEPT: &str = "10 minutes";

/// **How many rows a reader takes at once.**
const BATCH: i64 = 512;

/// **A host's id**: 128 bits from the operating system's random source,
/// made when the host opens a layer and never reused, written with each
/// outbox row its commits write.
pub(crate) fn new_host() -> String {
    format!(
        "h-{}",
        crate::identity::hex(&crate::identity::random_bytes::<16>())
    )
}

/// **Where a schema's outbox is ordered and heard**: its lock's key and its
/// channel, each named by the schema, so that two schemas in one database
/// (the feed's and the store's, or two tests') neither wait on nor wake each
/// other. A channel is an identifier PostgreSQL truncates past 63 bytes, so
/// it is the schema's SHA-256's first 64 bits, not its name.
#[derive(Debug, Clone)]
pub(crate) struct Outbox {
    pub schema: String,
    pub lock: String,
    pub channel: String,
    // Trusted platform names, selected only by these constructors.
    rows: &'static str,
    watermark: &'static str,
    instances: &'static str,
    // Generic tables already order their log under this exact i64 key.
    table_lock: Option<i64>,
    trim_limit: Option<i64>,
}

impl Outbox {
    pub(crate) fn of(schema: &str) -> Outbox {
        use sha2::Digest;
        let digest = sha2::Sha256::digest(schema.as_bytes());
        Outbox {
            schema: schema.to_string(),
            lock: format!("pw_outbox:{schema}"),
            channel: format!("pw_outbox_{}", crate::identity::hex(&digest[..8])),
            rows: "outbox",
            watermark: "outbox_trimmed",
            instances: "host_instances",
            table_lock: None,
            trim_limit: None,
        }
    }

    /// The generic adapter's platform namespace and its own ordering key.
    /// Application declarations cannot supply these SQL names.
    pub(crate) fn for_tables(schema: &str, channel: &str, lock: i64) -> Outbox {
        Outbox {
            schema: schema.to_string(),
            lock: format!("pw_outbox:{schema}"),
            channel: channel.to_string(),
            rows: "pw_outbox",
            watermark: "pw_outbox_trimmed",
            instances: "pw_host_instances",
            table_lock: Some(lock),
            trim_limit: Some(500),
        }
    }

    /// A test holds the publisher's actual ordering lock without sending a
    /// wakeup of its own, so it can distinguish commit order from notification.
    #[cfg(test)]
    pub(crate) fn hold_order_for_test(&self, c: &mut Client) -> Result<(), postgres::Error> {
        match self.table_lock {
            Some(key) => c.execute("SELECT pg_advisory_xact_lock($1)", &[&key]),
            None => c.execute("SELECT pg_advisory_xact_lock(hashtext($1))", &[&self.lock]),
        }
        .map(|_| ())
    }

    /// The outbox of the schema `c` searches first.
    pub(crate) fn current(c: &mut Client) -> Result<Outbox, String> {
        let schema: String = c
            .query_one("SELECT current_schema()::text", &[])
            .map_err(pg)?
            .get(0);
        Ok(Outbox::of(&schema))
    }
}

/// **The outbox's rows, in commit order** (ADR-0369): the lock taken in the
/// command's transaction, held to its end, before its rows take their ids;
/// and every host listening woken at the commit, and not before. One
/// statement, run only by a commit that hands the outbox anything.
pub(crate) fn ordered(c: &mut Client, outbox: &Outbox) -> Result<(), postgres::Error> {
    if let Some(key) = outbox.table_lock {
        return c
            .execute(
                "SELECT pg_advisory_xact_lock($1), pg_notify($2, '')",
                &[&key, &outbox.channel],
            )
            .map(|_| ());
    }
    c.execute(
        "SELECT pg_advisory_xact_lock(hashtext($1)), pg_notify($2, '')",
        &[&outbox.lock, &outbox.channel],
    )
    .map(|_| ())
}

/// **What a reader heard since its cursor**: each commit of another host,
/// in commit order, with the last id it read; and whether rows it had not
/// read were trimmed first.
#[derive(Debug, Default)]
pub(crate) struct Heard {
    pub commits: Vec<(i64, Outboxed)>,
    /// Rows past the cursor were trimmed before it read them: what they
    /// said can no longer be told.
    pub missed: bool,
    /// The last id read, its own rows' among them.
    pub through: i64,
    /// Whether it read as many rows as it takes at once, so more may wait.
    pub more: bool,
}

/// One outbox row as a reader keeps it: its id, its kind, its name and its
/// values' text.
type OutboxRow = (i64, String, String, String);

/// **A commit a reader is gathering**: its transaction, the last id read of
/// it, and its rows so far.
type Gathered = (String, i64, Vec<OutboxRow>);

/// **A host's reader of the outbox** (ADR-0369): its cursor, the last id it
/// has delivered or passed by, and a connection of its own that listens on
/// the schema's channel and reads the rows after the cursor.
pub(crate) struct Tail {
    url: String,
    outbox: Outbox,
    host: String,
    cursor: Mutex<i64>,
    /// The listening connection, made again when it drops.
    conn: Mutex<Option<Client>>,
    /// When it last trimmed.
    trimmed: Mutex<Option<std::time::Instant>>,
    /// A test's step between [`Tail::heard`]'s two reads, run once.
    #[cfg(test)]
    between: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}

/// **`PW_FEED_SCHEMA`, a test's setting** (the integrator's ruling 4 of
/// 2026-10-20 on this track): the schema a test run's hosts keep the feed's
/// tables in, honored only for a development deployment on this machine, as
/// the development identity provider is, and refused for any other, whose
/// tables are where its connection's search path says.
pub(crate) fn test_schema(
    schema: Option<&str>,
    deployment: &crate::identity::Deployment,
) -> Result<Option<String>, String> {
    match schema.filter(|s| !s.is_empty()) {
        None => Ok(None),
        Some(s) if deployment.development && deployment.loopback() => Ok(Some(s.to_string())),
        Some(_) => Err(
            "PW_FEED_SCHEMA is a test's setting, for a development deployment on this \
             machine, and this deployment is not one"
                .to_string(),
        ),
    }
}

// ---------------------------------------------------------------------------
// Pages numbered apart on every host (the integrator's ruling 1, 2026-10-20)
// ---------------------------------------------------------------------------

/// **How many low bits of a page's number its host counts its pages in**:
/// the bits above them name the host's instance, and the whole stays below
/// JavaScript's exact integers (2^53), as the runtime reads the number.
const PAGE_COUNT_BITS: u32 = 32;

/// **How many instances page numbers tell apart**: 2^20 - 1, from one.
const PAGE_INSTANCES: u64 = (1 << 20) - 1;

/// **A host's instance, taken from the database it shares** as it opens a
/// layer: the next of the schema's `host_instances`, so no two hosts on one
/// database have one alike, until a million hosts' starts have gone by.
pub(crate) fn instance(c: &mut Client) -> Result<u64, String> {
    instance_named(c, "host_instances")
}

pub(crate) fn instance_in(c: &mut Client, outbox: &Outbox) -> Result<u64, String> {
    instance_named(c, outbox.instances)
}

fn instance_named(c: &mut Client, sequence: &str) -> Result<u64, String> {
    let n: i64 = c
        .query_one("SELECT nextval($1::text::regclass)", &[&sequence])
        .map_err(pg)?
        .get(0);
    Ok((u64::try_from(n).unwrap_or(1).max(1) - 1) % PAGE_INSTANCES + 1)
}

/// **A host's first page number**: its instance in the high bits, so that a
/// page another host on the database served never has a number this one
/// gives, and none is taken for another's (a page of a session's on A
/// numbered as one of its pages on B was taken for B's own); 1 where the
/// host has no instance, its data in its memory, one host.
pub(crate) fn first_page(instance: Option<u64>) -> u64 {
    instance.map_or(0, |i| i << PAGE_COUNT_BITS) | 1
}

/// **Whether `page` is another host's**: its number in another host's
/// instance than the one this host's `first` page is in. A number in no
/// instance (one no host on a database gives) is no other host's: it is a
/// page this host does not hold.
pub(crate) fn another_hosts(page: u64, first: u64) -> bool {
    let (theirs, mine) = (page >> PAGE_COUNT_BITS, first >> PAGE_COUNT_BITS);
    theirs != 0 && theirs != mine
}

/// **The outbox's highest id**, read on `c`, a connection searching its
/// schema: where a reader opened now starts.
pub(crate) fn highest(c: &mut Client) -> Result<i64, String> {
    c.query_one("SELECT coalesce(max(id), 0)::bigint FROM outbox", &[])
        .map(|r| r.get(0))
        .map_err(pg)
}

/// A new generic host starts after retained and already-trimmed history.
/// Its first documents read the current database; old events need no replay.
pub(crate) fn highest_in(c: &mut Client, outbox: &Outbox) -> Result<i64, String> {
    c.query_one(&format!(
        "SELECT greatest(coalesce((SELECT max(id) FROM {}), 0), (SELECT through FROM {}))::bigint",
        outbox.rows, outbox.watermark
    ), &[]).map(|row| row.get(0)).map_err(pg)
}

impl Tail {
    /// **A reader opened with its layer**: its cursor at `highest`, the
    /// outbox's highest id ([`highest`]), which the layer read on a
    /// connection of its own as it opened. The host has no page yet, and
    /// every page it serves reads the database as it is then. Its listening
    /// connection is made by its first read, not here: a host connects what
    /// it uses, and a layer whose host never reads the outbox (a test's)
    /// holds none. Nothing is lost meanwhile: the first read listens, then
    /// reads every row after the cursor.
    pub(crate) fn at(url: &str, outbox: Outbox, host: &str, highest: i64) -> Tail {
        Tail {
            url: url.to_string(),
            outbox,
            host: host.to_string(),
            cursor: Mutex::new(highest),
            conn: Mutex::new(None),
            trimmed: Mutex::new(None),
            #[cfg(test)]
            between: Mutex::new(None),
        }
    }

    /// [`Tail::at`] the outbox's highest id, read on the reader's own
    /// connection: a test's reader beside the hosts'.
    #[cfg(test)]
    pub(crate) fn open(url: &str, outbox: Outbox, host: &str) -> Result<Tail, String> {
        let tail = Tail::at(url, outbox, host, 0);
        let highest = tail.with(|c| {
            c.query_one(
                &format!(
                    "SELECT coalesce(max(id), 0)::bigint FROM {}",
                    tail.outbox.rows
                ),
                &[],
            )
            .map(|r| r.get::<_, i64>(0))
        })?;
        *tail.cursor.lock().expect("cursor") = highest;
        Ok(tail)
    }

    /// Whether its listening connection is open.
    #[cfg(test)]
    pub(crate) fn listening(&self) -> bool {
        self.conn
            .lock()
            .expect("the tail's connection")
            .as_ref()
            .is_some_and(|c| !c.is_closed())
    }

    /// This host's id, which its own rows carry.
    pub(crate) fn host(&self) -> &str {
        &self.host
    }

    /// The schema's outbox it reads.
    pub(crate) fn outbox(&self) -> &Outbox {
        &self.outbox
    }

    /// The backend of its listening connection, which a test ends.
    #[cfg(test)]
    pub(crate) fn backend_pid(&self) -> Result<i32, String> {
        self.with(|c| {
            c.query_one("SELECT pg_backend_pid()", &[])
                .map(|r| r.get(0))
        })
    }

    /// The last id delivered or passed by.
    pub(crate) fn cursor(&self) -> i64 {
        *self.cursor.lock().expect("cursor")
    }

    /// **Its connection**, listening on the schema's channel, made again
    /// when it dropped. A connection made again listens first and reads
    /// after (PostgreSQL's advice for `LISTEN`), so nothing committed
    /// between the two is lost.
    fn with<T>(
        &self,
        f: impl FnOnce(&mut Client) -> Result<T, postgres::Error>,
    ) -> Result<T, String> {
        let mut conn = self.conn.lock().expect("the tail's connection");
        if conn.as_ref().is_none_or(Client::is_closed) {
            let mut c = Client::connect(&self.url, NoTls).map_err(pg)?;
            c.batch_execute(&format!(
                "SET search_path TO {}; LISTEN {}",
                ident(&self.outbox.schema),
                ident(&self.outbox.channel)
            ))
            .map_err(pg)?;
            *conn = Some(c);
        }
        let c = conn.as_mut().expect("connected");
        let out = f(c);
        if out.is_err() && c.is_closed() {
            *conn = None;
        }
        out.map_err(pg)
    }

    /// **The rows after its cursor**: another host's commits, each one by
    /// its transaction, in commit order; its own passed by. It moves
    /// nothing: the host moves the cursor once it told each
    /// ([`Tail::passed`]).
    ///
    /// The rows are read first, and what was trimmed after them: a trim
    /// that comes between the two reads leaves its mark past the cursor, and
    /// the host's pages reload, at worst for rows it did read. Read the other
    /// way round, a trim between them deletes rows past the cursor while the
    /// mark read before it says none was missed, and they are passed by in
    /// silence.
    pub(crate) fn heard(&self) -> Result<Heard, String> {
        let cursor = self.cursor();
        let (rows, trimmed) = self.with(|c| {
            let rows = c.query(
                &format!(
                    "SELECT id, host, committed_in::text, kind, name, args::text FROM {} \
                 WHERE id > $1 ORDER BY id LIMIT $2",
                    self.outbox.rows
                ),
                &[&cursor, &BATCH],
            )?;
            #[cfg(test)]
            self.between_reads();
            let trimmed: i64 = c
                .query_one(
                    &format!("SELECT through FROM {}", self.outbox.watermark),
                    &[],
                )?
                .get(0);
            Ok((rows, trimmed))
        })?;
        let mut heard = Heard {
            missed: trimmed > cursor,
            through: cursor.max(trimmed),
            more: rows.len() as i64 >= BATCH,
            ..Heard::default()
        };
        // A commit's rows are consecutive ids (its lock), and share its
        // transaction.
        let mut commit: Option<Gathered> = None;
        let done = |commit: Option<Gathered>, heard: &mut Heard| -> Result<(), String> {
            if let Some((_, last, read)) = commit {
                heard
                    .commits
                    .push((last, crate::feed_pg::outboxed_of(read)?));
            }
            Ok(())
        };
        for row in &rows {
            let (id, host, txn): (i64, String, String) = (row.get(0), row.get(1), row.get(2));
            heard.through = heard.through.max(id);
            // Its own, which its committing thread told at once; and any at
            // or below what was trimmed, which its pages reload for.
            if host == self.host || id <= trimmed {
                continue;
            }
            let same = commit.as_ref().is_some_and(|(t, _, _)| *t == txn);
            if !same {
                done(commit.take(), &mut heard)?;
                commit = Some((txn.clone(), id, Vec::new()));
            }
            let (_, last, read) = commit.as_mut().expect("a commit");
            *last = id;
            read.push((id, row.get(3), row.get(4), row.get(5)));
        }
        done(commit, &mut heard)?;
        Ok(heard)
    }

    /// A test's `step` run once between [`Tail::heard`]'s two reads.
    #[cfg(test)]
    pub(crate) fn between_reads_once(&self, step: impl FnOnce() + Send + 'static) {
        *self.between.lock().expect("between") = Some(Box::new(step));
    }

    #[cfg(test)]
    fn between_reads(&self) {
        let step = self.between.lock().expect("between").take();
        if let Some(step) = step {
            step();
        }
    }

    /// **Its cursor moved past `id`**, once what it read through `id` was
    /// told. Never back.
    pub(crate) fn passed(&self, id: i64) {
        let mut cursor = self.cursor.lock().expect("cursor");
        *cursor = (*cursor).max(id);
    }

    /// **Wait for a commit**: a notification on the schema's channel, or
    /// `ms` without one. A connection that dropped is made again by the next
    /// read, which reads from the cursor: what was said meanwhile is lost,
    /// what was committed is not.
    pub(crate) fn wait(&self, ms: u64) {
        let waited = self.with(|c| {
            use postgres::fallible_iterator::FallibleIterator;
            let first = c
                .notifications()
                .timeout_iter(std::time::Duration::from_millis(ms))
                .next()?;
            // And whatever else is already waiting: one read answers all.
            if first.is_some() {
                let mut waiting = c.notifications();
                let mut waiting = waiting.iter();
                while waiting.next()?.is_some() {}
            }
            Ok(())
        });
        if let Err(why) = waited {
            eprintln!("pw dev server: the outbox's listener: {why}");
            std::thread::sleep(std::time::Duration::from_millis(ms));
        }
    }

    /// **Rows older than [`KEPT`] trimmed**, at most once a minute, and the
    /// highest id trimmed recorded, which a reader below it learns it missed.
    pub(crate) fn trim(&self) {
        {
            let mut trimmed = self.trimmed.lock().expect("trimmed");
            if trimmed.is_some_and(|t| t.elapsed() < std::time::Duration::from_secs(60)) {
                return;
            }
            *trimmed = Some(std::time::Instant::now());
        }
        if let Err(why) = self.trim_older_than(KEPT) {
            eprintln!("pw dev server: the outbox was not trimmed: {why}");
        }
    }

    /// [`Tail::trim`], now, of rows older than `age` (an interval's text).
    pub(crate) fn trim_older_than(&self, age: &str) -> Result<(), String> {
        self.with(|c| {
            c.execute(
                &format!("WITH gone AS (DELETE FROM {rows} WHERE id IN (SELECT id FROM {rows} \
                 WHERE created < clock_timestamp() - $1::text::interval ORDER BY id LIMIT $2 \
                 FOR UPDATE SKIP LOCKED) RETURNING id) \
                 UPDATE {watermark} SET through = greatest(through, coalesce((SELECT max(id) FROM gone), 0))",
                 rows = self.outbox.rows, watermark = self.outbox.watermark),
                &[&age, &self.outbox.trim_limit],
            )
            .map(|_| ())
        })
    }
}

/// **Sessions and sign-ins kept in a layer's schema** (ADR-0369):
/// `sessions` and `sign_ins`, each by its secret's SHA-256
/// ([`crate::identity::kept_hash`]), on connections of their own, so that a
/// plain authority inspection does not lock a row. Open and close wait on
/// an admitted command's fence; an admission never does: it reads without
/// the session's lock, and writes only where nobody holds it (ADR-0389,
/// amended 2026-11-02). No more than [`SESSION_CONNECTIONS`] of these
/// connections are open at once.
pub(crate) struct SessionsPg {
    url: String,
    schema: String,
    /// The idle connections, and how many are open: idle, or taken.
    idle: Mutex<(Vec<Client>, usize)>,
    given_back: std::sync::Condvar,
    /// The statements it ran: one per request that reads its session.
    #[cfg(test)]
    pub(crate) ran: std::sync::atomic::AtomicU64,
    /// The most connections open at once.
    #[cfg(test)]
    most: std::sync::atomic::AtomicUsize,
    /// A test's step, run after each admission read and before any write it
    /// makes ([`crate::identity::Kept::after_each_read`]).
    #[cfg(test)]
    after_read: Mutex<Option<std::sync::Arc<dyn Fn() + Send + Sync>>>,
}

/// **How many connections a layer's sessions are read and written on at
/// once** (ADR-0369; the bound the render merge's ruling holds every pool
/// to: stated, and deadlock-free by structure): a request past it waits for
/// one to be given back. Each is taken for one inspection, one admission
/// (its read, and the write it may make without waiting) or one open/close
/// transaction. None is borrowed by a command staging: its fence runs on
/// the business connection, not a second session seat. None waits on a
/// command's fence for an admission, so a command holding its session's
/// lock holds none of them (ADR-0389, amended 2026-11-02).
pub(crate) const SESSION_CONNECTIONS: usize = 8;

/// **How long a kept session's use goes unrecorded**: a minute, or a
/// quarter of its idle time where that is shorter, so a session in use is
/// recorded well inside its idle time, and writes at most once a minute.
pub(crate) fn touched_after(lifetimes: &crate::identity::SessionLifetimes) -> std::time::Duration {
    std::time::Duration::from_secs(60).min(lifetimes.idle / 4)
}

/// **Whether a kept session is live**, by both its lifetimes on the
/// database's clock: opened less than its absolute lifetime ago (`$2`,
/// seconds) and used less than its idle time ago (`$3`). One text for an
/// admission's read and its write ([`ADMISSION_READ`], [`ADMISSION_WRITE`]),
/// so that the two cannot disagree on it.
macro_rules! live {
    () => {
        "opened > clock_timestamp() - make_interval(secs => $2) \
         AND seen > clock_timestamp() - make_interval(secs => $3)"
    };
}

/// **Whether a kept session's use is due to be recorded**: last recorded
/// more than [`touched_after`] ago (`$4`, seconds). One text for the read
/// and the write, as `live!`'s.
macro_rules! stale {
    () => {
        "seen < clock_timestamp() - make_interval(secs => $4)"
    };
}

/// **An admission's read** (ADR-0389, amended 2026-11-02): one statement,
/// run in no transaction of its own, that takes no session lock: the
/// session's row, whether it is live, and whether its use is due to be
/// recorded.
const ADMISSION_READ: &str = concat!(
    "SELECT user_id, handle, name, issuer, epoch, (",
    live!(),
    ") AS live, (",
    stale!(),
    ") AS stale FROM sessions WHERE hash = $1"
);

/// **An admission's write** (ADR-0369; the integrator's ruling 2 of
/// 2026-10-20), run only under the session's lock, its snapshot taken after
/// it: the session live by both lifetimes, its use recorded where that is
/// due, and one past either lifetime deleted. Its answer is the live row.
const ADMISSION_WRITE: &str = concat!(
    "WITH live AS (SELECT user_id, handle, name, issuer, seen, epoch FROM sessions \
     WHERE hash = $1 AND ",
    live!(),
    "), touched AS (UPDATE sessions SET seen = clock_timestamp() WHERE hash = $1 \
     AND EXISTS (SELECT 1 FROM live WHERE ",
    stale!(),
    ")), gone AS (DELETE FROM sessions WHERE hash = $1 \
     AND NOT EXISTS (SELECT 1 FROM live)) \
     SELECT user_id, handle, name, issuer, epoch FROM live"
);

/// One namespace shared by business staging and session open/close. The schema
/// isolates applications on one database; the hash never puts a cookie in SQL.
pub(crate) const SESSION_AUTHORITY_KEY: &str =
    "hashtextextended(current_schema() || ':pw_session:' || encode($1::bytea, 'hex'), 0)";
const SESSION_LOCK_TIMEOUT: &str = "SET LOCAL lock_timeout = '5s'";

/// **How long any statement on a session connection waits for a lock**, set
/// once per connection: the bound [`SESSION_LOCK_TIMEOUT`] sets in each
/// transaction, held too by what runs in none (ADR-0389, amended
/// 2026-11-02). An admission's read waits on no session lock, but a
/// migration's lock on the table would hold it: past the bound it fails, and
/// its request is refused, as when the admission was a transaction.
const SESSION_CONNECTION_LOCK_TIMEOUT: &str = "SET lock_timeout = '5s'";

/// Acquire outside the business transaction: its MVCC snapshot must start only
/// after this wait. An error leaves ownership uncertain: discard the connection.
/// Session advisory locks survive COMMIT, and are released explicitly on Drop.
pub(crate) fn command_session_lock(
    c: &mut Client,
    hash: &[u8],
    mut counted: impl FnMut(),
) -> Result<(), String> {
    counted();
    c.batch_execute("BEGIN").map_err(pg)?;
    counted();
    c.batch_execute(SESSION_LOCK_TIMEOUT).map_err(pg)?;
    counted();
    c.query_one(
        &format!("SELECT pg_advisory_lock({SESSION_AUTHORITY_KEY})"),
        &[&hash],
    )
    .map_err(pg)?;
    counted();
    c.batch_execute("COMMIT").map_err(pg)
}

/// Bound subsequent row/business/outbox lock waits in this transaction alone.
pub(crate) fn command_session_wait_bound(
    c: &mut Client,
    mut counted: impl FnMut(),
) -> Result<(), String> {
    counted();
    c.batch_execute(SESSION_LOCK_TIMEOUT).map_err(pg)
}

/// Lock even an expired row before validating its clock. An absent row is
/// protected by the advisory lock shared with open/close, including Anonymous.
pub(crate) fn command_session_row(
    c: &mut Client,
    hash: &[u8],
    mut counted: impl FnMut(),
) -> Result<(), String> {
    counted();
    c.query_opt(
        "SELECT 1 FROM sessions WHERE hash = $1 FOR UPDATE",
        &[&hash],
    )
    .map_err(pg)?;
    Ok(())
}

/// Current authority on the command's own transaction, checked after every
/// potentially blocking part of evaluation/publication and just before COMMIT.
/// Never substitutes a new actor for the immutable invocation's actor.
pub(crate) fn command_session_live(
    c: &mut Client,
    actor: &crate::identity::CommandActor,
    mut counted: impl FnMut(),
) -> Result<(), String> {
    use crate::identity::ReaderIdentity;
    if matches!(actor.reader, ReaderIdentity::Guest) {
        return actor.ensure_local_current().map_err(String::from);
    }
    if actor.is_local() {
        return Err("a PostgreSQL command has no persisted session authority".into());
    }
    let life = actor.lifetimes();
    let hash = actor.hash();
    let absolute = life.absolute.as_secs_f64();
    let idle = life.idle.as_secs_f64();
    counted();
    let row = c
        .query_opt(
            "SELECT user_id, handle, name, issuer, epoch FROM sessions \
         WHERE hash = $1 \
         AND opened > clock_timestamp() - make_interval(secs => $2) \
         AND seen > clock_timestamp() - make_interval(secs => $3)",
            &[&hash, &absolute, &idle],
        )
        .map_err(pg)?;
    let current = row.map(session_identity).transpose()?;
    match (&actor.reader, current) {
        (ReaderIdentity::Anonymous, None) => Ok(()),
        (ReaderIdentity::Account(expected), Some(current)) if *expected == current => Ok(()),
        _ => Err("session authority changed before commit".into()),
    }
}

/// RPC success is not release proof: false means the expected lock was absent.
pub(crate) fn command_session_unlock(
    c: &mut Client,
    hash: &[u8],
    mut counted: impl FnMut(),
) -> Result<(), String> {
    counted();
    let row = c
        .query_one(
            &format!("SELECT pg_advisory_unlock({SESSION_AUTHORITY_KEY})"),
            &[&hash],
        )
        .map_err(pg)?;
    let released: bool = row
        .try_get(0)
        .map_err(|_| "session unlock result could not be decoded")?;
    if released {
        Ok(())
    } else {
        Err("the command's session lock was not released".into())
    }
}

/// **A connection taken for one statement**, given back when dropped,
/// whatever the statement did.
struct Taken<'a> {
    sessions: &'a SessionsPg,
    client: Option<Client>,
    // A mutator marks its connection unsafe before BEGIN and only returns it
    // after acknowledged COMMIT. Panic/error/unknown ACK closes the socket.
    reusable: bool,
}

impl Drop for Taken<'_> {
    fn drop(&mut self) {
        let client = if self.reusable {
            self.client.take()
        } else {
            drop(self.client.take());
            None
        };
        self.sessions.give(client);
    }
}

impl SessionsPg {
    pub(crate) fn new(url: &str, schema: &str) -> SessionsPg {
        SessionsPg {
            url: url.to_string(),
            schema: schema.to_string(),
            idle: Mutex::new((Vec::new(), 0)),
            given_back: std::sync::Condvar::new(),
            #[cfg(test)]
            ran: std::sync::atomic::AtomicU64::new(0),
            #[cfg(test)]
            most: std::sync::atomic::AtomicUsize::new(0),
            #[cfg(test)]
            after_read: Mutex::new(None),
        }
    }

    /// An idle connection, or a new one while fewer than
    /// [`SESSION_CONNECTIONS`] are open; otherwise, the next given back.
    fn take(&self) -> Result<Taken<'_>, String> {
        let mut idle = self.idle.lock().expect("idle connections");
        loop {
            while let Some(c) = idle.0.pop() {
                if !c.is_closed() {
                    return Ok(Taken {
                        sessions: self,
                        client: Some(c),
                        reusable: true,
                    });
                }
                idle.1 -= 1;
            }
            if idle.1 < SESSION_CONNECTIONS {
                idle.1 += 1;
                #[cfg(test)]
                self.most
                    .fetch_max(idle.1, std::sync::atomic::Ordering::SeqCst);
                drop(idle);
                // Counted as open before it is made, and given back as gone
                // if it cannot be.
                let mut taken = Taken {
                    sessions: self,
                    client: None,
                    reusable: true,
                };
                let mut c = Client::connect(&self.url, NoTls).map_err(pg)?;
                c.batch_execute(&format!(
                    "SET search_path TO {}; {SESSION_CONNECTION_LOCK_TIMEOUT}",
                    ident(&self.schema)
                ))
                .map_err(pg)?;
                taken.client = Some(c);
                return Ok(taken);
            }
            idle = self.given_back.wait(idle).expect("idle connections");
        }
    }

    /// A connection given back: kept idle while it is open, and no longer
    /// counted once it is not.
    fn give(&self, c: Option<Client>) {
        let mut idle = self.idle.lock().expect("idle connections");
        match c.filter(|c| !c.is_closed()) {
            Some(c) => idle.0.push(c),
            None => idle.1 -= 1,
        }
        self.given_back.notify_one();
    }

    fn with<T>(
        &self,
        f: impl FnOnce(&mut Client) -> Result<T, postgres::Error>,
    ) -> Result<T, String> {
        let mut taken = self.take()?;
        #[cfg(test)]
        self.ran.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        f(taken.client.as_mut().expect("a connection taken")).map_err(pg)
    }

    fn counted(&self) {
        #[cfg(test)]
        self.ran.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }

    /// The advisory xact lock and mutation share one real transaction. Unlike
    /// `with`, every failed/panicking/uncertain mutator discards its connection.
    fn mutation<T>(
        &self,
        hash: &[u8],
        f: impl FnOnce(&mut Client) -> Result<T, postgres::Error>,
    ) -> Result<T, String> {
        let mut taken = self.take()?;
        taken.reusable = false;
        let result = {
            let c = taken.client.as_mut().expect("a connection taken");
            (|| {
                self.counted();
                c.batch_execute("BEGIN ISOLATION LEVEL READ COMMITTED")?;
                self.counted();
                c.batch_execute(SESSION_LOCK_TIMEOUT)?;
                self.counted();
                c.query_one(
                    &format!("SELECT pg_advisory_xact_lock({SESSION_AUTHORITY_KEY})"),
                    &[&hash],
                )?;
                self.counted();
                let out = f(c)?;
                self.counted();
                c.batch_execute("COMMIT")?;
                Ok(out)
            })()
        };
        if result.is_ok() {
            taken.reusable = true;
        } else if let Some(c) = taken.client.as_mut() {
            self.counted();
            // Even acknowledged rollback does not turn a failed mutator into a
            // reusable connection: uncertain COMMIT/lock ownership stays closed.
            let _ = c.batch_execute("ROLLBACK");
        }
        result.map_err(pg)
    }

    /// **A due write, made where nobody holds the session's lock** (ADR-0389,
    /// amended 2026-11-02), on the admission's own connection: a transaction
    /// of its own that tries the session's advisory lock and never waits for
    /// it. Taken, `f` runs under it, its snapshot taken after the lock (READ
    /// COMMITTED takes one per statement), so no open, close or command of
    /// the session, which take the same lock, changes the row under it; the
    /// answer is `f`'s. Held by another, nothing is written, and the answer
    /// is none. As in [`SessionsPg::mutation`], a failed statement or an
    /// unacknowledged end discards the connection.
    fn written_if_free<T>(
        &self,
        taken: &mut Taken<'_>,
        hash: &[u8],
        f: impl FnOnce(&mut Client) -> Result<T, postgres::Error>,
    ) -> Result<Option<T>, String> {
        taken.reusable = false;
        let c = taken.client.as_mut().expect("a connection taken");
        let result = (|| {
            self.counted();
            c.batch_execute("BEGIN ISOLATION LEVEL READ COMMITTED")?;
            self.counted();
            let free: bool = c
                .query_one(
                    &format!("SELECT pg_try_advisory_xact_lock({SESSION_AUTHORITY_KEY})"),
                    &[&hash],
                )?
                .try_get(0)?;
            if !free {
                self.counted();
                c.batch_execute("ROLLBACK")?;
                return Ok(None);
            }
            self.counted();
            let out = f(c)?;
            self.counted();
            c.batch_execute("COMMIT")?;
            Ok(Some(out))
        })();
        if result.is_ok() {
            taken.reusable = true;
        } else {
            self.counted();
            let _ = c.batch_execute("ROLLBACK");
        }
        result.map_err(pg)
    }

    /// A test's step after an admission read, run with the step's own lock
    /// let go: two admissions' steps may wait for each other.
    #[cfg(test)]
    fn read_step(&self) {
        let step = self.after_read.lock().expect("a test's step").clone();
        if let Some(step) = step {
            step();
        }
    }

    /// `seconds` of the database's sleep on one of its connections: a
    /// request holding it that long.
    #[cfg(test)]
    pub(crate) fn held_for_test(&self, seconds: f64) -> Result<(), String> {
        self.with(|c| c.execute("SELECT pg_sleep($1)", &[&seconds]).map(|_| ()))
    }

    /// The most connections open at once, so far.
    #[cfg(test)]
    pub(crate) fn most_open(&self) -> usize {
        self.most.load(std::sync::atomic::Ordering::SeqCst)
    }
}

fn session_identity(row: postgres::Row) -> Result<crate::identity::SessionIdentity, String> {
    let bytes: Vec<u8> = row
        .try_get(4)
        .map_err(|_| "session epoch could not be decoded")?;
    let epoch = bytes
        .try_into()
        .map_err(|_| "session epoch has an invalid length")?;
    Ok(crate::identity::SessionIdentity {
        principal: crate::identity::Principal {
            user: row
                .try_get(0)
                .map_err(|_| "session user could not be decoded")?,
            handle: row
                .try_get(1)
                .map_err(|_| "session handle could not be decoded")?,
            name: row
                .try_get(2)
                .map_err(|_| "session name could not be decoded")?,
            issuer: row
                .try_get(3)
                .map_err(|_| "session issuer could not be decoded")?,
        },
        epoch,
    })
}

impl crate::identity::Kept for SessionsPg {
    #[cfg(test)]
    fn ran(&self) -> u64 {
        self.ran.load(std::sync::atomic::Ordering::SeqCst)
    }

    #[cfg(test)]
    fn after_each_read(&self, step: Option<std::sync::Arc<dyn Fn() + Send + Sync>>) {
        *self.after_read.lock().expect("a test's step") = step;
    }

    /// **A session as an admission reads it** (ADR-0369; the integrator's
    /// ruling 2 of 2026-10-20; ADR-0389, amended 2026-11-02): one statement
    /// that takes no session lock ([`ADMISSION_READ`]). No row is no one. A
    /// live session whose use was recorded within [`touched_after`] is
    /// answered from the read, one statement in all, so a session in use
    /// writes at most that often. Otherwise a write is due, its use recorded
    /// or, past either lifetime, its row deleted, and it is tried without
    /// waiting ([`SessionsPg::written_if_free`]): where nobody holds the
    /// session's lock, [`ADMISSION_WRITE`] runs under it and answers (a
    /// session closed since the read is no one); where a command, an open
    /// or a close holds it, nothing is written and the read answers. An
    /// expired session is no one all the same, and is deleted by a later
    /// admission; a use left unrecorded is recorded by a later one, at the
    /// cost of at most [`touched_after`] of idle time.
    fn session(
        &self,
        hash: &[u8],
        lifetimes: &crate::identity::SessionLifetimes,
    ) -> Result<Option<crate::identity::SessionIdentity>, String> {
        let absolute = lifetimes.absolute.as_secs_f64();
        let idle = lifetimes.idle.as_secs_f64();
        let touch = touched_after(lifetimes).as_secs_f64();
        let params: [&(dyn postgres::types::ToSql + Sync); 4] = [&hash, &absolute, &idle, &touch];
        let mut taken = self.take()?;
        self.counted();
        let read = taken
            .client
            .as_mut()
            .expect("a connection taken")
            .query_opt(ADMISSION_READ, &params)
            .map_err(pg)?;
        #[cfg(test)]
        self.read_step();
        let Some(read) = read else {
            return Ok(None);
        };
        let live: bool = read
            .try_get(5)
            .map_err(|_| "session liveness could not be decoded")?;
        let stale: bool = read
            .try_get(6)
            .map_err(|_| "session use could not be decoded")?;
        let answer = if live {
            Some(session_identity(read)?)
        } else {
            None
        };
        // Its deletion, or its use recorded.
        let due = !live || stale;
        if !due {
            return Ok(answer);
        }
        match self.written_if_free(&mut taken, hash, |c| c.query_opt(ADMISSION_WRITE, &params))? {
            Some(written) => written.map(session_identity).transpose(),
            None => Ok(answer),
        }
    }

    fn inspect(
        &self,
        hash: &[u8],
        lifetimes: &crate::identity::SessionLifetimes,
    ) -> Result<Option<crate::identity::SessionIdentity>, String> {
        let absolute = lifetimes.absolute.as_secs_f64();
        let idle = lifetimes.idle.as_secs_f64();
        let row = self.with(|c| {
            c.query_opt(
                "SELECT user_id, handle, name, issuer, epoch FROM sessions \
                 WHERE hash = $1 \
                 AND opened > clock_timestamp() - make_interval(secs => $2) \
                 AND seen > clock_timestamp() - make_interval(secs => $3)",
                &[&hash, &absolute, &idle],
            )
        })?;
        row.map(session_identity).transpose()
    }

    fn open(
        &self,
        hash: &[u8],
        p: &crate::identity::Principal,
        epoch: &[u8; 16],
    ) -> Result<(), String> {
        self.mutation(hash, |c| {
            c.execute(
                "INSERT INTO sessions (hash, user_id, handle, name, issuer, epoch) \
                 VALUES ($1, $2, $3, $4, $5, $6) \
                 ON CONFLICT (hash) DO UPDATE SET user_id = EXCLUDED.user_id, \
                 handle = EXCLUDED.handle, name = EXCLUDED.name, issuer = EXCLUDED.issuer, epoch = EXCLUDED.epoch, opened = clock_timestamp(), seen = clock_timestamp()",
                &[&hash, &p.user, &p.handle, &p.name, &p.issuer, &&epoch[..]],
            )
            .map(|_| ())
        })
    }

    fn close(&self, hash: &[u8]) -> Result<(), String> {
        self.mutation(hash, |c| {
            c.execute("DELETE FROM sessions WHERE hash = $1", &[&hash])
                .map(|_| ())
        })
    }

    fn begin_sign_in(&self, hash: &[u8], nonce: &str, verifier: &str) -> Result<(), String> {
        // And those begun more than their lifetime ago forgotten, in the
        // same statement.
        self.with(|c| {
            c.execute(
                "WITH gone AS (DELETE FROM sign_ins \
                 WHERE started < clock_timestamp() - interval '10 minutes') \
                 INSERT INTO sign_ins (hash, nonce, verifier) VALUES ($1, $2, $3)",
                &[&hash, &nonce, &verifier],
            )
            .map(|_| ())
        })
    }

    fn take_sign_in(
        &self,
        hash: &[u8],
    ) -> Result<Option<(String, String, std::time::Duration)>, String> {
        self.with(|c| {
            Ok(c.query_opt(
                "DELETE FROM sign_ins WHERE hash = $1 RETURNING nonce, verifier, \
                 (extract(epoch FROM clock_timestamp() - started) * 1000)::bigint",
                &[&hash],
            )?
            .map(|r| {
                let ms: i64 = r.get(2);
                (
                    r.get(0),
                    r.get(1),
                    std::time::Duration::from_millis(u64::try_from(ms).unwrap_or(0)),
                )
            }))
        })
    }
}
