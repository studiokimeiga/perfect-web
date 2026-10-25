//! **`examples/tables/app.pw`, compiled and run through the table gate on
//! the memory adapter** (ADR-0386; the acceptance d2-integrate left).
//!
//! Its five queries read the published tables through every read primitive
//! (`all`, a unique index, a prefix, an index, a prefix's set form), each
//! linked to exactly its own imports; its four commands write through every
//! write primitive (`insert`, `put`, `delete`, `delete_of_list`), each in a
//! command of its own, committed where it answers and undone where it traps
//! or answers a declared error, as the platform runs one (ADR-0208,
//! ADR-0157). The writers' fixture (`writer.pw`) shows a command reading its
//! own writes before it commits, and one that stops after a write.

mod common;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use common::*;
use pw_host::engine::{HostFn, Val};
use pw_table_memory::MemoryTables;
use serde_json::json;

/// What the app's node grants: each table's reads and writes, and the outbox
/// its commands hand their invalidations to.
const NODE: &[&str] = &[
    "database.read<Lists>",
    "database.read<Items>",
    "database.write<Lists>",
    "database.write<Items>",
    "outbox.write",
];

/// The program's tables, holding two lists and four items, written through
/// the adapter's own transaction.
fn seeded(program: &Program) -> MemoryTables {
    let tables = MemoryTables::new(program.tables.clone()).expect("the adapter");
    tables
        .transaction(|tx| {
            tx.write(
                "pw:tables/lists#insert",
                &[list_json("groceries", "food", 20)],
            )?;
            tx.write("pw:tables/lists#insert", &[list_json("chores", "home", 10)])?;
            for (list, id, done, added) in [
                ("groceries", "milk", false, 3),
                ("groceries", "eggs", true, 1),
                ("chores", "sweep", false, 2),
                ("chores", "dust", false, 5),
            ] {
                tx.write(
                    "pw:tables/items#insert",
                    &[item_json(list, id, done, added)],
                )?;
            }
            Ok(())
        })
        .expect("seeded through the adapter's own transaction");
    tables
}

/// A query, answered from the published tables, linked to exactly its own
/// imports.
fn query(program: &Program, tables: &MemoryTables, id: &str, args: &[Val]) -> Vec<Val> {
    let reader = program.reader(id);
    let granted = reader.admitted(&reader.contract, &grants(NODE));
    let host = tables
        .read_host_fns(&reader.contract, &granted)
        .expect("the gate admits the query's reads");
    assert_eq!(
        host.keys().collect::<Vec<_>>(),
        reader.imports.iter().collect::<Vec<_>>(),
        "{id}: exactly its own imports"
    );
    reader
        .call(&granted, &host, tables.host_tables(), args)
        .unwrap_or_else(|why| panic!("{id}: {why}"))
}

/// What a command answered, and what it handed the platform's imports.
struct Ran {
    answer: Result<Vec<Val>, String>,
    platform: Vec<(String, Vec<Val>)>,
}

/// **A command, as the platform runs one**: in a command of its own, its
/// table functions that command's, its platform imports recorded; committed
/// where it answers, undone where it traps or answers a declared error
/// (ADR-0208, ADR-0157). `wrap` may wrap its host functions.
fn command_with(
    program: &Program,
    tables: &MemoryTables,
    id: &str,
    args: &[Val],
    wrap: impl FnOnce(&mut BTreeMap<String, HostFn>),
) -> Ran {
    let reader = program.reader(id);
    let granted = reader.admitted(&reader.contract, &grants(NODE));
    let staged = tables.begin().expect("its turn");
    let mut host = staged
        .host_fns(&reader.contract, &granted)
        .expect("the gate admits the command's reads and writes");
    let (platform, calls) = platform(reader);
    host.extend(platform);
    wrap(&mut host);
    let answer = reader.call(&granted, &host, tables.host_tables(), args);
    let declared_error = matches!(answer.as_deref(), Ok([Val::Result(Err(_))]));
    if answer.is_ok() && !declared_error {
        staged.commit().expect("its commit");
    }
    let platform = calls.lock().unwrap().clone();
    Ran { answer, platform }
}

fn command(program: &Program, tables: &MemoryTables, id: &str, args: &[Val]) -> Ran {
    command_with(program, tables, id, args, |_| {})
}

fn text(value: &str) -> Val {
    Val::String(value.into())
}

fn some(value: Val) -> Val {
    Val::Option(Some(Box::new(value)))
}

