//! **A program's data, as a deployment supplies it to the host** (ADR-0218).
//!
//! The host serves any built program: its pages from their plans, its
//! queries and commands from their components and contracts, its events and
//! invalidations from the outbox. What the program's contracts import of a
//! data layer (`store:data/carts#add`, `feed:data/posts#publish`) is the
//! deployment's, and this is what the host asks of it.

use super::*;
use std::collections::BTreeSet;

/// Operations by their key, `store:data/carts#add`.
pub(crate) type Ops = BTreeMap<String, HostFn>;

/// **One render's snapshot of a layer** (NEXT 4c, track `render-merge`):
/// the layer's own value, which the host holds for the render and hands back
/// with each query's reads ([`DataLayer::reads_in`]), and drops when the
/// render ends. What it holds is the layer's to say: the store as one moment
/// left it, or a read transaction.
pub(crate) type Snapshot = Box<dyn std::any::Any + Send + Sync>;

pub(crate) trait DataLayer: Send + Sync {
    /// **The operations a query reads through, for one session.** Nothing a
    /// query does commits.
    fn reads(&self, session: &str, stopped: Option<Stopped>) -> Ops;

    /// **One render's snapshot** (NEXT 4c, track `render-merge`): what every
    /// read of one render reads, asked for where the render begins and given
    /// up where it ends, so the render sees one state of the data whatever
    /// commits meanwhile: in memory, the data as it is when asked; on
    /// PostgreSQL, a seat among a bounded few, waited for while every one is
    /// taken, whose read transaction's snapshot is its first statement's
    /// (PostgreSQL 18, 13.2.2). By default none, and each query of a render
    /// reads as [`DataLayer::reads`] does: a layer whose data never changes
    /// (kiokun's files) needs none.
    fn snapshot(&self) -> Option<Snapshot> {
        None
    }

    /// **The operations one query of a render reads through**, as
    /// [`DataLayer::reads`] makes them for one session, every one reading
    /// `snapshot`, which this layer's [`DataLayer::snapshot`] took (NEXT 4c).
    /// By default [`DataLayer::reads`].
    fn reads_in(&self, _snapshot: &Snapshot, session: &str, stopped: Option<Stopped>) -> Ops {
        self.reads(session, stopped)
    }

