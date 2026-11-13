//! **A program's declared tables, served durably from PostgreSQL** (track
//! `durable-tables`; ADR-0338's D3 joined to D2's routing): the same gate as
//! the memory layer (`tables.rs`), the generic PostgreSQL adapter behind it
//! (`pw-table-postgres`, ADR-0393), chosen where the deployment names a
//! database for a program's tables (`PW_TABLES_DATABASE_URL`). No name of
//! any program's tables, contracts or routes is here: it serves whatever a
//! build's `tables.json` declares.
//!
//! - **The gate is the only way**, as in memory: no operation by name; a
//!   query links the reads the gate admits for its contract and grant, of
//!   the latest commit or of its render's snapshot; a command links its
//!   reads and writes, staged in its own serializable transaction.
//! - **A command is one PostgreSQL transaction, with every record of the
//!   platform's** (ADR-0208, ADR-0325, ADR-0334, ADR-0338 1.3): its rows,
//!   the runs it scheduled or the run it completes, its interaction's
//!   record and its outbox's rows, committed by the adapter at once
//!   (`Prepared::publish_with`) or not at all. The host's materializer
//!   keeps nothing of it ([`Commit::Applied`], [`DataLayer::keeps_interactions`],
//!   [`DataLayer::runs`]): a split between the materializer's memory and the
//!   database would keep a record of a write a restart lost, or lose the
//!   record of a write a restart kept, and run a retry again (the review of
//!   track `data-path`, 2026-11-02).
//! - **One render reads one state** (NEXT 4c, ADR-0354): its snapshot is a
//!   seat of the adapter's, a read-only repeatable-read transaction every
//!   query of the render reads in; one that could not be taken fails the
//!   render's reads, never reads the latest commit instead.
//! - **The reader's session is the database's** (ADR-0369): kept by its
//!   id's hash where a restarted host, or a second one, finds it; and each
//!   command fenced on it in its own transaction, so that a session signed
//!   out before the command commits keeps nothing of it. The session's lock
//!   is waited for before the host's turn is taken, so a command waiting for
//!   its session holds no other session's command up (the review of
//!   3ccd083a, 2026-11-02).
//! - **What the host is answered is what the database did**: a commit the
//!   database rolled back, or that never reached its `COMMIT`, is refused at
//!   once ([`Staged::definitely_refused`]); one whose `COMMIT` was sent and
//!   never answered, or whose interaction another commit recorded, is
//!   reconciled by the host from the record.
//! - **What the database keeps of the platform's stays bounded**: each
//!   commit's outbox rows are forgotten once delivered (no host tails this
//!   layer's outbox yet), and the scheduler's passes sweep the sessions and
//!   sign-ins past their lifetimes and the outbox rows a host that ended
//!   left behind ([`DataLayer::housekeeping`]).

use super::*;
use crate::data::{Commit, DataLayer, Ops, Provided, Snapshot, Staged};
use crate::identity::{CommandActor, ReaderIdentity};
use pw_table_postgres::{Command, Lifetimes, PgTables, Records};

/// **The most rows one sweep of the layer's housekeeping forgets**, of each
/// kind: one statement's batch, short whatever the backlog.
const SWEPT: usize = 500;

/// **How old an outbox row a host left behind is before it is forgotten**:
/// a committing host forgets its own as it delivers them, so a row older
/// than this is one whose host ended between its commit and its forgetting.
const OUTBOX_LEFT: std::time::Duration = std::time::Duration::from_secs(60);

/// **Where a deployment keeps a program's tables**: its database, the schema
/// its tables are in (a test's setting; the connection's own otherwise),
/// and how many renders read at once.
#[derive(Debug, Clone)]
pub(crate) struct Database {
    pub url: String,
    pub schema: Option<String>,
    pub seats: feed_pg::RenderSeats,
}

/// **`PW_TABLES_SCHEMA`, a test's setting**, as `PW_FEED_SCHEMA` is (the
/// integrator's ruling 4 of 2026-10-20 on track second-host): honoured only
/// for a development deployment on this machine, and refused at start for
/// any other, whose tables are where its connection's search path says.
pub(crate) fn test_schema(
    schema: Option<&str>,
    deployment: &identity::Deployment,
) -> Result<Option<String>, String> {
    match schema.filter(|s| !s.is_empty()) {
        None => Ok(None),
        Some(s) if deployment.development && deployment.loopback() => Ok(Some(s.to_string())),
        Some(_) => Err(
            "PW_TABLES_SCHEMA is a test's setting, for a development deployment on this \
             machine, and this deployment is not one"
                .to_string(),
        ),
    }
}

