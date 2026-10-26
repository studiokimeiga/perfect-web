//! **The feed's data in PostgreSQL** (ADR-0246), beside the in-memory layer
//! (`feed.rs`), which stays the default. A deployment names a database with
//! `PW_FEED_DATABASE_URL`; the host opens this layer on it, holds it to what
//! the feed's `source` states, and serves the same program.
//!
//! - **The schema is plain SQL** under `migrations/feed/`, applied once each
//!   and recorded in `pw_migrations`.
//! - **A command is one transaction**, opened at [`DataLayer::begin`] and
//!   serializable whatever the connection's default. Its writes, and what it
//!   handed the outbox, commit together or not at all (ADR-0208).
//! - **What is delivered is read back from the outbox** once the transaction
//!   committed, and the rows read are consumed. The host tells open sessions
//!   from them exactly as it does from the in-memory layer's events.
//!
//! [`DataLayer::begin`]: crate::data::DataLayer::begin

use super::*;
use crate::data::{Dropped, Handed, Outboxed, Provided};
use crate::feed::{PROFILE_POSTS, not_found, ok, profile_of, user_of, user_record, viewer_val};
use crate::identity::Principals;
use postgres::{Client, NoTls};
use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, Ordering};

/// The migrations, in the order they apply, by the name each is recorded by.
const MIGRATIONS: &[(&str, &str)] = &[
    (
        "0001_schema",
        include_str!("../migrations/feed/0001_schema.sql"),
    ),
    (
        "0002_seed",
        include_str!("../migrations/feed/0002_seed.sql"),
    ),
    (
        "0004_follows",
        include_str!("../migrations/feed/0004_follows.sql"),
    ),
    // Track `uploads` (ADR-0260): a post's image, kept with it.
    (
        "0005_post_images",
        include_str!("../migrations/feed/0005_post_images.sql"),
    ),
    // Track `notifications` (ADR-0274): what others do that involves a user.
    (
        "0006_notifications",
        include_str!("../migrations/feed/0006_notifications.sql"),
    ),
    // Track `messages` (ADR-0279): a message, and each reader's mark.
    (
        "0007_messages",
        include_str!("../migrations/feed/0007_messages.sql"),
    ),
    // ADR-0325: an interaction's answer, committed with its writes.
    (
        "0008_interactions",
        include_str!("../migrations/feed/0008_interactions.sql"),
    ),
    // The scheduled-commands track: the runs a command schedules.
    (
        "0009_schedules",
        include_str!("../migrations/feed/0009_schedules.sql"),
    ),
    // Track `second-host` (ADR-0369): the outbox a log every host reads.
    (
        "0010_outbox_log",
        include_str!("../migrations/feed/0010_outbox_log.sql"),
    ),
    // Track `second-host` (ADR-0369): sessions and sign-ins kept.
    (
        "0011_sessions",
        include_str!("../migrations/feed/0011_sessions.sql"),
    ),
    (
        "0012_scheduled_commit",
        include_str!("../migrations/feed/0012_scheduled_commit.sql"),
    ),
];

/// **How a command's transaction is opened.** The layer sets serializable on
/// each; a deployment that leaves it to the connection gets the database's
/// `default_transaction_isolation`, read committed unless configured, and
/// the host refuses to serve a source that states more.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Isolation {
    Serializable,
    ConnectionDefault,
}

/// A database error, as one line: its SQLSTATE and message where the server
/// sent one.
pub(crate) fn pg(e: postgres::Error) -> String {
    match e.as_db_error() {
        Some(db) => format!("PostgreSQL {}: {}", db.code().code(), db.message()),
        None => format!("PostgreSQL: {e}"),
    }
}

/// An identifier, quoted.
pub(crate) fn ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// **Connections, kept open between uses.** Each is in the layer's schema.
struct Pool {
    url: String,
    schema: Option<String>,
    idle: Mutex<Vec<Client>>,
}

impl Pool {
    fn connect(&self) -> Result<Client, String> {
        let mut c = Client::connect(&self.url, NoTls).map_err(pg)?;
        if let Some(schema) = &self.schema {
            c.batch_execute(&format!("SET search_path TO {}", ident(schema)))
                .map_err(pg)?;
        }
        Ok(c)
    }

    fn take(&self) -> Result<Client, String> {
        match self.idle.lock().expect("idle connections").pop() {
            Some(c) if !c.is_closed() => Ok(c),
            _ => self.connect(),
        }
    }

    fn give(&self, c: Client) {
        if !c.is_closed() {
            self.idle.lock().expect("idle connections").push(c);
        }
    }

    /// One read outside any command: a statement of its own, which reads the
    /// latest commit.
    fn with<T>(
        &self,
        f: impl FnOnce(&mut Client) -> Result<T, postgres::Error>,
    ) -> Result<T, String> {
        let mut c = self.take()?;
        let out = f(&mut c).map_err(pg);
        self.give(c);
        out
    }
}

/// **Open a command's transaction** on `c`, as `isolation` says: the one
/// place a command's transaction begins, which [`FeedPg::provides`] measures.
fn begin_command(c: &mut Client, isolation: Isolation) -> Result<(), postgres::Error> {
    match isolation {
        Isolation::Serializable => c.batch_execute("BEGIN ISOLATION LEVEL SERIALIZABLE"),
        Isolation::ConnectionDefault => c.batch_execute("BEGIN"),
    }
}

// TRACK SEAM (render-merge): a render's read transaction (NEXT 4c), the
// feed's and the store's (`store_pg.rs`).
/// **How a render's read transaction opens** (NEXT 4c): repeatable read, so
/// every statement of the render reads the snapshot its first statement took
/// (PostgreSQL 18, 13.2.2), and read only, so it never meets a serialization
/// failure and never waits for a safe snapshot, as a deferrable one would.
pub(crate) const READ_SNAPSHOT: &str = "BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY";

/// **How many renders read one layer's database at once, where the
/// deployment says nothing** (NEXT 4c, the integrator's ruling 2 of
/// 2026-10-15; its answer of 2026-10-17 makes it a deployment's setting,
/// `PW_RENDER_SEATS`, with this default, to be measured under load on the
/// production track): each holds a connection of its own from its first
/// read to its end, besides the connections the layer's commands and single
/// reads take. A render past them waits at its beginning
/// (`render_merge::begin`), where it holds no connection.
pub(crate) const RENDER_CONNECTIONS: usize = 8;

/// **How long a render waits for its seat, where the deployment says
/// nothing** (`PW_RENDER_SEAT_WAIT`, the integrator's answer of
/// 2026-10-17), before it fails, loudly: each of its reads answers that it
/// waited. Renders give their seats back by structure (`render_merge`), so
/// this is a backstop against a wait no one ends, never a bound a render is
/// expected to meet.
#[cfg(not(test))]
pub(crate) const SEAT_WAIT: std::time::Duration = std::time::Duration::from_secs(30);

/// [`SEAT_WAIT`], for a test's host: five seconds, so a mutation control
/// that keeps a seat fails a test in seconds, not minutes.
#[cfg(test)]
pub(crate) const SEAT_WAIT: std::time::Duration = std::time::Duration::from_secs(5);

/// **The renders' seats, as a deployment sets them** (the integrator's
/// answer of 2026-10-17): how many renders read one layer's database at
/// once, and how long a render waits for its seat; [`RENDER_CONNECTIONS`]
/// and [`SEAT_WAIT`] where the deployment says nothing. The host reads them
/// from its environment where it starts, for each layer it serves on
/// PostgreSQL.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RenderSeats {
    pub(crate) seats: usize,
    pub(crate) wait: std::time::Duration,
}

impl Default for RenderSeats {
    fn default() -> RenderSeats {
        RenderSeats {
            seats: RENDER_CONNECTIONS,
            wait: SEAT_WAIT,
        }
    }
}

