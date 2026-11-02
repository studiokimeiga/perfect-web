//! **Any program's declared tables, on PostgreSQL** (ADR-0338 D3): the
//! reference adapter after memory, behind the memory adapter's interface
//! (`pw-table-memory`, ADR-0385 and ADR-0386), so that a host plugs either in
//! and a component cannot tell them apart.
//!
//! - **The schema comes from `tables.json`** ([`migrate`]): every table, its
//!   key, checks, indexes and references, made once under an advisory lock
//!   and recorded with its hash in `pw_schema`; an additive change made, one
//!   that loses or reinterprets data only where the program's `migrate`
//!   states it; a database whose recorded schema or catalog is not the
//!   build's refuses to open (drift).
//! - **Every primitive is one prepared statement** ([`sql`]): a read by its
//!   key, a prefix or an index, its set form (`= ANY`, or `unnest` for
//!   several fields: one statement whatever the number of keys), `all` of a
//!   bounded table, and each write, whose refusal (the row holding its key or
//!   a unique value, the bound) is decided in the same statement, leaving the
//!   transaction whole, as the memory adapter answers it: a value.
//! - **A command is one transaction** ([`Command`]): `SERIALIZABLE`, set on
//!   each; one command of this adapter at a time, as the memory adapter's
//!   and every PostgreSQL layer's (ADR-0246); its reads see its writes, no
//!   other read sees them until it commits; its references held at
//!   [`Command::prepare`]; and its commit takes the platform's records with
//!   it ([`Prepared::publish_with`]): the interaction's (ADR-0325) and the
//!   outbox's rows (ADR-0208), in commit order (ADR-0369). A commit the
//!   database refuses keeps nothing, and says whether it may be run again
//!   ([`Error::Serialization`]) or was answered at all
//!   ([`Error::OutcomeUnknown`]).
//! - **Answers are the memory adapter's**: the same rows, each exactly its
//!   table's, in the same order (tables.json's, a `String` by its bytes),
//!   refused alike; `tests/parity.rs` holds them equal.
//! - **A render reads one snapshot in a bounded few seats** ([`Snapshot`],
//!   ADR-0354): repeatable read, read only, opened at its first read.
//! - **The platform's other tables** ([`platform`], track `durable-tables`):
//!   the runs a command schedules, written in its transaction; the sessions
//!   and sign-ins a deployment keeps; and a command fenced on the session it
//!   acts for, to its end.

mod migrate;
pub mod platform;
mod pool;
mod shape;
mod sql;
mod values;

pub use platform::{Fence, Lifetimes, Run, Scheduling, Session};

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use postgres::error::{Severity, SqlState};
use pw_core::tables::{Primitive, Schema, TablesJson};
use serde_json::{Map, Value};

use crate::pool::{Conn, Counters, Lease, Pool};
use crate::shape::{Key, Shape, check_row, row_key, same_rows, typed_selections, typed_selector};
use crate::sql::{Names, ident};
use crate::values::{Param, bind, bind_array, bind_atom, decode};

/// **What the adapter answers where it does not answer a value.** The memory
/// adapter's errors, each meaning what it means there, and the database's
/// own.
#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    /// The manifest is not one this adapter serves.
    Schema(String),
    /// What a function was given is not what it takes.
    Input(String),
    /// A write refused by the row that holds its key or a unique value it
    /// would hold. Nothing was written.
    Conflict { table: String, existing: Value },
    /// The command's transaction has ended (committed or undone), or it is
    /// committing and writes nothing more.
    Ended,
    /// A lock of the adapter's own was poisoned by a panic.
    StoragePoisoned,
    /// **The database refused the transaction as it could not serialize it**
    /// (SQLSTATE 40001, or a deadlock, 40P01): nothing of it is kept, and the
    /// whole command may run again from its beginning (PostgreSQL 18, 13.2.3).
    Serialization(String),
    /// The database refused the commit for another reason: nothing is kept.
    Refused(String),
    /// **A commit sent and never answered**: whether it committed is not
    /// known here, and is reconciled from what the database holds (the
    /// interaction's record), never taken for a refusal.
    OutcomeUnknown(String),
    /// The database failed a statement (the command's transaction, if any,
    /// is aborted and keeps nothing).
    Database(String),
    /// **The database's schema is not the build's**: its recorded schema, or
    /// its catalog, differs from what was applied, and the host does not
    /// start on it.
    Drift(String),
    /// A change from the recorded schema that nothing states, or that the
    /// database refused while applying it.
    Migration(String),
    /// No connection or seat was free within the wait.
    Unavailable(String),
}

/// A database error, as one line: its SQLSTATE and message where the server
/// sent one.
pub(crate) fn pg(e: &postgres::Error) -> String {
    match e.as_db_error() {
        Some(db) => format!("PostgreSQL {}: {}", db.code().code(), db.message()),
        None => format!("PostgreSQL: {e}"),
    }
}

/// Whether the database refused a transaction it could not serialize.
fn serialization(e: &postgres::Error) -> bool {
    e.code().is_some_and(|code| {
        *code == SqlState::T_R_SERIALIZATION_FAILURE || *code == SqlState::T_R_DEADLOCK_DETECTED
    })
}

pub(crate) fn database(e: postgres::Error) -> Error {
    if serialization(&e) {
        Error::Serialization(pg(&e))
    } else {
        Error::Database(pg(&e))
    }
}

/// A statement of a commit's records that the database refused: the commit
/// refused (nothing of the command is kept, its transaction aborted), or one
/// it could not serialize.
fn refusal(e: postgres::Error) -> Error {
    match e.as_db_error() {
        Some(_) if serialization(&e) => Error::Serialization(pg(&e)),
        Some(_) => Error::Refused(pg(&e)),
        None => Error::Database(pg(&e)),
    }
}

/// **How a command's transaction is opened.** The adapter sets serializable
/// on each; a deployment that leaves it to the connection gets the
/// database's `default_transaction_isolation`, which [`PgTables::provides`]
/// measures (ADR-0246).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Isolation {
    Serializable,
    ConnectionDefault,
}

/// How a render's read transaction opens (ADR-0354, decision 3): repeatable
/// read, so every read of it reads its first statement's snapshot, and read
/// only, so it never meets a serialization failure.
pub const READ_SNAPSHOT: &str = "BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY";

/// How many times [`PgTables::transaction`] runs its closure, at most, when
/// the database refuses it as it could not serialize it.
pub const ATTEMPTS: u32 = 4;

