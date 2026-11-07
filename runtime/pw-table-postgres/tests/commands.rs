//! **A command is one serializable transaction, with the platform's
//! records** (ADR-0338 1.3 and 5.5, parts 2, 3, 5, 6 and 7, on PostgreSQL):
//! its reads see its writes and no other read sees them before it commits;
//! one command of a host at a time; its references held when it prepares;
//! its writes, its interaction's record and its outbox's rows committed at
//! once or not at all; a refusal the database makes keeps nothing, and the
//! anomaly serializable refuses is shown where it is not asked for. And a
//! render's snapshot: one moment, in a bounded few seats (ADR-0354).

mod common;

#[path = "support/assert_result.rs"]
mod assert_result;
use assert_result::*;

use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use common::*;
use pw_table_postgres::{
    Error, Interaction, Isolation, Options, OutboxKind, Outboxed, PgTables, Records,
};
use serde_json::{Value, json};

/// The reader every command here is begun for (the owned-rows ADR): one
/// request's, in a table no reader owns, where it changes any row.
fn a_reader() -> pw_host::tables::Writer {
    pw_host::tables::Writer::Reader(
        pw_host::tables::Reader::new("session-1", None).expect("a reader"),
    )
}

fn seed_owners(tables: &PgTables, ids: &[&str]) {
    tables
        .transaction(a_reader(), |tx| {
            for id in ids {
                tx.write(&owner("insert"), &[owner_row(id)])?;
            }
            Ok(())
        })
        .assert_ok("seeded");
}

fn count_owners(tables: &PgTables) -> usize {
    tables
        .read(&owner("all"), &[])
        .assert_ok("all")
        .as_array()
        .map_or(0, Vec::len)
}

/// The same schema, opened again: another host on the database.
fn another_host(scratch: &Scratch, options: Options) -> PgTables {
    PgTables::open_with(&scratch.url, scratch.schema(), things(), options).assert_ok("another host")
}

#[test]
fn a_command_reads_its_own_writes_and_no_other_read_sees_them_before_it_commits() {
    let Some(url) = database() else { return };
    let scratch = Scratch::new(&url, "staged");
    let tables = open_things(&scratch);
    seed_owners(&tables, &["a"]);
    let mut tx = tables.begin(a_reader()).assert_ok("a command");
    tx.write(&thing("insert"), &[thing_row("a", 1, 5, "one", true)])
        .assert_ok("written");
    assert_eq!(
        tx.read(&thing("get"), &[json!("a"), json!(1)]),
        Ok(thing_row("a", 1, 5, "one", true))
    );
    assert_eq!(
        tables.read(&thing("get"), &[json!("a"), json!(1)]),
        Ok(Value::Null),
        "a read outside the command sees none of it"
    );
    let snapshot = tables.snapshot().assert_ok("a seat");
    assert_eq!(
        snapshot.read(&thing("of-owner"), &[json!("a")]),
        Ok(json!([]))
    );
    tx.commit().assert_ok("committed");
    assert_eq!(
        tables.read(&thing("get"), &[json!("a"), json!(1)]),
        Ok(thing_row("a", 1, 5, "one", true))
    );
}

