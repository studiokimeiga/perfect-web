//! **A table function, held to tables.json where it is linked** (ADR-0338
//! D2; the integrator's ruling 4 of 2026-10-25, ADR-0385; ADR-0386 for the
//! writes and the other reads).
//!
//! The engine links a function for each import with the type the component
//! declares (`func_new`), so a table import typed otherwise than tables.json
//! declares its function linked, and failed, if at all, only when it was
//! called. A component is compiled from the same declaration as its
//! tables.json, so a difference means one of the two is not what was built
//! with the other: a stale build, or a forged one. [`check_import`] compares
//! the import's type, read from the component, with the type tables.json
//! gives the function, before anything is instantiated, and refuses there.
//!
//! **And each answer, when it is given** (the integrator's rulings 7 and 8):
//! whatever answers a table function, the memory adapter's callback or any
//! other, its answer reaches the component only if it is the function's:
//! [`check_answer`] holds every row to the row tables.json declares, exactly
//! (no field more, none less, each of its type and within its bounds: an
//! answer is not projected onto the type, as ADR-0166 projects a host's
//! records); a read's rows to what it was asked (a key, a prefix, an index's
//! values), each row once, in tables.json's order, and an `all` to its
//! table's bound; a write's refusal to a row that holds what it would write.

#[cfg(test)]
use crate::test_assertions::*;

use std::collections::BTreeSet;

use wasmtime::component::Val;
use wasmtime::component::types::{ComponentFunc, Type};

use crate::tables::{Access, Column, Primitive, Read, Stored, Write};

/// **Is this import typed as tables.json declares its function?** A read
/// takes what it reads by (the key's, a prefix's or an index's fields; a set
/// form one list of them, each a field's value or a record of the fields;
/// `all` nothing) and answers an option of its row (a `get`, a unique
/// index's read) or a list of rows. A write takes a row (`insert`, `put`) or
/// the fields it deletes by, and answers `result<_, row>` where it can be
/// refused, nothing otherwise. A row is a record of the table's fields, in
/// their declared order, by their WIT names, each of its stored type.
pub fn check_import(access: &Access, func: &ComponentFunc) -> Result<(), String> {
    let params: Vec<Type> = func.params().map(|(_, ty)| ty).collect();
    let results: Vec<Type> = func.results().collect();
    let refused = |what: &str, why: String| {
        format!(
            "`{}` is typed otherwise than tables.json declares the {what}: {why}",
            access.import()
        )
    };
    match access {
        Access::Read(read) => {
            read_import(read, &params, &results).map_err(|why| refused("read", why))
        }
        Access::Write(write) => {
            write_import(write, &params, &results).map_err(|why| refused("write", why))
        }
    }
}

fn read_import(read: &Read, params: &[Type], results: &[Type]) -> Result<(), String> {
    let words = read.primitive.by_words();
    if read.many() {
        let [Type::List(given)] = params else {
            return Err(format!(
                "it takes ({}), not one list of {words}s",
                listed(params)
            ));
        };
        selector(words, &read.by, &given.ty())?;
    } else {
        fields(words, &read.by, params)?;
    }
    if read.one() {
        let [Type::Option(row)] = results else {
            return Err(format!(
                "it answers ({}), not an option of a row",
                listed(results)
            ));
        };
        record(&read.row, &row.ty())
    } else {
        let [Type::List(rows)] = results else {
            return Err(format!(
                "it answers ({}), not a list of rows",
                listed(results)
            ));
        };
        record(&read.row, &rows.ty())
    }
}

fn write_import(write: &Write, params: &[Type], results: &[Type]) -> Result<(), String> {
    // What it answers: where it can be refused, the row that refuses it, a
    // row of its table's; nothing otherwise.
    if write.refusable() {
        let [Type::Result(answer)] = results else {
            return Err(format!(
                "it answers ({}), not a result of the row that refuses it",
                listed(results)
            ));
        };
        if let Some(ok) = answer.ok() {
            return Err(format!(
                "its ok holds {}, where it holds nothing",
                named(&ok)
            ));
        }
        let Some(held) = answer.err() else {
            return Err("its error holds nothing, where it holds the row that refuses it".into());
        };
        record(&write.row, &held).map_err(|why| format!("its refusal's row: {why}"))?;
    } else if !results.is_empty() {
        return Err(format!("it answers ({}), not nothing", listed(results)));
    }
    // What it is given: a row, or the fields it deletes by.
    match write.primitive {
        Primitive::Insert | Primitive::Put => {
            let [row] = params else {
                return Err(format!("it takes ({}), not one row", listed(params)));
            };
            record(&write.row, row).map_err(|why| format!("its row: {why}"))
        }
        primitive => fields(primitive.by_words(), &write.by, params),
    }
}

