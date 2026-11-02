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
    assert_eq!(
        scratch.texts("SELECT count(*)::text FROM pw_sign_ins"),
        ["0"],
        "a sign-in's nonce and verifier are forgotten as it is taken"
    );
}

/// **An admission never waits for a command holding its session**
/// (ADR-0389, amended 2026-11-02; the integrator's ruling for this layer):
/// it reads the session in one statement that takes no session lock. A
/// write it finds due (its use recorded, or, past a lifetime, its row
/// forgotten) is made only where nobody holds the session's lock: under a
/// fenced command's lock it is left, the read answering (a session past its
/// lifetime no one all the same), and made by the first admission once the
/// command has ended.
#[test]
fn an_admission_never_waits_for_a_command_holding_its_session() {
    let Some(url) = database() else { return };
    let scratch = Scratch::new(&url, "admitted");
    let a = open_things(&scratch);
    let b = open_things(&scratch);
    let seen = |n: u8| {
        scratch
            .texts(&format!(
                "SELECT (extract(epoch FROM seen) * 1000000)::bigint::text FROM pw_sessions \
                 WHERE get_byte(hash, 0) = {n}"
            ))
            .join(",")
    };
    let ada = session("ada", 1);
    a.open_session(&hash(1), &ada).assert_ok("opened");
    let held = b
        .begin_fenced(fence(1, Some(ada.clone()), lifetimes()))
        .assert_ok("another host's command, fenced on Ada's session");
    let before = seen(1);
    let started = std::time::Instant::now();
    let admitted = a.admit_session(&hash(1), lifetimes(), Duration::ZERO);
    let elapsed = started.elapsed();
    assert!(
        elapsed < Duration::from_secs(2) && admitted == Ok(Some(ada.clone())),
        "admitted from its read at once: {admitted:?} in {elapsed:?}"
    );
    assert_eq!(
        seen(1),
        before,
        "its use, due, left under the command's lock"
    );
    // Past its lifetime under the lock: no one, its row left.
    let brief = Lifetimes {
        idle: Duration::from_millis(1),
        absolute: DAY,
    };
    std::thread::sleep(Duration::from_millis(20));
    let started = std::time::Instant::now();
    assert_eq!(a.admit_session(&hash(1), brief, Duration::ZERO), Ok(None));
    assert!(started.elapsed() < Duration::from_secs(2));
    assert_eq!(
        a.inspect_session(&hash(1), lifetimes()),
        Ok(Some(ada.clone())),
        "its row left under the lock"
    );
    // The command ended: the first admission makes what was due.
    drop(held);
    assert_eq!(
        a.admit_session(&hash(1), lifetimes(), Duration::ZERO),
        Ok(Some(ada.clone()))
    );
    assert_ne!(seen(1), before, "its use recorded once the lock is free");
    assert_eq!(a.admit_session(&hash(1), brief, Duration::ZERO), Ok(None));
    assert_eq!(
        a.inspect_session(&hash(1), lifetimes()),
        Ok(None),
        "and its row forgotten past its lifetime"
    );
}

