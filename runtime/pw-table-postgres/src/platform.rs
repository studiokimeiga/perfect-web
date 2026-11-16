//! **The platform's tables beside a program's, and what a command does in
//! them** (ADR-0338 1.3 and 5.2; track `durable-tables`): the runs commands
//! schedule (ADR-0334), and the sessions and sign-ins a deployment keeps
//! where its hosts share them (ADR-0369), in the adapter's schema, made by
//! its platform migration `0002_runs_and_sessions` (`migrate.rs`).
//!
//! - **A command's runs are statements of its transaction**:
//!   [`Command::consume`] takes the run the command completes (first: a run
//!   that schedules itself again is taken before its next generation is
//!   written), and [`Command::schedule`] writes each scheduling (the run
//!   pending for its key replaced, or cancelled). Both commit with the
//!   command's rows, its interaction's record and its outbox
//!   ([`crate::Prepared::publish_with`]), or not at all: a host never keeps
//!   a run whose scheduling command rolled back, or a scheduling whose run it
//!   lost. A run no longer the one selected is told apart from one whose
//!   deletion another hand suppressed: the first is not current, the second
//!   a refusal.
//! - **A command acts for a session it holds to its end**
//!   ([`PgTables::begin_fenced`], the PostgreSQL layers' fence of ADR-0369):
//!   the session's lock taken on the command's own connection before the
//!   command takes the host's turn, so a command waiting for its session
//!   holds no other session's command up; its transaction begun after the
//!   wait; the session's row locked in it; and the session as the command
//!   was admitted, still, at its start and again just before its `COMMIT`.
//!   The lock is given back once the command ends. A command is fenced from
//!   its first statement or never: there is no fencing a command that ran.
//! - **Sessions and sign-ins**, on connections of their own (at most
//!   [`SESSION_CONNECTIONS`]): an admission (its use recorded, one past its
//!   lifetimes forgotten), an opening and a closing, each a read-committed
//!   transaction under the session's lock, its connection closed rather than
//!   kept wherever it does not commit; an inspection, which reads and locks
//!   nothing; and sweeps, a bounded batch at a time, of the sessions past
//!   their lifetimes and the sign-ins past theirs (a sign-in's nonce and PKCE
//!   verifier are kept no longer than it lives).
//! - **The outbox's delivered rows forgotten**: a host that reads no other
//!   host's commits forgets each commit's rows once it has delivered them
//!   ([`PgTables::forget_outbox`]), and those a host that ended left behind
//!   ([`PgTables::trim_outbox`]).

use std::time::Duration;

use crate::pool::{Conn, Lease};
use crate::sql::ident;
use crate::{Command, Error, Inner, PgTables, database};

/// **How long a fenced command, or a session's change, waits for the
/// session's lock or a row**, as the other PostgreSQL layers wait
/// (ADR-0369's five seconds), set in each transaction that may wait.
const LOCK_TIMEOUT: &str = "SET LOCAL lock_timeout = '5s'";

/// **How long any statement on a session connection waits for a lock**, set
/// once on each (ADR-0389, amended 2026-11-02, as the other PostgreSQL
/// layers' session connections): an admission's read waits on no session
/// lock, but a lock on the table itself (a migration's) would hold it; past
/// the bound it fails, and its request is refused.
pub(crate) const SESSION_CONNECTION_LOCK_TIMEOUT: &str = "SET lock_timeout = '5s'";

/// **How many connections the sessions are read and written on at once**:
/// the PostgreSQL layers' bound (ADR-0369's `SESSION_CONNECTIONS`), apart
/// from the connections commands and reads take, so that a reader's
/// admission never waits for a place a command holds.
pub const SESSION_CONNECTIONS: usize = 8;

/// **A run a command scheduled**, as the platform keeps it (ADR-0334): its
/// command and arguments, the user it runs for, when it is due on the host's
/// clock, how many times it failed, and why it was kept failed for good.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    pub id: i64,
    pub command: String,
    pub args: String,
    pub user: String,
    pub due_ms: i64,
    pub attempts: i64,
    pub failed: Option<String>,
}

/// **A scheduling, as a command's commit writes it**: the run pending for
/// `key` replaced by one due at `due_ms`, or cancelled where it is none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scheduling {
    pub key: String,
    pub command: String,
    pub args: String,
    pub user: String,
    pub due_ms: Option<i64>,
}

/// **A session as it is kept** (ADR-0369): the principal it is, and the
/// generation it was opened in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub user: String,
    pub handle: String,
    pub name: String,
    pub issuer: String,
    pub epoch: [u8; 16],
}

/// **How long a session lives**, idle and in all, on the database's clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lifetimes {
    pub idle: Duration,
    pub absolute: Duration,
}

