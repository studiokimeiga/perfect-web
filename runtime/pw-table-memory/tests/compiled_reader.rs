//! A compiled component reads the memory adapter through exactly the table
//! host functions its admitted contract names (ADR-0338 D2, issue #27), and a
//! table import links through the table gate or not at all.
//!
//! The pieces were written apart: the adapter's import gate and selector
//! checks (PRs #29, #33), the host's WIT selector and row codec and its
//! callback (#30 to #32), and the compiled readers' admission and Wasmtime
//! calls with test callbacks (#34 to #40). Here they meet: the compiler's
//! own `tables.reader` components, admitted by the imports Wasmtime reads
//! from their bytes, call `MemoryTables` through
//! [`MemoryTables::read_host_fns`], linked by
//! `pw_host::engine::call_with_tables` with the program's tables.json.

mod common;

#[path = "support/assert_result.rs"]
mod assert_result;
use assert_result::*;

use std::collections::BTreeMap;
use std::sync::Arc;

use common::{
    BY_ID, BY_IDS, GET, GET_MANY, READ_LISTS, answering, manifest_where, program, todo_list,
};
use pw_host::engine::Val;
use pw_table_memory::{Error, MemoryTables};
use serde_json::json;

/// The program's tables.json in a memory adapter holding three lists.
fn seeded(tables: &pw_core::tables::TablesJson) -> Arc<MemoryTables> {
    let adapter = Arc::new(MemoryTables::new(tables.clone()).assert_ok("the adapter"));
    adapter
        .transaction(|tx| {
            for (id, name, added) in [("list-b", "List B", 20), ("list-a", "List A", 10)] {
                tx.write(
                    "pw:tables/lists#insert",
                    &[json!({"id": id, "name": name, "slug": id, "added": added})],
                )?;
            }
            tx.write(
                "pw:tables/lists#insert",
                &[json!({"id": "list-c", "name": "List C", "slug": "list-c", "added": 5})],
            )
        })
        .assert_ok("seeded through the adapter's own transaction");
    adapter
}

#[test]
fn compiled_get_reads_present_and_missing_rows_from_the_memory_adapter() {
    let program = program();
    let tables = seeded(&program.tables);
    let reader = program.reader(BY_ID);
    let granted = reader.admitted(&reader.contract, READ_LISTS);
    let host = tables
        .read_host_fns(&reader.contract, &granted)
        .assert_ok("the admitted get is served");
    assert_eq!(
        host.keys().collect::<Vec<_>>(),
        [GET],
        "exactly the reader's own import, not its table's other reads"
    );

    assert_eq!(
        reader.call(
            &granted,
            &host,
            tables.host_tables(),
            &[Val::String("list-a".into())]
        ),
        Ok(vec![Val::Option(Some(Box::new(todo_list(
            "list-a", "List A", 10
        ))))]),
        "a present key answers the stored row, typed as the WIT record"
    );
    assert_eq!(
        reader.call(
            &granted,
            &host,
            tables.host_tables(),
            &[Val::String("missing".into())]
        ),
        Ok(vec![Val::Option(None)]),
        "a missing key answers none, not a default row"
    );
}

#[test]
fn compiled_get_many_answers_the_declared_order_once_per_present_key() {
    let program = program();
    let tables = seeded(&program.tables);
    let reader = program.reader(BY_IDS);
    let granted = reader.admitted(&reader.contract, READ_LISTS);
    let host = tables
        .read_host_fns(&reader.contract, &granted)
        .assert_ok("the admitted get-many is served");
    assert_eq!(host.keys().collect::<Vec<_>>(), [GET_MANY]);

    let keys = ["list-b", "missing", "list-a", "list-b"]
        .map(|id| Val::String(id.into()))
        .to_vec();
    assert_eq!(
        reader.call(&granted, &host, tables.host_tables(), &[Val::List(keys)]),
        Ok(vec![Val::List(vec![
            todo_list("list-a", "List A", 10),
            todo_list("list-b", "List B", 20),
        ])]),
        "the declared order (by key), each present row once, a missing key none"
    );
    assert_eq!(
        reader.call(&granted, &host, tables.host_tables(), &[Val::List(vec![])]),
        Ok(vec![Val::List(vec![])])
    );
}

