//! ActualPG scheduler regressions. Feature-gated at the module boundary;
//! a selected test requires a database and never reports a passing no-DB skip.
use super::*;
use crate::feed_pg::Isolation;
use crate::store_pg::StorePg;
fn database() -> Option<String> {
    std::env::var("PW_STORE_DATABASE_URL")
        .ok()
        .filter(|s| !s.is_empty())
}

/// A compiled Clock command commits before cleanup fails. Fresh hosts at
/// 23/25 logical hours must not add the non-idempotent cart line again.
/// The trigger injects only cleanup failure, in this test's disposable schema.
#[test]
fn on_postgres_a_pending_schedule_is_not_repeated_after_receipt_expiry() {
    let url = database().expect("this reproduction requires PostgreSQL");
    let layer = Arc::new(StorePg::for_test(&url).expect("the isolated layer"));
    let (dir, out) =
        built_from_patches_in("examples", super::store_scheduled::with_scheduling, &[]);
    let host = || {
        let fresh = Arc::new(
            StorePg::open_with(
                &url,
                layer.schema(),
                Isolation::Serializable,
                crate::feed_pg::RenderSeats::default(),
            )
            .expect("a fresh host's layer"),
        );
        Server::from_build_layers(out.clone(), out.clone(), None, Some(fresh))
            .expect("a fresh host from the compiled program")
    };
    println!("REPLAY_SCHEMA {}", layer.schema());
    let a = host();
    assert!(super::store_scheduled::in_five(&a, "espresso", false, "replay-schedule").committed);
    layer
        .sql()
        .batch_execute(
            "CREATE FUNCTION refuse_schedule_cleanup() RETURNS trigger LANGUAGE plpgsql AS $$
         BEGIN RAISE EXCEPTION 'isolated cleanup fault after action commit'; END $$;
         CREATE TRIGGER refuse_schedule_cleanup BEFORE DELETE ON schedules
         FOR EACH ROW EXECUTE FUNCTION refuse_schedule_cleanup();",
        )
        .expect("the cleanup fault installed in the disposable schema");
    let original = layer
        .sql()
        .query_one(
            "SELECT id, key, command, args, user_id, due_ms FROM schedules",
            &[],
        )
        .expect("the exact scheduled generation");
    let original_id: i64 = original.get(0);
    let original_key: String = original.get(1);
    let original_args: String = original.get(3);
    let original_user: String = original.get(4);
    println!(
        "REPLAY_JOB id={original_id}; key={original_key:?}; args={original_args}; user={original_user:?}"
    );
    let receipt_key = std::cell::RefCell::new(None::<String>);
    const HOUR: u64 = 60 * 60 * 1000;
    const FIVE: u64 = 5 * 60 * 1000;
    let snapshot = |label: &str, s: &Server| {
        let row = layer
            .sql()
            .query_one(
                "SELECT (SELECT count(*) FROM schedules),
                    (SELECT count(*) FROM interactions WHERE key LIKE '%schedule-%'),
                    (SELECT count(*) FROM outbox WHERE kind = 'event')",
                &[],
            )
            .expect("the committed records inspected");
        let pending: i64 = row.get(0);
        let receipts: i64 = row.get(1);
        let events: i64 = row.get(2);
        let job = layer
            .sql()
            .query_one("SELECT id, key, args, user_id FROM schedules", &[])
            .expect("the pending generation");
        assert_eq!(job.get::<_, i64>(0), original_id);
        assert_eq!(job.get::<_, String>(1), original_key);
        assert_eq!(job.get::<_, String>(2), original_args);
        assert_eq!(job.get::<_, String>(3), original_user);
        let receipt = layer
            .sql()
            .query_one(
                "SELECT key, sent, answer, at_ms FROM interactions WHERE key LIKE '%schedule-%'",
                &[],
            )
            .expect("the exact committed job receipt");
        let key: String = receipt.get(0);
        if let Some(first) = &*receipt_key.borrow() {
            assert_eq!(&key, first);
        }
        *receipt_key.borrow_mut() = Some(key.clone());
        let sent: String = receipt.get(1);
        let answer: String = receipt.get(2);
        let at_ms: i64 = receipt.get(3);
        assert_eq!(sent, original_args);
        println!(
            "REPLAY_RECEIPT {label}: key={key:?}; at_ms={at_ms}; sent={sent}; answer={answer}"
        );
        let lines = s.cart_lines("a");
        println!(
            "REPLAY_SNAPSHOT {label}: lines={lines:?}; schedules={pending}; job_receipts={receipts}; events={events}"
        );
        assert_eq!(
            pending, 1,
            "cleanup fault leaves the original scheduling pending"
        );
        assert_eq!(receipts, 1, "the action's receipt exists");
        (lines, events)
    };
    a.query_clock.0.advance(FIVE);
    assert_eq!(
        a.run_due().0,
        1,
        "the real scheduler dispatches the compiled Clock command"
    );
    let first = snapshot("committed-before-cleanup", &a);
    assert_eq!(first.0, [("espresso".to_string(), 1)]);
    assert_eq!(first.1, 1);
    drop(a);
    let b = host();
    b.query_clock.0.advance(FIVE + 23 * HOUR);
    assert!(
        b.recalled_interaction(receipt_key.borrow().as_deref().expect("the key"))
            .expect("recall at23h")
            .is_some()
    );
    assert_eq!(b.run_due().0, 1);
    assert_eq!(
        snapshot("fresh-host-at-23h", &b),
        first,
        "within-window control"
    );
    drop(b);
    let c = host();
    c.query_clock.0.advance(FIVE + 25 * HOUR);
    assert!(
        c.recalled_interaction(receipt_key.borrow().as_deref().expect("the key"))
            .expect("recall at25h")
            .is_none()
    );
    assert_eq!(c.run_due().0, 1);
    let delayed = snapshot("fresh-host-at-25h", &c);
    assert_eq!(
        delayed, first,
        "a pending durable job must not outlive its completion protection"
    );
    drop(c);
    drop(dir);
}

