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