/// **A deployment's settings**, each with the default the PostgreSQL layers
/// have (ADR-0298's eight connections; ADR-0354's eight render seats and 30 s).
#[derive(Debug, Clone)]
pub struct Options {
    pub isolation: Isolation,
    /// Connections for commands and single reads, at most.
    pub connections: usize,
    /// How long a single read or a command waits for a connection.
    pub wait: Duration,
    /// Renders reading at once, at most, each in a seat of its own.
    pub render_seats: usize,
    /// How long a render waits for its seat.
    pub seat_wait: Duration,
    /// This host's id, which the outbox's rows it commits carry.
    pub host: String,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            isolation: Isolation::Serializable,
            connections: 8,
            wait: Duration::from_secs(30),
            render_seats: 8,
            seat_wait: Duration::from_secs(30),
            host: String::new(),
        }
    }
}

/// **What the database opened gives** (ADR-0246, decision 3): a command's
/// transactions as isolated as measured, reads of the primary (`strong`),
/// and no feed of its changes but the outbox.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provides {
    /// `serializable`, `snapshot` or `read_committed`: ADR-0207's words.
    pub transactions: &'static str,
    pub reads: &'static str,
    pub changes: &'static str,
}

/// **A row of the outbox**, as a command hands it the platform (ADR-0208):
/// an event, or an invalidated entry (ADR-0209), by its declaration's path,
/// with the values the command computed: each a `String`, an `Int`, a `Bool`,
/// or an invalidated entry's `null` (ADR-0256).
#[derive(Debug, Clone, PartialEq)]
pub struct Outboxed {
    pub kind: OutboxKind,
    pub name: String,
    pub values: Vec<Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutboxKind {
    Event,
    Invalidation,
}

impl OutboxKind {
    fn word(self) -> &'static str {
        match self {
            OutboxKind::Event => "event",
            OutboxKind::Invalidation => "invalidation",
        }
    }
}

/// **An interaction's record** (ADR-0325): its key (the session, the command
/// and the interaction), the arguments it was first sent with, its answer,
/// at the host's instant; those recorded before `forget_before_ms` forgotten
/// in the same transaction.
#[derive(Debug, Clone, Copy)]
pub struct Interaction<'a> {
    pub key: &'a str,
    pub sent: &'a str,
    pub answer: &'a str,
    pub at_ms: u64,
    pub forget_before_ms: u64,
}

/// **The platform's records a commit takes with it** (ADR-0338 1.3).
#[derive(Debug, Clone, Copy, Default)]
pub struct Records<'a> {
    pub interaction: Option<Interaction<'a>>,
    pub outbox: &'a [Outboxed],
}

/// **What a commit kept**: each table it wrote, by name, at the version this
/// adapter's commits have made of it, and the outbox's rows, by their ids,
/// in commit order.
#[derive(Debug, Clone, PartialEq)]
pub struct Committed {
    pub written: BTreeMap<String, u64>,
    pub outbox: Vec<(i64, Outboxed)>,
}

/// The platform's statements, in the adapter's schema.
struct Platform {
    interaction: String,
    outbox: String,
    recall: String,
    outbox_lock: i64,
}

struct Inner {
    shape: Shape,
    /// The same manifest as the host reads it (`pw_host::tables`): its gate,
    /// and what the engine holds each function to.
    host: pw_host::tables::Manifest,
    names: Names,
    /// Each function's statement, by its import.
    statements: BTreeMap<String, String>,
    platform: Platform,
    options: Options,
    counters: Counters,
    pool: Arc<Pool>,
    seats: Arc<Pool>,
    /// The open command, and the next command's id.
    turn: Mutex<(Option<u64>, u64)>,
    turned: Condvar,
    told: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
    /// Each table's version: this adapter's commits that wrote it.
    versions: Mutex<Vec<u64>>,
    /// Each reference's foreign key, by its name: the referring table and
    /// the referred, which a refusal names.
    foreign: BTreeMap<String, (String, String)>,
    /// `SET CONSTRAINTS <each foreign key> IMMEDIATE`: the program's
    /// references held where a command prepares, and nothing else of the
    /// database's deferred to its commit.
    hold_references: Option<String>,
    /// The platform's statements for runs and sessions ([`platform`]).
    places: platform::Statements,
    /// The sessions' own connections ([`platform::SESSION_CONNECTIONS`]).
    sessions: Arc<Pool>,
}

/// **One isolated program's tables, on one PostgreSQL schema.** Every read
/// outside a command reads the latest commit; one command at a time writes
/// ([`PgTables::begin`]), and what it writes is published only when it
/// commits.
pub struct PgTables {
    inner: Arc<Inner>,
}

impl PgTables {
    /// **The adapter on `url`**, its tables in `schema` (made where missing)
    /// or in the connection's current schema, the build's schema applied or
    /// held to, a command's transaction serializable.
    pub fn open(url: &str, schema: Option<&str>, manifest: TablesJson) -> Result<PgTables, Error> {
        PgTables::open_with(url, schema, manifest, Options::default())
    }

