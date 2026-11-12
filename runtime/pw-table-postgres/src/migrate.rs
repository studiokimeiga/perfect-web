//! **The schema, from the declarations** (ADR-0338 1.4): the platform's
//! tables, then the program's, applied once, in one transaction, under an
//! advisory lock no other host opening the same schema can share (ADR-0246's
//! way), and recorded with its hash in `pw_schema`.
//!
//! - **None recorded**: every table created whole, where none of them exists
//!   yet (a table of the program's name that no schema recorded is drift).
//! - **The build's recorded**: nothing to do but hold the catalog to what
//!   was recorded with it, so a column, a check, an index, a key or a trigger
//!   changed by hand refuses the start (drift).
//! - **Another recorded**: the change from it to the build's, in this
//!   database's terms. Where tables.json's migration is from the schema
//!   recorded, its stated steps (`renamed`, `dropped`, `backfilled with`)
//!   are the program's statements of what loses or reinterprets data, and
//!   every other change is generated; where it is from another, only what
//!   loses nothing is generated (a table, an `Option` field, an index, a
//!   reference, a check made), and anything else refuses the start, naming
//!   each change, so an old build never takes a newer database back.

use std::collections::{BTreeMap, BTreeSet};

use pw_core::tables::{
    FieldSchema, Literal, Plan, Schema, Step, Stored, StoredCase, StoredField, TableJson,
    TableSchema, TablesJson,
};

use crate::pool::{Conn, Counters};
use crate::shape::{Shape, TableShape, shape_of};
use crate::sql::{self, Names, Object, ident};
use crate::{Error, database};

/// **The platform's tables** (ADR-0338 5.2), by version, each applied once:
/// the applied schema, the outbox (ADR-0208, a log in commit order as
/// ADR-0369 keeps it), the interactions' records (ADR-0325), and the
/// function a `String`'s check counts its code points with; then the runs,
/// the sessions and the sign-ins (`0002`, track `durable-tables`). `{s}` is
/// the adapter's schema.
const PLATFORM: &[(&str, &str)] = &[
    (
        "0001_platform",
        "CREATE TABLE {s}.pw_schema (
         applied        bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
         hash           text NOT NULL,
         schema         json NOT NULL,
         catalog        text NOT NULL,
         server_version integer NOT NULL,
         applied_at     timestamptz NOT NULL DEFAULT now()
     );
     CREATE TABLE {s}.pw_outbox (
         id           bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
         kind         text NOT NULL CHECK (kind IN ('event', 'invalidation')),
         name         text NOT NULL,
         args         json NOT NULL,
         host         text NOT NULL,
         created      timestamptz NOT NULL DEFAULT clock_timestamp(),
         committed_in xid8 NOT NULL DEFAULT pg_current_xact_id()
     );
     CREATE INDEX pw_outbox_created ON {s}.pw_outbox (created);
     CREATE TABLE {s}.pw_interactions (
         key    text PRIMARY KEY,
         sent   text NOT NULL,
         answer text NOT NULL,
         at_ms  bigint NOT NULL
     );
     CREATE INDEX pw_interactions_at ON {s}.pw_interactions (at_ms);
     CREATE FUNCTION {s}.pw_code_points(bytea) RETURNS bigint
         LANGUAGE sql IMMUTABLE STRICT PARALLEL SAFE
         AS $$ SELECT count(*) FROM generate_series(0, length($1) - 1) AS i
               WHERE get_byte($1, i) & 192 <> 128 $$;",
    ),
    // Track `durable-tables` (ADR-0338 5.2): the runs commands schedule
    // (ADR-0334), one pending for each user, command and arguments (`key`,
    // the host's, as ADR-0334's amendment of 2026-11-07 makes it), a run
    // scheduled again a new id; and the sessions and sign-ins a
    // deployment keeps where its hosts share them (ADR-0369), each by its
    // secret's SHA-256, never the secret (`platform.rs`).
    (
        "0002_runs_and_sessions",
        "CREATE TABLE {s}.pw_runs (
         id       bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
         key      text NOT NULL UNIQUE,
         command  text NOT NULL,
         args     text NOT NULL,
         user_id  text NOT NULL,
         due_ms   bigint NOT NULL,
         attempts bigint NOT NULL DEFAULT 0,
         failed   text
     );
     CREATE INDEX pw_runs_due ON {s}.pw_runs (due_ms) WHERE failed IS NULL;
     CREATE TABLE {s}.pw_sessions (
         hash    bytea PRIMARY KEY CHECK (octet_length(hash) = 32),
         user_id text NOT NULL,
         handle  text NOT NULL,
         name    text NOT NULL,
         issuer  text NOT NULL,
         epoch   bytea NOT NULL CHECK (octet_length(epoch) = 16),
         opened  timestamptz NOT NULL DEFAULT clock_timestamp(),
         seen    timestamptz NOT NULL DEFAULT clock_timestamp()
     );
     CREATE TABLE {s}.pw_sign_ins (
         hash     bytea PRIMARY KEY CHECK (octet_length(hash) = 32),
         nonce    text NOT NULL,
         verifier text NOT NULL,
         started  timestamptz NOT NULL DEFAULT clock_timestamp()
     );",
    ),
];

