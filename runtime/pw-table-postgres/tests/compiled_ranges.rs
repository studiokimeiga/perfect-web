//! Real PostgreSQL range calls through the compiled owner/sharer boundary.
#![allow(dead_code)]
include!("../../pw-table-memory/tests/support/owned.rs");
include!("../../pw-table-memory/tests/support/ranges.rs");
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
fn ranges_count_strict_order_marks_and_keep_full_heterogeneous_request_identity() {
    let program = range_program(false);
    let Some(url) = common::database() else {
        return;
    };
    let scratch = common::Scratch::new(&url, "compiled_ranges");
    let tables =
        PgTables::open(&url, scratch.schema(), program.tables.clone()).assert_ok("PostgreSQL");
    tables
        .transaction(|tx| {
            for row in range_rows() {
                tx.write("pw:tables/notes#insert", &[row])?;
            }
            Ok(())
        })
        .assert_ok("seed");
    for (mark, count) in [(-1, 3), (0, 3), (1, 1), (5, 0), (i64::MAX, 0)] {
        assert_eq!(
            q(
                &program,
                &tables,
                "SinceMine",
                &as_user(ALICE),
                &[handle(ALICE), Val::S64(mark)]
            ),
            Ok(vec![Val::S64(count)])
        );
    }
    assert_eq!(
        q(
            &program,
            &tables,
            "SinceShared",
            &as_user(BOB),
            &[handle(BOB), Val::S64(0)]
        ),
        Ok(vec![Val::S64(3)])
    );
    let input = Val::List(vec![
        requested(vec![], 1),
        requested(vec![], 0),
        requested(vec![], 1),
        requested(vec![], 9),
    ]);
    assert_eq!(
        q(
            &program,
            &tables,
            "RangeMarks",
            &as_user(ALICE),
            &[handle(ALICE), input]
        ),
        Ok(vec![Val::List(vec![
            counted_range(vec![("author", handle(ALICE))], 0, 3),
            counted_range(vec![("author", handle(ALICE))], 1, 1),
            counted_range(vec![("author", handle(ALICE))], 9, 0),
        ])])
    );
    let input = Val::List(vec![
        requested(vec![("to", handle(CAROL))], 1),
        requested(vec![("to", handle(BOB))], 1),
        requested(vec![("to", handle(BOB))], 0),
        requested(vec![("to", handle("u-nobody"))], 0),
    ]);
    assert_eq!(
        q(
            &program,
            &tables,
            "RangeRecipients",
            &as_user(ALICE),
            &[handle(ALICE), input]
        ),
        Ok(vec![Val::List(vec![
            counted_range(vec![("author", handle(ALICE)), ("to", handle(BOB))], 0, 2),
            counted_range(vec![("author", handle(ALICE)), ("to", handle(BOB))], 1, 0),
            counted_range(vec![("author", handle(ALICE)), ("to", handle(CAROL))], 1, 1),
            counted_range(
                vec![("author", handle(ALICE)), ("to", handle("u-nobody"))],
                0,
                0
            ),
        ])])
    );
    let input = Val::List(vec![
        requested(vec![("added", Val::S64(1))], 1),
        requested(vec![("added", Val::S64(1))], 0),
        requested(vec![("added", Val::S64(9))], 0),
    ]);
    assert_eq!(
        q(
            &program,
            &tables,
            "RangeTimes",
            &as_user(BOB),
            &[handle(BOB), input]
        ),
        Ok(vec![Val::List(vec![
            counted_range(vec![("to", handle(BOB)), ("added", Val::S64(1))], 0, 3),
            counted_range(vec![("to", handle(BOB)), ("added", Val::S64(1))], 1, 0),
            counted_range(vec![("to", handle(BOB)), ("added", Val::S64(9))], 0, 0),
        ])])
    );
    let reads = Arc::new(AtomicUsize::new(0));
    for caller in [Caller::Nobody, as_user(BOB)] {
        let answer = q_counted(
            &program,
            &tables,
            "RangeMarks",
            &caller,
            &[handle(ALICE), Val::List(vec![requested(vec![], 0)])],
            &reads,
        );
        assert!(answer.is_err());
        assert_eq!(
            reads.load(Ordering::SeqCst),
            0,
            "forged/no caller reaches no range adapter"
        );
    }
    tables
        .transaction(|tx| tx.write("pw:tables/notes#delete", &[json!(ALICE), json!("n1")]))
        .assert_ok("delete before mark");
    assert_eq!(
        q(
            &program,
            &tables,
            "SinceMine",
            &as_user(ALICE),
            &[handle(ALICE), Val::S64(1)]
        ),
        Ok(vec![Val::S64(1)])
    );
    tables
        .transaction(|tx| tx.write("pw:tables/notes#delete", &[json!(ALICE), json!("n3")]))
        .assert_ok("delete after mark");
    assert_eq!(
        q(
            &program,
            &tables,
            "SinceMine",
            &as_user(ALICE),
            &[handle(ALICE), Val::S64(1)]
        ),
        Ok(vec![Val::S64(0)])
    );
}