    /// [`PgTables::open`], as a deployment's `options` say.
    pub fn open_with(
        url: &str,
        schema: Option<&str>,
        manifest: TablesJson,
        options: Options,
    ) -> Result<PgTables, Error> {
        let built = Schema::of(manifest.tables.iter().map(|t| t.schema.clone()).collect());
        if manifest.hash != built.hash || manifest.migration.to != built.hash {
            return Err(Error::Schema(
                "tables.json and its schema hash disagree".into(),
            ));
        }
        let sources: BTreeSet<_> = manifest.tables.iter().map(|t| &t.schema.source).collect();
        if sources.len() > 1 {
            return Err(Error::Schema(
                "multiple sources require separate adapters".into(),
            ));
        }
        if options.connections == 0 || options.render_seats == 0 {
            return Err(Error::Schema(
                "a pool of no connections serves nothing".into(),
            ));
        }
        let text = serde_json::to_string(&manifest)
            .map_err(|error| Error::Schema(format!("tables.json: {error}")))?;
        let host = pw_host::tables::Manifest::from_json(&text).map_err(Error::Schema)?;
        let shape = shape::shape_of(&manifest)?;
        let counters = Counters::default();
        let pool = Pool::new(url, options.connections, options.wait, "connections");
        let seats = Pool::new(url, options.render_seats, options.seat_wait, "render seats");
        let mut lease = pool.take()?;
        let conn = lease.conn()?;
        let schema = match schema {
            Some(schema) => schema.to_string(),
            None => conn
                .query(&counters, "SELECT current_schema()::text", &[])
                .map_err(database)?
                .first()
                .and_then(|r| r.get::<_, Option<String>>(0))
                .ok_or_else(|| Error::Schema("the connection has no current schema".into()))?,
        };
        if schema.is_empty() || schema.len() > shape::LONGEST_NAME {
            return Err(Error::Schema(format!(
                "`{schema}` is no PostgreSQL schema name"
            )));
        }
        let names = Names { schema };
        migrate::apply(conn, &counters, &names, &manifest, &shape)?;
        drop(lease);
        let s = ident(&names.schema);
        let platform = Platform {
            interaction: format!(
                "WITH forgotten AS (DELETE FROM {s}.pw_interactions WHERE at_ms < $1 AND key <> $2) \
                 INSERT INTO {s}.pw_interactions AS i (key, sent, answer, at_ms) VALUES ($2, $3, $4, $5) \
                 ON CONFLICT (key) DO UPDATE SET sent = EXCLUDED.sent, answer = EXCLUDED.answer, \
                 at_ms = EXCLUDED.at_ms WHERE i.at_ms < $1 RETURNING 1"
            ),
            outbox: format!(
                "WITH ordered AS (SELECT pg_advisory_xact_lock($1)) \
                 INSERT INTO {s}.pw_outbox (kind, name, args, host) \
                 SELECT o.kind, o.name, o.args::json, $5 \
                 FROM ordered, unnest($2::text[], $3::text[], $4::text[]) WITH ORDINALITY AS o(kind, name, args, place) \
                 ORDER BY o.place RETURNING id, kind, name, args::text"
            ),
            recall: format!(
                "SELECT sent, answer FROM {s}.pw_interactions WHERE key = $1 AND at_ms >= $2"
            ),
            outbox_lock: migrate::lock_key(&format!("pw_outbox:{}", names.schema)),
        };
        let mut foreign = BTreeMap::new();
        for table in &shape.tables {
            for reference in &table.references {
                let field = &table.columns[reference.field].name;
                foreign.insert(
                    sql::object_name(&table.name, &format!("{field}__fkey")),
                    (
                        table.name.clone(),
                        shape.tables[reference.target].name.clone(),
                    ),
                );
            }
        }
        let hold_references = (!foreign.is_empty()).then(|| {
            let named: Vec<String> = foreign.keys().map(|name| names.qualified(name)).collect();
            format!("SET CONSTRAINTS {} IMMEDIATE", named.join(", "))
        });
        let statements = sql::statements(&names, &shape);
        let versions = vec![0; shape.tables.len()];
        let places = platform::Statements::of(&names.schema);
        let sessions = Pool::new(
            url,
            platform::SESSION_CONNECTIONS,
            options.wait,
            "session connections",
        );
        Ok(PgTables {
            inner: Arc::new(Inner {
                shape,
                host,
                names,
                statements,
                platform,
                options,
                counters,
                pool,
                seats,
                turn: Mutex::new((None, 1)),
                turned: Condvar::new(),
                told: Mutex::new(None),
                versions: Mutex::new(versions),
                foreign,
                hold_references,
                places,
                sessions,
            }),
        })
    }

    /// [`PgTables::open_with`], given tables.json's text as a build wrote
    /// it: what a host reads, which links no compiler (track
    /// `durable-tables`).
    pub fn open_json(
        url: &str,
        schema: Option<&str>,
        tables_json: &str,
        options: Options,
    ) -> Result<PgTables, Error> {
        let manifest: TablesJson = serde_json::from_str(tables_json)
            .map_err(|error| Error::Schema(format!("tables.json: {error}")))?;
        PgTables::open_with(url, schema, manifest, options)
    }

    /// The schema its tables are in.
    pub fn schema(&self) -> &str {
        &self.inner.names.schema
    }

    /// The manifest as the host reads it: what a call that reads or writes
    /// these tables is given (`pw_host::engine::call_with_tables`).
    pub fn host_tables(&self) -> &pw_host::tables::Manifest {
        &self.inner.host
    }

    /// **Read through a compiler-emitted import**, outside any command: one
    /// statement, which reads the latest commit. A singleton selector takes
    /// positional values; a set selector one array of scalars or named-field
    /// records; `all` none.
    pub fn read(&self, import: &str, args: &[Value]) -> Result<Value, Error> {
        self.inner.read_published(import, args)
    }

    /// The read imports this admitted component may bind as a query: the
    /// host's gate (`pw_host::tables::Manifest::admit_reads`) on this
    /// adapter's manifest.
    pub fn read_host_imports(
        &self,
        contract: &pw_host::ComponentContract,
        granted: &pw_host::Granted,
    ) -> Result<Vec<String>, Error> {
        self.inner
            .host
            .admit_reads(contract, granted)
            .map(|reads| reads.into_keys().collect())
            .map_err(Error::Input)
    }

    /// **The table reads an admitted query links, as host functions**: one
    /// for each read the host's gate admits, each reading the latest commit
    /// through the import it is installed for, its keys and rows held by the
    /// engine's codecs (`pw_host::engine::table_read_host`).
    pub fn read_host_fns(
        &self,
        contract: &pw_host::ComponentContract,
        granted: &pw_host::Granted,
    ) -> Result<BTreeMap<String, pw_host::engine::HostFn>, Error> {
        use pw_host::engine::table_read_host::host_fn;
        let mut out = BTreeMap::new();
        let reads = self
            .inner
            .host
            .admit_reads(contract, granted)
            .map_err(Error::Input)?;
        for (import, read) in reads {
            let inner = Arc::clone(&self.inner);
            let bound = import.clone();
            let data = Arc::new(move |args: &[Value]| {
                inner
                    .read_published(&bound, args)
                    .map_err(|error| format!("`{bound}`: {error:?}"))
            });
            out.insert(import, host_fn(read, data));
        }
        Ok(out)
    }

