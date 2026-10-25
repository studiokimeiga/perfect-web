//! Encode the memory table adapter's read results as Wasmtime component values.
//!
//! This is an internal JSON-to-WIT boundary, not a JSON public ABI. Only flat
//! rows with s64, string, and boolean fields are supported in this first slice.
//! The caller still must select a manifest-declared, admitted get/get-many
//! import and let Wasmtime check the declared result type at invocation.

use std::collections::BTreeSet;

use super::Val;
use serde_json::Value;

fn row(value: &Value) -> Result<Val, String> {
    let object = value
        .as_object()
        .ok_or_else(|| "a table read row must be a record".to_string())?;
    if object.is_empty() {
        return Err("a table read row has no fields".into());
    }

    let mut names = BTreeSet::new();
    let mut fields = Vec::with_capacity(object.len());
    for (name, value) in object {
        // Pleris schema fields use underscores; generated WIT uses hyphens.
        // Refuse collisions instead of allowing two fields to become one.
        let wit_name = name.replace('_', "-");
        if !names.insert(wit_name.clone()) {
            return Err(format!("duplicate table result field {wit_name}"));
        }
        let encoded = match value {
            Value::String(s) => Val::String(s.clone()),
            Value::Bool(b) => Val::Bool(*b),
            Value::Number(n) => Val::S64(
                n.as_i64()
                    .ok_or_else(|| format!("table result field {name} is not a WIT s64"))?,
            ),
            _ => {
                return Err(format!(
                    "table result field {name} needs an unsupported WIT value"
                ));
            }
        };
        fields.push((wit_name, encoded));
    }
    Ok(Val::Record(fields))
}

/// Encode a single-key get: a missing row is WIT none; a present row is some.
/// This does not guess defaults for unsupported row fields.
pub fn encode_get(value: &Value) -> Result<Val, String> {
    if value.is_null() {
        Ok(Val::Option(None))
    } else {
        Ok(Val::Option(Some(Box::new(row(value)?))))
    }
}

/// Encode get-many in the exact order returned by the memory adapter.
/// An invalid element refuses the entire result, never dropping that row.
pub fn encode_get_many(value: &Value) -> Result<Val, String> {
    let rows = value
        .as_array()
        .ok_or_else(|| "a get-many result must be a list".to_string())?;
    Ok(Val::List(rows.iter().map(row).collect::<Result<_, _>>()?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn record(id: &str) -> Val {
        Val::Record(vec![
            ("active".into(), Val::Bool(true)),
            ("id".into(), Val::String(id.into())),
            ("sort-order".into(), Val::S64(-3)),
        ])
    }

    fn stored(id: &str) -> Value {
        json!({"id": id, "active": true, "sort_order": -3})
    }

    #[test]
    fn get_preserves_present_and_missing_option_rows() {
        assert_eq!(encode_get(&Value::Null), Ok(Val::Option(None)));
        assert_eq!(
            encode_get(&stored("first")),
            Ok(Val::Option(Some(Box::new(record("first")))))
        );
    }

    #[test]
    fn get_many_preserves_bounded_adapter_order_and_empty_lists() {
        assert_eq!(
            encode_get_many(&json!([stored("second"), stored("first")])),
            Ok(Val::List(vec![record("second"), record("first")]))
        );
        assert_eq!(encode_get_many(&json!([])), Ok(Val::List(vec![])));
    }

    #[test]
    fn unsupported_and_wrong_result_shapes_fail_closed() {
        for invalid in [
            json!(12),
            json!({"id": 1.5}),
            json!({"id": null}),
            json!({"id": [1]}),
            json!({"id": {"nested": true}}),
            json!({"id": u64::MAX}),
        ] {
            assert!(encode_get(&invalid).is_err(), "{invalid}");
        }
        assert!(encode_get_many(&Value::Null).is_err());
        assert!(encode_get_many(&json!([stored("ok"), {"id": []}])).is_err());
    }

    #[test]
    fn normalized_name_collisions_are_refused_without_poisoning_other_reads() {
        let collision = json!({"list_id": "one", "list-id": "two"});
        assert!(matches!(
            encode_get(&collision),
            Err(why) if why.contains("duplicate table result field")
        ));
        assert_eq!(
            encode_get(&stored("still-valid")),
            Ok(Val::Option(Some(Box::new(record("still-valid")))))
        );
    }
}
