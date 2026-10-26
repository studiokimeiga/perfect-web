//! **A table's write links through the gate, into a command, typed, and
//! answers only as its write** (ADR-0386), with the compiler's own
//! `examples/tables/app.pw` commands: the refusals, each with the neighbour
//! that links and runs.

mod common;

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use common::*;
use pw_host::engine::{HostFn, Val};
use pw_table_memory::{Error, MemoryTables};
use serde_json::json;

const NODE: &[&str] = &[
    "database.read<Lists>",
    "database.read<Items>",
    "database.write<Lists>",
    "database.write<Items>",
    "outbox.write",
];

fn text(value: &str) -> Val {
    Val::String(value.into())
}

/// `id`'s host functions in a command of `tables`, and its platform's.
fn host_of(
    program: &Program,
    tables: &MemoryTables,
    staged: &pw_table_memory::Command,
    id: &str,
    contract: &pw_host::ComponentContract,
) -> (pw_host::Granted, BTreeMap<String, HostFn>) {
    let reader = program.reader(id);
    let granted = reader.admitted(contract, &grants(NODE));
    let mut host = staged
        .host_fns(contract, &granted)
        .expect("the gate admits the command's functions");
    host.extend(platform(reader).0);
    let _ = tables;
    (granted, host)
}

/// **A write links only into a command.** `make_list`'s contract, its export
/// declared a query, links its insert nowhere: the engine refuses it, and the
/// query's gate refuses the contract. The real contract links and runs.
#[test]
fn a_write_links_only_into_a_command() {
    let program = app();
    let tables = MemoryTables::new(program.tables.clone()).unwrap();
    let maker = program.reader(MAKE_LIST);
    let mut query = maker.contract.clone();
    query.exports[0].kind = "query".into();
    let staged = tables.begin().unwrap();
    // A malicious host can still hand the engine callbacks constructed for
    // the real command; the engine independently refuses their query use.
    let (granted, host) = host_of(&program, &tables, &staged, MAKE_LIST, &maker.contract);
    assert!(
        matches!(staged.host_fns(&query, &granted), Err(Error::Input(why))
        if why.contains("one explicit command export"))
    );
    let refused = maker
        .call_as(
            &query,
            &granted,
            &host,
            Some(tables.host_tables()),
            &[list("todo", "work", 1)],
        )
        .expect_err("a write in a query");
    assert!(
        refused.contains("pw:tables/lists#insert")
            && refused.contains("a table's write links only into a command"),
        "{refused}"
    );
    assert!(matches!(
        tables.read_host_fns(&query, &granted),
        Err(Error::Input(why)) if why.contains("links only into a command")
    ));
    drop(staged);
    assert_eq!(
        tables.read("pw:tables/lists#get", &[json!("todo")]),
        Ok(serde_json::Value::Null)
    );
    // The neighbour: its own contract, a command.
    let staged = tables.begin().unwrap();
    let (granted, host) = host_of(&program, &tables, &staged, MAKE_LIST, &maker.contract);
    assert_eq!(
        maker.call(
            &granted,
            &host,
            tables.host_tables(),
            &[list("todo", "work", 1)]
        ),
        Ok(vec![Val::Result(Ok(None))])
    );
}

/// **A write's capability is its table's, granted.** A contract claiming
/// `database.write<Other>` on `pw:tables/lists#insert`, on a node granting
/// `Other`, is admitted and links nothing: the gate refuses it before any
/// callback runs. A node that does not grant `database.write<Lists>` admits
/// no `make_list`.
#[test]
fn a_forged_or_ungranted_write_is_refused() {
    let program = app();
    let tables = MemoryTables::new(program.tables.clone()).unwrap();
    let maker = program.reader(MAKE_LIST);
    let mut forged = maker.contract.clone();
    let insert = forged
        .imports
        .iter_mut()
        .find(|i| i.key() == "pw:tables/lists#insert")
        .unwrap();
    insert.capability = "database.write<Other>".into();
    let required = forged
        .required_capabilities
        .iter_mut()
        .find(|c| c.argument.as_deref() == Some("Lists"))
        .unwrap();
    required.argument = Some("Other".into());
    let granted = maker.admitted(&forged, &grants(&["database.write<Other>", "outbox.write"]));
    let staged = tables.begin().unwrap();
    assert!(matches!(
        staged.host_fns(&forged, &granted),
        Err(Error::Input(why)) if why.contains("database.write<Other>")
    ));
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&calls);
    let callback: HostFn = Arc::new(move |_| {
        counted.fetch_add(1, Ordering::SeqCst);
        Ok(vec![Val::Result(Ok(None))])
    });
    let mut host = platform(maker).0;
    host.insert("pw:tables/lists#insert".into(), callback);
    let refused = maker
        .call_as(
            &forged,
            &granted,
            &host,
            Some(tables.host_tables()),
            &[list("todo", "work", 1)],
        )
        .expect_err("the gate refuses another table's capability");
    assert!(refused.contains("database.write<Other>"), "{refused}");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let denied = maker.admission(
        &maker.contract,
        &grants(&["database.read<Lists>", "outbox.write"]),
    );
    assert!(
        denied
            .refusals()
            .iter()
            .any(|reason| matches!(reason, pw_host::Refusal::Ungranted { .. })),
        "{denied:?}"
    );
}