    /// **A command's transaction**: begun once the command before it has
    /// ended, one at a time. Its connection and `BEGIN` wait for its first
    /// statement, so a command that touches no table costs the database
    /// nothing. Its writes are kept only when it commits, and undone whole
    /// where it is dropped instead.
    pub fn begin(&self) -> Result<Command, Error> {
        let inner = &self.inner;
        let mut turn = inner.turn.lock().map_err(|_| Error::StoragePoisoned)?;
        if turn.0.is_some() {
            let tell = inner
                .told
                .lock()
                .map_err(|_| Error::StoragePoisoned)?
                .clone();
            drop(turn);
            if let Some(tell) = tell {
                tell();
            }
            turn = inner.turn.lock().map_err(|_| Error::StoragePoisoned)?;
        }
        while turn.0.is_some() {
            turn = inner
                .turned
                .wait(turn)
                .map_err(|_| Error::StoragePoisoned)?;
        }
        let id = turn.1;
        turn.1 += 1;
        turn.0 = Some(id);
        drop(turn);
        Ok(Command {
            inner: Arc::clone(inner),
            state: Arc::new(Mutex::new(Open {
                id,
                lease: None,
                begun: false,
                prepared: false,
                ended: false,
                failed: None,
                touched: BTreeMap::new(),
                fence: None,
            })),
            ended: false,
        })
    }

    /// **Tell `tell` each time a command finds another open and waits its
    /// turn**, as it is about to wait (the memory adapter's, for tests).
    pub fn tell_on_wait(&self, tell: impl Fn() + Send + Sync + 'static) {
        let observer: Arc<dyn Fn() + Send + Sync> = Arc::new(tell);
        let old = match self.inner.told.lock() {
            Ok(mut slot) => slot.replace(observer),
            Err(_) => return,
        };
        drop(old);
    }

    /// **A serializable transaction**, committed where `action` answers, and
    /// run again from its beginning where the database refuses it as it
    /// could not serialize it (PostgreSQL's advice, 13.2.3), [`ATTEMPTS`]
    /// times at most. A refused write, a missing reference, or `action`'s own
    /// error keeps nothing and is answered as it is. (The host runs a
    /// compiled command once per interaction, and answers its refused commit
    /// refused: its client's retry is answered once by the interaction's
    /// record, ADR-0246 and ADR-0325.)
    pub fn transaction<T>(
        &self,
        mut action: impl FnMut(&mut Command) -> Result<T, Error>,
    ) -> Result<T, Error> {
        let mut attempt = 1;
        loop {
            let ran = self.begin().and_then(|mut command| {
                let answer = action(&mut command)?;
                command.commit()?;
                Ok(answer)
            });
            match ran {
                Err(Error::Serialization(_)) if attempt < ATTEMPTS => attempt += 1,
                other => return other,
            }
        }
    }

    /// **A render's snapshot** (ADR-0354): a seat among the deployment's
    /// few, waited for while every one is taken, and refused past the wait;
    /// every read of it in one read-only repeatable-read transaction, opened
    /// at its first read and ended when it is dropped.
    pub fn snapshot(&self) -> Result<Snapshot, Error> {
        let lease = self.inner.seats.take()?;
        Ok(Snapshot {
            state: Arc::new(SnapshotState {
                inner: Arc::clone(&self.inner),
                seat: Mutex::new((lease, false)),
            }),
        })
    }

    /// **What the database gives a command** (ADR-0246): a transaction
    /// opened as a command's is, asked its isolation and whether it may
    /// write, and rolled back. A database that cannot write is refused.
    pub fn provides(&self) -> Result<Provides, Error> {
        let inner = &self.inner;
        let mut lease = inner.pool.take()?;
        let conn = lease.conn()?;
        let measured = (|| {
            inner.begin_command(conn)?;
            let rows = conn
                .query(
                    &inner.counters,
                    "SELECT current_setting('transaction_isolation'), current_setting('transaction_read_only')",
                    &[],
                )
                .map_err(database)?;
            let row = rows
                .first()
                .ok_or_else(|| Error::Database("no isolation measured".into()))?;
            Ok::<(String, String), Error>((row.get(0), row.get(1)))
        })();
        if conn.batch(&inner.counters, "ROLLBACK").is_err() {
            lease.discard();
        }
        let (isolation, read_only) = measured?;
        if read_only == "on" {
            return Err(Error::Refused(
                "the database is read only: no command's writes could commit".into(),
            ));
        }
        Ok(Provides {
            transactions: match isolation.as_str() {
                "serializable" => "serializable",
                "repeatable read" => "snapshot",
                _ => "read_committed",
            },
            reads: "strong",
            changes: "none",
        })
    }

    /// **An interaction's committed record** (ADR-0325), recorded at
    /// `since_ms` or later: the arguments it was sent with, and its answer.
    pub fn recalled(&self, key: &str, since_ms: u64) -> Result<Option<(String, String)>, Error> {
        let inner = &self.inner;
        let mut lease = inner.pool.take()?;
        let since = i64::try_from(since_ms).unwrap_or(i64::MAX);
        let rows = lease
            .conn()?
            .query(
                &inner.counters,
                &inner.platform.recall,
                &[Box::new(key.to_string()), Box::new(since)],
            )
            .map_err(database)?;
        Ok(rows.first().map(|r| (r.get(0), r.get(1))))
    }

    /// Every statement this adapter has run on its database, so far.
    pub fn statements(&self) -> u64 {
        self.inner.counters.statements.load(Ordering::Relaxed)
    }

    /// Every statement this adapter has prepared, so far: once per
    /// connection and statement, never per call.
    pub fn prepared(&self) -> u64 {
        self.inner.counters.prepared.load(Ordering::Relaxed)
    }

    /// The render seats: (taken now, the most taken at once, renders
    /// waiting for one now).
    pub fn seats(&self) -> (usize, usize, usize) {
        self.inner.seats.census()
    }
}

/// A row, its fields by name.
type Row = Map<String, Value>;

/// What a write binds, and the key and row it writes where it writes one.
type Bound = (Vec<Param>, Option<(Key, Row)>);

/// The open command's state, shared with its host functions.
struct Open {
    id: u64,
    lease: Option<Lease>,
    /// `BEGIN` sent.
    begun: bool,
    /// Held to its commit's checks, writing nothing more.
    prepared: bool,
    /// Committed or undone: its functions act on nothing.
    ended: bool,
    /// The statement that aborted its transaction: every later one fails so.
    failed: Option<Error>,
    /// Each key it wrote, by its table: the row it held before the command
    /// first wrote it, and the row it holds now.
    touched: BTreeMap<(usize, Key), (Option<Row>, Option<Row>)>,
    /// The session it acts for, held to its end ([`Command::fence`]).
    fence: Option<platform::Fenced>,
}

