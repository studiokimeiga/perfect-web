//! **What `tables.json` declares, read once and held whole**, as the memory
//! adapter reads it (`pw-table-memory`'s `shape_of`): the same refusals, the
//! same domain of values, the same selectors. What this adds is where each
//! field lives in PostgreSQL: its column's kind ([`Kind`]).
//!
//! Every check a value meets here is the memory adapter's, word for word
//! where it answers an error, so that the two adapters refuse alike: the
//! conformance tests (`tests/parity.rs`) hold them to it.

use std::collections::{BTreeMap, BTreeSet};

use pw_core::tables::{Primitive, Stored, TableJson, TablesJson};
use serde_json::{Map, Value};

use crate::Error;

/// A value every adapter keys by: a key's, a prefix's or an index's field.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum Atom {
    Bool(bool),
    Int(i64),
    Str(String),
}

impl Atom {
    pub(crate) fn parse(value: &Value) -> Result<Self, Error> {
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

    pub(crate) fn value(&self) -> Value {
        match self {
            Atom::Bool(v) => Value::from(*v),
            Atom::Int(v) => Value::from(*v),
            Atom::Str(v) => Value::from(v.clone()),
        }
    }
}

pub(crate) type Key = Vec<Atom>;

/// **How a column holds its field's value** (ADR-0338 5.2, as this adapter
/// decides it): `bigint`, `bytea` (a `String`'s UTF-8 bytes: every string,
/// U+0000 among them, which `text` refuses, ordered and compared byte by byte
/// as Rust's `String` is, whatever the database's collation), `boolean`,
/// `double precision`, and `json` for a `Unit`, a list or a record (its text
/// kept exactly; `jsonb` turns `-0.0` into `0.0`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Int,
    Bytes,
    Bool,
    Float,
    Json,
}

impl Kind {
    pub(crate) fn sql(self) -> &'static str {
        match self {
            Kind::Int => "bigint",
            Kind::Bytes => "bytea",
            Kind::Bool => "boolean",
            Kind::Float => "double precision",
            Kind::Json => "json",
        }
    }

    /// The array type a set read binds a field's values as.
    pub(crate) fn array(self) -> &'static str {
        match self {
            Kind::Int => "bigint[]",
            Kind::Bytes => "bytea[]",
            Kind::Bool => "boolean[]",
            Kind::Float => "double precision[]",
            Kind::Json => "text[]",
        }
    }
}