impl RenderSeats {
    /// **The seats as the deployment states them**: `seats`, the value of
    /// `PW_RENDER_SEATS`, a count of one at least; `wait`, the value of
    /// `PW_RENDER_SEAT_WAIT`, whole seconds, one at least. Each is its
    /// default where unset or empty, and anything else is refused, by name,
    /// so a host does not start on a setting it cannot read.
    pub(crate) fn stated(seats: Option<&str>, wait: Option<&str>) -> Result<RenderSeats, String> {
        let mut stated = RenderSeats::default();
        if let Some(v) = seats.filter(|v| !v.is_empty()) {
            stated.seats = match v.parse::<usize>() {
                Ok(n) if n >= 1 => n,
                _ => {
                    return Err(format!(
                        "PW_RENDER_SEATS={v} is not a count of seats, one at least"
                    ));
                }
            };
        }
        if let Some(v) = wait.filter(|v| !v.is_empty()) {
            stated.wait = match v.parse::<u64>() {
                Ok(n) if n >= 1 => std::time::Duration::from_secs(n),
                _ => {
                    return Err(format!(
                        "PW_RENDER_SEAT_WAIT={v} is not a count of seconds, one at least"
                    ));
                }
            };
        }
        Ok(stated)
    }
}

/// **The renders' connections of one layer** (NEXT 4c): no more seats
/// taken than its deployment's [`RenderSeats`] say, and no more connections
/// open than that, each a seat's or idle for the next render.
pub(crate) struct Renders {
    url: String,
    schema: Option<String>,
    /// How many renders are seated at once at most: the deployment's.
    bound: usize,
    seats: Mutex<Seats>,
    freed: std::sync::Condvar,
    /// How long a render waits for its seat: the deployment's, or a test's.
    wait: Mutex<std::time::Duration>,
}

/// The seats taken, and the connections.
#[derive(Default)]
struct Seats {
    /// Connections no render holds, kept for the next.
    idle: Vec<Client>,
    taken: usize,
    /// Connections open: idle, or a seat's.
    open: usize,
    /// The most seats taken at once, and connections open (NEXT 4c's
    /// tests).
    #[cfg(test)]
    most: (usize, usize),
    /// Renders waiting for a seat now (NEXT 4c's tests).
    #[cfg(test)]
    waiting: usize,
}

impl Renders {
    /// **The renders' connections of a layer on `url`**, seated as the
    /// deployment's `seats` say, one of them opened now and kept idle, so
    /// the host's first render does not wait for a connection to be made, as
    /// no read did before renders had their own (the commands' pool is warm
    /// once the layer has migrated). One that cannot be made now is made at
    /// the first read.
    pub(crate) fn new(url: &str, schema: Option<&str>, seats: RenderSeats) -> Arc<Renders> {
        let renders = Arc::new(Renders {
            url: url.to_string(),
            schema: schema.map(str::to_string),
            bound: seats.seats,
            seats: Mutex::default(),
            freed: std::sync::Condvar::new(),
            wait: Mutex::new(seats.wait),
        });
        if let Ok(c) = renders.connection() {
            renders.seats.lock().expect("seats").idle.push(c);
        }
        renders
    }

    /// **A render's seat**, once one is free: waited for while every seat is
    /// taken, for the deployment's wait at most; past it, a seat whose every
    /// read fails saying so.
    pub(crate) fn seat(self: &Arc<Self>) -> Seat {
        let wait = *self.wait.lock().expect("wait");
        let deadline = std::time::Instant::now() + wait;
        let mut seats = self.seats.lock().expect("seats");
        while seats.taken >= self.bound {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            if left.is_zero() {
                return Seat {
                    renders: self.clone(),
                    conn: Mutex::new(None),
                    refused: Some(format!(
                        "a render waited {wait:?} for one of the {} connections renders read in",
                        self.bound
                    )),
                };
            }
            #[cfg(test)]
            {
                seats.waiting += 1;
            }
            seats = self.freed.wait_timeout(seats, left).expect("seats").0;
            #[cfg(test)]
            {
                seats.waiting -= 1;
            }
        }
        seats.taken += 1;
        #[cfg(test)]
        {
            seats.most.0 = seats.most.0.max(seats.taken);
        }
        Seat {
            renders: self.clone(),
            conn: Mutex::new(None),
            refused: None,
        }
    }

    /// An idle connection, or a new one: never one more than the seats
    /// taken need, since a seat asks only while it holds none.
    fn connection(&self) -> Result<Client, String> {
        {
            let mut seats = self.seats.lock().expect("seats");
            while let Some(c) = seats.idle.pop() {
                if !c.is_closed() {
                    return Ok(c);
                }
                seats.open -= 1;
            }
            seats.open += 1;
            #[cfg(test)]
            {
                seats.most.1 = seats.most.1.max(seats.open);
            }
        }
        let connected = Client::connect(&self.url, NoTls)
            .map_err(pg)
            .and_then(|mut c| {
                if let Some(schema) = &self.schema {
                    c.batch_execute(&format!("SET search_path TO {}", ident(schema)))
                        .map_err(pg)?;
                }
                Ok(c)
            });
        if connected.is_err() {
            self.seats.lock().expect("seats").open -= 1;
        }
        connected
    }

    /// A seat's transaction ended `how`, and its connection kept idle where
    /// it could end it, closed otherwise.
    fn ended(&self, mut c: Client, how: &str) {
        let kept = c.batch_execute(how).is_ok();
        let mut seats = self.seats.lock().expect("seats");
        if kept {
            seats.idle.push(c);
        } else {
            seats.open -= 1;
        }
    }

    /// How long a render waits for its seat, from now (a test's shorter).
    #[cfg(test)]
    pub(crate) fn waiting_at_most(&self, wait: std::time::Duration) {
        *self.wait.lock().expect("wait") = wait;
    }

    /// The most seats taken at once, and connections open, so far.
    #[cfg(test)]
    pub(crate) fn most(&self) -> (usize, usize) {
        self.seats.lock().expect("seats").most
    }

    /// The seats taken now.
    #[cfg(test)]
    pub(crate) fn taken(&self) -> usize {
        self.seats.lock().expect("seats").taken
    }

    /// The renders waiting for a seat now.
    #[cfg(test)]
    pub(crate) fn waiting(&self) -> usize {
        self.seats.lock().expect("seats").waiting
    }
}

/// **A render's seat** (NEXT 4c): its connection, opened at the render's
/// first read with its read transaction ([`READ_SNAPSHOT`]), which every
/// later read of the render reads in, and committed when the render ends. A
/// read that fails ends the transaction, as it may have aborted it, and the
/// render's next read opens another. Given back when dropped.
pub(crate) struct Seat {
    renders: Arc<Renders>,
    conn: Mutex<Option<Client>>,
    /// Why there is no seat: the wait for one passed the deployment's.
    refused: Option<String>,
}

impl Seat {
    /// `f`, on the seat's connection, in the render's read transaction.
    pub(crate) fn read<T>(
        &self,
        f: impl FnOnce(&mut Client) -> Result<T, String>,
    ) -> Result<T, String> {
        if let Some(why) = &self.refused {
            return Err(why.clone());
        }
        let mut conn = self.conn.lock().expect("the render's connection");
        if conn.is_none() {
            let mut c = self.renders.connection()?;
            if let Err(e) = c.batch_execute(READ_SNAPSHOT) {
                drop(c);
                self.renders.seats.lock().expect("seats").open -= 1;
                return Err(pg(e));
            }
            *conn = Some(c);
        }
        let out = f(conn.as_mut().expect("opened"));
        if out.is_err() {
            self.renders.ended(conn.take().expect("opened"), "ROLLBACK");
        }
        out
    }
}

impl Drop for Seat {
    fn drop(&mut self) {
        let conn = match self.conn.get_mut() {
            Ok(c) => c.take(),
            Err(poisoned) => poisoned.into_inner().take(),
        };
        if let Some(c) = conn {
            self.renders.ended(c, "COMMIT");
        }
        if self.refused.is_none() {
            let mut seats = self.renders.seats.lock().expect("seats");
            seats.taken -= 1;
            self.renders.freed.notify_one();
        }
    }
}

/// One post as read: its author's handle and name where it has a row, its
/// likes counted, and what it replies to.
struct Row {
    id: String,
    author: String,
    known: Option<(String, String)>,
    text: String,
    likes: i64,
    reply_to: Option<String>,
    /// Track `uploads`: the image it carries.
    image: Option<crate::uploads::ImageRow>,
}