/// **A program's tables on PostgreSQL**, as its build declares them, what a
/// node is granted of them (each table's read and write), its sessions, and
/// the authority its scheduled runs are selected under.
pub(crate) struct TablesPg {
    tables: Arc<PgTables>,
    grants: Vec<&'static str>,
    kept: Arc<dyn identity::Kept>,
    queue_authority: pw_materialize::QueueAuthority,
    /// When its housekeeping next sweeps, on the host's clock
    /// ([`DataLayer::housekeeping`]): at once, where the host starts.
    housekept: Mutex<i64>,
    /// The database and the schema, for a test's SQL.
    #[cfg(test)]
    url: String,
    /// Each call of a table function a command linked, so far: what a test
    /// of a guarded command counts (none until its predicates answer).
    #[cfg(test)]
    pub(crate) command_calls: Arc<std::sync::atomic::AtomicU64>,
}

impl TablesPg {
    /// **The layer for what `pw build` built, on `database`**, where the
    /// program's contracts import its tables; none where they import none,
    /// whose host serves the program as before and leaves the database
    /// unused.
    pub(crate) fn for_build(
        build: &std::path::Path,
        database: &Database,
    ) -> Result<Option<TablesPg>, String> {
        let path = build.join("contracts.json");
        let contracts = std::fs::read_to_string(&path)
            .map_err(|e| format!("{}: {e}", path.display()))
            .and_then(|text| {
                ComponentContract::from_json(&text).map_err(|e| format!("contracts.json: {e}"))
            })?;
        if !tables::imports_tables(&contracts) {
            return Ok(None);
        }
        TablesPg::from_build(build, &contracts, database).map(Some)
    }

    /// **The layer for a build whose contracts import its tables**, on
    /// `database`: its `tables.json` applied to the database or held to what
    /// was applied there (ADR-0393's migration, drift refused). Refused,
    /// before the database is touched, where the build's tables may not be
    /// served, as the memory layer refuses them (`tables::servable`: a
    /// hand-written layer's operation beside them, a row's field of a
    /// handle's type, an upload).
    pub(crate) fn from_build(
        build: &std::path::Path,
        contracts: &[ComponentContract],
        database: &Database,
    ) -> Result<TablesPg, String> {
        tables::servable(build, contracts)?;
        let path = build.join("tables.json");
        let text =
            std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let options = pw_table_postgres::Options {
            render_seats: database.seats.seats,
            seat_wait: database.seats.wait,
            host: hosts::new_host(),
            ..pw_table_postgres::Options::default()
        };
        let tables = PgTables::open_json(&database.url, database.schema.as_deref(), &text, options)
            .map_err(|e| format!("PW_TABLES_DATABASE_URL: the program's tables: {e:?}"))?;
        let tables = Arc::new(tables);
        let grants = tables
            .host_tables()
            .names()
            .flat_map(|table| {
                [
                    tables::interned(format!("database.read<{table}>")),
                    tables::interned(format!("database.write<{table}>")),
                ]
            })
            .collect();
        Ok(TablesPg {
            kept: Arc::new(KeptSessions(Arc::clone(&tables))),
            tables,
            grants,
            queue_authority: pw_materialize::QueueAuthority::new(),
            housekept: Mutex::new(i64::MIN),
            #[cfg(test)]
            url: database.url.clone(),
            #[cfg(test)]
            command_calls: Arc::default(),
        })
    }

    /// The schema its tables are in.
    #[cfg(test)]
    pub(crate) fn schema(&self) -> &str {
        self.tables.schema()
    }
}

/// **A render's snapshot of the tables** (NEXT 4c): the adapter's seat,
/// shared by every read function the render links, or why none could be
/// taken, which every read of the render then answers.
struct PgSnapshot(Result<pw_table_postgres::Snapshot, String>);

impl DataLayer for TablesPg {
    /// No operation by name: a program's table functions are linked through
    /// the gate alone, per component ([`DataLayer::table_reads`]).
    fn reads(&self, _session: &str, _stopped: Option<Stopped>) -> Ops {
        Ops::new()
    }

    /// A seat of the adapter's, its read transaction opened at the render's
    /// first read: every query of one render reads it, whatever commits
    /// meanwhile. Where none could be taken within the wait, the render's
    /// reads fail, saying so: they never read the latest commit instead.
    fn snapshot(&self) -> Option<Snapshot> {
        Some(Box::new(PgSnapshot(self.tables.snapshot().map_err(|e| {
            format!("the render's snapshot of the tables: {e:?}")
        }))))
    }