#[test]
fn no_table_host_function_without_the_tables_read_grant() {
    let program = program();
    let tables = seeded(&program.tables);
    let reader = program.reader(BY_ID);
    // The real contract, its requirement removed so that a node granting
    // nothing admits it: what is admitted then holds no read of Lists, and
    // the adapter installs nothing for it.
    let mut ungranted = reader.contract.clone();
    ungranted.required_capabilities.clear();
    let granted = reader.admitted(&ungranted, "");
    assert!(matches!(
        tables.read_host_fns(&ungranted, &granted),
        Err(Error::Input(reason)) if reason.contains(GET)
    ));
    // Nor may the host link the read some other way: with no callback for
    // the granted import, the call is refused rather than stubbed.
    let granted = reader.admitted(&reader.contract, READ_LISTS);
    let refused = reader
        .call(
            &granted,
            &BTreeMap::new(),
            tables.host_tables(),
            &[Val::String("list-a".into())],
        )
        .assert_err("a granted import with no callback is refused");
    assert!(refused.contains("implements nothing"), "{refused}");
}

/// The integrator's ruling 2 of 2026-10-25: `linkable` trusts an import's own
/// claim of its capability, so a contract claiming `database.read<Other>` on
/// `pw:tables/lists#get`, on a node granting `Other`, is admitted, and until
/// this linked whatever the host map held for the import.
#[test]
fn a_forged_table_capability_is_refused_when_linking() {
    let program = program();
    let reader = program.reader(BY_ID);
    let mut forged = reader.contract.clone();
    forged.imports[0].capability = "database.read<Other>".into();
    forged.required_capabilities[0].argument = Some("Other".into());
    let granted = reader.admitted(&forged, r#""database.read<Other>""#);
    assert!(pw_host::linkable(&forged, &granted).contains(&GET.to_string()));

    let (callback, calls) = answering(vec![Val::Option(Some(Box::new(todo_list(
        "list-a", "List A", 10,
    ))))]);
    let host = BTreeMap::from([(GET.to_string(), callback)]);
    let args = [Val::String("list-a".into())];

    let without = reader
        .call_as(&forged, &granted, &host, None, &args)
        .assert_err("no table import links without the program's tables");
    assert!(without.contains("table gate"), "{without}");
    let gated = reader
        .call_as(&forged, &granted, &host, Some(&program.manifest), &args)
        .assert_err("the gate refuses a capability other than its table's");
    assert!(
        gated.contains(GET) && gated.contains("database.read<Other>"),
        "{gated}"
    );
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);

    // The neighbour: the real contract and its grant link and call it.
    let granted = reader.admitted(&reader.contract, READ_LISTS);
    assert_eq!(
        reader.call(&granted, &host, &program.manifest, &args),
        Ok(vec![Val::Option(Some(Box::new(todo_list(
            "list-a", "List A", 10
        ))))])
    );
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
}

/// Ruling 2 again: with its real contract and grant, a reader's table import
/// links only through `call_with_tables`; no other way to link one is left.
#[test]
fn no_other_call_or_instantiation_links_a_table_import() {
    let program = program();
    let reader = program.reader(BY_ID);
    let granted = reader.admitted(&reader.contract, READ_LISTS);
    let (callback, calls) = answering(vec![Val::Option(None)]);
    let host = BTreeMap::from([(GET.to_string(), callback)]);

    let call = reader
        .call_as(
            &reader.contract,
            &granted,
            &host,
            None,
            &[Val::String("x".into())],
        )
        .assert_err("call_within links no table import");
    assert!(call.contains("table gate"), "{call}");
    let instantiated = pw_host::engine::instantiate(&reader.bytes, &reader.contract, &granted)
        .assert_err("instantiation links no table import, not even a stub");
    assert!(instantiated.contains("table gate"), "{instantiated}");
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
}

