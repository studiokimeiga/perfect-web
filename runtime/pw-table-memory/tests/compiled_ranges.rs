//! Real compiled range calls: strict marks, heterogeneous keys and authority.
#![allow(dead_code)]
include!("support/owned.rs");
include!("support/ranges.rs");

#[test]
fn ranges_count_strict_order_marks_and_keep_full_heterogeneous_request_identity() {
    let program = range_program(false);
    let tables = MemoryTables::new(program.tables.clone()).assert_ok("memory");
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
            query(
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
        query(
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
        query(
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
        query(
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
        query(
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
        let answer = query_counted(
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
        query(
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
        query(
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
    let tables = MemoryTables::new(program.tables.clone()).assert_ok("memory");
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
        let tables = MemoryTables::new(program.tables.clone()).assert_ok("memory");
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
        let before = tables.statements();
        let rejected = query(
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
            tables.statements() - before,
            0,
            "bad mark reaches no adapter operation"
        );
        for (mark, count) in &cases {
            assert_eq!(
                query(
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
                query(
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
            query(
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
    }
}

#[test]
fn a_named_reader_and_its_range_mark_keep_distinct_wit_fields() {
    for owner in ["after", "after_mark", "values", "row"] {
        let program = range_program_owner(owner);
        let tables = MemoryTables::new(program.tables.clone()).assert_ok("memory");
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
            query(
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
fn a_forged_range_manifest_without_order_is_refused_and_reader_only_ranges_are_valid() {
    let program = range_program(false);
    let mut manifest = program.tables.clone();
    manifest.tables[0].functions.retain(|f| f.primitive.range());
    let mut broken = manifest.clone();
    broken.tables[0].schema.order = None;
    let schema =
        pw_core::tables::Schema::of(broken.tables.iter().map(|t| t.schema.clone()).collect());
    broken.hash = schema.hash.clone();
    broken.migration.to = schema.hash;

    let why = MemoryTables::new(broken).err();
    assert!(
        why.as_ref()
            .is_some_and(|why| format!("{why:?}").contains("declared order")),
        "{why:?}"
    );
    let tables = MemoryTables::new(manifest).assert_ok("declared order restored");
    assert_eq!(
        tables
            .read(
                "pw:tables/notes#count-after-by-author-many",
                &[json!([{"author":ALICE,"after":0}])]
            )
            .assert_ok("reader-only batched mark"),
        json!([{"key":{"author":ALICE,"after":0},"count":0}])
    );
}

#[test]
fn pleris_constructs_heterogeneous_ranges_before_one_set_import() {
    let program = range_program(false);
    let tables = MemoryTables::new(program.tables.clone()).assert_ok("memory");
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
    let before = tables.statements();
    assert_eq!(
        query(
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
        tables.statements() - before,
        1,
        "pure map constructs the records, one data read follows"
    );
}

#[test]
fn a_stale_scalar_range_import_is_refused_before_a_body_that_would_skip_it() {
    let mut stale = range_program(false);
    let current = range_program_order("bool");
    stale.manifest = current.manifest;
    let tables = MemoryTables::new(current.tables).assert_ok("current Bool-order tables");
    let before = tables.statements();
    let result = query(
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
    assert_eq!(tables.statements() - before, 0);
}