impl Inner {
    fn begin_command(&self, conn: &mut Conn) -> Result<(), Error> {
        let sql = match self.options.isolation {
            Isolation::Serializable => "BEGIN ISOLATION LEVEL SERIALIZABLE",
            Isolation::ConnectionDefault => "BEGIN",
        };
        conn.batch(&self.counters, sql).map_err(database)
    }

    /// One statement of its own, on a connection of the pool's (given back
    /// as it ends: a closed one is never kept), which reads the latest commit.
    fn read_published(&self, import: &str, args: &[Value]) -> Result<Value, Error> {
        let mut lease = self.pool.take()?;
        read(self, lease.conn()?, import, args)
    }

    /// The command's connection, in its transaction: taken and begun at its
    /// first statement.
    fn command_conn<'a>(&self, open: &'a mut Open) -> Result<&'a mut Conn, Error> {
        if open.ended {
            return Err(Error::Ended);
        }
        if let Some(failed) = &open.failed {
            return Err(failed.clone());
        }
        if open.lease.is_none() {
            open.lease = Some(self.pool.take()?);
        }
        let lease = open.lease.as_mut().ok_or(Error::StoragePoisoned)?;
        let conn = lease.conn()?;
        if !open.begun {
            if let Err(error) = self.begin_command(conn) {
                open.failed = Some(error.clone());
                return Err(error);
            }
            open.begun = true;
        }
        Ok(conn)
    }

    fn command_read(
        &self,
        state: &Mutex<Open>,
        import: &str,
        args: &[Value],
    ) -> Result<Value, Error> {
        let mut open = state.lock().map_err(|_| Error::StoragePoisoned)?;
        let open = &mut *open;
        let conn = self.command_conn(open)?;
        let answer = read(self, conn, import, args);
        if let Err(Error::Database(_) | Error::Serialization(_)) = &answer {
            open.failed = answer.clone().err();
        }
        answer
    }

    fn command_write(
        &self,
        state: &Mutex<Open>,
        import: &str,
        args: &[Value],
    ) -> Result<(), Error> {
        let mut open = state.lock().map_err(|_| Error::StoragePoisoned)?;
        let open = &mut *open;
        if open.ended || open.prepared {
            return Err(Error::Ended);
        }
        let function = self.shape.function(import)?;
        if !function.primitive.writes() {
            return Err(Error::Input(format!("{import} is not a table write")));
        }
        let table = &self.shape.tables[function.table];
        let sql = self
            .statements
            .get(import)
            .ok_or_else(|| Error::Input(format!("undeclared table operation {import}")))?;
        // Everything a write is given is held before the database sees it.
        let (params, written): Bound = match function.primitive {
            Primitive::Insert | Primitive::Put => {
                let [value] = args else {
                    return Err(Error::Input("insert/put expects one row".into()));
                };
                let (key, row) = check_row(table, value)?;
                let params = table
                    .columns
                    .iter()
                    .map(|c| {
                        let value = row.get(&c.name).ok_or_else(|| {
                            Error::Input(format!("{} row is missing {}", table.name, c.name))
                        })?;
                        bind(c, value)
                    })
                    .collect::<Result<_, _>>()?;
                (params, Some((key, row)))
            }
            Primitive::Delete | Primitive::DeletePrefix => {
                let key = typed_selector(table, function, import, args)?;
                let params = function
                    .by
                    .iter()
                    .zip(&key)
                    .map(|(&f, atom)| bind_atom(&table.columns[f], atom))
                    .collect::<Result<_, _>>()?;
                (params, None)
            }
            _ => return Err(Error::Input(format!("{import} is not a table write"))),
        };
        let conn = self.command_conn(open)?;
        let rows = match conn.query(&self.counters, sql, &params) {
            Ok(rows) => rows,
            Err(e) => {
                let error = database(e);
                open.failed = Some(error.clone());
                return Err(error);
            }
        };
        let n = table.columns.len();
        let at = function.table;
        let first = |open: &mut Open,
                     key: Key,
                     before: Option<Map<String, Value>>,
                     now: Option<Map<String, Value>>| {
            open.touched
                .entry((at, key))
                .and_modify(|(_, current)| *current = now.clone())
                .or_insert((before, now));
        };
        let conflict = |row: &postgres::Row| -> Result<Error, Error> {
            Ok(Error::Conflict {
                table: table.name.clone(),
                existing: Value::Object(decode(table, row, 2)?),
            })
        };
        let bound = || {
            Error::Input(format!(
                "{} cannot grow past its declared bound of {}",
                table.name,
                table.bounded.unwrap_or(0)
            ))
        };
        // What the statement answered, read. A refusal (the row holding the
        // key or a unique value, or the bound) is a value: nothing was
        // written, and the transaction stays whole. An answer that cannot
        // be read (a row another writer left out of its type) may follow a
        // write the statement made, which `touched` would then not hold:
        // the command fails, every later statement with it, and its commit
        // is refused (the review of 2026-11-02).
        let read_answer = |open: &mut Open| -> Result<Result<(), Error>, Error> {
            match function.primitive {
                Primitive::Insert => {
                    let row = rows.first().ok_or(Error::StoragePoisoned)?;
                    let count: i64 = row.get(0);
                    let holder: Option<i32> = row.get(1);
                    let (key, written) = written.ok_or(Error::StoragePoisoned)?;
                    if count == 1 {
                        first(open, key, None, Some(written));
                        Ok(Ok(()))
                    } else if holder.is_some() {
                        Ok(Err(conflict(row)?))
                    } else {
                        Ok(Err(bound()))
                    }
                }
                Primitive::Put => {
                    let row = rows.first().ok_or(Error::StoragePoisoned)?;
                    let count: i64 = row.get(0);
                    let holder: Option<i32> = row.get(1);
                    let old_held: bool = row.get(2 + n);
                    let (key, written) = written.ok_or(Error::StoragePoisoned)?;
                    if holder.is_some() {
                        Ok(Err(conflict(row)?))
                    } else if count == 1 || old_held {
                        let before = if old_held {
                            Some(decode(table, row, 3 + n)?)
                        } else {
                            None
                        };
                        first(open, key, before, Some(written));
                        Ok(Ok(()))
                    } else {
                        Ok(Err(bound()))
                    }
                }
                _ => {
                    for row in &rows {
                        let before = decode(table, row, 0)?;
                        let key = row_key(&before, &table.columns, &table.key)?;
                        first(open, key, Some(before), None);
                    }
                    Ok(Ok(()))
                }
            }
        };
        match read_answer(open) {
            Ok(answer) => answer,
            Err(unread) => {
                open.failed = Some(unread.clone());
                Err(unread)
            }
        }
    }

    /// The tables a command has changed: each key it touched whose row is
    /// not the one it held before.
    fn changed(&self, open: &Open) -> BTreeSet<usize> {
        open.touched
            .iter()
            .filter(|(_, (before, now))| !same_rows(before, now))
            .map(|((table, _), _)| *table)
            .collect()
    }

    /// **The command ends**: its connection given back (closed where its
    /// transaction could not be ended), its turn given to the next.
    fn end(&self, open: &mut Open, rollback: bool) {
        if open.ended {
            return;
        }
        open.ended = true;
        if let Some(mut lease) = open.lease.take() {
            if rollback && open.begun && lease.connected() {
                let ended = lease
                    .conn()
                    .and_then(|conn| conn.batch(&self.counters, "ROLLBACK").map_err(database));
                if ended.is_err() {
                    lease.discard();
                }
            }
            // The session's lock a fenced command held, given back on its
            // connection once its transaction has ended (a connection
            // closed above gave it back as it closed).
            if let Some(fenced) = &open.fence
                && fenced.locked
                && lease.connected()
            {
                self.unfence(&mut lease, fenced.key);
            }
        }
        let mut turn = match self.turn.lock() {
            Ok(turn) => turn,
            Err(poisoned) => poisoned.into_inner(),
        };
        if turn.0 == Some(open.id) {
            turn.0 = None;
        }
        drop(turn);
        self.turned.notify_one();
    }
}