/// The fields a read or a delete is given, each by its stored type, in
/// order.
fn fields(words: &str, by: &[Column], params: &[Type]) -> Result<(), String> {
    if params.len() != by.len() {
        return Err(format!(
            "it takes ({}), not the {words}'s {} field(s)",
            listed(params),
            by.len()
        ));
    }
    for (ty, column) in params.iter().zip(by) {
        stored(&column.stored, ty)
            .map_err(|why| format!("its {words}'s `{}`: {why}", column.name))?;
    }
    Ok(())
}

/// What a set form's list holds: its one field's value, or a record of its
/// fields.
fn selector(words: &str, by: &[Column], ty: &Type) -> Result<(), String> {
    match by {
        [column] => stored(&column.stored, ty)
            .map_err(|why| format!("its {words}'s `{}`: {why}", column.name)),
        columns => record(columns, ty).map_err(|why| format!("its {words}: {why}")),
    }
}

/// A record of these fields, in this order, by their WIT names, each of its
/// stored type.
fn record(columns: &[Column], ty: &Type) -> Result<(), String> {
    let Type::Record(record) = ty else {
        return Err(format!("{} where a record is declared", named(ty)));
    };
    let fields: Vec<_> = record.fields().collect();
    if fields.len() != columns.len() {
        return Err(format!(
            "a record of {} field(s) where tables.json declares {}: {}",
            fields.len(),
            columns.len(),
            columns
                .iter()
                .map(|column| column.wit.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    for (field, column) in fields.iter().zip(columns) {
        if field.name != column.wit {
            return Err(format!(
                "field `{}` where tables.json declares `{}`",
                field.name, column.wit
            ));
        }
        stored(&column.stored, &field.ty).map_err(|why| format!("`{}`: {why}", column.name))?;
    }
    Ok(())
}

/// A value's WIT type, as a field's stored type gives it.
fn stored(stored: &Stored, ty: &Type) -> Result<(), String> {
    match (stored, ty) {
        (Stored::Int { .. }, Type::S64)
        | (Stored::String { .. }, Type::String)
        | (Stored::Bool, Type::Bool)
        | (Stored::Float, Type::Float64) => Ok(()),
        // An adapter's row holds none as nothing (JSON's `null`, SQL's
        // `NULL`), so `Some(None)` would come back `None` (ADR-0386).
        (Stored::Option(inner), _) if matches!(**inner, Stored::Option(_)) => Err(
            "an Option of an Option, whose `Some(None)` a row cannot tell from `None`, is not \
             a type the table host serves yet"
                .into(),
        ),
        (Stored::Option(inner), Type::Option(option)) => self::stored(inner, &option.ty()),
        (Stored::List(inner), Type::List(list)) => self::stored(inner, &list.ty()),
        (Stored::Record(fields), ty) => {
            record(&fields.iter().map(Column::of).collect::<Vec<_>>(), ty)
        }
        (
            Stored::Unit
            | Stored::Set(_)
            | Stored::Map(..)
            | Stored::Cases(_)
            | Stored::Recursive(_),
            _,
        ) => Err(format!(
            "{} is not a type the table host serves yet",
            described(stored)
        )),
        (stored, ty) => Err(format!(
            "{} where tables.json declares {}",
            named(ty),
            described(stored)
        )),
    }
}

/// A stored type, in a refusal's words.
pub(crate) fn described(stored: &Stored) -> &'static str {
    match stored {
        Stored::Int { .. } => "an Int (s64)",
        Stored::String { .. } => "a String (string)",
        Stored::Bool => "a Bool (bool)",
        Stored::Float => "a Float (f64)",
        Stored::Unit => "a Unit",
        Stored::Option(_) => "an Option",
        Stored::List(_) => "a List",
        Stored::Set(_) => "a Set",
        Stored::Map(..) => "a Map",
        Stored::Record(_) => "a record",
        Stored::Cases(_) => "a type of cases",
        Stored::Recursive(_) => "a type that contains itself",
    }
}

/// A WIT type, by its name.
fn named(ty: &Type) -> &'static str {
    match ty {
        Type::Bool => "bool",
        Type::S8 => "s8",
        Type::U8 => "u8",
        Type::S16 => "s16",
        Type::U16 => "u16",
        Type::S32 => "s32",
        Type::U32 => "u32",
        Type::S64 => "s64",
        Type::U64 => "u64",
        Type::Float32 => "f32",
        Type::Float64 => "f64",
        Type::Char => "char",
        Type::String => "string",
        Type::List(_) => "a list",
        Type::Map(_) => "a map",
        Type::Record(_) => "a record",
        Type::Tuple(_) => "a tuple",
        Type::Variant(_) => "a variant",
        Type::Enum(_) => "an enum",
        Type::Option(_) => "an option",
        Type::Result(_) => "a result",
        Type::Flags(_) => "flags",
        Type::Own(_) | Type::Borrow(_) => "a resource",
        Type::Future(_) => "a future",
        Type::Stream(_) => "a stream",
        Type::ErrorContext => "an error context",
        Type::FixedLengthList(_) => "a fixed-length list",
    }
}

fn listed(types: &[Type]) -> String {
    types.iter().map(named).collect::<Vec<_>>().join(", ")
}

/// **Is this answer the function's?** See the module: a read's rows are
/// what it was asked (a `get`'s and a unique index's one row its key's or
/// value's), each once, in tables.json's order ([`Read::canonical`]), an
/// `all` within its table's bound; a write that can be refused answers ok or
/// the row holding what it would write; every row is exactly the row
/// tables.json declares ([`exact_row`]). A key or a value no row has is left
/// out, and an empty list is an answer.
pub fn check_answer(access: &Access, args: &[Val], results: &[Val]) -> Result<(), String> {
    match access {
        Access::Read(read) => read_answer(read, args, results),
        Access::Write(write) => write_answer(write, args, results),
    }
}

fn read_answer(read: &Read, args: &[Val], results: &[Val]) -> Result<(), String> {
    let [answer] = results else {
        return Err(format!("{} values where a read answers one", results.len()));
    };
    let by = || read.by.iter().map(|c| c.name.as_str());
    if read.one() {
        return match answer {
            Val::Option(None) => Ok(()),
            Val::Option(Some(row)) => {
                let fields = exact_row(&read.row, row)?;
                let key = atoms(&read.row, fields, by())?;
                let asked = args.iter().map(atom).collect::<Result<Vec<_>, _>>()?;
                if key == asked {
                    Ok(())
                } else {
                    Err(format!(
                        "the row of {} where {} was asked for",
                        shown(&key),
                        shown(&asked)
                    ))
                }
            }
            other => Err(format!(
                "{} where a {} answers an option of a row",
                val_named(other),
                read.primitive.name()
            )),
        };
    }
    let Val::List(rows) = answer else {
        return Err(format!(
            "{} where a {} answers a list of rows",
            val_named(answer),
            read.primitive.name()
        ));
    };
    // What each row's fields read by must be one of: none for `all`.
    let asked: Option<BTreeSet<Vec<Atom>>> = match read.primitive {
        Primitive::All => None,
        _ if read.many() => Some(asked_selectors(read, args)?),
        _ => Some(BTreeSet::from([args
            .iter()
            .map(atom)
            .collect::<Result<Vec<_>, _>>()?])),
    };
    if read.primitive == Primitive::All
        && let Some(bound) = read.bounded
        && rows.len() as u64 > bound
    {
        return Err(format!(
            "{} rows, where {} is bounded to {bound}",
            rows.len(),
            read.table
        ));
    }
    let order = read.canonical();
    let mut seen = BTreeSet::new();
    let mut last: Option<Vec<Atom>> = None;
    for (index, row) in rows.iter().enumerate() {
        let at = |why: String| format!("row {}: {why}", index + 1);
        let fields = exact_row(&read.row, row).map_err(at)?;
        if let Some(asked) = &asked {
            let found = atoms(&read.row, fields, by()).map_err(at)?;
            if !asked.contains(&found) {
                return Err(at(format!("{} was not asked for", shown(&found))));
            }
        }
        let key = atoms(&read.row, fields, read.key.iter().map(String::as_str)).map_err(at)?;
        if !seen.insert(key.clone()) {
            return Err(at(format!("{} was answered before", shown(&key))));
        }
        let place = atoms(&read.row, fields, order.iter().map(String::as_str)).map_err(at)?;
        if last.as_ref().is_some_and(|last| *last > place) {
            return Err(at(format!(
                "it is out of tables.json's order (by {})",
                order.join(", ")
            )));
        }
        last = Some(place);
    }
    Ok(())
}

fn write_answer(write: &Write, args: &[Val], results: &[Val]) -> Result<(), String> {
    if !write.refusable() {
        return match results {
            [] => Ok(()),
            _ => Err(format!(
                "{} values where a {} answers nothing",
                results.len(),
                write.primitive.name()
            )),
        };
    }
    let [answer] = results else {
        return Err(format!(
            "{} values where a {} answers one",
            results.len(),
            write.primitive.name()
        ));
    };
    let held = match answer {
        Val::Result(Ok(None)) => return Ok(()),
        Val::Result(Err(Some(held))) => held,
        other => {
            return Err(format!(
                "{} where a {} answers ok, or the row that refuses it",
                val_named(other),
                write.primitive.name()
            ));
        }
    };
    let [given] = args else {
        return Err(format!(
            "a {} is given one row, and was given {}",
            write.primitive.name(),
            args.len()
        ));
    };
    let held = exact_row(&write.row, held)?;
    let given = exact_row(&write.row, given)?;
    let key =
        |fields: &[(String, Val)]| atoms(&write.row, fields, write.key.iter().map(String::as_str));
    let (its, mine) = (key(held)?, key(given)?);
    let mut shares = false;
    for unique in &write.unique {
        let values =
            |fields: &[(String, Val)]| atoms(&write.row, fields, unique.iter().map(String::as_str));
        shares |= values(held)? == values(given)?;
    }
    // An insert is refused by the row of its key, or of a unique value it
    // would hold; a put, which replaces its key's row, by another row of a
    // unique value it would hold.
    let refuses = match write.primitive {
        Primitive::Insert => its == mine || shares,
        _ => its != mine && shares,
    };
    if refuses {
        Ok(())
    } else {
        Err(format!(
            "refused by the row of {}, which holds nothing the {} of {} would",
            shown(&its),
            write.primitive.name(),
            shown(&mine)
        ))
    }
}

/// **A row, exactly as tables.json declares it**: a record of the table's
/// fields in their declared order, by their WIT names, no field more and
/// none less, each value of its stored type and within its bounds. Its
/// fields, for reading by position.
pub fn exact_row<'a>(row: &[Column], value: &'a Val) -> Result<&'a [(String, Val)], String> {
    let Val::Record(fields) = value else {
        return Err(format!("{} where a row is a record", val_named(value)));
    };
    exact(row, fields)?;
    Ok(fields)
}