#[test]
fn a_command_dropped_or_refused_at_its_prepare_keeps_nothing() {
    let Some(url) = database() else { return };
    let scratch = Scratch::new(&url, "undone");
    let tables = open_things(&scratch);
    seed_owners(&tables, &["a"]);
    let mut tx = tables.begin(a_reader()).assert_ok("a command");
    tx.write(&owner("put"), &[json!({"id": "a", "name": "changed"})])
        .assert_ok("put");
    tx.write(&thing("insert"), &[thing_row("a", 1, 5, "one", true)])
        .assert_ok("written");
    drop(tx);
    assert_eq!(
        tables.read(&thing("of-owner"), &[json!("a")]),
        Ok(json!([]))
    );
    assert_eq!(
        tables.read(&owner("get"), &[json!("a")]),
        Ok(owner_row("a"))
    );
    // A thing whose owner no row is: refused where it prepares, named as
    // the memory adapter names it, and nothing kept.
    let mut tx = tables.begin(a_reader()).assert_ok("a command");
    tx.write(&owner("insert"), &[owner_row("b")])
        .assert_ok("an owner");
    tx.write(&thing("insert"), &[thing_row("zz", 1, 5, "one", true)])
        .assert_ok("written, its reference held at the commit");
    assert_eq!(
        tx.prepare().err(),
        Some(Error::Input("Things references a missing Owners".into()))
    );
    assert_eq!(count_owners(&tables), 1);
    // An owner deleted while a thing refers to it: refused alike.
    tables
        .transaction(a_reader(), |tx| {
            tx.write(&thing("insert"), &[thing_row("a", 1, 5, "one", true)])
        })
        .assert_ok("a thing of a");
    let mut tx = tables.begin(a_reader()).assert_ok("a command");
    tx.write(&owner("delete"), &[json!("a")])
        .assert_ok("deleted, for now");
    assert_eq!(
        tx.commit().err(),
        Some(Error::Input("Things references a missing Owners".into()))
    );
    assert_eq!(count_owners(&tables), 1);
}

#[test]
fn one_command_of_a_host_writes_at_a_time() {
    let Some(url) = database() else { return };
    let scratch = Scratch::new(&url, "turns");
    let tables = Arc::new(open_things(&scratch));
    seed_owners(&tables, &["a"]);
    let (told, waiting) = mpsc::channel();
    let told = Mutex::new(told);
    tables.tell_on_wait(move || {
        let _ = told.lock().map(|t| t.send(()));
    });
    let mut first = tables.begin(a_reader()).assert_ok("the first command");
    first
        .write(&owner("put"), &[json!({"id": "a", "name": "first"})])
        .assert_ok("the first writes");
    let (begun, began) = mpsc::channel();
    let second = {
        let tables = Arc::clone(&tables);
        std::thread::spawn(move || {
            let mut tx = tables.begin(a_reader())?;
            let _ = begun.send(());
            let seen = tx.read(&owner("get"), &[json!("a")])?;
            tx.write(&owner("put"), &[json!({"id": "a", "name": "second"})])?;
            tx.commit().map(|_| seen)
        })
    };
    // The second is held at its turn, and says so; and it does not begin
    // while the first is open (it would within microseconds of saying so).
    let waited = waiting.recv_timeout(Duration::from_secs(10));
    assert!(waited.is_ok(), "the second command waits its turn");
    let early = began.recv_timeout(Duration::from_millis(250));
    assert!(
        early.is_err(),
        "the second command began while the first was open"
    );
    first.commit().assert_ok("the first commits");
    let joined = second.join();
    // It began after the first ended: it read what the first committed.
    assert!(
        matches!(&joined, Ok(Ok(seen)) if *seen == json!({"id": "a", "name": "first"})),
        "{joined:?}"
    );
    assert_eq!(
        tables.read(&owner("get"), &[json!("a")]),
        Ok(json!({"id": "a", "name": "second"}))
    );
}