    fn tables(&self) -> Option<&pw_host::tables::Manifest> {
        Some(self.tables.host_tables())
    }

    fn table_reads(
        &self,
        contract: &ComponentContract,
        granted: &Granted,
        snapshot: Option<&Snapshot>,
    ) -> Result<Ops, String> {
        let read = match snapshot {
            Some(snapshot) => match snapshot.downcast_ref::<PgSnapshot>() {
                Some(PgSnapshot(Ok(taken))) => taken.read_host_fns(contract, granted),
                Some(PgSnapshot(Err(why))) => return Err(why.clone()),
                None => return Err("a render's snapshot is not this program's tables'".into()),
            },
            None => self.tables.read_host_fns(contract, granted),
        };
        read.map_err(|e| format!("`{}`'s tables: {e:?}", contract.component_id))
    }

    /// A command's staging, its transaction not begun: the host's turn is
    /// taken where the command first needs it ([`PgStaging`]).
    fn begin<'a>(&'a self, _session: &str) -> Box<dyn Staged + 'a> {
        Box::new(PgStaging {
            layer: self,
            turn: std::cell::RefCell::new(Turn::NotBegun),
            actor: None,
            authorization: Authorization::NotAttempted,
            refused: false,
        })
    }

    fn grants(&self) -> Vec<&'static str> {
        self.grants.clone()
    }

    fn default_page(&self) -> Option<&'static str> {
        None
    }

    /// What a command's transaction gets on the database opened (ADR-0246,
    /// ADR-0393's `provides`): measured, a read-only database refused.
    fn provides(&self) -> Result<Provided, String> {
        let provided = self
            .tables
            .provides()
            .map_err(|e| format!("the tables' database: {e:?}"))?;
        Ok(Provided {
            transactions: provided.transactions.to_string(),
            reads: std::collections::BTreeSet::from([provided.reads.to_string()]),
            feed: provided.changes == "feed",
        })
    }

    /// Its records are the database's, committed with its writes (ADR-0325).
    fn keeps_interactions(&self) -> bool {
        true
    }

    fn recalled(&self, key: &str, since_ms: u64) -> Result<Option<(String, String)>, String> {
        self.tables
            .recalled(key, since_ms)
            .map_err(|e| format!("the interaction's record: {e:?}"))
    }

    /// Its runs are the database's, written with its commands' writes.
    fn runs(&self) -> Option<&dyn schedules::Runs> {
        Some(self)
    }

    /// Its sessions are the database's (ADR-0369): a restarted host keeps
    /// its readers signed in, and so does a second host on the database.
    fn sessions(&self) -> Option<Arc<dyn identity::Kept>> {
        Some(Arc::clone(&self.kept))
    }

    /// **What the database keeps of the platform's, kept bounded** (track
    /// `durable-tables`), a minute apart at most on the host's clock, and a
    /// second apart while a batch comes back full: the sessions past their
    /// lifetimes, the sign-ins past theirs (a sign-in's nonce and PKCE
    /// verifier with them), and the outbox rows a host that ended left
    /// behind, each a bounded batch of rows no transaction holds, waiting
    /// for none. A sweep that fails is said in the host's log, and the next
    /// pass sweeps again.
    fn housekeeping(&self, now_ms: i64, sessions: &identity::SessionLifetimes) {
        let mut next = self
            .housekept
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if now_ms < *next {
            return;
        }
        let swept = [
            (
                "sessions past their lifetimes",
                self.tables.sweep_sessions(lifetimes_of(sessions), SWEPT),
            ),
            (
                "sign-ins past their lifetime",
                self.tables
                    .sweep_sign_ins(identity::SIGN_IN_LIFETIME, SWEPT),
            ),
            (
                "outbox rows a host left behind",
                self.tables.trim_outbox(OUTBOX_LEFT, SWEPT),
            ),
        ];
        let mut full = false;
        for (what, done) in swept {
            match done {
                Ok(n) => full |= n >= SWEPT as u64,
                Err(e) => eprintln!("pw dev server: the tables' {what} were not swept: {e:?}"),
            }
        }
        *next = now_ms.saturating_add(if full { 1_000 } else { 60_000 });
    }

    #[cfg(test)]
    fn statements(&self) -> Option<u64> {
        Some(self.tables.operation_statements())
    }

    #[cfg(test)]
    fn table_calls_for_test(&self) -> Option<u64> {
        Some(self.command_calls.load(std::sync::atomic::Ordering::SeqCst))
    }

    /// SQL another writer of the database runs, in the tables' schema.
    #[cfg(test)]
    fn execute_for_test(&self, sql: &str) -> Result<(), String> {
        let mut c = postgres::Client::connect(&self.url, postgres::NoTls).map_err(feed_pg::pg)?;
        c.batch_execute(&format!(
            "SET search_path TO {}; {sql}",
            feed_pg::ident(self.schema())
        ))
        .map_err(feed_pg::pg)
    }
}

