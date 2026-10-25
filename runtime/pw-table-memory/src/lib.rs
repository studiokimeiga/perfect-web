//! Generic in-memory execution of compiler-declared Pleris tables (ADR-0338
//! D2; ADR-0385, the reads; ADR-0386, the writes and the other reads).
//!
//! It consumes the compiler's `tables.json`, not a second schema format, and
//! serves every primitive for any table it declares: `get`, `get-many`, a
//! prefix's read and its set form, an index's and its set form, `all` of a
//! bounded table, and the writes `insert`, `put`, `delete` and
//! `delete-prefix`. An admitted component reaches them only as the host's
//! gate admits them (`pw_host::tables`): a query's reads through
//! [`MemoryTables::read_host_fns`], a command's reads and writes through
//! [`Command::host_fns`].
//!
//! **Maps by key and by index, under a lock** (ADR-0338 5.2's memory row):
//! each table's rows by their key, each index as a map of its values to the
//! keys holding them, and each reference as a map of the key it refers to,
//! all kept by one mutation ([`Data::set`]), which refuses a unique value
//! another row holds before it changes anything.
//!
//! **A command is staged** (ADR-0218's staging, ADR-0386): one command at a
//! time writes the adapter's working copy, reads its own writes there (as a
//! PostgreSQL transaction does, 13.2.2 and 13.2.3 of its manual), and none of
//! them is visible to any other read until it commits: queries read the
//! published copy. A refused write changes nothing; a command dropped
//! without committing (refused, failed, trapped) is undone whole; a commit
//! is held to the bound and the references first ([`Command::prepare`]), and
//! then published whole ([`Prepared::publish`]), which cannot fail.
//!
//! JSON is the internal adapter value representation, not the public WIT ABI.
//! Unsupported storage variants fail closed rather than losing information.
//! The development server's routing of a program's tables to this adapter,
//! the platform's records committed with a command's writes, and other
//! databases remain later obligations; no production claim follows.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};

use pw_core::tables::{Primitive, Schema, Stored, TableJson, TablesJson};
use serde_json::{Map, Value};

#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    Schema(String),
    Input(String),
    /// A write refused by the row that holds its key or a unique value it
    /// would hold. Nothing was written.
    Conflict {
        table: String,
        existing: Value,
    },
    /// The command's transaction has ended (committed or undone), or it is
    /// committing and writes nothing more.
    Ended,
    StoragePoisoned,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Atom {
    Bool(bool),
    Int(i64),
    Str(String),
}

impl Atom {
    fn parse(value: &Value) -> Result<Self, Error> {
        match value {
            Value::Bool(v) => Ok(Self::Bool(*v)),
            Value::String(v) => Ok(Self::Str(v.clone())),
            Value::Number(v) => v.as_i64().map(Self::Int).ok_or_else(|| {
                Error::Input("a table selector must be a scalar integer, boolean or string".into())
            }),
            _ => Err(Error::Input(
                "a table selector must be a scalar integer, boolean or string".into(),
            )),
        }
    }
}

type Key = Vec<Atom>;
type Row = Arc<Map<String, Value>>;

// --- what the manifest declares, read once --------------------------------

struct Shape {
    tables: Vec<TableShape>,
    /// Each function, by its import: its table's place and what it is.
    functions: BTreeMap<String, (usize, FunctionShape)>,
}

struct TableShape {
    name: String,
    fields: Vec<(String, Stored)>,
    key: Vec<String>,
    indexes: Vec<IndexShape>,
    references: Vec<ReferenceShape>,
    bounded: Option<u64>,
}

struct IndexShape {
    fields: Vec<String>,
    unique: bool,
}

/// `refers <field> to <Table>`: the field holds a key of the table at
/// `target`, or nothing where it is an `Option`.
struct ReferenceShape {
    field: String,
    target: usize,
}

struct FunctionShape {
    primitive: Primitive,
    by: Vec<String>,
    sorted_by: Vec<String>,
    /// Answers none or one row: a `get`, a unique index's read.
    one: bool,
}

impl Shape {
    fn function(&self, import: &str) -> Result<(usize, &FunctionShape), Error> {
        self.functions
            .get(import)
            .map(|(table, function)| (*table, function))
            .ok_or_else(|| Error::Input(format!("undeclared table operation {import}")))
    }
}

// --- a table's rows, and its maps ------------------------------------------

/// **One table's rows, by key, and its maps**: each index's values to the
/// keys holding them, each reference's referred key to the keys referring.
#[derive(Debug, Clone, Default)]
struct Data {
    rows: BTreeMap<Key, Row>,
    indexes: Vec<BTreeSet<(Key, Key)>>,
    referring: Vec<BTreeSet<(Key, Key)>>,
    /// Commits that wrote it.
    version: u64,
}

/// What [`Data::set`] did.
#[derive(Debug)]
enum Set {
    /// Written: the row the key held before, if any.
    Done(Option<Row>),
    /// Refused, nothing changed: the row of another key holding a unique
    /// value the row would hold.
    Held(Row),
}

impl Data {
    fn new(shape: &TableShape) -> Data {
        Data {
            rows: BTreeMap::new(),
            indexes: vec![BTreeSet::new(); shape.indexes.len()],
            referring: vec![BTreeSet::new(); shape.references.len()],
            version: 0,
        }
    }