/// **A read through a compiler-emitted import**, on `conn`, in whatever
/// transaction it is in: one statement (none for a set read given no keys).
fn read(inner: &Inner, conn: &mut Conn, import: &str, args: &[Value]) -> Result<Value, Error> {
    let function = inner.shape.function(import)?;
    if function.primitive.writes() {
        return Err(Error::Input(format!("{import} is a write, not a read")));
    }
    let table = &inner.shape.tables[function.table];
    let sql = inner
        .statements
        .get(import)
        .ok_or_else(|| Error::Input(format!("undeclared table operation {import}")))?;
    let params: Vec<Param> = match function.primitive {
        Primitive::All => {
            if !args.is_empty() {
                return Err(Error::Input("all() takes no selector".into()));
            }
            if table.bounded.is_none() {
                return Err(Error::Schema("whole-table read without a bound".into()));
            }
            Vec::new()
        }
        Primitive::GetMany | Primitive::PrefixMany | Primitive::IndexMany => {
            let keys = typed_selections(table, function, import, args)?;
            if keys.is_empty() {
                return Ok(Value::Array(Vec::new()));
            }
            function
                .by
                .iter()
                .enumerate()
                .map(|(i, &f)| {
                    let atoms: Vec<&shape::Atom> = keys.iter().map(|k| &k[i]).collect();
                    bind_array(&table.columns[f], &atoms)
                })
                .collect::<Result<_, _>>()?
        }
        Primitive::Get | Primitive::Prefix | Primitive::Index => {
            let key = typed_selector(table, function, import, args)?;
            function
                .by
                .iter()
                .zip(&key)
                .map(|(&f, atom)| bind_atom(&table.columns[f], atom))
                .collect::<Result<_, _>>()?
        }
        _ => return Err(Error::Input(format!("{import} is a write, not a read"))),
    };
    let rows = conn
        .query(&inner.counters, sql, &params)
        .map_err(database)?;
    if function.primitive == Primitive::All
        && let Some(bound) = table.bounded
        && rows.len() as u64 > bound
    {
        return Err(Error::Input(format!(
            "{} holds more than its bound of {bound} rows",
            table.name
        )));
    }
    if function.one {
        return Ok(match rows.first() {
            Some(row) => Value::Object(decode(table, row, 0)?),
            None => Value::Null,
        });
    }
    Ok(Value::Array(
        rows.iter()
            .map(|row| decode(table, row, 0).map(Value::Object))
            .collect::<Result<_, _>>()?,
    ))
}

/// **One command's transaction** (ADR-0386's, on PostgreSQL): its reads see
/// its own writes, and no other read sees them until it commits. Dropped
/// without committing, it is rolled back whole. Its host functions
/// ([`Command::host_fns`]) act on it alone, and on nothing once it has ended.
pub struct Command {
    inner: Arc<Inner>,
    state: Arc<Mutex<Open>>,
    ended: bool,
}

impl Command {
    /// Read through a compiler-emitted import, as [`PgTables::read`] does,
    /// in this command's transaction: its own writes seen.
    pub fn read(&self, import: &str, args: &[Value]) -> Result<Value, Error> {
        self.inner.command_read(&self.state, import, args)
    }

    /// **Perform insert, put, delete or delete-prefix through a generated
    /// import**, in this command's transaction. A write refused by the row
    /// holding its key or a unique value (`Error::Conflict`, carrying that
    /// row), or by its table's bound, changes nothing and leaves the
    /// transaction whole.
    pub fn write(&mut self, import: &str, args: &[Value]) -> Result<(), Error> {
        self.inner.command_write(&self.state, import, args)
    }

    /// **The table reads and writes an admitted command links, as host
    /// functions**: one for each function the host's gate admits into a
    /// command (`pw_host::tables::Manifest::admit_command`), each acting on
    /// this command; a refused write answers the row that refused it
    /// (`result<_, row>`'s error). Once the command has ended, each fails.
    pub fn host_fns(
        &self,
        contract: &pw_host::ComponentContract,
        granted: &pw_host::Granted,
    ) -> Result<BTreeMap<String, pw_host::engine::HostFn>, Error> {
        use pw_host::engine::{table_read_host, table_write_host};
        use pw_host::tables::Access;
        if !matches!(contract.exports.as_slice(), [export] if export.kind == "command") {
            return Err(Error::Input(
                "staged table callbacks require one explicit command export".into(),
            ));
        }
        let mut out = BTreeMap::new();
        let admitted = self
            .inner
            .host
            .admit_command(contract, granted)
            .map_err(Error::Input)?;
        for (import, access) in admitted {
            let inner = Arc::clone(&self.inner);
            let state = Arc::clone(&self.state);
            let bound = import.clone();
            let host = match access {
                Access::Read(read) => table_read_host::host_fn(
                    read,
                    Arc::new(move |args: &[Value]| {
                        inner
                            .command_read(&state, &bound, args)
                            .map_err(|error| format!("`{bound}`: {error:?}"))
                    }),
                ),
                Access::Write(write) => table_write_host::host_fn(
                    write,
                    Arc::new(move |args: &[Value]| {
                        match inner.command_write(&state, &bound, args) {
                            Ok(()) => Ok(None),
                            Err(Error::Conflict { existing, .. }) => Ok(Some(existing)),
                            Err(error) => Err(format!("`{bound}`: {error:?}")),
                        }
                    }),
                ),
            };
            out.insert(import, host);
        }
        Ok(out)
    }