/// A table's field: its name, what it stores, its column's kind, and
/// whether the column holds none (`Option`).
#[derive(Debug, Clone)]
pub(crate) struct Column {
    pub name: String,
    pub stored: Stored,
    pub kind: Kind,
    pub nullable: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct IndexShape {
    pub name: String,
    /// Column places, in the index's order.
    pub fields: Vec<usize>,
    pub unique: bool,
}

/// `refers <field> to <Table>`: the field holds a key of the table at
/// `target`, or nothing where it is an `Option`.
#[derive(Debug, Clone)]
pub(crate) struct ReferenceShape {
    pub field: usize,
    pub target: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct TableShape {
    pub assets: Vec<pw_core::tables::AssetClause>,
    pub name: String,
    pub columns: Vec<Column>,
    /// Column places of the key, in its order.
    pub key: Vec<usize>,
    pub order: Option<usize>,
    pub indexes: Vec<IndexShape>,
    pub references: Vec<ReferenceShape>,
    pub bounded: Option<u64>,
}

#[derive(Debug, Clone)]
pub(crate) struct RelatedShape {
    pub target: usize,
    pub source_match: usize,
    pub target_match: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct FunctionShape {
    pub related: Option<RelatedShape>,
    pub table: usize,
    pub primitive: Primitive,
    /// Column places it reads or writes by.
    pub by: Vec<usize>,
    /// Column places of tables.json's order, then the key's.
    pub sorted_by: Vec<usize>,
    /// Answers none or one row: a `get`, a unique index's read.
    pub one: bool,
}

/// **The manifest, read whole**: each table, and each function by its import.
#[derive(Debug, Clone)]
pub(crate) struct Shape {
    pub tables: Vec<TableShape>,
    pub functions: BTreeMap<String, FunctionShape>,
}

impl Shape {
    pub(crate) fn function(&self, import: &str) -> Result<&FunctionShape, Error> {
        self.functions
            .get(import)
            .ok_or_else(|| Error::Input(format!("undeclared table operation {import}")))
    }
}

fn case_tags(cases: &[pw_core::tables::StoredCase]) -> Option<Vec<String>> {
    pw_host::tables::closed_case_tags(cases.iter().map(|c| (c.name.as_str(), c.carries.len())))
}

/// A value every adapter keys by.
pub(crate) fn scalar(stored: &Stored) -> bool {
    matches!(
        stored,
        Stored::Int { .. } | Stored::String { .. } | Stored::Bool
    ) || matches!(stored,Stored::Cases(cases) if case_tags(cases).is_some())
}

/// A value this adapter stores without losing any of it, the memory
/// adapter's domain exactly: an `Option` of an `Option` is not (its
/// `Some(None)` would be stored as its `None`), nor an `Option<Unit>`.
pub(crate) fn supported(ty: &Stored) -> bool {
    match ty {
        Stored::Int { .. }
        | Stored::String { .. }
        | Stored::Bool
        | Stored::Float
        | Stored::Unit => true,
        Stored::Option(inner) => {
            !matches!(**inner, Stored::Option(_) | Stored::Unit) && supported(inner)
        }
        Stored::List(inner) => supported(inner),
        Stored::Record(fields) => fields.iter().all(|f| supported(&f.stored)),
        Stored::Cases(cases) => case_tags(cases).is_some(),
        Stored::Map(_, _) | Stored::Set(_) | Stored::Recursive(_) => false,
    }
}

/// The column a supported field is: its kind, and whether it holds none.
pub(crate) fn column_of(stored: &Stored) -> (Kind, bool) {
    match stored {
        Stored::Int { .. } => (Kind::Int, false),
        Stored::String { .. } | Stored::Cases(_) => (Kind::Bytes, false),
        Stored::Bool => (Kind::Bool, false),
        Stored::Float => (Kind::Float, false),
        Stored::Option(inner) => (column_of(inner).0, true),
        _ => (Kind::Json, false),
    }
}

/// Is `value` of the stored type `ty`, within its bounds? The memory
/// adapter's check, so that both admit the same values.
pub(crate) fn validates(value: &Value, ty: &Stored) -> bool {
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
        Stored::Cases(cases) => value
            .as_str()
            .is_some_and(|tag| case_tags(cases).is_some_and(|tags| tags.iter().any(|t| t == tag))),
        Stored::Set(_) | Stored::Map(_, _) | Stored::Recursive(_) => false,
    }
}

/// **PostgreSQL's longest name**, in bytes (`NAMEDATALEN` less one): a
/// longer one would be cut short where it is created, and two names sharing
/// their first 63 bytes would name one table.
pub(crate) const LONGEST_NAME: usize = 63;

/// The tables the adapter keeps beside the program's, which no table of the
/// program may be named.
pub(crate) const PLATFORM_TABLES: &[&str] = &[
    "pw_platform",
    "pw_schema",
    "pw_outbox",
    "pw_interactions",
    "pw_runs",
    "pw_sessions",
    "pw_sign_ins",
    "pw_outbox_trimmed",
    "pw_host_instances",
];

fn named(what: &str, name: &str) -> Result<(), Error> {
    if name.is_empty() || name.len() > LONGEST_NAME || name.contains('\0') {
        return Err(Error::Schema(format!(
            "{what} `{name}` is no PostgreSQL name: one to {LONGEST_NAME} bytes, no NUL"
        )));
    }
    Ok(())
}

fn stored<'a>(table: &'a TableJson, field: &str) -> Option<&'a Stored> {
    table
        .schema
        .fields
        .iter()
        .find(|f| f.name == field)
        .map(|f| &f.stored)
}