#[test]
fn written_names_each_table_a_command_changed_and_no_other() {
    let Some(url) = database() else { return };
    let scratch = Scratch::new(&url, "written");
    let tables = open_things(&scratch);
    seed_owners(&tables, &["a"]);
    tables
        .transaction(a_reader(), |tx| {
            tx.write(&thing("insert"), &[thing_row("a", 1, 5, "one", true)])
        })
        .assert_ok("a thing");
    let mut tx = tables.begin(a_reader()).assert_ok("a command");
    tx.write(&thing("insert"), &[thing_row("a", 2, 5, "two", true)])
        .assert_ok("in");
    tx.write(&thing("delete"), &[json!("a"), json!(2)])
        .assert_ok("out");
    tx.write(&thing("put"), &[thing_row("a", 1, 5, "one", true)])
        .assert_ok("as it was");
    tx.write(&owner("delete"), &[json!("nobody")])
        .assert_ok("no row");
    assert_eq!(tx.written(), Ok(Default::default()));
    // A float's sign is a change.
    let mut zero = thing_row("a", 1, 5, "one", true);
    zero["score"] = json!(-0.0);
    let mut was = thing_row("a", 1, 5, "one", true);
    was["score"] = json!(0.0);
    tx.write(&thing("put"), &[was.clone()]).assert_ok("0.0");
    tables
        .read(&owner("get"), &[json!("a")])
        .assert_ok("another read");
    let names: Vec<String> = tx.written().assert_ok("written").into_iter().collect();
    assert_eq!(names, ["Things"]);
    let committed = tx.commit().assert_ok("committed");
    assert_eq!(committed.written.keys().collect::<Vec<_>>(), ["Things"]);
    let mut tx = tables.begin(a_reader()).assert_ok("a command");
    tx.write(&thing("put"), &[zero]).assert_ok("-0.0");
    assert_eq!(
        tx.written()
            .assert_ok("written")
            .into_iter()
            .collect::<Vec<_>>(),
        ["Things"]
    );
    let committed = tx.commit().assert_ok("committed");
    // Its version: the second commit of this host to write it.
    assert_eq!(committed.written.get("Things"), Some(&3));
    let got = tables
        .read(&thing("get"), &[json!("a"), json!(1)])
        .assert_ok("read");
    assert_eq!(got["score"].to_string(), "-0.0");
}

/// **Write skew, refused** (ADR-0338 5.5, part 3): two hosts' commands, each
/// reading what the other writes, each writing what the other read; under
/// serializable one is refused, and keeps nothing.
#[test]
fn serializable_refuses_write_skew_between_two_hosts() {
    let Some(url) = database() else { return };
    let scratch = Scratch::new(&url, "skew");
    let one = open_things(&scratch);
    let two = another_host(&scratch, Options::default());
    seed_owners(&one, &["a"]);
    let (skewed, kept) = skew(&one, &two);
    assert!(
        matches!(skewed, Err(Error::Serialization(_))),
        "the second commit is refused: {skewed:?}"
    );
    assert_eq!(kept, 2, "one of the two inserts kept");
}

/// **The anomaly where serializable is not asked for** (the negative
/// control): the same two commands on the connection's default, read
/// committed, both commit.
#[test]
fn the_connections_default_read_committed_admits_write_skew() {
    let Some(url) = database() else { return };
    let scratch = Scratch::new(&url, "skewed");
    let one = open_things(&scratch);
    let loose = Options {
        isolation: Isolation::ConnectionDefault,
        ..Default::default()
    };
    let read_committed = with_setting(&url, "default_transaction_isolation", "read committed");
    let two = PgTables::open_with(&read_committed, scratch.schema(), things(), loose.clone())
        .assert_ok("a host left at read committed");
    let three = PgTables::open_with(&read_committed, scratch.schema(), things(), loose)
        .assert_ok("another");
    drop(one);
    seed_owners(&two, &["a"]);
    let (skewed, kept) = skew(&two, &three);
    assert!(skewed.is_ok(), "{skewed:?}");
    assert_eq!(kept, 3, "both inserts kept: the bound of two broken");
}

/// Each host reads the owners, finds one, and adds one (two at most, it
/// believes): the second commit's answer, and the owners kept.
fn skew(one: &PgTables, two: &PgTables) -> (Result<(), Error>, usize) {
    let mut first = one.begin(a_reader()).assert_ok("first");
    let mut second = two.begin(a_reader()).assert_ok("second");
    let seen_first = first.read(&owner("all"), &[]).assert_ok("first reads");
    let seen_second = second.read(&owner("all"), &[]).assert_ok("second reads");
    assert_eq!(seen_first.as_array().map(Vec::len), Some(1));
    assert_eq!(seen_second.as_array().map(Vec::len), Some(1));
    first
        .write(&owner("insert"), &[owner_row("b")])
        .assert_ok("first writes");
    let answer = second
        .write(&owner("insert"), &[owner_row("c")])
        .and_then(|()| {
            first.commit().assert_ok("the first commits");
            second.commit().map(|_| ())
        });
    (answer, count_owners(one))
}

