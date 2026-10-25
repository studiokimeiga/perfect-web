//! Adapt one already-authorized table read to the Wasmtime host callback ABI.
//!
//! The caller selects the exact manifest-declared get/get-many import and
//! supplies its adapter read closure. This module does not grant an import,
//! inspect a contract, install linker definitions, or invoke a component.
//! JSON is confined to the memory adapter boundary, never exposed as WIT.

use std::sync::Arc;

use serde_json::Value;

use super::HostFn;
use super::table_results::{encode_get, encode_get_many};
use super::table_selectors::decode_read_arguments;

/// The two read operations admitted for this initial table-host slice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadKind {
    Get,
    GetMany,
}

/// One adapter operation, with selectors and results in its internal format.
pub type TableRead = Arc<dyn Fn(&[Value]) -> Result<Value, String> + Send + Sync>;

/// Compose a previously selected read with strict argument/result conversion.
///
/// This returns an ordinary HostFn for the host's exact-key operation map.
/// It does not authorize the key: admission, the compiler's tables.json and
/// the component contract must select it before this callback is installed.
pub fn host_fn(kind: ReadKind, read: TableRead) -> HostFn {
    Arc::new(move |args| {
        let selectors = decode_read_arguments(args)?;
        let answer = read(&selectors)?;
        let result = match kind {
            ReadKind::Get => encode_get(&answer)?,
            ReadKind::GetMany => encode_get_many(&answer)?,
        };
        Ok(vec![result])
    })
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use serde_json::json;

    use super::super::Val;
    use super::*;

    #[test]
    fn get_passes_typed_selectors_and_returns_wit_some_or_none() {
        let found = host_fn(
            ReadKind::Get,
            Arc::new(|args| {
                assert_eq!(args, &[json!(-4), json!("first")]);
                Ok(json!({"id": "first", "active": true}))
            }),
        );
        assert_eq!(
            found(&[Val::S64(-4), Val::String("first".into())]),
            Ok(vec![Val::Option(Some(Box::new(Val::Record(vec![
                ("active".into(), Val::Bool(true)),
                ("id".into(), Val::String("first".into())),
            ]))))])
        );

        let absent = host_fn(ReadKind::Get, Arc::new(|_| Ok(Value::Null)));
        assert_eq!(
            absent(&[Val::String("missing".into())]),
            Ok(vec![Val::Option(None)])
        );
    }

    #[test]
    fn get_many_preserves_adapter_order_and_empty_reads() {
        let ordered = host_fn(
            ReadKind::GetMany,
            Arc::new(|args| {
                assert_eq!(args, &[json!(["two", "one"])]);
                Ok(json!([{"id": "two"}, {"id": "one"}]))
            }),
        );
        assert_eq!(
            ordered(&[Val::List(vec![
                Val::String("two".into()),
                Val::String("one".into())
            ])]),
            Ok(vec![Val::List(vec![
                Val::Record(vec![("id".into(), Val::String("two".into()))]),
                Val::Record(vec![("id".into(), Val::String("one".into()))]),
            ])])
        );
        let empty = host_fn(ReadKind::GetMany, Arc::new(|_| Ok(json!([]))));
        assert_eq!(empty(&[Val::List(vec![])]), Ok(vec![Val::List(vec![])]));
    }

    #[test]
    fn malformed_arguments_never_reach_the_adapter() {
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        let read = host_fn(
            ReadKind::Get,
            Arc::new(move |_| {
                observed.fetch_add(1, Ordering::Relaxed);
                Ok(json!({"id": 7}))
            }),
        );
        assert!(read(&[Val::Float64(1.5)]).is_err());
        assert_eq!(calls.load(Ordering::Relaxed), 0);
        assert_eq!(
            read(&[Val::S64(7)]),
            Ok(vec![Val::Option(Some(Box::new(Val::Record(vec![(
                "id".into(),
                Val::S64(7),
            )]))))])
        );
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn adapter_errors_and_unsupported_rows_are_refusals() {
        let failed = host_fn(ReadKind::Get, Arc::new(|_| Err("read refused".into())));
        assert!(matches!(
            failed(&[Val::S64(1)]),
            Err(reason) if reason == "read refused"
        ));

        let malformed = host_fn(
            ReadKind::GetMany,
            Arc::new(|_| Ok(json!([{"id": "valid"}, {"id": [1, 2]}]))),
        );
        assert!(malformed(&[Val::List(vec![])]).is_err());
    }
}