/// FNV-1a over `text`, as a lock's key.
pub(crate) fn lock_key(text: &str) -> i64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    i64::from_ne_bytes(h.to_ne_bytes())
}

/// **What the catalog says of the program's tables**, one line each, in one
/// order: each column's type and nullability, each constraint's and index's
/// definition, each trigger not the database's own, each table's kind,
/// persistence, row security and triggers not firing (a reference's own
/// among them), each rule, each table inheriting one, and the code points
/// function. What is recorded with a schema, and held to at each start.
const CATALOG: &str = "
SELECT c.relname::text, 'column', a.attname::text,
       pg_catalog.format_type(a.atttypid, a.atttypmod) || CASE WHEN a.attnotnull THEN ' NOT NULL' ELSE '' END
  FROM pg_catalog.pg_class c
  JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
  JOIN pg_catalog.pg_attribute a ON a.attrelid = c.oid
 WHERE n.nspname = $1 AND c.relname = ANY($2) AND a.attnum > 0 AND NOT a.attisdropped
UNION ALL
SELECT c.relname::text, 'constraint', o.conname::text, pg_catalog.pg_get_constraintdef(o.oid)
  FROM pg_catalog.pg_constraint o
  JOIN pg_catalog.pg_class c ON c.oid = o.conrelid
  JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
 WHERE n.nspname = $1 AND c.relname = ANY($2)
UNION ALL
SELECT c.relname::text, 'index', i.relname::text, pg_catalog.pg_get_indexdef(x.indexrelid)
  FROM pg_catalog.pg_index x
  JOIN pg_catalog.pg_class c ON c.oid = x.indrelid
  JOIN pg_catalog.pg_class i ON i.oid = x.indexrelid
  JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
 WHERE n.nspname = $1 AND c.relname = ANY($2)
UNION ALL
SELECT c.relname::text, 'table', c.relname::text,
       c.relkind::text || CASE WHEN c.relpersistence <> 'p' THEN ' persistence ' || c.relpersistence::text ELSE '' END
                       || CASE WHEN c.relrowsecurity THEN ' row security' ELSE '' END
                       || CASE WHEN c.relforcerowsecurity THEN ' forced' ELSE '' END
                       || (SELECT CASE WHEN count(*) > 0 THEN ' triggers not firing: ' || count(*) ELSE '' END
                             FROM pg_catalog.pg_trigger t WHERE t.tgrelid = c.oid AND t.tgenabled <> 'O')
  FROM pg_catalog.pg_class c
  JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
 WHERE n.nspname = $1 AND c.relname = ANY($2)
UNION ALL
SELECT c.relname::text, 'rule', r.rulename::text, pg_catalog.pg_get_ruledef(r.oid)
  FROM pg_catalog.pg_rewrite r
  JOIN pg_catalog.pg_class c ON c.oid = r.ev_class
  JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
 WHERE n.nspname = $1 AND c.relname = ANY($2)