/// **Run again where the database refused it** (`PgTables::transaction`):
/// a first attempt made to meet another host's commit is refused, and the
/// second, run from its beginning, commits.
#[test]
fn a_transaction_runs_again_where_the_database_could_not_serialize_it() {
    let Some(url) = database() else { return };
    let scratch = Scratch::new(&url, "retry");
    let one = open_things(&scratch);
    let two = another_host(&scratch, Options::default());
    seed_owners(&one, &["a"]);
    let mut attempts = 0;
    let answer = one.transaction(a_reader(), |tx| {
        attempts += 1;
        let seen = tx.read(&owner("all"), &[])?;
        if attempts == 1 {
            // Another host commits a row this read did not see, and reads
            // what this command is about to write.
            let mut other = two.begin(a_reader())?;
            other.read(&owner("get"), &[json!("b")])?;
            other.write(&owner("insert"), &[owner_row("c")])?;
            other.commit()?;
        }
        tx.write(&owner("insert"), &[owner_row("b")])?;
        Ok(seen.as_array().map_or(0, Vec::len))
    });
    assert_eq!(attempts, 2, "{answer:?}");
    assert_eq!(answer, Ok(2), "the second attempt read the other's commit");
    assert_eq!(count_owners(&one), 3);
}

fn record<'a>(key: &'a str, at_ms: u64) -> Interaction<'a> {
    Interaction {
        key,
        sent: "[\"sent\"]",
        answer: "{\"ok\":true}",
        at_ms,
        forget_before_ms: at_ms.saturating_sub(86_400_000),
    }
}

fn event(name: &str, values: Vec<Value>) -> Outboxed {
    Outboxed {
        kind: OutboxKind::Event,
        name: name.into(),
        values,
    }
}

#[test]
fn the_platforms_records_commit_with_the_writes() {
    let Some(url) = database() else { return };
    let scratch = Scratch::new(&url, "records");
    let options = Options {
        host: "host-one".into(),
        ..Default::default()
    };
    let one = PgTables::open_with(&url, scratch.schema(), things(), options).assert_ok("open");
    let outbox = [
        event("app.Added", vec![json!("a"), json!(1), json!(true)]),
        Outboxed {
            kind: OutboxKind::Invalidation,
            name: "app.Things".into(),
            values: vec![json!("a"), Value::Null],
        },
        event("app.Second", vec![]),
    ];
    let mut tx = one.begin(a_reader()).assert_ok("a command");
    tx.write(&owner("insert"), &[owner_row("a")])
        .assert_ok("written");
    let committed = tx
        .prepare()
        .assert_ok("prepared")
        .publish_with(Records {
            interaction: Some(record("s/app.add/1", 1_000_000_000)),
            outbox: &outbox,
        })
        .assert_ok("committed with its records");
    // The outbox's rows, as handed, ids in their order.
    let handed: Vec<&Outboxed> = committed.outbox.iter().map(|(_, o)| o).collect();
    assert_eq!(handed, outbox.iter().collect::<Vec<_>>());
    let ids: Vec<i64> = committed.outbox.iter().map(|(id, _)| *id).collect();
    assert!(ids.windows(2).all(|w| w[0] < w[1]), "{ids:?}");
    assert_eq!(
        scratch.texts("SELECT host || ' ' || name FROM pw_outbox ORDER BY id"),
        [
            "host-one app.Added",
            "host-one app.Things",
            "host-one app.Second"
        ]
    );
    assert_eq!(
        one.recalled("s/app.add/1", 1_000_000_000),
        Ok(Some((
            "[\"sent\"]".to_string(),
            "{\"ok\":true}".to_string()
        )))
    );
    // A second host meets the record by its key, and keeps nothing.
    let two = another_host(&scratch, Options::default());
    let mut tx = two.begin(a_reader()).assert_ok("a command");
    tx.write(&owner("insert"), &[owner_row("b")])
        .assert_ok("written");
    let again = tx.prepare().assert_ok("prepared").publish_with(Records {
        interaction: Some(record("s/app.add/1", 1_000_000_500)),
        outbox: &[event("app.Added", vec![json!("b")])],
    });
    assert!(matches!(again, Err(Error::Refused(_))), "{again:?}");
    assert_eq!(count_owners(&one), 1);
    assert_eq!(scratch.texts("SELECT name FROM pw_outbox").len(), 3);
    // A day and a minute later the key is a new interaction, and the
    // commit that records it forgets the old record.
    let later = 1_000_000_000 + 86_460_000;
    let mut tx = two.begin(a_reader()).assert_ok("a command");
    tx.write(&owner("insert"), &[owner_row("b")])
        .assert_ok("written");
    tx.prepare()
        .assert_ok("prepared")
        .publish_with(Records {
            interaction: Some(Interaction {
                answer: "{\"later\":true}",
                ..record("s/app.add/1", later)
            }),
            outbox: &[],
        })
        .assert_ok("a new interaction");
    assert_eq!(
        one.recalled("s/app.add/1", 0).map(|r| r.map(|(_, a)| a)),
        Ok(Some("{\"later\":true}".to_string()))
    );
    // A value no entry is keyed by is refused before anything is sent.
    let mut tx = one.begin(a_reader()).assert_ok("a command");
    tx.write(&owner("insert"), &[owner_row("c")])
        .assert_ok("written");
    let refused = tx.prepare().assert_ok("prepared").publish_with(Records {
        interaction: None,
        outbox: &[event("app.Bad", vec![json!(1.5)])],
    });
    assert!(matches!(refused, Err(Error::Input(_))), "{refused:?}");
    assert_eq!(count_owners(&one), 2);
}