/// **A fenced command commits only as the session it was admitted as**:
/// admitted as a session still open, it commits; as one closed, or opened
/// again in another generation, it is refused where it begins; as no one,
/// still no one, it commits; and a session past its lifetime between the
/// fence and the commit refuses the commit, keeping nothing. A refused
/// fence gives its lock back.
#[test]
fn a_fenced_command_commits_only_as_the_session_it_was_admitted_as() {
    let Some(url) = database() else { return };
    let scratch = Scratch::new(&url, "fenced");
    let tables = open_things(&scratch);
    let ada = session("ada", 1);
    tables.open_session(&hash(1), &ada).assert_ok("opened");
    let mut tx = tables
        .begin_fenced(fence(1, Some(ada.clone()), lifetimes()))
        .assert_ok("fenced");
    tx.write(&owner("insert"), &[owner_row("a")])
        .assert_ok("written");
    assert_eq!(tx.revalidate(), Ok(()));
    tx.commit().assert_ok("committed as Ada");
    assert_eq!(owner_of(&tables, "a"), owner_row("a"));
    // Signed out before the command began: refused, nothing written, and
    // its lock given back (a change of the session waits for nothing).
    tables.close_session(&hash(1)).assert_ok("closed");
    let refused = tables.begin_fenced(fence(1, Some(ada.clone()), lifetimes()));
    assert!(
        matches!(&refused, Err(Error::Refused(_))),
        "{:?}",
        refused.as_ref().err()
    );
    drop(refused);
    let started = std::time::Instant::now();
    tables
        .open_session(&hash(1), &session("ada", 2))
        .assert_ok("opened again");
    assert!(started.elapsed() < Duration::from_secs(3));
    // Signed in again, in another generation: not the session admitted.
    let refused = tables.begin_fenced(fence(1, Some(ada.clone()), lifetimes()));
    assert!(
        matches!(&refused, Err(Error::Refused(_))),
        "{:?}",
        refused.as_ref().err()
    );
    drop(refused);
    // No one, still no one: an anonymous reader's command commits.
    let mut tx = tables
        .begin_fenced(fence(9, None, lifetimes()))
        .assert_ok("no one");
    tx.write(&owner("insert"), &[owner_row("n")])
        .assert_ok("written");
    tx.commit().assert_ok("committed as no one");
    assert_eq!(owner_of(&tables, "n"), owner_row("n"));
    // No one, and someone now: refused.
    tables
        .open_session(&hash(9), &session("nina", 1))
        .assert_ok("opened");
    let refused = tables.begin_fenced(fence(9, None, lifetimes()));
    assert!(
        matches!(&refused, Err(Error::Refused(_))),
        "{:?}",
        refused.as_ref().err()
    );
    drop(refused);
    // Past its lifetime between the fence and the commit: refused there.
    let brief = Lifetimes {
        idle: Duration::from_millis(1500),
        absolute: Duration::from_millis(1500),
    };
    let cy = session("cy", 1);
    tables.open_session(&hash(5), &cy).assert_ok("opened");
    let mut tx = tables
        .begin_fenced(fence(5, Some(cy.clone()), brief))
        .assert_ok("fenced in time");
    tx.write(&owner("insert"), &[owner_row("c")])
        .assert_ok("written");
    std::thread::sleep(Duration::from_millis(1700));
    let late = tx.revalidate();
    assert!(matches!(late, Err(Error::Refused(_))), "{late:?}");
    drop(tx);
    tables.open_session(&hash(5), &cy).assert_ok("opened anew");
    let mut tx = tables
        .begin_fenced(fence(5, Some(cy), brief))
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
    let mut tx = a
        .begin_fenced(fence(1, Some(ada.clone()), lifetimes()))
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
    let tx = a
        .begin_fenced(fence(2, Some(session("bo", 1)), lifetimes()))
        .assert_ok("fenced");
    drop(tx);
    let started = std::time::Instant::now();
    assert_eq!(b.close_session(&hash(2)), Ok(()));
    assert!(started.elapsed() < Duration::from_secs(3));
    a.open_session(&hash(3), &session("di", 1))
        .assert_ok("opened");
    let tx = a
        .begin_fenced(fence(3, Some(session("di", 1)), lifetimes()))
        .assert_ok("fenced");
    tx.commit().assert_ok("committed, writing nothing");
    let started = std::time::Instant::now();
    assert_eq!(b.close_session(&hash(3)), Ok(()));
    assert!(started.elapsed() < Duration::from_secs(3));
}

/// **A command waiting for its session holds no other command of its host
/// up**: the session's lock is taken before the host's turn, so while one
/// command waits for a session another host's command holds, the host's
/// other commands begin and commit at once.
#[test]
fn a_command_waiting_for_its_session_holds_no_other_command_up() {
    let Some(url) = database() else { return };
    let scratch = Scratch::new(&url, "fence_turn");
    let a = open_things(&scratch);
    let app = format!("pw_turn_{}", std::process::id());
    let b =
        PgTables::open(&named(&url, &app), scratch.schema(), things()).assert_ok("another host");
    let ada = session("ada", 1);
    a.open_session(&hash(1), &ada).assert_ok("opened");
    let held = a
        .begin_fenced(fence(1, Some(ada.clone()), lifetimes()))
        .assert_ok("another host's command, fenced on Ada's session");
    let (waited, other, elapsed, still, late) = std::thread::scope(|scope| {
        let waiting = scope.spawn(|| {
            b.begin_fenced(fence(1, Some(ada.clone()), lifetimes()))
                .and_then(|tx| tx.commit().map(|_| ()))
        });
        let waited = until(|| waiting_on_advisory(&scratch, &app) == 1);
        let started = std::time::Instant::now();
        let other = b
            .transaction(|tx| tx.write(&owner("insert"), &[owner_row("o")]))
            .map(|_| ());
        let elapsed = started.elapsed();
        // The waiting command waits until the lock is given back, below;
        // one holding the turn through its wait let the other run only once
        // its own wait had failed.
        let still = !waiting.is_finished();
        drop(held);
        (waited, other, elapsed, still, waiting.join())
    });
    assert!(waited, "a command waits for the session another holds");
    assert_eq!(other, Ok(()), "the host's other command commits");
    assert!(
        still && elapsed < Duration::from_secs(4),
        "and is not held up by the waiting one: {elapsed:?}, which still waits: {still}"
    );
    assert!(matches!(late, Ok(Ok(()))), "{late:?}");
    assert_eq!(owner_of(&a, "o"), owner_row("o"));
}

