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

/// PR #38, with the integrator's ruling 7 of 2026-10-25: the compiled
/// get-many forwards its keys to its exact callback, once and as given
/// (here with a key twice), and answers typed rows only in tables.json's
/// order (by key), each key once.
#[test]
fn get_many_forwards_its_keys_and_answers_canonical_rows_once() {
    let program = program();
    let reader = program.reader(BY_IDS);
    let granted = reader.admitted(&reader.contract, READ_LISTS);
    let args = [Val::List(vec![
        Val::String("list-b".into()),
        Val::String("list-a".into()),
        Val::String("list-b".into()),
    ])];
    let rows = Val::List(vec![
        todo_list("list-a", "List A", 10),
        todo_list("list-b", "List B", 20),
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

/// Ruling 7: whatever answers a get-many, its rows are of keys asked for,
/// each once, in tables.json's order; an answer that is not is refused, not
/// passed on. Missing keys are left out, and an empty list is an answer.
#[test]
fn a_get_many_answer_out_of_order_twice_or_unasked_is_refused() {
    let program = program();
    let reader = program.reader(BY_IDS);
    let granted = reader.admitted(&reader.contract, READ_LISTS);
    let args = [Val::List(vec![
        Val::String("list-b".into()),
        Val::String("list-a".into()),
    ])];
    let a = todo_list("list-a", "List A", 10);
    let b = todo_list("list-b", "List B", 20);
    let z = todo_list("list-z", "List Z", 30);
    for (rows, words) in [
        (vec![b.clone(), a.clone()], "out of tables.json's order"),
        (vec![a.clone(), a.clone(), b.clone()], "was answered before"),
        (vec![a.clone(), z], "(\"list-z\") was not asked for"),
    ] {
        let (callback, calls) = answering(vec![Val::List(rows)]);
        let host = BTreeMap::from([(GET_MANY.to_string(), callback)]);
        let refused = reader
            .call(&granted, &host, &program.manifest, &args)
            .expect_err(words);
        assert!(refused.contains(words), "{words}: {refused}");
        assert_eq!(calls.load(Ordering::SeqCst), 1, "the answer, not the link");
    }
    // The neighbours: a key left out, and none found.
    for rows in [vec![b.clone()], vec![]] {
        let (callback, _) = answering(vec![Val::List(rows.clone())]);
        let host = BTreeMap::from([(GET_MANY.to_string(), callback)]);
        assert_eq!(
            reader.call(&granted, &host, &program.manifest, &args),
            Ok(vec![Val::List(rows)])
        );
    }
}

/// Ruling 7: a get answers none, or the row of the key it was given.
#[test]
fn a_get_answering_another_keys_row_is_refused() {
    let program = program();
    let reader = program.reader(BY_ID);
    let granted = reader.admitted(&reader.contract, READ_LISTS);
    let other = Val::Option(Some(Box::new(todo_list("list-z", "List Z", 30))));
    let (callback, calls) = answering(vec![other]);
    let host = BTreeMap::from([(GET.to_string(), callback)]);
    let refused = reader
        .call(
            &granted,
            &host,
            &program.manifest,
            &[Val::String("list-a".into())],
        )
        .expect_err("another key's row");
    assert!(
        refused.contains("the row of (\"list-z\") where (\"list-a\") was asked for"),
        "{refused}"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

/// The integrator's ruling 8 of 2026-10-25: a table read's row is exactly
/// the row tables.json declares. Until then the engine projected an answer
/// onto the component's type (ADR-0166), which drops a field the type does
/// not name: a `secret` reached no component, silently. Now a row with a
/// field more, or less, or out of its place, is refused.
#[test]
fn a_row_with_a_field_more_or_less_is_refused_not_projected() {
    let program = program();
    let fields = |list: Val| match list {
        Val::Record(fields) => fields,
        _ => unreachable!(),
    };
    let mut secret = fields(todo_list("list-a", "List A", 10));
    secret.push(("secret".into(), Val::String("leak".into())));
    let mut lacking = fields(todo_list("list-a", "List A", 10));
    lacking.pop();
    let mut reordered = fields(todo_list("list-a", "List A", 10));
    reordered.swap(0, 3);
    for (row, words) in [
        (
            secret,
            "the row has `secret`, which tables.json does not declare",
        ),
        (lacking, "the row lacks `added`"),
        (reordered, "`added` where tables.json declares `id`"),
    ] {
        for (id, answer, args) in [
            (
                BY_ID,
                Val::Option(Some(Box::new(Val::Record(row.clone())))),
                vec![Val::String("list-a".into())],
            ),
            (
                BY_IDS,
                Val::List(vec![Val::Record(row.clone())]),
                vec![Val::List(vec![Val::String("list-a".into())])],
            ),
        ] {
            let reader = program.reader(id);
            let granted = reader.admitted(&reader.contract, READ_LISTS);
            let (callback, calls) = answering(vec![answer]);
            let host = BTreeMap::from([(reader.imports[0].clone(), callback)]);
            let refused = reader
                .call(&granted, &host, &program.manifest, &args)
                .expect_err(words);
            assert!(refused.contains(words), "{id}: {words}: {refused}");
            assert_eq!(calls.load(Ordering::SeqCst), 1, "{id}");
        }
    }
}

/// PR #40, with the integrator's ruling 9 of 2026-10-25: a well-formed
/// `Val` of the wrong type for the compiled guest's WIT `Option<TodoList>`
/// is refused for its type, never read as a missing row, and a valid read
/// after it still works. #40's own case (`Some(s64)` for a row) was refused
/// by ADR-0179's invariant on `some.slug`, not by its type; it is kept, and
/// the case only a type can refuse is `added`, which holds no invariant, as
/// an s32.
#[test]
fn a_mistyped_get_answer_is_refused_for_its_type_and_a_valid_one_still_works() {
    let program = program();
    let reader = program.reader(BY_ID);
    let granted = reader.admitted(&reader.contract, READ_LISTS);
    let args = [Val::String("list-a".into())];
    let narrow = Val::Record(vec![
        ("id".into(), Val::String("list-a".into())),
        ("name".into(), Val::String("List A".into())),
        ("slug".into(), Val::String("list-a".into())),
        ("added".into(), Val::S32(10)),
    ]);
    for (answer, words) in [
        (
            Val::Option(Some(Box::new(narrow))),
            "`added`: s32 where tables.json declares an Int (s64)",
        ),
        (
            Val::Option(Some(Box::new(Val::S64(10)))),
            "s64 where a row is a record",
        ),
    ] {
        let (invalid, invalid_calls) = answering(vec![answer]);
        let host = BTreeMap::from([(GET.to_string(), invalid)]);
        let refused = reader
            .call(&granted, &host, &program.manifest, &args)
            .expect_err(words);
        assert!(refused.contains(words), "{refused}");
        assert_eq!(invalid_calls.load(Ordering::SeqCst), 1);
    }

    let present = Val::Option(Some(Box::new(todo_list("list-a", "List A", 10))));
    let (valid, valid_calls) = answering(vec![present.clone()]);
    let host = BTreeMap::from([(GET.to_string(), valid)]);
    assert_eq!(
        reader.call(&granted, &host, &program.manifest, &args),
        Ok(vec![present])
    );
    assert_eq!(valid_calls.load(Ordering::SeqCst), 1);
}