/// **The manifest, held whole** (the memory adapter's `shape_of`, refusal for
/// refusal), and each field's column.
pub(crate) fn shape_of(manifest: &TablesJson) -> Result<Shape, Error> {
    shape_of_with_assets(manifest, false)
}

pub(crate) fn shape_of_with_assets(
    manifest: &TablesJson,
    assets_bound: bool,
) -> Result<Shape, Error> {
    if manifest
        .tables
        .iter()
        .any(|table| !table.schema.assets.is_empty())
        && !assets_bound
    {
        return Err(Error::Schema(
            "generic asset reference storage is not installed".into(),
        ));
    }
    let mut tables = Vec::new();
    let mut functions = BTreeMap::new();
    let place = |name: &str| manifest.tables.iter().position(|t| t.schema.name == name);
    for (at, table) in manifest.tables.iter().enumerate() {
        let name = &table.schema.name;
        if place(name) != Some(at) {
            return Err(Error::Schema(format!("duplicate table {name}")));
        }
        named("table", name)?;
        if PLATFORM_TABLES.contains(&name.as_str()) || name.starts_with("pw_asset_") {
            return Err(Error::Schema(format!(
                "table {name} has the name of a table the adapter keeps beside the program's"
            )));
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
            named("field", &f.name)?;
        }
        for index in &table.schema.indexes {
            named("index", &index.name)?;
        }
        let columns: Vec<Column> = table
            .schema
            .fields
            .iter()
            .map(|f| {
                let (kind, nullable) = column_of(&f.stored);
                Column {
                    name: f.name.clone(),
                    stored: f.stored.clone(),
                    kind,
                    nullable,
                }
            })
            .collect();
        let places: BTreeMap<String, usize> = columns
            .iter()
            .enumerate()
            .map(|(i, c)| (c.name.clone(), i))
            .collect();
        let at_field = |field: &str| {
            places
                .get(field)
                .copied()
                .ok_or_else(|| Error::Schema(format!("{name} has unknown field {field}")))
        };
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
            // A foreign key holds only where both columns are of one kind.
            let target_key = stored(&manifest.tables[target], &referred.key[0])
                .map(column_of)
                .map(|(kind, _)| kind);
            if target_key != Some(column_of(held).0) {
                return Err(Error::Schema(format!(
                    "{name} refers by {}, which is not of {}'s key's type",
                    reference.field, reference.table
                )));
            }
            references.push(ReferenceShape {
                field: at_field(&reference.field)?,
                target,
            });
        }
        let shaped_table = TableShape {
            assets: table.schema.assets.clone(),
            name: name.clone(),
            key: table
                .schema
                .key
                .iter()
                .map(|f| at_field(f))
                .collect::<Result<_, _>>()?,
            order: table.schema.order.as_deref().map(at_field).transpose()?,
            indexes: table
                .schema
                .indexes
                .iter()
                .map(|index| {
                    Ok(IndexShape {
                        name: index.name.clone(),
                        fields: index
                            .fields
                            .iter()
                            .map(|f| at_field(f))
                            .collect::<Result<_, Error>>()?,
                        unique: index.unique,
                    })
                })
                .collect::<Result<_, Error>>()?,
            references,
            bounded: table.bounded,
            columns,
        };
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
            let related = if function.primitive.related() {
                let window = table
                    .windows
                    .iter()
                    .find(|window| Some(&window.name) == function.relation.as_ref())
                    .ok_or_else(|| Error::Schema("relation declaration is missing".into()))?;
                let target = place(&window.target)
                    .ok_or_else(|| Error::Schema("relation target is missing".into()))?;
                let target_match = manifest.tables[target]
                    .schema
                    .fields
                    .iter()
                    .position(|field| field.name == window.target_field)
                    .ok_or_else(|| Error::Schema("target match field is missing".into()))?;
                Some(RelatedShape {
                    target,
                    source_match: at_field(&window.source_field)?,
                    target_match,
                })
            } else {
                None
            };
            let result_table = related
                .as_ref()
                .map(|r| &manifest.tables[r.target])
                .unwrap_or(table);
            let shaped = FunctionShape {
                related,
                table: at,
                primitive: function.primitive,
                by: function
                    .by
                    .iter()
                    .map(|f| at_field(f))
                    .collect::<Result<_, _>>()?,
                sorted_by: function
                    .sorted_by
                    .iter()
                    .map(|name| {
                        result_table
                            .schema
                            .fields
                            .iter()
                            .position(|field| field.name == *name)
                            .ok_or_else(|| {
                                Error::Schema(format!("target sort field {name} is missing"))
                            })
                    })
                    .collect::<Result<_, _>>()?,
                one,
            };
            if functions.insert(function.import.clone(), shaped).is_some() {
                return Err(Error::Schema(format!(
                    "duplicate host import {}",
                    function.import
                )));
            }
        }
        tables.push(shaped_table);
    }
    // Every read by an index reads one of its table's.
    for (import, function) in &functions {
        if matches!(function.primitive, Primitive::Index | Primitive::IndexMany)
            && !tables[function.table]
                .indexes
                .iter()
                .any(|index| index.fields == function.by)
        {
            return Err(Error::Schema(format!("{import} reads by no index")));
        }
    }
    crate::sql::names_held(&tables)?;
    Ok(Shape { tables, functions })
}