UNION ALL
SELECT c.relname::text, 'inherited by', hn.nspname::text || '.' || h.relname::text, ''
  FROM pg_catalog.pg_inherits i
  JOIN pg_catalog.pg_class c ON c.oid = i.inhparent
  JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
  JOIN pg_catalog.pg_class h ON h.oid = i.inhrelid
  JOIN pg_catalog.pg_namespace hn ON hn.oid = h.relnamespace
 WHERE n.nspname = $1 AND c.relname = ANY($2)
UNION ALL
SELECT c.relname::text, 'trigger', t.tgname::text, pg_catalog.pg_get_triggerdef(t.oid)
  FROM pg_catalog.pg_trigger t
  JOIN pg_catalog.pg_class c ON c.oid = t.tgrelid
  JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
 WHERE n.nspname = $1 AND c.relname = ANY($2) AND NOT t.tgisinternal
UNION ALL
SELECT '', 'function', p.proname::text, pg_catalog.pg_get_functiondef(p.oid)
  FROM pg_catalog.pg_proc p
  JOIN pg_catalog.pg_namespace n ON n.oid = p.pronamespace
 WHERE n.nspname = $1 AND p.proname = 'pw_code_points'
ORDER BY 1, 2, 3";

/// One line of the catalog: (table, what, name, definition).
type Line = (String, String, String, String);

fn catalog(
    conn: &mut Conn,
    counters: &Counters,
    names: &Names,
    tables: &[String],
) -> Result<Vec<Line>, Error> {
    let rows = conn
        .query(
            counters,
            CATALOG,
            &[Box::new(names.schema.clone()), Box::new(tables.to_vec())],
        )
        .map_err(database)?;
    Ok(rows
        .iter()
        .map(|r| (r.get(0), r.get(1), r.get(2), r.get(3)))
        .collect())
}

