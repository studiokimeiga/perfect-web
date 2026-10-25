//! Table selector and row values at the Wasmtime boundary.
//!
//! This converts generated WIT argument values into the memory adapter's
//! internal JSON representation. It does not define a JSON WIT ABI, authorize
//! an import, or install a Wasmtime host function.
//!
//! **What a function is given is held to tables.json before any adapter
//! reads it** (the integrator's ruling 5 of 2026-10-25, defence in depth;
//! ADR-0386 for the writes): a selector's arity, each field's stored type and
//! bounds, a composite selector's fields by their WIT names, mapped back to
//! the Pleris names the adapter's rows use; a row exactly its table's, as
//! the engine holds an answered one (`table_link::exact_row`). A selector of
//! another type is refused, never read as a key no row has, and a row of
//! another shape is refused, never stored.

use serde_json::{Map, Value};

use super::Val;
use super::table_link::{check_value, exact_row};
use crate::tables::{Column, Primitive, Read, Stored, Write};

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

/// One selector as a set form's list holds it: its one field's value, or a
/// record of its fields, by their WIT names, in their declared order.
fn key(columns: &[Column], value: &Val) -> Result<Value, String> {
    match (columns, value) {
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

/// The positional fields a read or a delete is given, each its column's.
fn positional(primitive: Primitive, by: &[Column], args: &[Val]) -> Result<Vec<Value>, String> {
    if args.len() != by.len() {
        return Err(format!(
            "a {} takes its {}'s {} field(s), and was given {}",
            primitive.name(),
            primitive.by_words(),
            by.len(),
            args.len()
        ));
    }
    args.iter()
        .zip(by)
        .enumerate()
        .map(|(index, (value, column))| {
            scalar(column, value)
                .map_err(|reason| format!("table argument {}: {reason}", index + 1))
        })
        .collect()
}

/// Decode a read's arguments, held to what it reads by, without inventing
/// missing or unsupported values: a keyed read's fields (a `get`'s key, a
/// prefix's, an index's), a set form's one list of them, and `all`'s
/// nothing. The adapter then reads by them, and checks them again.
pub fn decode_read_arguments(read: &Read, args: &[Val]) -> Result<Vec<Value>, String> {
    if read.many() {
        let [Val::List(keys)] = args else {
            return Err(format!(
                "a {} takes one list of {}s",
                read.primitive.name(),
                read.primitive.by_words()
            ));
        };
        let keys = keys
            .iter()
            .enumerate()
            .map(|(index, value)| {
                key(&read.by, value).map_err(|why| format!("table read key {}: {why}", index + 1))
            })
            .collect::<Result<Vec<_>, _>>()?;
        return Ok(vec![Value::Array(keys)]);
    }
    positional(read.primitive, &read.by, args)
}

/// **Decode a write's arguments** (ADR-0386): an `insert`'s or a `put`'s
/// row, exactly its table's (its fields in their declared order, by their
/// WIT names, each of its stored type and within its bounds), as the
/// adapter's object by the Pleris names; a delete's key or prefix fields.
pub fn decode_write_arguments(write: &Write, args: &[Val]) -> Result<Vec<Value>, String> {
    match write.primitive {
        Primitive::Insert | Primitive::Put => {
            let [row] = args else {
                return Err(format!(
                    "a {} takes one row, and was given {} values",
                    write.primitive.name(),
                    args.len()
                ));
            };
            Ok(vec![row_value(&write.row, row)?])
        }
        primitive => positional(primitive, &write.by, args),
    }
}

/// A row, exactly its table's, as the adapter's object.
fn row_value(columns: &[Column], value: &Val) -> Result<Value, String> {
    let fields = exact_row(columns, value).map_err(|why| format!("a row: {why}"))?;
    record(columns, fields)
}

fn record(columns: &[Column], fields: &[(String, Val)]) -> Result<Value, String> {
    let mut object = Map::new();
    for (column, (_, value)) in columns.iter().zip(fields) {
        let json = json(&column.stored, value).map_err(|why| format!("`{}`: {why}", column.wit))?;
        object.insert(column.name.clone(), json);
    }
    Ok(Value::Object(object))
}

/// A value held exactly (`exact_row` checked its type and bounds), as the
/// adapter's JSON.
fn json(stored: &Stored, value: &Val) -> Result<Value, String> {
    Ok(match (stored, value) {
        (Stored::Int { .. }, Val::S64(n)) => Value::from(*n),
        (Stored::String { .. }, Val::String(s)) => Value::from(s.clone()),
        (Stored::Bool, Val::Bool(b)) => Value::from(*b),
        (Stored::Float, Val::Float64(x)) => serde_json::Number::from_f64(*x)
            .map(Value::Number)
            .ok_or_else(|| format!("{x} is not a finite number"))?,
        (Stored::Option(_), Val::Option(None)) => Value::Null,
        // `Some(None)` would be stored as `None` (ADR-0386).
        (Stored::Option(inner), Val::Option(Some(_))) if matches!(**inner, Stored::Option(_)) => {
            return Err("an Option of an Option is not stored".into());
        }
        (Stored::Option(inner), Val::Option(Some(value))) => json(inner, value)?,
        (Stored::List(inner), Val::List(items)) => Value::Array(
            items
                .iter()
                .map(|item| json(inner, item))
                .collect::<Result<_, _>>()?,
        ),
        (Stored::Record(fields), Val::Record(given)) => {
            record(&fields.iter().map(Column::of).collect::<Vec<_>>(), given)?
        }
        _ => {
            return Err(format!(
                "a value the table host does not store: {}",
                super::table_link::described(stored)
            ));
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
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

    /// A read bound to `primitive` by `key`, its row `key` and a title.
    fn read_by(primitive: Primitive, key: &[(&str, Stored)]) -> Read {
        let by: Vec<Column> = key
            .iter()
            .map(|(name, stored)| Column::new(name, stored.clone()))
            .collect();
        let mut row = by.clone();
        row.push(Column::new("title", string(None)));
        Read {
            import: format!("pw:tables/items#{}", primitive.name()),
            table: "Items".into(),
            primitive,
            key: by.iter().map(|c| c.name.clone()).collect(),
            by,
            row,
            order: None,
            sorted_by: Vec::new(),
            unique: false,
            bounded: None,
        }
    }

    /// A get, or a get-many, by `key`.
    fn read(many: bool, key: &[(&str, Stored)]) -> Read {
        read_by(
            if many {
                Primitive::GetMany
            } else {
                Primitive::Get
            },
            key,
        )
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

    /// ADR-0386: `all` is given nothing, and a prefix read its fields.
    #[test]
    fn all_takes_nothing_and_a_prefix_its_fields() {
        let all = read_by(Primitive::All, &[]);
        assert_eq!(decode_read_arguments(&all, &[]), Ok(vec![]));
        assert!(decode_read_arguments(&all, &[Val::String("a".into())]).is_err());
        let prefix = read_by(Primitive::Prefix, &[("list", string(None))]);
        assert_eq!(
            decode_read_arguments(&prefix, &[Val::String("a".into())]),
            Ok(vec![json!("a")])
        );
        assert!(decode_read_arguments(&prefix, &[Val::S64(1)]).is_err());
    }

    fn row() -> Vec<Column> {
        vec![
            Column::new("list_id", string(Some(4))),
            Column::new("count", int(Some(1))),
            Column::new("note", Stored::Option(Box::new(string(None)))),
            Column::new("weight", Stored::Float),
            Column::new("tags", Stored::List(Box::new(string(None)))),
        ]
    }

    fn write(primitive: Primitive) -> Write {
        let row = row();
        Write {
            import: format!("pw:tables/items#{}", primitive.name()),
            table: "Items".into(),
            primitive,
            by: match primitive {
                Primitive::Delete | Primitive::DeletePrefix => row[..1].to_vec(),
                _ => Vec::new(),
            },
            key: vec!["list_id".into()],
            unique: Vec::new(),
            row,
        }
    }

    fn given(list: &str, count: i64, weight: f64) -> Val {
        Val::Record(vec![
            ("list-id".into(), Val::String(list.into())),
            ("count".into(), Val::S64(count)),
            (
                "note".into(),
                Val::Option(Some(Box::new(Val::String("n".into())))),
            ),
            ("weight".into(), Val::Float64(weight)),
            (
                "tags".into(),
                Val::List(vec![Val::String("x".into()), Val::String("y".into())]),
            ),
        ])
    }

    /// ADR-0386: a row given to an insert or a put reaches the adapter as
    /// its table's, by the Pleris names, each value as it was given; one of
    /// another shape, type or bound never does.
    #[test]
    fn a_written_row_is_exactly_its_tables_or_refused() {
        for primitive in [Primitive::Insert, Primitive::Put] {
            let write = write(primitive);
            assert_eq!(
                decode_write_arguments(&write, &[given("a", 2, 1.5)]),
                Ok(vec![json!({
                    "list_id": "a", "count": 2, "note": "n", "weight": 1.5, "tags": ["x", "y"]
                })])
            );
            for (bad, words) in [
                (given("a", 0, 1.5), "`count`: 0 is outside its bounds"),
                (
                    given("abcde", 1, 1.5),
                    "`list-id`: 5 characters is outside its bounds",
                ),
                (given("a", 1, f64::INFINITY), "inf is not a finite number"),
                (Val::S64(1), "s64 where a row is a record"),
            ] {
                let refused = decode_write_arguments(&write, &[bad]).expect_err(words);
                assert!(refused.contains(words), "{words}: {refused}");
            }
            let Val::Record(mut fields) = given("a", 1, 1.5) else {
                unreachable!()
            };
            fields.swap(0, 1);
            assert!(decode_write_arguments(&write, &[Val::Record(fields.clone())]).is_err());
            fields.swap(0, 1);
            fields.push(("secret".into(), Val::String("leak".into())));
            let refused =
                decode_write_arguments(&write, &[Val::Record(fields)]).expect_err("a field more");
            assert!(refused.contains("`secret`"), "{refused}");
            assert!(decode_write_arguments(&write, &[]).is_err());
        }
        // An `Option` of an `Option`: its `Some(None)` would be stored as
        // its `None`, so it is refused, and its `None` is stored.
        let mut nested = write(Primitive::Insert);
        nested.row = vec![Column::new(
            "maybe",
            Stored::Option(Box::new(Stored::Option(Box::new(string(None))))),
        )];
        let some_none = Val::Record(vec![(
            "maybe".into(),
            Val::Option(Some(Box::new(Val::Option(None)))),
        )]);
        assert!(decode_write_arguments(&nested, &[some_none]).is_err());
        assert_eq!(
            decode_write_arguments(
                &nested,
                &[Val::Record(vec![("maybe".into(), Val::Option(None))])]
            ),
            Ok(vec![json!({"maybe": null})])
        );
        let delete = write(Primitive::Delete);
        assert_eq!(
            decode_write_arguments(&delete, &[Val::String("a".into())]),
            Ok(vec![json!("a")])
        );
        assert!(decode_write_arguments(&delete, &[Val::String("abcde".into())]).is_err());
        assert!(decode_write_arguments(&delete, &[]).is_err());
    }
}