    /// **The tables this command has written so far**, by name: what its
    /// commit publishes, and so what a host commits its outbox with; none
    /// where it wrote nothing, a delete of a key no row has, or a row put
    /// back as it was, among them.
    pub fn written(&self) -> Result<BTreeSet<String>, Error> {
        let open = self.state.lock().map_err(|_| Error::StoragePoisoned)?;
        if open.ended {
            return Err(Error::Ended);
        }
        Ok(self
            .inner
            .changed(&open)
            .into_iter()
            .map(|t| self.inner.shape.tables[t].name.clone())
            .collect())
    }

    /// **Hold this command's writes to what a commit keeps**: each reference
    /// written to a row that exists, and no row referring to one it deleted
    /// (the program's deferred foreign keys checked now, `SET CONSTRAINTS ..
    /// IMMEDIATE`, one statement). Refused, the command is rolled back whole;
    /// held, it writes nothing more and is published by [`Prepared::publish`].
    /// Its bound and its unique indexes need no check here: no write ever
    /// breaks one.
    pub fn prepare(mut self) -> Result<Prepared, Error> {
        let inner = Arc::clone(&self.inner);
        let written = {
            let mut open = self.state.lock().map_err(|_| Error::StoragePoisoned)?;
            let open = &mut *open;
            if open.ended {
                return Err(Error::Ended);
            }
            if let Some(failed) = &open.failed {
                return Err(failed.clone());
            }
            let changed = inner.changed(open);
            if !changed.is_empty()
                && let Some(hold) = &inner.hold_references
            {
                let conn = inner.command_conn(open)?;
                if let Err(e) = conn.batch(&inner.counters, hold) {
                    let error = match e.as_db_error() {
                        Some(db) if *db.code() == SqlState::FOREIGN_KEY_VIOLATION => {
                            match db.constraint().and_then(|c| inner.foreign.get(c)) {
                                Some((referring, referred)) => Error::Input(format!(
                                    "{referring} references a missing {referred}"
                                )),
                                None => database(e),
                            }
                        }
                        _ => database(e),
                    };
                    open.failed = Some(error.clone());
                    return Err(error);
                }
            }
            open.prepared = true;
            let versions = inner.versions.lock().map_err(|_| Error::StoragePoisoned)?;
            changed
                .iter()
                .map(|&t| (inner.shape.tables[t].name.clone(), versions[t] + 1))
                .collect::<BTreeMap<_, _>>()
        };
        let command = Command {
            inner,
            state: Arc::clone(&self.state),
            ended: false,
        };
        // This handle hands its transaction to the prepared one.
        self.ended = true;
        Ok(Prepared { command, written })
    }

    /// [`Command::prepare`], then [`Prepared::publish`].
    pub fn commit(self) -> Result<Committed, Error> {
        self.prepare()?.publish()
    }
}

impl Drop for Command {
    fn drop(&mut self) {
        if !self.ended {
            let mut open = match self.state.lock() {
                Ok(open) => open,
                Err(poisoned) => poisoned.into_inner(),
            };
            self.inner.end(&mut open, true);
        }
    }
}

/// **A command held to its commit's checks**: published by
/// [`Prepared::publish`] or [`Prepared::publish_with`], or rolled back whole
/// where it is dropped instead.
pub struct Prepared {
    command: Command,
    written: BTreeMap<String, u64>,
}

impl Prepared {
    /// Each table its publish writes, by name, and the version it makes.
    pub fn written(&self) -> &BTreeMap<String, u64> {
        &self.written
    }

    /// **Commit what the command wrote**: every read after it sees all of it,
    /// and none before saw any of it.
    pub fn publish(self) -> Result<Committed, Error> {
        self.publish_with(Records::default())
    }