/// The columns every read of a post selects, `p` the post.
const POST_COLUMNS: &str = "p.id, p.author, u.handle, u.name, p.text, \
     (SELECT count(*) FROM likes l WHERE l.post = p.id), p.reply_to, \
     p.image_key, p.image_kind, p.image_width, p.image_height, p.image_alt";

fn row_of(r: &postgres::Row) -> Row {
    let handle: Option<String> = r.get(2);
    let name: Option<String> = r.get(3);
    // Track `uploads`: all five columns, or none (`posts_image_whole`).
    let key: Option<String> = r.get(7);
    let kind: Option<String> = r.get(8);
    let (width, height): (Option<i32>, Option<i32>) = (r.get(9), r.get(10));
    let alt: Option<String> = r.get(11);
    let image = (|| {
        Some(crate::uploads::ImageRow {
            key: key?,
            kind: crate::uploads::sniff::Kind::named(&kind?)?,
            width: u32::try_from(width?).ok()?,
            height: u32::try_from(height?).ok()?,
            alt: alt?,
        })
    })();
    Row {
        id: r.get(0),
        author: r.get(1),
        known: handle.zip(name),
        text: r.get(4),
        likes: r.get(5),
        reply_to: r.get(6),
        image,
    }
}

/// A post as the program's `Post` is, `replies` inside it, its image served
/// where `l` says.
fn post_val(row: &Row, replies: Vec<Val>, l: Option<&crate::uploads::Leases>) -> Val {
    Val::Record(vec![
        ("id".into(), Val::String(row.id.clone())),
        ("author".into(), user_record(&row.author, row.known.clone())),
        ("text".into(), Val::String(row.text.clone())),
        ("likes".into(), Val::S64(row.likes)),
        ("replies".into(), Val::List(replies)),
        (
            "images".into(),
            crate::uploads::images_val(row.image.as_ref(), l),
        ),
    ])
}

/// **The timeline**: the newest `limit` posts that reply to none, each an
/// `Item` (ADR-0222), and whether `reader` wrote it (track `identity`).
fn timeline(
    c: &mut Client,
    limit: i64,
    reader: &str,
    l: Option<&crate::uploads::Leases>,
) -> Result<Val, postgres::Error> {
    let rows = c.query(
        &format!(
            "SELECT {POST_COLUMNS} FROM posts p LEFT JOIN users u ON u.id = p.author \
             WHERE p.reply_to IS NULL ORDER BY p.seq DESC LIMIT $1"
        ),
        &[&limit.max(0)],
    )?;
    Ok(Val::List(
        rows.iter().map(|r| item_of(row_of(r), reader, l)).collect(),
    ))
}

/// An `Item` as the timelines show it (ADR-0222), and whether `reader`
/// wrote it (track `identity`).
fn item_of(row: Row, reader: &str, l: Option<&crate::uploads::Leases>) -> Val {
    let mine = row.author == reader;
    Val::Record(vec![
        ("id".into(), Val::String(row.id)),
        ("author".into(), user_record(&row.author, row.known)),
        ("text".into(), Val::String(row.text)),
        ("likes".into(), Val::S64(row.likes)),
        ("mine".into(), Val::Bool(mine)),
        (
            "images".into(),
            crate::uploads::images_val(row.image.as_ref(), l),
        ),
    ])
}

/// **The timeline of those `reader` follows** (ADR-0257): its own posts and
/// theirs that reply to none, the newest `limit`.
fn following(
    c: &mut Client,
    reader: &str,
    limit: i64,
    l: Option<&crate::uploads::Leases>,
) -> Result<Val, postgres::Error> {
    let rows = c.query(
        &format!(
            "SELECT {POST_COLUMNS} FROM posts p LEFT JOIN users u ON u.id = p.author \
             WHERE p.reply_to IS NULL \
               AND (p.author = $1 \
                    OR p.author IN (SELECT followee FROM follows WHERE follower = $1)) \
             ORDER BY p.seq DESC LIMIT $2"
        ),
        &[&reader, &limit.max(0)],
    )?;
    Ok(Val::List(
        rows.iter().map(|r| item_of(row_of(r), reader, l)).collect(),
    ))
}

/// Whether the feed knows `id` (ADR-0257), as `feed.rs` reads it.
const KNOWN: &str = "EXISTS (SELECT 1 FROM users WHERE id = $1) \
     OR EXISTS (SELECT 1 FROM posts WHERE author = $1) \
     OR EXISTS (SELECT 1 FROM follows WHERE follower = $1 OR followee = $1)";

/// **A user's page** (ADR-0257), read in one statement, so one snapshot:
/// who they are, their counts, and their newest posts that reply to none.
fn profile(c: &mut Client, id: &str) -> Result<Val, postgres::Error> {
    let row = c.query_one(
        &format!(
            "SELECT u.handle, u.name, \
                    (SELECT count(*) FROM follows WHERE followee = $1), \
                    (SELECT count(*) FROM follows WHERE follower = $1), \
                    (SELECT coalesce(json_agg(json_build_array(p.id, p.text, \
                         (SELECT count(*) FROM likes l WHERE l.post = p.id)) \
                         ORDER BY p.seq DESC), '[]')::text \
                       FROM (SELECT * FROM posts WHERE author = $1 AND reply_to IS NULL \
                             ORDER BY seq DESC LIMIT {PROFILE_POSTS}) p), \
                    {KNOWN} \
             FROM (SELECT 1) AS one LEFT JOIN users u ON u.id = $1"
        ),
        &[&id],
    )?;
    let known: bool = row.get(5);
    if !known {
        return Ok(not_found());
    }
    let (handle, name): (Option<String>, Option<String>) = (row.get(0), row.get(1));
    let (followers, following): (i64, i64) = (row.get(2), row.get(3));
    let posts: Vec<(String, String, i64)> =
        serde_json::from_str(&row.get::<_, String>(4)).unwrap_or_default();
    Ok(ok(Val::Record(vec![
        ("user".into(), user_record(id, handle.zip(name))),
        ("followers".into(), Val::S64(followers)),
        ("following".into(), Val::S64(following)),
        (
            "posts".into(),
            Val::List(
                posts
                    .into_iter()
                    .map(|(id, text, likes)| {
                        Val::Record(vec![
                            ("id".into(), Val::String(id)),
                            ("text".into(), Val::String(text)),
                            ("likes".into(), Val::S64(likes)),
                        ])
                    })
                    .collect(),
            ),
        ),
    ])))
}

/// **What `reader` is to `user`** (ADR-0257), the program's `Relation`.
fn relation(c: &mut Client, reader: &str, user: &str) -> Result<Val, postgres::Error> {
    if reader == user {
        return Ok(Val::Variant("yourself".into(), None));
    }
    let follows: bool = c
        .query_one(
            "SELECT EXISTS (SELECT 1 FROM follows WHERE follower = $1 AND followee = $2)",
            &[&reader, &user],
        )?
        .get(0);
    let case = if follows {
        "following"
    } else {
        "not-following"
    };
    Ok(Val::Variant(case.into(), None))
}

/// **A thread**: the post and its replies, as deep as they go, read in one
/// statement, so one snapshot.
fn thread(
    c: &mut Client,
    id: &str,
    l: Option<&crate::uploads::Leases>,
) -> Result<Val, postgres::Error> {
    let rows = c.query(
        &format!(
            "WITH RECURSIVE t AS ( \
                 SELECT * FROM posts WHERE id = $1 \
                 UNION ALL \
                 SELECT p.* FROM posts p JOIN t ON p.reply_to = t.id) \
             SELECT {POST_COLUMNS} FROM t p LEFT JOIN users u ON u.id = p.author \
             ORDER BY p.seq"
        ),
        &[&id],
    )?;
    let rows: Vec<Row> = rows.iter().map(row_of).collect();
    // Each post's replies, in the order they were written.
    fn nested(rows: &[Row], row: &Row, l: Option<&crate::uploads::Leases>) -> Val {
        let replies = rows
            .iter()
            .filter(|r| r.reply_to.as_deref() == Some(row.id.as_str()))
            .map(|r| nested(rows, r, l))
            .collect();
        post_val(row, replies, l)
    }
    Ok(match rows.iter().find(|r| r.id == id) {
        Some(root) => ok(nested(&rows, root, l)),
        None => not_found(),
    })
}