/// **What a command is fenced on**: the session it acts for (its id's
/// SHA-256), the lifetimes it is held to, and the session the command was
/// admitted as, none where it was admitted as no one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fence {
    pub hash: Vec<u8>,
    pub lifetimes: Lifetimes,
    pub expected: Option<Session>,
}

/// A command's fence, and the key of the session's lock its connection
/// holds from before its turn to its end.
#[derive(Debug)]
pub(crate) struct Fenced {
    pub(crate) fence: Fence,
    pub(crate) key: i64,
}

/// **The platform's statements for runs and sessions**, in the adapter's
/// schema, each prepared once on each connection that runs it.
pub(crate) struct Statements {
    due: String,
    next: String,
    one: String,
    held_run: String,
    retry: String,
    consume: String,
    unschedule: String,
    schedule: String,
    every: String,
    forget: String,
    set: String,
    live: String,
    row: String,
    admission: String,
    admit: String,
    open: String,
    close: String,
    begin_sign_in: String,
    take_sign_in: String,
    sweep_sessions: String,
    sweep_sign_ins: String,
    forget_outbox: String,
    trim_outbox: String,
}

const RUN: &str = "id, command, args, user_id, due_ms, attempts, failed";
const SESSION: &str = "user_id, handle, name, issuer, epoch";

impl Statements {
    pub(crate) fn of(schema: &str) -> Statements {
        let s = ident(schema);
        let runs = format!("{s}.pw_runs");
        let sessions = format!("{s}.pw_sessions");
        let sign_ins = format!("{s}.pw_sign_ins");
        let outbox = format!("{s}.pw_outbox");
        let watermark = format!("{s}.pw_outbox_trimmed");
        // A session alive by both lifetimes on the database's clock; and its
        // use due to be recorded, last recorded more than `$4` seconds ago.
        let lifetimes = "opened > clock_timestamp() - make_interval(secs => $2) \
             AND seen > clock_timestamp() - make_interval(secs => $3)";
        let alive = format!("hash = $1 AND {lifetimes}");
        let stale = "seen < clock_timestamp() - make_interval(secs => $4)";
        Statements {
            due: format!(
                "SELECT {RUN} FROM {runs} WHERE failed IS NULL AND due_ms <= $1 \
                 ORDER BY due_ms, id LIMIT $2"
            ),
            next: format!("SELECT min(due_ms) FROM {runs} WHERE failed IS NULL"),
            one: format!("SELECT {RUN} FROM {runs} WHERE id = $1"),
            held_run: format!("SELECT {RUN} FROM {runs} WHERE id = $1 FOR UPDATE"),
            retry: format!(
                "UPDATE {runs} SET attempts = $7, due_ms = $8, failed = $9 \
                 WHERE id = $1 AND command = $2 AND args = $3 AND user_id = $4 \
                 AND due_ms = $5 AND attempts = $6 AND failed IS NOT DISTINCT FROM $10 \
                 RETURNING id"
            ),
            consume: format!(
                "DELETE FROM {runs} WHERE id = $1 AND command = $2 AND args = $3 \
                 AND user_id = $4 AND due_ms = $5 AND attempts = $6 AND failed IS NULL \
                 AND due_ms <= $7 RETURNING id"
            ),
            unschedule: format!("DELETE FROM {runs} WHERE key = $1"),
            schedule: format!(
                "INSERT INTO {runs} (key, command, args, user_id, due_ms) \
                 VALUES ($1, $2, $3, $4, $5)"
            ),
            every: format!("SELECT {RUN} FROM {runs} ORDER BY due_ms, id"),
            forget: format!("DELETE FROM {runs} WHERE id = $1"),
            set: format!("UPDATE {runs} SET attempts = $2, due_ms = $3, failed = $4 WHERE id = $1"),
            live: format!("SELECT {SESSION} FROM {sessions} WHERE {alive}"),
            row: format!("SELECT 1 FROM {sessions} WHERE hash = $1 FOR UPDATE"),
            // An admission's read: the session's row, whether it lives, and
            // whether its use is due to be recorded. One statement, in no
            // transaction of its own, taking no lock of a session's.
            admission: format!(
                "SELECT {SESSION}, ({lifetimes}) AS live, ({stale}) AS stale \
                 FROM {sessions} WHERE hash = $1"
            ),
            // An admission's write, made only under the session's lock: its
            // use recorded where that is due, and one past either lifetime
            // forgotten. Its answer is the live row.
            admit: format!(
                "WITH live AS (SELECT {SESSION}, seen FROM {sessions} WHERE {alive}), \
                 touched AS (UPDATE {sessions} SET seen = clock_timestamp() WHERE hash = $1 \
                     AND EXISTS (SELECT 1 FROM live WHERE {stale})), \
                 gone AS (DELETE FROM {sessions} WHERE hash = $1 \
                     AND NOT EXISTS (SELECT 1 FROM live)) \
                 SELECT {SESSION} FROM live"
            ),
            open: format!(
                "INSERT INTO {sessions} (hash, user_id, handle, name, issuer, epoch) \
                 VALUES ($1, $2, $3, $4, $5, $6) \
                 ON CONFLICT (hash) DO UPDATE SET user_id = EXCLUDED.user_id, \
                 handle = EXCLUDED.handle, name = EXCLUDED.name, issuer = EXCLUDED.issuer, \
                 epoch = EXCLUDED.epoch, opened = clock_timestamp(), seen = clock_timestamp()"
            ),
            close: format!("DELETE FROM {sessions} WHERE hash = $1"),
            begin_sign_in: format!(
                "WITH gone AS (DELETE FROM {sign_ins} \
                     WHERE started < clock_timestamp() - interval '10 minutes') \
                 INSERT INTO {sign_ins} (hash, nonce, verifier) VALUES ($1, $2, $3)"
            ),
            take_sign_in: format!(
                "DELETE FROM {sign_ins} WHERE hash = $1 RETURNING nonce, verifier, \
                 (extract(epoch FROM clock_timestamp() - started) * 1000)::bigint"
            ),
            // A bounded batch, never waiting: a row a command or a session's
            // change holds is the next sweep's.
            sweep_sessions: format!(
                "DELETE FROM {sessions} WHERE hash IN (SELECT hash FROM {sessions} \
                 WHERE opened <= clock_timestamp() - make_interval(secs => $1) \
                 OR seen <= clock_timestamp() - make_interval(secs => $2) \
                 LIMIT $3 FOR UPDATE SKIP LOCKED) RETURNING 1"
            ),
            sweep_sign_ins: format!(
                "DELETE FROM {sign_ins} WHERE hash IN (SELECT hash FROM {sign_ins} \
                 WHERE started < clock_timestamp() - make_interval(secs => $1) \
                 LIMIT $2 FOR UPDATE SKIP LOCKED) RETURNING 1"
            ),
            forget_outbox: format!(
                "WITH gone AS (DELETE FROM {outbox} WHERE id = ANY($1) RETURNING id), \
                 marked AS (UPDATE {watermark} SET through = greatest(through, \
                     coalesce((SELECT max(id) FROM gone), 0))) SELECT 1 FROM gone"
            ),
            trim_outbox: format!(
                "WITH gone AS (DELETE FROM {outbox} WHERE id IN (SELECT id FROM {outbox} \
                 WHERE created < clock_timestamp() - make_interval(secs => $1) \
                 ORDER BY id LIMIT $2 FOR UPDATE SKIP LOCKED) RETURNING id), \
                 marked AS (UPDATE {watermark} SET through = greatest(through, \
                     coalesce((SELECT max(id) FROM gone), 0))) SELECT 1 FROM gone"
            ),
        }
    }
}

