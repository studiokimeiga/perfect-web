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

// --- L2 (ADR-0395): PR #91's races and its platform test, for design A --------

/// The owned fixture's tables on PostgreSQL, opened at `isolation`, each
/// connection named `name` so `pg_stat_activity` finds it.
fn pg_at(
    program: &Program,
    scratch: &common::Scratch,
    isolation: pw_table_postgres::Isolation,
    name: &str,
) -> PgTables {
    let url = common::named(
        &common::with_setting(
            &scratch.url,
            "default_transaction_isolation",
            "read committed",
        ),
        name,
    );
    PgTables::open_with(
        &url,
        scratch.schema(),
        program.tables.clone(),
        pw_table_postgres::Options {
            isolation,
            ..pw_table_postgres::Options::default()
        },
    )
    .assert_ok("the adapter opens")
}

/// A note as another connection's SQL writes it, the adapter's way: each
/// string a field's UTF-8 bytes.
fn note_sql(author: &str, id: &str, text: &str) -> String {
    format!(
        "INSERT INTO \"Notes\" (author, id, \"to\", text, draft, added) VALUES \
         (convert_to('{author}', 'UTF8'), convert_to('{id}', 'UTF8'), convert_to('carol', 'UTF8'), \
         convert_to('{text}', 'UTF8'), convert_to('{text}', 'UTF8'), 0)"
    )
}

/// Each note's author, id and text, as the database holds them, in order.
fn stored(scratch: &common::Scratch) -> Vec<String> {
    scratch.texts(
        "SELECT convert_from(author, 'UTF8') || ' ' || convert_from(id, 'UTF8') || ' ' || \
         convert_from(text, 'UTF8') FROM \"Notes\" ORDER BY author, id",
    )
}

/// **Bob's put of his `n1`, while a transaction of another connection runs
/// `theirs` and is held open**: what his command's commit answers (the
/// tables it wrote), and whether it had to wait on theirs, which commits
/// once his put has ended or waited. His connections are named `name`.
fn put_beside(
    program: &Program,
    scratch: &common::Scratch,
    isolation: pw_table_postgres::Isolation,
    name: &str,
    theirs: &[String],
) -> (Result<Vec<String>, pw_table_postgres::Error>, bool) {
    let tables = pg_at(program, scratch, isolation, name);
    let mut other =
        postgres::Client::connect(&scratch.url, postgres::NoTls).assert_ok("a connection");
    other
        .batch_execute(&format!(
            "SET search_path TO \"{}\"; BEGIN; {}",
            scratch.schema,
            theirs.join("; ")
        ))
        .assert_ok("their transaction, not yet committed");
    let done = AtomicBool::new(false);
    let (answered, waited) = std::thread::scope(|scope| {
        let his = scope.spawn(|| {
            let answered = (|| {
                let mut command = tables.begin()?;
                let mut row = note_json(BOB, "n1", CAROL, 0);
                row["text"] = json!("his");
                command.write("pw:tables/notes#put", &[row])?;
                command
                    .commit()
                    .map(|committed| committed.written.into_keys().collect::<Vec<_>>())
            })();
            done.store(true, Ordering::SeqCst);
            answered
        });
        // His put ends, or waits on a lock theirs holds.
        let settled = common::until(|| {
            done.load(Ordering::SeqCst)
                || scratch
                    .texts(&format!(
                        "SELECT count(*)::text FROM pg_stat_activity \
                         WHERE application_name = '{name}' AND wait_event_type = 'Lock'"
                    ))
                    .first()
                    .is_some_and(|n| n != "0")
        });
        let waited = settled && !done.load(Ordering::SeqCst);
        other.batch_execute("COMMIT").assert_ok("their commit");
        let answered = his.join().map_err(|_| "his command panicked");
        (answered, waited)
    });
    (answered.assert_ok("his command ends"), waited)
}

/// **Two owners' rows of one id are two rows, whatever runs between them**
/// (PR #91's race scripts, rewritten for design A, ADR-0407). B's key was
/// global, so a put of `milk` met, waited on and could take another
/// reader's `milk`; A's key leads with its owner, so alice's uncommitted
/// `n1` is no row bob's put of his `n1` can meet. His put neither waits on
/// her transaction nor changes her row, and his commit says it wrote
/// `Notes`; at read committed (a deployment's connections' default) and at
/// serializable, the adapter's own.
#[test]
fn two_owners_rows_of_one_id_are_two_rows_whatever_runs_between_them() {
    let Some(url) = common::database() else {
        return;
    };
    let program = program();
    for (isolation, name) in [
        (
            pw_table_postgres::Isolation::ConnectionDefault,
            "owned_race_beside",
        ),
        (
            pw_table_postgres::Isolation::Serializable,
            "owned_race_beside_serializable",
        ),
    ] {
        let scratch = common::Scratch::new(&url, name);
        let (answered, waited) = put_beside(
            &program,
            &scratch,
            isolation,
            name,
            &[note_sql(ALICE, "n1", "hers")],
        );
        // Each assertion on a line of its own: the strict producer counts a
        // failure only at an assertion's own line.
        assert_eq!(answered, Ok(vec!["Notes".to_string()]), "{name}");
        assert!(!waited, "{name}: bob's put waited on alice's transaction");
        assert_eq!(stored(&scratch), ["alice n1 hers", "bob n1 his"], "{name}");
    }
}