    /// **The one mutation**: `key`'s row becomes `row` (none: it is removed),
    /// every index and reference entry of the row before removed and of the
    /// row after added. A unique index's value held by another key refuses
    /// it before anything changes; and everything that can fail is done
    /// before anything changes, so that a row and its maps never part.
    fn set(&mut self, shape: &TableShape, key: &Key, row: Option<Row>) -> Result<Set, Error> {
        let after = match &row {
            Some(row) => Some(entries(shape, row)?),
            None => None,
        };
        if let Some((values, _)) = &after {
            for ((index, declared), values) in self.indexes.iter().zip(&shape.indexes).zip(values) {
                if declared.unique
                    && let Some(holder) = holding(index, values).find(|held| *held != key)
                {
                    let held = self.rows.get(holder).cloned().ok_or_else(|| {
                        Error::Schema(format!("{}'s index names a missing row", shape.name))
                    })?;
                    return Ok(Set::Held(held));
                }
            }
        }
        let before = match self.rows.get(key) {
            Some(prior) => Some(entries(shape, prior)?),
            None => None,
        };
        let prior = match row {
            Some(row) => self.rows.insert(key.clone(), row),
            None => self.rows.remove(key),
        };
        if let Some((values, referred)) = before {
            for (index, values) in self.indexes.iter_mut().zip(values) {
                index.remove(&(values, key.clone()));
            }
            for (map, referred) in self.referring.iter_mut().zip(referred) {
                if let Some(referred) = referred {
                    map.remove(&(referred, key.clone()));
                }
            }
        }
        if let Some((values, referred)) = after {
            for (index, values) in self.indexes.iter_mut().zip(values) {
                index.insert((values, key.clone()));
            }
            for (map, referred) in self.referring.iter_mut().zip(referred) {
                if let Some(referred) = referred {
                    map.insert((referred, key.clone()));
                }
            }
        }
        Ok(Set::Done(prior))
    }

    /// The rows of keys beginning with `prefix`, in key order.
    fn prefixed<'a>(&'a self, prefix: &Key) -> impl Iterator<Item = (&'a Key, &'a Row)> + use<'a> {
        let prefix = prefix.clone();
        self.rows
            .range(prefix.clone()..)
            .take_while(move |(key, _)| key.starts_with(&prefix))
    }
}

/// The keys an index holds for `values`.
fn holding<'a>(
    index: &'a BTreeSet<(Key, Key)>,
    values: &Key,
) -> impl Iterator<Item = &'a Key> + use<'a> {
    let values = values.clone();
    index
        .range((values.clone(), Vec::new())..)
        .take_while(move |(held, _)| *held == values)
        .map(|(_, key)| key)
}

/// A row's entries in its table's maps: each index's values, and the key
/// each reference refers to.
fn entries(shape: &TableShape, row: &Row) -> Result<(Vec<Key>, Vec<Option<Key>>), Error> {
    let values = shape
        .indexes
        .iter()
        .map(|index| row_key(row, &index.fields))
        .collect::<Result<_, _>>()?;
    Ok((values, references(shape, row)?))
}

/// The key each of a row's references refers to; none where it holds none.
fn references(shape: &TableShape, row: &Row) -> Result<Vec<Option<Key>>, Error> {
    shape
        .references
        .iter()
        .map(|reference| match row.get(&reference.field) {
            None | Some(Value::Null) => Ok(None),
            Some(value) => Ok(Some(vec![Atom::parse(value)?])),
        })
        .collect()
}

// --- the adapter -----------------------------------------------------------

struct Inner {
    shape: Shape,
    /// The same manifest as the host reads it (`pw_host::tables`): its gate,
    /// and what the engine holds each function to.
    host: pw_host::tables::Manifest,
    /// What every read outside a command reads: the last commit's tables.
    published: Mutex<Arc<Vec<Arc<Data>>>>,
    /// The writer's copy: the published tables and the open command's
    /// writes, which only that command reads.
    working: Mutex<Working>,
    /// Told when a command ends, so that the next may begin.
    turn: Condvar,
    /// What is told when a command finds another open and waits its turn
    /// ([`MemoryTables::tell_on_wait`]).
    told: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
}

struct Working {
    tables: Vec<Data>,
    open: Option<Open>,
    next: u64,
}

/// The open command.
struct Open {
    id: u64,
    /// Held to its commit's checks, and writing nothing more.
    prepared: bool,
    /// Each key it wrote, by its table, and the row it held before: what
    /// undoes it, and what its commit publishes.
    touched: BTreeMap<(usize, Key), Option<Row>>,
}

/// **One isolated in-memory database.** Every read outside a command reads
/// the published tables; one command at a time writes ([`MemoryTables::begin`]),
/// and what it writes is published only when it commits.
pub struct MemoryTables {
    inner: Arc<Inner>,
}

