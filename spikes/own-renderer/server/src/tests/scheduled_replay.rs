//! ActualPG scheduler regressions. Feature-gated at the module boundary;
//! a selected test requires a database and never reports a passing no-DB skip.
use super::scheduled_probe::probe;
use super::*;
use crate::feed_pg::Isolation;
use crate::store_pg::StorePg;
fn database() -> Option<String> {
    std::env::var("PW_STORE_DATABASE_URL")
        .ok()
        .filter(|s| !s.is_empty())
}

/// A fresh namespace is created without IF NOT EXISTS; never adopt old work.
struct SchedulerSchema {
    url: String,
    schema: String,
}
impl SchedulerSchema {
    fn new(url: &str, kind: &str) -> Self {
        let mut bytes = [0u8; 8];
        getrandom::fill(&mut bytes).expect("test namespace entropy");
        let schema = format!(
            "pw_scheduler_{kind}_{}",
            bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
        );
        postgres::Client::connect(url, postgres::NoTls)
            .expect("schema admin connection")
            .batch_execute(&format!("CREATE SCHEMA {schema}"))
            .expect("fresh schema, never replacement");
        let owned = Self {
            url: url.to_string(),
            schema,
        };
        if let Ok(path) = std::env::var("PW_SCHEDULER_SCHEMA_RECEIPT") {
            use std::io::Write;
            let mut file = std::fs::OpenOptions::new()
                .append(true)
                .open(path)
                .expect("producer-owned cleanup receipt");
            writeln!(file, "{}", owned.schema)
                .expect("schema ownership recorded for cleanup verification");
        }
        println!("SCHEDULER_SCHEMA {}", owned.schema);
        owned
    }
}
impl Drop for SchedulerSchema {
    fn drop(&mut self) {
        if let Ok(mut c) = postgres::Client::connect(&self.url, postgres::NoTls) {
            let _ = c.batch_execute(&format!(
                "SET lock_timeout = '5s'; DROP SCHEMA {} CASCADE",
                self.schema
            ));
        }
    }
}
fn fresh_store(url: &str) -> (SchedulerSchema, Arc<StorePg>) {
    let schema = SchedulerSchema::new(url, "store");
    let layer = Arc::new(
        StorePg::open_with(
            url,
            &schema.schema,
            Isolation::Serializable,
            crate::feed_pg::RenderSeats::default(),
        )
        .expect("isolated store adapter"),
    );
    (schema, layer)
}

/// The completion DELETE is part of the action transaction. Refusal retains
/// neither business rows, receipt nor event; a fresh host can later succeed.
#[test]
fn on_postgres_a_refused_completion_rolls_back_and_late_retry_commits_once() {
    let url = database().expect("this regression requires PostgreSQL");
    let (_schema, layer) = fresh_store(&url);
    println!("ATOMIC_COMPLETION_SCHEMA {}", layer.schema());
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
            .expect("a fresh host's layer"),
        );
        Server::from_build_layers(out.clone(), out.clone(), None, Some(fresh))
            .expect("the compiled program served")
    };
    let a = host();
    assert!(super::store_scheduled::in_five(&a, "espresso", false, "atomic-schedule").committed);
    layer.sql().batch_execute("CREATE FUNCTION refuse_schedule_completion() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'isolated completion fault'; END $$; CREATE TRIGGER refuse_schedule_completion BEFORE DELETE ON schedules FOR EACH ROW EXECUTE FUNCTION refuse_schedule_completion();").expect("the isolated fault installed");
    a.query_clock.0.advance(5 * 60 * 1000);
    assert_eq!(a.run_due().0, 1);
    assert!(
        host().cart_lines("a").is_empty(),
        "business writes rolled back"
    );
    let row = layer.sql().query_one("SELECT (SELECT count(*) FROM schedules), (SELECT count(*) FROM interactions WHERE key LIKE '%schedule-%'), (SELECT count(*) FROM outbox WHERE kind = 'event')", &[]).expect("committed state inspected");
    assert_eq!(row.get::<_, i64>(0), 1);
    assert_eq!(
        row.get::<_, i64>(1),
        0,
        "no result receipt survived refusal"
    );
    assert_eq!(row.get::<_, i64>(2), 0, "no event survived refusal");
    let waiting = a.runs().all_runs().expect("the retry row");
    assert_eq!(waiting[0].attempts, 1);
    assert!(waiting[0].failed.is_none());
    layer
        .sql()
        .batch_execute("DROP TRIGGER refuse_schedule_completion ON schedules")
        .expect("fault removed");
    drop(a);
    let b = host();
    b.query_clock.0.advance(5 * 60 * 1000 + 25 * 60 * 60 * 1000);
    assert_eq!(
        b.run_due().0,
        1,
        "new work first completed beyond the client receipt horizon"
    );
    assert_eq!(host().cart_lines("a"), [("espresso".to_string(), 1)]);
    let row = layer.sql().query_one("SELECT (SELECT count(*) FROM schedules), (SELECT count(*) FROM interactions WHERE key LIKE '%schedule-%'), (SELECT count(*) FROM outbox WHERE kind = 'event')", &[]).expect("completed state inspected");
    assert_eq!(row.get::<_, i64>(0), 0);
    assert_eq!(row.get::<_, i64>(1), 1);
    assert_eq!(row.get::<_, i64>(2), 1);
    assert_eq!(b.run_due().0, 0);
}