/// **A put waiting on its owner's row is answered by what it wrote** (PR
/// #91's "replaced" race, for design A; master's `RETURNING old`, ADR-0407
/// decision 4). Another connection deletes bob's `n1` and writes alice's
/// `n1` where B's global key would have put it, uncommitted. Under A hers
/// is another row: bob's put waits on his own row's delete alone, then
/// writes his row anew and says it wrote `Notes`, and alice's row is
/// untouched. B's put met her row there instead: answered as a write of
/// hers, which changed nothing, at read committed, and refused at
/// serializable.
#[test]
fn a_put_waiting_on_its_owners_row_is_answered_by_what_it_wrote() {
    let Some(url) = common::database() else {
        return;
    };
    let program = program();
    let theirs = [
        "DELETE FROM \"Notes\" WHERE author = convert_to('bob', 'UTF8') \
         AND id = convert_to('n1', 'UTF8')"
            .to_string(),
        note_sql(ALICE, "n1", "hers"),
    ];
    for (isolation, name) in [
        (
            pw_table_postgres::Isolation::ConnectionDefault,
            "owned_race_replaced",
        ),
        (
            pw_table_postgres::Isolation::Serializable,
            "owned_race_replaced_serializable",
        ),
    ] {
        let scratch = common::Scratch::new(&url, name);
        pg(&program, &scratch)
            .transaction(|tx| {
                let mut row = note_json(BOB, "n1", CAROL, 0);
                row["text"] = json!("his before");
                tx.write("pw:tables/notes#insert", &[row])
            })
            .assert_ok("bob's note");
        let (answered, waited) = put_beside(&program, &scratch, isolation, name, &theirs);
        assert!(
            waited,
            "{name}: bob's put did not wait on his own row's delete"
        );
        assert_eq!(answered, Ok(vec!["Notes".to_string()]), "{name}");
        assert_eq!(stored(&scratch), ["alice n1 hers", "bob n1 his"], "{name}");
    }
}

/// **A reader's command is never the platform's, on PostgreSQL** (PR #91's
/// test of the same name, for design A; the memory adapter's test of the
/// same name): `remove`, made by a host as no one, is refused its owned
/// table in its command's transaction before it writes, whatever its
/// platform's principal says; made as bob, it removes his `n1` alone; made
/// as alice, hers.
#[test]
fn a_readers_command_is_never_the_platforms() {
    let Some(url) = common::database() else {
        return;
    };
    let scratch = common::Scratch::new(&url, "owned_never_platform");
    let program = program();
    let tables = pg(&program, &scratch);
    tables
        .transaction(|tx| {
            tx.write("pw:tables/notes#insert", &[note_json(ALICE, "n1", BOB, 1)])?;
            tx.write("pw:tables/notes#insert", &[note_json(BOB, "n1", ALICE, 2)])
        })
        .assert_ok("seed");
    let remove = program.component("remove");
    let prepared =
        pw_host::engine::Prepared::compile(&remove.bytes).assert_ok("the export compiles");
    let made = |caller: &Caller, principal: &str| -> Result<Vec<Val>, String> {
        let command = tables.begin().assert_ok("its turn");
        let answer = prepared
            .call_authorized_with_tables_as(
                &remove.contract,
                &remove.granted(),
                &pw_host::Limits::unbounded(),
                &program.platform(principal),
                &command,
                &program.predicates(),
                caller,
                &remove.export(),
                &[s("n1"), s(ALICE)],
                |_, _| Ok(true),
            )
            .map_err(|error| error.to_string())?;
        command.commit().assert_ok("its commit");
        Ok(answer)
    };
    let authors =
        || scratch.texts("SELECT convert_from(author, 'UTF8') FROM \"Notes\" ORDER BY author");
    let refused = made(&Caller::Nobody, ALICE).assert_err("made as no one");
    assert!(refused.contains("no one makes this call"), "{refused}");
    assert_eq!(authors(), [ALICE, BOB], "a command made as no one wrote");
    assert_eq!(made(&as_user(BOB), BOB), Ok(vec![]));
    assert_eq!(authors(), [ALICE], "bob's request removed alice's note");
    assert_eq!(made(&as_user(ALICE), ALICE), Ok(vec![]));
    assert_eq!(authors(), Vec::<String>::new());
}