/// The integrator's ruling 4 of 2026-10-25: the engine links a function
/// with the type the component declares, so a table import typed otherwise
/// than tables.json declares its read linked, and was called. It is refused
/// where it is linked now, before the component is instantiated, and the
/// callback never runs.
#[test]
fn a_table_import_typed_otherwise_than_tables_json_is_refused_when_linking() {
    let program = program();
    fn lists(json: &mut serde_json::Value) -> &mut serde_json::Value {
        json["tables"]
            .as_array_mut()
            .assert_ok("valid domain result")
            .iter_mut()
            .find(|table| table["name"] == "Lists")
            .assert_ok("valid domain result")
    }
    type Edit = fn(&mut serde_json::Value);
    let rows: [(&str, Edit); 3] = [
        ("`added`: s64 where tables.json declares a String", |json| {
            lists(json)["fields"][3]["stored"] = json!({"string": {}})
        }),
        (
            "a record of 4 field(s) where tables.json declares 5",
            |json| {
                let secret = json!({"name": "secret", "type": "String", "stored": {"string": {}}});
                lists(json)["fields"]
                    .as_array_mut()
                    .assert_ok("valid domain result")
                    .push(secret);
            },
        ),
        ("field `name` where tables.json declares `title`", |json| {
            lists(json)["fields"][1]["name"] = json!("title")
        }),
    ];
    let key: (&str, Edit) = (
        "its key's `id`: string where tables.json declares an Int",
        |json| lists(json)["fields"][0]["stored"] = json!({"int": {}}),
    );
    for (id, args) in [
        (BY_ID, vec![Val::String("list-a".into())]),
        (BY_IDS, vec![Val::List(vec![Val::String("list-a".into())])]),
    ] {
        let reader = program.reader(id);
        let granted = reader.admitted(&reader.contract, READ_LISTS);
        let import = reader.imports[0].clone();
        let answer = if id == BY_ID {
            Val::Option(Some(Box::new(todo_list("list-a", "List A", 10))))
        } else {
            Val::List(vec![todo_list("list-a", "List A", 10)])
        };
        let (callback, calls) = answering(vec![answer.clone()]);
        let host = BTreeMap::from([(import, callback)]);
        for (words, edit) in rows.iter().chain([&key]) {
            let manifest = manifest_where(&program.tables, *edit).assert_ok("a whole manifest");
            let refused = reader
                .call(&granted, &host, &manifest, &args)
                .assert_err(&format!("{id} refused when linked: {words}"));
            assert!(
                refused.contains("typed otherwise than tables.json") && refused.contains(words),
                "{id}: {words}: {refused}"
            );
        }
        // A key of two fields in tables.json, where the reader was built
        // for one: a get takes one value, a get-many's keys are not records.
        // The manifest is whole otherwise: each function by the new key, in
        // D1's order by it.
        let manifest = manifest_where(&program.tables, |json| {
            lists(json)["key"] = json!(["id", "slug"]);
            for function in lists(json)["functions"]
                .as_array_mut()
                .assert_ok("valid domain result")
            {
                let name = function["name"]
                    .as_str()
                    .assert_ok("valid domain result")
                    .to_string();
                if ["get", "get_many", "delete"].contains(&name.as_str()) {
                    function["by"] = json!(["id", "slug"]);
                }
                match name.as_str() {
                    "get_many" => function["sorted_by"] = json!(["id", "slug", "added"]),
                    "by_slug_many" => function["sorted_by"] = json!(["slug", "added", "id"]),
                    "all" => function["sorted_by"] = json!(["added", "id", "slug"]),
                    _ => {}
                }
            }
        })
        .assert_ok("a whole manifest");
        let words = if id == BY_ID {
            "it takes (string), not the key's 2 field(s)"
        } else {
            "its key: string where a record is declared"
        };
        let refused = reader
            .call(&granted, &host, &manifest, &args)
            .assert_err(&format!("{id} refused when linked: {words}"));
        assert!(refused.contains(words), "{id}: {refused}");
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0, "{id}");
        // The neighbour: typed as its tables.json declares, it links and runs.
        assert_eq!(
            reader.call(&granted, &host, &program.manifest, &args),
            Ok(vec![answer]),
            "{id}"
        );
    }
}

