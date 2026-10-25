//! The compiled readers' host boundary, test by test (PRs #35 to #40,
//! moved here from `pw-core`'s `table_host_reader_fixture` by the
//! integrator's ruling 10 of 2026-10-25, so that no `pw-core` test build
//! links Wasmtime; split by ruling 11, so that a renamed component fails
//! loudly instead of skipping a branch).
//!
//! Each test drives the compiler's own `tables.reader` components through
//! the real engine: what Wasmtime finds in their bytes, their admission, and
//! their calls with exact test callbacks, linked through the table gate with
//! the program's tables.json (`call_with_tables`). The memory adapter itself
//! is `compiled_reader.rs`'s.

mod common;

use std::collections::BTreeMap;
use std::sync::atomic::Ordering;

use common::{BY_ID, BY_IDS, GET, GET_MANY, READ_LISTS, answering, program, todo_list};
use pw_host::engine::{HostFn, Val};

/// PR #35: compiler bookkeeping is not proof of what Wasmtime will load, so
/// each reader's imports are read from its compiled bytes by the engine:
/// exactly its own manifest-declared read, no sibling, write, WASI or
/// wildcard import.
#[test]
fn each_reader_imports_exactly_its_manifest_declared_read() {
    let program = program();
    for (id, key, primitive) in [
        (BY_ID, GET, pw_core::tables::Primitive::Get),
        (BY_IDS, GET_MANY, pw_core::tables::Primitive::GetMany),
    ] {
        let reader = program.reader(id);
        assert_eq!(reader.imports, [key], "{id}");
        let lists = program
            .tables
            .tables
            .iter()
            .find(|table| table.schema.name == "Lists")
            .expect("tables.json declares Lists");
        let function = lists
            .functions
            .iter()
            .find(|function| function.import == key)
            .expect("tables.json declares the read");
        assert_eq!(
            (function.primitive, function.effect.as_str()),
            (primitive, "read")
        );
        let declared: Vec<String> = reader.contract.imports.iter().map(|i| i.key()).collect();
        assert_eq!(declared, [key], "{id}: no neighbouring or wildcard import");
        assert_eq!(
            reader.contract.imports[0].capability,
            "database.read<Lists>"
        );
    }
}

/// PR #36: admission by the imports in the bytes links exactly each reader's
/// own read.
#[test]
fn admission_links_only_each_readers_own_read() {
    let program = program();
    for id in [BY_ID, BY_IDS] {
        let reader = program.reader(id);
        let granted = reader.admitted(&reader.contract, READ_LISTS);
        assert_eq!(
            pw_host::linkable(&reader.contract, &granted),
            reader.imports,
            "{id}: the host links only the declared read"
        );
    }
}

/// PR #36: a matching contract without the table's read grant is refused.
#[test]
fn a_node_without_the_read_grant_refuses_admission() {
    let program = program();
    for id in [BY_ID, BY_IDS] {
        let reader = program.reader(id);
        let denied = reader.admission(&reader.contract, "");
        assert!(
            denied
                .refusals()
                .iter()
                .any(|reason| matches!(reason, pw_host::Refusal::Ungranted { .. })),
            "{id}: {denied:?}"
        );
    }
}

/// PR #36: the sibling is another real compiled reader, and its import is
/// absent from this reader's contract.
#[test]
fn a_siblings_import_is_undeclared_for_a_reader() {
    let program = program();
    for (id, sibling) in [(BY_ID, BY_IDS), (BY_IDS, BY_ID)] {
        let reader = program.reader(id);
        let topology = pw_host::Topology::from_json(&format!(
            r#"{{"nodes":[{{"name":"origin","world":"origin","grants":[{READ_LISTS}]}}]}}"#
        ))
        .unwrap();
        let mismatch = pw_host::admit(
            &reader.contract,
            &topology,
            "origin",
            &program.reader(sibling).imports,
        );
        assert!(
            mismatch
                .refusals()
                .iter()
                .any(|reason| matches!(reason, pw_host::Refusal::UndeclaredImport { .. })),
            "{id}: {mismatch:?}"
        );
    }
}

