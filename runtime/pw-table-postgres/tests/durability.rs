//! **A commit outlives the host that made it** (ADR-0338 D3): one process
//! opens the tables, commits rows with their interaction's record and their
//! outbox's rows, leaves a second command open, and ends; a host started
//! after it reads every committed row, the record and the outbox back, and
//! nothing of the command the first never committed.

mod common;

#[path = "support/assert_result.rs"]
mod assert_result;
use assert_result::*;

use common::*;
use pw_table_postgres::{Interaction, OutboxKind, Outboxed, PgTables, Records};
use serde_json::{Value, json};

/// Set to the schema the first host writes in: this binary run as that
/// host, its one test [`the_first_host_commits_and_ends`].
const FIRST_HOST: &str = "PW_D3_FIRST_HOST_SCHEMA";

#[test]
fn a_commit_outlives_the_host_that_made_it() {
    let Some(url) = database() else { return };
    let scratch = Scratch::new(&url, "durable");
    let exe = std::env::current_exe().assert_ok("this test binary");
    let first = std::process::Command::new(exe)
        .args([
            "--exact",
            "the_first_host_commits_and_ends",
            "--test-threads=1",
        ])
        .env(FIRST_HOST, &scratch.schema)
        .env("PW_TABLES_DATABASE_URL", &url)
        .output()
        .assert_ok("the first host ran");
    // What the first host said, each line marked as its own, so that its
    // test harness's words are never read as this one's.
    let said = |bytes: &[u8]| -> String {
        String::from_utf8_lossy(bytes)
            .lines()
            .map(|line| {
                format!(
                    "  first host| {}",
                    line.replace("panicked at", "stopped at")
                )
            })
            .collect::<Vec<_>>()
            .join(
                "
",
            )
    };
    assert!(
        first.status.success(),
        "the first host failed:
{}
{}",
        said(&first.stdout),
        said(&first.stderr)
    );
    // The first host's process has ended: a second starts.
    let second = open_things(&scratch);
    assert_eq!(
        second.read(&owner("all"), &[]),
        Ok(json!([owner_row("a"), owner_row("b")]))
    );
    assert_eq!(
        second.read(&thing("get"), &[json!("a"), json!(1)]),
        Ok(thing_row("a", 1, 5, "one", true))
    );
    assert_eq!(
        second.recalled("s/durable/1", 0),
        Ok(Some(("[1]".to_string(), "\"kept\"".to_string())))
    );
    assert_eq!(
        scratch.texts("SELECT name FROM pw_outbox ORDER BY id"),
        ["app.Kept"]
    );
    // Nothing of the command it never committed.
    assert_eq!(
        second.read(&owner("get"), &[json!("never")]),
        Ok(Value::Null)
    );
    assert_eq!(scratch.texts("SELECT hash FROM pw_schema").len(), 1);
}

/// **The first host**: run only as [`a_commit_outlives_the_host_that_made_it`]'s
/// child process (otherwise it does nothing), which ends with a command open.
#[test]
fn the_first_host_commits_and_ends() {
    let Ok(schema) = std::env::var(FIRST_HOST) else {
        return;
    };
    let Some(url) = database() else { return };
    let tables = PgTables::open(&url, Some(&schema), things()).assert_ok("the first host");
    let mut tx = tables.begin().assert_ok("a command");
    for id in ["a", "b"] {
        tx.write(&owner("insert"), &[owner_row(id)])
            .assert_ok("an owner");
    }
    tx.write(&thing("insert"), &[thing_row("a", 1, 5, "one", true)])
        .assert_ok("a thing");
    tx.prepare()
        .assert_ok("prepared")
        .publish_with(Records {
            interaction: Some(Interaction {
                key: "s/durable/1",
                sent: "[1]",
                answer: "\"kept\"",
                at_ms: 1,
                forget_before_ms: 0,
            }),
            outbox: &[Outboxed {
                kind: OutboxKind::Event,
                name: "app.Kept".into(),
                values: vec![],
            }],
        })
        .assert_ok("committed");
    let mut open = tables.begin().assert_ok("a command it never commits");
    open.write(&owner("insert"), &[owner_row("never")])
        .assert_ok("written");
    // The host ends here, its command open: its process, its connections.
    std::process::exit(0);
}
