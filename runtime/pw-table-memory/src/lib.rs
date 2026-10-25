//! Generic in-memory execution of compiler-declared Pleris tables (ADR-0338 D2).
//!
//! It consumes the compiler's tables.json types, not a second schema format.
//! It proves key/prefix/index reads and staged, atomic writes in one memory
//! source, and serves an admitted component's `get` and `get-many` imports as
//! Wasmtime host functions (`read_host_fns`). Writes through the host, the
//! development server's routing of a program's tables, the transactional
//! outbox, deployment migration and other databases remain independent
//! integration obligations; no production claim follows.
//!
//! JSON is the internal adapter value representation, not the public WIT ABI.
//! Unsupported storage variants fail closed rather than losing information.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use pw_core::tables::{FunctionJson, Primitive, Schema, Stored, TableJson, TablesJson};
use serde_json::{Map, Value};

#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    Schema(String),
    Input(String),
    Conflict { table: String, existing: Value },
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
type Row = Map<String, Value>;
type Rows = BTreeMap<Key, Row>;
type State = BTreeMap<String, Rows>;

/// One isolated in-memory database. Every transaction holds the lock until
/// commit/rollback, and the state is only published on a successful commit.
pub struct MemoryTables {
    manifest: TablesJson,
    state: Mutex<State>,
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
        let mut state = BTreeMap::new();
        let mut imports = BTreeSet::new();
        for table in &manifest.tables {
            if state
                .insert(table.schema.name.clone(), Rows::new())
                .is_some()
            {
                return Err(Error::Schema(format!(
                    "duplicate table {}",
                    table.schema.name
                )));
            }
            if table.schema.key.is_empty() {
                return Err(Error::Schema(format!("{} has no key", table.schema.name)));
            }
            let fields: BTreeSet<_> = table
                .schema
                .fields
                .iter()
                .map(|f| f.name.as_str())
                .collect();
            if fields.len() != table.schema.fields.len() {
                return Err(Error::Schema(format!(
                    "{} repeats a field",
                    table.schema.name
                )));
            }
            for name in table
                .schema
                .key
                .iter()
                .chain(table.schema.order.iter())
                .chain(table.schema.indexes.iter().flat_map(|i| &i.fields))
            {
                let field = table
                    .schema
                    .fields
                    .iter()
                    .find(|f| &f.name == name)
                    .ok_or_else(|| {
                        Error::Schema(format!("{} has unknown field {name}", table.schema.name))
                    })?;
                if !matches!(
                    &field.stored,
                    Stored::Int { .. } | Stored::String { .. } | Stored::Bool
                ) {
                    return Err(Error::Schema(format!(
                        "{} selector {name} is not a supported scalar",
                        table.schema.name
                    )));
                }
            }
            for f in &table.schema.fields {
                if !supported(&f.stored) {
                    return Err(Error::Schema(format!(
                        "{} field {} needs an unsupported storage encoding",
                        table.schema.name, f.name
                    )));
                }
            }
            for fun in &table.functions {
                if !imports.insert(fun.import.as_str()) {
                    return Err(Error::Schema(format!(
                        "duplicate host import {}",
                        fun.import
                    )));
                }
                if fun.primitive == Primitive::All && table.bounded.is_none() {
                    return Err(Error::Schema(format!(
                        "{} grants an unbounded all()",
                        table.schema.name
                    )));
                }
                let selector_fields = &fun.by;
                if !matches!(
                    fun.primitive,
                    Primitive::All | Primitive::Insert | Primitive::Put
                ) && selector_fields.is_empty()
                {
                    return Err(Error::Schema(format!(
                        "{} has an empty selector",
                        fun.import
                    )));
                }
                if selector_fields
                    .iter()
                    .any(|field| !fields.contains(field.as_str()))
                {
                    return Err(Error::Schema(format!(
                        "{} selects a missing field",
                        fun.import
                    )));
                }
            }
        }
        Ok(Self {
            manifest,
            state: Mutex::new(state),
        })
    }

    /// Read a compiler-emitted host import, with its arguments in function
    /// parameter order. A singleton selector takes positional values; a set
    /// selector takes one array of scalars or named-field records.
    pub fn read(&self, import: &str, args: &[Value]) -> Result<Value, Error> {
        let state = self.state.lock().map_err(|_| Error::StoragePoisoned)?;
        read(&self.manifest, &state, import, args)
    }

    /// Select the exact read-only table imports this admitted component may
    /// bind. Both the compiler's table manifest and the admitted contract must
    /// name each import; a node's other capabilities grant nothing here.
    ///
    /// This is a pre-link gate, not a WIT value codec or Wasmtime execution.
    /// A later host binding must use only these keys when installing HostFns.
    pub fn read_host_imports(
        &self,
        contract: &pw_host::ComponentContract,
        granted: &pw_host::Granted,
    ) -> Result<Vec<String>, Error> {
        let linkable: BTreeSet<String> = pw_host::linkable(contract, granted).into_iter().collect();
        let manifest_reads: BTreeMap<&str, String> = self
            .manifest
            .tables
            .iter()
            .flat_map(|table| {
                table.functions.iter().filter_map(|function| {
                    let operation = match function.primitive {
                        Primitive::Get => "get",
                        Primitive::GetMany => "get-many",
                        _ => return None,
                    };
                    (function.effect == "read"
                        && function.import == format!("{}#{operation}", table.interface))
                    .then(|| {
                        (
                            function.import.as_str(),
                            format!("database.read<{}>", table.schema.name),
                        )
                    })
                })
            })
            .collect();

        let mut selected = BTreeSet::new();
        for import in contract
            .imports
            .iter()
            .filter(|import| import.interface.starts_with("pw:tables/"))
        {
            let key = import.key();
            if import.kind != pw_host::ImportKind::HostCapability
                || manifest_reads.get(key.as_str()).map(String::as_str)
                    != Some(import.capability.as_str())
                || !linkable.contains(&key)
            {
                return Err(Error::Input(format!(
                    "read-only table host cannot bind `{key}`"
                )));
            }
            selected.insert(key);
        }
        Ok(selected.into_iter().collect())
    }

    /// **The table reads an admitted component links, as host functions**
    /// (ADR-0338 D2): one callback for each import
    /// [`MemoryTables::read_host_imports`] selects, keyed as the engine links
    /// it (`pw:tables/<table>#get`), and nothing else. Each reads this
    /// adapter through the import it is installed for, its arguments decoded
    /// and its rows encoded by `pw_host::engine::table_read_host`; the engine
    /// then reads each answer through the type the component declares
    /// (ADR-0166), so a row reaches the component as its WIT record or not at
    /// all. A component with any other table import gets an error, not a
    /// partial map.
    pub fn read_host_fns(
        self: &Arc<Self>,
        contract: &pw_host::ComponentContract,
        granted: &pw_host::Granted,
    ) -> Result<BTreeMap<String, pw_host::engine::HostFn>, Error> {
        use pw_host::engine::table_read_host::{ReadKind, host_fn};
        let mut out = BTreeMap::new();
        for import in self.read_host_imports(contract, granted)? {
            let (_, function) = operation(&self.manifest, &import)?;
            let kind = match function.primitive {
                Primitive::Get => ReadKind::Get,
                Primitive::GetMany => ReadKind::GetMany,
                _ => {
                    return Err(Error::Input(format!(
                        "read-only table host cannot bind `{import}`"
                    )));
                }
            };
            let tables = Arc::clone(self);
            let bound = import.clone();
            let read = Arc::new(move |args: &[Value]| {
                tables
                    .read(&bound, args)
                    .map_err(|error| format!("`{bound}`: {error:?}"))
            });
            out.insert(import, host_fn(kind, read));
        }
        Ok(out)
    }

    /// A serializable transaction in this memory database. A failed command,
    /// unique violation, missing reference or user error publishes nothing.
    pub fn transaction<T>(
        &self,
        action: impl FnOnce(&mut Transaction<'_>) -> Result<T, Error>,
    ) -> Result<T, Error> {
        let mut guard = self.state.lock().map_err(|_| Error::StoragePoisoned)?;
        let mut staged = guard.clone();
        let answer = {
            let mut tx = Transaction {
                manifest: &self.manifest,
                state: &mut staged,
            };
            let answer = action(&mut tx)?;
            validate_state(&self.manifest, &*tx.state)?;
            answer
        };
        *guard = staged;
        Ok(answer)
    }
}