/// **A row, held to its table** (the memory adapter's `check_row`): an
/// object of exactly its fields, each of its stored type and within its
/// bounds; its key; and the row as this adapter answers it once stored,
/// its fields in their declared order, a `Float` as a float.
pub(crate) fn check_row(
    table: &TableShape,
    value: &Value,
) -> Result<(Key, Map<String, Value>), Error> {
    let row = value
        .as_object()
        .ok_or_else(|| Error::Input("row must be an object".into()))?;
    if row.len() != table.columns.len() {
        return Err(Error::Input(format!(
            "{} row fields do not match its schema",
            table.name
        )));
    }
    let mut stored = Map::new();
    for column in &table.columns {
        let v = row.get(&column.name).ok_or_else(|| {
            Error::Input(format!("{} row is missing {}", table.name, column.name))
        })?;
        if !validates(v, &column.stored) {
            return Err(Error::Input(format!(
                "{} field {} fails its stored type or bound",
                table.name, column.name
            )));
        }
        stored.insert(column.name.clone(), as_stored(column, v));
    }
    let key = row_key(&stored, &table.columns, &table.key)?;
    Ok((key, stored))
}

/// A field's value as its column gives it back: a `Float` (or one in an
/// `Option`) as a float, whatever number it was given as; every other value
/// as it is (a `json` column keeps its text).
fn as_stored(column: &Column, value: &Value) -> Value {
    match (column.kind, value.as_f64()) {
        (Kind::Float, Some(float)) => Value::from(float),
        _ => value.clone(),
    }
}

pub(crate) fn row_key(
    row: &Map<String, Value>,
    columns: &[Column],
    fields: &[usize],
) -> Result<Key, Error> {
    fields
        .iter()
        .map(|&f| {
            let name = &columns[f].name;
            Atom::parse(
                row.get(name)
                    .ok_or_else(|| Error::Input(format!("row is missing selector {name}")))?,
            )
        })
        .collect()
}

fn selector(by: &[usize], args: &[Value]) -> Result<Key, Error> {
    if args.len() != by.len() {
        return Err(Error::Input(format!(
            "expected {} selector values, got {}",
            by.len(),
            args.len()
        )));
    }
    args.iter().map(Atom::parse).collect()
}

