//! **The other reads, typed where they link and held to what they were
//! asked** (ADR-0386): a prefix's, an index's, a unique index's, a set
//! form's and `all`, through `examples/tables/app.pw`'s compiled queries,
//! each refusal with the neighbour that links and answers.

mod common;

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use common::*;
use pw_host::engine::{HostFn, Val};
use serde_json::json;

fn text(value: &str) -> Val {
    Val::String(value.into())
}

fn table<'a>(json: &'a mut serde_json::Value, name: &str) -> &'a mut serde_json::Value {
    json["tables"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|table| table["name"] == name)
        .unwrap()
}

fn function<'a>(
    json: &'a mut serde_json::Value,
    table_name: &str,
    name: &str,
) -> &'a mut serde_json::Value {
    table(json, table_name)["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == name)
        .unwrap()
}

const READS: &[&str] = &["database.read<Lists>", "database.read<Items>"];

/// `reader`'s table imports, each answering `answer` and counting its calls.
fn answering_all(
    reader: &Reader,
    answer: Vec<Val>,
) -> (BTreeMap<String, HostFn>, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut host = BTreeMap::new();
    for import in &reader.imports {
        let (counted, answer) = (Arc::clone(&calls), answer.clone());
        let callback: HostFn = Arc::new(move |_| {
            counted.fetch_add(1, Ordering::SeqCst);
            Ok(answer.clone())
        });
        host.insert(import.clone(), callback);
    }
    (host, calls)
}

/// **Typed where it links**: a prefix's field of another type, a unique
/// index tables.json says is not (a list where the component takes an
/// option), an index's value of another type; each refused before anything
/// is instantiated, its callback never run.
#[test]
fn a_read_typed_otherwise_than_tables_json_is_refused_when_linking() {
    let program = app();
    type Edit = fn(&mut serde_json::Value);
    let cases: [(&str, Vec<Val>, &str, Edit); 3] = [
        (
            ITEMS_OF,
            vec![text("groceries")],
            "its prefix's `list`: string where tables.json declares an Int",
            |json| table(json, "Items")["fields"][0]["stored"] = json!({"int": {}}),
        ),
        (
            LIST_AT,
            vec![text("food")],
            "it answers (an option), not a list of rows",
            |json| {
                table(json, "Lists")["indexes"][0]["unique"] = json!(false);
                function(json, "Lists", "by_slug")["sorted_by"] = json!(["added", "id"]);
            },
        ),
        (
            OPEN_ITEMS,
            vec![],
            "`done`: bool where tables.json declares a String",
            |json| table(json, "Items")["fields"][3]["stored"] = json!({"string": {}}),
        ),
    ];
    for (id, args, words, edit) in cases {
        let reader = program.reader(id);
        let granted = reader.admitted(&reader.contract, &grants(READS));
        let manifest = manifest_where(&program.tables, edit).expect("a whole manifest");
        let (host, calls) = answering_all(reader, vec![Val::List(vec![])]);
        let refused = reader
            .call(&granted, &host, &manifest, &args)
            .expect_err(words);
        assert!(
            refused.contains("typed otherwise than tables.json declares the read")
                && refused.contains(words),
            "{id}: {words}: {refused}"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0, "{id}");
    }
    // The neighbours: the compiler's own manifest links each.
    for (id, args, answer) in [
        (ITEMS_OF, vec![text("groceries")], Val::List(vec![])),
        (LIST_AT, vec![text("food")], Val::Option(None)),
        (OPEN_ITEMS, vec![], Val::List(vec![])),
    ] {
        let reader = program.reader(id);
        let granted = reader.admitted(&reader.contract, &grants(READS));
        let (host, _) = answering_all(reader, vec![answer.clone()]);
        assert_eq!(
            reader.call(&granted, &host, &program.manifest, &args),
            Ok(vec![answer]),
            "{id}"
        );
    }
}