/// **A command holding the host's turn never waits for a connection**: each
/// command takes its place among the pool's connections before its turn,
/// so the commands waiting for the turn or for a place (a fenced command's
/// holds its session's lock on its place) never leave the one holding the
/// turn without a connection; and each of them commits after it.
#[test]
fn a_command_holding_the_turn_never_waits_for_a_connection() {
    let Some(url) = database() else { return };
    let scratch = Scratch::new(&url, "places");
    let options = pw_table_postgres::Options {
        connections: 2,
        wait: Duration::from_secs(5),
        ..pw_table_postgres::Options::default()
    };
    let tables = std::sync::Arc::new(
        PgTables::open_with(&url, scratch.schema(), things(), options).assert_ok("two connections"),
    );
    for n in [2, 3] {
        tables
            .open_session(&hash(n), &session(&format!("u{n}"), n))
            .assert_ok("opened");
    }
    let (told, waits) = std::sync::mpsc::channel();
    let told = std::sync::Mutex::new(told);
    tables.tell_on_wait(move || {
        let _ = told.lock().map(|t| t.send(()));
    });
    let mut first = tables.begin().assert_ok("the command holding the turn");
    let waiting: Vec<_> = [2u8, 3]
        .into_iter()
        .map(|n| {
            let tables = std::sync::Arc::clone(&tables);
            std::thread::spawn(move || {
                let expected = Some(session(&format!("u{n}"), n));
                let mut tx = tables.begin_fenced(fence(n, expected, lifetimes()))?;
                tx.write(&owner("insert"), &[owner_row(&format!("w{n}"))])?;
                tx.commit().map(|_| ())
            })
        })
        .collect();
    // One waits for the turn, holding the pool's other place; the other
    // waits for a place (had the command holding the turn none, both would
    // hold one, and it none).
    let one = waits.recv_timeout(Duration::from_secs(10)).is_ok();
    std::thread::sleep(Duration::from_millis(300));
    let started = std::time::Instant::now();
    let wrote = first.write(&owner("insert"), &[owner_row("t")]);
    let committed = first.commit().map(|_| ());
    let elapsed = started.elapsed();
    let joined: Vec<_> = waiting.into_iter().map(|w| w.join()).collect();
    assert!(one, "a fenced command waits for the turn, holding a place");
    assert_eq!(
        (wrote, committed),
        (Ok(()), Ok(())),
        "the command holding the turn runs"
    );
    assert!(
        elapsed < Duration::from_secs(3),
        "without waiting for a connection: {elapsed:?}"
    );
    assert!(joined.iter().all(|j| matches!(j, Ok(Ok(())))), "{joined:?}");
    assert_eq!(owner_of(&tables, "w3"), owner_row("w3"));
}

/// **A connection a session's change did not commit on is closed, not
/// kept** (as the other PostgreSQL layers' changes are): it is marked so
/// before its transaction begins, so a change the database refuses (a hash
/// of 31 bytes, which the table's check refuses) leaves behind no
/// connection whose transaction or lock is not known; one that commits
/// keeps its connection for the next.
#[test]
fn a_connection_whose_session_change_failed_is_closed() {
    let Some(url) = database() else { return };
    let scratch = Scratch::new(&url, "change_failed");
    let app = format!("pw_change_{}", std::process::id());
    let tables = PgTables::open(&named(&url, &app), scratch.schema(), things()).assert_ok("open");
    let open = || {
        scratch
            .texts(&format!(
                "SELECT count(*)::text FROM pg_stat_activity WHERE application_name = '{app}'"
            ))
            .join(",")
    };
    tables
        .open_session(&hash(1), &session("ada", 1))
        .assert_ok("opened");
    let before = open();
    let refused = tables.open_session(&[7u8; 31], &session("cy", 1));
    assert!(matches!(refused, Err(Error::Database(_))), "{refused:?}");
    let closed =
        until(|| open().parse::<usize>().ok() == before.parse::<usize>().ok().map(|n| n - 1));
    assert!(closed, "its connection closed: {before} then {}", open());
    // The neighbour: changes that commit keep their connection.
    tables
        .open_session(&hash(2), &session("bo", 1))
        .assert_ok("opened");
    let kept = open();
    tables
        .open_session(&hash(3), &session("di", 1))
        .assert_ok("opened");
    tables.close_session(&hash(3)).assert_ok("closed");
    assert_eq!(open(), kept, "a committed change's connection is kept");
}

