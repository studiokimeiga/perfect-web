//! **A table read, held to tables.json where it is linked** (ADR-0338 D2;
//! the integrator's ruling 4 of 2026-10-25).
//!
//! The engine links a function for each import with the type the component
//! declares (`func_new`), so a table import typed otherwise than tables.json
//! declares its read linked, and failed, if at all, only when it was called.
//! A component is compiled from the same declaration as its tables.json, so
//! a difference means one of the two is not what was built with the other:
//! a stale build, or a forged one. [`check_import`] compares the import's
//! type, read from the component, with the type tables.json gives the read,
//! before anything is instantiated, and refuses there.
//!
//! **And each answer, when it is given** (the integrator's rulings 7 and 8):
//! whatever answers a table read, the memory adapter's callback or any
//! other, its answer reaches the component only if it is the read's:
//! [`check_answer`] holds every row to the row tables.json declares, exactly
//! (no field more, none less, each of its type and within its bounds: an
//! answer is not projected onto the type, as ADR-0166 projects a host's
//! records), a `get`'s row to the key asked for, and a `get-many`'s rows to
//! keys asked for, each once, in tables.json's order.

use std::collections::BTreeSet;

use wasmtime::component::Val;
use wasmtime::component::types::{ComponentFunc, Type};

use crate::tables::{Column, Read, Stored};

/// **Is this import typed as tables.json declares its read?** A `get` takes
/// its key's fields and answers an option of its row; a `get-many` takes a
/// list of keys (a key's one field, or a record of its fields) and answers a
/// list of rows; a row is a record of the table's fields, in their declared
/// order, by their WIT names, each of its stored type.
pub fn check_import(read: &Read, func: &ComponentFunc) -> Result<(), String> {
    let params: Vec<Type> = func.params().map(|(_, ty)| ty).collect();
    let results: Vec<Type> = func.results().collect();
    let refused = |why: String| {
        format!(
            "`{}` is typed otherwise than tables.json declares the read: {why}",
            read.import
        )
    };
    if read.many {
        let [Type::List(keys)] = params.as_slice() else {
            return Err(refused(format!(
                "it takes ({}), not one list of keys",
                listed(&params)
            )));
        };
        key(read, &keys.ty()).map_err(refused)?;
        let [Type::List(rows)] = results.as_slice() else {
            return Err(refused(format!(
                "it answers ({}), not a list of rows",
                listed(&results)
            )));
        };
        record(&read.row, &rows.ty()).map_err(refused)?;
    } else {
        if params.len() != read.by.len() {
            return Err(refused(format!(
                "it takes ({}), not the key's {} field(s)",
                listed(&params),
                read.by.len()
            )));
        }
        for (ty, column) in params.iter().zip(&read.by) {
            stored(&column.stored, ty)
                .map_err(|why| refused(format!("its key's `{}`: {why}", column.name)))?;
        }
        let [Type::Option(row)] = results.as_slice() else {
            return Err(refused(format!(
                "it answers ({}), not an option of a row",
                listed(&results)
            )));
        };
        record(&read.row, &row.ty()).map_err(refused)?;
    }
    Ok(())
}