/// **A commit the database refuses keeps nothing** (ADR-0338 5.5, part 5,
/// PostgreSQL's way: a deferred constraint trigger, as ADR-0246's test):
/// not its writes, its interaction's record or its outbox's rows.
#[test]
fn a_commit_the_database_refuses_keeps_nothing() {
    let Some(url) = database() else { return };
    let scratch = Scratch::new(&url, "refusedcommit");
    let tables = open_things(&scratch);
    scratch
        .sql(
            "CREATE FUNCTION refuse() RETURNS trigger LANGUAGE plpgsql AS $$ \
             BEGIN IF NEW.name = 'refuse' THEN RAISE EXCEPTION 'refused at commit'; END IF; RETURN NULL; END $$; \
             CREATE CONSTRAINT TRIGGER refuse AFTER INSERT ON pw_outbox \
             DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION refuse();",
        )
        .assert_ok("a trigger that refuses at the commit");
    let mut tx = tables.begin(a_reader()).assert_ok("a command");
    tx.write(&owner("insert"), &[owner_row("a")])
        .assert_ok("written");
    let refused = tx.prepare().assert_ok("prepared").publish_with(Records {
        interaction: Some(record("s/refused/1", 5_000)),
        outbox: &[event("refuse", vec![])],
    });
    assert!(matches!(refused, Err(Error::Refused(_))), "{refused:?}");
    assert_eq!(count_owners(&tables), 0);
    assert_eq!(tables.recalled("s/refused/1", 0), Ok(None));
    assert!(scratch.texts("SELECT name FROM pw_outbox").is_empty());
    // The adapter serves on: the next command commits.
    seed_owners(&tables, &["a"]);
    assert_eq!(count_owners(&tables), 1);
}

