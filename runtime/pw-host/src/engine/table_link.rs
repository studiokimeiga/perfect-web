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