/// **The key a session's lock is taken by**: its schema's and its hash's,
/// so that two schemas in one database (two programs, two tests) never wait
/// on each other, and a command and a session's change on one session
/// always do. FNV-1a, as the adapter's other locks.
pub(crate) fn session_key(schema: &str, hash: &[u8]) -> i64 {
    let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
    crate::migrate::lock_key(&format!("pw_session:{schema}:{hex}"))
}

fn run_of(row: &postgres::Row) -> Result<Run, Error> {
    let read = |e: postgres::Error| Error::Database(format!("a run as kept: {e}"));
    Ok(Run {
        id: row.try_get(0).map_err(read)?,
        command: row.try_get(1).map_err(read)?,
        args: row.try_get(2).map_err(read)?,
        user: row.try_get(3).map_err(read)?,
        due_ms: row.try_get(4).map_err(read)?,
        attempts: row.try_get(5).map_err(read)?,
        failed: row.try_get(6).map_err(read)?,
    })
}

fn session_of(row: &postgres::Row) -> Result<Session, Error> {
    let read = |e: postgres::Error| Error::Database(format!("a session as kept: {e}"));
    let epoch: Vec<u8> = row.try_get(4).map_err(read)?;
    Ok(Session {
        user: row.try_get(0).map_err(read)?,
        handle: row.try_get(1).map_err(read)?,
        name: row.try_get(2).map_err(read)?,
        issuer: row.try_get(3).map_err(read)?,
        epoch: epoch
            .try_into()
            .map_err(|_| Error::Database("a session's generation is not 16 bytes".into()))?,
    })
}