/// **Where one command's transaction is.**
enum Turn {
    /// Not begun: begun fenced on its reader's session where the session's
    /// authority is acquired first ([`Staged::authorize`]), and unfenced at
    /// its first need otherwise (a scheduled run's, a guest's).
    NotBegun,
    /// Begun: its functions stage in it.
    Open(Command),
    /// Committed, with every record of the platform's.
    Committed,
    /// Refused, or failed: it keeps nothing, and says why.
    Failed(String),
}

impl Turn {
    /// **The command's transaction**, begun on `tables` unfenced where
    /// nothing began it yet.
    fn begun(&mut self, tables: &PgTables) -> Result<&mut Command, String> {
        if matches!(self, Turn::NotBegun) {
            *self = match tables.begin() {
                Ok(command) => Turn::Open(command),
                Err(e) => Turn::Failed(format!("the tables began no command: {e:?}")),
            };
        }
        match self {
            Turn::Open(command) => Ok(command),
            Turn::Failed(why) => Err(why.clone()),
            Turn::Committed => Err("a command's tables commit once".into()),
            Turn::NotBegun => Err("the tables began no command".into()),
        }
    }
}

/// **Whether the command's session authority was acquired** (the fence).
#[derive(PartialEq, Eq)]
enum Authorization {
    NotAttempted,
    Acquired,
    Failed,
}

/// **One command's transaction on a program's tables**: its writes, its
/// runs and its records, committed by the adapter at once, or undone whole
/// where it is dropped before (the adapter's own `Drop`).
///
/// Its transaction, and the host's turn with it, is taken where the
/// command first needs it, not where the host begins the staging: a
/// command acting for a reader's session waits for the session's lock
/// before it takes the turn ([`PgTables::begin_fenced`]), so that a command
/// waiting for its session holds no other session's command up.
pub(crate) struct PgStaging<'a> {
    layer: &'a TablesPg,
    turn: std::cell::RefCell<Turn>,
    /// The browser actor admitted, whose session the command is fenced on.
    actor: Option<CommandActor>,
    authorization: Authorization,
    /// The commit failed knowing it kept nothing ([`Staged::definitely_refused`]).
    refused: bool,
}

impl PgStaging<'_> {
    fn command(&mut self) -> Result<&mut Command, String> {
        self.turn.get_mut().begun(&self.layer.tables)
    }

    /// A commit refused, knowing it kept nothing.
    fn refuse(&mut self, why: String) -> Result<Commit, String> {
        self.refused = true;
        Err(why)
    }

    /// **The session's authority, for `actor`**, as [`Staged::authorize`]
    /// acquires it: the command's transaction begun fenced on its reader's
    /// session, or a guest's own held.
    fn acquire(&mut self, actor: &CommandActor) -> Result<(), String> {
        if self.authorization != Authorization::NotAttempted {
            return Err("a staging cannot acquire session authority twice".into());
        }
        if !matches!(self.turn.get_mut(), Turn::NotBegun) {
            return Err(
                "a command's session authority is acquired before its transaction begins".into(),
            );
        }
        if matches!(actor.reader, ReaderIdentity::Guest) {
            actor.ensure_local_current()?;
            self.actor = Some(actor.clone());
            return Ok(());
        }
        if actor.is_local() || !actor.belongs_to(&self.layer.kept) {
            return Err("command session belongs to another authority adapter".into());
        }
        let fence = pw_table_postgres::Fence {
            hash: actor.hash(),
            lifetimes: lifetimes_of(&actor.lifetimes()),
            expected: admitted_as(actor),
        };
        let command = self
            .layer
            .tables
            .begin_fenced(fence)
            .map_err(|e| format!("session authority changed before commit: {e:?}"))?;
        *self.turn.get_mut() = Turn::Open(command);
        self.actor = Some(actor.clone());
        Ok(())
    }
}

/// The session `actor` was admitted as, as the adapter keeps one: none
/// where it was admitted as no one.
fn admitted_as(actor: &CommandActor) -> Option<pw_table_postgres::Session> {
    match &actor.reader {
        ReaderIdentity::Account(identity) => Some(session_of(identity)),
        ReaderIdentity::Anonymous | ReaderIdentity::Guest => None,
    }
}