    /// **A command's writes, staged until its transaction commits.** The
    /// staging holds whatever the layer needs held from the call to the
    /// commit, and gives it up when dropped: a command that fails, or traps,
    /// commits nothing.
    fn begin<'a>(&'a self, session: &str) -> Box<dyn Staged + 'a>;

    /// **What a node grants it**, beyond the platform's session and outbox.
    fn grants(&self) -> Vec<&'static str>;

    /// **The page a document with none recorded is**, where it has one.
    fn default_page(&self) -> Option<&'static str>;

    /// **The style its pages carry in their head** (ADR-0220), where it has
    /// one: the store's menu's containment (ADR-0187).
    fn style(&self) -> &'static str {
        ""
    }

    /// **Whether a session's documents are versioned by an entry of its
    /// own** (the store's cart), which a commit regenerates; otherwise a
    /// commit moves the session's documents on the host's clock.
    fn session_entry(&self) -> bool {
        false
    }

    /// **What the database it opened provides** (ADR-0246), which the host
    /// compares with what the program's sources state before it serves. By
    /// default what the host's own database gives a resource no source holds
    /// (ADR-0207): serializable transactions, reads of the latest commit, and
    /// no feed of its changes. An in-memory layer gives that under its lock.
    /// **The statements its reads and commands have run** (NEXT 4b): a
    /// layer that counts none (memory) says nothing.
    #[cfg(test)]
    fn statements(&self) -> Option<u64> {
        None
    }

    /// **Each operation's calls and statements so far**, by its name, on a
    /// layer that counts them (NEXT 4c's baseline).
    #[cfg(test)]
    fn statements_by_op(&self) -> BTreeMap<String, (u64, u64)> {
        BTreeMap::new()
    }

    /// **Each command's transaction's statements**, in the order the
    /// commands began, on a layer that counts them (NEXT 4b).
    #[cfg(test)]
    fn statements_by_transaction(&self) -> Vec<u64> {
        Vec::new()
    }

    /// **SQL a test runs on the layer's database**, where it has one: fifty
    /// stores more (NEXT 4b).
    #[cfg(test)]
    fn execute_for_test(&self, sql: &str) -> Result<(), String> {
        Err(format!("this layer runs no SQL: {sql}"))
    }

    /// **The connections this layer's renders read in** (NEXT 4c's tests):
    /// a database's layer's, and none in memory.
    #[cfg(test)]
    fn renders_for_test(&self) -> Option<Arc<crate::feed_pg::Renders>> {
        None
    }

    fn provides(&self) -> Result<Provided, String> {
        Ok(Provided::host_database())
    }

    /// **Each session's principal, as the identity keeps it** (track
    /// `identity`, ADR-0258): handed to the layer once, when the server is
    /// built, so that it maps a session to the user it acts as. A layer that
    /// knows no users ignores it.
    fn identified_by(&self, _principals: crate::identity::Principals) {}

    /// **The uploads it commits with what it writes** (track `uploads`):
    /// handed once, before anything is served, to a layer whose program
    /// declares an upload. By default a layer keeps no upload.
    fn uploaded_by(&self, _leases: crate::uploads::Leases) {}

    /// **Whether a committed row names the blob `key`** (ADR-0261): the
    /// uploads serve a blob only while one does, so a deleted post's image
    /// is not served. By default a layer names none, and a layer that keeps
    /// no upload has none to serve.
    fn names_blob(&self, _key: &str) -> Result<bool, String> {
        Ok(false)
    }

    /// **Whether it keeps a command's interactions in its own transactions**
    /// (ADR-0325): a layer whose writes outlive the host, which a restarted
    /// host or a second one reads, keeps their records beside them. By
    /// default the host's materializer keeps them, with the rows it commits.
    fn keeps_interactions(&self) -> bool {
        false
    }

    /// **An interaction's committed record** (ADR-0325), recorded at
    /// `since_ms` or later: the arguments it was sent with, and what it
    /// answered. A layer that keeps none has none.
    fn recalled(&self, _key: &str, _since_ms: u64) -> Result<Option<(String, String)>, String> {
        Ok(None)
    }

    /// **The runs its commands schedule, where it keeps them** (the
    /// scheduled-commands track): written in its own transactions with the
    /// commands' writes ([`Staged::commit`]), by a layer whose writes outlive
    /// the host, as its interactions are. By default the host's materializer
    /// keeps them, with the rows it commits.
    fn runs(&self) -> Option<&dyn crate::schedules::Runs> {
        None
    }

    /// **This host's reader of the other hosts' commits** (ADR-0369, track
    /// `second-host`), where the layer's database is shared: the outbox read
    /// from the host's cursor. A layer in memory is one host by definition,
    /// its data the process's, and has none.
    fn tail(&self) -> Option<&crate::hosts::Tail> {
        None
    }

    /// **This host's instance on the database it shares** (track
    /// `second-host`, the integrator's ruling 1 of 2026-10-20), which every
    /// page number it gives carries, so that hosts number their pages apart.
    /// A layer in memory is one host by definition, and has none.
    fn instance(&self) -> Option<u64> {
        None
    }

    /// **Where its sessions and sign-ins are kept beyond the host's memory**
    /// (ADR-0369, track `second-host`): its database, where the hosts share
    /// one, so a restarted host or another keeps a reader signed in. A layer
    /// in memory keeps them in the host's, as long as it runs.
    fn sessions(&self) -> Option<Arc<dyn crate::identity::Kept>> {
        None
    }

    /// **Every operation it supplies**, read from the functions it builds,
    /// not from a list kept beside them.
    fn operations(&self) -> BTreeSet<String> {
        let mut ops: BTreeSet<String> = self.reads("", None).into_keys().collect();
        ops.extend(self.begin("").ops().into_keys());
        ops
    }
}

/// **One command's writes, staged** (ADR-0218).
/// The authoritative commit location/outcome; NotCurrent never means fallback.
pub(crate) enum Commit {
    Materializer,
    Applied(Outboxed),
    NotCurrent,
}

/// Scope of the materializer projection, distinct from authoritative business rows.
pub(crate) enum ProjectionScope {
    Application,
    Session,
}

pub(crate) trait Staged {
    /// The command's operations: the layer's reads, and its writes, which
    /// stage.
    fn ops(&self) -> Ops;
    /// What the commit writes in the materializer's transaction, by key, or
    /// none where nothing was written.
    fn rows(&self) -> Option<Vec<(String, String)>>;
    /// Session presentation has no retained target for a Clock-origin invocation.
    fn projection_scope(&self) -> ProjectionScope {
        ProjectionScope::Application
    }

    /// After the commit: what was staged is the layer's.
    fn publish(&mut self);