#[test]
fn range_predicates_read_staged_rows_and_the_pinned_query_keeps_predicate_and_body() {
    let program = range_program(true);
    let Some(url) = common::database() else {
        return;
    };
    let scratch = common::Scratch::new(&url, "compiled_ranges");
    let tables =
        PgTables::open(&url, scratch.schema(), program.tables.clone()).assert_ok("PostgreSQL");
    tables
        .transaction(|tx| {
            for row in range_rows() {
                tx.write("pw:tables/notes#insert", &[row])?;
            }
            Ok(())
        })
        .assert_ok("seed");
    let s = |v: &str| Val::String(v.into());
    let mut command = tables.begin().assert_ok("command");
    let watched = Watched::new(&command);
    let denied = authorized(
        &program,
        "edit",
        ALICE,
        &watched,
        &pw_host::Limits::unbounded(),
        &[s("n3"), s(CAROL), s("WRONG")],
        false,
        &AtomicUsize::new(0),
    );
    assert!(
        denied.as_ref().is_err_and(|why| why.contains("Wrote")),
        "no newer row: {denied:?}"
    );
    assert_eq!(watched.body.load(Ordering::SeqCst), 0);
    command
        .write("pw:tables/notes#insert", &[note_json(ALICE, "n5", BOB, 9)])
        .assert_ok("stage after mark");
    assert_eq!(
        authorized(
            &program,
            "edit",
            ALICE,
            &command,
            &pw_host::Limits::unbounded(),
            &[s("n3"), s(CAROL), s("EDITED")],
            false,
            &AtomicUsize::new(0)
        ),
        Ok(vec![])
    );
    command.commit().assert_ok("commit");
    let snapshot = tables.snapshot().assert_ok("snapshot");
    let args = [handle(ALICE), s("n3")];
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
    .assert_ok("pin range predicate/body");
    assert!(matches!(held.as_slice(), [Val::Option(Some(_))]));
    tables
        .transaction(|tx| {
            let mut changed = note_json(ALICE, "n3", CAROL, 5);
            changed["text"] = json!("CHANGED AFTER PIN");
            tx.write("pw:tables/notes#put", &[changed])?;
            tx.write("pw:tables/notes#delete", &[json!(ALICE), json!("n5")])
        })
        .assert_ok("remove newer row and change body");
    assert_eq!(
        tables
            .read("pw:tables/notes#get", &[json!(ALICE), json!("n3")])
            .assert_ok("live body")["text"],
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
    let fresh = tables.snapshot().assert_ok("fresh");
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
        "fresh range predicate sees deletion: {denied:?}"
    );
}

