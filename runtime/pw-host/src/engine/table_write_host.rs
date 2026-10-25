//! **Adapt one already-authorized table write to the Wasmtime host callback
//! ABI** (ADR-0386).
//!
//! The caller passes a write the table gate admitted into a command
//! (`pw_host::tables::Manifest::admit_command`) and its adapter's write
//! closure, which stages the write in the command's transaction. This module
//! does not grant an import, begin or commit a transaction, or invoke a
//! component. JSON is confined to the adapter boundary, never exposed as WIT:
//! a written row is held exactly to its table's row, and a delete's fields to
//! their declared types and bounds, before the adapter sees them
//! (`table_selectors`); the answer is encoded as the write's WIT type
//! (`table_results`), and the engine holds it to the write again
//! (`table_link::check_answer`).

use std::sync::Arc;

use serde_json::Value;

use super::HostFn;
use super::table_results::encode_write;
use super::table_selectors::decode_write_arguments;
use crate::tables::Write;

/// **One adapter write, staged in a command's transaction**: `Ok(None)`
/// where it was written, `Ok(Some(row))` where it was refused by `row`, which
/// holds its key or a unique value it would hold (nothing written), and
/// `Err` where it failed, which fails the command.
pub type TableWrite = Arc<dyn Fn(&[Value]) -> Result<Option<Value>, String> + Send + Sync>;

/// Compose a previously admitted write with strict argument and answer
/// conversion. It does not authorize the key: the gate selects the write,
/// and `call_with_tables` links it only through the gate, into a command.
pub fn host_fn(write: Write, data: TableWrite) -> HostFn {
    Arc::new(move |args| {
        let values = decode_write_arguments(&write, args)?;
        let refused = data(&values)?;
        encode_write(&write, refused.as_ref())
    })
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use serde_json::json;

    use super::super::Val;
    use super::*;
    use crate::tables::{Column, Primitive, Stored};

    fn write(primitive: Primitive) -> Write {
        let id = Column::new(
            "id",
            Stored::String {
                min_length: None,
                max_length: Some(4),
            },
        );
        let done = Column::new("done", Stored::Bool);
        Write {
            import: format!("pw:tables/todos#{}", primitive.name()),
            table: "Todos".into(),
            primitive,
            by: match primitive {
                Primitive::Delete => vec![id.clone()],
                _ => Vec::new(),
            },
            key: vec!["id".into()],
            unique: Vec::new(),
            row: vec![id, done],
        }
    }

    fn todo(id: &str) -> Val {
        Val::Record(vec![
            ("id".into(), Val::String(id.into())),
            ("done".into(), Val::Bool(false)),
        ])
    }

    #[test]
    fn a_write_passes_its_row_and_answers_its_wit_type() {
        let insert = host_fn(
            write(Primitive::Insert),
            Arc::new(|args| {
                assert_eq!(args, &[json!({"id": "a", "done": false})]);
                Ok(None)
            }),
        );
        assert_eq!(insert(&[todo("a")]), Ok(vec![Val::Result(Ok(None))]));
        let refused = host_fn(
            write(Primitive::Insert),
            Arc::new(|_| Ok(Some(json!({"id": "a", "done": true})))),
        );
        assert_eq!(
            refused(&[todo("a")]),
            Ok(vec![Val::Result(Err(Some(Box::new(Val::Record(vec![
                ("id".into(), Val::String("a".into())),
                ("done".into(), Val::Bool(true)),
            ])))))])
        );
        let delete = host_fn(
            write(Primitive::Delete),
            Arc::new(|args| {
                assert_eq!(args, &[json!("a")]);
                Ok(None)
            }),
        );
        assert_eq!(delete(&[Val::String("a".into())]), Ok(Vec::new()));
    }

    #[test]
    fn a_malformed_row_never_reaches_the_adapter() {
        let calls = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&calls);
        let put = host_fn(
            write(Primitive::Put),
            Arc::new(move |_| {
                counted.fetch_add(1, Ordering::SeqCst);
                Ok(None)
            }),
        );
        for bad in [
            Val::Record(vec![("id".into(), Val::String("a".into()))]),
            Val::Record(vec![
                ("done".into(), Val::Bool(false)),
                ("id".into(), Val::String("a".into())),
            ]),
            todo("toolong"),
            Val::String("a".into()),
        ] {
            assert!(put(&[bad]).is_err());
        }
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(put(&[todo("a")]), Ok(Vec::new()));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        // An adapter's failure fails the call: it is no refusal.
        let failed = host_fn(write(Primitive::Put), Arc::new(|_| Err("full".into())));
        assert_eq!(failed(&[todo("a")]), Err("full".into()));
    }
}