/// **The session a fenced command acts for, still as it was admitted**: one
/// statement, in the command's transaction. Another (signed out, signed in
/// as another, past its lifetimes) refuses the command.
fn still(inner: &Inner, conn: &mut Conn, fence: &Fence) -> Result<(), Error> {
    let rows = conn
        .query(
            &inner.counters,
            &inner.places.live,
            &[
                Box::new(fence.hash.clone()),
                Box::new(fence.lifetimes.absolute.as_secs_f64()),
                Box::new(fence.lifetimes.idle.as_secs_f64()),
            ],
        )
        .map_err(database)?;
    let now = rows.first().map(session_of).transpose()?;
    if now == fence.expected {
        Ok(())
    } else {
        Err(Error::Refused(
            "the session the command acts for changed before it committed".into(),
        ))
    }
}

/// **A connection that is kept only where its transaction ended as it
/// meant to** (committed, or rolled back on purpose): a session's change or
/// write marks its connection so before it begins (as the other PostgreSQL
/// layers' session changes do), and closes it on every other way out, a
/// panic's unwinding among them: whether such a connection's transaction or
/// lock is still held is not known.
struct KeptIfEnded<'a> {
    lease: &'a mut Lease,
    ended: bool,
}

impl Drop for KeptIfEnded<'_> {
    fn drop(&mut self) {
        if !self.ended {
            self.lease.discard();
        }
    }
}

impl Inner {
    /// **The session's lock, taken on a connection of the pool's** in a
    /// transaction of its own that ends at once (a session lock outlives
    /// it), outside any command's turn. Failed, the connection is closed,
    /// which gives back whatever lock it holds.
    fn lock_session(&self, key: i64) -> Result<Lease, Error> {
        let mut lease = self.pool.take()?;
        let locked = (|| {
            let conn = lease.conn()?;
            conn.batch(&self.counters, "BEGIN").map_err(database)?;
            conn.batch(&self.counters, LOCK_TIMEOUT).map_err(database)?;
            conn.query(
                &self.counters,
                "SELECT pg_advisory_lock($1)",
                &[Box::new(key)],
            )
            .map_err(database)?;
            conn.batch(&self.counters, "COMMIT").map_err(database)
        })();
        match locked {
            Ok(()) => Ok(lease),
            Err(error) => {
                lease.discard();
                Err(error)
            }
        }
    }

    /// **The session's lock given back**, on the connection that holds it,
    /// once its command has ended. An answer that is not "given back" (an
    /// error, or `false`: the lock was not held) closes the connection,
    /// which gives back whatever it holds.
    pub(crate) fn unfence(&self, lease: &mut Lease, key: i64) {
        let given = lease.conn().and_then(|conn| {
            conn.query(
                &self.counters,
                "SELECT pg_advisory_unlock($1)",
                &[Box::new(key)],
            )
            .map_err(database)
        });
        let released = matches!(
            given
                .as_ref()
                .map(|rows| rows.first().map(|r| r.try_get::<_, bool>(0))),
            Ok(Some(Ok(true)))
        );
        if !released {
            lease.discard();
        }
    }

    /// **A session's change**: one transaction at read committed (whatever
    /// the connection's default, so that a change that waited reads what
    /// the command it waited for committed), under the session's lock. Its
    /// connection is kept only where the transaction committed.
    fn session_change<T>(
        &self,
        hash: &[u8],
        change: impl FnOnce(&Inner, &mut Conn) -> Result<T, Error>,
    ) -> Result<T, Error> {
        let mut lease = self.sessions.take()?;
        let key = session_key(&self.names.schema, hash);
        let mut kept = KeptIfEnded {
            lease: &mut lease,
            ended: false,
        };
        let done = (|| {
            let conn = kept.lease.conn()?;
            conn.batch(&self.counters, "BEGIN ISOLATION LEVEL READ COMMITTED")
                .map_err(database)?;
            conn.batch(&self.counters, LOCK_TIMEOUT).map_err(database)?;
            conn.query(
                &self.counters,
                "SELECT pg_advisory_xact_lock($1)",
                &[Box::new(key)],
            )
            .map_err(database)?;
            let out = change(self, conn)?;
            conn.batch(&self.counters, "COMMIT").map_err(database)?;
            Ok(out)
        })();
        match &done {
            Ok(_) => kept.ended = true,
            // Rolled back where it can be; closed either way.
            Err(_) => {
                if kept.lease.connected()
                    && let Ok(conn) = kept.lease.conn()
                {
                    let _ = conn.batch(&self.counters, "ROLLBACK");
                }
            }
        }
        done
    }