impl MemoryTables {
    pub fn new(manifest: TablesJson) -> Result<Self, Error> {
        let schema = Schema::of(manifest.tables.iter().map(|t| t.schema.clone()).collect());
        if manifest.hash != schema.hash || manifest.migration.to != schema.hash {
            return Err(Error::Schema(
                "tables.json and its schema hash disagree".into(),
            ));
        }
        // D2 deliberately handles one source per database. Combining sources
        // requires the separate Sources routing/transaction design.
        let sources: BTreeSet<_> = manifest.tables.iter().map(|t| &t.schema.source).collect();
        if sources.len() > 1 {
            return Err(Error::Schema(
                "multiple sources require separate adapters".into(),
            ));
        }
        // Read back as the host reads it, so that what this adapter serves
        // and what the engine admits are one manifest, held whole.
        let text = serde_json::to_string(&manifest)
            .map_err(|error| Error::Schema(format!("tables.json: {error}")))?;
        let host = pw_host::tables::Manifest::from_json(&text).map_err(Error::Schema)?;
        let shape = shape_of(&manifest)?;
        let tables: Vec<Data> = shape.tables.iter().map(Data::new).collect();
        Ok(Self {
            inner: Arc::new(Inner {
                published: Mutex::new(Arc::new(tables.iter().cloned().map(Arc::new).collect())),
                working: Mutex::new(Working {
                    tables,
                    open: None,
                    next: 1,
                }),
                turn: Condvar::new(),
                told: Mutex::new(None),
                shape,
                host,
            }),
        })
    }

    /// The manifest as the host reads it: what a call that reads or writes
    /// these tables is given (`pw_host::engine::call_with_tables`).
    pub fn host_tables(&self) -> &pw_host::tables::Manifest {
        &self.inner.host
    }

    /// Read a compiler-emitted host import from the published tables, with
    /// its arguments in function parameter order. A singleton selector takes
    /// positional values; a set selector takes one array of scalars or
    /// named-field records; `all` takes none.
    pub fn read(&self, import: &str, args: &[Value]) -> Result<Value, Error> {
        self.inner.read_published(import, args)
    }

    /// Select the exact read imports this admitted component may bind as a
    /// query: the host's gate (`pw_host::tables::Manifest::admit_reads`,
    /// which the engine applies to every table import it links), on this
    /// adapter's manifest. Both the compiler's table manifest and the
    /// admitted contract must name each import; a node's other capabilities
    /// grant nothing here, and a write refuses the component.
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

    /// **The table reads an admitted query links, as host functions**
    /// (ADR-0338 D2): one callback for each read the host's gate admits
    /// ([`MemoryTables::read_host_imports`]), keyed as the engine links it
    /// (`pw:tables/<table>#get`), and nothing else. Each reads the published
    /// tables through the import it is installed for, its keys held to the
    /// read's and its rows encoded as the read's row declares
    /// (`pw_host::engine::table_read_host`); the engine links them only with
    /// this manifest (`call_with_tables` and [`MemoryTables::host_tables`]),
    /// and holds each answer to the read again. A component with any other
    /// table import, a write among them, gets an error, not a partial map.
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

    /// **A command's transaction** (ADR-0386): begun once the command before
    /// it has ended, one at a time. Its writes are staged until it commits
    /// ([`Command::commit`]), and undone whole where it is dropped instead.
    pub fn begin(&self) -> Result<Command, Error> {
        let mut working = self
            .inner
            .working
            .lock()
            .map_err(|_| Error::StoragePoisoned)?;
        if working.open.is_some()
            && let Ok(told) = self.inner.told.lock()
            && let Some(tell) = told.as_ref()
        {
            tell();
        }
        while working.open.is_some() {
            working = self
                .inner
                .turn
                .wait(working)
                .map_err(|_| Error::StoragePoisoned)?;
        }
        let id = working.next;
        working.next += 1;
        working.open = Some(Open {
            id,
            prepared: false,
            touched: BTreeMap::new(),
        });
        Ok(Command {
            inner: Arc::clone(&self.inner),
            id,
            ended: false,
        })
    }

    /// **Tell `tell` each time a command finds another open and waits its
    /// turn**, as it is about to wait (for tests and diagnostics, as the
    /// development server's write guard tells, ADR-0365: "waiting at its
    /// turn" is a fact the adapter can state, where "has not begun yet" is a
    /// silence a slow thread makes too).
    pub fn tell_on_wait(&self, tell: impl Fn() + Send + Sync + 'static) {
        if let Ok(mut told) = self.inner.told.lock() {
            *told = Some(Arc::new(tell));
        }
    }

    /// A serializable transaction in this memory database. A failed command,
    /// unique violation, missing reference or user error publishes nothing.
    pub fn transaction<T>(
        &self,
        action: impl FnOnce(&mut Command) -> Result<T, Error>,
    ) -> Result<T, Error> {
        let mut command = self.begin()?;
        let answer = action(&mut command)?;
        command.commit()?;
        Ok(answer)
    }
}

/// **One command's transaction** (ADR-0386): its reads see its own writes,
/// and no other read sees them until it commits. Dropped without committing,
/// it is undone whole. Its host functions ([`Command::host_fns`]) act on it
/// alone, and on nothing once it has ended.
pub struct Command {
    inner: Arc<Inner>,
    id: u64,
    ended: bool,
}

impl Command {
    /// Read through a compiler-emitted import, as [`MemoryTables::read`]
    /// does, from the tables as this command has written them.
    pub fn read(&self, import: &str, args: &[Value]) -> Result<Value, Error> {
        self.inner.command_read(self.id, import, args)
    }

    /// **Perform insert, put, delete or delete-prefix through a generated
    /// import**, staged in this command. A write refused by the row holding
    /// its key or a unique value (`Error::Conflict`, carrying that row) or
    /// failing changes nothing.
    pub fn write(&mut self, import: &str, args: &[Value]) -> Result<(), Error> {
        self.inner.command_write(self.id, import, args)
    }

