//! Encode a table adapter's read results as Wasmtime component values.
//!
//! This is an internal JSON-to-WIT boundary, not a JSON public ABI. A row is
//! encoded as the record tables.json declares (the integrator's rulings 6 and
//! 8 of 2026-10-25): its fields in their declared order, by their WIT names,
//! each by its stored type (an `Int` s64, a `String`, a `Bool`, a `Float`
//! f64, an `Option`, a `List`, a record) and within its bounds, no field
//! more and none less. Nothing depends on the order of a JSON object's keys,
//! and nothing is guessed: a row that is not its table's is refused whole.
//! The engine then holds the encoded answer to the read again
//! (`table_link::check_answer`), whoever encoded it.

use serde_json::Value;

use super::Val;
use super::table_link::check_value;
use crate::tables::{Column, Read, Stored};

fn row(read: &Read, value: &Value) -> Result<Val, String> {
    record(&read.row, value).map_err(|why| format!("a {} row: {why}", read.table))
}

/// A record of exactly these fields, in this order.
fn record(columns: &[Column], value: &Value) -> Result<Val, String> {
    let object = value
        .as_object()
        .ok_or_else(|| "a table read row must be a record".to_string())?;
    if let Some(extra) = object
        .keys()
        .find(|name| !columns.iter().any(|column| &column.name == *name))
    {
        return Err(format!(
            "it has `{extra}`, which tables.json does not declare"
        ));
    }
    let mut fields = Vec::with_capacity(columns.len());
    for column in columns {
        let value = object
            .get(&column.name)
            .ok_or_else(|| format!("it lacks `{}`", column.name))?;
        let encoded =
            encode(&column.stored, value).map_err(|why| format!("`{}`: {why}", column.name))?;
        fields.push((column.wit.clone(), encoded));
    }
    Ok(Val::Record(fields))
}

/// One value, by its stored type, and within its bounds.
fn encode(stored: &Stored, value: &Value) -> Result<Val, String> {
    let encoded = match (stored, value) {
        (Stored::Int { .. }, Value::Number(n)) => {
            Val::S64(n.as_i64().ok_or_else(|| format!("{n} is not a WIT s64"))?)
        }
        (Stored::String { .. }, Value::String(s)) => Val::String(s.clone()),
        (Stored::Bool, Value::Bool(b)) => Val::Bool(*b),
        (Stored::Float, Value::Number(n)) => {
            Val::Float64(n.as_f64().ok_or_else(|| format!("{n} is not a WIT f64"))?)
        }
        (Stored::Option(_), Value::Null) => Val::Option(None),
        (Stored::Option(inner), value) => Val::Option(Some(Box::new(encode(inner, value)?))),
        (Stored::List(inner), Value::Array(items)) => Val::List(
            items
                .iter()
                .map(|item| encode(inner, item))
                .collect::<Result<_, _>>()?,
        ),
        (Stored::Record(fields), value) => {
            record(&fields.iter().map(Column::of).collect::<Vec<_>>(), value)?
        }
        (stored, value) => {
            return Err(format!(
                "{} where tables.json declares {}",
                json_named(value),
                super::table_link::described(stored)
            ));
        }
    };
    check_value(stored, &encoded)?;
    Ok(encoded)
}

fn json_named(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}

/// Encode a single-key get: a missing row is WIT none; a present row is some.
pub fn encode_get(read: &Read, value: &Value) -> Result<Val, String> {
    if value.is_null() {
        Ok(Val::Option(None))
    } else {
        Ok(Val::Option(Some(Box::new(row(read, value)?))))
    }
}