pub struct Transaction<'a> {
    manifest: &'a TablesJson,
    state: &'a mut State,
}

impl Transaction<'_> {
    pub fn read(&self, import: &str, args: &[Value]) -> Result<Value, Error> {
        read(self.manifest, self.state, import, args)
    }

    /// Perform insert, put, delete or delete-prefix through a generated
    /// import. Errors restore the pre-call state even if caught by the caller.
    /// Conflict carries the existing row, for the future WIT Result mapping.
    pub fn write(&mut self, import: &str, args: &[Value]) -> Result<(), Error> {
        let before = self.state.clone();
        let result = write(self.manifest, self.state, import, args)
            .and_then(|()| validate_uniqueness(self.manifest, self.state));
        if result.is_err() {
            *self.state = before;
        }
        result
    }
}

fn operation<'a>(
    manifest: &'a TablesJson,
    import: &str,
) -> Result<(&'a TableJson, &'a FunctionJson), Error> {
    manifest
        .tables
        .iter()
        .find_map(|t| {
            t.functions
                .iter()
                .find(|f| f.import == import)
                .map(|f| (t, f))
        })
        .ok_or_else(|| Error::Input(format!("undeclared table operation {import}")))
}

fn supported(ty: &Stored) -> bool {
    match ty {
        Stored::Int { .. }
        | Stored::String { .. }
        | Stored::Bool
        | Stored::Float
        | Stored::Unit => true,
        Stored::Option(inner) | Stored::List(inner) => supported(inner),
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

fn check_row(table: &TableJson, value: &Value) -> Result<(Key, Row), Error> {
    let row = value
        .as_object()
        .ok_or_else(|| Error::Input("row must be an object".into()))?;
    if row.len() != table.schema.fields.len() {
        return Err(Error::Input(format!(
            "{} row fields do not match its schema",
            table.schema.name
        )));
    }
    for f in &table.schema.fields {
        let v = row.get(&f.name).ok_or_else(|| {
            Error::Input(format!("{} row is missing {}", table.schema.name, f.name))
        })?;
        if !validates(v, &f.stored) {
            return Err(Error::Input(format!(
                "{} field {} fails its stored type or bound",
                table.schema.name, f.name
            )));
        }
    }
    let key = row_key(row, &table.schema.key)?;
    Ok((key, row.clone()))
}

fn row_key(row: &Row, fields: &[String]) -> Result<Key, Error> {
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

fn typed_read_selections(
    table: &TableJson,
    fun: &FunctionJson,
    args: &[Value],
) -> Result<BTreeSet<Key>, Error> {
    let keys = selections(&fun.by, args, fun.primitive.many())?;
    // Atom::parse checks the value's shape, but a boolean is still a valid
    // Atom when the declared selector is a string. A mismatched read must
    // fail, not turn into the same answer as an absent row.
    for key in &keys {
        for (name, atom) in fun.by.iter().zip(key) {
            let field = table
                .schema
                .fields
                .iter()
                .find(|field| field.name == *name)
                .ok_or_else(|| Error::Schema(format!("unknown table selector field {name}")))?;
            let value = match atom {
                Atom::Bool(value) => Value::from(*value),
                Atom::Int(value) => Value::from(*value),
                Atom::Str(value) => Value::from(value.clone()),
            };
            if !validates(&value, &field.stored) {
                return Err(Error::Input(format!(
                    "{} selector {name} fails its declared type or bound",
                    fun.import
                )));
            }
        }
    }
    Ok(keys)
}

fn read(
    manifest: &TablesJson,
    state: &State,
    import: &str,
    args: &[Value],
) -> Result<Value, Error> {
    let (table, fun) = operation(manifest, import)?;
    if fun.primitive.writes() {
        return Err(Error::Input(format!("{import} is a write, not a read")));
    }
    let rows = state
        .get(&table.schema.name)
        .ok_or_else(|| Error::Schema("missing table state".into()))?;
    if fun.primitive == Primitive::All && table.bounded.is_none() {
        return Err(Error::Schema("whole-table read without a bound".into()));
    }
    if fun.primitive == Primitive::All && !args.is_empty() {
        return Err(Error::Input("all() takes no selector".into()));
    }
    let keys = if fun.primitive == Primitive::All {
        BTreeSet::new()
    } else {
        typed_read_selections(table, fun, args)?
    };
    let mut found: Vec<&Row> = rows
        .values()
        .filter(|row| {
            fun.primitive == Primitive::All
                || row_key(row, &fun.by).is_ok_and(|key| keys.contains(&key))
        })
        .collect();
    found.sort_by(|a, b| compare_rows(a, b, &fun.sorted_by, &table.schema.key));
    let one = fun.primitive == Primitive::Get
        || (fun.primitive == Primitive::Index
            && table
                .schema
                .indexes
                .iter()
                .any(|i| i.unique && i.fields == fun.by));
    if one {
        Ok(found
            .first()
            .map_or(Value::Null, |row| Value::Object((*row).clone())))
    } else {
        Ok(Value::Array(
            found
                .into_iter()
                .map(|row| Value::Object(row.clone()))
                .collect(),
        ))
    }
}

fn compare_rows(a: &Row, b: &Row, by: &[String], key: &[String]) -> Ordering {
    for field in by.iter().chain(key.iter()) {
        let left = a.get(field).and_then(|v| Atom::parse(v).ok());
        let right = b.get(field).and_then(|v| Atom::parse(v).ok());
        let comparison = left.cmp(&right);
        if comparison != Ordering::Equal {
            return comparison;
        }
    }
    Ordering::Equal
}

fn write(
    manifest: &TablesJson,
    state: &mut State,
    import: &str,
    args: &[Value],
) -> Result<(), Error> {
    let (table, fun) = operation(manifest, import)?;
    let rows = state
        .get_mut(&table.schema.name)
        .ok_or_else(|| Error::Schema("missing table state".into()))?;
    match fun.primitive {
        Primitive::Insert | Primitive::Put => {
            let [value] = args else {
                return Err(Error::Input("insert/put expects one row".into()));
            };
            let (key, row) = check_row(table, value)?;
            if fun.primitive == Primitive::Insert
                && let Some(existing) = rows.get(&key)
            {
                return Err(Error::Conflict {
                    table: table.schema.name.clone(),
                    existing: Value::Object(existing.clone()),
                });
            }
            rows.insert(key, row);
        }
        Primitive::Delete | Primitive::DeletePrefix => {
            let key = selector(&fun.by, args)?;
            if fun.primitive == Primitive::Delete {
                rows.remove(&key);
            } else {
                rows.retain(|_, row| row_key(row, &fun.by).map_or(true, |found| found != key));
            }
        }
        _ => return Err(Error::Input(format!("{import} is not a table write"))),
    }
    Ok(())
}

fn validate_uniqueness(manifest: &TablesJson, state: &State) -> Result<(), Error> {
    for table in &manifest.tables {
        let rows = state
            .get(&table.schema.name)
            .ok_or_else(|| Error::Schema("missing table state".into()))?;
        if table.bounded.is_some_and(|bound| rows.len() as u64 > bound) {
            return Err(Error::Input(format!(
                "{} exceeds its declared bound",
                table.schema.name
            )));
        }
        for index in table.schema.indexes.iter().filter(|i| i.unique) {
            let mut seen = BTreeMap::<Key, &Row>::new();
            for row in rows.values() {
                let key = row_key(row, &index.fields)?;
                if let Some(existing) = seen.insert(key, row) {
                    return Err(Error::Conflict {
                        table: table.schema.name.clone(),
                        existing: Value::Object(existing.clone()),
                    });
                }
            }
        }
    }
    Ok(())
}

fn validate_state(manifest: &TablesJson, state: &State) -> Result<(), Error> {
    validate_uniqueness(manifest, state)?;
    for table in &manifest.tables {
        let rows = state
            .get(&table.schema.name)
            .ok_or_else(|| Error::Schema("missing table state".into()))?;
        for reference in &table.schema.references {
            let target = manifest
                .tables
                .iter()
                .find(|t| t.schema.name == reference.table)
                .ok_or_else(|| {
                    Error::Schema(format!("unknown referenced table {}", reference.table))
                })?;
            if target.schema.source != table.schema.source || target.schema.key.len() != 1 {
                return Err(Error::Schema(
                    "cross-source or composite references are unsupported in memory D2".into(),
                ));
            }
            let target_rows = state
                .get(&target.schema.name)
                .ok_or_else(|| Error::Schema("missing target state".into()))?;
            for row in rows.values() {
                let value = row
                    .get(&reference.field)
                    .ok_or_else(|| Error::Schema("missing reference field".into()))?;
                if value.is_null() {
                    continue;
                }
                if !target_rows.contains_key(&vec![Atom::parse(value)?]) {
                    return Err(Error::Input(format!(
                        "{} references a missing {}",
                        table.schema.name, target.schema.name
                    )));
                }
            }
        }
    }
    Ok(())
}
