//! The same compiled count contract, on real PostgreSQL.
#![allow(dead_code)]
include!("../../pw-table-memory/tests/support/owned.rs");
include!("../../pw-table-memory/tests/support/counts.rs");
mod common;
use pw_table_postgres::PgTables;

#[test]
fn counts_use_the_same_compiled_owner_and_sharer_boundary() {
    let Some(url) = common::database() else {
        return;
    };
    let scratch = common::Scratch::new(&url, "compiled_counts");
    let program = count_program();
    let tables =
        PgTables::open(&url, scratch.schema(), program.tables.clone()).assert_ok("PostgreSQL");
    tables
        .transaction(|tx| {
            tx.write("pw:tables/notes#insert", &[note_json(ALICE, "n1", BOB, 1)])?;
            tx.write(
                "pw:tables/notes#insert",
                &[note_json(ALICE, "n2", CAROL, 2)],
            )?;
            tx.write("pw:tables/notes#insert", &[note_json(BOB, "n1", ALICE, 3)])
        })
        .assert_ok("seed");
    assert_compiled_counts(&program, &tables.snapshot().assert_ok("snapshot"));
    drop(tables);
    let reopened = PgTables::open(&url, scratch.schema(), program.tables.clone())
        .assert_ok("reopened PostgreSQL");
    assert_compiled_counts(
        &program,
        &reopened.snapshot().assert_ok("reopened snapshot"),
    );
}

#[test]
fn count_result_rows_bytes_and_compiled_fuel_stay_bounded_at_large_cardinality() {
    let Some(url) = common::database() else {
        return;
    };
    let scratch = common::Scratch::new(&url, "count_cost");
    let program = count_program();
    let tables =
        PgTables::open(&url, scratch.schema(), program.tables.clone()).assert_ok("PostgreSQL");
    for size in [5, 1500] {
        tables
            .transaction(|tx| {
                tx.write("pw:tables/notes#delete-of-author", &[json!(ALICE)])?;
                for i in 0..size {
                    let mut row = note_json(ALICE, &format!("n{i}"), BOB, i);
                    row["text"] = json!("P".repeat(280));
                    row["draft"] = json!("S".repeat(280));
                    tx.write("pw:tables/notes#insert", &[row])?;
                }
                Ok(())
            })
            .assert_ok("seed substantial rows");
        let before = (
            tables.operation_statements(),
            tables.returned_rows(),
            tables.returned_bytes(),
        );
        assert_eq!(
            count_query(
                &program,
                &tables.snapshot().assert_ok("query snapshot"),
                "CountMine",
                &as_user(ALICE),
                &[handle(ALICE)],
                1000
            ),
            Ok(vec![Val::S64(size)])
        );
        assert_eq!(tables.operation_statements() - before.0, 1);
        assert_eq!(
            tables.returned_rows() - before.1,
            1,
            "one actual SQL result row at cardinality {size}"
        );
        assert_eq!(
            tables.returned_bytes() - before.2,
            8,
            "only the count bigint payload, no stored records"
        );
        let args = [
            handle(BOB),
            Val::List(vec![Val::S64(0), Val::S64(99999), Val::S64(0)]),
        ];
        let before = (
            tables.operation_statements(),
            tables.returned_rows(),
            tables.returned_bytes(),
        );
        let answer = count_query(
            &program,
            &tables.snapshot().assert_ok("query snapshot"),
            "CountTimes",
            &as_user(BOB),
            &args,
            3000,
        )
        .assert_ok("compiled set count");
        let key = |added| {
            Val::Record(vec![
                ("to".into(), Val::String(BOB.into())),
                ("added".into(), Val::S64(added)),
            ])
        };
        assert_eq!(
            answer,
            vec![Val::List(vec![
                count_entry(key(0), 1),
                count_entry(key(99999), 0)
            ])]
        );
        assert_eq!(tables.operation_statements() - before.0, 1);
        assert_eq!(tables.returned_rows() - before.1, 2);
        assert_eq!(
            tables.returned_bytes() - before.2,
            32,
            "two bigint slots plus counts"
        );
        let before = (tables.returned_rows(), tables.returned_bytes());
        let all = tables
            .read("pw:tables/notes#of-author", &[json!(ALICE)])
            .assert_ok("unbounded-read cost neighbor");
        assert_eq!(all.as_array().unwrap().len(), size as usize);
        assert_eq!(tables.returned_rows() - before.0, size as u64);
        assert!(
            tables.returned_bytes() - before.1 > size as u64 * 560,
            "instrument observes row payload growth"
        );
    }
    let denied = count_query(
        &program,
        &tables.snapshot().assert_ok("query snapshot"),
        "CountMine",
        &as_user(ALICE),
        &[handle(ALICE)],
        1,
    );
    assert!(
        denied.as_ref().is_err_and(|why| why.contains("fuel")),
        "fuel neighbor is enforced: {denied:?}"
    );
}