    /// **The command's writes and what it handed the outbox, committed in
    /// the layer's own transaction** (ADR-0208, ADR-0246), where the layer
    /// keeps its own outbox: events and invalidated entries, each by its
    /// declaration's path with the values the command computed.
    ///
    /// Returns what its outbox committed, read back once the transaction
    /// committed, which the host delivers. `None` where the layer keeps no
    /// outbox: the host's materializer commits the events with [`rows`].
    /// An error may be refusal or an unknown COMMIT acknowledgement. Reconcile
    /// from authoritative receipt/queue state; never infer rollback from Err alone.
    ///
    /// [`rows`]: Staged::rows
    ///
    /// **An interaction's record** (ADR-0325), where the command has one, is
    /// committed in the same transaction by a layer that keeps its own
    /// ([`DataLayer::keeps_interactions`]), and by the host's materializer
    /// with [`rows`] otherwise; and so are **the runs it scheduled, or
    /// cancelled** (the scheduled-commands track), by a layer that keeps its
    /// own ([`DataLayer::runs`]).
    fn commit(
        &mut self,
        _events: &[Handed],
        _invalidated: &[Dropped],
        _interaction: Option<&Recorded<'_>>,
        _scheduled: &[crate::schedules::Scheduled],
        _completing: Option<crate::schedules::Completing<'_>>,
    ) -> Result<Commit, String> {
        Ok(Commit::Materializer)
    }

    /// Idempotent blob preparation only, inside the materializer transaction
    /// after its job fence. This is never a fallible business publication seam.
    fn prepare_materialized(&mut self) -> Result<(), String> {
        Ok(())
    }
}

/// **The guard of a layer's writes** (ADR-0218, ADR-0352): one command of
/// this host at a time, from its `begin` to its staging's drop, and each
/// scheduler write (`Runs::ran`, `Runs::retry`) between two commands, never
/// inside one. The std mutex, as `lock` returns it.
///
/// In a test's build a taker that finds the guard held tells the test first,
/// if the test asked ([`Writes::tell_on_wait`]), and then waits. A guard's
/// test waits for that report: "held at the guard" is a fact the guard can
/// state, where "has not returned yet" is a silence that a slow write makes
/// too.
pub(crate) struct Writes(Mutex<()>);

impl Writes {
    pub(crate) fn new() -> Writes {
        Writes(Mutex::new(()))
    }

    /// The guard, once whoever holds it lets go.
    pub(crate) fn lock(&self) -> std::sync::LockResult<std::sync::MutexGuard<'_, ()>> {
        #[cfg(test)]
        match self.0.try_lock() {
            Ok(guard) => return Ok(guard),
            Err(std::sync::TryLockError::Poisoned(poisoned)) => return Err(poisoned),
            // Held: a test that asked is told, and this thread then waits.
            Err(std::sync::TryLockError::WouldBlock) => WAITING.with(|tell| {
                if let Some(tell) = &*tell.borrow() {
                    tell();
                }
            }),
        }
        self.0.lock()
    }
}

#[cfg(test)]
thread_local! {
    /// What this thread's test asked to be told when it finds a write guard
    /// held ([`Writes::tell_on_wait`]).
    static WAITING: std::cell::RefCell<Option<Box<dyn Fn()>>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
impl Writes {
    /// **A test's: `tell` is called on this thread each time it finds a
    /// layer's write guard held**, just before it waits for the guard, until
    /// the returned [`Telling`] is dropped. Only this thread's own waits are
    /// told, so a test that spawns the write it watches hears that write and
    /// no other.
    pub(crate) fn tell_on_wait(tell: impl Fn() + 'static) -> Telling {
        WAITING.with(|waiting| *waiting.borrow_mut() = Some(Box::new(tell)));
        Telling
    }
}

/// A test's request to be told of a wait for a write guard, until it is
/// dropped ([`Writes::tell_on_wait`]).
#[cfg(test)]
pub(crate) struct Telling;

#[cfg(test)]
impl Drop for Telling {
    fn drop(&mut self) {
        WAITING.with(|waiting| *waiting.borrow_mut() = None);
    }
}

/// **An interaction's record, committed with its command's writes**
/// (ADR-0325, PW0348): its key (the session, the command and the
/// interaction), the arguments it was sent with, and what it answered as
/// ADR-0121 keeps it, at `at_ms` on the host's clock. Those recorded before
/// `forget_before_ms` are forgotten in the same transaction.
pub(crate) struct Recorded<'a> {
    pub key: &'a str,
    pub sent: &'a str,
    pub answer: &'a str,
    pub at_ms: u64,
    pub forget_before_ms: u64,
}