    /// **A due write, made where nobody holds the session's lock** (ADR-0389,
    /// amended 2026-11-02, as the other PostgreSQL layers' admissions make
    /// theirs), on `lease`: a transaction at read committed of its own that
    /// tries the session's lock and never waits for it. Taken, `write` runs
    /// under it, its snapshot after the lock, so no change or command of the
    /// session, which take the same lock, changes the row under it; and its
    /// answer is the write's. Held by another, nothing is written, and the
    /// answer is none. A failed statement closes the connection.
    fn written_if_free<T>(
        &self,
        lease: &mut Lease,
        hash: &[u8],
        write: impl FnOnce(&Inner, &mut Conn) -> Result<T, Error>,
    ) -> Result<Option<T>, Error> {
        let key = session_key(&self.names.schema, hash);
        let mut kept = KeptIfEnded {
            lease,
            ended: false,
        };
        let done = (|| {
            let conn = kept.lease.conn()?;
            conn.batch(&self.counters, "BEGIN ISOLATION LEVEL READ COMMITTED")
                .map_err(database)?;
            let free = conn
                .query(
                    &self.counters,
                    "SELECT pg_try_advisory_xact_lock($1)",
                    &[Box::new(key)],
                )
                .map_err(database)?
                .first()
                .map(|row| row.try_get::<_, bool>(0))
                .transpose()
                .map_err(|e| Error::Database(format!("the session's lock, tried: {e}")))?;
            if free != Some(true) {
                conn.batch(&self.counters, "ROLLBACK").map_err(database)?;
                return Ok(None);
            }
            let out = write(self, conn)?;
            conn.batch(&self.counters, "COMMIT").map_err(database)?;
            Ok(Some(out))
        })();
        match &done {
            Ok(_) => kept.ended = true,
            Err(_) => {
                if kept.lease.connected()
                    && let Ok(conn) = kept.lease.conn()
                {
                    let _ = conn.batch(&self.counters, "ROLLBACK");
                }
            }
        }
        done
    }

    /// One statement on a connection of the sessions' own.
    fn session_read(
        &self,
        sql: &str,
        params: &[crate::values::Param],
    ) -> Result<Vec<postgres::Row>, Error> {
        let mut lease = self.sessions.take()?;
        lease
            .conn()?
            .query(&self.counters, sql, params)
            .map_err(database)
    }

    /// One statement on a connection of the pool's, outside any command.
    fn run_read(
        &self,
        sql: &str,
        params: &[crate::values::Param],
    ) -> Result<Vec<postgres::Row>, Error> {
        let mut lease = self.pool.take()?;
        lease
            .conn()?
            .query(&self.counters, sql, params)
            .map_err(database)
    }
}

impl Command {
    /// **The session a fenced command acts for, held to its fence again**,
    /// in its transaction: what a command that writes nothing, or answers a
    /// declared error, is answered by. A command not fenced holds nothing.
    pub fn revalidate(&self) -> Result<(), Error> {
        let mut open = self.state.lock().map_err(|_| Error::StoragePoisoned)?;
        let open = &mut *open;
        let Some(fence) = open.fence.as_ref().map(|f| f.fence.clone()) else {
            return Ok(());
        };
        let inner = &self.inner;
        let held = (|| {
            let conn = inner.command_conn(open)?;
            still(inner, conn, &fence)
        })();
        if let Err(error) = &held {
            open.failed = Some(error.clone());
        }
        held
    }

    /// **The runs this command schedules, or cancels**, written in its
    /// transaction (ADR-0334): for each, the run pending for its key
    /// removed, and, where it is due, one made in its place, under a new id.
    /// A command that completes a run takes it first ([`Command::consume`]),
    /// so a run that schedules itself again is taken before its next
    /// generation replaces it.
    pub fn schedule(&mut self, runs: &[Scheduling]) -> Result<(), Error> {
        if runs.is_empty() {
            return Ok(());
        }
        let mut open = self.state.lock().map_err(|_| Error::StoragePoisoned)?;
        let open = &mut *open;
        if open.ended || open.prepared {
            return Err(Error::Ended);
        }
        let inner = &self.inner;
        let written = (|| -> Result<(), Error> {
            let conn = inner.command_conn(open)?;
            for run in runs {
                conn.query(
                    &inner.counters,
                    &inner.places.unschedule,
                    &[Box::new(run.key.clone())],
                )
                .map_err(database)?;
                if let Some(due) = run.due_ms {
                    conn.query(
                        &inner.counters,
                        &inner.places.schedule,
                        &[
                            Box::new(run.key.clone()),
                            Box::new(run.command.clone()),
                            Box::new(run.args.clone()),
                            Box::new(run.user.clone()),
                            Box::new(due),
                        ],
                    )
                    .map_err(database)?;
                }
            }
            Ok(())
        })();
        if let Err(error) = &written {
            open.failed = Some(error.clone());
        }
        written
    }

