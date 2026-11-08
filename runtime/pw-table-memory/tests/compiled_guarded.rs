//! **A command a predicate guards reads and writes its tables through the
//! gate once its predicate holds, and runs nothing where it does not**
//! (track `data-path`; ADR-0386's "authorized commands using tables", left
//! open): `examples/tables/guarded.pw`'s `make_signed`, which `requires
//! SignedIn`, compiled and called through
//! `pw_host::engine::call_authorized_with_tables`, each refusal beside the
//! call that runs.

mod common;

#[path = "support/assert_result.rs"]
mod assert_result;
use assert_result::*;

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use common::*;
use pw_host::engine::{HostFn, Val};
use pw_table_memory::MemoryTables;
use serde_json::json;

const NODE: &[&str] = &["database.read<Lists>", "database.write<Lists>"];

const INSERT: &str = "pw:tables/lists#insert";

/// The command's table functions in a command of `tables`, each call of its
/// insert counted.
fn staged_host(
    program: &Program,
    staged: &pw_table_memory::Command,
) -> (pw_host::Granted, BTreeMap<String, HostFn>, Arc<AtomicUsize>) {
    let maker = program.reader(MAKE_SIGNED);
    let granted = maker.admitted(&maker.contract, &grants(NODE));
    let mut host = staged
        .host_fns(&maker.contract, &granted)
        .assert_ok("the gate admits the command's write");
    assert_eq!(
        host.keys().collect::<Vec<_>>(),
        [INSERT],
        "exactly its own table function"
    );
    let inserts = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&inserts);
    let insert = host.remove(INSERT).assert_ok("its insert");
    host.insert(
        INSERT.to_string(),
        Arc::new(move |args: &[Val]| {
            counted.fetch_add(1, Ordering::SeqCst);
            insert(args)
        }),
    );
    (granted, host, inserts)
}

/// `make_signed` called with its predicate answered by `holds`, and each
/// predicate it was asked.
fn guarded_call(
    program: &Program,
    tables: &MemoryTables,
    granted: &pw_host::Granted,
    host: &BTreeMap<String, HostFn>,
    holds: bool,
) -> (Result<Vec<Val>, String>, Vec<String>) {
    let maker = program.reader(MAKE_SIGNED);
    let mut asked = Vec::new();
    let answer = pw_host::engine::call_authorized_with_tables(
        &maker.bytes,
        &maker.contract,
        granted,
        &pw_host::Limits::unbounded(),
        host,
        tables.host_tables(),
        &maker.export(),
        &[list("signed", "signed", 1)],
        |predicate, _| {
            asked.push(predicate.to_string());
            Ok(holds)
        },
    );
    (answer, asked)
}

fn published(tables: &MemoryTables) -> serde_json::Value {
    tables
        .read("pw:tables/lists#get", &[json!("signed")])
        .assert_ok("the published tables read")
}

/// **Its predicate holds: it writes through the gate**, and its commit
/// publishes the row.
#[test]
fn a_guarded_command_writes_through_the_gate_once_its_predicate_holds() {
    let program = guarded();
    let tables = MemoryTables::new(program.tables.clone()).assert_ok("the adapter");
    let staged = tables.begin().assert_ok("its turn");
    let (granted, host, inserts) = staged_host(&program, &staged);
    let (answer, asked) = guarded_call(&program, &tables, &granted, &host, true);
    assert_eq!(asked, ["SignedIn"], "its one predicate, asked first");
    let answer = answer.assert_ok("the guarded command runs");
    assert_eq!(answer, [Val::Result(Ok(None))], "its insert's answer");
    assert_eq!(inserts.load(Ordering::SeqCst), 1, "one insert, staged");
    assert_eq!(
        published(&tables),
        json!(null),
        "invisible until it commits"
    );
    staged.commit().assert_ok("its commit");
    assert_eq!(
        published(&tables)["id"],
        json!("signed"),
        "its row, published"
    );
}

/// **Its predicate refuses: nothing runs**, so no write is staged and no
/// row is published, though its command would commit.
#[test]
fn a_refused_predicate_runs_nothing_and_writes_nothing() {
    let program = guarded();
    let tables = MemoryTables::new(program.tables.clone()).assert_ok("the adapter");
    let staged = tables.begin().assert_ok("its turn");
    let (granted, host, inserts) = staged_host(&program, &staged);
    let (answer, asked) = guarded_call(&program, &tables, &granted, &host, false);
    assert_eq!(asked, ["SignedIn"]);
    let refused = answer.assert_err("a refused predicate runs nothing");
    assert!(
        refused.contains("`SignedIn` denied the invocation"),
        "{refused}"
    );
    assert_eq!(inserts.load(Ordering::SeqCst), 0, "no write was staged");
    assert_eq!(
        staged.written().assert_ok("what it wrote"),
        Default::default(),
        "its command wrote nothing"
    );
    staged.commit().assert_ok("an empty commit");
    assert_eq!(published(&tables), json!(null), "nothing published");
}

/// **Unevaluated, it is refused; and without the gate, its table function
/// links nowhere.** `call_with_tables` refuses a command whose predicates no
/// one answered, and `call_authorized_within`, which answers them, links no
/// table import, so the guarded call is the one way it runs.
#[test]
fn a_guarded_tables_command_runs_only_answered_and_through_the_gate() {
    let program = guarded();
    let maker = program.reader(MAKE_SIGNED);
    let tables = MemoryTables::new(program.tables.clone()).assert_ok("the adapter");
    let staged = tables.begin().assert_ok("its turn");
    let (granted, host, inserts) = staged_host(&program, &staged);
    let unanswered = maker
        .call(
            &granted,
            &host,
            tables.host_tables(),
            &[list("signed", "signed", 1)],
        )
        .assert_err("its predicates unanswered");
    assert!(
        unanswered.contains("authorization precondition(s) that have not been evaluated"),
        "{unanswered}"
    );
    let ungated = pw_host::engine::call_authorized_within(
        &maker.bytes,
        &maker.contract,
        &granted,
        &pw_host::Limits::unbounded(),
        &host,
        &maker.export(),
        &[list("signed", "signed", 1)],
        |_, _| Ok(true),
    )
    .assert_err("no table import links without the gate");
    assert!(
        ungated.contains("links only through its program's tables.json"),
        "{ungated}"
    );
    assert_eq!(inserts.load(Ordering::SeqCst), 0, "neither ran its write");
    // The neighbour: answered, and through the gate, it runs.
    let (answer, _) = guarded_call(&program, &tables, &granted, &host, true);
    answer.assert_ok("the guarded call runs");
    assert_eq!(inserts.load(Ordering::SeqCst), 1);
}
