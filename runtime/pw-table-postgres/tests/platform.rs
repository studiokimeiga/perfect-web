//! **The platform's other tables, on PostgreSQL** (track `durable-tables`,
//! ADR-0338 1.3 and 5.2): a command's runs written in its transaction, with
//! its rows, its interaction's record and its outbox, or not at all; a run
//! taken once, by the generation it was selected as; the sessions and
//! sign-ins a deployment keeps; and a command fenced on the session it acts
//! for, which commits only as the session it was admitted as, and which a
//! session's change waits for.

mod common;

#[path = "support/assert_result.rs"]
mod assert_result;
use assert_result::*;

use std::time::Duration;

use common::*;
use pw_table_postgres::{
    Error, Fence, Interaction, Lifetimes, OutboxKind, Outboxed, PgTables, Records, Run, Scheduling,
    Session,
};
use serde_json::{Value, json};

const DAY: Duration = Duration::from_secs(24 * 60 * 60);

fn lifetimes() -> Lifetimes {
    Lifetimes {
        idle: DAY,
        absolute: DAY * 2,
    }
}

/// A session id's hash: 32 bytes, as a SHA-256 is.
fn hash(n: u8) -> Vec<u8> {
    vec![n; 32]
}

fn session(user: &str, epoch: u8) -> Session {
    Session {
        user: user.to_string(),
        handle: format!("@{user}"),
        name: format!("Name {user}"),
        issuer: "https://id.example.test".to_string(),
        epoch: [epoch; 16],
    }
}

fn scheduling(key: &str, due: Option<i64>) -> Scheduling {
    Scheduling {
        key: key.to_string(),
        command: "app.remind".to_string(),
        args: format!("[\"{key}\"]"),
        user: "u-a".to_string(),
        due_ms: due,
    }
}

fn fence(n: u8, expected: Option<Session>, lifetimes: Lifetimes) -> Fence {
    Fence {
        hash: hash(n),
        lifetimes,
        expected,
    }
}

/// Each run kept: its key's command, and when it is due.
fn kept(tables: &PgTables) -> Vec<(String, i64)> {
    tables
        .runs()
        .assert_ok("the runs")
        .into_iter()
        .map(|r| (r.args, r.due_ms))
        .collect()
}

/// The one run due at `now`.
fn due(tables: &PgTables, now: i64) -> Run {
    let mut due = tables.due_runs(now, 16).assert_ok("the runs due");
    assert_eq!(due.len(), 1, "{due:?}");
    due.remove(0)
}

fn owner_of(tables: &PgTables, id: &str) -> Value {
    tables
        .read(&owner("get"), &[json!(id)])
        .assert_ok("an owner read")
}