/// A second host already fetched the run before A committed and consumed it.
/// Normal cleanup succeeds; resuming the fetched generation after the client
/// receipt horizon must still be fenced. No SQL fault is installed here.
#[test]
fn on_postgres_a_stale_fetched_schedule_cannot_repeat_a_completed_action() {
    let url = database().expect("this reproduction requires PostgreSQL");
    let layer = Arc::new(StorePg::for_test(&url).expect("the isolated layer"));
    println!("STALE_REPLAY_SCHEMA {}", layer.schema());
    let (_dir, out) =
        built_from_patches_in("examples", super::store_scheduled::with_scheduling, &[]);
    let host = || {
        let fresh = Arc::new(
            StorePg::open_with(
                &url,
                layer.schema(),
                Isolation::Serializable,
                crate::feed_pg::RenderSeats::default(),
            )
            .expect("an independent host's layer"),
        );
        Server::from_build_layers(out.clone(), out.clone(), None, Some(fresh))
            .expect("served from the compiled program")
    };
    const FIVE: u64 = 5 * 60 * 1000;
    const HOUR: u64 = 60 * 60 * 1000;
    let a = host();
    assert!(super::store_scheduled::in_five(&a, "espresso", false, "stale-schedule").committed);
    let b = host();
    let c = host();
    a.query_clock.0.advance(FIVE);
    b.query_clock.0.advance(FIVE);
    c.query_clock.0.advance(FIVE);
    let before = b.runs().due_runs(b.now(), 16).expect("B selects due work");
    let c_before = c.runs().due_runs(c.now(), 16).expect("C selects due work");
    assert_eq!(before.len(), 1);
    assert_eq!(c_before.len(), 1);
    assert_eq!(before[0].id, c_before[0].id);
    assert_eq!(before[0].command, c_before[0].command);
    assert_eq!(before[0].args, c_before[0].args);
    assert_eq!(
        a.run_due().0,
        1,
        "A dispatches and normally consumes the run"
    );
    assert_eq!(
        layer
            .sql()
            .query_one("SELECT count(*) FROM schedules", &[])
            .expect("queue")
            .get::<_, i64>(0),
        0
    );
    assert_eq!(host().cart_lines("a"), [("espresso".to_string(), 1)]);
    b.query_clock.0.advance(23 * HOUR);
    b.run_scheduled(&before[0]);
    assert_eq!(
        host().cart_lines("a"),
        [("espresso".to_string(), 1)],
        "within-window stale snapshot control"
    );
    c.query_clock.0.advance(25 * HOUR);
    c.run_scheduled(&c_before[0]);
    let committed = host().cart_lines("a");
    let row = layer
        .sql()
        .query_one(
            "SELECT (SELECT count(*) FROM schedules),
                (SELECT count(*) FROM interactions WHERE key LIKE '%schedule-%'),
                (SELECT count(*) FROM outbox WHERE kind = 'event')",
            &[],
        )
        .expect("fresh committed state");
    let pending: i64 = row.get(0);
    let receipts: i64 = row.get(1);
    let events: i64 = row.get(2);
    println!(
        "STALE_REPLAY_SNAPSHOT id={}; lines={committed:?}; schedules={pending}; job_receipts={receipts}; events={events}",
        before[0].id
    );
    assert_eq!(pending, 0, "normal cleanup succeeds");
    assert_eq!(receipts, 1);
    assert_eq!(
        (committed, events),
        (vec![("espresso".to_string(), 1)], 1),
        "an already consumed job's stale generation must not act again"
    );
}

/// A known pre-commit refusal is retryable after the missing grant is restored.
/// The durable scheduler must not replay a client-cache refusal forever.
#[test]
fn on_postgres_a_scheduled_refusal_can_recover_on_the_same_host() {
    let url = database().expect("this reproduction requires PostgreSQL");
    let layer = Arc::new(StorePg::for_test(&url).expect("the isolated layer"));
    println!("RETRY_CACHE_SCHEMA {}", layer.schema());
    let (_dir, out) =
        built_from_patches_in("examples", super::store_scheduled::with_scheduling, &[]);
    let mut s = Server::from_build_layers(out.clone(), out, None, Some(layer.clone()))
        .expect("served from the compiled program");
    assert!(super::store_scheduled::in_five(&s, "espresso", false, "retry-schedule").committed);
    let original_grants = s.topology.nodes[0].grants.clone();
    s.topology.nodes[0].grants.retain(|g| g != "outbox.write");
    s.query_clock.0.advance(5 * 60 * 1000);
    assert_eq!(s.run_due().0, 1);
    assert!(
        s.cart_lines("a").is_empty(),
        "first refusal commits no business effect"
    );
    let first = s.runs().all_runs().expect("pending after refusal");
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].attempts, 1);
    s.topology.nodes[0].grants = original_grants;
    s.query_clock.0.advance(2000);
    assert_eq!(s.run_due().0, 1, "retry when its backoff ends");
    let pending = s.runs().all_runs().expect("pending after retry");
    let committed = s.cart_lines("a");
    println!("RETRY_CACHE_SNAPSHOT lines={committed:?}; pending={pending:?}");
    assert_eq!(
        committed,
        [("espresso".to_string(), 1)],
        "a restored known-refused job must execute rather than replay its cached refusal"
    );
    assert!(pending.is_empty());
}