fn session_of(identity: &identity::SessionIdentity) -> pw_table_postgres::Session {
    pw_table_postgres::Session {
        user: identity.principal.user.clone(),
        handle: identity.principal.handle.clone(),
        name: identity.principal.name.clone(),
        issuer: identity.principal.issuer.clone(),
        epoch: identity.epoch,
    }
}

fn lifetimes_of(lifetimes: &identity::SessionLifetimes) -> Lifetimes {
    Lifetimes {
        idle: lifetimes.idle,
        absolute: lifetimes.absolute,
    }
}

/// A value an outbox row keeps: a `String`, an `Int`, a `Bool`, or an
/// invalidated entry's `_` (ADR-0256); anything else keys no entry.
fn kept_value(name: &str, value: Option<&Val>) -> Result<serde_json::Value, String> {
    match value {
        None => Ok(serde_json::Value::Null),
        Some(Val::String(s)) => Ok(serde_json::Value::String(s.clone())),
        Some(Val::S64(n)) => Ok(serde_json::Value::from(*n)),
        Some(Val::Bool(b)) => Ok(serde_json::Value::Bool(*b)),
        Some(other) => Err(format!("`{name}` carries {other:?}, which keys no entry")),
    }
}

/// A value read back from the outbox, as it was handed it.
fn val_of(name: &str, value: &serde_json::Value) -> Result<Option<Val>, String> {
    match value {
        serde_json::Value::Null => Ok(None),
        serde_json::Value::String(s) => Ok(Some(Val::String(s.clone()))),
        serde_json::Value::Bool(b) => Ok(Some(Val::Bool(*b))),
        serde_json::Value::Number(n) => n
            .as_i64()
            .map(|n| Some(Val::S64(n)))
            .ok_or_else(|| format!("the outbox kept `{name}`'s {n}, which is no Int")),
        other => Err(format!(
            "the outbox kept `{name}`'s {other}, which keys no entry"
        )),
    }
}

/// **What the outbox committed, as the host delivers it**: the events, then
/// the invalidated entries, each in the order the commit took them.
fn delivered(committed: Vec<(i64, pw_table_postgres::Outboxed)>) -> Result<data::Outboxed, String> {
    let (mut events, mut dropped) = (Vec::new(), Vec::new());
    for (_, row) in committed {
        let values = row
            .values
            .iter()
            .map(|v| val_of(&row.name, v))
            .collect::<Result<Vec<_>, String>>()?;
        match row.kind {
            pw_table_postgres::OutboxKind::Event => {
                let values = values
                    .into_iter()
                    .map(|v| v.ok_or_else(|| format!("the event `{}` kept a null", row.name)))
                    .collect::<Result<Vec<_>, String>>()?;
                events.push((row.name, values));
            }
            pw_table_postgres::OutboxKind::Invalidation => dropped.push((row.name, values)),
        }
    }
    Ok((events, dropped))
}