    /// **Commit what the command wrote, and the platform's records with it,
    /// in its transaction** (ADR-0338 1.3): the interaction's record, and the
    /// outbox's rows, which take their ids under the outbox's lock, held to
    /// the commit, so ids are in commit order (ADR-0369). All of it commits,
    /// or none of it: a record of the interaction already kept refuses the
    /// commit, as does the database.
    pub fn publish_with(mut self, records: Records<'_>) -> Result<Committed, Error> {
        let inner = Arc::clone(&self.command.inner);
        let state = Arc::clone(&self.command.state);
        let mut open = state.lock().map_err(|_| Error::StoragePoisoned)?;
        let open = &mut *open;
        if open.ended {
            return Err(Error::Ended);
        }
        let fence = open.fence.as_ref().map(|f| f.fence.clone());
        let mut handed: Vec<(String, String, String)> = Vec::new();
        for row in records.outbox {
            let values = row
                .values
                .iter()
                .map(|v| match v {
                    Value::String(_) | Value::Bool(_) | Value::Null => Ok(v.clone()),
                    Value::Number(n) if n.as_i64().is_some() => Ok(v.clone()),
                    other => Err(Error::Input(format!(
                        "`{}` carries {other}, which keys no entry",
                        row.name
                    ))),
                })
                .collect::<Result<Vec<_>, _>>()?;
            handed.push((
                row.kind.word().to_string(),
                row.name.clone(),
                Value::Array(values).to_string(),
            ));
        }
        let mut outbox = Vec::new();
        if records.interaction.is_some() || !handed.is_empty() || open.begun {
            let conn = inner.command_conn(open)?;
            let ran = (|| {
                if let Some(r) = records.interaction {
                    let before = i64::try_from(r.forget_before_ms).unwrap_or(i64::MAX);
                    let at = i64::try_from(r.at_ms).unwrap_or(i64::MAX);
                    let recorded = conn
                        .query(
                            &inner.counters,
                            &inner.platform.interaction,
                            &[
                                Box::new(before),
                                Box::new(r.key.to_string()),
                                Box::new(r.sent.to_string()),
                                Box::new(r.answer.to_string()),
                                Box::new(at),
                            ],
                        )
                        .map_err(refusal)?;
                    if recorded.is_empty() {
                        return Err(Error::Refused(format!(
                            "interaction {} is recorded already",
                            r.key
                        )));
                    }
                }
                if !handed.is_empty() {
                    let kinds: Vec<String> = handed.iter().map(|h| h.0.clone()).collect();
                    let names: Vec<String> = handed.iter().map(|h| h.1.clone()).collect();
                    let args: Vec<String> = handed.iter().map(|h| h.2.clone()).collect();
                    let rows = conn
                        .query(
                            &inner.counters,
                            &inner.platform.outbox,
                            &[
                                Box::new(inner.platform.outbox_lock),
                                Box::new(kinds),
                                Box::new(names),
                                Box::new(args),
                                Box::new(inner.options.host.clone()),
                            ],
                        )
                        .map_err(refusal)?;
                    // Every row the commit carries, or no commit: a trigger
                    // of another hand answering `NULL` drops a row with no
                    // error (the review of 2026-11-02).
                    if rows.len() != handed.len() {
                        return Err(Error::Refused(format!(
                            "the outbox kept {} of the {} rows the commit carries",
                            rows.len(),
                            handed.len()
                        )));
                    }
                    for row in &rows {
                        let id: i64 = row.get(0);
                        let kind: String = row.get(1);
                        let name: String = row.get(2);
                        let args: String = row.get(3);
                        let values: Vec<Value> = serde_json::from_str(&args)
                            .map_err(|e| Error::Database(format!("the outbox's `{name}`: {e}")))?;
                        outbox.push((
                            id,
                            Outboxed {
                                kind: if kind == "event" {
                                    OutboxKind::Event
                                } else {
                                    OutboxKind::Invalidation
                                },
                                name,
                                values,
                            },
                        ));
                    }
                    outbox.sort_by_key(|(id, _)| *id);
                }
                Ok(())
            })();
            if let Err(error) = ran {
                open.failed = Some(error.clone());
                return Err(error);
            }
            // A fenced command's session, still as it was admitted, just
            // before its commit (ADR-0369's last check, track
            // `durable-tables`): signed out meanwhile, nothing is kept.
            if let Err(error) = platform::still_fenced(&inner, conn, fence.as_ref()) {
                open.failed = Some(error.clone());
                return Err(error);
            }
            // The writes and the records, at once or not at all. A commit
            // the database refuses has rolled back; one sent and never
            // answered may have committed.
            if let Err(e) = conn.batch(&inner.counters, "COMMIT") {
                let error = match e.as_db_error() {
                    // An ERROR the server answered the COMMIT with: the
                    // transaction rolled back.
                    Some(db) if db.parsed_severity() == Some(Severity::Error) => {
                        if serialization(&e) {
                            Error::Serialization(pg(&e))
                        } else {
                            Error::Refused(pg(&e))
                        }
                    }
                    // No answer, or the session ended (FATAL, PANIC): it
                    // may have committed before it ended.
                    _ => Error::OutcomeUnknown(pg(&e)),
                };
                if let Some(lease) = open.lease.as_mut() {
                    lease.discard();
                }
                open.failed = Some(error.clone());
                inner.end(open, false);
                self.command.ended = true;
                return Err(error);
            }
        }
        {
            let mut versions = inner.versions.lock().map_err(|_| Error::StoragePoisoned)?;
            for t in inner.changed(open) {
                versions[t] += 1;
            }
        }
        inner.end(open, false);
        self.command.ended = true;
        Ok(Committed {
            written: std::mem::take(&mut self.written),
            outbox,
        })
    }
}

/// The seat and whether its read transaction is open.
struct SnapshotState {
    inner: Arc<Inner>,
    seat: Mutex<(Lease, bool)>,
}

impl SnapshotState {
    fn read(&self, import: &str, args: &[Value]) -> Result<Value, Error> {
        let mut seat = self.seat.lock().map_err(|_| Error::StoragePoisoned)?;
        let (lease, open) = &mut *seat;
        let conn = lease.conn()?;
        if !*open {
            conn.batch(&self.inner.counters, READ_SNAPSHOT)
                .map_err(database)?;
            *open = true;
        }
        let read = read(&self.inner, conn, import, args);
        if read.is_err() {
            // A read that fails ends the transaction, as it may have aborted
            // it; the render's next read opens another.
            if conn.batch(&self.inner.counters, "ROLLBACK").is_err() {
                lease.discard();
            }
            *open = false;
        }
        read
    }
}

impl Drop for SnapshotState {
    fn drop(&mut self) {
        let mut seat = match self.seat.lock() {
            Ok(seat) => seat,
            Err(poisoned) => poisoned.into_inner(),
        };
        let (lease, open) = &mut *seat;
        if *open {
            let ended = lease
                .conn()
                .and_then(|conn| conn.batch(&self.inner.counters, "COMMIT").map_err(database));
            if ended.is_err() {
                lease.discard();
            }
        }
    }
}

/// **A render's snapshot** (ADR-0354): one seat, one read-only
/// repeatable-read transaction, whatever commits meanwhile. Given back when
/// dropped (its host functions keep it while they live).
pub struct Snapshot {
    state: Arc<SnapshotState>,
}

impl Snapshot {
    /// Read through a compiler-emitted import, in this snapshot.
    pub fn read(&self, import: &str, args: &[Value]) -> Result<Value, Error> {
        self.state.read(import, args)
    }

    /// The table reads an admitted query links, as host functions, every
    /// one reading this snapshot.
    pub fn read_host_fns(
        &self,
        contract: &pw_host::ComponentContract,
        granted: &pw_host::Granted,
    ) -> Result<BTreeMap<String, pw_host::engine::HostFn>, Error> {
        use pw_host::engine::table_read_host::host_fn;
        let reads = self
            .state
            .inner
            .host
            .admit_reads(contract, granted)
            .map_err(Error::Input)?;
        let mut out = BTreeMap::new();
        for (import, read) in reads {
            let state = Arc::clone(&self.state);
            let bound = import.clone();
            let data = Arc::new(move |args: &[Value]| {
                state
                    .read(&bound, args)
                    .map_err(|error| format!("`{bound}`: {error:?}"))
            });
            out.insert(import, host_fn(read, data));
        }
        Ok(out)
    }
}
