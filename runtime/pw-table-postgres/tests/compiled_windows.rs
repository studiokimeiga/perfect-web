//! Actual compiled window WIT/authority boundary on real PostgreSQL.
#![allow(dead_code)]
include!("../../pw-table-memory/tests/support/owned.rs");
include!("../../pw-table-memory/tests/support/windows.rs");
mod common;
use pw_host::engine::CallTables;
use pw_table_postgres::PgTables;

fn q(
    program: &Program,
    tables: &PgTables,
    id: &str,
    caller: &Caller,
    args: &[Val],
) -> Result<Vec<Val>, String> {
    q_counted(program, tables, id, caller, args, &Arc::default())
}

fn q_counted(
    program: &Program,
    tables: &PgTables,
    id: &str,
    caller: &Caller,
    args: &[Val],
    calls: &Arc<AtomicUsize>,
) -> Result<Vec<Val>, String> {
    q_limited(
        program,
        tables,
        id,
        caller,
        args,
        calls,
        &pw_host::Limits::unbounded(),
    )
}

fn q_limited(
    program: &Program,
    tables: &PgTables,
    id: &str,
    caller: &Caller,
    args: &[Val],
    calls: &Arc<AtomicUsize>,
    limits: &pw_host::Limits,
) -> Result<Vec<Val>, String> {
    let component = program.component(id);
    let granted = component.granted();
    let snapshot = tables.snapshot().map_err(|e| format!("{e:?}"))?;
    let mut host = snapshot
        .functions(&component.contract, &granted)?
        .into_iter()
        .map(|(key, call)| (key, counted(call, calls)))
        .collect::<BTreeMap<_, _>>();
    let user = match caller {
        Caller::User(user) => user.as_str(),
        _ => "",
    };
    host.extend(platform(component, user));
    pw_host::engine::call_with_tables_as(
        &component.bytes,
        &component.contract,
        &granted,
        limits,
        &host,
        &program.manifest,
        caller,
        &component.export(),
        args,
    )
}

#[test]
fn windows_return_bounded_ordered_rows_through_the_actual_owner_and_sharer_wit() {
    let program = window_program();
    let Some(url) = common::database() else {
        return;
    };
    let scratch = common::Scratch::new(&url, "compiled_windows");
    let tables =
        PgTables::open(&url, scratch.schema(), program.tables.clone()).assert_ok("PostgreSQL");
    tables
        .transaction(|tx| {
            for (author, id, to, stamp) in [
                (ALICE, "n1", BOB, 1),
                (ALICE, "n2", BOB, 1),
                (ALICE, "n3", CAROL, 5),
                (CAROL, "n4", BOB, 1),
            ] {
                tx.write(
                    "pw:tables/notes#insert",
                    &[note_json(author, id, to, stamp)],
                )?;
            }
            Ok(())
        })
        .assert_ok("seed");
    assert_eq!(
        q(
            &program,
            &tables,
            "FirstMine",
            &as_user(ALICE),
            &[handle(ALICE), Val::S64(2)]
        ),
        Ok(vec![Val::List(vec![
            note(ALICE, "n1", BOB, 1),
            note(ALICE, "n2", BOB, 1)
        ])])
    );
    assert_eq!(
        q(
            &program,
            &tables,
            "LastMine",
            &as_user(ALICE),
            &[handle(ALICE), Val::S64(2)]
        ),
        Ok(vec![Val::List(vec![
            note(ALICE, "n3", CAROL, 5),
            note(ALICE, "n2", BOB, 1)
        ])])
    );
    assert_eq!(
        q(
            &program,
            &tables,
            "FirstRecipients",
            &as_user(ALICE),
            &[
                handle(ALICE),
                Val::List(vec![
                    Val::String(CAROL.into()),
                    Val::String(BOB.into()),
                    Val::String(BOB.into())
                ]),
                Val::S64(2)
            ]
        ),
        Ok(vec![Val::List(vec![
            note(ALICE, "n1", BOB, 1),
            note(ALICE, "n2", BOB, 1),
            note(ALICE, "n3", CAROL, 5)
        ])])
    );
    assert_eq!(
        q(
            &program,
            &tables,
            "LastTimes",
            &as_user(BOB),
            &[
                handle(BOB),
                Val::List(vec![Val::S64(99), Val::S64(1), Val::S64(1)]),
                Val::S64(2)
            ]
        ),
        Ok(vec![Val::List(vec![
            seen(CAROL, "n4", BOB, 1),
            seen(ALICE, "n2", BOB, 1)
        ])])
    );
    for n in [0, -1] {
        assert_eq!(
            q(
                &program,
                &tables,
                "LastMine",
                &as_user(ALICE),
                &[handle(ALICE), Val::S64(n)]
            ),
            Ok(vec![Val::List(vec![])])
        );
    }
    let reads = Arc::new(AtomicUsize::new(0));
    for caller in [Caller::Nobody, as_user(BOB)] {
        let denied = q_counted(
            &program,
            &tables,
            "LastMine",
            &caller,
            &[handle(ALICE), Val::S64(2)],
            &reads,
        );
        assert!(denied.is_err());
        assert_eq!(
            reads.load(Ordering::SeqCst),
            0,
            "a forged reader reaches no adapter callback"
        );
    }
}