/// The integrator's rulings 6 to 8 of 2026-10-25, through the composed
/// callback any adapter would use (`table_read_host::host_fn`): an adapter's
/// row is encoded as the record tables.json declares, in its order, or
/// refused whole (a field more, `secret`, or less); and an adapter that
/// answers another key's row, or rows out of order, is refused by the
/// engine whatever encoded them.
#[test]
fn an_adapters_answer_is_held_to_its_read_whoever_wrote_it() {
    use pw_host::engine::table_read_host::host_fn;
    let program = program();
    let row = |id: &str| json!({"added": 10, "id": id, "name": "List A", "slug": id});
    for (id, answer, args, words) in [
        (
            BY_ID,
            {
                let mut secret = row("list-a");
                secret["secret"] = json!("leak");
                secret
            },
            vec![Val::String("list-a".into())],
            "it has `secret`, which tables.json does not declare",
        ),
        (
            BY_ID,
            json!({"id": "list-a", "name": "List A", "slug": "list-a"}),
            vec![Val::String("list-a".into())],
            "it lacks `added`",
        ),
        (
            BY_ID,
            row("list-z"),
            vec![Val::String("list-a".into())],
            "the row of (\"list-z\") where (\"list-a\") was asked for",
        ),
        (
            BY_IDS,
            json!([row("list-b"), row("list-a")]),
            vec![Val::List(vec![
                Val::String("list-a".into()),
                Val::String("list-b".into()),
            ])],
            "out of tables.json's order",
        ),
    ] {
        let reader = program.reader(id);
        let granted = reader.admitted(&reader.contract, READ_LISTS);
        let reads = program
            .manifest
            .admit_reads(&reader.contract, &granted)
            .assert_ok("the gate admits the reader's read");
        let (import, read) = reads.into_iter().next().assert_ok("one read");
        let data = Arc::new(move |_: &[serde_json::Value]| Ok(answer.clone()));
        let host = BTreeMap::from([(import, host_fn(read, data))]);
        let refused = reader
            .call(&granted, &host, &program.manifest, &args)
            .assert_err(words);
        assert!(refused.contains(words), "{id}: {words}: {refused}");
    }
}

/// The integrator's ruling 3 of 2026-10-25: PR #29's gate held a function's
/// import to its table's interface, and no test failed without that. A
/// manifest whose `Lists.get` is imported at `Items`' interface is refused
/// where it is read, so no gate ever sees it.
#[test]
fn a_manifest_import_outside_its_tables_interface_is_refused() {
    let program = program();
    // Lists' function whose import is `import`, edited by `edit`.
    let lists_function = |import: &'static str, edit: fn(&mut serde_json::Value)| {
        move |json: &mut serde_json::Value| {
            let lists = json["tables"]
                .as_array_mut()
                .assert_ok("valid domain result")
                .iter_mut()
                .find(|table| table["name"] == "Lists")
                .assert_ok("valid domain result");
            let function = lists["functions"]
                .as_array_mut()
                .assert_ok("valid domain result")
                .iter_mut()
                .find(|function| function["import"] == import)
                .assert_ok("valid domain result");
            edit(function);
        }
    };
    let get_elsewhere = lists_function(GET, |get| get["import"] = json!("pw:tables/elsewhere#get"));
    let refused = manifest_where(&program.tables, get_elsewhere).assert_err("refused when read");
    assert!(
        refused.contains("outside `pw:tables/lists#get`"),
        "{refused}"
    );
    let mut changed = serde_json::to_value(&program.tables).assert_ok("valid domain result");
    get_elsewhere(&mut changed);
    let changed: pw_core::tables::TablesJson =
        serde_json::from_value(changed).assert_ok("valid domain result");
    assert!(matches!(
        MemoryTables::new(changed),
        Err(Error::Schema(reason)) if reason.contains("outside")
    ));

    // Any function, not only the reads the host serves today.
    let index_elsewhere = lists_function("pw:tables/lists#by-slug", |index| {
        index["import"] = json!("pw:tables/elsewhere#by-slug")
    });
    let refused = manifest_where(&program.tables, index_elsewhere).assert_err("refused when read");
    assert!(
        refused.contains("outside `pw:tables/lists#by-slug`"),
        "{refused}"
    );

    // A get at its own table's interface, but not at `#get`.
    let get_renamed = lists_function(GET, |get| {
        get["name"] = json!("fetch");
        get["import"] = json!("pw:tables/lists#fetch");
    });
    let refused = manifest_where(&program.tables, get_renamed).assert_err("refused when read");
    assert!(refused.contains("elsewhere than `#get`"), "{refused}");

    // The neighbour: the compiler's own manifest is whole.
    assert!(manifest_where(&program.tables, |_| {}).is_ok());
    assert!(MemoryTables::new(program.tables.clone()).is_ok());
}