/// **A commit keeps every outbox row it was given, or nothing** (the review
/// of 2026-11-02): a trigger of another hand that drops an outbox row
/// (`BEFORE INSERT` answering `NULL`) would lose an event silently, the
/// commit answered with fewer rows than it carried; the adapter counts the
/// rows the insert answers and refuses the commit short of one.
#[test]
fn a_commit_keeps_every_outbox_row_it_was_given_or_nothing() {
    let Some(url) = database() else { return };
    let scratch = Scratch::new(&url, "lostrow");
    let tables = open_things(&scratch);
    scratch
        .sql(
            "CREATE FUNCTION lose() RETURNS trigger LANGUAGE plpgsql AS $$ \
             BEGIN IF NEW.name = 'lost' THEN RETURN NULL; END IF; RETURN NEW; END $$; \
             CREATE TRIGGER lose BEFORE INSERT ON pw_outbox FOR EACH ROW EXECUTE FUNCTION lose();",
        )
        .assert_ok("a trigger that drops a row");
    let mut tx = tables.begin(a_reader()).assert_ok("a command");
    tx.write(&owner("insert"), &[owner_row("a")])
        .assert_ok("written");
    let refused = tx.prepare().assert_ok("prepared").publish_with(Records {
        interaction: None,
        outbox: &[event("kept", vec![]), event("lost", vec![])],
    });
    assert!(matches!(refused, Err(Error::Refused(_))), "{refused:?}");
    assert_eq!(count_owners(&tables), 0);
    assert!(scratch.texts("SELECT name FROM pw_outbox").is_empty());
}

#[test]
fn a_command_touching_no_table_costs_the_database_nothing() {
    let Some(url) = database() else { return };
    let scratch = Scratch::new(&url, "free");
    let tables = open_things(&scratch);
    let before = tables.statements();
    let tx = tables.begin(a_reader()).assert_ok("a command");
    let committed = tx.commit().assert_ok("committed");
    assert!(committed.written.is_empty());
    drop(tables.begin(a_reader()).assert_ok("another, dropped"));
    assert_eq!(tables.statements(), before);
    // One that writes: its BEGIN, its write, its check of references, its
    // COMMIT; with records, one statement each more.
    let before = tables.statements();
    let mut tx = tables.begin(a_reader()).assert_ok("a command");
    tx.write(&owner("insert"), &[owner_row("a")])
        .assert_ok("written");
    tx.commit().assert_ok("committed");
    assert_eq!(tables.statements() - before, 4);
    let before = tables.statements();
    let mut tx = tables.begin(a_reader()).assert_ok("a command");
    tx.write(&owner("put"), &[owner_row("a")])
        .assert_ok("written");
    tx.prepare()
        .assert_ok("prepared")
        .publish_with(Records {
            interaction: Some(record("s/x/1", 1)),
            outbox: &[event("e", vec![]), event("f", vec![json!(1)])],
        })
        .assert_ok("committed");
    assert_eq!(tables.statements() - before, 5);
}

#[test]
fn provides_measures_what_a_command_is_given() {
    let Some(url) = database() else { return };
    let scratch = Scratch::new(&url, "provides");
    let tables = open_things(&scratch);
    assert_eq!(
        tables
            .provides()
            .map(|p| (p.transactions, p.reads, p.changes)),
        Ok(("serializable", "strong", "none"))
    );
    // Left to a read committed default: what it is, which a host refuses a
    // source stating serializable.
    let loose = Options {
        isolation: Isolation::ConnectionDefault,
        ..Default::default()
    };
    let read_committed = with_setting(&url, "default_transaction_isolation", "read committed");
    let left = PgTables::open_with(&read_committed, scratch.schema(), things(), loose.clone())
        .assert_ok("open");
    assert_eq!(
        left.provides().map(|p| p.transactions),
        Ok("read_committed")
    );
    // Left to a serializable default: serializable.
    let serializable = with_setting(&url, "default_transaction_isolation", "serializable");
    let left =
        PgTables::open_with(&serializable, scratch.schema(), things(), loose).assert_ok("open");
    assert_eq!(left.provides().map(|p| p.transactions), Ok("serializable"));
    // A database that cannot write is refused where it opens.
    let read_only = with_setting(&url, "default_transaction_read_only", "on");
    let opened = PgTables::open(&read_only, scratch.schema(), things());
    assert!(opened.is_err(), "a read-only database opened");
}