#[test]
fn a_window_predicate_sees_staged_rows_and_a_held_query_keeps_one_snapshot() {
    let Some(url) = common::database() else {
        return;
    };
    let scratch = common::Scratch::new(&url, "window_states");
    let program = window_program_kind(true);
    let tables =
        PgTables::open(&url, scratch.schema(), program.tables.clone()).assert_ok("PostgreSQL");
    tables
        .transaction(|tx| {
            tx.write("pw:tables/notes#insert", &[note_json(ALICE, "n1", BOB, 1)])?;
            tx.write(
                "pw:tables/notes#insert",
                &[note_json(ALICE, "n3", CAROL, 5)],
            )
        })
        .assert_ok("seed");
    let mut command = tables.begin().assert_ok("command");
    command
        .write("pw:tables/notes#insert", &[note_json(ALICE, "n4", BOB, 9)])
        .assert_ok("stage newest");
    let s = |v: &str| Val::String(v.into());
    assert_eq!(
        authorized(
            &program,
            "edit",
            ALICE,
            &command,
            &pw_host::Limits::unbounded(),
            &[s("n4"), s(BOB), s("EDITED")],
            false,
            &AtomicUsize::new(0)
        ),
        Ok(vec![])
    );
    let watched = Watched::new(&command);
    let denied = authorized(
        &program,
        "edit",
        ALICE,
        &watched,
        &pw_host::Limits::unbounded(),
        &[s("n1"), s(BOB), s("WRONG")],
        false,
        &AtomicUsize::new(0),
    );
    assert!(
        denied.as_ref().is_err_and(|why| why.contains("Wrote")),
        "an existing non-newest note is refused: {denied:?}"
    );
    assert_eq!(watched.body.load(Ordering::SeqCst), 0);
    assert_eq!(
        tables
            .read("pw:tables/notes#get", &[json!(ALICE), json!("n4")])
            .assert_ok("published before commit"),
        serde_json::Value::Null
    );
    command.commit().assert_ok("commit");
    assert_eq!(
        tables
            .read("pw:tables/notes#get", &[json!(ALICE), json!("n4")])
            .assert_ok("published")["text"],
        "EDITED"
    );
    let snapshot = tables.snapshot().assert_ok("snapshot");
    let args = [handle(ALICE), s("n4")];
    let held = authorized(
        &program,
        "Shown",
        ALICE,
        &snapshot,
        &pw_host::Limits::unbounded(),
        &args,
        false,
        &AtomicUsize::new(0),
    )
    .assert_ok("pin newest");
    assert!(matches!(held.as_slice(), [Val::Option(Some(_))]));
    tables
        .transaction(|tx| {
            let mut changed = note_json(ALICE, "n4", BOB, 9);
            changed["text"] = json!("CHANGED AFTER PIN");
            tx.write("pw:tables/notes#put", &[changed])?;
            tx.write("pw:tables/notes#insert", &[note_json(ALICE, "n5", BOB, 10)])
        })
        .assert_ok("change the pinned body and the newest predicate");
    assert_eq!(
        tables
            .read("pw:tables/notes#get", &[json!(ALICE), json!("n4")])
            .assert_ok("later body")["text"],
        "CHANGED AFTER PIN"
    );
    assert_eq!(
        authorized(
            &program,
            "Shown",
            ALICE,
            &snapshot,
            &pw_host::Limits::unbounded(),
            &args,
            false,
            &AtomicUsize::new(0)
        ),
        Ok(held)
    );
    let fresh = tables.snapshot().assert_ok("fresh snapshot");
    let denied = authorized(
        &program,
        "Shown",
        ALICE,
        &fresh,
        &pw_host::Limits::unbounded(),
        &args,
        false,
        &AtomicUsize::new(0),
    );
    assert!(
        denied.as_ref().is_err_and(|why| why.contains("Wrote")),
        "a fresh predicate sees the new newest: {denied:?}"
    );
}