/// Encode get-many in the exact order returned by the adapter.
/// An invalid element refuses the entire result, never dropping that row.
pub fn encode_get_many(read: &Read, value: &Value) -> Result<Val, String> {
    let rows = value
        .as_array()
        .ok_or_else(|| "a get-many result must be a list".to_string())?;
    Ok(Val::List(
        rows.iter()
            .map(|value| row(read, value))
            .collect::<Result<_, _>>()?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A table whose row is declared `id, active, sort_order, note`: not the
    /// order of a JSON object's keys (`active, id, note, sort_order`).
    fn read() -> Read {
        let row = vec![
            Column::new(
                "id",
                Stored::String {
                    min_length: None,
                    max_length: Some(8),
                },
            ),
            Column::new("active", Stored::Bool),
            Column::new(
                "sort_order",
                Stored::Int {
                    at_least: Some(-5),
                    at_most: None,
                },
            ),
            Column::new(
                "note",
                Stored::Option(Box::new(Stored::String {
                    min_length: None,
                    max_length: None,
                })),
            ),
        ];
        Read {
            import: "pw:tables/likes#get".into(),
            table: "Likes".into(),
            many: false,
            by: vec![row[0].clone()],
            key: vec!["id".into()],
            row,
            order: None,
            sorted_by: Vec::new(),
        }
    }

    fn record(id: &str, note: Option<&str>) -> Val {
        Val::Record(vec![
            ("id".into(), Val::String(id.into())),
            ("active".into(), Val::Bool(true)),
            ("sort-order".into(), Val::S64(-3)),
            (
                "note".into(),
                Val::Option(note.map(|n| Box::new(Val::String(n.into())))),
            ),
        ])
    }

    fn stored(id: &str) -> Value {
        json!({"id": id, "active": true, "sort_order": -3, "note": null})
    }

    #[test]
    fn get_preserves_present_and_missing_option_rows() {
        assert_eq!(encode_get(&read(), &Value::Null), Ok(Val::Option(None)));
        assert_eq!(
            encode_get(&read(), &stored("first")),
            Ok(Val::Option(Some(Box::new(record("first", None)))))
        );
        assert_eq!(
            encode_get(
                &read(),
                &json!({"note": "kept", "sort_order": -3, "active": true, "id": "noted"})
            ),
            Ok(Val::Option(Some(Box::new(record("noted", Some("kept"))))))
        );
    }

    /// The integrator's ruling 6: the fields in their declared order, whatever
    /// the order of the JSON object's keys (serde's map orders them by name).
    #[test]
    fn rows_are_records_in_their_declared_order() {
        let Ok(Val::Option(Some(row))) = encode_get(&read(), &stored("first")) else {
            panic!("a present row");
        };
        let Val::Record(fields) = *row else {
            panic!("a record");
        };
        let names: Vec<&str> = fields.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, ["id", "active", "sort-order", "note"]);
    }

    #[test]
    fn get_many_preserves_bounded_adapter_order_and_empty_lists() {
        assert_eq!(
            encode_get_many(&read(), &json!([stored("second"), stored("first")])),
            Ok(Val::List(vec![
                record("second", None),
                record("first", None)
            ]))
        );
        assert_eq!(encode_get_many(&read(), &json!([])), Ok(Val::List(vec![])));
    }

    #[test]
    fn unsupported_and_wrong_result_shapes_fail_closed() {
        let with = |field: &str, value: Value| {
            let mut row = stored("first");
            row[field] = value;
            row
        };
        for invalid in [
            json!(12),
            with("id", json!(1)),
            with("id", json!(null)),
            with("id", json!("too-long-for-it")),
            with("sort_order", json!(1.5)),
            with("sort_order", json!(u64::MAX)),
            with("sort_order", json!(-6)),
            with("active", json!("true")),
            with("note", json!(["a"])),
        ] {
            assert!(encode_get(&read(), &invalid).is_err(), "{invalid}");
        }
        assert!(encode_get_many(&read(), &Value::Null).is_err());
        assert!(encode_get_many(&read(), &json!([stored("ok"), {"id": []}])).is_err());
    }

    /// The integrator's ruling 8: a row with a field its table does not
    /// declare (`secret`), or without one it does, is refused whole, never
    /// trimmed or filled.
    #[test]
    fn a_row_with_more_or_fewer_fields_than_declared_is_refused() {
        let mut secret = stored("first");
        secret["secret"] = json!("leak");
        let refused = encode_get(&read(), &secret).expect_err("an extra field");
        assert!(refused.contains("`secret`"), "{refused}");
        let mut lacking = stored("first");
        lacking.as_object_mut().unwrap().remove("note");
        let refused = encode_get(&read(), &lacking).expect_err("a missing field");
        assert!(refused.contains("lacks `note`"), "{refused}");
        let collision =
            json!({"id": "x", "active": true, "sort_order": 1, "sort-order": 1, "note": null});
        assert!(encode_get(&read(), &collision).is_err());
        // The neighbour, after every refusal.
        assert_eq!(
            encode_get(&read(), &stored("still-ok")),
            Ok(Val::Option(Some(Box::new(record("still-ok", None)))))
        );
    }
}