#[test]
fn a_snapshot_reads_one_moment_whatever_commits_meanwhile() {
    let Some(url) = database() else { return };
    let scratch = Scratch::new(&url, "snapshot");
    let tables = open_things(&scratch);
    seed_owners(&tables, &["a"]);
    let snapshot = tables.snapshot().assert_ok("a seat");
    assert_eq!(
        snapshot
            .read(&owner("all"), &[])
            .map(|a| a.as_array().map(Vec::len)),
        Ok(Some(1))
    );
    seed_owners(&tables, &["b"]);
    assert_eq!(
        snapshot.read(&owner("get"), &[json!("b")]),
        Ok(Value::Null),
        "the snapshot's moment is its first read's"
    );
    assert_eq!(count_owners(&tables), 2);
    drop(snapshot);
    let snapshot = tables.snapshot().assert_ok("another seat");
    assert_eq!(
        snapshot.read(&owner("get"), &[json!("b")]),
        Ok(owner_row("b"))
    );
    // A read that fails ends its transaction; the next opens another.
    let failed = snapshot.read(&owner("get"), &[json!(1)]);
    assert!(matches!(failed, Err(Error::Input(_))), "{failed:?}");
    assert_eq!(
        snapshot.read(&owner("get"), &[json!("a")]),
        Ok(owner_row("a"))
    );
}