/// One thing a command handed the outbox: its declaration's path, and the
/// values the command computed.
pub(crate) type Handed = (String, Vec<Val>);

/// **An entry a command drops** (ADR-0209, ADR-0256): its query's path, and
/// the value at each of its parameters, `None` where the command wrote `_`,
/// every value there.
pub(crate) type Dropped = (String, Vec<Option<Val>>);

/// **What a layer's outbox committed** (ADR-0246): the events, then the
/// invalidated entries, each in the order the command handed them.
pub(crate) type Outboxed = (Vec<Handed>, Vec<Dropped>);

/// **What a database provides** (ADR-0207's vocabulary, ADR-0246): the
/// isolation a command's transaction gets, what a read may promise, and
/// whether it tells what changed once committed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Provided {
    /// `serializable`, `snapshot`, `read_committed` or `none`.
    pub transactions: String,
    pub reads: BTreeSet<String>,
    pub feed: bool,
}

impl Provided {
    /// What the host's own database gives (ADR-0005, ADR-0207).
    pub(crate) fn host_database() -> Provided {
        Provided {
            transactions: "serializable".to_string(),
            reads: BTreeSet::from(["strong".to_string()]),
            feed: false,
        }
    }
}

/// **A source's clauses, as `pw build` wrote them** (`sources.json`).
#[derive(Debug, Clone, serde::Deserialize)]
pub(crate) struct Declared {
    pub name: String,
    pub holds: Vec<String>,
    pub transactions: String,
    pub reads: Vec<String>,
    pub changes: String,
}

/// How much an isolation prevents (Berenson et al., SIGMOD 1995), in the
/// checker's order: serializable, snapshot, read committed, none.
fn isolation(word: &str) -> u8 {
    match word {
        "serializable" => 3,
        "snapshot" => 2,
        "read_committed" => 1,
        _ => 0,
    }
}

/// **Does what a database provides give what a source states?**
/// (ADR-0246): its transactions at least as isolated, each promise its reads
/// make (`strong` gives every one, and every database gives `eventual`), and
/// a feed of its changes where it states one. Each shortfall, one line.
pub(crate) fn shortfalls(declared: &Declared, provided: &Provided) -> Vec<String> {
    let mut out = Vec::new();
    if isolation(&declared.transactions) > isolation(&provided.transactions) {
        out.push(format!(
            "`{}` states `transactions {}`, and the database's are {}",
            declared.name, declared.transactions, provided.transactions
        ));
    }
    for read in &declared.reads {
        let given = read == "eventual"
            || provided.reads.contains("strong")
            || provided.reads.contains(read);
        if !given {
            out.push(format!(
                "`{}` states `reads {read}`, and the database's reads give {}",
                declared.name,
                provided
                    .reads
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }
    if declared.changes == "feed" && !provided.feed {
        out.push(format!(
            "`{}` states `changes feed`, and its data layer delivers no feed of its changes",
            declared.name
        ));
    }
    out
}

/// **What a layer's database must give, and each way it falls short**
/// (ADR-0246): each declared source holding a resource the layer's grants
/// name (`database.read<Post>` names `Post`), and, for a resource no source
/// holds, the host's own database's guarantees (ADR-0207).
pub(crate) fn held_to(sources: &[Declared], grants: &[&str], provided: &Provided) -> Vec<String> {
    let resources: BTreeSet<&str> = grants
        .iter()
        .filter_map(|g| {
            g.strip_prefix("database.read<")
                .or_else(|| g.strip_prefix("database.write<"))
                .and_then(|r| r.strip_suffix('>'))
        })
        .collect();
    let mut out = Vec::new();
    for source in sources
        .iter()
        .filter(|s| s.holds.iter().any(|h| resources.contains(h.as_str())))
    {
        out.extend(shortfalls(source, provided));
    }
    let unheld: Vec<&str> = resources
        .iter()
        .copied()
        .filter(|r| !sources.iter().any(|s| s.holds.iter().any(|h| h == r)))
        .collect();
    if !unheld.is_empty() {
        let host = Provided::host_database();
        let implied = Declared {
            name: format!("the host's database, holding {}", unheld.join(", ")),
            holds: unheld.iter().map(|r| r.to_string()).collect(),
            transactions: host.transactions,
            reads: host.reads.into_iter().collect(),
            changes: "none".to_string(),
        };
        out.extend(shortfalls(&implied, provided));
    }
    out
}