    /// **The table reads and writes an admitted command links, as host
    /// functions**: one callback for each function the host's gate admits
    /// into a command (`pw_host::tables::Manifest::admit_command`), keyed as
    /// the engine links it, and nothing else. Each acts on this command: a
    /// read sees its writes, a write is staged in it, and a refused write
    /// answers the row that refused it (`result<_, row>`'s error), nothing
    /// written. Once the command has ended, each fails.
    pub fn host_fns(
        &self,
        contract: &pw_host::ComponentContract,
        granted: &pw_host::Granted,
    ) -> Result<BTreeMap<String, pw_host::engine::HostFn>, Error> {
        use pw_host::engine::{table_read_host, table_write_host};
        use pw_host::tables::Access;
        let mut out = BTreeMap::new();
        let admitted = self
            .inner
            .host
            .admit_command(contract, granted)
            .map_err(Error::Input)?;
        for (import, access) in admitted {
            let inner = Arc::clone(&self.inner);
            let (id, bound) = (self.id, import.clone());
            let host = match access {
                Access::Read(read) => table_read_host::host_fn(
                    read,
                    Arc::new(move |args: &[Value]| {
                        inner
                            .command_read(id, &bound, args)
                            .map_err(|error| format!("`{bound}`: {error:?}"))
                    }),
                ),
                Access::Write(write) => table_write_host::host_fn(
                    write,
                    Arc::new(
                        move |args: &[Value]| match inner.command_write(id, &bound, args) {
                            Ok(()) => Ok(None),
                            Err(Error::Conflict { existing, .. }) => Ok(Some(existing)),
                            Err(error) => Err(format!("`{bound}`: {error:?}")),
                        },
                    ),
                ),
            };
            out.insert(import, host);
        }
        Ok(out)
    }

    /// **The tables this command has written so far**, by name: what its
    /// commit publishes, and so what a host commits its outbox with
    /// (`Staged::rows`); none where it wrote nothing, a delete of a key no
    /// row has, or a row put back as it was, among them.
    pub fn written(&self) -> Result<BTreeSet<String>, Error> {
        let working = self.inner.working_of(self.id)?;
        let open = working.open.as_ref().ok_or(Error::Ended)?;
        Ok(changed(&working.tables, &open.touched)
            .iter()
            .map(|(table, _, _)| self.inner.shape.tables[*table].name.clone())
            .collect())
    }

    /// **Hold this command's writes to what a commit keeps** (ADR-0386):
    /// each written table within its bound, each reference written to a row
    /// that exists, and no row referring to one it deleted. Refused, the
    /// command is undone whole and nothing is published; held, it writes
    /// nothing more and is published by [`Prepared::publish`], which cannot
    /// fail. A unique index needs no check here: no write ever breaks one.
    pub fn prepare(mut self) -> Result<Prepared, Error> {
        let written = {
            let mut working = self.inner.working_of(self.id)?;
            let Working { tables, open, .. } = &mut *working;
            let open = open.as_mut().ok_or(Error::Ended)?;
            let changed = changed(tables, &open.touched);
            self.inner.held(tables, &changed)?;
            open.prepared = true;
            let mut written = BTreeMap::new();
            for (table, _, _) in &changed {
                written.insert(
                    self.inner.shape.tables[*table].name.clone(),
                    tables[*table].version + 1,
                );
            }
            written
        };
        let command = Command {
            inner: Arc::clone(&self.inner),
            id: self.id,
            ended: false,
        };
        // This handle hands its transaction to the prepared one.
        self.ended = true;
        Ok(Prepared { command, written })
    }

    /// [`Command::prepare`], then [`Prepared::publish`].
    pub fn commit(self) -> Result<Committed, Error> {
        Ok(self.prepare()?.publish())
    }
}

impl Drop for Command {
    fn drop(&mut self) {
        if !self.ended {
            self.inner.end(self.id, false);
        }
    }
}

/// **A command held to its commit's checks**: published by
/// [`Prepared::publish`], or undone whole where it is dropped instead.
pub struct Prepared {
    command: Command,
    written: BTreeMap<String, u64>,
}

impl Prepared {
    /// Each table its publish writes, by name, and the version it makes.
    pub fn written(&self) -> &BTreeMap<String, u64> {
        &self.written
    }

    /// **Publish what the command staged**: every read after it sees all of
    /// it, and none before saw any of it.
    pub fn publish(mut self) -> Committed {
        self.command.ended = true;
        self.command.inner.end(self.command.id, true);
        Committed {
            written: std::mem::take(&mut self.written),
        }
    }
}

/// What a commit published: each table it wrote, by name, at its new version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Committed {
    pub written: BTreeMap<String, u64>,
}

impl Inner {
    fn read_published(&self, import: &str, args: &[Value]) -> Result<Value, Error> {
        let tables = Arc::clone(&*self.published.lock().map_err(|_| Error::StoragePoisoned)?);
        read(&self.shape, &tables, import, args)
    }