/// PR #37: a capability grant alone never supplies a default or a stub.
#[test]
fn a_granted_get_with_no_callback_is_refused_not_stubbed() {
    let program = program();
    let reader = program.reader(BY_ID);
    let granted = reader.admitted(&reader.contract, READ_LISTS);
    let refused = reader
        .call(
            &granted,
            &BTreeMap::new(),
            &program.manifest,
            &[Val::String("missing".into())],
        )
        .expect_err("get with no host callback must be refused");
    assert!(
        refused.contains(GET),
        "the missing exact read is named: {refused}"
    );
}

/// PR #37: the compiled get calls its one exact callback, once, with the
/// key it was given, and answers what the callback answers: here, none.
#[test]
fn a_missing_key_answers_none_through_the_exact_callback() {
    let program = program();
    let reader = program.reader(BY_ID);
    let granted = reader.admitted(&reader.contract, READ_LISTS);
    let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let observed = std::sync::Arc::clone(&calls);
    let handler: HostFn = std::sync::Arc::new(move |received| {
        assert_eq!(received, &[Val::String("missing".into())]);
        observed.fetch_add(1, Ordering::SeqCst);
        Ok(vec![Val::Option(None)])
    });
    let host = BTreeMap::from([(GET.to_string(), handler)]);
    assert_eq!(
        reader.call(
            &granted,
            &host,
            &program.manifest,
            &[Val::String("missing".into())]
        ),
        Ok(vec![Val::Option(None)])
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

/// PR #39: a present row keeps its typed WIT `Option<TodoList>` across the
/// compiled guest and its exact callback.
#[test]
fn a_present_row_round_trips_typed() {
    let program = program();
    let reader = program.reader(BY_ID);
    let granted = reader.admitted(&reader.contract, READ_LISTS);
    let present = Val::Option(Some(Box::new(todo_list("list-a", "List A", 10))));
    let (callback, calls) = answering(vec![present.clone()]);
    let host = BTreeMap::from([(GET.to_string(), callback)]);
    assert_eq!(
        reader.call(
            &granted,
            &host,
            &program.manifest,
            &[Val::String("list-a".into())]
        ),
        Ok(vec![present])
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

/// PR #38: the compiled get-many forwards its keys to its exact callback,
/// once, and answers its typed rows.
#[test]
fn get_many_forwards_its_keys_and_answers_typed_rows() {
    let program = program();
    let reader = program.reader(BY_IDS);
    let granted = reader.admitted(&reader.contract, READ_LISTS);
    let args = [Val::List(vec![
        Val::String("list-b".into()),
        Val::String("list-a".into()),
    ])];
    let rows = Val::List(vec![
        todo_list("list-b", "List B", 20),
        todo_list("list-a", "List A", 10),
    ]);
    let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let observed = std::sync::Arc::clone(&calls);
    let expected = args.clone();
    let supplied = rows.clone();
    let handler: HostFn = std::sync::Arc::new(move |received| {
        assert_eq!(received, &expected[..]);
        observed.fetch_add(1, Ordering::SeqCst);
        Ok(vec![supplied.clone()])
    });
    let host = BTreeMap::from([(GET_MANY.to_string(), handler)]);
    assert_eq!(
        reader.call(&granted, &host, &program.manifest, &args),
        Ok(vec![rows])
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

/// PR #40: a well-formed `Val` of the wrong shape for the compiled guest's
/// WIT `Option<TodoList>` is refused, never read as a missing row, and a
/// valid read after it still works.
#[test]
fn a_malformed_get_answer_is_refused_and_a_valid_one_still_works() {
    let program = program();
    let reader = program.reader(BY_ID);
    let granted = reader.admitted(&reader.contract, READ_LISTS);
    let args = [Val::String("list-a".into())];
    let (invalid, invalid_calls) = answering(vec![Val::Option(Some(Box::new(Val::S64(10))))]);
    let host = BTreeMap::from([(GET.to_string(), invalid)]);
    assert!(
        reader
            .call(&granted, &host, &program.manifest, &args)
            .is_err(),
        "a scalar Some value must not satisfy the compiled WIT row result"
    );
    assert_eq!(invalid_calls.load(Ordering::SeqCst), 1);

    let present = Val::Option(Some(Box::new(todo_list("list-a", "List A", 10))));
    let (valid, valid_calls) = answering(vec![present.clone()]);
    let host = BTreeMap::from([(GET.to_string(), valid)]);
    assert_eq!(
        reader.call(&granted, &host, &program.manifest, &args),
        Ok(vec![present])
    );
    assert_eq!(valid_calls.load(Ordering::SeqCst), 1);
}
