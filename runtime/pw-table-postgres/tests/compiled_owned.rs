//! The same compiled owned fixture as memory, run on real PostgreSQL.
#![allow(dead_code)]
include!("../../pw-table-memory/tests/support/owned.rs");
mod common;
use pw_table_postgres::PgTables;
use std::sync::atomic::AtomicBool;

fn pg(program: &Program, scratch: &common::Scratch) -> Arc<PgTables> {
    Arc::new(
        PgTables::open(&scratch.url, scratch.schema(), program.tables.clone())
            .assert_ok("PostgreSQL tables"),
    )
}
fn s(value: &str) -> Val {
    Val::String(value.into())
}
fn call(
    program: &Program,
    id: &str,
    user: &str,
    tables: &dyn pw_host::engine::CallTables,
    args: &[Val],
) -> Result<Vec<Val>, String> {
    authorized(
        program,
        id,
        user,
        tables,
        &pw_host::Limits::unbounded(),
        args,
        false,
        &AtomicUsize::new(0),
    )
}

#[test]
fn a_postgres_predicate_reads_its_commands_staged_rows_and_no_refused_body_runs() {
    let Some(url) = common::database() else {
        return;
    };
    let scratch = common::Scratch::new(&url, "owned_staged");
    let program = program();
    let tables = pg(&program, &scratch);
    let mut command = tables.begin().assert_ok("command");
    command
        .write("pw:tables/notes#insert", &[note_json(ALICE, "n1", BOB, 0)])
        .assert_ok("staged note");
    assert_eq!(
        tables
            .read("pw:tables/notes#get", &[json!(ALICE), json!("n1")])
            .assert_ok("outside"),
        serde_json::Value::Null
    );
    assert_eq!(
        call(
            &program,
            "edit",
            ALICE,
            &command,
            &[s("n1"), s(BOB), s("EDITED")]
        ),
        Ok(vec![])
    );
    command.commit().assert_ok("commit");
    assert_eq!(
        tables
            .read("pw:tables/notes#get", &[json!(ALICE), json!("n1")])
            .assert_ok("published")["text"],
        "EDITED"
    );
    let command = tables.begin().assert_ok("refused command");
    let watched = Watched::new(&command);
    let denied = call(
        &program,
        "edit",
        CAROL,
        &watched,
        &[s("n1"), s(BOB), s("DEFACED")],
    );
    assert!(
        denied
            .as_ref()
            .is_err_and(|why| why.contains("`Wrote` denied")),
        "{denied:?}"
    );
    assert_eq!(watched.body.load(Ordering::SeqCst), 0);
    assert_eq!(watched.asked.load(Ordering::SeqCst), 1);
    command.commit().assert_ok("nothing to commit");
}

#[test]
fn a_postgres_query_keeps_one_snapshot_for_its_predicate_and_body() {
    let Some(url) = common::database() else {
        return;
    };
    let scratch = common::Scratch::new(&url, "owned_snapshot");
    let program = program();
    let tables = pg(&program, &scratch);
    let writer = pg(&program, &scratch);
    writer
        .transaction(|tx| tx.write("pw:tables/notes#insert", &[note_json(ALICE, "n1", BOB, 0)]))
        .assert_ok("seed");
    let snapshot = tables.snapshot().assert_ok("snapshot");
    let mut watched = Watched::new(&snapshot);
    let removed = Arc::new(AtomicBool::new(false));
    let signal = Arc::clone(&removed);
    watched.after_read = Some(Arc::new(move || {
        if !signal.swap(true, Ordering::SeqCst) {
            writer
                .transaction(|tx| tx.write("pw:tables/notes#delete", &[json!(ALICE), json!("n1")]))
                .assert_ok("concurrent removal");
        }
    }));
    let args = [s(ALICE), s("n1")];
    let answer = call(&program, "Shown", ALICE, &watched, &args).assert_ok("one snapshot");
    assert!(
        matches!(answer.as_slice(), [Val::Option(Some(_))]),
        "{answer:?}"
    );
    assert!(removed.load(Ordering::SeqCst));
    assert_eq!(
        tables
            .read("pw:tables/notes#get", &[json!(ALICE), json!("n1")])
            .assert_ok("removal committed"),
        serde_json::Value::Null
    );
    assert_eq!(
        call(&program, "Shown", ALICE, &snapshot, &args).assert_ok("old snapshot remains"),
        answer
    );
    let fresh = tables.snapshot().assert_ok("fresh snapshot");
    let denied = call(&program, "Shown", ALICE, &fresh, &args);
    assert!(
        denied
            .as_ref()
            .is_err_and(|why| why.contains("`Wrote` denied")),
        "{denied:?}"
    );
}