/// A second host already fetched the run before A committed and consumed it.
/// Normal cleanup succeeds; resuming the fetched generation after the client
/// receipt horizon must still be fenced. No SQL fault is installed here.
#[test]
fn on_postgres_a_stale_fetched_schedule_cannot_repeat_a_completed_action() {
    let url = database().expect("this reproduction requires PostgreSQL");
    let (_schema, layer) = fresh_store(&url);
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
    let before = schedules::select(b.runs(), b.now(), 16).expect("B selects due work");
    let c_before = schedules::select(c.runs(), c.now(), 16).expect("C selects due work");
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
    let (_schema, layer) = fresh_store(&url);
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

struct PgReplay {
    _schema: SchedulerSchema,
    layer: Arc<StorePg>,
    out: std::path::PathBuf,
    _dir: tempfile::TempDir,
    url: String,
}
impl PgReplay {
    fn new(change: fn(&str) -> String) -> Self {
        let url = database().expect("selected scheduler tests require PostgreSQL");
        let (schema, layer) = fresh_store(&url);
        println!("SCHEDULER_SCHEMA {}", layer.schema());
        let (dir, out) = built_from_patches_in("examples", change, &[]);
        Self {
            _schema: schema,
            layer,
            out,
            _dir: dir,
            url,
        }
    }
    fn host(&self) -> Server {
        let layer = Arc::new(
            StorePg::open_with(
                &self.url,
                self.layer.schema(),
                Isolation::Serializable,
                crate::feed_pg::RenderSeats::default(),
            )
            .expect("independent host layer"),
        );
        Server::from_build_layers(self.out.clone(), self.out.clone(), None, Some(layer))
            .expect("the compiled program served")
    }
    fn count(&self, sql: &str) -> i64 {
        self.layer
            .sql()
            .query_one(sql, &[])
            .expect("committed state")
            .get(0)
    }
}
const FIVE: u64 = 5 * 60 * 1000;

#[test]
fn a_foreign_adapter_candidate_must_reselect_even_on_the_same_database() {
    let lab = PgReplay::new(super::store_scheduled::with_scheduling);
    let a = lab.host();
    let b = lab.host();
    assert!(super::store_scheduled::in_five(&a, "espresso", false, "foreign-seed").committed);
    a.query_clock.0.advance(FIVE);
    b.query_clock.0.advance(FIVE);
    let foreign = schedules::select(b.runs(), b.now(), 16).expect("B selects");
    a.run_scheduled(&foreign[0]);
    assert!(lab.host().cart_lines("a").is_empty());
    assert_eq!(
        lab.count("SELECT attempts FROM schedules"),
        0,
        "foreign failure must not update another authority's revision"
    );
    assert_eq!(a.run_due().0, 1, "A reselects from its own adapter");
    assert_eq!(lab.host().cart_lines("a"), [("espresso".to_string(), 1)]);
}

#[test]
fn stale_retries_failed_rows_and_changed_descriptors_cannot_act_or_clean_up() {
    let lab = PgReplay::new(super::store_scheduled::with_scheduling);
    let a = lab.host();
    assert!(super::store_scheduled::in_five(&a, "espresso", false, "revision-seed").committed);
    a.query_clock.0.advance(FIVE);
    let held = schedules::select(a.runs(), a.now(), 16).expect("selected");
    lab.layer
        .sql()
        .execute(
            "UPDATE schedules SET attempts = 1, due_ms = due_ms + 60000",
            &[],
        )
        .expect("a newer retry revision");
    a.run_scheduled(&held[0]);
    assert_eq!(lab.count("SELECT attempts FROM schedules"), 1);
    assert!(lab.host().cart_lines("a").is_empty());
    lab.layer
        .sql()
        .execute("UPDATE schedules SET failed = 'terminal'", &[])
        .expect("a terminal revision");
    a.run_scheduled(&held[0]);
    assert_eq!(
        lab.count("SELECT count(*) FROM schedules WHERE failed = 'terminal'"),
        1
    );
    lab.layer
        .sql()
        .execute(
            "UPDATE schedules SET failed = NULL, attempts = 0, due_ms = $1, args = '[\"cortado\"]'",
            &[&held[0].due_ms],
        )
        .expect("a changed descriptor");
    a.run_scheduled(&held[0]);
    assert!(lab.host().cart_lines("a").is_empty());
    assert_eq!(lab.count("SELECT count(*) FROM schedules"), 1);
}

#[test]
fn rescheduling_after_selection_preserves_the_replacement_generation() {
    let lab = PgReplay::new(super::store_scheduled::with_scheduling);
    let a = lab.host();
    assert!(super::store_scheduled::in_five(&a, "espresso", false, "replace-first").committed);
    a.query_clock.0.advance(FIVE);
    let old = schedules::select(a.runs(), a.now(), 16).expect("old selected");
    assert!(super::store_scheduled::in_five(&a, "espresso", false, "replace-second").committed);
    let replacement = a.runs().all_runs().expect("replacement");
    assert_ne!(replacement[0].id, old[0].id);
    a.run_scheduled(&old[0]);
    assert!(lab.host().cart_lines("a").is_empty());
    assert_eq!(a.runs().all_runs().expect("retained"), replacement);
    a.query_clock.0.advance(FIVE);
    assert_eq!(a.run_due().0, 1);
    assert_eq!(lab.host().cart_lines("a"), [("espresso".to_string(), 1)]);
}

#[test]
fn two_hosts_racing_the_same_generation_commit_one_action_and_event() {
    let lab = PgReplay::new(super::store_scheduled::with_scheduling);
    let a = lab.host();
    let b = lab.host();
    assert!(super::store_scheduled::in_five(&a, "espresso", false, "race-seed").committed);
    a.query_clock.0.advance(FIVE);
    b.query_clock.0.advance(FIVE);
    let ac = schedules::select(a.runs(), a.now(), 16).expect("A selected");
    let bc = schedules::select(b.runs(), b.now(), 16).expect("B selected");
    let barrier = std::sync::Barrier::new(2);
    std::thread::scope(|scope| {
        let first = scope.spawn(|| {
            barrier.wait();
            a.run_scheduled(&ac[0]);
        });
        let second = scope.spawn(|| {
            barrier.wait();
            b.run_scheduled(&bc[0]);
        });
        first.join().expect("A worker completed");
        second.join().expect("B worker completed");
    });
    assert_eq!(lab.host().cart_lines("a"), [("espresso".to_string(), 1)]);
    assert_eq!(lab.count("SELECT count(*) FROM schedules"), 0);
    assert_eq!(
        lab.count("SELECT count(*) FROM outbox WHERE kind = 'event'"),
        1
    );
    assert_eq!(
        lab.count("SELECT count(*) FROM interactions WHERE key LIKE '%schedule-%'"),
        1
    );
}

#[test]
fn legacy_generations_are_quarantined_without_replaying_ambiguous_history() {
    let lab = PgReplay::new(super::store_scheduled::with_scheduling);
    let a = lab.host();
    assert!(super::store_scheduled::in_five(&a, "espresso", false, "legacy-seed").committed);
    lab.layer
        .sql()
        .execute("UPDATE schedules SET protocol = 0", &[])
        .expect("legacy fixture");
    a.query_clock.0.advance(FIVE + 25 * 60 * 60 * 1000);
    assert_eq!(a.run_due().0, 1);
    assert!(lab.host().cart_lines("a").is_empty());
    assert_eq!(
        lab.count("SELECT count(*) FROM schedules WHERE failed LIKE '%reconciliation%'"),
        1
    );
    assert_eq!(a.run_due().0, 0);
}

#[test]
fn retry_persistence_failure_still_requests_a_bounded_wait_each_driver_tick() {
    let lab = PgReplay::new(super::store_scheduled::with_scheduling);
    let a = lab.host();
    assert!(super::store_scheduled::in_five(&a, "espresso", false, "blocked-seed").committed);
    lab.layer.sql().batch_execute("CREATE FUNCTION deny_completion_and_retry() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'isolated persistence failure'; END $$; CREATE TRIGGER deny_completion BEFORE DELETE OR UPDATE ON schedules FOR EACH ROW EXECUTE FUNCTION deny_completion_and_retry();").expect("isolated failure");
    a.query_clock.0.advance(FIVE);
    let ticks = std::cell::Cell::new(0);
    let waits = std::cell::RefCell::new(Vec::new());
    a.drive_scheduled(
        || {
            let n = ticks.get();
            ticks.set(n + 1);
            n < 3
        },
        |delay| waits.borrow_mut().push(delay),
    );
    assert_eq!(
        *waits.borrow(),
        [1000, 1000, 1000],
        "selection count is not a reason to skip the failure wait"
    );
    assert!(lab.host().cart_lines("a").is_empty());
    assert_eq!(
        lab.count("SELECT attempts FROM schedules"),
        0,
        "failed persistence does not claim a retry was kept"
    );
}

fn with_terminal_clock(app: &str) -> String {
    format!(
        "{}{}",
        super::store_scheduled::with_scheduling(app),
        r#"
command pure_clock() -> Result<Int, CartError>
    invoked_by Clock
    idempotent_by InteractionId
{ Ok(42) }
command notice_clock() -> Result<Int, CartError>
    invoked_by Clock
    idempotent_by InteractionId
    emits UserCartChanged(current_user())
{ Ok(7) }
command failing_clock(item: MenuItemId) -> Result<Int, CartError>
    invoked_by Clock
    idempotent_by InteractionId
    transaction serializable
    emits UserCartChanged(current_user())
{
    let cart = UserCarts.add(current_user(), item, PositiveInt(1))?
    schedule pure_clock() after minutes(5)
    Err(CartError.ItemUnavailable(item))
}
command repeat_clock(item: MenuItemId) -> Result<Int, CartError>
    invoked_by Clock
    idempotent_by InteractionId
    transaction serializable
    emits UserCartChanged(current_user())
{
    let cart = UserCarts.add(current_user(), item, PositiveInt(1))?
    schedule repeat_clock(item) after minutes(5)
    Ok(1)
}
command queue_pure() -> Result<Int, CartError>
    idempotent_by InteractionId
{ schedule pure_clock() after minutes(5)
  Ok(1) }
command queue_notice() -> Result<Int, CartError>
    idempotent_by InteractionId
{ schedule notice_clock() after minutes(5)
  Ok(1) }
command queue_failure(item: MenuItemId) -> Result<Int, CartError>
    idempotent_by InteractionId
{ schedule failing_clock(item) after minutes(5)
  Ok(1) }
command queue_repeat(item: MenuItemId) -> Result<Int, CartError>
    idempotent_by InteractionId
{ schedule repeat_clock(item) after minutes(5)
  Ok(1) }
"#
    )
}

#[test]
fn no_write_success_and_event_only_success_still_consume_and_record() {
    let lab = PgReplay::new(with_terminal_clock);
    let a = lab.host();
    assert!(
        a.command_answered("store.page.queue_pure", "a", &[], Some("pure-seed"))
            .expect("pure scheduled")
            .committed
    );
    a.query_clock.0.advance(FIVE);
    assert_eq!(a.run_due().0, 1);
    assert_eq!(lab.count("SELECT count(*) FROM schedules"), 0);
    assert_eq!(
        lab.count("SELECT count(*) FROM interactions WHERE key LIKE '%schedule-%'"),
        1
    );
    assert_eq!(
        lab.count("SELECT count(*) FROM outbox WHERE kind = 'event'"),
        0
    );
    assert!(
        a.command_answered("store.page.queue_notice", "a", &[], Some("notice-seed"))
            .expect("notice scheduled")
            .committed
    );
    a.query_clock.0.advance(FIVE);
    assert_eq!(a.run_due().0, 1);
    assert_eq!(lab.count("SELECT count(*) FROM schedules"), 0);
    assert_eq!(
        lab.count("SELECT count(*) FROM interactions WHERE key LIKE '%schedule-%'"),
        2
    );
    assert_eq!(
        lab.count("SELECT count(*) FROM outbox WHERE kind = 'event'"),
        1
    );
    assert!(lab.host().cart_lines("a").is_empty());
}

#[test]
fn declared_error_discards_business_and_new_scheduling_then_completes() {
    let lab = PgReplay::new(with_terminal_clock);
    let a = lab.host();
    assert!(
        a.command_answered(
            "store.page.queue_failure",
            "a",
            &[Val::String("espresso".into())],
            Some("error-seed")
        )
        .expect("failure scheduled")
        .committed
    );
    a.query_clock.0.advance(FIVE);
    assert_eq!(a.run_due().0, 1);
    assert!(lab.host().cart_lines("a").is_empty());
    assert_eq!(
        lab.count("SELECT count(*) FROM schedules"),
        0,
        "the attempted child scheduling rolled back"
    );
    assert_eq!(
        lab.count("SELECT count(*) FROM outbox WHERE kind = 'event'"),
        0
    );
    let raw: String = lab
        .layer
        .sql()
        .query_one(
            "SELECT answer FROM interactions WHERE key LIKE '%schedule-%'",
            &[],
        )
        .expect("terminal receipt")
        .get(0);
    let kept: serde_json::Value = serde_json::from_str(&raw).expect("structured answer");
    assert_eq!(kept["committed"], false);
    assert_eq!(kept["why"], serde_json::Value::Null);
    assert!(!kept["result"].is_null());
    assert_eq!(lab.host().run_due().0, 0);
}

#[test]
fn self_rescheduling_creates_a_new_generation_before_old_cleanup_can_touch_it() {
    let lab = PgReplay::new(with_terminal_clock);
    let a = lab.host();
    assert!(
        a.command_answered(
            "store.page.queue_repeat",
            "a",
            &[Val::String("espresso".into())],
            Some("repeat-seed")
        )
        .expect("repeat scheduled")
        .committed
    );
    a.query_clock.0.advance(FIVE);
    let old = schedules::select(a.runs(), a.now(), 16).expect("old selected");
    assert_eq!(a.run_due().0, 1);
    assert_eq!(lab.host().cart_lines("a"), [("espresso".to_string(), 1)]);
    let next = a.runs().all_runs().expect("replacement");
    assert_eq!(next.len(), 1);
    assert_ne!(next[0].id, old[0].id);
    a.run_scheduled(&old[0]);
    assert_eq!(a.runs().all_runs().expect("still replacement"), next);
    a.query_clock.0.advance(FIVE);
    assert_eq!(a.run_due().0, 1);
    assert_eq!(lab.host().cart_lines("a"), [("espresso".to_string(), 2)]);
    assert_eq!(lab.count("SELECT count(*) FROM schedules"), 1);
}

#[test]
fn ordinary_client_receipt_expiry_and_argument_binding_are_preserved() {
    let lab = PgReplay::new(super::store_scheduled::with_scheduling);
    let first = lab.host();
    let request = [shown("espresso"), serde_json::json!(1)];
    let answer = first
        .command_answer(ADD, "a", &request, Some("ordinary-id"))
        .expect("initial client answer");
    assert!(answer.committed);
    drop(first);
    let within = lab.host();
    within.query_clock.0.advance(23 * 60 * 60 * 1000);
    assert_eq!(
        within
            .command_answer(ADD, "a", &request, Some("ordinary-id"))
            .expect("within receipt window"),
        answer
    );
    assert!(
        within
            .command_answer(
                ADD,
                "a",
                &[shown("cortado"), serde_json::json!(1)],
                Some("ordinary-id")
            )
            .is_err()
    );
    assert_eq!(lab.host().cart_lines("a"), [("espresso".to_string(), 1)]);
    let later = lab.host();
    later.query_clock.0.advance(25 * 60 * 60 * 1000);
    assert!(
        later
            .command_answer(ADD, "a", &request, Some("ordinary-id"))
            .expect("new expired-client request")
            .committed
    );
    assert_eq!(lab.host().cart_lines("a"), [("espresso".to_string(), 2)]);
}

#[test]
fn a_suppressed_completion_delete_is_refusal_and_keeps_retry_backoff() {
    let lab = PgReplay::new(super::store_scheduled::with_scheduling);
    let a = lab.host();
    assert!(super::store_scheduled::in_five(&a, "espresso", false, "suppressed-seed").committed);
    lab.layer.sql().batch_execute("CREATE FUNCTION suppress_completion() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RETURN NULL; END $$; CREATE TRIGGER suppress_completion BEFORE DELETE ON schedules FOR EACH ROW EXECUTE FUNCTION suppress_completion();").expect("suppression installed");
    a.query_clock.0.advance(FIVE);
    assert_eq!(a.run_due().0, 1);
    assert!(lab.host().cart_lines("a").is_empty());
    let pending = a.runs().all_runs().expect("retry persisted");
    assert_eq!(pending[0].attempts, 1);
    assert!(pending[0].due_ms > a.now());
    assert_eq!(
        lab.count("SELECT count(*) FROM interactions WHERE key LIKE '%schedule-%'"),
        0
    );
    assert_eq!(
        lab.count("SELECT count(*) FROM outbox WHERE kind = 'event'"),
        0
    );
}

struct FeedReplay {
    _schema: SchedulerSchema,
    url: String,
    schema: String,
    out: std::path::PathBuf,
    _dir: tempfile::TempDir,
}
impl FeedReplay {
    fn new() -> Self {
        let url =
            std::env::var("PW_FEED_DATABASE_URL").expect("selected feed tests require PostgreSQL");
        let owned = SchedulerSchema::new(&url, "feed");
        let schema = owned.schema.clone();
        let (dir, out) = built_feed_with(super::feed_pg::with_scheduling);
        Self {
            _schema: owned,
            url,
            schema,
            out,
            _dir: dir,
        }
    }
    fn host(&self) -> Server {
        let layer =
            crate::feed_pg::FeedPg::open(&self.url, Some(&self.schema)).expect("feed layer");
        Server::from_build_with(self.out.clone(), self.out.clone(), Some(Arc::new(layer)))
            .expect("compiled feed served")
    }
    fn sql(&self) -> postgres::Client {
        let mut c = postgres::Client::connect(&self.url, postgres::NoTls)
            .expect("fresh inspection connection");
        c.batch_execute(&format!("SET search_path TO {}", self.schema))
            .expect("owned namespace");
        c
    }
    fn count(&self, sql: &str) -> i64 {
        self.sql()
            .query_one(sql, &[])
            .expect("committed feed state")
            .get(0)
    }
}

#[test]
fn feed_post_and_event_commit_once_across_hosts_and_expired_stale_selection() {
    let lab = FeedReplay::new();
    let a = lab.host();
    let b = lab.host();
    let before = lab.count("SELECT count(*) FROM posts");
    let scheduled = posted(
        &a,
        "/command/feed.app.post_in_five",
        "a",
        "feed-seed",
        "[\"Scheduled synthetic post\"]",
    );
    assert!(scheduled.contains("\"committed\":true"), "{scheduled}");
    a.query_clock.0.advance(FIVE);
    b.query_clock.0.advance(FIVE);
    let held = schedules::select(b.runs(), b.now(), 16).expect("B selected feed work");
    let version = a.clock.now();
    assert_eq!(a.run_due().0, 1);
    assert!(
        a.clock.now() > version,
        "feed delivery version advances for Clock origin"
    );
    assert_eq!(lab.count("SELECT count(*) FROM posts"), before + 1);
    let events = lab.count("SELECT count(*) FROM outbox WHERE kind = 'event'");
    assert_eq!(events, 1);
    b.query_clock.0.advance(25 * 60 * 60 * 1000);
    b.run_scheduled(&held[0]);
    assert_eq!(lab.count("SELECT count(*) FROM posts"), before + 1);
    assert_eq!(lab.count("SELECT count(*) FROM schedules"), 0);
    assert_eq!(
        lab.count("SELECT count(*) FROM outbox WHERE kind = 'event'"),
        events
    );
}

#[test]
fn feed_refused_completion_retains_no_post_receipt_or_event() {
    let lab = FeedReplay::new();
    let a = lab.host();
    let before = lab.count("SELECT count(*) FROM posts");
    let scheduled = posted(
        &a,
        "/command/feed.app.post_in_five",
        "a",
        "feed-fault",
        "[\"Scheduled synthetic refusal\"]",
    );
    assert!(scheduled.contains("\"committed\":true"), "{scheduled}");
    lab.sql().batch_execute("CREATE FUNCTION deny_feed_completion() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'isolated feed refusal'; END $$; CREATE TRIGGER deny_feed_completion BEFORE DELETE ON schedules FOR EACH ROW EXECUTE FUNCTION deny_feed_completion();").expect("fault installed");
    a.query_clock.0.advance(FIVE);
    assert_eq!(a.run_due().0, 1);
    assert_eq!(lab.count("SELECT count(*) FROM posts"), before);
    assert_eq!(
        lab.count("SELECT count(*) FROM interactions WHERE key LIKE '%schedule-%'"),
        0
    );
    assert_eq!(
        lab.count("SELECT count(*) FROM outbox WHERE kind = 'event'"),
        0
    );
    assert_eq!(lab.count("SELECT attempts FROM schedules"), 1);
}

#[test]
fn suppressed_retry_persistence_is_not_mistaken_for_a_stale_generation() {
    let lab = PgReplay::new(super::store_scheduled::with_scheduling);
    let a = lab.host();
    assert!(super::store_scheduled::in_five(&a, "espresso", false, "suppressed-retry").committed);
    lab.layer.sql().batch_execute("CREATE FUNCTION deny_completion() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'isolated completion refusal'; END $$; CREATE FUNCTION suppress_retry() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RETURN NULL; END $$; CREATE TRIGGER deny_completion BEFORE DELETE ON schedules FOR EACH ROW EXECUTE FUNCTION deny_completion(); CREATE TRIGGER suppress_retry BEFORE UPDATE ON schedules FOR EACH ROW EXECUTE FUNCTION suppress_retry();").expect("isolated persistence fault installed");
    a.query_clock.0.advance(FIVE);
    let selected = a.runs().all_runs().expect("original due revision");
    let ticks = std::cell::Cell::new(0);
    let waits = std::cell::RefCell::new(Vec::new());
    a.drive_scheduled(
        || {
            let n = ticks.get();
            ticks.set(n + 1);
            n < 3
        },
        |delay| waits.borrow_mut().push(delay),
    );
    assert_eq!(
        *waits.borrow(),
        [1000, 1000, 1000],
        "unchanged due work needs a persistence-failure wait"
    );
    assert_eq!(a.runs().all_runs().expect("revision retained"), selected);
    assert!(lab.host().cart_lines("a").is_empty());
}

#[test]
fn queue_revisions_after_evaluation_are_fenced_before_business_commit() {
    for sql in [
        "DELETE FROM schedules",
        "UPDATE schedules SET due_ms=due_ms+60000, attempts=attempts+1",
        "UPDATE schedules SET failed='newer terminal revision'",
        "UPDATE schedules SET args='[\"cortado\"]'",
    ] {
        let lab = PgReplay::new(super::store_scheduled::with_scheduling);
        let mut a = lab.host();
        assert!(super::store_scheduled::in_five(&a, "espresso", false, "commit-race").committed);
        let authority = lab.layer.clone();
        let fired = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let observed = fired.clone();
        let crossed = probe(
            &mut a,
            Some(Box::new(move || {
                authority
                    .sql()
                    .batch_execute(sql)
                    .expect("revision changed at the pre-commit seam");
                observed.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            })),
            false,
            false,
        );
        a.query_clock.0.advance(FIVE);
        assert_eq!(a.run_due().0, 1);
        assert_eq!(
            fired.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "the change followed evaluation and early admission"
        );
        assert_eq!(crossed.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert!(
            lab.host().cart_lines("a").is_empty(),
            "stale evaluation retained no business effect"
        );
        assert_eq!(
            lab.count("SELECT count(*) FROM outbox WHERE kind='event'"),
            0
        );
        assert_eq!(
            lab.count("SELECT count(*) FROM interactions WHERE key LIKE '%schedule-%'"),
            0
        );
        if sql.starts_with("DELETE") {
            assert_eq!(lab.count("SELECT count(*) FROM schedules"), 0);
        } else {
            assert_eq!(
                lab.count("SELECT count(*) FROM schedules"),
                1,
                "the newer generation/revision survives"
            );
        }
    }
}

#[test]
fn lost_adapter_ack_after_real_commit_never_resurrects_consumed_work() {
    for hidden in [false, true] {
        let lab = PgReplay::new(super::store_scheduled::with_scheduling);
        let mut a = lab.host();
        assert!(super::store_scheduled::in_five(&a, "espresso", false, "lost-ack").committed);
        let crossed = probe(&mut a, None, true, hidden);
        a.query_clock.0.advance(FIVE);
        let held = schedules::select(a.runs(), a.now(), 16).expect("a real candidate");
        a.run_scheduled(&held[0]);
        assert_eq!(
            crossed.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "real adapter reported Applied before injected acknowledgement loss"
        );
        assert_eq!(lab.host().cart_lines("a"), [("espresso".to_string(), 1)]);
        assert_eq!(lab.count("SELECT count(*) FROM schedules"), 0);
        assert_eq!(
            lab.count("SELECT count(*) FROM outbox WHERE kind='event'"),
            1
        );
        assert_eq!(
            lab.count("SELECT count(*) FROM interactions WHERE key LIKE '%schedule-%'"),
            1
        );
        lab.layer
            .sql()
            .batch_execute("DELETE FROM interactions")
            .expect("all result receipts explicitly pruned");
        a.query_clock.0.advance(25 * 60 * 60 * 1000);
        a.run_scheduled(&held[0]);
        assert_eq!(a.run_due().0, 0);
        assert_eq!(lab.host().run_due().0, 0);
        assert_eq!(crossed.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(lab.host().cart_lines("a"), [("espresso".to_string(), 1)]);
        assert_eq!(
            lab.count("SELECT count(*) FROM outbox WHERE kind='event'"),
            1
        );
        assert_eq!(lab.count("SELECT count(*) FROM interactions"), 0);
    }
}

#[test]
fn declared_error_losing_to_another_terminal_completion_keeps_one_receipt() {
    let lab = PgReplay::new(with_terminal_clock);
    let mut a = lab.host();
    assert!(
        a.command_answered(
            "store.page.queue_failure",
            "a",
            &[Val::String("espresso".into())],
            Some("terminal-race")
        )
        .expect("scheduled")
        .committed
    );
    let b = lab.host();
    a.query_clock.0.advance(FIVE);
    b.query_clock.0.advance(FIVE);
    let ran = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let observed = ran.clone();
    let crossed = probe(
        &mut a,
        Some(Box::new(move || {
            let selected = b.run_due().0;
            observed.store(selected, std::sync::atomic::Ordering::SeqCst);
        })),
        false,
        false,
    );
    assert_eq!(a.run_due().0, 1);
    assert_eq!(
        ran.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "B wins while A is completing its declared error"
    );
    assert_eq!(
        crossed.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "A cannot claim B's generation"
    );
    assert!(lab.host().cart_lines("a").is_empty());
    assert_eq!(lab.count("SELECT count(*) FROM schedules"), 0);
    assert_eq!(
        lab.count("SELECT count(*) FROM interactions WHERE key LIKE '%schedule-%'"),
        1
    );
    assert_eq!(
        lab.count("SELECT count(*) FROM outbox WHERE kind='event'"),
        0
    );
}

#[test]
fn pure_clock_cancelled_after_evaluation_cannot_claim_a_terminal_receipt() {
    let lab = PgReplay::new(with_terminal_clock);
    let mut a = lab.host();
    assert!(
        a.command_answered("store.page.queue_pure", "a", &[], Some("pure-cancel-race"))
            .expect("pure scheduled")
            .committed
    );
    let authority = lab.layer.clone();
    let fired = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let observed = fired.clone();
    let crossed = probe(
        &mut a,
        Some(Box::new(move || {
            authority
                .sql()
                .batch_execute("DELETE FROM schedules")
                .expect("cancellation after pure evaluation");
            observed.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        })),
        false,
        false,
    );
    a.query_clock.0.advance(FIVE);
    assert_eq!(a.run_due().0, 1);
    assert_eq!(fired.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(
        crossed.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "an absent generation is not a successful terminal completion"
    );
    assert_eq!(lab.count("SELECT count(*) FROM schedules"), 0);
    assert_eq!(
        lab.count("SELECT count(*) FROM interactions WHERE key LIKE '%schedule-%'"),
        0
    );
    assert_eq!(
        lab.count("SELECT count(*) FROM outbox WHERE kind='event'"),
        0
    );
}