impl Staged for PgStaging<'_> {
    /// **The command fenced on its reader's session** (ADR-0369), before it
    /// runs: the session's lock taken, outside the host's turn, on the
    /// connection the command's transaction then begins on and holds to its
    /// end, and the session as the actor was admitted, held now and again
    /// just before the commit. A guest is the host's own, as in memory. A
    /// command is fenced from its first statement or never: one whose
    /// transaction began unfenced is refused here, and keeps nothing.
    fn authorize(&mut self, actor: &CommandActor) -> Result<(), String> {
        let acquired = self.acquire(actor);
        match &acquired {
            Ok(()) => self.authorization = Authorization::Acquired,
            // Refused, the staging is unusable: whatever began is undone,
            // and nothing links, runs or commits in it from now on.
            Err(why) => {
                self.authorization = Authorization::Failed;
                *self.turn.get_mut() = Turn::Failed(why.clone());
            }
        }
        acquired
    }

    /// A command that writes nothing, or answers a declared error, is held
    /// to its reader's session again, in its transaction.
    fn revalidate(&mut self) -> Result<(), String> {
        if self.authorization == Authorization::Failed {
            return Err("command session authority was not acquired".into());
        }
        match &self.actor {
            Some(actor) if matches!(actor.reader, ReaderIdentity::Guest) => {
                actor.ensure_local_current().map_err(String::from)
            }
            Some(_) => self
                .command()?
                .revalidate()
                .map_err(|e| format!("session authority changed before commit: {e:?}")),
            None => Ok(()),
        }
    }

    /// No operation by name ([`Staged::table_ops`]).
    fn ops(&self) -> Ops {
        Ops::new()
    }

    fn table_ops(&self, contract: &ComponentContract, granted: &Granted) -> Result<Ops, String> {
        let linked = {
            let mut turn = self.turn.borrow_mut();
            if matches!(*turn, Turn::Committed) {
                return Err("a command's tables are linked before it commits".into());
            }
            turn.begun(&self.layer.tables)?
                .host_fns(contract, granted)
                .map_err(|e| format!("`{}`'s tables: {e:?}", contract.component_id))?
        };
        // A test counts each call of them.
        #[cfg(test)]
        let linked = linked
            .into_iter()
            .map(|(name, f)| {
                let calls = Arc::clone(&self.layer.command_calls);
                let counted: HostFn = Arc::new(move |args: &[Val]| {
                    calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    f(args)
                });
                (name, counted)
            })
            .collect();
        Ok(linked)
    }

    fn table_predicate_reads(
        &self,
        contract: &ComponentContract,
        granted: &Granted,
    ) -> Result<Ops, String> {
        let mut turn = self.turn.borrow_mut();
        if matches!(*turn, Turn::Committed) {
            return Err("a command's predicate reads are linked before it commits".into());
        }
        turn.begun(&self.layer.tables)?
            .predicate_host_fns(contract, granted)
            .map_err(|e| format!("`{}`'s predicate reads: {e:?}", contract.component_id))
    }

    /// Whether the command wrote, which its commit then keeps: the database
    /// keeps its rows, and the host's materializer none. A command whose
    /// writes cannot be read goes on to its commit, which refuses it.
    fn rows(&self) -> Option<Vec<(String, String)>> {
        match &*self.turn.borrow() {
            // Nothing ran: nothing was written.
            Turn::NotBegun => None,
            Turn::Open(command) => match command.written() {
                Ok(written) if written.is_empty() => None,
                _ => Some(Vec::new()),
            },
            _ => Some(Vec::new()),
        }
    }

    /// Committed in [`Staged::commit`]: nothing is left to make the layer's.
    fn publish(&mut self) {}

    /// **The command's writes, its runs, its interaction's record and its
    /// outbox, in its one transaction** (ADR-0338 1.3): the run it completes
    /// taken first (none taken, nothing commits: [`Commit::NotCurrent`]);
    /// its schedulings written; its references held; then its record and
    /// its outbox written and, its reader's session held to its fence, the
    /// `COMMIT`. Refused anywhere, nothing of it is kept; an unanswered
    /// `COMMIT` is said so, and the host reconciles it from the record.
    fn commit(
        &mut self,
        events: &[data::Handed],
        invalidated: &[data::Dropped],
        interaction: Option<&data::Recorded<'_>>,
        scheduled: &[schedules::Scheduled],
        completing: Option<schedules::Completing<'_>>,
    ) -> Result<Commit, String> {
        // Every way out before the `COMMIT` is sent keeps nothing, and is
        // said so ([`Staged::definitely_refused`]).
        let outbox = events
            .iter()
            .map(|(name, values)| {
                Ok(pw_table_postgres::Outboxed {
                    kind: pw_table_postgres::OutboxKind::Event,
                    name: name.clone(),
                    values: values
                        .iter()
                        .map(|v| kept_value(name, Some(v)))
                        .collect::<Result<_, String>>()?,
                })
            })
            .chain(invalidated.iter().map(|(name, values)| {
                Ok(pw_table_postgres::Outboxed {
                    kind: pw_table_postgres::OutboxKind::Invalidation,
                    name: name.clone(),
                    values: values
                        .iter()
                        .map(|v| kept_value(name, v.as_ref()))
                        .collect::<Result<_, String>>()?,
                })
            }))
            .collect::<Result<Vec<_>, String>>();
        let outbox = match outbox {
            Ok(outbox) => outbox,
            Err(why) => return self.refuse(why),
        };
        // Its transaction: begun where nothing began it (a run's command
        // that wrote nothing before it completes the run, a guest's that
        // only schedules).
        if let Err(why) = self.turn.get_mut().begun(&self.layer.tables) {
            // Committed once already: not a refusal of anything.
            if matches!(*self.turn.get_mut(), Turn::Committed) {
                return Err(why);
            }
            return self.refuse(why);
        }
        let mut command =
            match std::mem::replace(self.turn.get_mut(), Turn::Failed("committing".into())) {
                Turn::Open(command) => command,
                _ => return self.refuse("the tables began no command".into()),
            };
        let schedulings: Vec<pw_table_postgres::Scheduling> = scheduled
            .iter()
            .map(|s| pw_table_postgres::Scheduling {
                key: s.key.clone(),
                command: s.command.clone(),
                args: s.args.clone(),
                user: s.user.clone(),
                due_ms: s.due_ms,
            })
            .collect();
        if let Some(completion) = completing {
            if !completion.belongs_to(&self.layer.queue_authority) {
                return self.refuse("scheduled completion belongs to another queue adapter".into());
            }
            let Some(run) = run_of(completion.run) else {
                return self.refuse("scheduled completion of a run this layer never kept".into());
            };
            // Taken first, before its schedulings are written: a run that
            // schedules itself again is taken before its next generation
            // replaces it under its key.
            match command.consume(&run, completion.now_ms) {
                Ok(true) => {}
                // Not the run selected any more: nothing of the command
                // commits, and its transaction is undone as it is dropped.
                Ok(false) => return Ok(Commit::NotCurrent),
                // The run selected, and not taken (its deletion suppressed,
                // or the statement failed): refused, never taken for a run
                // another command completed.
                Err(e) => return self.refuse(format!("the scheduled run was not taken: {e:?}")),
            }
        }
        if let Err(e) = command.schedule(&schedulings) {
            return self.refuse(format!("the runs were not scheduled: {e:?}"));
        }
        if let Some(actor) = &self.actor
            && matches!(actor.reader, ReaderIdentity::Guest)
            && let Err(e) = actor.ensure_local_current()
        {
            return self.refuse(e.into());
        }
        let prepared = match command.prepare() {
            Ok(prepared) => prepared,
            Err(e) => return self.refuse(format!("the tables refused the commit: {e:?}")),
        };
        let committed = match prepared.publish_with(Records {
            interaction: interaction.map(|r| pw_table_postgres::Interaction {
                key: r.key,
                sent: r.sent,
                answer: r.answer,
                at_ms: r.at_ms,
                forget_before_ms: r.forget_before_ms,
            }),
            outbox: &outbox,
        }) {
            Ok(committed) => committed,
            // A `COMMIT` sent and never answered may have committed, and the
            // interaction recorded by another commit is what answers this
            // one: both are reconciled from the record, by the host.
            Err(
                e @ (pw_table_postgres::Error::OutcomeUnknown(_)
                | pw_table_postgres::Error::Recorded(_)),
            ) => return Err(format!("the tables did not commit: {e:?}")),
            // Rolled back, as the database answered: a serialization
            // failure, a deadlock, a refusal, a statement that failed.
            Err(e) => return self.refuse(format!("the tables did not commit: {e:?}")),
        };
        *self.turn.get_mut() = Turn::Committed;
        // Delivered by this host, and read by no other (no host tails this
        // layer's outbox yet, track `durable-tables`): its rows are forgotten
        // at once, and any a host that ended left behind by the layer's
        // housekeeping ([`DataLayer::housekeeping`]).
        let ids: Vec<i64> = committed.outbox.iter().map(|(id, _)| *id).collect();
        if let Err(e) = self.layer.tables.forget_outbox(&ids) {
            eprintln!("pw dev server: the tables' delivered outbox rows were kept: {e:?}");
        }
        // Committed: what the outbox kept is what is delivered. One that
        // cannot be read back is what the command handed it.
        match delivered(committed.outbox) {
            Ok(outboxed) => Ok(Commit::Applied(outboxed)),
            Err(why) => {
                eprintln!("pw dev server: the tables' outbox was not read back: {why}");
                Ok(Commit::Applied((events.to_vec(), invalidated.to_vec())))
            }
        }
    }

    fn definitely_refused(&self) -> bool {
        self.refused
    }
}