#[test]
fn range_marks_keep_refinements_and_native_bool_and_utf8_order_through_compiled_calls() {
    for (kind, values, cases, invalid) in order_cases() {
        let program = range_program_order(kind);
        let Some(url) = common::database() else {
            return;
        };
        let scratch = common::Scratch::new(&url, "range_order_kinds");
        let tables =
            PgTables::open(&url, scratch.schema(), program.tables.clone()).assert_ok("PostgreSQL");
        tables
            .transaction(|tx| {
                for (index, value) in values.iter().enumerate() {
                    let mut row = note_json(ALICE, &format!("n{index}"), BOB, 0);
                    row["added"] = value.clone();
                    tx.write("pw:tables/notes#insert", &[row])?;
                }
                Ok(())
            })
            .assert_ok("seed actual order values");
        assert_eq!(
            tables
                .read("pw:tables/notes#count-by-author", &[json!(ALICE)])
                .assert_ok("absent initial mark uses ordinary count"),
            json!(values.len())
        );
        let before = tables.operation_statements();
        let rejected = q(
            &program,
            &tables,
            "SinceMine",
            &as_user(ALICE),
            &[handle(ALICE), invalid],
        );
        assert!(
            rejected.is_err(),
            "out-of-domain {kind} mark is refused: {rejected:?}"
        );
        assert_eq!(
            tables.operation_statements() - before,
            0,
            "bad mark reaches no adapter operation"
        );
        for (mark, count) in &cases {
            assert_eq!(
                q(
                    &program,
                    &tables,
                    "SinceMine",
                    &as_user(ALICE),
                    &[handle(ALICE), order_value(mark)]
                ),
                Ok(vec![Val::S64(*count)]),
                "{kind} {mark}"
            );
            assert_eq!(
                q(
                    &program,
                    &tables,
                    "SinceShared",
                    &as_user(BOB),
                    &[handle(BOB), order_value(mark)]
                ),
                Ok(vec![Val::S64(*count)]),
                "shared {kind} {mark}"
            );
        }
        let (low, low_count) = &cases[0];
        let (high, high_count) = cases.last().unwrap();
        let requests = Val::List(vec![
            requested_order(vec![], order_value(high)),
            requested_order(vec![], order_value(low)),
            requested_order(vec![], order_value(low)),
        ]);
        assert_eq!(
            q(
                &program,
                &tables,
                "RangeMarks",
                &as_user(ALICE),
                &[handle(ALICE), requests]
            ),
            Ok(vec![Val::List(vec![
                counted_order(
                    vec![("author", handle(ALICE))],
                    order_value(low),
                    *low_count
                ),
                counted_order(
                    vec![("author", handle(ALICE))],
                    order_value(high),
                    *high_count
                ),
            ])]),
            "{kind} records retain typed distinct marks"
        );
        drop(tables);
        let reopened =
            PgTables::open(&url, scratch.schema(), program.tables.clone()).assert_ok("reopen");
        assert_eq!(
            reopened
                .read(
                    "pw:tables/notes#count-after-by-author",
                    &[json!(ALICE), cases[0].0.clone()]
                )
                .assert_ok("range count after reopen"),
            json!(cases[0].1)
        );
    }
}

#[test]
fn malformed_range_requests_validate_whole_before_sql_and_empty_sets_execute_none() {
    let Some(url) = common::database() else {
        return;
    };
    let scratch = common::Scratch::new(&url, "range_inputs");
    let program = range_program(false);
    let tables =
        PgTables::open(&url, scratch.schema(), program.tables.clone()).assert_ok("PostgreSQL");
    for (name, args) in [
        ("count-after-by-author", vec![]),
        ("count-after-by-author", vec![json!(ALICE)]),
        (
            "count-after-by-author",
            vec![json!(ALICE), json!(0), json!(0)],
        ),
        ("count-after-by-author", vec![json!(7), json!(0)]),
        ("count-after-by-author", vec![json!(ALICE), json!(true)]),
        ("count-after-by-author", vec![json!(ALICE), json!(u64::MAX)]),
        (
            "count-after-by-author-many",
            vec![json!([{"author":"u-missing","after":0},{"author":ALICE,"after":true}])],
        ),
        (
            "count-after-by-author-many",
            vec![json!([{"author":ALICE}])],
        ),
        (
            "count-after-by-author-many",
            vec![json!([{"author":ALICE,"after":0,"extra":true}])],
        ),
        ("count-after-by-author-many", vec![json!([ALICE])]),
    ] {
        let before = (
            tables.operation_statements(),
            tables.returned_rows(),
            tables.returned_bytes(),
        );
        let answer = tables.read(&format!("pw:tables/notes#{name}"), &args);
        assert!(answer.is_err(), "bad range {name}: {answer:?}");
        assert_eq!(
            (
                tables.operation_statements(),
                tables.returned_rows(),
                tables.returned_bytes()
            ),
            before,
            "malformed range issues no SQL"
        );
    }
    let before = (
        tables.operation_statements(),
        tables.returned_rows(),
        tables.returned_bytes(),
    );
    assert_eq!(
        tables
            .read("pw:tables/notes#count-after-by-author-many", &[json!([])])
            .assert_ok("empty ranges"),
        json!([])
    );
    assert_eq!(
        (
            tables.operation_statements(),
            tables.returned_rows(),
            tables.returned_bytes()
        ),
        before
    );
    assert_eq!(
        tables
            .read(
                "pw:tables/notes#count-after-by-author",
                &[json!(ALICE), json!(0)]
            )
            .assert_ok("valid absent scalar"),
        json!(0)
    );
    assert_eq!(tables.operation_statements() - before.0, 1);
    assert_eq!(tables.returned_rows() - before.1, 1);
    assert_eq!(tables.returned_bytes() - before.2, 8);
}