#[test]
fn window_inputs_validate_whole_selectors_before_zero_or_empty_shortcuts() {
    let Some(url) = common::database() else {
        return;
    };
    let scratch = common::Scratch::new(&url, "window_inputs");
    let program = window_program();
    let tables =
        PgTables::open(&url, scratch.schema(), program.tables.clone()).assert_ok("PostgreSQL");
    for (name, args) in [
        ("last-by-author", vec![]),
        ("last-by-author", vec![json!(ALICE)]),
        ("last-by-author", vec![json!(ALICE), json!(true)]),
        ("last-by-author", vec![json!(7), json!(0)]),
        ("last-by-author", vec![json!(ALICE), json!(u64::MAX)]),
        (
            "first-by-author-to-many",
            vec![
                json!([{"author":ALICE,"to":BOB},{"author":ALICE,"to":7}]),
                json!(0),
            ],
        ),
        (
            "first-by-author-to-many",
            vec![json!([{"author":ALICE,"to":BOB,"extra":true}]), json!(-1)],
        ),
    ] {
        let before = tables.statements();
        let answer = tables.read(&format!("pw:tables/notes#{name}"), &args);
        assert!(answer.is_err(), "bad {name}: {answer:?}");
        assert_eq!(tables.statements(), before, "invalid inputs run no SQL");
    }
    for (name, args) in [
        ("last-by-author", vec![json!(ALICE), json!(0)]),
        ("last-by-author", vec![json!(ALICE), json!(-1)]),
        ("first-by-author-to-many", vec![json!([]), json!(i64::MAX)]),
        (
            "first-by-author-to-many",
            vec![json!([{"author":ALICE,"to":BOB}]), json!(0)],
        ),
    ] {
        let before = tables.statements();
        assert_eq!(
            tables
                .read(&format!("pw:tables/notes#{name}"), &args)
                .assert_ok("empty window"),
            json!([])
        );
        assert_eq!(tables.statements(), before, "an empty window runs no SQL");
    }
}

fn answer_ids(answer: &[Val]) -> Vec<String> {
    let [Val::List(rows)] = answer else {
        return Vec::new();
    };
    rows.iter()
        .map(|row| match row {
            Val::Record(fields) => fields
                .iter()
                .find_map(|(name, value)| match (name.as_str(), value) {
                    ("id", Val::String(id)) => Some(id.clone()),
                    _ => None,
                })
                .unwrap_or_default(),
            _ => String::new(),
        })
        .collect()
}