/// A run as the scheduler selected it, as the adapter keeps one: none for a
/// run of another protocol than the one this layer writes.
fn run_of(run: &pw_materialize::ScheduledRun) -> Option<pw_table_postgres::Run> {
    (run.protocol == 1).then(|| pw_table_postgres::Run {
        id: run.id,
        command: run.command.clone(),
        args: run.args.clone(),
        user: run.user.clone(),
        due_ms: run.due_ms,
        attempts: run.attempts,
        failed: run.failed.clone(),
    })
}

/// A run as the adapter keeps it, as the scheduler selects one.
fn scheduled_run(run: pw_table_postgres::Run) -> pw_materialize::ScheduledRun {
    pw_materialize::ScheduledRun {
        protocol: 1,
        id: run.id,
        command: run.command,
        args: run.args,
        user: run.user,
        due_ms: run.due_ms,
        attempts: run.attempts,
        failed: run.failed,
    }
}

/// **Its runs, outside a command** (the scheduled-commands track): those the
/// adapter keeps, each one statement on a connection of its pool's.
impl schedules::Runs for TablesPg {
    fn authority(&self) -> &pw_materialize::QueueAuthority {
        &self.queue_authority
    }

    fn current(
        &self,
        expected: &pw_materialize::ScheduledRun,
        now_ms: i64,
    ) -> Result<bool, String> {
        let Some(run) = run_of(expected) else {
            return Ok(false);
        };
        self.tables
            .run_current(&run, now_ms)
            .map_err(|e| format!("{e:?}"))
    }