    /// The working copy, where `id` is the open command.
    fn working_of(&self, id: u64) -> Result<MutexGuard<'_, Working>, Error> {
        let working = self.working.lock().map_err(|_| Error::StoragePoisoned)?;
        match &working.open {
            Some(open) if open.id == id => Ok(working),
            _ => Err(Error::Ended),
        }
    }

    fn command_read(&self, id: u64, import: &str, args: &[Value]) -> Result<Value, Error> {
        let working = self.working_of(id)?;
        read(&self.shape, &working.tables, import, args)
    }

    fn command_write(&self, id: u64, import: &str, args: &[Value]) -> Result<(), Error> {
        let mut working = self.working_of(id)?;
        let Working { tables, open, .. } = &mut *working;
        let open = open.as_mut().ok_or(Error::Ended)?;
        if open.prepared {
            return Err(Error::Ended);
        }
        write(&self.shape, tables, &mut open.touched, import, args)
    }

    /// **What a commit keeps** (see [`Command::prepare`]), of each row it
    /// changed.
    fn held(&self, tables: &[Data], changed: &[(usize, Key, Option<Row>)]) -> Result<(), Error> {
        let written: BTreeSet<usize> = changed.iter().map(|(table, _, _)| *table).collect();
        for &table in &written {
            let shape = &self.shape.tables[table];
            if let Some(bound) = shape.bounded
                && tables[table].rows.len() as u64 > bound
            {
                return Err(Error::Input(format!(
                    "{} exceeds its declared bound: {} rows, where it holds at most {bound}",
                    shape.name,
                    tables[table].rows.len()
                )));
            }
        }
        for (table, key, _) in changed {
            let shape = &self.shape.tables[*table];
            match tables[*table].rows.get(key) {
                // A row written: each row it refers to exists.
                Some(row) => {
                    for (reference, referred) in
                        shape.references.iter().zip(references(shape, row)?)
                    {
                        let Some(referred) = referred else { continue };
                        if !tables[reference.target].rows.contains_key(&referred) {
                            return Err(Error::Input(format!(
                                "{} references a missing {}",
                                shape.name, self.shape.tables[reference.target].name
                            )));
                        }
                    }
                }
                // A row deleted: no row refers to it.
                None => {
                    for (referring, other) in self.shape.tables.iter().enumerate() {
                        for (place, reference) in other.references.iter().enumerate() {
                            if reference.target == *table
                                && holding(&tables[referring].referring[place], key)
                                    .next()
                                    .is_some()
                            {
                                return Err(Error::Input(format!(
                                    "{} references a missing {}",
                                    other.name, shape.name
                                )));
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// **The open command `id` ends**: published, its touched keys' rows
    /// replayed into the published tables, or undone, its touched keys'
    /// rows restored in the working copy; then the next command may begin.
    /// Both remove every touched key first and then set each row it holds
    /// after, so that no unique value is held twice between the two.
    fn end(&self, id: u64, publish: bool) {
        let Ok(mut working) = self.working.lock() else {
            return;
        };
        let Working { tables, open, .. } = &mut *working;
        if !open.as_ref().is_some_and(|open| open.id == id) {
            return;
        }
        let Some(ended) = open.take() else {
            return;
        };
        if publish {
            // Each key the command changed, with its row as it left it.
            let rows = changed(tables, &ended.touched);
            let written: BTreeSet<usize> = rows.iter().map(|(table, _, _)| *table).collect();
            if let Ok(mut published) = self.published.lock() {
                let state = Arc::make_mut(&mut *published);
                replay(&self.shape, state, &rows);
                for &table in &written {
                    Arc::make_mut(&mut state[table]).version += 1;
                }
            }
            for &table in &written {
                tables[table].version += 1;
            }
        } else {
            // Each touched key's row before the command wrote it.
            let rows: Vec<(usize, Key, Option<Row>)> = ended
                .touched
                .into_iter()
                .map(|((table, key), prior)| (table, key, prior))
                .collect();
            replay(&self.shape, tables, &rows);
        }
        drop(working);
        self.turn.notify_one();
    }
}

/// **Each key a command changed**, with its row now: each it touched whose
/// row is not the one it held before.
fn changed(
    tables: &[Data],
    touched: &BTreeMap<(usize, Key), Option<Row>>,
) -> Vec<(usize, Key, Option<Row>)> {
    touched
        .iter()
        .filter_map(|((table, key), before)| {
            let now = tables[*table].rows.get(key).cloned();
            let unchanged = same_rows(&now, before);
            (!unchanged).then(|| (*table, key.clone(), now))
        })
        .collect()
}

/// Storage equality preserves floating-point representation. JSON numeric
/// equality merges +0.0 and -0.0, which must not leave staging and published
/// snapshots holding different bits after a successful command.
fn same_rows(left: &Option<Row>, right: &Option<Row>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => same_fields(left, right),
        (None, None) => true,
        _ => false,
    }
}

fn same_fields(left: &Map<String, Value>, right: &Map<String, Value>) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .all(|(key, value)| right.get(key).is_some_and(|other| same_value(value, other)))
}

fn same_value(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Number(left), Value::Number(right)) if left.is_f64() || right.is_f64() => {
            left.is_f64() == right.is_f64()
                && left.as_f64().map(f64::to_bits) == right.as_f64().map(f64::to_bits)
        }
        (Value::Array(left), Value::Array(right)) => {
            left.len() == right.len() && left.iter().zip(right).all(|(a, b)| same_value(a, b))
        }
        (Value::Object(left), Value::Object(right)) => same_fields(left, right),
        _ => left == right,
    }
}

/// Tables a replay sets rows in: the working copy's, or the published ones,
/// each copied first where a reader still holds it.
trait Tables {
    fn table(&mut self, at: usize) -> &mut Data;
}

impl Tables for Vec<Data> {
    fn table(&mut self, at: usize) -> &mut Data {
        &mut self[at]
    }
}

impl Tables for Vec<Arc<Data>> {
    fn table(&mut self, at: usize) -> &mut Data {
        Arc::make_mut(&mut self[at])
    }
}

/// Every key of `rows` removed, then set to the row it gives. The rows are
/// a state every write already held to its unique indexes, so no set is
/// refused; one that were would be a broken adapter, which stops here
/// rather than leave half a commit or half an undo.
fn replay(shape: &Shape, tables: &mut impl Tables, rows: &[(usize, Key, Option<Row>)]) {
    for (t, key, _) in rows {
        let done = tables.table(*t).set(&shape.tables[*t], key, None);
        assert!(
            matches!(done, Ok(Set::Done(_))),
            "a removal is never refused"
        );
    }
    for (t, key, row) in rows {
        if let Some(row) = row {
            let done = tables
                .table(*t)
                .set(&shape.tables[*t], key, Some(row.clone()));
            assert!(
                matches!(done, Ok(Set::Done(_))),
                "a state every write held to its unique indexes is set whole"
            );
        }
    }
}

// --- reading the manifest ----------------------------------------------------

fn shape_of(manifest: &TablesJson) -> Result<Shape, Error> {
    let mut tables = Vec::new();
    let mut functions = BTreeMap::new();
    let place = |name: &str| manifest.tables.iter().position(|t| t.schema.name == name);
    for (at, table) in manifest.tables.iter().enumerate() {
        let name = &table.schema.name;
        if place(name) != Some(at) {
            return Err(Error::Schema(format!("duplicate table {name}")));
        }
        if table.schema.key.is_empty() {
            return Err(Error::Schema(format!("{name} has no key")));
        }
        let fields: BTreeSet<_> = table
            .schema
            .fields
            .iter()
            .map(|f| f.name.as_str())
            .collect();
        if fields.len() != table.schema.fields.len() {
            return Err(Error::Schema(format!("{name} repeats a field")));
        }
        for selector in table
            .schema
            .key
            .iter()
            .chain(table.schema.order.iter())
            .chain(table.schema.indexes.iter().flat_map(|i| &i.fields))
        {
            let field = stored(table, selector)
                .ok_or_else(|| Error::Schema(format!("{name} has unknown field {selector}")))?;
            if !scalar(field) {
                return Err(Error::Schema(format!(
                    "{name} selector {selector} is not a supported scalar"
                )));
            }
        }
        for f in &table.schema.fields {
            if !supported(&f.stored) {
                return Err(Error::Schema(format!(
                    "{name} field {} needs an unsupported storage encoding",
                    f.name
                )));
            }
        }
        let mut references = Vec::new();
        for reference in &table.schema.references {
            let target = place(&reference.table).ok_or_else(|| {
                Error::Schema(format!("unknown referenced table {}", reference.table))
            })?;
            let referred = &manifest.tables[target].schema;
            if referred.source != table.schema.source || referred.key.len() != 1 {
                return Err(Error::Schema(
                    "cross-source or composite references are unsupported in memory D2".into(),
                ));
            }
            let field = stored(table, &reference.field).ok_or_else(|| {
                Error::Schema(format!(
                    "{name} refers by unknown field {}",
                    reference.field
                ))
            })?;
            let held = match field {
                Stored::Option(inner) => inner.as_ref(),
                other => other,
            };
            if !scalar(held) {
                return Err(Error::Schema(format!(
                    "{name} refers by {}, which holds no key",
                    reference.field
                )));
            }
            references.push(ReferenceShape {
                field: reference.field.clone(),
                target,
            });
        }
        for function in &table.functions {
            if function.primitive == Primitive::All && table.bounded.is_none() {
                return Err(Error::Schema(format!("{name} grants an unbounded all()")));
            }
            if !matches!(
                function.primitive,
                Primitive::All | Primitive::Insert | Primitive::Put
            ) && function.by.is_empty()
            {
                return Err(Error::Schema(format!(
                    "{} has an empty selector",
                    function.import
                )));
            }
            if function
                .by
                .iter()
                .any(|field| !fields.contains(field.as_str()))
            {
                return Err(Error::Schema(format!(
                    "{} selects a missing field",
                    function.import
                )));
            }
            let one = function.primitive == Primitive::Get
                || (function.primitive == Primitive::Index
                    && table
                        .schema
                        .indexes
                        .iter()
                        .any(|i| i.unique && i.fields == function.by));
            let shaped = FunctionShape {
                primitive: function.primitive,
                by: function.by.clone(),
                sorted_by: function.sorted_by.clone(),
                one,
            };
            if functions
                .insert(function.import.clone(), (at, shaped))
                .is_some()
            {
                return Err(Error::Schema(format!(
                    "duplicate host import {}",
                    function.import
                )));
            }
        }
        tables.push(TableShape {
            name: name.clone(),
            fields: table
                .schema
                .fields
                .iter()
                .map(|f| (f.name.clone(), f.stored.clone()))
                .collect(),
            key: table.schema.key.clone(),
            indexes: table
                .schema
                .indexes
                .iter()
                .map(|index| IndexShape {
                    fields: index.fields.clone(),
                    unique: index.unique,
                })
                .collect(),
            references,
            bounded: table.bounded,
        });
    }
    Ok(Shape { tables, functions })
}

fn stored<'a>(table: &'a TableJson, field: &str) -> Option<&'a Stored> {
    table
        .schema
        .fields
        .iter()
        .find(|f| f.name == field)
        .map(|f| &f.stored)
}