    /// **The run this command completes, taken in its transaction**: its
    /// row locked and held to the run selected (the same generation,
    /// unfailed, due at `now_ms`), then removed, for good once the command
    /// commits. `Ok(false)` where it is not the run selected any more, and
    /// nothing of the command should commit; a refusal where it is, and its
    /// deletion removed nothing (another hand's trigger or rule suppressed
    /// it), which must not be taken for a run already done.
    pub fn consume(&mut self, run: &Run, now_ms: i64) -> Result<bool, Error> {
        if run.failed.is_some() || run.due_ms > now_ms {
            return Ok(false);
        }
        let mut open = self.state.lock().map_err(|_| Error::StoragePoisoned)?;
        let open = &mut *open;
        if open.ended || open.prepared {
            return Err(Error::Ended);
        }
        let inner = &self.inner;
        let taken = (|| {
            let conn = inner.command_conn(open)?;
            let held = conn
                .query(&inner.counters, &inner.places.held_run, &[Box::new(run.id)])
                .map_err(database)?;
            if held.first().map(run_of).transpose()?.as_ref() != Some(run) {
                return Ok(false);
            }
            let removed = conn
                .query(
                    &inner.counters,
                    &inner.places.consume,
                    &[
                        Box::new(run.id),
                        Box::new(run.command.clone()),
                        Box::new(run.args.clone()),
                        Box::new(run.user.clone()),
                        Box::new(run.due_ms),
                        Box::new(run.attempts),
                        Box::new(now_ms),
                    ],
                )
                .map_err(database)?;
            if removed.len() == 1 {
                Ok(true)
            } else {
                Err(Error::Refused(format!(
                    "run {} is the run selected, and its deletion removed nothing",
                    run.id
                )))
            }
        })();
        if let Err(error) = &taken {
            open.failed = Some(error.clone());
        }
        taken
    }
}

/// The fence's check before a command's `COMMIT`, on its connection
/// (`publish_with`): nothing where the command is not fenced.
pub(crate) fn still_fenced(
    inner: &Inner,
    conn: &mut Conn,
    fence: Option<&Fence>,
) -> Result<(), Error> {
    match fence {
        Some(fence) => still(inner, conn, fence),
        None => Ok(()),
    }
}

impl PgTables {
    /// **A command fenced on the session it acts for** (ADR-0369): the
    /// session's lock taken first, on a connection of the pool's, outside
    /// the host's turn (a command waiting for its session holds no other
    /// session's command up; a session's commands are one at a time on a
    /// host already, so no two wait on each other); then the turn, as
    /// [`PgTables::begin`] takes it; then, on that connection, the command's
    /// transaction begun, the session's row locked in it, and the session
    /// held to `fence`'s `expected`, now and again just before the command
    /// commits ([`crate::Prepared::publish_with`]). The lock is held until
    /// the command ends. Refused, nothing of it ran and nothing is kept.
    pub fn begin_fenced(&self, fence: Fence) -> Result<Command, Error> {
        let key = session_key(&self.inner.names.schema, &fence.hash);
        let place = self.inner.lock_session(key)?;
        // The lock is the command's from here, given back as it ends, by
        // its commit, its refusal or its drop.
        let command = self.turn(
            place,
            Some(Fenced {
                fence: fence.clone(),
                key,
            }),
        )?;
        {
            let mut open = command.state.lock().map_err(|_| Error::StoragePoisoned)?;
            let open = &mut *open;
            let inner = &self.inner;
            let held = (|| {
                let conn = inner.command_conn(open)?;
                conn.batch(&inner.counters, LOCK_TIMEOUT)
                    .map_err(database)?;
                conn.query(
                    &inner.counters,
                    &inner.places.row,
                    &[Box::new(fence.hash.clone())],
                )
                .map_err(database)?;
                still(inner, conn, &fence)
            })();
            if let Err(error) = held {
                open.failed = Some(error.clone());
                return Err(error);
            }
        }
        Ok(command)
    }

    /// **The runs due at `now_ms`**, at most `limit`, earliest first; those
    /// failed for good aside. One statement.
    pub fn due_runs(&self, now_ms: i64, limit: usize) -> Result<Vec<Run>, Error> {
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        self.inner
            .run_read(&self.inner.places.due, &[Box::new(now_ms), Box::new(limit)])?
            .iter()
            .map(run_of)
            .collect()
    }

    /// When the next run not failed for good is due, where one is.
    pub fn next_run(&self) -> Result<Option<i64>, Error> {
        let rows = self.inner.run_read(&self.inner.places.next, &[])?;
        match rows.first() {
            Some(row) => row
                .try_get(0)
                .map_err(|e| Error::Database(format!("the next run: {e}"))),
            None => Ok(None),
        }
    }

    /// **Whether `expected` is still the run kept**: the same generation,
    /// unfailed, and due at `now_ms`.
    pub fn run_current(&self, expected: &Run, now_ms: i64) -> Result<bool, Error> {
        let rows = self
            .inner
            .run_read(&self.inner.places.one, &[Box::new(expected.id)])?;
        let found = rows.first().map(run_of).transpose()?;
        Ok(found.as_ref() == Some(expected)
            && expected.failed.is_none()
            && expected.due_ms <= now_ms)
    }