/// A key as a `get-many`'s list holds it: its one field's value, or a record
/// of its fields.
fn key(read: &Read, ty: &Type) -> Result<(), String> {
    match read.by.as_slice() {
        [column] => {
            stored(&column.stored, ty).map_err(|why| format!("its key's `{}`: {why}", column.name))
        }
        columns => record(columns, ty).map_err(|why| format!("its key: {why}")),
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
            "{} is not a type the table host reads yet",
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

/// **Is this answer the read's?** A `get` answers none, or the row of the
/// key it was given; a `get-many` answers rows of keys it was given, each
/// key once, in tables.json's order ([`Read::canonical`]); every row is
/// exactly the row tables.json declares ([`exact_row`]). A key missing from
/// the table is left out of a `get-many`, and an empty list is an answer.
pub fn check_answer(read: &Read, args: &[Val], results: &[Val]) -> Result<(), String> {
    let [answer] = results else {
        return Err(format!("{} values where a read answers one", results.len()));
    };
    if !read.many {
        return match answer {
            Val::Option(None) => Ok(()),
            Val::Option(Some(row)) => {
                let fields = exact_row(read, row)?;
                let key = atoms(read, fields, read.by.iter().map(|c| c.name.as_str()))?;
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
                "{} where a get answers an option of a row",
                val_named(other)
            )),
        };
    }
    let Val::List(rows) = answer else {
        return Err(format!(
            "{} where a get-many answers a list of rows",
            val_named(answer)
        ));
    };
    let asked = asked_keys(read, args)?;
    let order = read.canonical();
    let mut seen = BTreeSet::new();
    let mut last: Option<Vec<Atom>> = None;
    for (index, row) in rows.iter().enumerate() {
        let at = |why: String| format!("row {}: {why}", index + 1);
        let fields = exact_row(read, row).map_err(at)?;
        let key = atoms(read, fields, read.by.iter().map(|c| c.name.as_str())).map_err(at)?;
        if !asked.contains(&key) {
            return Err(at(format!("{} was not asked for", shown(&key))));
        }
        if !seen.insert(key.clone()) {
            return Err(at(format!("{} was answered before", shown(&key))));
        }
        let place = atoms(read, fields, order.iter().map(String::as_str)).map_err(at)?;
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

/// **A row, exactly as tables.json declares it**: a record of the table's
/// fields in their declared order, by their WIT names, no field more and
/// none less, each value of its stored type and within its bounds. Its
/// fields, for reading by position.
pub fn exact_row<'a>(read: &Read, row: &'a Val) -> Result<&'a [(String, Val)], String> {
    let Val::Record(fields) = row else {
        return Err(format!("{} where a row is a record", val_named(row)));
    };
    exact(&read.row, fields)?;
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
    read: &Read,
    fields: &[(String, Val)],
    names: impl Iterator<Item = &'a str>,
) -> Result<Vec<Atom>, String> {
    names
        .map(|name| {
            let at = read
                .row
                .iter()
                .position(|column| column.name == name)
                .ok_or_else(|| format!("`{name}` is no field of the row"))?;
            atom(&fields[at].1)
        })
        .collect()
}

/// The keys a `get-many` was given: each its one field's value, or a record
/// of its fields by their WIT names.
fn asked_keys(read: &Read, args: &[Val]) -> Result<BTreeSet<Vec<Atom>>, String> {
    let [Val::List(keys)] = args else {
        return Err("a get-many is given one list of keys".into());
    };
    keys.iter()
        .map(|key| match (read.by.as_slice(), key) {
            ([_], key) => Ok(vec![atom(key)?]),
            (columns, Val::Record(fields)) if fields.len() == columns.len() => columns
                .iter()
                .zip(fields)
                .map(|(column, (name, value))| {
                    if *name == column.wit {
                        atom(value)
                    } else {
                        Err(format!(
                            "a key's `{name}` where `{}` is declared",
                            column.wit
                        ))
                    }
                })
                .collect(),
            (_, other) => Err(format!("{} where a key is a record", val_named(other))),
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

    /// Items, keyed by `list` then `id`, ordered by `added`; its `count` at
    /// least 1 and its `note` optional. A get-many's answers come by its key's
    /// fields, then `added`, then the key.
    fn items(many: bool) -> Read {
        let row = vec![
            Column::new("list", string()),
            Column::new("id", string()),
            Column::new(
                "count",
                Stored::Int {
                    at_least: Some(1),
                    at_most: None,
                },
            ),
            Column::new(
                "added",
                Stored::Int {
                    at_least: None,
                    at_most: None,
                },
            ),
            Column::new("note", Stored::Option(Box::new(string()))),
        ];
        Read {
            import: "pw:tables/items#get-many".into(),
            table: "Items".into(),
            many,
            by: row[..2].to_vec(),
            key: vec!["list".into(), "id".into()],
            order: Some("added".into()),
            sorted_by: vec!["list".into(), "id".into(), "added".into()],
            row,
        }
    }

    fn item(list: &str, id: &str, count: i64, note: Option<&str>) -> Val {
        Val::Record(vec![
            ("list".into(), Val::String(list.into())),
            ("id".into(), Val::String(id.into())),
            ("count".into(), Val::S64(count)),
            ("added".into(), Val::S64(0)),
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

    #[test]
    fn a_composite_get_many_answers_asked_keys_once_in_order() {
        let read = items(true);
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
            let refused = check_answer(&read, &asked, &rows(answer)).expect_err(words);
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
        let read = items(false);
        let asked = [Val::String("a".into()), Val::String("1".into())];
        let one = |row: Val| [Val::Option(Some(Box::new(row)))];
        assert_eq!(check_answer(&read, &asked, &[Val::Option(None)]), Ok(()));
        assert_eq!(
            check_answer(&read, &asked, &one(item("a", "1", 1, None))),
            Ok(())
        );
        let refused =
            check_answer(&read, &asked, &one(item("a", "2", 1, None))).expect_err("other");
        assert!(
            refused.contains("the row of (\"a\", \"2\") where (\"a\", \"1\")"),
            "{refused}"
        );
        assert!(check_answer(&read, &asked, &[Val::List(vec![])]).is_err());
        assert!(check_answer(&read, &asked, &[]).is_err());
        assert!(check_answer(&read, &asked, &[Val::Option(None), Val::Option(None)]).is_err());
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
