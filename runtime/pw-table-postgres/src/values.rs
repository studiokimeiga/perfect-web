//! **A value, to its column and back**: what a statement is given, bound as
//! its column's type, and what it answers, decoded into the row the memory
//! adapter answers (its fields in their declared order, each of its stored
//! type), held to the declaration again before anything reads it.

use postgres::Row;
use postgres::types::ToSql;
use serde_json::{Map, Value};

use crate::Error;
use crate::shape::{Atom, Column, Kind, TableShape, validates};

/// A statement's parameter.
pub(crate) type Param = Box<dyn ToSql + Sync + Send>;

/// The parameters as a statement takes them.
pub(crate) fn refs(params: &[Param]) -> Vec<&(dyn ToSql + Sync)> {
    params
        .iter()
        .map(|p| p.as_ref() as &(dyn ToSql + Sync))
        .collect()
}

/// **A field's value, bound as its column's type**: an `Option`'s none as
/// SQL's `NULL`; a `json` column's value as its JSON text, a `Unit`'s
/// `null` among them (a value, not SQL's `NULL`).
pub(crate) fn bind(column: &Column, value: &Value) -> Result<Param, Error> {
    let wrong = || Error::Input(format!("field {} is not of its column's type", column.name));
    if column.nullable && value.is_null() {
        return Ok(match column.kind {
            Kind::Int => Box::new(None::<i64>),
            Kind::Bytes => Box::new(None::<Vec<u8>>),
            Kind::Bool => Box::new(None::<bool>),
            Kind::Float => Box::new(None::<f64>),
            Kind::Json => Box::new(None::<String>),
        });
    }
    Ok(match column.kind {
        Kind::Int => Box::new(value.as_i64().ok_or_else(wrong)?),
        Kind::Bytes => Box::new(value.as_str().ok_or_else(wrong)?.as_bytes().to_vec()),
        Kind::Bool => Box::new(value.as_bool().ok_or_else(wrong)?),
        Kind::Float => Box::new(value.as_f64().ok_or_else(wrong)?),
        Kind::Json => Box::new(
            serde_json::to_string(value)
                .map_err(|e| Error::Input(format!("{}: {e}", column.name)))?,
        ),
    })
}

/// **A selector's value**, bound as its field's column: a key's, a prefix's
/// or an index's field is a scalar's.
pub(crate) fn bind_atom(column: &Column, atom: &Atom) -> Result<Param, Error> {
    match (column.kind, atom) {
        (Kind::Int, Atom::Int(v)) => Ok(Box::new(*v)),
        (Kind::Bytes, Atom::Str(v)) => Ok(Box::new(v.as_bytes().to_vec())),
        (Kind::Bool, Atom::Bool(v)) => Ok(Box::new(*v)),
        _ => Err(Error::Input(format!(
            "selector {} is not of its column's type",
            column.name
        ))),
    }
}

/// **A set read's values of one field**, bound as one array.
pub(crate) fn bind_array(column: &Column, atoms: &[&Atom]) -> Result<Param, Error> {
    let wrong = || {
        Error::Input(format!(
            "selector {} is not of its column's type",
            column.name
        ))
    };
    Ok(match column.kind {
        Kind::Int => Box::new(
            atoms
                .iter()
                .map(|a| match a {
                    Atom::Int(v) => Ok(*v),
                    _ => Err(wrong()),
                })
                .collect::<Result<Vec<i64>, _>>()?,
        ),
        Kind::Bytes => Box::new(
            atoms
                .iter()
                .map(|a| match a {
                    Atom::Str(v) => Ok(v.as_bytes().to_vec()),
                    _ => Err(wrong()),
                })
                .collect::<Result<Vec<Vec<u8>>, _>>()?,
        ),
        Kind::Bool => Box::new(
            atoms
                .iter()
                .map(|a| match a {
                    Atom::Bool(v) => Ok(*v),
                    _ => Err(wrong()),
                })
                .collect::<Result<Vec<bool>, _>>()?,
        ),
        Kind::Float | Kind::Json => return Err(wrong()),
    })
}

fn stored_wrong(table: &TableShape, column: &Column, why: impl std::fmt::Display) -> Error {
    Error::Schema(format!(
        "a row {} holds is not its declared row: {} {why}",
        table.name, column.name
    ))
}

/// **A row, as a statement answered it** from column `first` on: the row
/// the memory adapter answers, its fields in their declared order, each held
/// to its stored type and bounds; a row stored otherwise (by another writer
/// of the database) fails, never reaching a component.
pub(crate) fn decode(
    table: &TableShape,
    row: &Row,
    first: usize,
) -> Result<Map<String, Value>, Error> {
    let mut out = Map::new();
    for (i, column) in table.columns.iter().enumerate() {
        let at = first + i;
        let value = match column.kind {
            Kind::Int => row
                .try_get::<_, Option<i64>>(at)
                .map_err(|e| stored_wrong(table, column, e))?
                .map(Value::from),
            Kind::Bytes => row
                .try_get::<_, Option<Vec<u8>>>(at)
                .map_err(|e| stored_wrong(table, column, e))?
                .map(|bytes| String::from_utf8(bytes).map(Value::from))
                .transpose()
                .map_err(|e| stored_wrong(table, column, e))?,
            Kind::Bool => row
                .try_get::<_, Option<bool>>(at)
                .map_err(|e| stored_wrong(table, column, e))?
                .map(Value::from),
            Kind::Float => row
                .try_get::<_, Option<f64>>(at)
                .map_err(|e| stored_wrong(table, column, e))?
                .map(Value::from),
            Kind::Json => row
                .try_get::<_, Option<String>>(at)
                .map_err(|e| stored_wrong(table, column, e))?
                .map(|text| serde_json::from_str::<Value>(&text))
                .transpose()
                .map_err(|e| stored_wrong(table, column, e))?,
        };
        let value = match value {
            Some(value) => value,
            None if column.nullable => Value::Null,
            None => return Err(stored_wrong(table, column, "is NULL")),
        };
        if !validates(&value, &column.stored) {
            return Err(stored_wrong(
                table,
                column,
                "fails its stored type or bound",
            ));
        }
        out.insert(column.name.clone(), value);
    }
    Ok(out)
}