    /// **A run that failed, to run again at `due_ms`** after `attempts`, or
    /// kept failed for good where `failed` says why: true where `expected`
    /// was still the run kept, and is changed; false where it was not.
    pub fn retry_run(
        &self,
        expected: &Run,
        attempts: i64,
        due_ms: i64,
        failed: Option<&str>,
    ) -> Result<bool, Error> {
        let rows = self.inner.run_read(
            &self.inner.places.retry,
            &[
                Box::new(expected.id),
                Box::new(expected.command.clone()),
                Box::new(expected.args.clone()),
                Box::new(expected.user.clone()),
                Box::new(expected.due_ms),
                Box::new(expected.attempts),
                Box::new(attempts),
                Box::new(due_ms),
                Box::new(failed.map(str::to_string)),
                Box::new(expected.failed.clone()),
            ],
        )?;
        Ok(rows.len() == 1)
    }

    /// Every run kept, pending or failed for good, earliest first.
    pub fn runs(&self) -> Result<Vec<Run>, Error> {
        self.inner
            .run_read(&self.inner.places.every, &[])?
            .iter()
            .map(run_of)
            .collect()
    }

    /// A run removed, whatever it is now (a test's step: a run answered).
    pub fn forget_run(&self, id: i64) -> Result<(), Error> {
        self.inner
            .run_read(&self.inner.places.forget, &[Box::new(id)])
            .map(|_| ())
    }

    /// A run's attempts, its time and why it failed for good, set whatever
    /// it is now (a test's step: a run retried).
    pub fn set_run(
        &self,
        id: i64,
        attempts: i64,
        due_ms: i64,
        failed: Option<&str>,
    ) -> Result<(), Error> {
        self.inner
            .run_read(
                &self.inner.places.set,
                &[
                    Box::new(id),
                    Box::new(attempts),
                    Box::new(due_ms),
                    Box::new(failed.map(str::to_string)),
                ],
            )
            .map(|_| ())
    }

    /// **A session admitted for a request** (ADR-0369; ADR-0389, amended
    /// 2026-11-02, as the other PostgreSQL layers admit theirs): the one
    /// alive by both `lifetimes` whose id hashes to `hash`, read by one
    /// statement that takes no session lock, so that no admission waits for
    /// a command holding its session. A live session whose use was recorded
    /// within `touch_after` is answered from the read. Otherwise a write is
    /// due (its use recorded, or, past either lifetime, its row forgotten),
    /// and it is made only where nobody holds the session's lock, under it;
    /// where a command, an opening or a closing holds it, nothing is written
    /// and the read answers: a session past its lifetimes is no one all the
    /// same, forgotten by a later admission or the host's sweep.
    pub fn admit_session(
        &self,
        hash: &[u8],
        lifetimes: Lifetimes,
        touch_after: Duration,
    ) -> Result<Option<Session>, Error> {
        let inner = &self.inner;
        let params = || -> Vec<crate::values::Param> {
            vec![
                Box::new(hash.to_vec()),
                Box::new(lifetimes.absolute.as_secs_f64()),
                Box::new(lifetimes.idle.as_secs_f64()),
                Box::new(touch_after.as_secs_f64()),
            ]
        };
        let mut lease = inner.sessions.take()?;
        let read = lease
            .conn()?
            .query(&inner.counters, &inner.places.admission, &params())
            .map_err(database)?;
        let Some(row) = read.first() else {
            return Ok(None);
        };
        let flag = |at: usize| {
            row.try_get::<_, bool>(at)
                .map_err(|e| Error::Database(format!("a session as kept: {e}")))
        };
        let (live, stale) = (flag(5)?, flag(6)?);
        let answer = if live { Some(session_of(row)?) } else { None };
        if live && !stale {
            return Ok(answer);
        }
        let written = inner.written_if_free(&mut lease, hash, |inner, conn| {
            conn.query(&inner.counters, &inner.places.admit, &params())
                .map_err(database)?
                .first()
                .map(session_of)
                .transpose()
        })?;
        Ok(written.unwrap_or(answer))
    }

    /// **A session as it is now**, alive by both `lifetimes`: read, and
    /// nothing recorded, locked or forgotten.
    pub fn inspect_session(
        &self,
        hash: &[u8],
        lifetimes: Lifetimes,
    ) -> Result<Option<Session>, Error> {
        self.inner
            .session_read(
                &self.inner.places.live,
                &[
                    Box::new(hash.to_vec()),
                    Box::new(lifetimes.absolute.as_secs_f64()),
                    Box::new(lifetimes.idle.as_secs_f64()),
                ],
            )?
            .first()
            .map(session_of)
            .transpose()
    }