fn selections(
    table: &TableShape,
    by: &[usize],
    args: &[Value],
    many: bool,
) -> Result<BTreeSet<Key>, Error> {
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
                .map(|&f| {
                    let name = &table.columns[f].name;
                    Atom::parse(
                        obj.get(name)
                            .ok_or_else(|| Error::Input(format!("set selector lacks {name}")))?,
                    )
                })
                .collect::<Result<Key, _>>()?
        };
        out.insert(key);
    }
    Ok(out)
}

/// **What a function is given, each value held to its declared field's type
/// and bound** (the memory adapter's `typed_selections`): a boolean where a
/// string is declared fails, rather than reading as an absent row.
pub(crate) fn typed_selections(
    table: &TableShape,
    function: &FunctionShape,
    import: &str,
    args: &[Value],
) -> Result<BTreeSet<Key>, Error> {
    let keys = selections(table, &function.by, args, function.primitive.many())?;
    for key in &keys {
        for (&field, atom) in function.by.iter().zip(key) {
            let column = &table.columns[field];
            if !validates(&atom.value(), &column.stored) {
                return Err(Error::Input(format!(
                    "{import} selector {} fails its declared type or bound",
                    column.name
                )));
            }
        }
    }
    Ok(keys)
}

/// The one key a keyed function is given.
pub(crate) fn typed_selector(
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

/// **Two rows the same, as stored**: a float compared by its bits, so that
/// `-0.0` and `0.0` differ (ADR-0386's review repair); the memory adapter's
/// `same_rows`.
pub(crate) fn same_rows(
    left: &Option<Map<String, Value>>,
    right: &Option<Map<String, Value>>,
) -> bool {
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

/// Typed request columns: equality selectors, then a separately named order mark.
pub(crate) fn range_columns(
    table: &TableShape,
    function: &FunctionShape,
) -> Result<Vec<Column>, Error> {
    let order = table
        .order
        .ok_or_else(|| Error::Schema("range count requires declared order".into()))?;
    let mut columns = function
        .by
        .iter()
        .map(|&at| table.columns[at].clone())
        .collect::<Vec<_>>();
    let mut mark = table.columns[order].clone();
    mark.name = pw_core::tables::after_field(table.columns.iter().map(|c| c.name.as_str()));
    columns.push(mark);
    Ok(columns)
}

/// The cutoff participates in dedup/order, never in the equality-selector inventory.
pub(crate) fn typed_ranges(
    table: &TableShape,
    function: &FunctionShape,
    args: &[Value],
) -> Result<(Vec<Column>, Vec<Key>), Error> {
    let columns = range_columns(table, function)?;
    let supplied = if function.primitive.many() {
        let [Value::Array(requests)] = args else {
            return Err(Error::Input(
                "set range count takes one array of selector-and-mark records".into(),
            ));
        };
        requests
            .iter()
            .map(|request| {
                let record = request
                    .as_object()
                    .ok_or_else(|| Error::Input("range request is a record".into()))?;
                if record.len() != columns.len() {
                    return Err(Error::Input("range request fields differ".into()));
                }
                columns
                    .iter()
                    .map(|column| {
                        record.get(&column.name).ok_or_else(|| {
                            Error::Input(format!("range request lacks {}", column.name))
                        })
                    })
                    .collect::<Result<Vec<_>, Error>>()
            })
            .collect::<Result<Vec<_>, Error>>()?
    } else {
        if args.len() != columns.len() {
            return Err(Error::Input(
                "range count takes equality selectors and an order mark".into(),
            ));
        }
        vec![args.iter().collect()]
    };
    let mut keys = BTreeSet::new();
    for request in supplied {
        let key = columns
            .iter()
            .zip(request)
            .map(|(column, value)| {
                if !validates(value, &column.stored) {
                    return Err(Error::Input(format!(
                        "range field {} fails its declared type or bound",
                        column.name
                    )));
                }
                Atom::parse(value)
            })
            .collect::<Result<Key, Error>>()?;
        keys.insert(key);
    }
    Ok((columns, keys.into_iter().collect()))
}
