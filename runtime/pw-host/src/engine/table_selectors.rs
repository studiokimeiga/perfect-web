//! Read-only Pleris table selector values at the Wasmtime boundary.
//!
//! This converts generated WIT argument values into the memory adapter's
//! internal JSON representation. It does not define a JSON WIT ABI, authorize
//! an import, or install a Wasmtime host function. The table manifest still
//! decides the allowed selector fields, arity and operation.

use super::Val;
use serde_json::{Map, Value};

fn scalar(value: &Val) -> Result<Value, String> {
    match value {
        Val::S64(n) => Ok(Value::from(*n)),
        Val::String(s) => Ok(Value::from(s.clone())),
        Val::Bool(b) => Ok(Value::from(*b)),
        _ => Err("a table selector key must be a WIT s64, string or bool".into()),
    }
}

fn record(fields: &[(String, Val)]) -> Result<Value, String> {
    let mut object = Map::new();
    for (name, value) in fields {
        // WIT uses kebab-case; tables.json names the Pleris source fields.
        // Never let two names collapse into one selector during conversion.
        let name = name.replace('-', "_");
        if object.contains_key(&name) {
            return Err(format!("duplicate table selector field `{name}`"));
        }
        object.insert(name, scalar(value)?);
    }
    Ok(Value::Object(object))
}

fn argument(value: &Val) -> Result<Value, String> {
    match value {
        Val::List(values) => {
            // A get-many argument is a list of scalar keys or flat key records.
            // Nested containers, options, variants and numeric coercions are
            // not key values. Reject rather than silently dropping them.
            let values = values
                .iter()
                .map(|value| match value {
                    Val::Record(fields) => record(fields),
                    _ => scalar(value),
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Value::Array(values))
        }
        Val::Record(fields) => record(fields),
        _ => scalar(value),
    }
}

/// Decode positional read arguments without inventing missing or unsupported
/// values. The caller must first select a manifest- and admission-granted
/// get/get-many import, then pass this result to the table adapter, which
/// validates the selector's declared arity, fields and stored types.
pub fn decode_read_arguments(args: &[Val]) -> Result<Vec<Value>, String> {
    args.iter()
        .enumerate()
        .map(|(index, value)| {
            argument(value).map_err(|reason| format!("table read argument {}: {reason}", index + 1))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn get_keeps_scalar_key_values_and_types() {
        assert_eq!(
            decode_read_arguments(&[Val::S64(-7), Val::Bool(false), Val::String("item-1".into()),]),
            Ok(vec![json!(-7), json!(false), json!("item-1")])
        );
    }

    #[test]
    fn get_many_keeps_composite_records_and_normalizes_wit_field_names() {
        let args = [Val::List(vec![
            Val::Record(vec![
                ("list-id".into(), Val::String("first".into())),
                ("item-id".into(), Val::S64(3)),
            ]),
            Val::Record(vec![
                ("list-id".into(), Val::String("second".into())),
                ("item-id".into(), Val::S64(4)),
            ]),
        ])];
        assert_eq!(
            decode_read_arguments(&args),
            Ok(vec![json!([
                {"list_id": "first", "item_id": 3},
                {"list_id": "second", "item_id": 4}
            ])])
        );
        assert_eq!(
            decode_read_arguments(&[Val::List(vec![])]),
            Ok(vec![json!([])])
        );
    }

    #[test]
    fn refuses_unsupported_key_shapes_without_defaults_or_coercion() {
        for unsupported in [
            Val::S32(1),
            Val::Float64(2.5),
            Val::Option(None),
            Val::Option(Some(Box::new(Val::String("key".into())))),
            Val::Tuple(vec![Val::S64(1)]),
            Val::List(vec![Val::List(vec![Val::S64(1)])]),
            Val::List(vec![Val::Record(vec![("key".into(), Val::Option(None))])]),
        ] {
            assert!(decode_read_arguments(&[unsupported]).is_err());
        }
        // A neighboring valid read remains accepted after every refusal.
        assert_eq!(
            decode_read_arguments(&[Val::String("key".into())]),
            Ok(vec![json!("key")])
        );
    }

    #[test]
    fn refuses_duplicate_record_fields_even_after_wit_normalization() {
        for fields in [
            vec![
                ("list-id".into(), Val::S64(1)),
                ("list_id".into(), Val::S64(2)),
            ],
            vec![
                ("list-id".into(), Val::S64(1)),
                ("list-id".into(), Val::S64(2)),
            ],
        ] {
            assert!(matches!(
                decode_read_arguments(&[Val::List(vec![Val::Record(fields)])]),
                Err(reason) if reason.contains("duplicate table selector field")
            ));
        }
    }
}