/// **A run whose deletion is suppressed is refused, not taken for a run
/// done**: its row is the run selected, and the deletion another hand's
/// trigger answers `NULL` removes nothing; the taking is a refusal, the
/// command's transaction fails, and the run is kept.
#[test]
fn a_run_whose_deletion_is_suppressed_is_refused() {
    let Some(url) = database() else { return };
    let scratch = Scratch::new(&url, "suppressed");
    let tables = open_things(&scratch);
    tables
        .transaction(|tx| tx.schedule(&[scheduling("k", Some(1000))]))
        .assert_ok("scheduled");
    let run = due(&tables, 1000);
    scratch
        .sql(
            "CREATE FUNCTION keep() RETURNS trigger LANGUAGE plpgsql AS \
             $$ BEGIN RETURN NULL; END $$; \
             CREATE TRIGGER keep BEFORE DELETE ON pw_runs FOR EACH ROW EXECUTE FUNCTION keep();",
        )
        .assert_ok("a trigger suppressing every deletion of a run");
    let mut tx = tables.begin().assert_ok("a command");
    let taken = tx.consume(&run, 1000);
    assert!(matches!(taken, Err(Error::Refused(_))), "{taken:?}");
    assert!(
        tx.write(&owner("insert"), &[owner_row("x")]).is_err(),
        "the command's transaction fails with it"
    );
    drop(tx);
    assert_eq!(tables.run_current(&run, 1000), Ok(true), "the run is kept");
    assert_eq!(owner_of(&tables, "x"), Value::Null);
    // The neighbour: another generation is not current, and no refusal.
    let mut tx = tables.begin().assert_ok("a command");
    let stale = Run {
        attempts: run.attempts + 1,
        ..run.clone()
    };
    assert_eq!(tx.consume(&stale, 1000), Ok(false));
}

/// **The sessions past their lifetimes and the sign-ins past theirs are
/// swept, a bounded batch at a time**, those alive kept, and a session a
/// command holds left for the next sweep.
#[test]
fn sessions_and_sign_ins_past_their_lifetimes_are_swept() {
    let Some(url) = database() else { return };
    let scratch = Scratch::new(&url, "swept");
    let tables = open_things(&scratch);
    for n in 1..=4 {
        tables
            .open_session(&hash(n), &session(&format!("u{n}"), n))
            .assert_ok("opened");
    }
    // Three of them past their idle time; the fourth alive.
    scratch
        .sql(
            "UPDATE pw_sessions SET seen = seen - interval '3 days' \
             WHERE get_byte(hash, 0) IN (1, 2, 3)",
        )
        .assert_ok("aged");
    let brief = Lifetimes {
        idle: DAY,
        absolute: DAY * 30,
    };
    assert_eq!(tables.sweep_sessions(brief, 2), Ok(2), "two at most");
    assert_eq!(tables.sweep_sessions(brief, 10), Ok(1), "then the third");
    assert_eq!(tables.sweep_sessions(brief, 10), Ok(0));
    assert_eq!(
        tables.inspect_session(&hash(4), brief),
        Ok(Some(session("u4", 4))),
        "the live session kept"
    );
    // Sign-ins: one begun eleven minutes ago, one now.
    tables
        .begin_sign_in(&hash(5), "old-nonce", "old-verifier")
        .assert_ok("begun");
    scratch
        .sql("UPDATE pw_sign_ins SET started = started - interval '11 minutes'")
        .assert_ok("aged");
    tables
        .begin_sign_in(&hash(6), "new-nonce", "new-verifier")
        .assert_ok("begun");
    // The second sign-in's own statement forgets the first; aged again, the
    // sweep forgets what remains past its ten minutes.
    tables
        .begin_sign_in(&hash(7), "late-nonce", "late-verifier")
        .assert_ok("begun");
    scratch
        .sql(
            "UPDATE pw_sign_ins SET started = started - interval '11 minutes' \
             WHERE get_byte(hash, 0) = 7",
        )
        .assert_ok("aged");
    assert_eq!(tables.sweep_sign_ins(Duration::from_secs(600), 10), Ok(1));
    assert_eq!(
        scratch.texts("SELECT nonce FROM pw_sign_ins"),
        ["new-nonce"],
        "only the sign-in under way keeps its nonce and verifier"
    );
}