/// **A command's runs commit with its writes, or not at all** (ADR-0334 in
/// the adapter's transaction): committed, the row and the run; dropped,
/// neither; refused at its `COMMIT` by the database, neither, nor its
/// record, nor its outbox; scheduled again, the run pending for its key
/// replaced under a new id; cancelled, gone.
#[test]
fn a_commands_runs_commit_with_its_writes_or_not_at_all() {
    let Some(url) = database() else { return };
    let scratch = Scratch::new(&url, "runs");
    let tables = open_things(&scratch);
    let mut tx = tables.begin().assert_ok("a command");
    tx.write(&owner("insert"), &[owner_row("a")])
        .assert_ok("an owner");
    tx.schedule(&[scheduling("k1", Some(1000))])
        .assert_ok("scheduled");
    assert_eq!(
        kept(&tables),
        [],
        "no other read sees a run before it commits"
    );
    tx.commit().assert_ok("committed");
    assert_eq!(kept(&tables), [("[\"k1\"]".to_string(), 1000)]);
    assert_eq!(owner_of(&tables, "a"), owner_row("a"));
    // Dropped: neither.
    let mut tx = tables.begin().assert_ok("a command");
    tx.write(&owner("insert"), &[owner_row("b")])
        .assert_ok("an owner");
    tx.schedule(&[scheduling("k2", Some(2000))])
        .assert_ok("scheduled");
    drop(tx);
    assert_eq!(owner_of(&tables, "b"), Value::Null);
    assert_eq!(kept(&tables), [("[\"k1\"]".to_string(), 1000)]);
    // Refused by the database at its COMMIT (a deferred trigger of another
    // hand's): the row, the run, the record and the outbox, none kept.
    scratch
        .sql(
            "CREATE FUNCTION refuse() RETURNS trigger LANGUAGE plpgsql AS \
             $$ BEGIN RAISE EXCEPTION 'refused at the commit'; END $$; \
             CREATE CONSTRAINT TRIGGER refuse AFTER INSERT ON \"Owners\" \
             DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION refuse();",
        )
        .assert_ok("a trigger refusing at the commit");
    let mut tx = tables.begin().assert_ok("a command");
    tx.write(&owner("insert"), &[owner_row("c")])
        .assert_ok("an owner");
    tx.schedule(&[scheduling("k3", Some(3000))])
        .assert_ok("scheduled");
    let refused = tx.prepare().assert_ok("prepared").publish_with(Records {
        interaction: Some(Interaction {
            key: "s/refused/1",
            sent: "[]",
            answer: "\"no\"",
            at_ms: 1,
            forget_before_ms: 0,
        }),
        outbox: &[Outboxed {
            kind: OutboxKind::Event,
            name: "app.Refused".into(),
            values: vec![],
        }],
    });
    assert!(matches!(refused, Err(Error::Refused(_))), "{refused:?}");
    assert_eq!(owner_of(&tables, "c"), Value::Null);
    assert_eq!(kept(&tables), [("[\"k1\"]".to_string(), 1000)]);
    assert_eq!(tables.recalled("s/refused/1", 0), Ok(None));
    assert_eq!(
        scratch.texts("SELECT name FROM pw_outbox"),
        Vec::<String>::new()
    );
    scratch
        .sql("DROP TRIGGER refuse ON \"Owners\"")
        .assert_ok("the trigger dropped");
    // Scheduled again: one run for its key, under a new id.
    let first = due(&tables, 1000);
    tables
        .transaction(|tx| tx.schedule(&[scheduling("k1", Some(5000))]))
        .assert_ok("scheduled again");
    let again = tables.runs().assert_ok("the runs");
    assert_eq!(again.len(), 1, "{again:?}");
    assert_ne!(again[0].id, first.id, "a new generation");
    assert_eq!(again[0].due_ms, 5000);
    // Cancelled: gone.
    tables
        .transaction(|tx| tx.schedule(&[scheduling("k1", None)]))
        .assert_ok("cancelled");
    assert_eq!(kept(&tables), []);
    assert_eq!(tables.next_run(), Ok(None));
}

/// **A run is taken once, by the command that completes it**, and only as
/// the generation it was selected as: not before it is due, not as another
/// generation, not twice; a taking dropped keeps it.
#[test]
fn a_run_is_taken_once_by_the_command_that_completes_it() {
    let Some(url) = database() else { return };
    let scratch = Scratch::new(&url, "taken");
    let tables = open_things(&scratch);
    tables
        .transaction(|tx| tx.schedule(&[scheduling("k", Some(1000))]))
        .assert_ok("scheduled");
    assert_eq!(tables.due_runs(999, 16), Ok(vec![]), "not due yet");
    assert_eq!(tables.next_run(), Ok(Some(1000)));
    let run = due(&tables, 1000);
    assert_eq!(tables.run_current(&run, 1000), Ok(true));
    assert_eq!(tables.run_current(&run, 999), Ok(false));
    let mut tx = tables.begin().assert_ok("a command");
    let stale = Run {
        attempts: 1,
        ..run.clone()
    };
    assert_eq!(tx.consume(&stale, 1000), Ok(false), "another generation");
    assert_eq!(tx.consume(&run, 999), Ok(false), "not due yet");
    drop(tx);
    // A taking dropped keeps it.
    let mut tx = tables.begin().assert_ok("a command");
    assert_eq!(tx.consume(&run, 1000), Ok(true));
    drop(tx);
    assert_eq!(tables.run_current(&run, 1000), Ok(true));
    // Taken with the command's write, and committed: gone, the row kept.
    let mut tx = tables.begin().assert_ok("a command");
    assert_eq!(tx.consume(&run, 1000), Ok(true));
    tx.write(&owner("insert"), &[owner_row("r")])
        .assert_ok("its write");
    tx.commit().assert_ok("committed");
    assert_eq!(kept(&tables), []);
    assert_eq!(owner_of(&tables, "r"), owner_row("r"));
    // Taken again (a second host, after the first committed): not current.
    let mut tx = tables.begin().assert_ok("a command");
    assert_eq!(tx.consume(&run, 1000), Ok(false));
    drop(tx);
}