fn every_list(program: &Program, tables: &MemoryTables) -> Vec<Val> {
    query(program, tables, EVERY_LIST, &[])
}

fn items_of(program: &Program, tables: &MemoryTables, list: &str) -> Vec<Val> {
    query(program, tables, ITEMS_OF, &[text(list)])
}

fn open_items(program: &Program, tables: &MemoryTables) -> Vec<Val> {
    query(program, tables, OPEN_ITEMS, &[])
}

/// Every read primitive, through the app's compiled queries: `all` in the
/// table's order, a unique index's one row or none, a prefix's rows by
/// `added` then the key, an index's rows likewise, and a prefix's set form
/// grouped by the lists asked.
#[test]
fn every_query_reads_its_tables_through_the_gate_in_d1s_order() {
    let program = app();
    let tables = seeded(&program);
    assert_eq!(
        every_list(&program, &tables),
        [Val::List(vec![
            list("chores", "home", 10),
            list("groceries", "food", 20)
        ])]
    );
    assert_eq!(
        query(&program, &tables, LIST_AT, &[text("food")]),
        [some(list("groceries", "food", 20))]
    );
    assert_eq!(
        query(&program, &tables, LIST_AT, &[text("nowhere")]),
        [Val::Option(None)]
    );
    assert_eq!(
        items_of(&program, &tables, "groceries"),
        [Val::List(vec![
            item("groceries", "eggs", true, 1),
            item("groceries", "milk", false, 3)
        ])]
    );
    assert_eq!(items_of(&program, &tables, "none"), [Val::List(vec![])]);
    assert_eq!(
        open_items(&program, &tables),
        [Val::List(vec![
            item("chores", "sweep", false, 2),
            item("groceries", "milk", false, 3),
            item("chores", "dust", false, 5),
        ])]
    );
    assert_eq!(
        query(&program, &tables, EVERY_ITEM, &[]),
        [Val::List(vec![
            item("chores", "sweep", false, 2),
            item("chores", "dust", false, 5),
            item("groceries", "eggs", true, 1),
            item("groceries", "milk", false, 3),
        ])]
    );
}

/// `make_list` inserts, its invalidations computed by the command and handed
/// to the platform first (ADR-0209); a second list of its key, or of its
/// unique slug, is refused with the row that holds it as `make_list`'s
/// declared error, and nothing is written.
#[test]
fn make_list_inserts_or_answers_the_row_that_holds_its_key_or_slug() {
    let program = app();
    let tables = seeded(&program);
    let made = command(&program, &tables, MAKE_LIST, &[list("todo", "work", 30)]);
    assert_eq!(made.answer, Ok(vec![Val::Result(Ok(None))]));
    assert_eq!(
        made.platform,
        [
            (
                "pw:host/invalidations#tables-app-every-list".to_string(),
                vec![]
            ),
            (
                "pw:host/invalidations#tables-app-list-at".to_string(),
                vec![text("work")]
            ),
            (
                "pw:host/invalidations#tables-app-every-item".to_string(),
                vec![]
            ),
        ],
        "the entries its write invalidates, as the command declares them"
    );
    let three = every_list(&program, &tables);
    assert_eq!(
        three,
        [Val::List(vec![
            list("chores", "home", 10),
            list("groceries", "food", 20),
            list("todo", "work", 30)
        ])]
    );
    let held = Ok(vec![Val::Result(Err(Some(Box::new(list(
        "todo", "work", 30,
    )))))]);
    // Its key held, by another slug; its slug held, by another key.
    for again in [list("todo", "other", 40), list("next", "work", 50)] {
        assert_eq!(command(&program, &tables, MAKE_LIST, &[again]).answer, held);
        assert_eq!(every_list(&program, &tables), three, "nothing written");
    }
    assert_eq!(
        query(&program, &tables, LIST_AT, &[text("other")]),
        [Val::Option(None)]
    );
}