/// tables.json's `Lists`, edited.
fn lists(json: &mut serde_json::Value) -> &mut serde_json::Value {
    table(json, "Lists")
}

fn table<'a>(json: &'a mut serde_json::Value, name: &str) -> &'a mut serde_json::Value {
    json["tables"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|table| table["name"] == name)
        .unwrap()
}

/// **A write's import is typed as tables.json declares it, where it links**:
/// a row's field of another type, a put that tables.json says answers its
/// refusal where the component's answers nothing, a delete's key of another
/// type: each refused before anything is instantiated, its callback never
/// run.
#[test]
fn a_write_typed_otherwise_than_tables_json_is_refused_when_linking() {
    let program = app();
    type Edit = fn(&mut serde_json::Value);
    let cases: [(&str, Vec<Val>, &str, Edit); 4] = [
        (
            MAKE_LIST,
            vec![list("todo", "work", 1)],
            "its refusal's row: `added`: s64 where tables.json declares a String",
            |json| lists(json)["fields"][3]["stored"] = json!({"string": {}}),
        ),
        (
            KEEP_ITEM,
            vec![item("groceries", "milk", false, 1)],
            "its row: `added`: s64 where tables.json declares a String",
            |json| table(json, "Items")["fields"][5]["stored"] = json!({"string": {}}),
        ),
        (
            KEEP_ITEM,
            vec![item("groceries", "milk", false, 1)],
            "it answers (), not a result of the row that refuses it",
            |json| {
                table(json, "Items")["indexes"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!({"name": "by_title", "fields": ["title"], "unique": true}))
            },
        ),
        (
            DROP_ITEM,
            vec![text("groceries"), text("milk")],
            "its key's `id`: string where tables.json declares an Int",
            |json| table(json, "Items")["fields"][1]["stored"] = json!({"int": {}}),
        ),
    ];
    for (id, args, words, edit) in cases {
        let reader = program.reader(id);
        let manifest = manifest_where(&program.tables, edit).expect("a whole manifest");
        let granted = reader.admitted(&reader.contract, &grants(NODE));
        let calls = Arc::new(AtomicUsize::new(0));
        let mut host = platform(reader).0;
        for import in &reader.imports {
            if pw_host::tables::is_table_import(import) {
                let counted = Arc::clone(&calls);
                let callback: HostFn = Arc::new(move |_| {
                    counted.fetch_add(1, Ordering::SeqCst);
                    Ok(Vec::new())
                });
                host.insert(import.clone(), callback);
            }
        }
        let refused = reader
            .call(&granted, &host, &manifest, &args)
            .expect_err(words);
        assert!(
            refused.contains("typed otherwise than tables.json declares the write")
                && refused.contains(words),
            "{id}: {words}: {refused}"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0, "{id}");
        // The neighbour: the compiler's own manifest, and the adapter.
        let tables = MemoryTables::new(program.tables.clone()).unwrap();
        tables
            .transaction(|tx| {
                tx.write(
                    "pw:tables/lists#insert",
                    &[list_json("groceries", "food", 1)],
                )
            })
            .unwrap();
        let staged = tables.begin().unwrap();
        let mut host = staged.host_fns(&reader.contract, &granted).unwrap();
        host.extend(platform(reader).0);
        assert!(
            reader
                .call(&granted, &host, tables.host_tables(), &args)
                .is_ok(),
            "{id}"
        );
    }
}

