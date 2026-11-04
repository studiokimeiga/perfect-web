//! Counts aggregate existing declared selectors without materializing rows.
#[path = "support/assert_result.rs"]
mod assert_result;
mod common;
use assert_result::*;
use common::*;
use serde_json::json;

#[test]
fn counts_cover_distinct_missing_and_typed_selectors_in_one_statement() {
    let Some(url) = database() else { return };
    let scratch = Scratch::new(&url, "counts");
    let tables = open_things(&scratch);
    tables
        .transaction(|tx| {
            for id in ["a", "b"] {
                tx.write(&owner("insert"), &[owner_row(id)])?;
            }
            for (o, n, rank, slug, flag) in [
                ("a", 1, 10, "one", true),
                ("a", 2, 20, "two", true),
                ("b", 1, 30, "three", false),
            ] {
                tx.write(&thing("insert"), &[thing_row(o, n, rank, slug, flag)])?;
            }
            Ok(())
        })
        .assert_ok("seed");
    for (name, args, expected) in [
        ("count-by-owner", vec![json!("a")], json!(2)),
        ("count-by-owner", vec![json!("absent")], json!(0)),
        ("count-by-owner-n", vec![json!("a"), json!(1)], json!(1)),
        ("count-by-slug", vec![json!("one")], json!(1)),
        ("count-by-flag", vec![json!(true)], json!(2)),
        (
            "count-by-owner-many",
            vec![json!(["z", "b", "a", "a"])],
            json!([
                {"key":"a", "count":2}, {"key":"b", "count":1}, {"key":"z", "count":0}
            ]),
        ),
        (
            "count-by-flag-many",
            vec![json!([true, false, true])],
            json!([
                {"key":false, "count":1}, {"key":true, "count":2}
            ]),
        ),
        (
            "count-by-owner-n-many",
            vec![json!([{"owner":"b", "n":9}, {"owner":"a", "n":1}, {"owner":"a", "n":1}])],
            json!([
                {"key":{"owner":"a", "n":1}, "count":1}, {"key":{"owner":"b", "n":9}, "count":0}
            ]),
        ),
        (
            "count-by-owner-rank-many",
            vec![json!([{"owner":"b", "rank":10}, {"owner":"a", "rank":10}])],
            json!([
                {"key":{"owner":"a", "rank":10}, "count":1}, {"key":{"owner":"b", "rank":10}, "count":0}
            ]),
        ),
    ] {
        let before = tables.operation_statements();
        assert_eq!(
            tables.read(&thing(name), &args).assert_ok(name),
            expected,
            "{name}"
        );
        assert_eq!(
            tables.operation_statements() - before,
            1,
            "one aggregate for {name}"
        );
    }
    let before = tables.statements();
    assert_eq!(
        tables
            .read(&thing("count-by-owner-many"), &[json!([])])
            .assert_ok("empty"),
        json!([])
    );
    assert_eq!(tables.statements(), before, "empty set runs no statement");
    for args in [vec![], vec![json!(17)], vec![json!(["a", 17])]] {
        let before = tables.statements();
        assert!(tables.read(&thing("count-by-owner-many"), &args).is_err());
        assert_eq!(tables.statements(), before, "bad selectors fail before SQL");
    }
}

#[test]
fn counts_read_the_command_and_held_snapshot_states() {
    let Some(url) = database() else { return };
    let scratch = Scratch::new(&url, "count_states");
    let tables = open_things(&scratch);
    tables
        .transaction(|tx| {
            tx.write(&owner("insert"), &[owner_row("a")])?;
            tx.write(&thing("insert"), &[thing_row("a", 1, 10, "one", true)])
        })
        .assert_ok("seed");
    let snapshot = tables.snapshot().assert_ok("snapshot");
    let import = thing("count-by-owner");
    assert_eq!(
        snapshot
            .read(&import, &[json!("a")])
            .assert_ok("pin snapshot"),
        json!(1)
    );
    let mut command = tables.begin().assert_ok("command");
    command
        .write(&thing("insert"), &[thing_row("a", 2, 20, "two", true)])
        .assert_ok("staged insert");
    assert_eq!(
        command.read(&import, &[json!("a")]).assert_ok("staged"),
        json!(2)
    );
    assert_eq!(
        tables.read(&import, &[json!("a")]).assert_ok("published"),
        json!(1)
    );
    command.commit().assert_ok("commit");
    assert_eq!(
        tables
            .read(&import, &[json!("a")])
            .assert_ok("published after commit"),
        json!(2)
    );
    assert_eq!(
        snapshot
            .read(&import, &[json!("a")])
            .assert_ok("held snapshot"),
        json!(1)
    );
    let mut command = tables.begin().assert_ok("later command");
    command
        .write(&thing("delete-of-owner"), &[json!("a")])
        .assert_ok("staged delete");
    assert_eq!(
        command
            .read(&import, &[json!("a")])
            .assert_ok("staged delete count"),
        json!(0)
    );
    drop(command);
    assert_eq!(
        tables.read(&import, &[json!("a")]).assert_ok("rollback"),
        json!(2)
    );
}

#[test]
fn result_counters_include_prepared_startup_reads_on_open_and_reopen() {
    let Some(url) = database() else { return };
    let scratch = Scratch::new(&url, "count_startup");
    for _ in 0..2 {
        let tables = open_things(&scratch);
        assert!(
            tables.returned_rows() > 0,
            "startup catalog/schema prepared reads are included"
        );
        assert!(
            tables.returned_bytes() > 0,
            "startup binary payloads are included"
        );
        let before = (tables.returned_rows(), tables.returned_bytes());
        assert_eq!(
            tables
                .read(&thing("count-by-owner"), &[json!("missing")])
                .assert_ok("count"),
            json!(0)
        );
        assert_eq!(
            (
                tables.returned_rows() - before.0,
                tables.returned_bytes() - before.1
            ),
            (1, 8)
        );
    }
}