/// `keep_item` puts a row whole, and the index it reads by is kept on the
/// write: an item done leaves the open items, a new one joins them in its
/// place in the order.
#[test]
fn keep_item_puts_whole_rows_and_keeps_the_index() {
    let program = app();
    let tables = seeded(&program);
    let kept = command(
        &program,
        &tables,
        KEEP_ITEM,
        &[item("groceries", "milk", true, 3)],
    );
    assert_eq!(kept.answer, Ok(vec![]));
    assert_eq!(
        open_items(&program, &tables),
        [Val::List(vec![
            item("chores", "sweep", false, 2),
            item("chores", "dust", false, 5),
        ])]
    );
    let added = command(
        &program,
        &tables,
        KEEP_ITEM,
        &[item("groceries", "bread", false, 4)],
    );
    assert_eq!(added.answer, Ok(vec![]));
    assert_eq!(
        open_items(&program, &tables),
        [Val::List(vec![
            item("chores", "sweep", false, 2),
            item("groceries", "bread", false, 4),
            item("chores", "dust", false, 5),
        ])]
    );
    assert_eq!(
        items_of(&program, &tables, "groceries"),
        [Val::List(vec![
            item("groceries", "eggs", true, 1),
            item("groceries", "milk", true, 3),
            item("groceries", "bread", false, 4),
        ])]
    );
}

/// `drop_item` deletes one row by its key, a key no row has deleting
/// nothing; `clear_list` deletes every row of a prefix of the key, and no
/// other list's.
#[test]
fn drop_item_and_clear_list_delete_by_key_and_by_prefix() {
    let program = app();
    let tables = seeded(&program);
    for missing in ["dust", "missing"] {
        let dropped = command(
            &program,
            &tables,
            DROP_ITEM,
            &[text("chores"), text(missing)],
        );
        assert_eq!(dropped.answer, Ok(vec![]));
    }
    assert_eq!(
        items_of(&program, &tables, "chores"),
        [Val::List(vec![item("chores", "sweep", false, 2)])]
    );
    let cleared = command(&program, &tables, CLEAR_LIST, &[text("groceries")]);
    assert_eq!(cleared.answer, Ok(vec![]));
    assert_eq!(
        items_of(&program, &tables, "groceries"),
        [Val::List(vec![])]
    );
    assert_eq!(
        query(&program, &tables, EVERY_ITEM, &[]),
        [Val::List(vec![item("chores", "sweep", false, 2)])]
    );
    assert_eq!(
        open_items(&program, &tables),
        [Val::List(vec![item("chores", "sweep", false, 2)])]
    );
}

/// **A command's writes are invisible to every other read until it
/// commits**: a query, and the adapter's own read, run while the command is
/// open, see none of what it wrote; after its commit, both see all of it.
#[test]
fn a_commands_writes_are_invisible_until_it_commits() {
    let program = app();
    let tables = seeded(&program);
    let keeper = program.reader(KEEP_ITEM);
    let granted = keeper.admitted(&keeper.contract, &grants(NODE));
    let staged = tables.begin().expect("its turn");
    let mut host = staged.host_fns(&keeper.contract, &granted).unwrap();
    host.extend(platform(keeper).0);
    let before = open_items(&program, &tables);
    for row in [
        item("groceries", "bread", false, 4),
        item("chores", "sweep", true, 2),
    ] {
        assert_eq!(
            keeper.call(&granted, &host, tables.host_tables(), &[row]),
            Ok(vec![])
        );
    }
    assert_eq!(
        open_items(&program, &tables),
        before,
        "a query, before the commit"
    );
    assert_eq!(
        tables.read("pw:tables/items#get", &[json!("groceries"), json!("bread")]),
        Ok(serde_json::Value::Null),
        "the adapter's own read, before the commit"
    );
    staged.commit().expect("its commit");
    assert_eq!(
        open_items(&program, &tables),
        [Val::List(vec![
            item("groceries", "milk", false, 3),
            item("groceries", "bread", false, 4),
            item("chores", "dust", false, 5),
        ])]
    );
}

/// **A command not committed publishes nothing**, however many writes it
/// made: dropped, every write is undone, the index's entries with them.
#[test]
fn a_command_not_committed_publishes_none_of_its_writes() {
    let program = app();
    let tables = seeded(&program);
    let staged = tables.begin().expect("its turn");
    for (id, args) in [
        (KEEP_ITEM, vec![item("groceries", "milk", true, 3)]),
        (CLEAR_LIST, vec![text("chores")]),
        (MAKE_LIST, vec![list("todo", "work", 30)]),
    ] {
        let reader = program.reader(id);
        let granted = reader.admitted(&reader.contract, &grants(NODE));
        let mut host = staged.host_fns(&reader.contract, &granted).unwrap();
        host.extend(platform(reader).0);
        reader
            .call(&granted, &host, tables.host_tables(), &args)
            .unwrap_or_else(|why| panic!("{id}: {why}"));
    }
    drop(staged);
    let after = seeded(&program);
    for id in [EVERY_LIST, OPEN_ITEMS, EVERY_ITEM] {
        assert_eq!(
            query(&program, &tables, id, &[]),
            query(&program, &after, id, &[]),
            "{id}: as seeded"
        );
    }
    // And the next command reads the tables as they were.
    let made = command(&program, &tables, MAKE_LIST, &[list("todo", "work", 30)]);
    assert_eq!(made.answer, Ok(vec![Val::Result(Ok(None))]));
}

