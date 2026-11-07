//! **The statements a declaration gives** (ADR-0338 1.4 and 1.5): each
//! table's `CREATE TABLE`, its checks, indexes and foreign keys as named
//! objects, and each function's one statement, generated once when the
//! adapter opens and prepared once on each connection that runs it.
//!
//! Every name is quoted and qualified by the adapter's schema, so no
//! `search_path` decides what a statement reads. A `json` column is read as
//! its text and written from text (`::text`, `::text::json`), so the client
//! binds only `bigint`, `bytea`, `boolean`, `double precision` and `text`.

use std::collections::BTreeMap;

use pw_core::tables::{Primitive, Stored};

use crate::Error;
use crate::shape::{Column, FunctionShape, Kind, LONGEST_NAME, Shape, TableShape};

/// An identifier, quoted.
pub(crate) fn ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// **The adapter's own column for a row's owner** (the owned-rows ADR), in a
/// table whose rows record one: the id of the reader who inserted the row,
/// or null, a row no reader owns (the platform's, or one held before its
/// table recorded owners). No field of a program's is named so (a field's
/// name is a letter or `_`, then letters, digits and `_`), and no statement
/// reads it into a row.
pub(crate) const OWNER: &str = "pw:owner";

/// **Does its writer change the row `alias`**: `$any` (the platform's
/// command, which changes any row), or the row records `$me` as its owner.
/// Never null: a row recording no owner is no reader's.
fn mine(alias: &str, me: usize, any: usize) -> String {
    format!(
        "(${any}::boolean OR COALESCE({alias}.{} = ${me}::text, false))",
        ident(OWNER)
    )
}

/// FNV-1a, 64 bits: a long derived name's suffix.
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// **The name of an object a table owns** (a check, an index, a foreign
/// key): `<table>__<what>`, or, where that is longer than PostgreSQL keeps,
/// its first bytes and the hash of the whole, so that two objects never
/// share a name cut short.
pub(crate) fn object_name(table: &str, what: &str) -> String {
    let whole = format!("{table}__{what}");
    if whole.len() <= LONGEST_NAME {
        return whole;
    }
    let mut cut = LONGEST_NAME - 17;
    while !whole.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}_{:016x}", &whole[..cut], fnv1a(whole.as_bytes()))
}

/// The adapter's schema, and the names qualified in it.
#[derive(Debug, Clone)]
pub(crate) struct Names {
    pub schema: String,
}

impl Names {
    pub(crate) fn qualified(&self, name: &str) -> String {
        format!("{}.{}", ident(&self.schema), ident(name))
    }
}

/// A column as a statement reads it: `t."c"`, a `json` one as its text.
fn read_column(alias: &str, column: &Column) -> String {
    match column.kind {
        Kind::Json => format!("{alias}.{}::text", ident(&column.name)),
        _ => format!("{alias}.{}", ident(&column.name)),
    }
}