/// **A failed run is retried by its generation alone**: run again later,
/// its attempts counted; an older selection changes nothing; failed for
/// good, kept and never due.
#[test]
fn a_failed_run_is_retried_by_its_generation_alone() {
    let Some(url) = database() else { return };
    let scratch = Scratch::new(&url, "retried");
    let tables = open_things(&scratch);
    tables
        .transaction(|tx| tx.schedule(&[scheduling("k", Some(1000))]))
        .assert_ok("scheduled");
    let run = due(&tables, 1000);
    assert_eq!(tables.retry_run(&run, 1, 3000, None), Ok(true));
    let after = due(&tables, 3000);
    assert_eq!((after.id, after.attempts, after.due_ms), (run.id, 1, 3000));
    assert_eq!(
        tables.retry_run(&run, 2, 9000, None),
        Ok(false),
        "an older selection"
    );
    assert_eq!(due(&tables, 3000), after);
    assert_eq!(tables.retry_run(&after, 2, 3000, Some("trapped")), Ok(true));
    assert_eq!(tables.due_runs(i64::MAX, 16), Ok(vec![]));
    assert_eq!(tables.next_run(), Ok(None));
    let failed = tables.runs().assert_ok("the runs");
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].failed.as_deref(), Some("trapped"));
    assert_eq!(tables.run_current(&failed[0], i64::MAX), Ok(false));
}

/// **Sessions are kept by their hash and live by their lifetimes**: opened,
/// inspected and admitted; closed, no one; past a lifetime, no one, and
/// forgotten by an admission but not by an inspection; a sign-in taken once.
#[test]
fn sessions_are_kept_by_their_hash_and_live_by_their_lifetimes() {
    let Some(url) = database() else { return };
    let scratch = Scratch::new(&url, "sessions");
    let tables = open_things(&scratch);
    let ada = session("ada", 1);
    tables.open_session(&hash(1), &ada).assert_ok("opened");
    assert_eq!(
        tables.inspect_session(&hash(1), lifetimes()),
        Ok(Some(ada.clone()))
    );
    assert_eq!(
        tables.admit_session(&hash(1), lifetimes(), Duration::ZERO),
        Ok(Some(ada.clone()))
    );
    assert_eq!(tables.inspect_session(&hash(2), lifetimes()), Ok(None));
    // Opened again, in another generation: the new one.
    let again = session("ada", 2);
    tables
        .open_session(&hash(1), &again)
        .assert_ok("opened again");
    assert_eq!(
        tables.inspect_session(&hash(1), lifetimes()),
        Ok(Some(again))
    );
    tables.close_session(&hash(1)).assert_ok("closed");
    assert_eq!(tables.inspect_session(&hash(1), lifetimes()), Ok(None));
    assert_eq!(
        tables.admit_session(&hash(1), lifetimes(), Duration::ZERO),
        Ok(None)
    );
    // Past a lifetime of a millisecond.
    let bob = session("bob", 3);
    tables.open_session(&hash(3), &bob).assert_ok("opened");
    std::thread::sleep(Duration::from_millis(50));
    let brief = Lifetimes {
        idle: Duration::from_millis(1),
        absolute: DAY,
    };
    assert_eq!(tables.inspect_session(&hash(3), brief), Ok(None));
    assert_eq!(
        tables.inspect_session(&hash(3), lifetimes()),
        Ok(Some(bob.clone())),
        "an inspection forgets nothing"
    );
    assert_eq!(
        tables.admit_session(&hash(3), brief, Duration::ZERO),
        Ok(None)
    );
    assert_eq!(
        tables.inspect_session(&hash(3), lifetimes()),
        Ok(None),
        "an admission forgets a session past its lifetime"
    );
    // A sign-in, taken once.
    tables
        .begin_sign_in(&hash(4), "nonce", "verifier")
        .assert_ok("begun");
    let taken = tables.take_sign_in(&hash(4)).assert_ok("taken");
    assert!(
        matches!(&taken, Some((n, v, age)) if n == "nonce" && v == "verifier" && *age < Duration::from_secs(60)),
        "{taken:?}"
    );
    assert_eq!(tables.take_sign_in(&hash(4)), Ok(None));
}