/// **A command reads its own writes before it commits** (`writer.pw`), as a
/// PostgreSQL transaction's queries see its own earlier updates: its `get`
/// answers the row it inserted, while a read outside it, made at that
/// moment, answers none.
#[test]
fn a_command_reads_its_own_writes_before_it_commits() {
    let program = writer();
    let tables = Arc::new(MemoryTables::new(program.tables.clone()).unwrap());
    let outside: Arc<Mutex<Vec<serde_json::Value>>> = Arc::default();
    let seen = Arc::clone(&outside);
    let reading = Arc::clone(&tables);
    let ran = command_with(
        &program,
        &tables,
        INSERT_THEN_GET,
        &[list("todo", "work", 30)],
        move |host| {
            let get = host.remove("pw:tables/lists#get").expect("its get");
            let wrapped: HostFn = Arc::new(move |args| {
                seen.lock().unwrap().push(
                    reading
                        .read("pw:tables/lists#get", &[json!("todo")])
                        .unwrap(),
                );
                get(args)
            });
            host.insert("pw:tables/lists#get".into(), wrapped);
        },
    );
    assert_eq!(
        ran.answer,
        Ok(vec![Val::Result(Ok(Some(Box::new(some(list(
            "todo", "work", 30
        ))))))])
    );
    assert_eq!(
        *outside.lock().unwrap(),
        [serde_json::Value::Null],
        "outside the command, at its own read, nothing"
    );
    assert_eq!(
        tables.read("pw:tables/lists#get", &[json!("todo")]),
        Ok(list_json("todo", "work", 30)),
        "after its commit, all of it"
    );
}

/// A command's index read sees its own put through the index the put kept.
#[test]
fn a_commands_index_read_sees_its_own_puts() {
    let program = writer();
    let tables = MemoryTables::new(program.tables.clone()).unwrap();
    tables
        .transaction(|tx| {
            tx.write(
                "pw:tables/lists#insert",
                &[list_json("groceries", "food", 1)],
            )?;
            tx.write(
                "pw:tables/items#insert",
                &[item_json("groceries", "milk", false, 3)],
            )
        })
        .unwrap();
    let done = command(
        &program,
        &tables,
        PUT_THEN_OPEN,
        &[item("groceries", "milk", true, 3)],
    );
    assert_eq!(done.answer, Ok(vec![Val::List(vec![])]));
    let opened = command(
        &program,
        &tables,
        PUT_THEN_OPEN,
        &[item("groceries", "bread", false, 2)],
    );
    assert_eq!(
        opened.answer,
        Ok(vec![Val::List(vec![item("groceries", "bread", false, 2)])])
    );
}

/// **A command that stops after a write publishes nothing**: its insert
/// staged, its division by zero traps, and the platform does not commit it;
/// the neighbour, dividing by four, commits its insert.
#[test]
fn a_command_that_stops_after_a_write_publishes_nothing() {
    let program = writer();
    let tables = MemoryTables::new(program.tables.clone()).unwrap();
    let stopped = command(
        &program,
        &tables,
        INSERT_THEN_DIVIDE,
        &[list("todo", "work", 30), Val::S64(0)],
    );
    let why = stopped.answer.expect_err("a division by zero stops it");
    assert!(why.contains("division by zero"), "{why}");
    assert_eq!(
        tables.read("pw:tables/lists#get", &[json!("todo")]),
        Ok(serde_json::Value::Null)
    );
    let divided = command(
        &program,
        &tables,
        INSERT_THEN_DIVIDE,
        &[list("todo", "work", 30), Val::S64(4)],
    );
    assert_eq!(
        divided.answer,
        Ok(vec![Val::Result(Ok(Some(Box::new(Val::S64(25)))))])
    );
    assert_eq!(
        tables.read("pw:tables/lists#get", &[json!("todo")]),
        Ok(list_json("todo", "work", 30))
    );
}