/// Every column of a row, as a statement reads it, in the declared order.
pub(crate) fn row_list(alias: &str, table: &TableShape) -> String {
    table
        .columns
        .iter()
        .map(|c| read_column(alias, c))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Every column of a row, as a CTE gives it: `c0`, `c1`, ...
fn named_row_list(alias: &str, table: &TableShape) -> String {
    table
        .columns
        .iter()
        .enumerate()
        .map(|(i, c)| format!("{} AS c{i}", read_column(alias, c)))
        .collect::<Vec<_>>()
        .join(", ")
}

fn aliased_row(alias: &str, table: &TableShape) -> String {
    (0..table.columns.len())
        .map(|i| format!("{alias}.c{i}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Parameter `n`, cast to its column's type.
fn param(n: usize, kind: Kind) -> String {
    match kind {
        Kind::Json => format!("${n}::text::json"),
        kind => format!("${n}::{}", kind.sql()),
    }
}

/// `(t."a", t."b") = ($1::bytea, $2::bigint)`, the parameters numbered from
/// `first`, or one per field at `places`.
fn equal(alias: &str, table: &TableShape, fields: &[usize], params: &[usize]) -> String {
    let columns: Vec<String> = fields
        .iter()
        .map(|&f| format!("{alias}.{}", ident(&table.columns[f].name)))
        .collect();
    let values: Vec<String> = fields
        .iter()
        .zip(params)
        .map(|(&f, &n)| param(n, table.columns[f].kind))
        .collect();
    if fields.len() == 1 {
        format!("{} = {}", columns[0], values[0])
    } else {
        format!("({}) = ({})", columns.join(", "), values.join(", "))
    }
}

/// A set read's condition: one field `= ANY($1::bytea[])`; several, each
/// field's values an array of their own, paired by place (`unnest`): one
/// statement whatever the number of keys.
fn among(alias: &str, table: &TableShape, fields: &[usize]) -> String {
    if fields.len() == 1 {
        let column = &table.columns[fields[0]];
        return format!(
            "{alias}.{} = ANY($1::{})",
            ident(&column.name),
            column.kind.array()
        );
    }
    let columns: Vec<String> = fields
        .iter()
        .map(|&f| format!("{alias}.{}", ident(&table.columns[f].name)))
        .collect();
    let arrays: Vec<String> = fields
        .iter()
        .enumerate()
        .map(|(i, &f)| format!("${}::{}", i + 1, table.columns[f].kind.array()))
        .collect();
    format!(
        "({}) IN (SELECT * FROM unnest({}))",
        columns.join(", "),
        arrays.join(", ")
    )
}

/// `ORDER BY` tables.json's order, then the key: each field ascending, a
/// `String`'s bytes in their order (code points'), `false` before `true`.
fn order_by(alias: &str, table: &TableShape, sorted_by: &[usize]) -> String {
    let mut fields: Vec<usize> = Vec::new();
    for &f in sorted_by.iter().chain(&table.key) {
        if !fields.contains(&f) {
            fields.push(f);
        }
    }
    let columns: Vec<String> = fields
        .iter()
        .map(|&f| format!("{alias}.{} ASC", ident(&table.columns[f].name)))
        .collect();
    format!(" ORDER BY {}", columns.join(", "))
}

/// The row's values, parameters 1 to n in the declared order, cast.
fn row_params(table: &TableShape) -> String {
    table
        .columns
        .iter()
        .enumerate()
        .map(|(i, c)| param(i + 1, c.kind))
        .collect::<Vec<_>>()
        .join(", ")
}

fn column_names(table: &TableShape) -> String {
    table
        .columns
        .iter()
        .map(|c| ident(&c.name))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The parameter numbers of `fields` in a row's parameters.
fn row_places(fields: &[usize]) -> Vec<usize> {
    fields.iter().map(|&f| f + 1).collect()
}

/// **A unique value held by another row, first by the key, then by each
/// unique index in its declared order**: what a write is refused by, as the
/// memory adapter checks (its key first, then `Data::set`'s indexes).
fn holder(names: &Names, table: &TableShape, with_key: bool) -> String {
    let t = names.qualified(&table.name);
    let key = equal("t", table, &table.key, &row_places(&table.key));
    let mut cases: Vec<String> = Vec::new();
    if with_key {
        cases.push(key.clone());
    }
    for index in table.indexes.iter().filter(|i| i.unique) {
        cases.push(equal("t", table, &index.fields, &row_places(&index.fields)));
    }
    if cases.is_empty() {
        return format!(
            "SELECT {}, 0 AS pw_rank FROM {t} t WHERE false",
            named_row_list("t", table)
        );
    }
    let rank: Vec<String> = cases
        .iter()
        .enumerate()
        .map(|(i, c)| format!("WHEN {c} THEN {i}"))
        .collect();
    let mut condition = format!("({})", cases.join(" OR "));
    if !with_key {
        condition = format!("{condition} AND NOT ({key})");
    }
    format!(
        "SELECT {}, CASE {} END AS pw_rank FROM {t} t WHERE {condition} ORDER BY pw_rank LIMIT 1",
        named_row_list("t", table),
        rank.join(" ")
    )
}

/// Whether the table has room for one row more, where it is bounded: its
/// rows counted to its bound at most.
fn room(names: &Names, table: &TableShape) -> Option<String> {
    let bound = table.bounded?;
    Some(format!(
        "(SELECT count(*) FROM (SELECT 1 FROM {} LIMIT {bound}) AS rows) < {bound}",
        names.qualified(&table.name)
    ))
}

/// **Each function's one statement**, by its import.
pub(crate) fn statements(names: &Names, shape: &Shape) -> BTreeMap<String, String> {
    shape
        .functions
        .iter()
        .map(|(import, function)| {
            let table = &shape.tables[function.table];
            (import.clone(), statement(names, table, function))
        })
        .collect()
}

fn statement(names: &Names, table: &TableShape, function: &FunctionShape) -> String {
    let t = names.qualified(&table.name);
    let rows = row_list("t", table);
    let by: Vec<usize> = (1..=function.by.len()).collect();
    match function.primitive {
        Primitive::Get | Primitive::Prefix | Primitive::Index => {
            let mut sql = format!(
                "SELECT {rows} FROM {t} t WHERE {}",
                equal("t", table, &function.by, &by)
            );
            if !function.one {
                sql.push_str(&order_by("t", table, &function.sorted_by));
            }
            sql
        }
        Primitive::GetMany | Primitive::PrefixMany | Primitive::IndexMany => format!(
            "SELECT {rows} FROM {t} t WHERE {}{}",
            among("t", table, &function.by),
            order_by("t", table, &function.sorted_by)
        ),
        Primitive::All => format!(
            "SELECT {rows} FROM {t} t{} LIMIT {}",
            order_by("t", table, &function.sorted_by),
            table.bounded.unwrap_or(0).saturating_add(1)
        ),
        Primitive::Insert => {
            let room = room(names, table)
                .map(|r| format!(" AND {r}"))
                .unwrap_or_default();
            // An owned row records its writer: the reader's id, or null.
            let (columns, values) = match table.owner {
                Some(_) => (
                    format!("{}, {}", column_names(table), ident(OWNER)),
                    format!("{}, ${}::text", row_params(table), table.columns.len() + 1),
                ),
                None => (column_names(table), row_params(table)),
            };
            format!(
                "WITH holder AS ({}), \
                 written AS (INSERT INTO {t} ({columns}) SELECT {values} WHERE NOT EXISTS (SELECT 1 FROM holder){room} RETURNING 1) \
                 SELECT (SELECT count(*) FROM written), h.pw_rank, {} \
                 FROM (VALUES (1)) AS one LEFT JOIN holder h ON true",
                holder(names, table, true),
                aliased_row("h", table),
            )
        }
        Primitive::Put => {
            let room = room(names, table)
                .map(|r| format!(" AND (EXISTS (SELECT 1 FROM old) OR {r})"))
                .unwrap_or_default();
            let key_names: Vec<String> = table
                .key
                .iter()
                .map(|&f| ident(&table.columns[f].name))
                .collect();
            let rest: Vec<String> = table
                .columns
                .iter()
                .enumerate()
                .filter(|(i, _)| !table.key.contains(i))
                .map(|(_, c)| format!("{0} = EXCLUDED.{0}", ident(&c.name)))
                .collect();
            let conflict = if rest.is_empty() {
                format!("ON CONFLICT ({}) DO NOTHING", key_names.join(", "))
            } else {
                format!(
                    "ON CONFLICT ({}) DO UPDATE SET {}",
                    key_names.join(", "),
                    rest.join(", ")
                )
            };
            if table.owner.is_none() {
                return format!(
                    "WITH holder AS ({}), \
                     old AS (SELECT {}, 1 AS pw_old FROM {t} t WHERE {}), \
                     written AS (INSERT INTO {t} ({}) SELECT {} WHERE NOT EXISTS (SELECT 1 FROM holder){room} {conflict} RETURNING 1) \
                     SELECT (SELECT count(*) FROM written), h.pw_rank, {}, o.pw_old IS NOT NULL, {} \
                     FROM (VALUES (1)) AS one LEFT JOIN holder h ON true LEFT JOIN old o ON true",
                    holder(names, table, false),
                    named_row_list("t", table),
                    equal("t", table, &table.key, &row_places(&table.key)),
                    column_names(table),
                    row_params(table),
                    aliased_row("h", table),
                    aliased_row("o", table),
                );
            }
            // An owned table's (the owned-rows ADR): a row written anew
            // records its writer, and the update changes the row at the key
            // only where its writer changes it, judged on the row the
            // conflict found, whatever the transaction's isolation (under
            // read committed, a row another command inserted after this
            // statement's snapshot). Another reader's row is then left as it
            // was, and its owner too: no write. `old` answers whether the row
            // was its writer's, last, so every other column keeps its place.
            let (me, any) = (table.columns.len() + 1, table.columns.len() + 2);
            let guarded = if rest.is_empty() {
                conflict
            } else {
                format!("{conflict} WHERE {}", mine("pw_t", me, any))
            };
            // Whether a row anew had room, last: a put that wrote nothing
            // with room, no holder and no row at its key in the snapshot met
            // a row inserted since, which the guard left as it was.
            let had_room = self::room(names, table).unwrap_or_else(|| "true".to_string());
            format!(
                "WITH holder AS ({}), \
                 old AS (SELECT {}, 1 AS pw_old, {} AS pw_mine FROM {t} t WHERE {}), \
                 written AS (INSERT INTO {t} AS pw_t ({}, {}) SELECT {}, ${me}::text \
                 WHERE NOT EXISTS (SELECT 1 FROM holder){room} {guarded} RETURNING 1) \
                 SELECT (SELECT count(*) FROM written), h.pw_rank, {}, o.pw_old IS NOT NULL, {}, COALESCE(o.pw_mine, false), \
                 {had_room} \
                 FROM (VALUES (1)) AS one LEFT JOIN holder h ON true LEFT JOIN old o ON true",
                holder(names, table, false),
                named_row_list("t", table),
                mine("t", me, any),
                equal("t", table, &table.key, &row_places(&table.key)),
                column_names(table),
                ident(OWNER),
                row_params(table),
                aliased_row("h", table),
                aliased_row("o", table),
            )
        }
        // An owned table's delete removes its writer's rows alone: another
        // reader's row is absent to it.
        Primitive::Delete | Primitive::DeletePrefix => match table.owner {
            None => format!(
                "DELETE FROM {t} AS t WHERE {} RETURNING {rows}",
                equal("t", table, &function.by, &by)
            ),
            Some(_) => format!(
                "DELETE FROM {t} AS t WHERE {} AND {} RETURNING {rows}",
                equal("t", table, &function.by, &by),
                mine("t", by.len() + 1, by.len() + 2)
            ),
        },
    }
}

// --- the schema's objects ---------------------------------------------------

/// What kind of object a table owns, in the order they are dropped (a
/// foreign key first) and the reverse order they are made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ObjectKind {
    Foreign,
    Check,
    Index,
}

/// **An object a table owns**, by its name: how it is made and dropped.
/// Two schemas' objects of one name are one object where they are made
/// alike.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Object {
    pub kind: ObjectKind,
    pub create: String,
    pub drop: String,
}

/// `CREATE TABLE`, its columns and its key: what an object of it needs.
pub(crate) fn create_table(names: &Names, table: &TableShape) -> String {
    let mut parts: Vec<String> = table
        .columns
        .iter()
        .map(|c| {
            format!(
                "{} {}{}",
                ident(&c.name),
                c.kind.sql(),
                if c.nullable { "" } else { " NOT NULL" }
            )
        })
        .collect();
    let key: Vec<String> = table
        .key
        .iter()
        .map(|&f| ident(&table.columns[f].name))
        .collect();
    if table.owner.is_some() {
        parts.push(format!("{} text", ident(OWNER)));
    }
    parts.push(format!(
        "CONSTRAINT {} PRIMARY KEY ({})",
        ident(&object_name(&table.name, "pkey")),
        key.join(", ")
    ));
    format!(
        "CREATE TABLE {} ({})",
        names.qualified(&table.name),
        parts.join(", ")
    )
}

/// `ALTER TABLE .. ADD COLUMN`, for a field no release had: an `Option`'s,
/// holding none, or one `backfilled with` a value every row holds.
pub(crate) fn add_column(
    names: &Names,
    table: &str,
    column: &Column,
    backfill: Option<&str>,
) -> Vec<String> {
    let t = names.qualified(table);
    let c = ident(&column.name);
    match backfill {
        None => vec![format!(
            "ALTER TABLE {t} ADD COLUMN {c} {}{}",
            column.kind.sql(),
            if column.nullable { "" } else { " NOT NULL" }
        )],
        Some(value) => vec![
            format!(
                "ALTER TABLE {t} ADD COLUMN {c} {} NOT NULL DEFAULT {value}",
                column.kind.sql()
            ),
            format!("ALTER TABLE {t} ALTER COLUMN {c} DROP DEFAULT"),
        ],
    }
}

/// **The owner column made, or given up** (the owned-rows ADR): a table that
/// records its rows' owner from now on holds every row it held for no
/// reader; one that no longer does gives the owners recorded up.
pub(crate) fn owner_column(names: &Names, table: &str, recorded: bool) -> String {
    let t = names.qualified(table);
    match recorded {
        true => format!("ALTER TABLE {t} ADD COLUMN {} text", ident(OWNER)),
        false => format!("ALTER TABLE {t} DROP COLUMN {}", ident(OWNER)),
    }
}

/// **A refinement as a `CHECK`** (ADR-0338 1.4): an `Int`'s bounds, a
/// `String`'s length in code points (its bytes counted by
/// `pw_code_points`), a `Float` finite, a `json` column's shape; an
/// `Option`'s on the value it holds (a check whose value is null holds).
fn check_of(names: &Names, column: &Column) -> Option<String> {
    let c = ident(&column.name);
    let stored = match &column.stored {
        Stored::Option(inner) => inner.as_ref(),
        other => other,
    };
    let mut parts: Vec<String> = Vec::new();
    match stored {
        Stored::Int { at_least, at_most } => {
            if let Some(min) = at_least {
                parts.push(format!("{c} >= {min}"));
            }
            if let Some(max) = at_most {
                parts.push(format!("{c} <= {max}"));
            }
        }
        Stored::String {
            min_length,
            max_length,
        } => {
            let counted = format!("{}.pw_code_points({c})", ident(&names.schema));
            if let Some(min) = min_length {
                parts.push(format!("{counted} >= {min}"));
            }
            if let Some(max) = max_length {
                parts.push(format!("{counted} <= {max}"));
            }
        }
        Stored::Float => parts.push(format!(
            "{c} <> 'NaN'::double precision AND {c} <> 'Infinity'::double precision \
             AND {c} <> '-Infinity'::double precision"
        )),
        Stored::Unit => parts.push(format!("json_typeof({c}) = 'null'")),
        Stored::List(_) => parts.push(format!("json_typeof({c}) = 'array'")),
        Stored::Record(_) => parts.push(format!("json_typeof({c}) = 'object'")),
        _ => {}
    }
    (!parts.is_empty()).then(|| parts.join(" AND "))
}

/// The columns an index holds: a unique one its fields; any other its
/// fields, then the order and the key, so that its read comes in the
/// order it answers in with no sort.
pub(crate) fn index_columns(table: &TableShape, fields: &[usize], unique: bool) -> Vec<usize> {
    let mut out: Vec<usize> = fields.to_vec();
    if !unique {
        for &f in table.order.iter().chain(&table.key) {
            if !out.contains(&f) {
                out.push(f);
            }
        }
    }
    out
}

/// **Every object of `tables`**, by name: each field's check, each index,
/// each reference's foreign key, deferred to the commit (a command's writes
/// held to their references at its end, as the memory adapter holds them
/// at `prepare`).
pub(crate) fn objects(names: &Names, tables: &[TableShape]) -> BTreeMap<String, Object> {
    let mut out = BTreeMap::new();
    for table in tables {
        let t = names.qualified(&table.name);
        for column in &table.columns {
            if let Some(check) = check_of(names, column) {
                let name = object_name(&table.name, &format!("{}__check", column.name));
                out.insert(
                    name.clone(),
                    Object {
                        kind: ObjectKind::Check,
                        create: format!(
                            "ALTER TABLE {t} ADD CONSTRAINT {} CHECK ({check})",
                            ident(&name)
                        ),
                        drop: format!("ALTER TABLE {t} DROP CONSTRAINT {}", ident(&name)),
                    },
                );
            }
        }
        for index in &table.indexes {
            let name = object_name(&table.name, &index.name);
            let columns: Vec<String> = index_columns(table, &index.fields, index.unique)
                .iter()
                .map(|&f| ident(&table.columns[f].name))
                .collect();
            out.insert(
                name.clone(),
                Object {
                    kind: ObjectKind::Index,
                    create: format!(
                        "CREATE {}INDEX {} ON {t} ({})",
                        if index.unique { "UNIQUE " } else { "" },
                        ident(&name),
                        columns.join(", ")
                    ),
                    drop: format!("DROP INDEX {}", names.qualified(&name)),
                },
            );
        }
        for reference in &table.references {
            let target = &tables[reference.target];
            let field = &table.columns[reference.field].name;
            let name = object_name(&table.name, &format!("{field}__fkey"));
            out.insert(
                name.clone(),
                Object {
                    kind: ObjectKind::Foreign,
                    create: format!(
                        "ALTER TABLE {t} ADD CONSTRAINT {} FOREIGN KEY ({}) REFERENCES {} ({}) \
                         DEFERRABLE INITIALLY DEFERRED",
                        ident(&name),
                        ident(field),
                        names.qualified(&target.name),
                        ident(&target.columns[target.key[0]].name)
                    ),
                    drop: format!("ALTER TABLE {t} DROP CONSTRAINT {}", ident(&name)),
                },
            );
        }
    }
    out
}

/// The relations the adapter keeps beside the program's, by name: its
/// tables, their keys' indexes, its indexes and its identities' sequences
/// (`migrate`'s platform version 0001).
const PLATFORM_RELATIONS: &[&str] = &[
    "pw_platform",
    "pw_platform_pkey",
    "pw_schema",
    "pw_schema_pkey",
    "pw_schema_applied_seq",
    "pw_outbox",
    "pw_outbox_pkey",
    "pw_outbox_id_seq",
    "pw_outbox_created",
    "pw_interactions",
    "pw_interactions_pkey",
    "pw_interactions_at",
];

/// **Every name the declaration's tables and objects take, each taken
/// once** (the review of 2026-11-02): each table, its key's constraint and
/// index, each check, index and foreign key, beside the platform's
/// relations. An object is named `<table>__<what>`, and a name may hold
/// `__` (an index `by_rank__check`, a field `by_rank`'s check), so two
/// objects could be given one name; [`objects`] keeps them by name, and one
/// would silently replace the other: a check never made, or a reference
/// never held. Such a declaration is refused where the adapter opens.
pub(crate) fn names_held(tables: &[TableShape]) -> Result<(), Error> {
    let mut held: BTreeMap<String, String> = PLATFORM_RELATIONS
        .iter()
        .map(|name| (name.to_string(), "a relation the adapter keeps".to_string()))
        .collect();
    let mut take = |name: String, what: String| match held.get(&name) {
        Some(first) => Err(Error::Schema(format!(
            "{what} and {first} would both be named `{name}`"
        ))),
        None => {
            held.insert(name, what);
            Ok(())
        }
    };
    // Whether a column has a check: what `check_of` makes, whatever schema.
    let unnamed = Names {
        schema: String::new(),
    };
    for table in tables {
        let t = &table.name;
        take(t.clone(), format!("table `{t}`"))?;
        take(object_name(t, "pkey"), format!("`{t}`'s key"))?;
        for column in &table.columns {
            if check_of(&unnamed, column).is_some() {
                take(
                    object_name(t, &format!("{}__check", column.name)),
                    format!("`{t}.{}`'s check", column.name),
                )?;
            }
        }
        for index in &table.indexes {
            take(
                object_name(t, &index.name),
                format!("`{t}`'s index `{}`", index.name),
            )?;
        }
        for reference in &table.references {
            let field = &table.columns[reference.field].name;
            take(
                object_name(t, &format!("{field}__fkey")),
                format!("`{t}.{field}`'s reference"),
            )?;
        }
    }
    Ok(())
}

/// The constraint a field's check or foreign key is named, which a field
/// renamed renames.
pub(crate) fn field_objects(table: &str, field: &str) -> [String; 2] {
    [
        object_name(table, &format!("{field}__check")),
        object_name(table, &format!("{field}__fkey")),
    ]
}
