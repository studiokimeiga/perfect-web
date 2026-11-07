//! **One render reads one state of the tables** (track `data-path`;
//! ADR-0386's "a render's snapshot", offered): the compiled reader `ById`,
//! given [`MemoryTables::read_host_fns_at`] a snapshot, answers the tables as
//! the snapshot holds them though a commit follows, beside the latest read,
//! which answers the commit; and a snapshot another adapter took reads none
//! of these tables.

mod common;

#[path = "support/assert_result.rs"]
mod assert_result;
use assert_result::*;

use common::{BY_ID, GET, READ_LISTS, program, todo_list};
use pw_host::engine::Val;
use pw_table_memory::{Error, MemoryTables};
use serde_json::json;

fn renamed(tables: &MemoryTables, name: &str) {
    tables
        .transaction(|tx| {
            tx.write(
                "pw:tables/lists#put",
                &[json!({"id": "list-a", "name": name, "slug": "list-a", "added": 10})],
            )
        })
        .assert_ok("a commit");
}

fn list_a(name: &str) -> Vec<Val> {
    vec![Val::Option(Some(Box::new(todo_list("list-a", name, 10))))]
}

#[test]
fn a_snapshot_reads_the_tables_as_it_found_them_though_a_commit_follows() {
    let program = program();
    let tables = MemoryTables::new(program.tables.clone()).assert_ok("the adapter");
    renamed(&tables, "Before");
    let reader = program.reader(BY_ID);
    let granted = reader.admitted(&reader.contract, READ_LISTS);
    let snapshot = tables.snapshot().assert_ok("a snapshot");
    let at = tables
        .read_host_fns_at(&snapshot, &reader.contract, &granted)
        .assert_ok("the gate admits the reader's get");
    assert_eq!(
        at.keys().collect::<Vec<_>>(),
        [GET],
        "exactly the reader's own import"
    );
    renamed(&tables, "After");
    let key = [Val::String("list-a".into())];
    assert_eq!(
        reader
            .call(&granted, &at, tables.host_tables(), &key)
            .assert_ok("the snapshot's read"),
        list_a("Before"),
        "the snapshot's state, though a commit followed"
    );
    let latest = tables
        .read_host_fns(&reader.contract, &granted)
        .assert_ok("the latest reads");
    assert_eq!(
        reader
            .call(&granted, &latest, tables.host_tables(), &key)
            .assert_ok("the latest read"),
        list_a("After"),
        "the latest read answers the commit"
    );
    // A snapshot taken after the commit reads it too.
    let after = tables.snapshot().assert_ok("a later snapshot");
    let at_after = tables
        .read_host_fns_at(&after, &reader.contract, &granted)
        .assert_ok("its reads");
    assert_eq!(
        reader
            .call(&granted, &at_after, tables.host_tables(), &key)
            .assert_ok("the later snapshot's read"),
        list_a("After")
    );
}

#[test]
fn a_snapshot_another_adapter_took_reads_none_of_these_tables() {
    let program = program();
    let tables = MemoryTables::new(program.tables.clone()).assert_ok("the adapter");
    let other = MemoryTables::new(program.tables.clone()).assert_ok("another adapter");
    let reader = program.reader(BY_ID);
    let granted = reader.admitted(&reader.contract, READ_LISTS);
    let theirs = other.snapshot().assert_ok("their snapshot");
    let refused = tables
        .read_host_fns_at(&theirs, &reader.contract, &granted)
        .err();
    assert!(
        matches!(&refused, Some(Error::Input(why)) if why.contains("another adapter")),
        "{refused:?}"
    );
    // The neighbour: its own snapshot is read.
    let mine = tables.snapshot().assert_ok("its snapshot");
    tables
        .read_host_fns_at(&mine, &reader.contract, &granted)
        .assert_ok("its own snapshot's reads");
}