/// A manifest is held whole where the host reads it (`Manifest::from_json`),
/// so that what the gate and the engine read of it is what the compiler
/// wrote: each refusal names what is wrong, and the compiler's own manifest
/// is the neighbour that passes.
#[test]
fn a_manifest_that_is_not_whole_is_refused_where_it_is_read() {
    let program = program();
    fn lists(json: &mut serde_json::Value) -> &mut serde_json::Value {
        json["tables"]
            .as_array_mut()
            .assert_ok("valid domain result")
            .iter_mut()
            .find(|table| table["name"] == "Lists")
            .assert_ok("valid domain result")
    }
    fn function<'a>(json: &'a mut serde_json::Value, import: &str) -> &'a mut serde_json::Value {
        lists(json)["functions"]
            .as_array_mut()
            .assert_ok("valid domain result")
            .iter_mut()
            .find(|function| function["import"] == import)
            .assert_ok("valid domain result")
    }
    type Edit = fn(&mut serde_json::Value);
    let cases: [(&str, Edit); 9] = [
        ("interface is `pw:tables/items`", |json| {
            lists(json)["interface"] = json!("pw:tables/items")
        }),
        ("repeats a field", |json| {
            let name = lists(json)["fields"][1].clone();
            lists(json)["fields"]
                .as_array_mut()
                .assert_ok("valid domain result")
                .push(name);
        }),
        ("key is not its fields'", |json| {
            lists(json)["key"] = json!(["nope"])
        }),
        ("order is not one of its fields", |json| {
            lists(json)["order"] = json!("nope")
        }),
        ("is two functions'", |json| {
            let get = function(json, GET).clone();
            lists(json)["functions"]
                .as_array_mut()
                .assert_ok("valid domain result")
                .push(get);
        }),
        ("is a read, not a write", |json| {
            function(json, GET)["effect"] = json!("write")
        }),
        ("names a field Lists lacks", |json| {
            function(json, GET_MANY)["by"] = json!(["nope"])
        }),
        ("names a field Lists lacks", |json| {
            function(json, GET_MANY)["sorted_by"] = json!(["nope"])
        }),
        ("reads by other than Lists's key", |json| {
            function(json, GET)["by"] = json!(["slug"])
        }),
    ];
    for (words, edit) in cases {
        let refused = manifest_where(&program.tables, edit)
            .assert_err(&format!("a manifest whose Lists {words} is refused"));
        assert!(refused.contains(words), "{words}: {refused}");
    }
    assert!(manifest_where(&program.tables, |_| {}).is_ok());
}

/// The host names a record's fields as the compiler does, so that a row the
/// host builds is the record the component declares.
#[test]
fn the_hosts_wit_names_are_the_compilers() {
    let program = program();
    let mut names: Vec<String> = [
        "sort_order",
        "sortOrder",
        "HTTPServer",
        "a__b",
        "_x",
        "x_",
        "1abc",
        "",
        "list2",
        "Cart.Lines",
        "x-y",
        "über",
        "CartLines",
        "of_owner_store_many",
    ]
    .map(String::from)
    .to_vec();
    for table in &program.tables.tables {
        names.push(table.schema.name.clone());
        names.extend(table.schema.fields.iter().map(|field| field.name.clone()));
        names.extend(table.functions.iter().map(|function| function.name.clone()));
    }
    for name in names {
        assert_eq!(
            pw_host::tables::wit_name(&name),
            pw_core::wit::ident(&name),
            "{name:?}"
        );
    }
}