#[test]
fn range_counts_bound_real_sql_results_and_compiled_fuel_at_large_cardinality() {
    let Some(url) = common::database() else {
        return;
    };
    let scratch = common::Scratch::new(&url, "range_cost");
    let program = range_program(false);
    let tables =
        PgTables::open(&url, scratch.schema(), program.tables.clone()).assert_ok("PostgreSQL");
    let fuel = pw_host::Limits {
        fuel: Some(1000),
        ..pw_host::Limits::unbounded()
    };
    let set_fuel = pw_host::Limits {
        fuel: Some(6000),
        ..pw_host::Limits::unbounded()
    };
    for size in [12, 1500] {
        tables
            .transaction(|tx| {
                tx.write("pw:tables/notes#delete-of-author", &[json!(ALICE)])?;
                for index in 0..size {
                    let mut row = note_json(ALICE, &format!("n{index}"), BOB, index % 3);
                    row["text"] = json!("P".repeat(280));
                    row["draft"] = json!("S".repeat(280));
                    tx.write("pw:tables/notes#insert", &[row])?;
                }
                Ok(())
            })
            .assert_ok("seed large range");
        let before = (
            tables.operation_statements(),
            tables.returned_rows(),
            tables.returned_bytes(),
        );
        assert_eq!(
            q_limited(
                &program,
                &tables,
                "SinceMine",
                &as_user(ALICE),
                &[handle(ALICE), Val::S64(1)],
                &Arc::default(),
                &fuel
            )
            .assert_ok("compiled scalar range"),
            vec![Val::S64(size / 3)]
        );
        assert_eq!(tables.operation_statements() - before.0, 1);
        assert_eq!(tables.returned_rows() - before.1, 1);
        assert_eq!(tables.returned_bytes() - before.2, 8);
        let requests = Val::List(vec![
            requested(vec![("to", handle(BOB))], 1),
            requested(vec![("to", handle(CAROL))], 0),
            requested(vec![("to", handle(BOB))], 0),
            requested(vec![("to", handle(BOB))], 1),
        ]);
        let before = (
            tables.operation_statements(),
            tables.returned_rows(),
            tables.returned_bytes(),
        );
        assert_eq!(
            q_limited(
                &program,
                &tables,
                "RangeRecipients",
                &as_user(ALICE),
                &[handle(ALICE), requests],
                &Arc::default(),
                &set_fuel
            )
            .assert_ok("compiled heterogeneous ranges"),
            vec![Val::List(vec![
                counted_range(
                    vec![("author", handle(ALICE)), ("to", handle(BOB))],
                    0,
                    size * 2 / 3
                ),
                counted_range(
                    vec![("author", handle(ALICE)), ("to", handle(BOB))],
                    1,
                    size / 3
                ),
                counted_range(vec![("author", handle(ALICE)), ("to", handle(CAROL))], 0, 0),
            ])]
        );
        assert_eq!(tables.operation_statements() - before.0, 1);
        assert_eq!(tables.returned_rows() - before.1, 3);
        assert_eq!(
            tables.returned_bytes() - before.2,
            48,
            "three slot/count pairs, independent of {size}"
        );
        let before = (tables.returned_rows(), tables.returned_bytes());
        let all = tables
            .read("pw:tables/notes#of-author", &[json!(ALICE)])
            .assert_ok("growing unrestricted neighbor");
        assert_eq!(all.as_array().map_or(0, Vec::len), size as usize);
        assert_eq!(tables.returned_rows() - before.0, size as u64);
        assert!(tables.returned_bytes() - before.1 > size as u64 * 560);
    }
    let tiny = pw_host::Limits {
        fuel: Some(1),
        ..pw_host::Limits::unbounded()
    };
    let denied = q_limited(
        &program,
        &tables,
        "SinceMine",
        &as_user(ALICE),
        &[handle(ALICE), Val::S64(1)],
        &Arc::default(),
        &tiny,
    );
    assert!(
        denied.as_ref().is_err_and(|why| why.contains("fuel")),
        "metered failing neighbor: {denied:?}"
    );
}