fn written(lines: &[Line]) -> String {
    lines
        .iter()
        .map(|(t, w, n, d)| format!("{t}\t{w}\t{n}\t{d}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The lines as a server of another major version may write them: what each
/// is, by name, and each column's type, never a definition's text.
fn structural(text: &str) -> Vec<String> {
    text.lines()
        .map(|line| {
            let parts: Vec<&str> = line.splitn(4, '\t').collect();
            match parts.as_slice() {
                [t, w, n, d] if matches!(*w, "column" | "table") => format!("{t}\t{w}\t{n}\t{d}"),
                [t, w, n, _] => format!("{t}\t{w}\t{n}"),
                _ => line.to_string(),
            }
        })
        .collect()
}

/// What differs between two catalogs' lines, for a refusal to name.
fn differences(recorded: &[String], now: &[String]) -> String {
    let before: BTreeSet<&String> = recorded.iter().collect();
    let after: BTreeSet<&String> = now.iter().collect();
    let mut out: Vec<String> = Vec::new();
    for gone in before.difference(&after).take(5) {
        out.push(format!(
            "recorded, and no longer: {}",
            gone.replace('\t', " ")
        ));
    }
    for new in after.difference(&before).take(5) {
        out.push(format!(
            "found, and never recorded: {}",
            new.replace('\t', " ")
        ));
    }
    out.join("; ")
}

/// **A recorded schema's tables, as the adapter shapes them**: what its
/// objects were made from.
fn shapes_of(tables: &[TableSchema]) -> Result<Vec<TableShape>, Error> {
    let manifest = TablesJson {
        hash: String::new(),
        tables: tables
            .iter()
            .map(|schema| TableJson {
                schema: schema.clone(),
                module: schema.name.clone(),
                interface: String::new(),
                row: String::new(),
                bounded: None,
                functions: Vec::new(),
            })
            .collect(),
        migration: Plan {
            from: None,
            to: String::new(),
            steps: Vec::new(),
        },
    };
    Ok(shape_of(&manifest)?.tables)
}

/// The same type, its bounds aside: what a change of bounds keeps
/// (`pw_core::tables`'s own, which is the compiler's).
fn unbounded(stored: &Stored) -> Stored {
    match stored {
        Stored::Int { .. } => Stored::Int {
            at_least: None,
            at_most: None,
        },
        Stored::String { .. } => Stored::String {
            min_length: None,
            max_length: None,
        },
        Stored::Option(t) => Stored::Option(Box::new(unbounded(t))),
        Stored::List(t) => Stored::List(Box::new(unbounded(t))),
        Stored::Set(t) => Stored::Set(Box::new(unbounded(t))),
        Stored::Map(k, v) => Stored::Map(Box::new(unbounded(k)), Box::new(unbounded(v))),
        Stored::Record(fs) => Stored::Record(
            fs.iter()
                .map(|f| StoredField {
                    name: f.name.clone(),
                    stored: unbounded(&f.stored),
                })
                .collect(),
        ),
        Stored::Cases(cs) => Stored::Cases(
            cs.iter()
                .map(|c| StoredCase {
                    name: c.name.clone(),
                    carries: c.carries.iter().map(unbounded).collect(),
                })
                .collect(),
        ),
        other => other.clone(),
    }
}

/// A backfilled value as SQL writes it into its column, where it is of the
/// column's type and within its bounds: an `Int` quoted (the least `Int`
/// is no negated literal, `-9223372036854775808::bigint` being out of
/// range), a `String`'s bytes in hex, decoded (a backslash in an ordinary
/// literal is an escape where `standard_conforming_strings` is off, so
/// `'\x6162'` would be `a62`; hex digits are read alike either way).
fn literal(field: &FieldSchema, value: &Literal) -> Option<String> {
    if !field.stored.admits(value) {
        return None;
    }
    Some(match value {
        Literal::Int(n) => format!("'{n}'::bigint"),
        Literal::Bool(b) => format!("{b}"),
        Literal::Str(s) => {
            let hex: String = s.as_bytes().iter().map(|b| format!("{b:02x}")).collect();
            format!("pg_catalog.decode('{hex}', 'hex')")
        }
    })
}

/// **The change from `recorded` to `built`**, in DDL, or a refusal naming
/// each change nothing states (see the module's note).
pub(crate) fn changes(
    names: &Names,
    recorded: &Schema,
    built: &Schema,
    plan: &Plan,
) -> Result<Vec<String>, Error> {
    let stated_applies = plan.from.as_deref() == Some(recorded.hash.as_str());
    let stated: &[Step] = if stated_applies { &plan.steps } else { &[] };
    let mut used: BTreeSet<usize> = BTreeSet::new();
    let mut refused: Vec<String> = Vec::new();
    let mut tables_sql: Vec<String> = Vec::new();
    let mut columns_sql: Vec<String> = Vec::new();
    // The recorded tables, renamed, without the columns dropped or retyped:
    // what the catalog holds once the columns are changed.
    let mut model: Vec<TableSchema> = Vec::new();
    let recorded_shapes = shapes_of(&recorded.tables)?;
    let recorded_objects = sql::objects(names, &recorded_shapes);

    for was in &recorded.tables {
        let Some(now) = built.tables.iter().find(|t| t.name == was.name) else {
            refused.push(format!(
                "`{}` is recorded and declared no more: its rows would be lost",
                was.name
            ));
            continue;
        };
        let table = &was.name;
        if was.source != now.source {
            refused.push(format!(
                "`{table}` is recorded in another source: its rows would stay in the other database"
            ));
            continue;
        }
        // The renames stated, old name to new.
        let mut renamed: BTreeMap<String, String> = BTreeMap::new();
        for (i, step) in stated.iter().enumerate() {
            if let Step::RenameField { table: t, from, to } = step
                && t == table
                && was.fields.iter().any(|f| f.name == *from)
                && !was.fields.iter().any(|f| f.name == *to)
                && now.fields.iter().any(|f| f.name == *to)
            {
                renamed.insert(from.clone(), to.clone());
                used.insert(i);
                columns_sql.push(format!(
                    "ALTER TABLE {} RENAME COLUMN {} TO {}",
                    names.qualified(table),
                    ident(from),
                    ident(to)
                ));
                for (old, new) in sql::field_objects(table, from)
                    .into_iter()
                    .zip(sql::field_objects(table, to))
                {
                    if recorded_objects.contains_key(&old) {
                        columns_sql.push(format!(
                            "ALTER TABLE {} RENAME CONSTRAINT {} TO {}",
                            names.qualified(table),
                            ident(&old),
                            ident(&new)
                        ));
                    }
                }
            }
        }
        let now_name = |n: &str| renamed.get(n).cloned().unwrap_or_else(|| n.to_string());
        let key: Vec<String> = was.key.iter().map(|k| now_name(k)).collect();
        if key != now.key {
            refused.push(format!(
                "`{table}`'s key is recorded as `{}` and declared `{}`: each row would be found by another key",
                was.key.join(", "),
                now.key.join(", ")
            ));
            continue;
        }
        // The columns gone: dropped, or dropped and added again retyped.
        let mut gone: BTreeSet<String> = BTreeSet::new();
        for f in &was.fields {
            let to = now_name(&f.name);
            match now.fields.iter().find(|g| g.name == to) {
                Some(g)
                    if unbounded(&g.stored) != unbounded(&f.stored) && now.key.contains(&to) =>
                {
                    // A retype drops the column and adds it backfilled: of
                    // the key, the primary key would go with it, and every
                    // row would hold one key (the review of 2026-11-02).
                    // Its stated `backfilled with` is answered here.
                    used.extend(stated.iter().position(|s| {
                        matches!(s, Step::RetypeField { table: t, field, .. } if t == table && *field == to)
                    }));
                    refused.push(format!(
                        "`{table}.{to}` is of `{table}`'s key, recorded as `{}` and declared `{}`: \
                         each row would be found by another key (a key retyped is a new table)",
                        f.ty, g.ty
                    ));
                }
                Some(g) if unbounded(&g.stored) != unbounded(&f.stored) => {
                    let stated_retype = stated.iter().enumerate().find_map(|(i, s)| match s {
                        Step::RetypeField {
                            table: t,
                            field,
                            backfill,
                        } if t == table && *field == to => Some((i, backfill)),
                        _ => None,
                    });
                    let Some((i, backfill)) = stated_retype else {
                        refused.push(format!(
                            "`{table}.{to}` is recorded as `{}` and declared `{}`: each row's value \
                             would be read as another type (state `backfilled with`)",
                            f.ty, g.ty
                        ));
                        continue;
                    };
                    let Some(value) = literal(g, backfill) else {
                        refused.push(format!(
                            "`{table}.{to}` is backfilled with {backfill}, which is no value of `{}`",
                            g.ty
                        ));
                        continue;
                    };
                    used.insert(i);
                    gone.insert(to.clone());
                    let column = column_of(g);
                    columns_sql.push(format!(
                        "ALTER TABLE {} DROP COLUMN {}",
                        names.qualified(table),
                        ident(&to)
                    ));
                    columns_sql.extend(sql::add_column(names, table, &column, Some(&value)));
                }
                Some(_) => {}
                None => {
                    let stated_drop = stated.iter().position(|s| {
                        matches!(s, Step::DropField { table: t, field } if t == table && *field == f.name)
                    });
                    match stated_drop {
                        Some(i) => {
                            used.insert(i);
                            gone.insert(to.clone());
                            columns_sql.push(format!(
                                "ALTER TABLE {} DROP COLUMN {}",
                                names.qualified(table),
                                ident(&f.name)
                            ));
                        }
                        None => refused.push(format!(
                            "`{table}.{}` is recorded and is no field of `{table}`'s row: its values \
                             would be lost (state `dropped`, or `renamed from`)",
                            f.name
                        )),
                    }
                }
            }
        }
        // The fields no recorded row holds.
        let recorded_as: BTreeSet<String> = was.fields.iter().map(|f| now_name(&f.name)).collect();
        for g in now.fields.iter().filter(|g| !recorded_as.contains(&g.name)) {
            if let Stored::Option(_) = g.stored {
                columns_sql.extend(sql::add_column(names, table, &column_of(g), None));
                continue;
            }
            let stated_add = stated.iter().enumerate().find_map(|(i, s)| match s {
                Step::AddField {
                    table: t,
                    field,
                    backfill: Some(backfill),
                } if t == table && *field == g.name => Some((i, backfill)),
                _ => None,
            });
            match stated_add.and_then(|(i, v)| literal(g, v).map(|v| (i, v))) {
                Some((i, value)) => {
                    used.insert(i);
                    columns_sql.extend(sql::add_column(names, table, &column_of(g), Some(&value)));
                }
                None => refused.push(format!(
                    "`{table}.{}` is new, of `{}`, and the rows recorded hold no value for it \
                     (make it an `Option`, or state `backfilled with`)",
                    g.name, g.ty
                )),
            }
        }
        // What the catalog holds of it once its columns are changed.
        let mut kept = was.clone();
        for f in &mut kept.fields {
            f.name = now_name(&f.name);
        }
        kept.key = key;
        kept.order = kept.order.as_deref().map(now_name);
        for index in &mut kept.indexes {
            index.fields = index.fields.iter().map(|f| now_name(f)).collect();
        }
        for reference in &mut kept.references {
            reference.field = now_name(&reference.field);
        }
        kept.fields.retain(|f| !gone.contains(&f.name));
        let order_gone = kept.order.as_ref().is_some_and(|o| gone.contains(o));
        if order_gone {
            kept.order = None;
        }
        // An index holding a column gone was dropped with it; a plain one
        // holds the order too.
        kept.indexes
            .retain(|i| !i.fields.iter().any(|f| gone.contains(f)) && (i.unique || !order_gone));
        kept.references.retain(|r| !gone.contains(&r.field));
        model.push(kept);
    }
    // Each table the change adds is shaped with the whole build, so that a
    // reference to a table held, or to one added with it, resolves (the
    // review of PR #66: shaped alone, it refused `unknown referenced table`).
    let built_shapes = shapes_of(&built.tables)?;
    for shape in &built_shapes {
        if !recorded.tables.iter().any(|t| t.name == shape.name) {
            tables_sql.push(sql::create_table(names, shape));
        }
    }
    // A statement this change does not use states nothing this database
    // holds: tables.json's migration is not the one from its schema.
    for (i, step) in stated.iter().enumerate() {
        let destructive = matches!(
            step,
            Step::RenameField { .. }
                | Step::DropField { .. }
                | Step::RetypeField { .. }
                | Step::AddField {
                    backfill: Some(_),
                    ..
                }
        );
        if destructive && !used.contains(&i) {
            refused.push(format!(
                "tables.json's migration states {step:?}, which takes the recorded schema nowhere"
            ));
        }
    }
    // The objects: each the build makes and the catalog does not hold made,
    // each the catalog holds and the build does not make dropped.
    let model_tables: Vec<TableSchema> = model;
    let old = objects_where(names, &model_tables)?;
    let new = sql::objects(names, &built_shapes);
    let mut drops: Vec<&Object> = old
        .iter()
        .filter(|(name, object)| new.get(*name) != Some(*object))
        .map(|(_, object)| object)
        .collect();
    let mut creates: Vec<&Object> = new
        .iter()
        .filter(|(name, object)| old.get(*name) != Some(*object))
        .map(|(_, object)| object)
        .collect();
    if !stated_applies {
        for object in &drops {
            refused.push(format!(
                "{} would be dropped, and tables.json's migration is not from the schema recorded",
                object.drop
            ));
        }
    }
    if !refused.is_empty() {
        let from = plan.from.as_deref().unwrap_or("none");
        return Err(Error::Migration(format!(
            "the database holds schema {}, and this build declares {} (its migration is from {from}): {}",
            recorded.hash,
            built.hash,
            refused.join("; ")
        )));
    }
    drops.sort_by_key(|o| o.kind);
    creates.sort_by_key(|o| std::cmp::Reverse(o.kind));
    let mut out = tables_sql;
    out.extend(columns_sql);
    out.extend(drops.into_iter().map(|o| o.drop.clone()));
    out.extend(creates.into_iter().map(|o| o.create.clone()));
    Ok(out)
}

/// The objects of tables some of whose references may name a table not
/// among them (dropped with the model's columns): those references left out.
fn objects_where(names: &Names, tables: &[TableSchema]) -> Result<BTreeMap<String, Object>, Error> {
    let present: BTreeSet<&str> = tables.iter().map(|t| t.name.as_str()).collect();
    let mut kept: Vec<TableSchema> = tables.to_vec();
    for t in &mut kept {
        t.references.retain(|r| present.contains(r.table.as_str()));
    }
    Ok(sql::objects(names, &shapes_of(&kept)?))
}

fn column_of(field: &FieldSchema) -> crate::shape::Column {
    let (kind, nullable) = match &field.stored {
        Stored::Option(inner) => (kind_of(inner), true),
        other => (kind_of(other), false),
    };
    crate::shape::Column {
        name: field.name.clone(),
        stored: field.stored.clone(),
        kind,
        nullable,
    }
}

fn kind_of(stored: &Stored) -> crate::shape::Kind {
    use crate::shape::Kind;
    match stored {
        Stored::Int { .. } => Kind::Int,
        Stored::String { .. } => Kind::Bytes,
        Stored::Bool => Kind::Bool,
        Stored::Float => Kind::Float,
        _ => Kind::Json,
    }
}

/// The schema recorded last, if any: its hash, its tables, the catalog
/// recorded with it and the server's version then.
struct Recorded {
    schema: Schema,
    catalog: String,
    server_version: i32,
}

/// **Apply the platform's tables and the build's schema**, once, under the
/// lock (see the module's note). `conn` is the adapter's first connection.
pub(crate) fn apply(
    conn: &mut Conn,
    counters: &Counters,
    names: &Names,
    manifest: &TablesJson,
    shape: &Shape,
) -> Result<(), Error> {
    // Read committed whatever the connection's default: each statement after
    // the lock is granted reads what the host before committed. At
    // repeatable read or serializable the snapshot would be the lock's own
    // `SELECT`'s, taken before the lock was granted ("frozen at the start of
    // its first query", PostgreSQL 18, 13.4.2, which advises read committed
    // where explicit locking prevents concurrent changes), and a host that
    // waited would not see the schema the first made (the review of
    // 2026-11-02).
    conn.batch(counters, "BEGIN ISOLATION LEVEL READ COMMITTED")
        .map_err(database)?;
    let applied = apply_in(conn, counters, names, manifest, shape);
    match applied {
        Ok(()) => conn.batch(counters, "COMMIT").map_err(database),
        Err(error) => {
            let _ = conn.batch(counters, "ROLLBACK");
            Err(error)
        }
    }
}

fn apply_in(
    conn: &mut Conn,
    counters: &Counters,
    names: &Names,
    manifest: &TablesJson,
    shape: &Shape,
) -> Result<(), Error> {
    let s = ident(&names.schema);
    conn.query(
        counters,
        "SELECT pg_advisory_xact_lock($1)",
        &[Box::new(lock_key(&format!(
            "pw_tables_migrations:{}",
            names.schema
        )))],
    )
    .map_err(database)?;
    conn.batch(counters, &format!("CREATE SCHEMA IF NOT EXISTS {s}"))
        .map_err(database)?;
    let encoding: String = conn
        .query(counters, "SELECT current_setting('server_encoding')", &[])
        .map_err(database)?
        .first()
        .map(|r| r.get(0))
        .unwrap_or_default();
    if encoding != "UTF8" {
        return Err(Error::Schema(format!(
            "the database's encoding is {encoding}; a declared table's JSON needs UTF8"
        )));
    }
    conn.batch(
        counters,
        &format!(
            "CREATE TABLE IF NOT EXISTS {s}.pw_platform (
                 version    text PRIMARY KEY,
                 applied_at timestamptz NOT NULL DEFAULT now()
             )"
        ),
    )
    .map_err(database)?;
    for (version, text) in PLATFORM {
        let applied = !conn
            .query(
                counters,
                &format!("SELECT 1 FROM {s}.pw_platform WHERE version = $1"),
                &[Box::new(version.to_string())],
            )
            .map_err(database)?
            .is_empty();
        if applied {
            continue;
        }
        conn.batch(counters, &text.replace("{s}", &s))
            .map_err(|e| {
                Error::Schema(format!(
                    "the platform's migration {version}: {}",
                    crate::pg(&e)
                ))
            })?;
        conn.query(
            counters,
            &format!("INSERT INTO {s}.pw_platform (version) VALUES ($1)"),
            &[Box::new(version.to_string())],
        )
        .map_err(database)?;
    }
    let built = Schema::of(manifest.tables.iter().map(|t| t.schema.clone()).collect());
    let tables: Vec<String> = shape.tables.iter().map(|t| t.name.clone()).collect();
    let server_version: i32 = conn
        .query(
            counters,
            "SELECT current_setting('server_version_num')::integer",
            &[],
        )
        .map_err(database)?
        .first()
        .map(|r| r.get(0))
        .unwrap_or_default();
    let recorded = conn
        .query(
            counters,
            &format!(
                "SELECT schema::text, catalog, server_version, hash FROM {s}.pw_schema ORDER BY applied DESC LIMIT 1"
            ),
            &[],
        )
        .map_err(database)?
        .first()
        .map(|r| {
            let text: String = r.get(0);
            let hash: String = r.get(3);
            let schema = Schema::read(&text).map_err(|why| {
                Error::Drift(format!("the schema recorded in {s}.pw_schema: {why}"))
            })?;
            if schema.hash != hash {
                return Err(Error::Drift(format!(
                    "{s}.pw_schema records hash {hash} beside a schema whose hash is {}",
                    schema.hash
                )));
            }
            Ok(Recorded {
                schema,
                catalog: r.get(1),
                server_version: r.get(2),
            })
        })
        .transpose()?;
    let ddl = match &recorded {
        None => {
            let lines = catalog(conn, counters, names, &tables)?;
            if let Some((table, ..)) = lines
                .iter()
                .find(|(t, w, ..)| !t.is_empty() && w == "table")
            {
                return Err(Error::Drift(format!(
                    "{s} holds a table `{table}`, and no schema is recorded there: \
                     it was made by something other than this adapter"
                )));
            }
            let mut out: Vec<String> = shape
                .tables
                .iter()
                .map(|t| sql::create_table(names, t))
                .collect();
            let mut objects: Vec<Object> =
                sql::objects(names, &shape.tables).into_values().collect();
            objects.sort_by_key(|o| std::cmp::Reverse(o.kind));
            out.extend(objects.into_iter().map(|o| o.create));
            out
        }
        Some(rec) => {
            let recorded_tables: Vec<String> =
                rec.schema.tables.iter().map(|t| t.name.clone()).collect();
            let now = written(&catalog(conn, counters, names, &recorded_tables)?);
            let same_major = rec.server_version / 10000 == server_version / 10000;
            let (was, is) = if same_major {
                (
                    rec.catalog.lines().map(str::to_string).collect::<Vec<_>>(),
                    now.lines().map(str::to_string).collect::<Vec<_>>(),
                )
            } else {
                (structural(&rec.catalog), structural(&now))
            };
            if was != is {
                return Err(Error::Drift(format!(
                    "the catalog of {s} is not the one recorded with schema {}: {}",
                    rec.schema.hash,
                    differences(&was, &is)
                )));
            }
            if rec.schema.hash == built.hash {
                if !same_major {
                    // The same schema, read by another major version: its
                    // catalog recorded again as this server writes it.
                    record(conn, counters, names, &built, &now, server_version)?;
                }
                return Ok(());
            }
            changes(names, &rec.schema, &built, &manifest.migration)?
        }
    };
    for statement in &ddl {
        conn.batch(counters, statement)
            .map_err(|e| Error::Migration(format!("{statement}: {}", crate::pg(&e))))?;
    }
    let now = written(&catalog(conn, counters, names, &tables)?);
    record(conn, counters, names, &built, &now, server_version)
}

fn record(
    conn: &mut Conn,
    counters: &Counters,
    names: &Names,
    built: &Schema,
    catalog: &str,
    server_version: i32,
) -> Result<(), Error> {
    let text = serde_json::to_string(built).map_err(|e| Error::Schema(format!("schema: {e}")))?;
    conn.query(
        counters,
        &format!(
            "INSERT INTO {}.pw_schema (hash, schema, catalog, server_version) VALUES ($1, $2::text::json, $3, $4)",
            ident(&names.schema)
        ),
        &[
            Box::new(built.hash.clone()),
            Box::new(text),
            Box::new(catalog.to_string()),
            Box::new(server_version),
        ],
    )
    .map_err(database)?;
    Ok(())
}