/// **A fenced command commits only as the session it was admitted as**:
/// admitted as a session still open, it commits; as one closed, or opened
/// again in another generation, it is refused where it is fenced; as no
/// one, still no one, it commits; and a session past its lifetime between
/// the fence and the commit refuses the commit, keeping nothing.
#[test]
fn a_fenced_command_commits_only_as_the_session_it_was_admitted_as() {
    let Some(url) = database() else { return };
    let scratch = Scratch::new(&url, "fenced");
    let tables = open_things(&scratch);
    let ada = session("ada", 1);
    tables.open_session(&hash(1), &ada).assert_ok("opened");
    let mut tx = tables.begin().assert_ok("a command");
    tx.fence(fence(1, Some(ada.clone()), lifetimes()))
        .assert_ok("fenced");
    tx.write(&owner("insert"), &[owner_row("a")])
        .assert_ok("written");
    assert_eq!(tx.revalidate(), Ok(()));
    tx.commit().assert_ok("committed as Ada");
    assert_eq!(owner_of(&tables, "a"), owner_row("a"));
    // Signed out before the command was fenced: refused, nothing written.
    tables.close_session(&hash(1)).assert_ok("closed");
    let mut tx = tables.begin().assert_ok("a command");
    let refused = tx.fence(fence(1, Some(ada.clone()), lifetimes()));
    assert!(matches!(refused, Err(Error::Refused(_))), "{refused:?}");
    assert!(tx.write(&owner("insert"), &[owner_row("b")]).is_err());
    drop(tx);
    assert_eq!(owner_of(&tables, "b"), Value::Null);
    // Signed in again, in another generation: not the session admitted.
    tables
        .open_session(&hash(1), &session("ada", 2))
        .assert_ok("opened again");
    let mut tx = tables.begin().assert_ok("a command");
    let refused = tx.fence(fence(1, Some(ada.clone()), lifetimes()));
    assert!(matches!(refused, Err(Error::Refused(_))), "{refused:?}");
    drop(tx);
    // No one, still no one: an anonymous reader's command commits.
    let mut tx = tables.begin().assert_ok("a command");
    tx.fence(fence(9, None, lifetimes())).assert_ok("no one");
    tx.write(&owner("insert"), &[owner_row("n")])
        .assert_ok("written");
    tx.commit().assert_ok("committed as no one");
    assert_eq!(owner_of(&tables, "n"), owner_row("n"));
    // No one, and someone now: refused.
    tables
        .open_session(&hash(9), &session("nina", 1))
        .assert_ok("opened");
    let mut tx = tables.begin().assert_ok("a command");
    let refused = tx.fence(fence(9, None, lifetimes()));
    assert!(matches!(refused, Err(Error::Refused(_))), "{refused:?}");
    drop(tx);
    // Past its lifetime between the fence and the commit: refused there.
    let brief = Lifetimes {
        idle: Duration::from_millis(1500),
        absolute: Duration::from_millis(1500),
    };
    let cy = session("cy", 1);
    tables.open_session(&hash(5), &cy).assert_ok("opened");
    let mut tx = tables.begin().assert_ok("a command");
    tx.fence(fence(5, Some(cy.clone()), brief))
        .assert_ok("fenced in time");
    tx.write(&owner("insert"), &[owner_row("c")])
        .assert_ok("written");
    std::thread::sleep(Duration::from_millis(1700));
    let late = tx.revalidate();
    assert!(matches!(late, Err(Error::Refused(_))), "{late:?}");
    drop(tx);
    tables.open_session(&hash(5), &cy).assert_ok("opened anew");
    let mut tx = tables.begin().assert_ok("a command");
    tx.fence(fence(5, Some(cy), brief))
        .assert_ok("fenced in time");
    tx.write(&owner("insert"), &[owner_row("c")])
        .assert_ok("written");
    std::thread::sleep(Duration::from_millis(1700));
    let committed = tx.commit();
    assert!(matches!(committed, Err(Error::Refused(_))), "{committed:?}");
    assert_eq!(owner_of(&tables, "c"), Value::Null);
}