#[test]
fn a_named_reader_and_its_range_mark_keep_distinct_wit_fields() {
    for owner in ["after", "after_mark", "values", "row"] {
        let program = range_program_owner(owner);
        let Some(url) = common::database() else {
            return;
        };
        let scratch = common::Scratch::new(&url, "range_reader_names");
        let tables =
            PgTables::open(&url, scratch.schema(), program.tables.clone()).assert_ok("PostgreSQL");
        tables
            .transaction(|tx| {
                let mut row = note_json(ALICE, "n1", BOB, 2);
                let row = row.as_object_mut().unwrap();
                let author = row.remove("author").unwrap();
                row.insert(owner.into(), author);
                tx.write(
                    "pw:tables/notes#insert",
                    &[serde_json::Value::Object(row.clone())],
                )
            })
            .assert_ok("seed renamed reader field");
        let mark = if owner == "after" {
            "after_mark"
        } else {
            "after"
        };
        let request = Val::Record(vec![(pw_host::tables::wit_name(mark), Val::S64(1))]);
        let expected = Val::Record(vec![
            (
                "key".into(),
                Val::Record(vec![
                    (pw_host::tables::wit_name(owner), handle(ALICE)),
                    (pw_host::tables::wit_name(mark), Val::S64(1)),
                ]),
            ),
            ("count".into(), Val::S64(1)),
        ]);
        assert_eq!(
            q(
                &program,
                &tables,
                "RangeMarks",
                &as_user(ALICE),
                &[handle(ALICE), Val::List(vec![request])]
            ),
            Ok(vec![Val::List(vec![expected])]),
            "reader {owner}, mark {mark}"
        );
    }
}

#[test]
fn pleris_constructs_heterogeneous_ranges_before_one_set_import() {
    let program = range_program(false);
    let Some(url) = common::database() else {
        return;
    };
    let scratch = common::Scratch::new(&url, "range_constructed");
    let tables =
        PgTables::open(&url, scratch.schema(), program.tables.clone()).assert_ok("PostgreSQL");
    tables
        .transaction(|tx| {
            for row in range_rows() {
                tx.write("pw:tables/notes#insert", &[row])?;
            }
            Ok(())
        })
        .assert_ok("seed");
    let request = |to: &str, mark| {
        Val::Record(vec![
            ("to".into(), handle(to)),
            ("mark".into(), Val::S64(mark)),
        ])
    };
    let args = [
        handle(ALICE),
        Val::List(vec![
            request(CAROL, 1),
            request(BOB, 1),
            request(BOB, 0),
            request(BOB, 1),
        ]),
    ];
    let before = tables.operation_statements();
    assert_eq!(
        q(
            &program,
            &tables,
            "RangeFromRequests",
            &as_user(ALICE),
            &args
        ),
        Ok(vec![Val::List(vec![
            counted_range(vec![("author", handle(ALICE)), ("to", handle(BOB))], 0, 2),
            counted_range(vec![("author", handle(ALICE)), ("to", handle(BOB))], 1, 0),
            counted_range(vec![("author", handle(ALICE)), ("to", handle(CAROL))], 1, 1),
        ])])
    );
    assert_eq!(
        tables.operation_statements() - before,
        1,
        "pure map constructs the records, one data read follows"
    );
}

#[test]
fn a_stale_scalar_range_import_is_refused_before_a_body_that_would_skip_it() {
    let mut stale = range_program(false);
    let current = range_program_order("bool");
    stale.manifest = current.manifest;
    let Some(url) = common::database() else {
        return;
    };
    let scratch = common::Scratch::new(&url, "range_stale_import");
    let tables = PgTables::open(&url, scratch.schema(), current.tables)
        .assert_ok("current Bool-order tables");
    let before = tables.operation_statements();
    let result = q(
        &stale,
        &tables,
        "MaybeSince",
        &as_user(ALICE),
        &[handle(ALICE), Val::S64(0), Val::Bool(false)],
    );
    assert!(
        result
            .as_ref()
            .is_err_and(|why| why.contains("typed otherwise than tables.json")),
        "stale Int import must fail at link, even with its call on the unchosen branch: {result:?}"
    );
    assert_eq!(tables.operation_statements() - before, 0);
}