/// A value every adapter keys by.
fn scalar(stored: &Stored) -> bool {
    matches!(
        stored,
        Stored::Int { .. } | Stored::String { .. } | Stored::Bool
    )
}

/// A value this adapter stores without losing any of it: an `Option` of an
/// `Option` is not, since its `Some(None)` is stored as its `None`.
fn supported(ty: &Stored) -> bool {
    match ty {
        Stored::Int { .. }
        | Stored::String { .. }
        | Stored::Bool
        | Stored::Float
        | Stored::Unit => true,
        Stored::Option(inner) => !matches!(**inner, Stored::Option(_)) && supported(inner),
        Stored::List(inner) => supported(inner),
        Stored::Record(fields) => fields.iter().all(|f| supported(&f.stored)),
        Stored::Map(_, _) | Stored::Set(_) | Stored::Cases(_) | Stored::Recursive(_) => false,
    }
}

fn validates(value: &Value, ty: &Stored) -> bool {
    match ty {
        Stored::Int { at_least, at_most } => value.as_i64().is_some_and(|n| {
            at_least.is_none_or(|min| n >= min) && at_most.is_none_or(|max| n <= max)
        }),
        Stored::String {
            min_length,
            max_length,
        } => value.as_str().is_some_and(|s| {
            let n = i64::try_from(s.chars().count()).unwrap_or(i64::MAX);
            min_length.is_none_or(|min| n >= min) && max_length.is_none_or(|max| n <= max)
        }),
        Stored::Bool => value.is_boolean(),
        Stored::Float => value.as_f64().is_some_and(f64::is_finite),
        Stored::Unit => value.is_null(),
        Stored::Option(inner) => value.is_null() || validates(value, inner),
        Stored::List(inner) => value
            .as_array()
            .is_some_and(|xs| xs.iter().all(|v| validates(v, inner))),
        Stored::Record(fields) => value.as_object().is_some_and(|obj| {
            obj.len() == fields.len()
                && fields
                    .iter()
                    .all(|f| obj.get(&f.name).is_some_and(|v| validates(v, &f.stored)))
        }),
        Stored::Set(_) | Stored::Map(_, _) | Stored::Cases(_) | Stored::Recursive(_) => false,
    }
}