#[test]
fn a_postgres_command_conflicting_after_its_predicate_is_refused_by_serializability() {
    let Some(url) = common::database() else {
        return;
    };
    let scratch = common::Scratch::new(&url, "owned_conflict");
    let program = program();
    let tables = pg(&program, &scratch);
    let writer = pg(&program, &scratch);
    writer
        .transaction(|tx| tx.write("pw:tables/notes#insert", &[note_json(ALICE, "n1", BOB, 0)]))
        .assert_ok("seed");
    let command = tables.begin().assert_ok("command");
    let mut watched = Watched::new(&command);
    let changed = Arc::new(AtomicBool::new(false));
    let signal = Arc::clone(&changed);
    watched.after_read = Some(Arc::new(move || {
        if !signal.swap(true, Ordering::SeqCst) {
            let mut row = note_json(ALICE, "n1", BOB, 0);
            row["text"] = json!("CONCURRENT");
            writer
                .transaction(|tx| tx.write("pw:tables/notes#put", std::slice::from_ref(&row)))
                .assert_ok("conflicting write committed");
        }
    }));
    let answer = call(
        &program,
        "edit",
        ALICE,
        &watched,
        &[s("n1"), s(BOB), s("LOST_WRITE")],
    );
    assert!(changed.load(Ordering::SeqCst));
    assert!(
        answer
            .as_ref()
            .is_err_and(|why| why.contains("Serialization")),
        "{answer:?}"
    );
    drop(command);
    assert_eq!(
        tables
            .read("pw:tables/notes#get", &[json!(ALICE), json!("n1")])
            .assert_ok("winner")["text"],
        "CONCURRENT"
    );
    let command = tables.begin().assert_ok("new command");
    assert_eq!(
        call(
            &program,
            "edit",
            ALICE,
            &command,
            &[s("n1"), s(BOB), s("AFTER_CONFLICT")]
        ),
        Ok(vec![])
    );
    command.commit().assert_ok("neighbor commits");
}

#[test]
fn postgres_predicate_callbacks_end_with_the_command_and_refuse_a_nonpredicate_contract() {
    use pw_host::engine::CallTables;
    let Some(url) = common::database() else {
        return;
    };
    let scratch = common::Scratch::new(&url, "owned_ended");
    let program = program();
    let tables = pg(&program, &scratch);
    let command = tables.begin().assert_ok("command");
    let predicate = program.component("Wrote");
    let reads = command
        .predicate_reads(&predicate.contract, &predicate.granted())
        .assert_ok("predicate reads");
    let query = program.component("One");
    assert!(
        command
            .predicate_reads(&query.contract, &query.granted())
            .is_err()
    );
    let snapshot = tables.snapshot().assert_ok("snapshot");
    assert!(
        snapshot
            .predicate_reads(&query.contract, &query.granted())
            .is_err()
    );
    drop(command);
    let read = reads
        .get("pw:tables/notes#get")
        .expect("predicate read import");
    assert!(
        read(&[s(ALICE), s("n1")]).is_err(),
        "an ended command cannot be read"
    );
}

#[test]
fn postgres_operation_costs_keep_transaction_overhead_in_the_total() {
    let Some(url) = common::database() else {
        return;
    };
    let scratch = common::Scratch::new(&url, "owned_cost");
    let program = program();
    let tables = pg(&program, &scratch);
    let baseline = (tables.statements(), tables.operation_statements());
    let snapshot = tables.snapshot().assert_ok("snapshot");
    snapshot
        .read("pw:tables/notes#get", &[json!(ALICE), json!("n1")])
        .assert_ok("snapshot read");
    drop(snapshot);
    assert_eq!(
        tables.statements() - baseline.0,
        3,
        "BEGIN, read, COMMIT all counted"
    );
    assert_eq!(
        tables.operation_statements() - baseline.1,
        1,
        "one read operation"
    );
    let baseline = (tables.statements(), tables.operation_statements());
    let mut command = tables.begin().assert_ok("command");
    command
        .write("pw:tables/notes#insert", &[note_json(ALICE, "n1", BOB, 0)])
        .assert_ok("cold first write");
    assert_eq!(
        tables.statements() - baseline.0,
        2,
        "BEGIN and write counted"
    );
    assert_eq!(
        tables.operation_statements() - baseline.1,
        1,
        "one write operation"
    );
    drop(command);
    assert_eq!(
        tables.statements() - baseline.0,
        3,
        "rollback remains in total"
    );
    assert_eq!(
        tables.operation_statements() - baseline.1,
        1,
        "rollback adds no data operation"
    );
}