fn exact(columns: &[Column], given: &[(String, Val)]) -> Result<(), String> {
    for (index, column) in columns.iter().enumerate() {
        let Some((name, value)) = given.get(index) else {
            return Err(format!("the row lacks `{}`", column.wit));
        };
        if *name != column.wit {
            return Err(format!(
                "`{name}` where tables.json declares `{}`",
                column.wit
            ));
        }
        check_value(&column.stored, value).map_err(|why| format!("`{}`: {why}", column.wit))?;
    }
    if let Some((extra, _)) = given.get(columns.len()) {
        return Err(format!(
            "the row has `{extra}`, which tables.json does not declare"
        ));
    }
    Ok(())
}

/// **A value of its stored type, within its bounds** (an `Int`'s range, a
/// `String`'s length in code points, a finite `Float`).
pub fn check_value(stored: &Stored, value: &Val) -> Result<(), String> {
    match (stored, value) {
        (Stored::Int { at_least, at_most }, Val::S64(n)) => {
            if at_least.is_some_and(|least| *n < least) || at_most.is_some_and(|most| *n > most) {
                Err(format!("{n} is outside its bounds"))
            } else {
                Ok(())
            }
        }
        (
            Stored::String {
                min_length,
                max_length,
            },
            Val::String(text),
        ) => {
            let length = i64::try_from(text.chars().count()).unwrap_or(i64::MAX);
            if min_length.is_some_and(|least| length < least)
                || max_length.is_some_and(|most| length > most)
            {
                Err(format!("{length} characters is outside its bounds"))
            } else {
                Ok(())
            }
        }
        (Stored::Bool, Val::Bool(_)) => Ok(()),
        (Stored::Float, Val::Float64(x)) if x.is_finite() => Ok(()),
        (Stored::Float, Val::Float64(x)) => Err(format!("{x} is not a finite number")),
        (Stored::Option(_), Val::Option(None)) => Ok(()),
        (Stored::Option(inner), Val::Option(Some(value))) => check_value(inner, value),
        (Stored::List(inner), Val::List(items)) => {
            items.iter().enumerate().try_for_each(|(index, item)| {
                check_value(inner, item).map_err(|why| format!("item {}: {why}", index + 1))
            })
        }
        (Stored::Record(fields), Val::Record(given)) => {
            exact(&fields.iter().map(Column::of).collect::<Vec<_>>(), given)
        }
        (stored, value) => Err(format!(
            "{} where tables.json declares {}",
            val_named(value),
            described(stored)
        )),
    }
}