fn check_row(table: &TableShape, value: &Value) -> Result<(Key, Row), Error> {
    let row = value
        .as_object()
        .ok_or_else(|| Error::Input("row must be an object".into()))?;
    if row.len() != table.fields.len() {
        return Err(Error::Input(format!(
            "{} row fields do not match its schema",
            table.name
        )));
    }
    for (name, stored) in &table.fields {
        let v = row
            .get(name)
            .ok_or_else(|| Error::Input(format!("{} row is missing {name}", table.name)))?;
        if !validates(v, stored) {
            return Err(Error::Input(format!(
                "{} field {name} fails its stored type or bound",
                table.name
            )));
        }
    }
    let key = row_key(row, &table.key)?;
    Ok((key, Arc::new(row.clone())))
}

fn row_key(row: &Map<String, Value>, fields: &[String]) -> Result<Key, Error> {
    fields
        .iter()
        .map(|f| {
            Atom::parse(
                row.get(f)
                    .ok_or_else(|| Error::Input(format!("row is missing selector {f}")))?,
            )
        })
        .collect()
}

fn selector(by: &[String], args: &[Value]) -> Result<Key, Error> {
    if args.len() != by.len() {
        return Err(Error::Input(format!(
            "expected {} selector values, got {}",
            by.len(),
            args.len()
        )));
    }
    args.iter().map(Atom::parse).collect()
}

fn selections(by: &[String], args: &[Value], many: bool) -> Result<BTreeSet<Key>, Error> {
    if !many {
        return Ok(BTreeSet::from([selector(by, args)?]));
    }
    let [Value::Array(values)] = args else {
        return Err(Error::Input("a set selector needs one array".into()));
    };
    let mut out = BTreeSet::new();
    for value in values {
        let key = if by.len() == 1 {
            vec![Atom::parse(value)?]
        } else {
            let obj = value
                .as_object()
                .ok_or_else(|| Error::Input("composite set selectors are records".into()))?;
            if obj.len() != by.len() {
                return Err(Error::Input(
                    "composite set selector fields do not match".into(),
                ));
            }
            by.iter()
                .map(|f| {
                    Atom::parse(
                        obj.get(f)
                            .ok_or_else(|| Error::Input(format!("set selector lacks {f}")))?,
                    )
                })
                .collect::<Result<Key, _>>()?
        };
        out.insert(key);
    }
    Ok(out)
}

/// What a function is given, each value held to its declared field's type
/// and bound: a boolean is a valid atom where a string is declared, and a
/// mismatched read must fail, not turn into the same answer as an absent
/// row.
fn typed_selections(
    table: &TableShape,
    function: &FunctionShape,
    import: &str,
    args: &[Value],
) -> Result<BTreeSet<Key>, Error> {
    let keys = selections(&function.by, args, function.primitive.many())?;
    for key in &keys {
        for (name, atom) in function.by.iter().zip(key) {
            let (_, stored) = table
                .fields
                .iter()
                .find(|(field, _)| field == name)
                .ok_or_else(|| Error::Schema(format!("unknown table selector field {name}")))?;
            let value = match atom {
                Atom::Bool(value) => Value::from(*value),
                Atom::Int(value) => Value::from(*value),
                Atom::Str(value) => Value::from(value.clone()),
            };
            if !validates(&value, stored) {
                return Err(Error::Input(format!(
                    "{import} selector {name} fails its declared type or bound"
                )));
            }
        }
    }
    Ok(keys)
}