#[test]
fn windows_bound_actual_sql_results_and_compiled_fuel_at_large_cardinality() {
    let Some(url) = common::database() else {
        return;
    };
    let scratch = common::Scratch::new(&url, "window_cost");
    let program = window_program();
    let tables =
        PgTables::open(&url, scratch.schema(), program.tables.clone()).assert_ok("PostgreSQL");
    let fuel = pw_host::Limits {
        fuel: Some(10_000),
        ..pw_host::Limits::unbounded()
    };
    let caller = as_user(ALICE);
    for size in [12, 1500] {
        tables
            .transaction(|tx| {
                tx.write("pw:tables/notes#delete-of-author", &[json!(ALICE)])?;
                for i in 0..size {
                    let mut row = note_json(ALICE, &format!("n{i}"), BOB, i % 3);
                    row["text"] = json!("P".repeat(280));
                    row["draft"] = json!("S".repeat(280));
                    tx.write("pw:tables/notes#insert", &[row])?;
                }
                Ok(())
            })
            .assert_ok("seed cardinality");
        let mut order = (0..size)
            .map(|i| (i % 3, format!("n{i}")))
            .collect::<Vec<_>>();
        order.sort();
        let expected = order
            .iter()
            .rev()
            .take(2)
            .map(|(_, id)| id.clone())
            .collect::<Vec<_>>();
        let before = (
            tables.operation_statements(),
            tables.returned_rows(),
            tables.returned_bytes(),
        );
        let answer = q_limited(
            &program,
            &tables,
            "LastMine",
            &caller,
            &[handle(ALICE), Val::S64(2)],
            &Arc::default(),
            &fuel,
        )
        .assert_ok("bounded compiled scalar window");
        assert_eq!(answer_ids(&answer), expected);
        assert_eq!(tables.operation_statements() - before.0, 1);
        assert_eq!(tables.returned_rows() - before.1, 2);
        let bytes = tables.returned_bytes() - before.2;
        assert!(
            bytes > 2 * 560 && bytes <= 2 * 1024,
            "two full SQL row payloads at {size}: {bytes}"
        );
        let expected = [1, 2]
            .into_iter()
            .flat_map(|stamp| {
                order
                    .iter()
                    .rev()
                    .filter(move |(n, _)| *n == stamp)
                    .take(2)
                    .map(|(_, id)| id.clone())
            })
            .collect::<Vec<_>>();
        let args = [
            handle(BOB),
            Val::List(vec![Val::S64(2), Val::S64(1), Val::S64(2), Val::S64(9)]),
            Val::S64(2),
        ];
        let before = (
            tables.operation_statements(),
            tables.returned_rows(),
            tables.returned_bytes(),
        );
        let answer = q_limited(
            &program,
            &tables,
            "LastTimes",
            &as_user(BOB),
            &args,
            &Arc::default(),
            &fuel,
        )
        .assert_ok("bounded compiled shared set window");
        assert_eq!(answer_ids(&answer), expected);
        assert_eq!(tables.operation_statements() - before.0, 1);
        assert_eq!(
            tables.returned_rows() - before.1,
            4,
            "two keys spend separate bounds"
        );
        let bytes = tables.returned_bytes() - before.2;
        assert!(
            bytes > 4 * 560 && bytes <= 4 * 1024,
            "four full SQL row payloads at {size}: {bytes}"
        );
        let before = (tables.returned_rows(), tables.returned_bytes());
        let all = tables
            .read("pw:tables/notes#of-author", &[json!(ALICE)])
            .assert_ok("growing unbounded neighbor");
        assert_eq!(all.as_array().map_or(0, Vec::len), size as usize);
        assert_eq!(tables.returned_rows() - before.0, size as u64);
        assert!(tables.returned_bytes() - before.1 > size as u64 * 560);
    }
    let one = pw_host::Limits {
        fuel: Some(1),
        ..pw_host::Limits::unbounded()
    };
    let denied = q_limited(
        &program,
        &tables,
        "LastMine",
        &caller,
        &[handle(ALICE), Val::S64(2)],
        &Arc::default(),
        &one,
    );
    assert!(
        denied.as_ref().is_err_and(|why| why.contains("fuel")),
        "fuel guard is exercised: {denied:?}"
    );
}