    fn retry_current(
        &self,
        expected: &pw_materialize::ScheduledRun,
        attempts: i64,
        due_ms: i64,
        failed: Option<&str>,
    ) -> Result<bool, String> {
        let Some(run) = run_of(expected) else {
            return Ok(false);
        };
        self.tables
            .retry_run(&run, attempts, due_ms, failed)
            .map_err(|e| format!("{e:?}"))
    }

    fn due_runs(
        &self,
        now_ms: i64,
        limit: usize,
    ) -> Result<Vec<pw_materialize::ScheduledRun>, String> {
        self.tables
            .due_runs(now_ms, limit)
            .map(|runs| runs.into_iter().map(scheduled_run).collect())
            .map_err(|e| format!("{e:?}"))
    }

    fn next_run(&self) -> Result<Option<i64>, String> {
        self.tables.next_run().map_err(|e| format!("{e:?}"))
    }

    #[cfg(test)]
    fn ran(&self, id: i64) -> Result<(), String> {
        self.tables.forget_run(id).map_err(|e| format!("{e:?}"))
    }

    #[cfg(test)]
    fn retry(
        &self,
        id: i64,
        attempts: i64,
        due_ms: i64,
        failed: Option<&str>,
    ) -> Result<(), String> {
        self.tables
            .set_run(id, attempts, due_ms, failed)
            .map_err(|e| format!("{e:?}"))
    }

    #[cfg(test)]
    fn all_runs(&self) -> Result<Vec<pw_materialize::ScheduledRun>, String> {
        self.tables
            .runs()
            .map(|runs| runs.into_iter().map(scheduled_run).collect())
            .map_err(|e| format!("{e:?}"))
    }
}

/// **Sessions and sign-ins kept with the program's tables** (ADR-0369): in
/// the adapter's schema, by each secret's SHA-256, on connections of their
/// own; a session's change under the lock each command fenced on it holds.
struct KeptSessions(Arc<PgTables>);

impl identity::Kept for KeptSessions {
    fn session(
        &self,
        hash: &[u8],
        lifetimes: &identity::SessionLifetimes,
    ) -> Result<Option<identity::SessionIdentity>, String> {
        self.0
            .admit_session(
                hash,
                lifetimes_of(lifetimes),
                hosts::touched_after(lifetimes),
            )
            .map(|kept| kept.map(identity_of))
            .map_err(|e| format!("{e:?}"))
    }

    fn inspect(
        &self,
        hash: &[u8],
        lifetimes: &identity::SessionLifetimes,
    ) -> Result<Option<identity::SessionIdentity>, String> {
        self.0
            .inspect_session(hash, lifetimes_of(lifetimes))
            .map(|kept| kept.map(identity_of))
            .map_err(|e| format!("{e:?}"))
    }

    fn open(
        &self,
        hash: &[u8],
        principal: &identity::Principal,
        epoch: &[u8; 16],
    ) -> Result<(), String> {
        self.0
            .open_session(
                hash,
                &pw_table_postgres::Session {
                    user: principal.user.clone(),
                    handle: principal.handle.clone(),
                    name: principal.name.clone(),
                    issuer: principal.issuer.clone(),
                    epoch: *epoch,
                },
            )
            .map_err(|e| format!("{e:?}"))
    }

    fn close(&self, hash: &[u8]) -> Result<(), String> {
        self.0.close_session(hash).map_err(|e| format!("{e:?}"))
    }

    fn begin_sign_in(&self, hash: &[u8], nonce: &str, verifier: &str) -> Result<(), String> {
        self.0
            .begin_sign_in(hash, nonce, verifier)
            .map_err(|e| format!("{e:?}"))
    }

    fn take_sign_in(
        &self,
        hash: &[u8],
    ) -> Result<Option<(String, String, std::time::Duration)>, String> {
        self.0.take_sign_in(hash).map_err(|e| format!("{e:?}"))
    }
}

fn identity_of(kept: pw_table_postgres::Session) -> identity::SessionIdentity {
    identity::SessionIdentity {
        principal: identity::Principal {
            user: kept.user,
            handle: kept.handle,
            name: kept.name,
            issuer: kept.issuer,
        },
        epoch: kept.epoch,
    }
}