/// A key's or an order's value, compared as tables.json orders them: an
/// `Int` by value, a `String` by code point, `false` before `true`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Atom {
    Bool(bool),
    Int(i64),
    Str(String),
}

fn atom(value: &Val) -> Result<Atom, String> {
    match value {
        Val::Bool(b) => Ok(Atom::Bool(*b)),
        Val::S64(n) => Ok(Atom::Int(*n)),
        Val::String(s) => Ok(Atom::Str(s.clone())),
        other => Err(format!("{} is no key's or order's value", val_named(other))),
    }
}

/// The values of these fields of an exact row, by their Pleris names.
fn atoms<'a>(
    row: &[Column],
    fields: &[(String, Val)],
    names: impl Iterator<Item = &'a str>,
) -> Result<Vec<Atom>, String> {
    names
        .map(|name| {
            let at = row
                .iter()
                .position(|column| column.name == name)
                .ok_or_else(|| format!("`{name}` is no field of the row"))?;
            atom(&fields[at].1)
        })
        .collect()
}

/// What a set form was given: each its one field's value, or a record of
/// its fields by their WIT names.
fn asked_selectors(read: &Read, args: &[Val]) -> Result<BTreeSet<Vec<Atom>>, String> {
    let words = read.primitive.by_words();
    let [Val::List(given)] = args else {
        return Err(format!(
            "a {} is given one list of {words}s",
            read.primitive.name()
        ));
    };
    given
        .iter()
        .map(|value| match (read.by.as_slice(), value) {
            ([_], value) => Ok(vec![atom(value)?]),
            (columns, Val::Record(fields)) if fields.len() == columns.len() => columns
                .iter()
                .zip(fields)
                .map(|(column, (name, value))| {
                    if *name == column.wit {
                        atom(value)
                    } else {
                        Err(format!(
                            "a {words}'s `{name}` where `{}` is declared",
                            column.wit
                        ))
                    }
                })
                .collect(),
            (_, other) => Err(format!("{} where a {words} is a record", val_named(other))),
        })
        .collect()
}