/// **A write that tables.json says answers nothing links only where the
/// component's answers nothing**: `writer.pw`'s `rename` puts a list, which a
/// unique slug lets be refused; with the slug's index plain in tables.json,
/// its import answers what the write does not, and is refused where it
/// links. The compiler's own manifest links and runs it, and its refusal is
/// the list that holds the slug.
#[test]
fn a_write_answering_what_tables_json_says_it_does_not_is_refused() {
    let program = writer();
    let renamer = program.reader(RENAME);
    let granted = renamer.admitted(&renamer.contract, &grants(NODE));
    let plain = manifest_where(&program.tables, |json| {
        lists(json)["indexes"][0]["unique"] = json!(false);
        for function in lists(json)["functions"].as_array_mut().unwrap() {
            if function["name"] == "by_slug" {
                function["sorted_by"] = json!(["added", "id"]);
            }
        }
    })
    .expect("a whole manifest");
    let (callback, calls) = answering(Vec::new());
    let host = BTreeMap::from([("pw:tables/lists#put".to_string(), callback)]);
    let refused = renamer
        .call(&granted, &host, &plain, &[list("todo", "work", 1)])
        .expect_err("a put that tables.json says cannot be refused");
    assert!(
        refused.contains("it answers (a result), not nothing"),
        "{refused}"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    // The neighbour: the compiler's manifest, on the adapter.
    let tables = MemoryTables::new(program.tables.clone()).unwrap();
    tables
        .transaction(|tx| {
            tx.write("pw:tables/lists#insert", &[list_json("todo", "work", 1)])?;
            tx.write("pw:tables/lists#insert", &[list_json("next", "play", 2)])
        })
        .unwrap();
    let staged = tables.begin().unwrap();
    let host = staged.host_fns(&renamer.contract, &granted).unwrap();
    assert_eq!(
        renamer.call(
            &granted,
            &host,
            tables.host_tables(),
            &[list("next", "work", 2)]
        ),
        Ok(vec![Val::Result(Err(Some(Box::new(list(
            "todo", "work", 1
        )))))])
    );
    assert_eq!(
        renamer.call(
            &granted,
            &host,
            tables.host_tables(),
            &[list("next", "fun", 2)]
        ),
        Ok(vec![Val::Result(Ok(None))])
    );
}

/// **A write answers only as its write**, whoever answers it: an insert's
/// refusal by a row holding neither its key nor its unique slug, and a
/// delete answering a value, are refused by the engine.
#[test]
fn a_writes_answer_that_is_not_its_own_is_refused() {
    let program = app();
    let tables = MemoryTables::new(program.tables.clone()).unwrap();
    let maker = program.reader(MAKE_LIST);
    let granted = maker.admitted(&maker.contract, &grants(NODE));
    let admitted = tables
        .host_tables()
        .admit_command(&maker.contract, &granted)
        .unwrap();
    let Some(pw_host::tables::Access::Write(insert)) =
        admitted.get("pw:tables/lists#insert").cloned()
    else {
        panic!("the gate admits make_list's insert");
    };
    let mut host = platform(maker).0;
    host.insert(
        "pw:tables/lists#insert".into(),
        pw_host::engine::table_write_host::host_fn(
            insert,
            Arc::new(|_| Ok(Some(list_json("other", "elsewhere", 9)))),
        ),
    );
    let refused = maker
        .call(
            &granted,
            &host,
            tables.host_tables(),
            &[list("todo", "work", 1)],
        )
        .expect_err("a row that refuses nothing");
    assert!(
        refused.contains("holds nothing the insert of (\"todo\") would"),
        "{refused}"
    );
    let dropper = program.reader(DROP_ITEM);
    let granted = dropper.admitted(&dropper.contract, &grants(NODE));
    let mut host = platform(dropper).0;
    let answering: HostFn = Arc::new(|_| Ok(vec![Val::Bool(true)]));
    host.insert("pw:tables/items#delete".into(), answering);
    let refused = dropper
        .call(
            &granted,
            &host,
            tables.host_tables(),
            &[text("groceries"), text("milk")],
        )
        .expect_err("a delete answers nothing");
    assert!(
        refused.contains("1 values where a delete answers nothing"),
        "{refused}"
    );
}

/// A put replaces its own key; only another key sharing a unique value
/// can legitimately refuse it. An unrelated row grants no such refusal.
#[test]
fn a_put_refusal_must_hold_a_unique_value_for_another_key() {
    let program = writer();
    let tables = MemoryTables::new(program.tables.clone()).unwrap();
    let renamer = program.reader(RENAME);
    let granted = renamer.admitted(&renamer.contract, &grants(NODE));
    let given = list("next", "work", 2);
    for held in [list("other", "elsewhere", 9), list("next", "work", 2)] {
        let (callback, calls) = answering(vec![Val::Result(Err(Some(Box::new(held))))]);
        let host = BTreeMap::from([("pw:tables/lists#put".to_string(), callback)]);
        let answer = renamer.call(
            &granted,
            &host,
            tables.host_tables(),
            std::slice::from_ref(&given),
        );
        assert!(
            answer.is_err(),
            "a put may not be refused by an unrelated row or its own key"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
    let held = list("other", "work", 9);
    let expected = vec![Val::Result(Err(Some(Box::new(held))))];
    let (callback, calls) = answering(expected.clone());
    let host = BTreeMap::from([("pw:tables/lists#put".to_string(), callback)]);
    assert_eq!(
        renamer.call(&granted, &host, tables.host_tables(), &[given]),
        Ok(expected)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}