    /// **A session opened as `session`**, its lifetimes from now; one kept
    /// under the same hash replaced.
    pub fn open_session(&self, hash: &[u8], session: &Session) -> Result<(), Error> {
        let hash = hash.to_vec();
        self.inner.session_change(&hash, |inner, conn| {
            conn.query(
                &inner.counters,
                &inner.places.open,
                &[
                    Box::new(hash.clone()),
                    Box::new(session.user.clone()),
                    Box::new(session.handle.clone()),
                    Box::new(session.name.clone()),
                    Box::new(session.issuer.clone()),
                    Box::new(session.epoch.to_vec()),
                ],
            )
            .map(|_| ())
            .map_err(database)
        })
    }

    /// **A session closed**: no one from now on, on every host.
    pub fn close_session(&self, hash: &[u8]) -> Result<(), Error> {
        let hash = hash.to_vec();
        self.inner.session_change(&hash, |inner, conn| {
            conn.query(
                &inner.counters,
                &inner.places.close,
                &[Box::new(hash.clone())],
            )
            .map(|_| ())
            .map_err(database)
        })
    }

    /// **A sign-in begun**: its nonce and its PKCE verifier, kept by its
    /// state's hash; those begun more than ten minutes ago forgotten.
    pub fn begin_sign_in(&self, hash: &[u8], nonce: &str, verifier: &str) -> Result<(), Error> {
        self.inner
            .session_read(
                &self.inner.places.begin_sign_in,
                &[
                    Box::new(hash.to_vec()),
                    Box::new(nonce.to_string()),
                    Box::new(verifier.to_string()),
                ],
            )
            .map(|_| ())
    }

    /// **A sign-in taken, once**: its nonce, its verifier and how long ago
    /// it began, forgotten as they are taken; none where it was never begun
    /// or was taken already.
    pub fn take_sign_in(&self, hash: &[u8]) -> Result<Option<(String, String, Duration)>, Error> {
        let rows = self
            .inner
            .session_read(&self.inner.places.take_sign_in, &[Box::new(hash.to_vec())])?;
        let Some(row) = rows.first() else {
            return Ok(None);
        };
        let read = |e: postgres::Error| Error::Database(format!("a sign-in as kept: {e}"));
        let ms: i64 = row.try_get(2).map_err(read)?;
        Ok(Some((
            row.try_get(0).map_err(read)?,
            row.try_get(1).map_err(read)?,
            Duration::from_millis(u64::try_from(ms).unwrap_or(0)),
        )))
    }

    /// **The sessions past either of `lifetimes`, forgotten**, `limit` at
    /// most, those a command or a session's change holds aside (the next
    /// sweep's): how many. One statement, which waits for nothing.
    pub fn sweep_sessions(&self, lifetimes: Lifetimes, limit: usize) -> Result<u64, Error> {
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        self.inner
            .session_read(
                &self.inner.places.sweep_sessions,
                &[
                    Box::new(lifetimes.absolute.as_secs_f64()),
                    Box::new(lifetimes.idle.as_secs_f64()),
                    Box::new(limit),
                ],
            )
            .map(|rows| rows.len() as u64)
    }

    /// **The sign-ins begun more than `lifetime` ago, forgotten**, their
    /// nonces and verifiers with them, `limit` at most: how many.
    pub fn sweep_sign_ins(&self, lifetime: Duration, limit: usize) -> Result<u64, Error> {
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        self.inner
            .session_read(
                &self.inner.places.sweep_sign_ins,
                &[Box::new(lifetime.as_secs_f64()), Box::new(limit)],
            )
            .map(|rows| rows.len() as u64)
    }

    /// Explicit maintenance removal of outbox rows. The same statement
    /// advances the trim watermark, so a shared-log reader detects a gap.
    /// Normal host delivery retains its rows until the shared retention age.
    /// Returns how many rows were removed.
    pub fn forget_outbox(&self, ids: &[i64]) -> Result<u64, Error> {
        if ids.is_empty() {
            return Ok(0);
        }
        self.inner
            .run_read(&self.inner.places.forget_outbox, &[Box::new(ids.to_vec())])
            .map(|rows| rows.len() as u64)
    }

    /// Outbox rows older than `age`, removed in a batch of at most `limit`,
    /// with their highest ID recorded in the same statement's watermark.
    /// Returns how many rows were removed.
    pub fn trim_outbox(&self, age: Duration, limit: usize) -> Result<u64, Error> {
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        self.inner
            .run_read(
                &self.inner.places.trim_outbox,
                &[Box::new(age.as_secs_f64()), Box::new(limit)],
            )
            .map(|rows| rows.len() as u64)
    }
}