/// One post with its likes, its replies left to its thread.
fn post_alone(
    c: &mut Client,
    id: &str,
    l: Option<&crate::uploads::Leases>,
) -> Result<Option<Val>, postgres::Error> {
    let row = c.query_opt(
        &format!(
            "SELECT {POST_COLUMNS} FROM posts p LEFT JOIN users u ON u.id = p.author \
             WHERE p.id = $1"
        ),
        &[&id],
    )?;
    Ok(row.map(|r| post_val(&row_of(&r), Vec::new(), l)))
}

/// One read, on a connection.
type Read<'r> = &'r mut dyn FnMut(&mut Client) -> Result<Val, String>;

/// Where a read runs: a connection of the pool's, or the command's own
/// transaction.
type Run = Arc<dyn Fn(Read<'_>) -> Result<Val, String> + Send + Sync>;

/// The reads, each through `run`, a session's user its principal's, a
/// post's image served where `leases` says.
fn reads_through(
    run: Run,
    principals: Principals,
    leases: Option<crate::uploads::Leases>,
) -> crate::data::Ops {
    let mut ops: crate::data::Ops = BTreeMap::new();
    // Track `uploads`: the image a session attached, waiting for its post.
    ops.insert(
        "feed:data/uploads#attached".to_string(),
        crate::uploads::attached_op(leases.clone()),
    );
    let (r, p, l) = (run.clone(), principals.clone(), leases.clone());
    ops.insert(
        "feed:data/posts#timeline".to_string(),
        Arc::new(move |args: &[Val]| match args {
            [Val::String(session), Val::S64(limit)] => {
                let (limit, reader) = (*limit, p.user_of(session));
                Ok(vec![r(&mut |c| {
                    timeline(c, limit, &reader, l.as_ref()).map_err(pg)
                })?])
            }
            other => Err(format!("posts#timeline received {other:?}")),
        }),
    );
    let (r, l) = (run.clone(), leases.clone());
    ops.insert(
        "feed:data/posts#thread".to_string(),
        Arc::new(move |args: &[Val]| match args {
            [Val::String(id)] => Ok(vec![r(&mut |c| thread(c, id, l.as_ref()).map_err(pg))?]),
            other => Err(format!("posts#thread received {other:?}")),
        }),
    );
    let (r, p, l) = (run.clone(), principals.clone(), leases);
    ops.insert(
        "feed:data/posts#following".to_string(),
        Arc::new(move |args: &[Val]| match args {
            [Val::String(session), Val::S64(limit)] => {
                let (reader, limit) = (p.user_of(session), *limit);
                Ok(vec![r(&mut |c| {
                    following(c, &reader, limit, l.as_ref()).map_err(pg)
                })?])
            }
            other => Err(format!("posts#following received {other:?}")),
        }),
    );
    let r = run.clone();
    ops.insert(
        "feed:data/users#profile".to_string(),
        Arc::new(move |args: &[Val]| match args {
            [Val::String(id)] => Ok(vec![r(&mut |c| profile(c, id).map_err(pg))?]),
            other => Err(format!("users#profile received {other:?}")),
        }),
    );
    // Track `notifications` (ADR-0274): a reader's notifications and how
    // many are unread, by the reader's handle, their id on the wire; and who
    // wrote a post, an id.
    let r = run.clone();
    ops.insert(
        "feed:data/notifications#list".to_string(),
        Arc::new(move |args: &[Val]| match args {
            [Val::String(reader), Val::S64(limit)] => Ok(vec![r(&mut |c| {
                crate::notifications::pg::list(c, reader, *limit).map_err(pg)
            })?]),
            other => Err(format!("notifications#list received {other:?}")),
        }),
    );
    let r = run.clone();
    ops.insert(
        "feed:data/notifications#unread".to_string(),
        Arc::new(move |args: &[Val]| match args {
            [Val::String(reader)] => Ok(vec![r(&mut |c| {
                crate::notifications::pg::unread(c, reader)
                    .map(Val::S64)
                    .map_err(pg)
            })?]),
            other => Err(format!("notifications#unread received {other:?}")),
        }),
    );
    let r = run.clone();
    ops.insert(
        "feed:data/posts#author".to_string(),
        Arc::new(move |args: &[Val]| match args {
            [Val::String(id)] => Ok(vec![r(&mut |c| {
                c.query_opt("SELECT author FROM posts WHERE id = $1", &[id])
                    .map(|row| Val::String(row.map(|r| r.get(0)).unwrap_or_default()))
                    .map_err(pg)
            })?]),
            other => Err(format!("posts#author received {other:?}")),
        }),
    );
    // Track `messages` (ADR-0279): a reader's conversation, their list and
    // count, each by the reader's handle, and who may message whom: in the
    // command's transaction where a command reads it.
    let r = run.clone();
    ops.insert(
        "feed:data/messages#conversation".to_string(),
        Arc::new(move |args: &[Val]| match args {
            [Val::String(reader), Val::String(with), Val::S64(limit)] => Ok(vec![r(&mut |c| {
                crate::messages::pg::conversation(c, reader, with, *limit).map_err(pg)
            })?]),
            other => Err(format!("messages#conversation received {other:?}")),
        }),
    );
    let r = run.clone();
    ops.insert(
        "feed:data/messages#list".to_string(),
        Arc::new(move |args: &[Val]| match args {
            [Val::String(reader)] => Ok(vec![r(&mut |c| {
                crate::messages::pg::list(c, reader).map_err(pg)
            })?]),
            other => Err(format!("messages#list received {other:?}")),
        }),
    );
    let r = run.clone();
    ops.insert(
        "feed:data/messages#unread".to_string(),
        Arc::new(move |args: &[Val]| match args {
            [Val::String(reader)] => Ok(vec![r(&mut |c| {
                crate::messages::pg::unread(c, reader)
                    .map(Val::S64)
                    .map_err(pg)
            })?]),
            other => Err(format!("messages#unread received {other:?}")),
        }),
    );
    let r = run.clone();
    ops.insert(
        crate::messages::MAY.to_string(),
        Arc::new(move |args: &[Val]| match args {
            [Val::String(from), Val::String(to)] => Ok(vec![r(&mut |c| {
                crate::messages::pg::may(c, from, to)
                    .map(Val::Bool)
                    .map_err(pg)
            })?]),
            other => Err(format!("messages#may received {other:?}")),
        }),
    );
    let (r, p) = (run, principals.clone());
    ops.insert(
        "feed:data/users#relation".to_string(),
        Arc::new(move |args: &[Val]| match args {
            [Val::String(session), Val::String(user)] => {
                let reader = p.user_of(session);
                Ok(vec![r(&mut |c| relation(c, &reader, user).map_err(pg))?])
            }
            other => Err(format!("users#relation received {other:?}")),
        }),
    );
    let p = principals.clone();
    ops.insert(
        "feed:data/users#of-session".to_string(),
        Arc::new(move |args: &[Val]| match args {
            [Val::String(session)] => Ok(vec![Val::String(p.user_of(session))]),
            other => Err(format!("users#of-session received {other:?}")),
        }),
    );
    ops.insert(
        "feed:data/users#viewer".to_string(),
        Arc::new(move |args: &[Val]| match args {
            [Val::String(session)] => Ok(vec![viewer_val(principals.of(session))]),
            other => Err(format!("users#viewer received {other:?}")),
        }),
    );
    ops
}

/// **The author `session` writes as** (track `identity`): its principal's
/// user, and a signed-in one's handle and name written with what they
/// write, as their provider last gave them.
fn author(c: &mut Client, principals: &Principals, session: &str) -> Result<String, String> {
    let Some(p) = principals.of(session) else {
        return Ok(user_of(session));
    };
    if let Some((handle, name)) = profile_of(&p) {
        c.execute(
            "INSERT INTO users (id, handle, name) VALUES ($1, $2, $3) \
             ON CONFLICT (id) DO UPDATE SET handle = EXCLUDED.handle, name = EXCLUDED.name",
            &[&p.user, &handle, &name],
        )
        .map_err(pg)?;
    }
    Ok(p.user)
}

/// **The feed's data in one PostgreSQL database.**
pub(crate) struct FeedPg {
    queue_authority: pw_materialize::QueueAuthority,
    pool: Arc<Pool>,
    /// TRACK SEAM (render-merge): the renders' connections (NEXT 4c).
    renders: Arc<Renders>,
    isolation: Isolation,
    /// One command of this host at a time, from its first write to its
    /// commit, as the in-memory layer's lock is: a host never conflicts with
    /// itself. A serializable transaction is what holds against any other
    /// writer of the database, and a commit it refuses keeps nothing.
    writes: crate::data::Writes,
    /// Track `uploads`: the leases a post's image is committed from.
    uploaded: std::sync::OnceLock<crate::uploads::Leases>,
    /// Each session's principal (track `identity`): whom a session writes
    /// as. Until the server hands its own, every session is its own guest.
    principals: std::sync::RwLock<Principals>,
    /// **This host's reader of the outbox** (ADR-0369): every other host's
    /// commits, from its cursor; and its id, which its own rows carry.
    tail: crate::hosts::Tail,
    /// **Its sessions and sign-ins, kept where the hosts share them**
    /// (ADR-0369).
    sessions: Arc<crate::hosts::SessionsPg>,
    /// **This host's instance on the database** (ADR-0369), which its page
    /// numbers carry.
    instance: u64,
}

impl FeedPg {
    fn principals(&self) -> Principals {
        self.principals.read().expect("principals").clone()
    }

    /// **The layer on `url`**, its tables in `schema` (created if missing)
    /// or in the connection's own search path, migrated, and a command's
    /// transaction serializable.
    #[cfg(test)]
    pub(crate) fn open(url: &str, schema: Option<&str>) -> Result<FeedPg, String> {
        FeedPg::open_with(url, schema, Isolation::Serializable, RenderSeats::default())
    }

    /// [`FeedPg::open`], a command's transaction opened as `isolation` says,
    /// and its renders seated as the deployment's `seats` say.
    pub(crate) fn open_with(
        url: &str,
        schema: Option<&str>,
        isolation: Isolation,
        seats: RenderSeats,
    ) -> Result<FeedPg, String> {
        let pool = Arc::new(Pool {
            url: url.to_string(),
            schema: schema.map(str::to_string),
            idle: Mutex::new(Vec::new()),
        });
        if let Some(schema) = schema {
            let mut c = Client::connect(url, NoTls).map_err(pg)?;
            c.batch_execute(&format!("CREATE SCHEMA IF NOT EXISTS {}", ident(schema)))
                .map_err(pg)?;
        }
        let mut c = pool.connect()?;
        migrate(&mut c)?;
        // **The outbox is every host's** (ADR-0369): a host just opened
        // reads it from its highest id, and deletes nothing another host
        // has yet to read. Until then it deleted the outbox, a second host's
        // undelivered rows among it.
        let outbox = crate::hosts::Outbox::current(&mut c)?;
        let highest = crate::hosts::highest(&mut c)?;
        let instance = crate::hosts::instance(&mut c)?;
        pool.give(c);
        let host = crate::hosts::new_host();
        let sessions = Arc::new(crate::hosts::SessionsPg::new(url, &outbox.schema));
        let tail = crate::hosts::Tail::at(url, outbox, &host, highest);
        Ok(FeedPg {
            queue_authority: pw_materialize::QueueAuthority::new(),
            pool,
            renders: Renders::new(url, schema, seats),
            isolation,
            writes: crate::data::Writes::new(),
            uploaded: std::sync::OnceLock::new(),
            principals: std::sync::RwLock::new(Principals::default()),
            tail,
            sessions,
            instance,
        })
    }
}

/// **Apply each migration not yet applied**, in order, in one transaction,
/// under a lock no other host migrating the same database can share.
fn migrate(c: &mut Client) -> Result<(), String> {
    let mut tx = c.transaction().map_err(pg)?;
    tx.batch_execute(
        "SELECT pg_advisory_xact_lock(hashtext('pw_feed_migrations'));
         CREATE TABLE IF NOT EXISTS pw_migrations (
             version    text PRIMARY KEY,
             applied_at timestamptz NOT NULL DEFAULT now()
         );",
    )
    .map_err(pg)?;
    for (version, sql) in MIGRATIONS {
        let applied = tx
            .query_opt("SELECT 1 FROM pw_migrations WHERE version = $1", &[version])
            .map_err(pg)?
            .is_some();
        if applied {
            continue;
        }
        tx.batch_execute(sql)
            .map_err(|e| format!("migration {version}: {}", pg(e)))?;
        tx.execute(
            "INSERT INTO pw_migrations (version) VALUES ($1)",
            &[version],
        )
        .map_err(pg)?;
    }
    tx.commit().map_err(pg)
}

/// **A value the outbox keeps**, as the command computed it (ADR-0208): a
/// `String`, an `Int` or a `Bool`, which key an entry, and an invalidated
/// entry's `_`, every value at its position, as `null` (ADR-0256). Anything
/// else keys none and is refused, as the host's own outbox refuses it.
pub(crate) fn json_of(name: &str, values: &[Option<Val>]) -> Result<String, String> {
    let values = values
        .iter()
        .map(|v| match v {
            None => Ok(serde_json::Value::Null),
            Some(Val::String(s)) => Ok(serde_json::Value::String(s.clone())),
            Some(Val::S64(n)) => Ok(serde_json::Value::from(*n)),
            Some(Val::Bool(b)) => Ok(serde_json::Value::Bool(*b)),
            Some(other) => Err(format!("`{name}` carries {other:?}, which keys no entry")),
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(serde_json::Value::Array(values).to_string())
}

/// A value read back from the outbox, as it was written.
fn val_of(json: &serde_json::Value) -> Result<Val, String> {
    match json {
        serde_json::Value::String(s) => Ok(Val::String(s.clone())),
        serde_json::Value::Bool(b) => Ok(Val::Bool(*b)),
        serde_json::Value::Number(n) => n
            .as_i64()
            .map(Val::S64)
            .ok_or_else(|| format!("the outbox kept {n}, which is no Int")),
        other => Err(format!("the outbox kept {other}, which keys no entry")),
    }
}

/// **A command's transaction** (ADR-0218, ADR-0246): its connection, in
/// the transaction `begin` opened, from the call to the commit.
pub(crate) struct Staging<'a> {
    queue_authority: &'a pw_materialize::QueueAuthority,
    _writes: std::sync::MutexGuard<'a, ()>,
    pool: Arc<Pool>,
    /// The connection, given back to the pool when the staging is dropped.
    conn: Arc<Mutex<Option<Client>>>,
    /// Whether the command wrote anything.
    wrote: Arc<AtomicBool>,
    /// Whether COMMIT was acknowledged. Drop attempts rollback otherwise;
    /// transport loss can still leave the authoritative outcome unknown.
    committed: bool,
    ops: crate::data::Ops,
    /// Track `uploads`: the leases the command claimed, given back where it
    /// does not commit.
    claims: crate::uploads::Claiming,
    /// The leases, and the images of the posts the command deletes, each
    /// collected once it commits where no post names it (ADR-0261).
    leases: Option<crate::uploads::Leases>,
    released: Arc<Mutex<Vec<String>>>,
    /// The schema's outbox, which its rows are ordered in, and this host's
    /// id, which they carry (ADR-0369).
    outbox: &'a crate::hosts::Outbox,
    host: &'a str,
}

impl crate::data::Staged for Staging<'_> {
    fn ops(&self) -> crate::data::Ops {
        self.ops.clone()
    }

    fn rows(&self) -> Option<Vec<(String, String)>> {
        // Its rows are the database's: the host's materializer keeps none.
        self.wrote.load(Ordering::SeqCst).then(Vec::new)
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
        let mut conn = self.conn.lock().expect("the command's connection");
        let c = conn
            .as_mut()
            .ok_or("the command's transaction did not open")?;
        if let Some(completion) = completing {
            if !completion.belongs_to(self.queue_authority) {
                return Err("scheduled completion belongs to another queue adapter".into());
            }
            match crate::schedules::pg::consume(c, completion.run, completion.now_ms, || {})
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
        // Track `uploads`: a post's image in the deployment's blob storage
        // before its row commits, so no committed post names bytes the
        // deployment does not have.
        self.claims.lock().expect("claims").keep()?;
        // **The interaction's answer, with the writes** (ADR-0325).
        if let Some(r) = interaction {
            record(c, r)?;
        }
        // **And the runs it scheduled, or cancelled** (the
        // scheduled-commands track).
        crate::schedules::pg::write(c, scheduled).map_err(pg)?;
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
        // listening woken at the commit.
        if !handed.is_empty() {
            crate::hosts::ordered(c, self.outbox).map_err(pg)?;
        }
        let mut ids: Vec<i64> = Vec::new();
        for (kind, name, values) in handed {
            let args = json_of(name, &values)?;
            let row = c
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
        // Track `uploads`: each lease the command committed or discarded
        // ended, now that its transaction has.
        self.claims.lock().expect("claims").settle();
        // **A deleted post's image, collected where no post names it**
        // (ADR-0261): read after the commit, while this command still holds
        // the writes' lock, so no other commits between the look and the
        // delete.
        if let Some(leases) = &self.leases {
            for key in self.released.lock().expect("released").drain(..) {
                let collected = names_blob(c, &key)
                    .map_err(pg)
                    .and_then(|named| leases.collect(&key, named));
                if let Err(why) = collected {
                    eprintln!("pw dev server: blob {key} was not collected: {why}");
                }
            }
        }
        match delivered(c, &ids) {
            Ok(read) => Ok(crate::data::Commit::Applied(read)),
            // Committed, and not read back: what it handed is what
            // committed, and the other hosts read its rows from their
            // cursors; the command is not answered as refused.
            Err(why) => {
                eprintln!("pw dev server: the feed's outbox was not read back: {why}");
                Ok(crate::data::Commit::Applied((
                    events.to_vec(),
                    invalidated.to_vec(),
                )))
            }
        }
    }
}

/// **Whether a committed post names the blob `key`** (ADR-0261).
/// **An interaction's record, in its command's transaction** (ADR-0325):
/// those recorded more than a day before forgotten, then this one. A key
/// recorded already, by another host, refuses it, and the command's
/// transaction rolls back.
pub(crate) fn record(c: &mut Client, r: &crate::data::Recorded<'_>) -> Result<(), String> {
    let before = i64::try_from(r.forget_before_ms).unwrap_or(i64::MAX);
    let at = i64::try_from(r.at_ms).unwrap_or(i64::MAX);
    c.execute("DELETE FROM interactions WHERE at_ms < $1", &[&before])
        .map_err(pg)?;
    c.execute(
        "INSERT INTO interactions (key, sent, answer, at_ms) VALUES ($1, $2, $3, $4)",
        &[&r.key, &r.sent, &r.answer, &at],
    )
    .map_err(|e| format!("interaction not recorded: {}", pg(e)))?;
    Ok(())
}

/// **An interaction's committed record** (ADR-0325), recorded at
/// `since_ms` or later: the arguments it was sent with, and its answer.
pub(crate) fn recalled(
    c: &mut Client,
    key: &str,
    since_ms: u64,
) -> Result<Option<(String, String)>, postgres::Error> {
    let since = i64::try_from(since_ms).unwrap_or(i64::MAX);
    Ok(c.query_opt(
        "SELECT sent, answer FROM interactions WHERE key = $1 AND at_ms >= $2",
        &[&key, &since],
    )?
    .map(|row| (row.get(0), row.get(1))))
}

fn names_blob(c: &mut Client, key: &str) -> Result<bool, postgres::Error> {
    c.query_one(
        "SELECT EXISTS (SELECT 1 FROM posts WHERE image_key = $1)",
        &[&key],
    )
    .map(|row| row.get(0))
}

/// **What the outbox committed, read back**: what is delivered is what was
/// committed, and nothing else. Its rows are kept for the other hosts on
/// the database, which read them from their cursors, until they are trimmed
/// (ADR-0369, `hosts.rs`); until then a delivery consumed them, and a second
/// host heard nothing.
pub(crate) fn delivered(c: &mut Client, ids: &[i64]) -> Result<Outboxed, String> {
    let rows = c
        .query(
            "SELECT id, kind, name, args::text FROM outbox WHERE id = ANY($1)",
            &[&ids],
        )
        .map_err(pg)?;
    outboxed_of(
        rows.iter()
            .map(|r| (r.get(0), r.get(1), r.get(2), r.get(3)))
            .collect(),
    )
}

/// **The outbox's rows as what was handed it**: each row's id, kind, name
/// and values, in the order of their ids, the events and then the
/// invalidated entries.
pub(crate) fn outboxed_of(
    mut read: Vec<(i64, String, String, String)>,
) -> Result<Outboxed, String> {
    read.sort();
    let (mut emitted, mut dropped) = (Vec::new(), Vec::new());
    for (_, kind, name, args) in read {
        let values: Vec<serde_json::Value> =
            serde_json::from_str(&args).map_err(|e| format!("the outbox's `{name}`: {e}"))?;
        match kind.as_str() {
            "event" => {
                let values = values.iter().map(val_of).collect::<Result<Vec<_>, _>>()?;
                emitted.push((name, values));
            }
            // An invalidated entry's `null` is every value at its position
            // (ADR-0256).
            _ => {
                let values = values
                    .iter()
                    .map(|v| match v {
                        serde_json::Value::Null => Ok(None),
                        v => val_of(v).map(Some),
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                dropped.push((name, values));
            }
        }
    }
    Ok((emitted, dropped))
}

impl Drop for Staging<'_> {
    fn drop(&mut self) {
        let Some(mut c) = self.conn.lock().expect("the command's connection").take() else {
            return;
        };
        // Roll back an unacknowledged transaction when possible. A transport
        // failure around COMMIT may have committed; queue/receipt state reconciles it.
        if !self.committed && c.batch_execute("ROLLBACK").is_err() {
            return;
        }
        self.pool.give(c);
    }
}

/// An operation of a command, run in its transaction: `wrote` says whether
/// it wrote.
type InTransaction =
    dyn Fn(&mut Client, &[Val]) -> Result<(Val, bool), String> + Send + Sync + 'static;

/// **Its runs, outside a command** (the scheduled-commands track): those
/// its `schedules` keeps, each statement on a connection of its own.
impl crate::schedules::Runs for FeedPg {
    fn authority(&self) -> &pw_materialize::QueueAuthority {
        &self.queue_authority
    }
    fn current(
        &self,
        expected: &pw_materialize::ScheduledRun,
        now_ms: i64,
    ) -> Result<bool, String> {
        self.pool
            .with(|c| crate::schedules::pg::current(c, expected, now_ms))
    }
    fn retry_current(
        &self,
        expected: &pw_materialize::ScheduledRun,
        attempts: i64,
        due_ms: i64,
        failed: Option<&str>,
    ) -> Result<bool, String> {
        let _writes = self.writes.lock().unwrap_or_else(|e| e.into_inner());
        self.pool
            .with(|c| crate::schedules::pg::retry_current(c, expected, attempts, due_ms, failed))
    }
    fn due_runs(
        &self,
        now_ms: i64,
        limit: usize,
    ) -> Result<Vec<pw_materialize::ScheduledRun>, String> {
        self.pool
            .with(|c| crate::schedules::pg::due_runs(c, now_ms, limit))
    }

    fn next_run(&self) -> Result<Option<i64>, String> {
        self.pool.with(crate::schedules::pg::next_run)
    }

    #[cfg(test)]
    fn ran(&self, id: i64) -> Result<(), String> {
        let _writes = self.writes.lock().unwrap_or_else(|e| e.into_inner());
        self.pool.with(|c| crate::schedules::pg::ran(c, id))
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
        self.pool
            .with(|c| crate::schedules::pg::retry(c, id, attempts, due_ms, failed))
    }

    #[cfg(test)]
    fn all_runs(&self) -> Result<Vec<pw_materialize::ScheduledRun>, String> {
        self.pool.with(crate::schedules::pg::all_runs)
    }
}

impl crate::data::DataLayer for FeedPg {
    fn reads(&self, _session: &str, _stopped: Option<Stopped>) -> crate::data::Ops {
        let pool = self.pool.clone();
        reads_through(
            Arc::new(move |f: Read<'_>| pool.with(|c| Ok(f(c))).and_then(|r| r)),
            self.principals(),
            self.uploaded.get().cloned(),
        )
    }

    // TRACK SEAM (render-merge): a render's snapshot, its seat (waited for
    // while every seat is taken) and the read transaction it holds, and each
    // of its queries' reads in it (NEXT 4c).
    fn snapshot(&self) -> Option<crate::data::Snapshot> {
        Some(Box::new(Arc::new(self.renders.seat())))
    }

    fn reads_in(
        &self,
        snapshot: &crate::data::Snapshot,
        session: &str,
        stopped: Option<Stopped>,
    ) -> crate::data::Ops {
        // Never another layer's: a render's snapshot is its layer's
        // (`render_merge`, by the layer's address).
        let Some(seat) = snapshot.downcast_ref::<Arc<Seat>>() else {
            return self.reads(session, stopped);
        };
        let seat = seat.clone();
        reads_through(
            Arc::new(move |f: Read<'_>| seat.read(|c| f(c))),
            self.principals(),
            self.uploaded.get().cloned(),
        )
    }

    /// Another writer's commit, between a render's reads (NEXT 4c's tests).
    #[cfg(test)]
    fn execute_for_test(&self, sql: &str) -> Result<(), String> {
        self.pool.with(|c| c.batch_execute(sql))
    }

    /// The renders' connections (NEXT 4c's tests).
    #[cfg(test)]
    fn renders_for_test(&self) -> Option<Arc<Renders>> {
        Some(self.renders.clone())
    }

    fn uploaded_by(&self, leases: crate::uploads::Leases) {
        let _ = self.uploaded.set(leases);
    }

    fn names_blob(&self, key: &str) -> Result<bool, String> {
        self.pool.with(|c| names_blob(c, key))
    }

    fn keeps_interactions(&self) -> bool {
        true
    }

    fn recalled(&self, key: &str, since_ms: u64) -> Result<Option<(String, String)>, String> {
        self.pool.with(|c| recalled(c, key, since_ms))
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

    fn identified_by(&self, principals: Principals) {
        *self.principals.write().expect("principals") = principals;
    }

    fn begin<'a>(&'a self, _session: &str) -> Box<dyn crate::data::Staged + 'a> {
        let writes = self.writes.lock().unwrap_or_else(|e| e.into_inner());
        // The transaction, open before the command runs: what it reads, it
        // reads in it. A connection or a `BEGIN` that fails leaves none, and
        // every operation of the command fails with why.
        let opened =
            self.pool
                .take()
                .and_then(|mut c| match begin_command(&mut c, self.isolation) {
                    Ok(()) => Ok(c),
                    Err(e) => Err(pg(e)),
                });
        let (conn, why) = match opened {
            Ok(c) => (Some(c), String::new()),
            Err(why) => (None, why),
        };
        let conn = Arc::new(Mutex::new(conn));
        let wrote: Arc<AtomicBool> = Arc::default();
        let run: Run = {
            let (conn, why) = (conn.clone(), why.clone());
            Arc::new(move |f: Read<'_>| {
                match conn.lock().expect("the command's connection").as_mut() {
                    Some(c) => f(c),
                    None => Err(why.clone()),
                }
            })
        };
        let principals = self.principals();
        let leases = self.uploaded.get().cloned();
        let mut ops = reads_through(run, principals.clone(), leases.clone());
        // Track `uploads`: a command claims its session's attached image,
        // commits it with a post, or discards it.
        let claims = crate::uploads::Claims::new(leases.clone());
        ops.insert(
            "feed:data/uploads#claim".to_string(),
            crate::uploads::claim_op(claims.clone()),
        );
        let discarded = wrote.clone();
        ops.insert(
            "feed:data/uploads#discard".to_string(),
            crate::uploads::discard_op(claims.clone(), move || {
                // A discard commits, so its lease ends with the transaction.
                discarded.store(true, Ordering::SeqCst)
            }),
        );
        let released: Arc<Mutex<Vec<String>>> = Arc::default();
        let mut write = |key: &str, op: Arc<InTransaction>| {
            let (conn, why, wrote) = (conn.clone(), why.clone(), wrote.clone());
            ops.insert(
                key.to_string(),
                Arc::new(move |args: &[Val]| {
                    let mut conn = conn.lock().expect("the command's connection");
                    let c = conn.as_mut().ok_or_else(|| why.clone())?;
                    let (answer, written) = op(c, args)?;
                    if written {
                        wrote.store(true, Ordering::SeqCst);
                    }
                    Ok(vec![answer])
                }),
            );
        };
        let (p, l) = (principals.clone(), leases.clone());
        write(
            "feed:data/posts#publish",
            Arc::new(move |c: &mut Client, args: &[Val]| match args {
                [Val::String(session), Val::String(text)] => {
                    let author = author(c, &p, session)?;
                    let row = c
                        .query_one(
                            "INSERT INTO posts (author, text) VALUES ($1, $2) RETURNING id",
                            &[&author, text],
                        )
                        .map_err(pg)?;
                    let id: String = row.get(0);
                    let post = post_alone(c, &id, l.as_ref())
                        .map_err(pg)?
                        .ok_or("a post just written")?;
                    Ok((ok(post), true))
                }
                other => Err(format!("posts#publish received {other:?}")),
            }),
        );
        // **A post with its image** (track `uploads`): the image the
        // command's claim holds, written with the post in its transaction, by
        // the author `publish` writes as. A handle this command did not claim
        // is not found.
        let (p, l, holding) = (principals.clone(), leases.clone(), claims.clone());
        write(
            "feed:data/posts#publish-image",
            Arc::new(move |c: &mut Client, args: &[Val]| match args {
                [
                    Val::String(session),
                    Val::String(text),
                    Val::String(alt),
                    Val::String(upload),
                ] => {
                    let Some(image) = holding.lock().expect("claims").commit(session, upload, alt)
                    else {
                        return Ok((not_found(), false));
                    };
                    let author = author(c, &p, session)?;
                    let row = c
                        .query_one(
                            "INSERT INTO posts (author, text, image_key, image_kind, \
                                 image_width, image_height, image_alt) \
                             VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING id",
                            &[
                                &author,
                                text,
                                &image.key,
                                &image.kind.word(),
                                &(image.width as i32),
                                &(image.height as i32),
                                &image.alt,
                            ],
                        )
                        .map_err(pg)?;
                    let id: String = row.get(0);
                    let post = post_alone(c, &id, l.as_ref())
                        .map_err(pg)?
                        .ok_or("a post just written")?;
                    Ok((ok(post), true))
                }
                other => Err(format!("posts#publish-image received {other:?}")),
            }),
        );
        // **A reply** (ADR-0231): a post that replies to `to`. To a post that
        // is not there, none.
        let (p, l) = (principals.clone(), leases.clone());
        write(
            "feed:data/posts#reply",
            Arc::new(move |c: &mut Client, args: &[Val]| match args {
                [Val::String(session), Val::String(to), Val::String(text)] => {
                    let there = c
                        .query_opt("SELECT 1 FROM posts WHERE id = $1", &[to])
                        .map_err(pg)?
                        .is_some();
                    if !there {
                        return Ok((not_found(), false));
                    }
                    let author = author(c, &p, session)?;
                    let row = c
                        .query_one(
                            "INSERT INTO posts (author, text, reply_to) VALUES ($1, $2, $3) \
                             RETURNING id",
                            &[&author, text, to],
                        )
                        .map_err(pg)?;
                    let id: String = row.get(0);
                    // Track `notifications`: the replied-to post's author
                    // told, in the reply's transaction, unless it is theirs.
                    crate::notifications::pg::replied(c, &id, &author).map_err(pg)?;
                    let post = post_alone(c, &id, l.as_ref())
                        .map_err(pg)?
                        .ok_or("a reply just written")?;
                    Ok((ok(post), true))
                }
                other => Err(format!("posts#reply received {other:?}")),
            }),
        );
        // **A follow** (ADR-0257): once, however often; one the feed does
        // not know, or the follower itself, is not found, and nothing is
        // written. A follow again writes what is there, and commits, so its
        // event refreshes every page that shows it.
        let p = principals.clone();
        write(
            "feed:data/users#follow",
            Arc::new(move |c: &mut Client, args: &[Val]| match args {
                [Val::String(session), Val::String(user)] => {
                    let follower = p.user_of(session);
                    let known: bool = c
                        .query_one(&format!("SELECT {KNOWN}"), &[user])
                        .map_err(pg)?
                        .get(0);
                    if follower == *user || !known {
                        return Ok((not_found(), false));
                    }
                    let new = c
                        .execute(
                            "INSERT INTO follows (follower, followee) VALUES ($1, $2) \
                             ON CONFLICT DO NOTHING",
                            &[&follower, user],
                        )
                        .map_err(pg)?;
                    // Track `notifications`: the one followed told, once:
                    // following again writes nothing, and tells no one.
                    if new == 1 {
                        crate::notifications::pg::followed(c, user, &follower).map_err(pg)?;
                    }
                    Ok((ok(Val::String(user.clone())), true))
                }
                other => Err(format!("users#follow received {other:?}")),
            }),
        );
        // **An unfollow** (ADR-0257): one not followed is no change.
        let p = principals.clone();
        write(
            "feed:data/users#unfollow",
            Arc::new(move |c: &mut Client, args: &[Val]| match args {
                [Val::String(session), Val::String(user)] => {
                    let known: bool = c
                        .query_one(&format!("SELECT {KNOWN}"), &[user])
                        .map_err(pg)?
                        .get(0);
                    if !known {
                        return Ok((not_found(), false));
                    }
                    c.execute(
                        "DELETE FROM follows WHERE follower = $1 AND followee = $2",
                        &[&p.user_of(session), user],
                    )
                    .map_err(pg)?;
                    Ok((ok(Val::String(user.clone())), true))
                }
                other => Err(format!("users#unfollow received {other:?}")),
            }),
        );
        // Track `notifications`: a user's notifications, read.
        write(
            "feed:data/notifications#read",
            Arc::new(|c: &mut Client, args: &[Val]| match args {
                [Val::String(reader)] => {
                    let read = crate::notifications::pg::read_all(c, reader).map_err(pg)?;
                    Ok((ok(Val::S64(read as i64)), true))
                }
                other => Err(format!("notifications#read received {other:?}")),
            }),
        );
        // **A post deleted** (track `identity`): it, every reply under it,
        // and their likes, in one statement, so one snapshot. Track
        // `notifications`: and their notifications, by their foreign key's
        // `ON DELETE CASCADE`, in the same statement. Who may is
        // `requires OwnsPost(post)`'s, evaluated in this transaction before
        // the command runs.
        write(
            "feed:data/posts#delete",
            Arc::new({
                let released = released.clone();
                move |c: &mut Client, args: &[Val]| match args {
                    [Val::String(_), Val::String(id)] => {
                        // And the images of what it deletes (ADR-0261), each
                        // collected once the command commits.
                        let row = c
                            .query_one(
                                "WITH RECURSIVE t AS ( \
                                 SELECT id FROM posts WHERE id = $1 \
                                 UNION ALL \
                                 SELECT p.id FROM posts p JOIN t ON p.reply_to = t.id), \
                             unliked AS (DELETE FROM likes WHERE post IN (SELECT id FROM t)), \
                             deleted AS (DELETE FROM posts WHERE id IN (SELECT id FROM t) \
                                 RETURNING id, image_key) \
                             SELECT count(*), \
                                 COALESCE(array_remove(array_agg(DISTINCT image_key), NULL), '{}') \
                             FROM deleted",
                                &[id],
                            )
                            .map_err(pg)?;
                        let gone: i64 = row.get(0);
                        if gone == 0 {
                            return Ok((not_found(), false));
                        }
                        let keys: Vec<String> = row.get(1);
                        released.lock().expect("released").extend(keys);
                        Ok((ok(Val::String(id.clone())), true))
                    }
                    other => Err(format!("posts#delete received {other:?}")),
                }
            }),
        );
        // Track `messages` (ADR-0279): a message from the session's user,
        // written by the author a post is, and the sender's side read to it,
        // in the command's transaction. Who may is `requires
        // MayMessage(to)`'s, evaluated in this transaction before the command
        // runs.
        let p = principals.clone();
        write(
            "feed:data/messages#send",
            Arc::new(move |c: &mut Client, args: &[Val]| match args {
                [Val::String(session), Val::String(to), Val::String(text)] => {
                    let from = author(c, &p, session)?;
                    let sent = crate::messages::pg::send(c, &from, to, text).map_err(pg)?;
                    Ok((ok(sent), true))
                }
                other => Err(format!("messages#send received {other:?}")),
            }),
        );
        // Track `messages`: a reader's conversation, read.
        write(
            "feed:data/messages#read",
            Arc::new(|c: &mut Client, args: &[Val]| match args {
                [Val::String(reader), Val::String(with)] => {
                    let read = crate::messages::pg::read(c, reader, with).map_err(pg)?;
                    Ok((ok(Val::S64(read)), true))
                }
                other => Err(format!("messages#read received {other:?}")),
            }),
        );
        let (p, l) = (principals, leases.clone());
        write(
            "feed:data/posts#like",
            Arc::new(move |c: &mut Client, args: &[Val]| match args {
                [Val::String(session), Val::String(id)] => {
                    let there = c
                        .query_opt("SELECT 1 FROM posts WHERE id = $1", &[id])
                        .map_err(pg)?
                        .is_some();
                    if !there {
                        return Ok((not_found(), false));
                    }
                    let liker = p.user_of(session);
                    c.execute(
                        "INSERT INTO likes (post, liker) VALUES ($1, $2)",
                        &[id, &liker],
                    )
                    .map_err(pg)?;
                    // Track `notifications`: the post's author told, in the
                    // like's transaction, unless it is theirs.
                    crate::notifications::pg::liked(c, id, &liker).map_err(pg)?;
                    let post = post_alone(c, id, l.as_ref())
                        .map_err(pg)?
                        .ok_or("a post just liked")?;
                    Ok((ok(post), true))
                }
                other => Err(format!("posts#like received {other:?}")),
            }),
        );
        Box::new(Staging {
            queue_authority: &self.queue_authority,
            _writes: writes,
            pool: self.pool.clone(),
            conn,
            wrote,
            committed: false,
            ops,
            claims,
            leases,
            released,
            outbox: self.tail.outbox(),
            host: self.tail.host(),
        })
    }

    fn grants(&self) -> Vec<&'static str> {
        crate::feed::GRANTS.to_vec()
    }

    fn default_page(&self) -> Option<&'static str> {
        None
    }

    /// **What the database provides, measured** (ADR-0246): a transaction
    /// opened as a command's is, asked its isolation and whether it may
    /// write.
    /// - Serializable is `serializable`; PostgreSQL's repeatable read is
    ///   snapshot isolation, `snapshot`; read committed is `read_committed`.
    /// - A transaction that may write is on the primary, whose statements
    ///   read the latest commit: `strong`. A hot standby's are read only
    ///   and refuse serializable, so the layer opens on none.
    /// - It delivers no feed of its changes: the program's events are its
    ///   outbox's.
    fn provides(&self) -> Result<Provided, String> {
        let mut c = self.pool.take()?;
        let measured = begin_command(&mut c, self.isolation).and_then(|()| {
            let row = c.query_one(
                "SELECT current_setting('transaction_isolation'), \
                        current_setting('transaction_read_only')",
                &[],
            );
            // Whatever the query did, the transaction ends here.
            c.batch_execute("ROLLBACK")?;
            row
        });
        self.pool.give(c);
        let row = measured.map_err(pg)?;
        let (isolation, read_only): (String, String) = (row.get(0), row.get(1));
        if read_only == "on" {
            return Err(
                "the feed's database opens a command's transaction read only: no write commits"
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
