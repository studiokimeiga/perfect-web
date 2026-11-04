//! Adapt one already-authorized table read to the Wasmtime host callback ABI.
//!
//! The caller passes a read the table gate admitted (`pw_host::tables`): a
//! keyed read, a set form, a prefix's, an index's or `all` (ADR-0386), and
//! its adapter's read closure. This module does not grant an import, inspect
//! a contract, install linker definitions, or invoke a component. JSON is
//! confined to the adapter boundary, never exposed as WIT: the selectors are
//! held to the read's key before the adapter sees them, and the rows encoded
//! as the read's row declares (`table_selectors`, `table_results`).

use std::sync::Arc;

use serde_json::Value;

use super::HostFn;
use super::table_results::encode_read;
use super::table_selectors::decode_read_arguments;
use crate::tables::Read;

/// One adapter operation, with selectors and results in its internal format.
pub type TableRead = Arc<dyn Fn(&[Value]) -> Result<Value, String> + Send + Sync>;

/// Compose a previously admitted read with strict argument/result conversion.
///
/// This returns an ordinary HostFn for the host's exact-key operation map.
/// It does not authorize the key: the gate selects the read, and
/// `call_with_tables` links it only through the gate.
pub fn host_fn(read: Read, data: TableRead) -> HostFn {
    Arc::new(move |args| {
        let selectors = decode_read_arguments(&read, args)?;
        let answer = data(&selectors)?;
        Ok(vec![encode_read(&read, &answer)?])
    })
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use serde_json::json;

    use super::super::Val;
    use super::*;
    use crate::tables::{Column, Primitive, Stored};

    /// A read keyed by an Int `rank` and a String `id`, its row those two and
    /// a Bool `active`; or, `many`, keyed by `id` alone.
    fn read(many: bool) -> Read {
        let rank = Column::new(
            "rank",
            Stored::Int {
                at_least: None,
                at_most: None,
            },
        );
        let id = Column::new(
            "id",
            Stored::String {
                min_length: None,
                max_length: None,
            },
        );
        let active = Column::new("active", Stored::Bool);
        let by = if many {
            vec![id.clone()]
        } else {
            vec![rank.clone(), id.clone()]
        };
        Read {
            handle: None,
            import: "pw:tables/likes#get".into(),
            table: "Likes".into(),
            primitive: if many {
                Primitive::GetMany
            } else {
                Primitive::Get
            },
            key: by.iter().map(|c| c.name.clone()).collect(),
            by,
            row: vec![rank, id, active],
            order: None,
            sorted_by: Vec::new(),
            unique: false,
            bounded: None,
        }
    }

    #[test]
    fn count_callbacks_validate_selectors_before_data_and_encode_only_count_shapes() {
        let mut count = read(false);
        count.primitive = Primitive::Count;
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&calls);
        let scalar = host_fn(
            count.clone(),
            Arc::new(move |args| {
                observed.fetch_add(1, Ordering::Relaxed);
                assert_eq!(args, &[json!(7), json!("x")]);
                Ok(json!(0))
            }),
        );
        for wrong in [
            vec![Val::String("7".into()), Val::String("x".into())],
            vec![Val::S64(7)],
            vec![Val::S64(7), Val::Bool(false)],
        ] {
            assert!(scalar(&wrong).is_err());
        }
        assert_eq!(calls.load(Ordering::Relaxed), 0);
        assert_eq!(
            scalar(&[Val::S64(7), Val::String("x".into())]),
            Ok(vec![Val::S64(0)])
        );
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        for wrong in [
            json!(-1),
            json!(1.5),
            json!(true),
            json!(null),
            json!({"count":2}),
            json!(18446744073709551615u64),
        ] {
            let callback = host_fn(count.clone(), Arc::new(move |_| Ok(wrong.clone())));
            assert!(callback(&[Val::S64(7), Val::String("x".into())]).is_err());
        }
        count.primitive = Primitive::CountMany;
        let selected = Val::Record(vec![
            ("rank".into(), Val::S64(7)),
            ("id".into(), Val::String("x".into())),
        ]);
        let args = [Val::List(vec![selected.clone()])];
        let correct = host_fn(
            count.clone(),
            Arc::new(|_| Ok(json!([{"key":{"id":"x", "rank":7}, "count":0}]))),
        );
        assert_eq!(
            correct(&args),
            Ok(vec![Val::List(vec![Val::Record(vec![
                ("key".into(), selected),
                ("count".into(), Val::S64(0))
            ])])])
        );
        for wrong in [
            json!([{"key":{"id":"x", "rank":7}, "count":-1}]),
            json!([{"key":{"id":"x", "rank":7}, "count":0, "secret":"hidden"}]),
            json!([{"key":{"id":"x"}, "count":0}]),
            json!([{"key":{"id":"x", "rank":7, "active":true}, "count":0}]),
            json!([{"key":{"id":"x", "rank":7}}]),
        ] {
            let callback = host_fn(count.clone(), Arc::new(move |_| Ok(wrong.clone())));
            assert!(callback(&args).is_err());
        }
    }

    #[test]
    fn window_selectors_and_typed_limits_are_checked_before_the_adapter_even_at_zero() {
        let mut window = read(false);
        window.primitive = Primitive::First;
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&calls);
        let first = host_fn(
            window.clone(),
            Arc::new(move |args| {
                observed.fetch_add(1, Ordering::Relaxed);
                assert_eq!(args.len(), 3);
                assert_eq!(&args[..2], &[json!(7), json!("x")]);
                Ok(if args[2].as_i64().unwrap_or(0) <= 0 {
                    json!([])
                } else {
                    json!([{"rank":7,"id":"x","active":true}])
                })
            }),
        );
        for args in [
            vec![Val::S64(7), Val::String("x".into())],
            vec![Val::S64(7), Val::String("x".into()), Val::Bool(false)],
            vec![Val::Bool(true), Val::String("x".into()), Val::S64(0)],
            vec![Val::S64(7), Val::Bool(false), Val::S64(-1)],
        ] {
            assert!(first(&args).is_err());
        }
        assert_eq!(calls.load(Ordering::Relaxed), 0);
        assert_eq!(
            first(&[Val::S64(7), Val::String("x".into()), Val::S64(2)]),
            Ok(vec![Val::List(vec![row(7, "x")])])
        );
        assert_eq!(
            first(&[Val::S64(7), Val::String("x".into()), Val::S64(0)]),
            Ok(vec![Val::List(vec![])])
        );
        assert_eq!(calls.load(Ordering::Relaxed), 2);
        window.primitive = Primitive::FirstMany;
        let observed = Arc::clone(&calls);
        let many = host_fn(
            window,
            Arc::new(move |_| {
                observed.fetch_add(1, Ordering::Relaxed);
                Ok(json!([]))
            }),
        );
        let good = Val::Record(vec![
            ("rank".into(), Val::S64(7)),
            ("id".into(), Val::String("x".into())),
        ]);
        let bad = Val::Record(vec![
            ("rank".into(), Val::Bool(false)),
            ("id".into(), Val::String("x".into())),
        ]);
        assert!(many(&[Val::List(vec![good, bad]), Val::S64(0)]).is_err());
        assert_eq!(
            calls.load(Ordering::Relaxed),
            2,
            "validate the complete set before any empty-window shortcut"
        );
    }

    fn row(rank: i64, id: &str) -> Val {
        Val::Record(vec![
            ("rank".into(), Val::S64(rank)),
            ("id".into(), Val::String(id.into())),
            ("active".into(), Val::Bool(true)),
        ])
    }

    #[test]
    fn get_passes_typed_selectors_and_returns_wit_some_or_none() {
        let found = host_fn(
            read(false),
            Arc::new(|args| {
                assert_eq!(args, &[json!(-4), json!("first")]);
                Ok(json!({"id": "first", "active": true, "rank": -4}))
            }),
        );
        assert_eq!(
            found(&[Val::S64(-4), Val::String("first".into())]),
            Ok(vec![Val::Option(Some(Box::new(row(-4, "first"))))])
        );

        let absent = host_fn(read(false), Arc::new(|_| Ok(Value::Null)));
        assert_eq!(
            absent(&[Val::S64(1), Val::String("missing".into())]),
            Ok(vec![Val::Option(None)])
        );
    }

    #[test]
    fn get_many_preserves_adapter_order_and_empty_reads() {
        let ordered = host_fn(
            read(true),
            Arc::new(|args| {
                assert_eq!(args, &[json!(["two", "one"])]);
                Ok(json!([
                    {"rank": 2, "id": "two", "active": true},
                    {"rank": 1, "id": "one", "active": true}
                ]))
            }),
        );
        assert_eq!(
            ordered(&[Val::List(vec![
                Val::String("two".into()),
                Val::String("one".into())
            ])]),
            Ok(vec![Val::List(vec![row(2, "two"), row(1, "one")])])
        );
        let empty = host_fn(read(true), Arc::new(|_| Ok(json!([]))));
        assert_eq!(empty(&[Val::List(vec![])]), Ok(vec![Val::List(vec![])]));
    }

    #[test]
    fn malformed_or_mistyped_arguments_never_reach_the_adapter() {
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        let get = host_fn(
            read(false),
            Arc::new(move |_| {
                observed.fetch_add(1, Ordering::Relaxed);
                Ok(json!({"rank": 7, "id": "x", "active": true}))
            }),
        );
        assert!(get(&[Val::Float64(1.5), Val::String("x".into())]).is_err());
        assert!(get(&[Val::S64(7), Val::Bool(true)]).is_err());
        assert!(get(&[Val::S64(7)]).is_err());
        assert_eq!(calls.load(Ordering::Relaxed), 0);
        assert_eq!(
            get(&[Val::S64(7), Val::String("x".into())]),
            Ok(vec![Val::Option(Some(Box::new(row(7, "x"))))])
        );
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn adapter_errors_and_unsupported_rows_are_refusals() {
        let failed = host_fn(read(false), Arc::new(|_| Err("read refused".into())));
        assert!(matches!(
            failed(&[Val::S64(1), Val::String("a".into())]),
            Err(reason) if reason == "read refused"
        ));

        let malformed = host_fn(
            read(true),
            Arc::new(|_| {
                Ok(json!([
                    {"rank": 1, "id": "valid", "active": true},
                    {"rank": 2, "id": [1, 2], "active": true}
                ]))
            }),
        );
        assert!(malformed(&[Val::List(vec![])]).is_err());
    }
}