/// **Held to what it was asked, whoever answers**: a prefix answering
/// another list's item or its items out of order, an index answering an
/// item of the other value, a unique index another slug's list, and `all`
/// more lists than `Lists`' bound of 100; each refused by the engine.
#[test]
fn an_answer_not_its_reads_is_refused() {
    let program = app();
    let hundred_and_one: Vec<Val> = (0..101)
        .map(|n| list(&format!("l{n:03}"), &format!("s{n:03}"), n))
        .collect();
    let cases: Vec<(&str, Vec<Val>, Val, &str)> = vec![
        (
            ITEMS_OF,
            vec![text("groceries")],
            Val::List(vec![item("chores", "sweep", false, 1)]),
            "(\"chores\") was not asked for",
        ),
        (
            ITEMS_OF,
            vec![text("groceries")],
            Val::List(vec![
                item("groceries", "milk", false, 3),
                item("groceries", "eggs", false, 1),
            ]),
            "out of tables.json's order",
        ),
        (
            OPEN_ITEMS,
            vec![],
            Val::List(vec![item("groceries", "eggs", true, 1)]),
            "(true) was not asked for",
        ),
        (
            LIST_AT,
            vec![text("food")],
            Val::Option(Some(Box::new(list("chores", "home", 1)))),
            "the row of (\"home\") where (\"food\") was asked for",
        ),
        (
            EVERY_LIST,
            vec![],
            Val::List(hundred_and_one.clone()),
            "101 rows, where Lists is bounded to 100",
        ),
    ];
    for (id, args, answer, words) in cases {
        let reader = program.reader(id);
        let granted = reader.admitted(&reader.contract, &grants(READS));
        let (host, calls) = answering_all(reader, vec![answer]);
        let refused = reader
            .call(&granted, &host, &program.manifest, &args)
            .expect_err(words);
        assert!(refused.contains(words), "{id}: {words}: {refused}");
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "{id}: the answer, not the link"
        );
    }
    // The neighbours: each read's own rows, in order, within the bound.
    let mut hundred = hundred_and_one;
    hundred.pop();
    for (id, args, answer) in [
        (
            ITEMS_OF,
            vec![text("groceries")],
            Val::List(vec![
                item("groceries", "eggs", false, 1),
                item("groceries", "milk", false, 3),
            ]),
        ),
        (
            OPEN_ITEMS,
            vec![],
            Val::List(vec![item("groceries", "eggs", false, 1)]),
        ),
        (
            LIST_AT,
            vec![text("food")],
            Val::Option(Some(Box::new(list("groceries", "food", 1)))),
        ),
        (EVERY_LIST, vec![], Val::List(hundred)),
    ] {
        let reader = program.reader(id);
        let granted = reader.admitted(&reader.contract, &grants(READS));
        let (host, _) = answering_all(reader, vec![answer.clone()]);
        assert_eq!(
            reader.call(&granted, &host, &program.manifest, &args),
            Ok(vec![answer]),
            "{id}"
        );
    }
}

/// **A manifest is whole for every primitive**: each reads by what its
/// primitive reads by, under the name D1 gives it, in D1's order; an index
/// is one of its fields; `all` only of a bounded table. Each refusal names
/// what is wrong; the compiler's own manifest is the neighbour.
#[test]
fn a_manifest_is_whole_for_every_primitive() {
    let program = program();
    type Edit = fn(&mut serde_json::Value);
    let cases: [(&str, Edit); 10] = [
        (
            "reads by other than a proper prefix of Items's key",
            |json| function(json, "Items", "of_list")["by"] = json!(["id"]),
        ),
        ("reads by other than one of Items's indexes", |json| {
            function(json, "Items", "by_done")["by"] = json!(["title"])
        }),
        ("is an index, imported elsewhere than `#by-done`", |json| {
            let by_done = function(json, "Items", "by_done");
            by_done["name"] = json!("by_finished");
            by_done["import"] = json!("pw:tables/items#by-finished");
        }),
        ("is an all of Lists, which states no bound", |json| {
            table(json, "Lists")
                .as_object_mut()
                .unwrap()
                .remove("bounded");
        }),
        (
            "answers by (list, id), where D1's order is (added, list, id)",
            |json| function(json, "Items", "of_list")["sorted_by"] = json!(["list", "id"]),
        ),
        ("answers by (added, id), where D1's order is ()", |json| {
            function(json, "Lists", "by_slug")["sorted_by"] = json!(["added", "id"])
        }),
        (
            "is given a whole row or nothing, not Lists's fields",
            |json| function(json, "Lists", "insert")["by"] = json!(["id"]),
        ),
        (
            "index `by_list_done` is not one index of its fields",
            |json| {
                table(json, "Items")["indexes"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!({"name": "by_list_done", "fields": ["done"], "unique": true}))
            },
        ),
        ("Lists is bounded to no row", |json| {
            table(json, "Lists")["bounded"] = json!(0)
        }),
        ("Items's key names a field twice", |json| {
            table(json, "Items")["key"] = json!(["list", "list"])
        }),
    ];
    for (words, edit) in cases {
        let refused = manifest_where(&program.tables, edit)
            .expect_err(&format!("a manifest whose {words} is refused"));
        assert!(refused.contains(words), "{words}: {refused}");
    }
    assert!(manifest_where(&program.tables, |_| {}).is_ok());
}