fn shown(key: &[Atom]) -> String {
    let values: Vec<String> = key
        .iter()
        .map(|atom| match atom {
            Atom::Bool(b) => b.to_string(),
            Atom::Int(n) => n.to_string(),
            Atom::Str(s) => format!("{s:?}"),
        })
        .collect();
    format!("({})", values.join(", "))
}

/// A value's WIT type, by its name.
fn val_named(value: &Val) -> &'static str {
    match value {
        Val::Bool(_) => "bool",
        Val::S8(_) => "s8",
        Val::U8(_) => "u8",
        Val::S16(_) => "s16",
        Val::U16(_) => "u16",
        Val::S32(_) => "s32",
        Val::U32(_) => "u32",
        Val::S64(_) => "s64",
        Val::U64(_) => "u64",
        Val::Float32(_) => "f32",
        Val::Float64(_) => "f64",
        Val::Char(_) => "char",
        Val::String(_) => "string",
        Val::List(_) => "a list",
        Val::Map(_) => "a map",
        Val::Record(_) => "a record",
        Val::Tuple(_) => "a tuple",
        Val::Variant(..) => "a variant",
        Val::Enum(_) => "an enum",
        Val::Option(_) => "an option",
        Val::Result(_) => "a result",
        Val::Flags(_) => "flags",
        Val::Resource(_) => "a resource",
        Val::Future(_) => "a future",
        Val::Stream(_) => "a stream",
        Val::ErrorContext(_) => "an error context",
        Val::FixedLengthList(_) => "a fixed-length list",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn string() -> Stored {
        Stored::String {
            min_length: None,
            max_length: None,
        }
    }

    fn int(at_least: Option<i64>) -> Stored {
        Stored::Int {
            at_least,
            at_most: None,
        }
    }

    /// Items' row: keyed by `list` then `id`, ordered by `added`; its `count`
    /// at least 1, its `note` optional.
    fn row() -> Vec<Column> {
        vec![
            Column::new("list", string()),
            Column::new("id", string()),
            Column::new("count", int(Some(1))),
            Column::new("added", int(None)),
            Column::new("note", Stored::Option(Box::new(string()))),
        ]
    }

    /// Items' read bound to `primitive` by `by`, its rows in D1's order.
    fn items(primitive: Primitive, by: &[&str]) -> Read {
        let row = row();
        let by: Vec<Column> = by
            .iter()
            .map(|name| {
                row.iter()
                    .find(|c| c.name == *name)
                    .assert_ok("valid domain result")
                    .clone()
            })
            .collect();
        let grouped: Vec<String> = if primitive.many() {
            by.iter().map(|c| c.name.clone()).collect()
        } else {
            Vec::new()
        };
        let mut sorted_by: Vec<String> = Vec::new();
        for field in
            grouped
                .into_iter()
                .chain(["added".to_string(), "list".to_string(), "id".to_string()])
        {
            if !sorted_by.contains(&field) {
                sorted_by.push(field);
            }
        }
        Read {
            import: format!("pw:tables/items#{}", primitive.name()),
            table: "Items".into(),
            primitive,
            by,
            key: vec!["list".into(), "id".into()],
            order: Some("added".into()),
            sorted_by: if primitive == Primitive::Get {
                Vec::new()
            } else {
                sorted_by
            },
            unique: false,
            bounded: None,
            row,
        }
    }

    fn item(list: &str, id: &str, count: i64, note: Option<&str>) -> Val {
        added(list, id, count, 0, note)
    }

    fn added(list: &str, id: &str, count: i64, at: i64, note: Option<&str>) -> Val {
        Val::Record(vec![
            ("list".into(), Val::String(list.into())),
            ("id".into(), Val::String(id.into())),
            ("count".into(), Val::S64(count)),
            ("added".into(), Val::S64(at)),
            (
                "note".into(),
                Val::Option(note.map(|n| Box::new(Val::String(n.into())))),
            ),
        ])
    }

    fn key(list: &str, id: &str) -> Val {
        Val::Record(vec![
            ("list".into(), Val::String(list.into())),
            ("id".into(), Val::String(id.into())),
        ])
    }

    fn read(read: Read) -> Access {
        Access::Read(read)
    }

    #[test]
    fn a_composite_get_many_answers_asked_keys_once_in_order() {
        let read = read(items(Primitive::GetMany, &["list", "id"]));
        let asked = [Val::List(vec![key("b", "1"), key("a", "2"), key("a", "1")])];
        let rows = |rows: Vec<Val>| [Val::List(rows)];
        assert_eq!(
            check_answer(
                &read,
                &asked,
                &rows(vec![
                    item("a", "1", 1, None),
                    item("a", "2", 3, Some("x")),
                    item("b", "1", 2, None)
                ])
            ),
            Ok(())
        );
        assert_eq!(check_answer(&read, &asked, &rows(vec![])), Ok(()));
        for (answer, words) in [
            (
                vec![item("a", "2", 1, None), item("a", "1", 1, None)],
                "out of tables.json's order",
            ),
            (
                vec![item("a", "1", 1, None), item("a", "1", 1, None)],
                "answered before",
            ),
            (
                vec![item("c", "1", 1, None)],
                "(\"c\", \"1\") was not asked for",
            ),
            (
                vec![item("a", "1", 0, None)],
                "`count`: 0 is outside its bounds",
            ),
            (
                vec![item("a", "1", 1, Some("x")).clone(), Val::S64(1)],
                "s64 where a row is a record",
            ),
        ] {
            let refused = check_answer(&read, &asked, &rows(answer)).assert_err(words);
            assert!(refused.contains(words), "{words}: {refused}");
        }
        // A key given in another shape than the read's is no key.
        assert!(
            check_answer(
                &read,
                &[Val::List(vec![Val::String("a".into())])],
                &rows(vec![])
            )
            .is_err()
        );
        assert!(
            check_answer(
                &read,
                &[Val::List(vec![Val::Record(vec![
                    ("id".into(), Val::String("1".into())),
                    ("list".into(), Val::String("a".into())),
                ])])],
                &rows(vec![])
            )
            .is_err()
        );
    }

    #[test]
    fn a_composite_get_answers_its_own_key_or_none() {
        let read = read(items(Primitive::Get, &["list", "id"]));
        let asked = [Val::String("a".into()), Val::String("1".into())];
        let one = |row: Val| [Val::Option(Some(Box::new(row)))];
        assert_eq!(check_answer(&read, &asked, &[Val::Option(None)]), Ok(()));
        assert_eq!(
            check_answer(&read, &asked, &one(item("a", "1", 1, None))),
            Ok(())
        );
        let refused =
            check_answer(&read, &asked, &one(item("a", "2", 1, None))).assert_err("other");
        assert!(
            refused.contains("the row of (\"a\", \"2\") where (\"a\", \"1\")"),
            "{refused}"
        );
        assert!(check_answer(&read, &asked, &[Val::List(vec![])]).is_err());
        assert!(check_answer(&read, &asked, &[]).is_err());
        assert!(check_answer(&read, &asked, &[Val::Option(None), Val::Option(None)]).is_err());
    }

    /// ADR-0386: a prefix's rows are of the prefix asked, each once, by the
    /// table's order then its key, not by the key alone.
    #[test]
    fn a_prefix_answers_its_own_prefix_once_by_the_tables_order() {
        let prefix = read(items(Primitive::Prefix, &["list"]));
        let asked = [Val::String("a".into())];
        let rows = |rows: Vec<Val>| [Val::List(rows)];
        // By `added`, then the key: "2" before "1".
        let ok = rows(vec![
            added("a", "2", 1, 1, None),
            added("a", "1", 1, 5, None),
        ]);
        assert_eq!(check_answer(&prefix, &asked, &ok), Ok(()));
        assert_eq!(check_answer(&prefix, &asked, &rows(vec![])), Ok(()));
        for (answer, words) in [
            (
                vec![added("a", "1", 1, 5, None), added("a", "2", 1, 1, None)],
                "out of tables.json's order",
            ),
            (
                vec![added("b", "1", 1, 0, None)],
                "(\"b\") was not asked for",
            ),
            (
                vec![added("a", "1", 1, 0, None), added("a", "1", 1, 0, None)],
                "(\"a\", \"1\") was answered before",
            ),
        ] {
            let refused = check_answer(&prefix, &asked, &rows(answer)).assert_err(words);
            assert!(refused.contains(words), "{words}: {refused}");
        }
        // Its set form: rows of the prefixes asked, grouped by them.
        let many = read(items(Primitive::PrefixMany, &["list"]));
        let asked = [Val::List(vec![
            Val::String("b".into()),
            Val::String("a".into()),
        ])];
        let grouped = rows(vec![
            added("a", "1", 1, 5, None),
            added("b", "2", 1, 1, None),
        ]);
        assert_eq!(check_answer(&many, &asked, &grouped), Ok(()));
        let ungrouped = rows(vec![
            added("b", "2", 1, 1, None),
            added("a", "1", 1, 5, None),
        ]);
        assert!(
            check_answer(&many, &asked, &ungrouped)
                .assert_err("grouped by the prefix first")
                .contains("out of tables.json's order")
        );
    }

    /// ADR-0386: an index's rows hold the value asked; a unique index's read
    /// answers none or the row of that value; `all` answers no more rows than
    /// its table's bound.
    #[test]
    fn index_and_all_answers_hold_their_values_and_bound() {
        let mut done = items(Primitive::Index, &["count"]);
        let asked = [Val::S64(2)];
        let rows = |rows: Vec<Val>| [Val::List(rows)];
        assert_eq!(
            check_answer(
                &read(done.clone()),
                &asked,
                &rows(vec![item("a", "1", 2, None)])
            ),
            Ok(())
        );
        let refused = check_answer(
            &read(done.clone()),
            &asked,
            &rows(vec![item("a", "1", 3, None)]),
        )
        .assert_err("another value's row");
        assert!(refused.contains("(3) was not asked for"), "{refused}");
        done.unique = true;
        done.sorted_by = Vec::new();
        let one = |row: Val| [Val::Option(Some(Box::new(row)))];
        assert_eq!(
            check_answer(&read(done.clone()), &asked, &one(item("a", "1", 2, None))),
            Ok(())
        );
        assert!(
            check_answer(&read(done.clone()), &asked, &one(item("a", "1", 3, None)))
                .assert_err("another value's row")
                .contains("the row of (3) where (2) was asked for")
        );
        let mut all = items(Primitive::All, &[]);
        all.bounded = Some(2);
        let two = rows(vec![item("a", "1", 1, None), item("a", "2", 1, None)]);
        assert_eq!(check_answer(&read(all.clone()), &[], &two), Ok(()));
        let three = rows(vec![
            item("a", "1", 1, None),
            item("a", "2", 1, None),
            item("a", "3", 1, None),
        ]);
        let refused = check_answer(&read(all), &[], &three).assert_err("past the bound");
        assert!(
            refused.contains("3 rows, where Items is bounded to 2"),
            "{refused}"
        );
    }

    fn write(primitive: Primitive, unique: &[&[&str]]) -> Access {
        let row = row();
        let by = match primitive {
            Primitive::Delete => row[..2].to_vec(),
            Primitive::DeletePrefix => row[..1].to_vec(),
            _ => Vec::new(),
        };
        Access::Write(Write {
            import: format!("pw:tables/items#{}", primitive.name()),
            table: "Items".into(),
            primitive,
            by,
            key: vec!["list".into(), "id".into()],
            unique: unique
                .iter()
                .map(|fields| fields.iter().map(|f| f.to_string()).collect())
                .collect(),
            row,
        })
    }

    /// ADR-0386: an insert is refused only by the row of its key or of a
    /// unique value it would hold; a put only by another key's row of a
    /// unique value; a write that cannot be refused answers nothing.
    #[test]
    fn a_writes_refusal_is_the_row_that_holds_what_it_would_write() {
        let refused = |held: Val| [Val::Result(Err(Some(Box::new(held))))];
        let written = [Val::Result(Ok(None))];
        let given = [item("a", "1", 2, None)];
        let insert = write(Primitive::Insert, &[&["count"]]);
        assert_eq!(check_answer(&insert, &given, &written), Ok(()));
        // Its key's row, and another key's row of its unique count.
        assert_eq!(
            check_answer(&insert, &given, &refused(item("a", "1", 7, None))),
            Ok(())
        );
        assert_eq!(
            check_answer(&insert, &given, &refused(item("b", "9", 2, None))),
            Ok(())
        );
        let other = check_answer(&insert, &given, &refused(item("b", "9", 3, None)))
            .assert_err("a row that holds nothing it would");
        assert!(other.contains("holds nothing the insert"), "{other}");
        assert!(check_answer(&insert, &given, &[]).is_err());
        assert!(check_answer(&insert, &given, &[Val::Option(None)]).is_err());
        assert!(
            check_answer(&insert, &given, &refused(Val::S64(1)))
                .assert_err("no row")
                .contains("s64 where a row is a record")
        );
        let put = write(Primitive::Put, &[&["count"]]);
        assert_eq!(check_answer(&put, &given, &written), Ok(()));
        assert_eq!(
            check_answer(&put, &given, &refused(item("b", "9", 2, None))),
            Ok(())
        );
        // A put replaces its key's row: that row refuses nothing.
        assert!(check_answer(&put, &given, &refused(item("a", "1", 2, None))).is_err());
        // Without a unique index, a put and the deletes answer nothing.
        for primitive in [Primitive::Put, Primitive::Delete, Primitive::DeletePrefix] {
            let plain = write(primitive, &[]);
            assert_eq!(check_answer(&plain, &given, &[]), Ok(()));
            assert!(
                check_answer(&plain, &given, &written).is_err(),
                "{primitive:?}"
            );
        }
    }

    #[test]
    fn values_hold_their_stored_type_and_bounds() {
        let bounded = Stored::String {
            min_length: Some(1),
            max_length: Some(2),
        };
        assert_eq!(check_value(&bounded, &Val::String("ab".into())), Ok(()));
        assert!(check_value(&bounded, &Val::String("".into())).is_err());
        assert!(check_value(&bounded, &Val::String("abc".into())).is_err());
        let small = Stored::Int {
            at_least: None,
            at_most: Some(5),
        };
        assert_eq!(check_value(&small, &Val::S64(5)), Ok(()));
        assert!(check_value(&small, &Val::S64(6)).is_err());
        assert!(check_value(&small, &Val::S32(1)).is_err());
        assert_eq!(check_value(&Stored::Float, &Val::Float64(1.5)), Ok(()));
        assert!(check_value(&Stored::Float, &Val::Float64(f64::NAN)).is_err());
        assert!(check_value(&Stored::Float, &Val::Float32(1.5)).is_err());
        let list = Stored::List(Box::new(small.clone()));
        assert_eq!(
            check_value(&list, &Val::List(vec![Val::S64(1), Val::S64(2)])),
            Ok(())
        );
        assert!(check_value(&list, &Val::List(vec![Val::S64(1), Val::S64(9)])).is_err());
        let nested = Stored::Record(vec![crate::tables::Field {
            name: "at_most".into(),
            stored: small,
        }]);
        assert_eq!(
            check_value(&nested, &Val::Record(vec![("at-most".into(), Val::S64(1))])),
            Ok(())
        );
        assert!(check_value(&nested, &Val::Record(vec![("at_most".into(), Val::S64(1))])).is_err());
        assert!(check_value(&Stored::Unit, &Val::Tuple(vec![])).is_err());
    }
}
