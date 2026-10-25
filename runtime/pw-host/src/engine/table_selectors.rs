//! Read-only Pleris table selector values at the Wasmtime boundary.
//!
//! This converts generated WIT argument values into the memory adapter's
//! internal JSON representation. It does not define a JSON WIT ABI, authorize
//! an import, or install a Wasmtime host function.
//!
//! **Each key is held to tables.json before any adapter reads it** (the
//! integrator's ruling 5 of 2026-10-25, defence in depth): its arity, each
//! field's stored type and bounds, and a composite key's fields by their WIT
//! names, mapped back to the Pleris names the adapter's rows use. A key of
//! another type is refused, never read as a key no row has.

use serde_json::{Map, Value};

use super::Val;
use super::table_link::check_value;
use crate::tables::{Column, Read};

/// One key field's value, of its column's stored type and within its bounds.
fn scalar(column: &Column, value: &Val) -> Result<Value, String> {
    check_value(&column.stored, value)
        .map_err(|why| format!("key field `{}`: {why}", column.name))?;
    match value {
        Val::S64(n) => Ok(Value::from(*n)),
        Val::String(s) => Ok(Value::from(s.clone())),
        Val::Bool(b) => Ok(Value::from(*b)),
        _ => Err(format!(
            "key field `{}`: a key is a WIT s64, string or bool",
            column.name
        )),
    }
}

/// One key as a get-many's list holds it: its one field's value, or a record
/// of its fields, by their WIT names, in the key's order.
fn key(read: &Read, value: &Val) -> Result<Value, String> {
    match (read.by.as_slice(), value) {
        ([column], value) => scalar(column, value),
        (columns, Val::Record(fields)) => {
            if fields.len() != columns.len() {
                return Err(format!(
                    "a key of {} field(s) where tables.json declares {}",
                    fields.len(),
                    columns.len()
                ));
            }
            let mut object = Map::new();
            for ((name, value), column) in fields.iter().zip(columns) {
                // WIT uses kebab-case; tables.json names the Pleris fields.
                // Each field by the name the key declares, in its place, so
                // that no two names can collapse into one selector.
                if *name != column.wit {
                    return Err(format!(
                        "a key's field `{name}` where tables.json declares `{}`",
                        column.wit
                    ));
                }
                object.insert(column.name.clone(), scalar(column, value)?);
            }
            Ok(Value::Object(object))
        }
        _ => Err("a composite key is a record of its fields".into()),
    }
}