#[test]
fn render_seats_are_bounded_and_every_one_given_back() {
    let Some(url) = database() else { return };
    let scratch = Scratch::new(&url, "seats");
    let options = Options {
        render_seats: 2,
        seat_wait: Duration::from_secs(2),
        ..Default::default()
    };
    let tables = Arc::new(
        PgTables::open_with(&url, scratch.schema(), things(), options).assert_ok("two seats"),
    );
    seed_owners(&tables, &["a"]);
    let first = tables.snapshot().assert_ok("first");
    let second = tables.snapshot().assert_ok("second");
    first.read(&owner("all"), &[]).assert_ok("read");
    second.read(&owner("all"), &[]).assert_ok("read");
    let third = tables.snapshot();
    assert!(
        matches!(third, Err(Error::Unavailable(_))),
        "a third seat, past the wait"
    );
    // One waiting is seated when one is given back: the waiter is seen
    // waiting (the pool's own count, not a sleep), and then one is.
    let waiter = {
        let tables = Arc::clone(&tables);
        std::thread::spawn(move || {
            let seat = tables.snapshot();
            seat.and_then(|s| s.read(&owner("get"), &[json!("a")]))
        })
    };
    let mut waiting = false;
    for _ in 0..2000 {
        if tables.seats().2 == 1 {
            waiting = true;
            break;
        }
        std::thread::yield_now();
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(waiting, "the third render waits for a seat");
    drop(first);
    let seated = waiter.join();
    assert!(matches!(seated, Ok(Ok(_))), "{seated:?}");
    drop(second);
    assert_eq!(tables.seats(), (0, 2, 0));
}

/// **A reference may be written before the row it refers to**: references
/// are held where the command prepares, as the memory adapter holds them,
/// so related rows are written in either order.
#[test]
fn a_reference_may_be_written_before_the_row_it_refers_to() {
    let Some(url) = database() else { return };
    let scratch = Scratch::new(&url, "either");
    let tables = open_things(&scratch);
    let committed = tables.transaction(a_reader(), |tx| {
        tx.write(&thing("insert"), &[thing_row("q", 1, 5, "one", true)])?;
        tx.write(&owner("insert"), &[owner_row("q")])
    });
    assert_eq!(committed, Ok(()));
    assert_eq!(
        tables.read(&thing("of-owner"), &[json!("q")]),
        Ok(json!([thing_row("q", 1, 5, "one", true)]))
    );
}

/// **The outbox's ids are in commit order** (ADR-0369): a commit takes the
/// outbox's lock before its rows take their ids and holds it to its end, so
/// a second host's commit waits for the first's, and its rows come after
/// (a reader at a cursor misses none). The first commit is held at its end
/// by a lock this test holds; the second is seen waiting for the outbox's.
#[test]
fn the_outbox_takes_its_ids_in_commit_order() {
    let Some(url) = database() else { return };
    let scratch = Scratch::new(&url, "ordered");
    let joiner = if url.contains('?') { '&' } else { '?' };
    let named = format!("{url}{joiner}application_name={}", scratch.schema);
    let one =
        Arc::new(PgTables::open(&named, scratch.schema(), things()).assert_ok("the first host"));
    let two =
        Arc::new(PgTables::open(&named, scratch.schema(), things()).assert_ok("the second host"));
    scratch
        .sql(
            "CREATE FUNCTION held() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN \
             IF NEW.name = 'held' THEN PERFORM pg_advisory_xact_lock(hashtext(TG_TABLE_SCHEMA)); END IF; \
             RETURN NULL; END $$; \
             CREATE CONSTRAINT TRIGGER held AFTER INSERT ON pw_outbox \
             DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION held();",
        )
        .assert_ok("a commit held at its end");
    let mut holder = postgres::Client::connect(&url, postgres::NoTls).assert_ok("the holder");
    holder
        .batch_execute(&format!(
            "BEGIN; SELECT pg_advisory_xact_lock(hashtext('{}'))",
            scratch.schema
        ))
        .assert_ok("the lock held");
    // Each writes a thing of its own owner: no row the other reads, so the
    // two commits never conflict, and only the outbox's order orders them.
    seed_owners(&one, &["a", "b"]);
    let publish = |tables: Arc<PgTables>, name: &'static str, of: &'static str| {
        std::thread::spawn(move || {
            let mut tx = tables.begin(a_reader())?;
            tx.write(&thing("insert"), &[thing_row(of, 1, 5, of, true)])?;
            tx.prepare()?.publish_with(Records {
                interaction: None,
                outbox: &[event(name, vec![])],
            })
        })
    };
    let first = publish(Arc::clone(&one), "held", "a");
    let first_held = until(|| waiting_on_advisory(&scratch, &scratch.schema) == 1);
    let second = publish(Arc::clone(&two), "after", "b");
    let second_waits = until(|| waiting_on_advisory(&scratch, &scratch.schema) == 2);
    holder
        .batch_execute("COMMIT")
        .assert_ok("the lock given up");
    let first = first.join();
    let second = second.join();
    assert!(first_held, "the first commit is held at its end");
    assert!(
        second_waits,
        "the second commit waits for the first's outbox lock"
    );
    let ids =
        |joined: &std::thread::Result<Result<pw_table_postgres::Committed, Error>>| match joined {
            Ok(Ok(committed)) => committed.outbox.iter().map(|(id, _)| *id).collect(),
            _ => Vec::new(),
        };
    let (first, second) = (ids(&first), ids(&second));
    assert_eq!((first.len(), second.len()), (1, 1), "{first:?} {second:?}");
    assert!(first[0] < second[0], "{first:?} {second:?}");
}

/// **A commit never answered is not taken for a refusal**: the command's
/// session ended under it (its backend terminated) before its `COMMIT` was
/// answered, so whether it committed is not known to the adapter, and it
/// says so; what the database holds decides (here, nothing was kept).
#[test]
fn a_commit_never_answered_is_not_taken_for_a_refusal() {
    let Some(url) = database() else { return };
    let scratch = Scratch::new(&url, "unknown");
    let joiner = if url.contains('?') { '&' } else { '?' };
    let named = format!("{url}{joiner}application_name={}", scratch.schema);
    let tables = PgTables::open(&named, scratch.schema(), things()).assert_ok("open");
    let mut tx = tables.begin(a_reader()).assert_ok("a command");
    tx.write(&owner("insert"), &[owner_row("a")])
        .assert_ok("written");
    let prepared = tx.prepare().assert_ok("prepared");
    let ended = scratch.texts(&format!(
        "SELECT pg_terminate_backend(pid)::text FROM pg_stat_activity \
         WHERE application_name = '{}' AND state = 'idle in transaction'",
        scratch.schema
    ));
    assert_eq!(ended, ["true"], "the command's session ended");
    let published = prepared.publish();
    assert!(
        matches!(published, Err(Error::OutcomeUnknown(_))),
        "{published:?}"
    );
    assert_eq!(tables.read(&owner("get"), &[json!("a")]), Ok(Value::Null));
}