/// The one key a keyed function is given.
fn typed_selector(
    table: &TableShape,
    function: &FunctionShape,
    import: &str,
    args: &[Value],
) -> Result<Key, Error> {
    typed_selections(table, function, import, args)?
        .into_iter()
        .next()
        .ok_or_else(|| Error::Input(format!("{import} was given no selector")))
}

fn read(
    shape: &Shape,
    tables: &[impl AsRef<Data>],
    import: &str,
    args: &[Value],
) -> Result<Value, Error> {
    let (at, function) = shape.function(import)?;
    if function.primitive.writes() {
        return Err(Error::Input(format!("{import} is a write, not a read")));
    }
    let table = &shape.tables[at];
    let data = tables
        .get(at)
        .map(AsRef::as_ref)
        .ok_or_else(|| Error::Schema("missing table state".into()))?;
    let mut found: Vec<&Row> = Vec::new();
    match function.primitive {
        Primitive::All => {
            if !args.is_empty() {
                return Err(Error::Input("all() takes no selector".into()));
            }
            let Some(bound) = table.bounded else {
                return Err(Error::Schema("whole-table read without a bound".into()));
            };
            // Only a command's own writes, before it commits, can hold more.
            if data.rows.len() as u64 > bound {
                return Err(Error::Input(format!(
                    "{} holds {} rows, more than its bound of {bound}",
                    table.name,
                    data.rows.len()
                )));
            }
            found.extend(data.rows.values());
        }
        Primitive::Get | Primitive::GetMany => {
            for key in typed_selections(table, function, import, args)? {
                found.extend(data.rows.get(&key));
            }
        }
        Primitive::Prefix | Primitive::PrefixMany => {
            for prefix in typed_selections(table, function, import, args)? {
                found.extend(data.prefixed(&prefix).map(|(_, row)| row));
            }
        }
        Primitive::Index | Primitive::IndexMany => {
            let index = table
                .indexes
                .iter()
                .position(|index| index.fields == function.by)
                .ok_or_else(|| Error::Schema(format!("{import} reads by no index")))?;
            for values in typed_selections(table, function, import, args)? {
                for key in holding(&data.indexes[index], &values) {
                    found.push(data.rows.get(key).ok_or_else(|| {
                        Error::Schema(format!("{}'s index names a missing row", table.name))
                    })?);
                }
            }
        }
        _ => return Err(Error::Input(format!("{import} is a write, not a read"))),
    }
    if function.one {
        return Ok(found
            .first()
            .map_or(Value::Null, |row| Value::Object((***row).clone())));
    }
    found.sort_by_cached_key(|row| place(row, &function.sorted_by, &table.key));
    Ok(Value::Array(
        found
            .into_iter()
            .map(|row| Value::Object((**row).clone()))
            .collect(),
    ))
}

/// A row's place in tables.json's order: its `sorted_by` fields, then its
/// key.
fn place(row: &Row, sorted_by: &[String], key: &[String]) -> Vec<Option<Atom>> {
    sorted_by
        .iter()
        .chain(key)
        .map(|field| row.get(field).and_then(|v| Atom::parse(v).ok()))
        .collect()
}

fn write(
    shape: &Shape,
    tables: &mut [Data],
    touched: &mut BTreeMap<(usize, Key), Option<Row>>,
    import: &str,
    args: &[Value],
) -> Result<(), Error> {
    let (at, function) = shape.function(import)?;
    let table = &shape.tables[at];
    let data = tables
        .get_mut(at)
        .ok_or_else(|| Error::Schema("missing table state".into()))?;
    let conflict = |held: &Row| Error::Conflict {
        table: table.name.clone(),
        existing: Value::Object((**held).clone()),
    };
    // Each key's row before this command first wrote it.
    let mut first = |key: &Key, prior: Option<Row>| {
        touched.entry((at, key.clone())).or_insert(prior);
    };
    match function.primitive {
        Primitive::Insert | Primitive::Put => {
            let [value] = args else {
                return Err(Error::Input("insert/put expects one row".into()));
            };
            let (key, row) = check_row(table, value)?;
            if function.primitive == Primitive::Insert
                && let Some(existing) = data.rows.get(&key)
            {
                return Err(conflict(existing));
            }
            match data.set(table, &key, Some(row))? {
                Set::Held(held) => return Err(conflict(&held)),
                Set::Done(prior) => first(&key, prior),
            }
        }
        Primitive::Delete => {
            let key = typed_selector(table, function, import, args)?;
            if data.rows.contains_key(&key)
                && let Set::Done(prior) = data.set(table, &key, None)?
            {
                first(&key, prior);
            }
        }
        Primitive::DeletePrefix => {
            let prefix = typed_selector(table, function, import, args)?;
            let keys: Vec<Key> = data.prefixed(&prefix).map(|(key, _)| key.clone()).collect();
            for key in keys {
                if let Set::Done(prior) = data.set(table, &key, None)? {
                    first(&key, prior);
                }
            }
        }
        _ => return Err(Error::Input(format!("{import} is not a table write"))),
    }
    Ok(())
}

impl AsRef<Data> for Data {
    fn as_ref(&self) -> &Data {
        self
    }
}