/// **A session's change waits for the command fenced on it**, on another
/// host as on this one, and the command commits as the session it was
/// admitted as; once the command has ended, committed or not, its lock is
/// given back and a change waits for nothing.
#[test]
fn a_sessions_change_waits_for_the_command_fenced_on_it() {
    let Some(url) = database() else { return };
    let scratch = Scratch::new(&url, "fence_wait");
    let a = open_things(&scratch);
    let app = format!("pw_fence_{}", std::process::id());
    let b =
        PgTables::open(&named(&url, &app), scratch.schema(), things()).assert_ok("another host");
    let ada = session("ada", 1);
    a.open_session(&hash(1), &ada).assert_ok("opened");
    let mut tx = a.begin().assert_ok("a command");
    tx.fence(fence(1, Some(ada.clone()), lifetimes()))
        .assert_ok("fenced");
    tx.write(&owner("insert"), &[owner_row("a")])
        .assert_ok("written");
    let (closed, after) = std::thread::scope(|scope| {
        let closing = scope.spawn(|| b.close_session(&hash(1)));
        let waited = until(|| waiting_on_advisory(&scratch, &app) == 1);
        let early = closing.is_finished();
        let committed = tx.commit();
        let closed = closing.join();
        assert!(waited, "the sign-out waits for the fenced command");
        assert!(!early, "and does not finish first");
        (closed, committed)
    });
    assert!(after.is_ok(), "the fenced command commits: {after:?}");
    assert!(matches!(closed, Ok(Ok(()))), "{closed:?}");
    assert_eq!(owner_of(&a, "a"), owner_row("a"));
    assert_eq!(a.inspect_session(&hash(1), lifetimes()), Ok(None));
    // Once a fenced command ended, by its commit or its drop, a change on
    // another host waits for nothing: it would wait out the lock otherwise.
    a.open_session(&hash(2), &session("bo", 1))
        .assert_ok("opened");
    let mut tx = a.begin().assert_ok("a command");
    tx.fence(fence(2, Some(session("bo", 1)), lifetimes()))
        .assert_ok("fenced");
    drop(tx);
    let started = std::time::Instant::now();
    assert_eq!(b.close_session(&hash(2)), Ok(()));
    assert!(started.elapsed() < Duration::from_secs(3));
    a.open_session(&hash(3), &session("di", 1))
        .assert_ok("opened");
    let mut tx = a.begin().assert_ok("a command");
    tx.fence(fence(3, Some(session("di", 1)), lifetimes()))
        .assert_ok("fenced");
    tx.commit().assert_ok("committed, writing nothing");
    let started = std::time::Instant::now();
    assert_eq!(b.close_session(&hash(3)), Ok(()));
    assert!(started.elapsed() < Duration::from_secs(3));
}

/// **A command is fenced once, before its first statement**: a fence after
/// a write, or a second fence, is refused.
#[test]
fn a_command_is_fenced_once_before_its_first_statement() {
    let Some(url) = database() else { return };
    let scratch = Scratch::new(&url, "fence_once");
    let tables = open_things(&scratch);
    let ada = session("ada", 1);
    tables.open_session(&hash(1), &ada).assert_ok("opened");
    let mut tx = tables.begin().assert_ok("a command");
    tx.write(&owner("insert"), &[owner_row("a")])
        .assert_ok("written");
    let late = tx.fence(fence(1, Some(ada.clone()), lifetimes()));
    assert!(matches!(late, Err(Error::Input(_))), "{late:?}");
    drop(tx);
    let mut tx = tables.begin().assert_ok("a command");
    tx.fence(fence(1, Some(ada.clone()), lifetimes()))
        .assert_ok("fenced");
    let twice = tx.fence(fence(1, Some(ada), lifetimes()));
    assert!(matches!(twice, Err(Error::Input(_))), "{twice:?}");
}
