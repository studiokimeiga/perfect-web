//! Actual compiled owned/shared windows, including per-key limits and ties.
#![allow(dead_code)]
include!("support/owned.rs");
include!("support/windows.rs");

#[test]
fn windows_return_bounded_ordered_rows_through_the_actual_owner_and_sharer_wit() {
    let program = window_program();
    let tables = MemoryTables::new(program.tables.clone()).assert_ok("memory");
    window_seed(&tables);
    assert_eq!(
        query(
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
        query(
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
        query(
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
        query(
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
            query(
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
        let denied = query_counted(
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
    let program = window_program_kind(true);
    let tables = MemoryTables::new(program.tables.clone()).assert_ok("memory");
    window_seed(&tables);
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
        .transaction(|tx| tx.write("pw:tables/notes#insert", &[note_json(ALICE, "n5", BOB, 10)]))
        .assert_ok("later newest");
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