/// **A session a fenced command holds is not swept from under it**: its row
/// is locked by the command, and the sweep passes it by, waiting for nothing.
#[test]
fn a_session_a_command_holds_is_left_for_the_next_sweep() {
    let Some(url) = database() else { return };
    let scratch = Scratch::new(&url, "swept_held");
    let tables = open_things(&scratch);
    let ada = session("ada", 1);
    tables.open_session(&hash(1), &ada).assert_ok("opened");
    let tx = tables
        .begin_fenced(fence(1, Some(ada.clone()), lifetimes()))
        .assert_ok("fenced");
    let brief = Lifetimes {
        idle: Duration::from_secs(1),
        absolute: DAY,
    };
    std::thread::sleep(Duration::from_millis(1200));
    let started = std::time::Instant::now();
    assert_eq!(tables.sweep_sessions(brief, 10), Ok(0), "held, passed by");
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "without waiting"
    );
    drop(tx);
    assert_eq!(tables.sweep_sessions(brief, 10), Ok(1), "then swept");
}

/// **The outbox's delivered rows are forgotten**: those named, and no
/// other; those a host left behind, once older than a given age.
#[test]
fn the_outboxs_delivered_rows_are_forgotten() {
    let Some(url) = database() else { return };
    let scratch = Scratch::new(&url, "outbox");
    let tables = open_things(&scratch);
    let event = |name: &str| Outboxed {
        kind: OutboxKind::Event,
        name: name.into(),
        values: vec![],
    };
    let mut ids = Vec::new();
    for name in ["a", "b", "c"] {
        let mut tx = tables.begin().assert_ok("a command");
        tx.write(&owner("insert"), &[owner_row(name)])
            .assert_ok("written");
        let committed = tx
            .prepare()
            .assert_ok("prepared")
            .publish_with(Records {
                interaction: None,
                outbox: &[event(name)],
            })
            .assert_ok("committed");
        ids.extend(committed.outbox.iter().map(|(id, _)| *id));
    }
    assert_eq!(ids.len(), 3);
    assert_eq!(tables.forget_outbox(&ids[..2]), Ok(2));
    assert_eq!(scratch.texts("SELECT name FROM pw_outbox"), ["c"]);
    assert_eq!(tables.forget_outbox(&[]), Ok(0));
    // Left behind: kept while young, forgotten once older than the age.
    assert_eq!(tables.trim_outbox(Duration::from_secs(60), 10), Ok(0));
    scratch
        .sql("UPDATE pw_outbox SET created = created - interval '2 minutes'")
        .assert_ok("aged");
    assert_eq!(tables.trim_outbox(Duration::from_secs(60), 10), Ok(1));
    assert_eq!(scratch.texts("SELECT count(*)::text FROM pw_outbox"), ["0"]);
}

/// **A render's reads end at a read that fails**: the snapshot's
/// transaction ended with it, and a later read of the render fails rather
/// than open another transaction, which would read a later state.
#[test]
fn a_renders_reads_end_at_a_read_that_fails() {
    let Some(url) = database() else { return };
    let scratch = Scratch::new(&url, "render_fails");
    let tables = open_things(&scratch);
    tables
        .transaction(|tx| tx.write(&owner("insert"), &[owner_row("a")]))
        .assert_ok("a");
    let snapshot = tables.snapshot().assert_ok("a seat");
    assert_eq!(
        snapshot.read(&owner("all"), &[]),
        Ok(json!([owner_row("a")]))
    );
    tables
        .transaction(|tx| tx.write(&owner("insert"), &[owner_row("b")]))
        .assert_ok("b, committed during the render");
    assert!(
        snapshot.read(&owner("get"), &[json!(42)]).is_err(),
        "a read given what its function does not take fails"
    );
    let after = snapshot.read(&owner("all"), &[]);
    assert!(
        after.is_err(),
        "a later read of the render fails, and reads no other state: {after:?}"
    );
    // The neighbour: a snapshot of its own reads what is committed now.
    let fresh = tables.snapshot().assert_ok("a seat");
    assert_eq!(
        fresh.read(&owner("all"), &[]),
        Ok(json!([owner_row("a"), owner_row("b")]))
    );
}