/// Decode a read's positional arguments, held to its key, without inventing
/// missing or unsupported values: a get's key fields, or a get-many's one
/// list of keys. The adapter then reads by them, and checks them again.
pub fn decode_read_arguments(read: &Read, args: &[Val]) -> Result<Vec<Value>, String> {
    if read.many {
        let [Val::List(keys)] = args else {
            return Err("a get-many takes one list of keys".into());
        };
        let keys = keys
            .iter()
            .enumerate()
            .map(|(index, value)| {
                key(read, value).map_err(|why| format!("table read key {}: {why}", index + 1))
            })
            .collect::<Result<Vec<_>, _>>()?;
        return Ok(vec![Value::Array(keys)]);
    }
    if args.len() != read.by.len() {
        return Err(format!(
            "a get takes its key's {} field(s), and was given {}",
            read.by.len(),
            args.len()
        ));
    }
    args.iter()
        .zip(&read.by)
        .enumerate()
        .map(|(index, (value, column))| {
            scalar(column, value)
                .map_err(|reason| format!("table read argument {}: {reason}", index + 1))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tables::Stored;
    use serde_json::json;

    fn string(max: Option<i64>) -> Stored {
        Stored::String {
            min_length: None,
            max_length: max,
        }
    }

    fn int(least: Option<i64>) -> Stored {
        Stored::Int {
            at_least: least,
            at_most: None,
        }
    }

    /// A read keyed by `key`, its row `key` and a title.
    fn read(many: bool, key: &[(&str, Stored)]) -> Read {
        let by: Vec<Column> = key
            .iter()
            .map(|(name, stored)| Column::new(name, stored.clone()))
            .collect();
        let mut row = by.clone();
        row.push(Column::new("title", string(None)));
        Read {
            import: "pw:tables/items#get".into(),
            table: "Items".into(),
            many,
            key: by.iter().map(|c| c.name.clone()).collect(),
            by,
            row,
            order: None,
            sorted_by: Vec::new(),
        }
    }

    #[test]
    fn get_keeps_scalar_key_values_and_types() {
        let get = read(
            false,
            &[
                ("rank", int(None)),
                ("done", Stored::Bool),
                ("id", string(None)),
            ],
        );
        assert_eq!(
            decode_read_arguments(
                &get,
                &[Val::S64(-7), Val::Bool(false), Val::String("item-1".into())]
            ),
            Ok(vec![json!(-7), json!(false), json!("item-1")])
        );
    }

    #[test]
    fn get_many_keeps_composite_records_by_the_keys_own_names() {
        let get_many = read(true, &[("list_id", string(None)), ("item_id", int(None))]);
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
            decode_read_arguments(&get_many, &args),
            Ok(vec![json!([
                {"list_id": "first", "item_id": 3},
                {"list_id": "second", "item_id": 4}
            ])])
        );
        assert_eq!(
            decode_read_arguments(&get_many, &[Val::List(vec![])]),
            Ok(vec![json!([])])
        );
    }

    #[test]
    fn refuses_unsupported_key_shapes_without_defaults_or_coercion() {
        let get = read(false, &[("id", string(None))]);
        for unsupported in [
            Val::S32(1),
            Val::Float64(2.5),
            Val::Option(None),
            Val::Option(Some(Box::new(Val::String("key".into())))),
            Val::Tuple(vec![Val::S64(1)]),
            Val::List(vec![Val::List(vec![Val::S64(1)])]),
            Val::Record(vec![("id".into(), Val::String("key".into()))]),
        ] {
            assert!(decode_read_arguments(&get, &[unsupported]).is_err());
        }
        let get_many = read(true, &[("id", string(None))]);
        assert!(
            decode_read_arguments(
                &get_many,
                &[Val::List(vec![Val::Record(vec![(
                    "id".into(),
                    Val::Option(None)
                )])])]
            )
            .is_err()
        );
        // A neighbouring valid read remains accepted after every refusal.
        assert_eq!(
            decode_read_arguments(&get, &[Val::String("key".into())]),
            Ok(vec![json!("key")])
        );
    }

    /// The integrator's ruling 5: a WIT-valid key of another type than the
    /// key's declared field, or outside its bounds, is refused before any
    /// adapter could read it as a key no row has.
    #[test]
    fn refuses_keys_of_another_declared_type_or_out_of_bounds() {
        let get = read(false, &[("id", string(Some(3)))]);
        for (value, words) in [
            (Val::Bool(true), "bool where tables.json declares a String"),
            (Val::S64(1), "s64 where tables.json declares a String"),
            (Val::String("four".into()), "outside its bounds"),
        ] {
            let refused = decode_read_arguments(&get, &[value]).expect_err(words);
            assert!(refused.contains(words), "{refused}");
        }
        let get = read(false, &[("count", int(Some(1)))]);
        assert!(decode_read_arguments(&get, &[Val::String("1".into())]).is_err());
        assert!(decode_read_arguments(&get, &[Val::S64(0)]).is_err());
        let get_many = read(true, &[("id", string(Some(3)))]);
        assert!(decode_read_arguments(&get_many, &[Val::List(vec![Val::Bool(false)])]).is_err());
        // The neighbours: within the declared type and bounds.
        assert_eq!(
            decode_read_arguments(&get, &[Val::S64(1)]),
            Ok(vec![json!(1)])
        );
        assert_eq!(
            decode_read_arguments(&get_many, &[Val::List(vec![Val::String("abc".into())])]),
            Ok(vec![json!(["abc"])])
        );
    }

    #[test]
    fn refuses_a_key_of_other_arity_or_field_names() {
        // Two fields of one type, given by each other's names: only the
        // names tell them apart, and they are refused, not swapped.
        let pair = read(true, &[("owner", string(None)), ("store", string(None))]);
        let swapped = Val::Record(vec![
            ("store".into(), Val::String("s".into())),
            ("owner".into(), Val::String("o".into())),
        ]);
        assert!(decode_read_arguments(&pair, &[Val::List(vec![swapped])]).is_err());
        let named = Val::Record(vec![
            ("owner".into(), Val::String("o".into())),
            ("store".into(), Val::String("s".into())),
        ]);
        assert_eq!(
            decode_read_arguments(&pair, &[Val::List(vec![named])]),
            Ok(vec![json!([{"owner": "o", "store": "s"}])])
        );

        let get = read(false, &[("id", string(None))]);
        assert!(decode_read_arguments(&get, &[]).is_err());
        assert!(
            decode_read_arguments(&get, &[Val::String("a".into()), Val::String("b".into())])
                .is_err()
        );
        let get_many = read(true, &[("list_id", string(None)), ("item_id", int(None))]);
        for fields in [
            vec![
                ("list-id".into(), Val::String("a".into())),
                ("list_id".into(), Val::S64(2)),
            ],
            vec![
                ("list-id".into(), Val::String("a".into())),
                ("list-id".into(), Val::S64(2)),
            ],
            vec![
                ("item-id".into(), Val::S64(2)),
                ("list-id".into(), Val::String("a".into())),
            ],
            vec![("list-id".into(), Val::String("a".into()))],
        ] {
            assert!(
                decode_read_arguments(&get_many, &[Val::List(vec![Val::Record(fields)])]).is_err()
            );
        }
        assert!(decode_read_arguments(&get_many, &[Val::List(vec![Val::S64(1)])]).is_err());
        assert!(decode_read_arguments(&get_many, &[]).is_err());
    }
}
