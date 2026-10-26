//! Memory/materializer acceptance for the same scheduler transaction contract.
use super::*;
const FIVE: u64 = 5 * 60 * 1000;
fn memory() -> Served {
    // This suite selects its own adapter, even in the whole-server PG matrix.
    let (dir, out) =
        built_from_patches_in("examples", super::store_scheduled::with_scheduling, &[]);
    let s = Served {
        server: Server::from_build_layers(
            out.clone(),
            out,
            None,
            Some(Arc::new(store::StoreData::new())),
        )
        .expect("explicit memory adapter served"),
        _dir: dir,
    };
    if s.data.runs().is_some() {
        panic!("this selected gate requires the memory adapter");
    }
    s
}
fn sqlite_count(s: &Server, sql: &str) -> i64 {
    let mut count = 0;
    s.materializer
        .command(|tx| {
            count = tx
                .query_row(sql, [], |r| r.get(0))
                .map_err(|_| "state inspection failed".to_string())?;
            Ok::<_, String>(Vec::new())
        })
        .expect("committed materializer state inspected");
    count
}
fn audit(s: &Server) {
    s.materializer.command(|tx| {
        tx.execute_batch("CREATE TABLE scheduler_events_seen (name TEXT NOT NULL); CREATE TRIGGER scheduler_events_seen AFTER INSERT ON outbox BEGIN INSERT INTO scheduler_events_seen VALUES (NEW.event); END;").map_err(|_| "audit setup failed".to_string())?;
        Ok::<_, String>(Vec::new())
    }).expect("independent event audit installed");
}

#[test]
fn memory_consumed_work_is_fenced_after_all_client_receipts_are_pruned() {
    let s = memory();
    audit(&s);
    assert!(super::store_scheduled::in_five(&s, "espresso", false, "memory-seed").committed);
    s.query_clock.0.advance(FIVE);
    let old = schedules::select(s.runs(), s.now(), 16).expect("selected");
    assert_eq!(s.run_due().0, 1);
    s.materializer
        .command(|tx| {
            Materializer::prune_interactions(tx, u64::MAX);
            Ok::<_, String>(Vec::new())
        })
        .expect("old receipts pruned");
    s.query_clock.0.advance(25 * 60 * 60 * 1000);
    s.run_scheduled(&old[0]);
    assert_eq!(s.cart_lines("a"), [("espresso".to_string(), 1)]);
    assert_eq!(sqlite_count(&s, "SELECT count(*) FROM schedules"), 0);
    assert_eq!(
        sqlite_count(&s, "SELECT count(*) FROM scheduler_events_seen"),
        1
    );
    assert_eq!(sqlite_count(&s, "SELECT count(*) FROM interactions"), 0);
}

#[test]
fn memory_completion_refusal_rolls_back_publication_and_recovers() {
    let s = memory();
    audit(&s);
    assert!(super::store_scheduled::in_five(&s, "espresso", false, "memory-fault").committed);
    s.materializer.command(|tx| {
        tx.execute_batch("CREATE TRIGGER deny_completion BEFORE DELETE ON schedules BEGIN SELECT RAISE(ABORT, 'isolated completion refusal'); END;").map_err(|_| "fault setup failed".to_string())?;
        Ok::<_, String>(Vec::new())
    }).expect("fault installed");
    s.query_clock.0.advance(FIVE);
    assert_eq!(s.run_due().0, 1);
    assert!(s.cart_lines("a").is_empty());
    assert_eq!(
        sqlite_count(&s, "SELECT count(*) FROM scheduler_events_seen"),
        0
    );
    assert_eq!(sqlite_count(&s, "SELECT count(*) FROM interactions"), 0);
    s.materializer
        .command(|tx| {
            tx.execute_batch("DROP TRIGGER deny_completion")
                .map_err(|_| "fault removal failed".to_string())?;
            Ok::<_, String>(Vec::new())
        })
        .expect("fault removed");
    s.query_clock.0.advance(2000);
    assert_eq!(s.run_due().0, 1);
    assert_eq!(s.cart_lines("a"), [("espresso".to_string(), 1)]);
    assert_eq!(sqlite_count(&s, "SELECT count(*) FROM schedules"), 0);
    assert_eq!(
        sqlite_count(&s, "SELECT count(*) FROM scheduler_events_seen"),
        1
    );
}

#[test]
fn memory_old_generation_does_not_execute_or_remove_its_replacement() {
    let s = memory();
    assert!(super::store_scheduled::in_five(&s, "espresso", false, "memory-first").committed);
    s.query_clock.0.advance(FIVE);
    let old = schedules::select(s.runs(), s.now(), 16).expect("old selected");
    assert!(super::store_scheduled::in_five(&s, "espresso", false, "memory-second").committed);
    let replacement = s.runs().all_runs().expect("replacement");
    assert_ne!(old[0].id, replacement[0].id);
    s.run_scheduled(&old[0]);
    assert!(s.cart_lines("a").is_empty());
    assert_eq!(s.runs().all_runs().expect("retained"), replacement);
    s.query_clock.0.advance(FIVE);
    assert_eq!(s.run_due().0, 1);
    assert_eq!(s.cart_lines("a"), [("espresso".to_string(), 1)]);
}

#[test]
fn memory_foreign_materializer_selection_is_not_rebound_to_another_server() {
    let a = memory();
    let b = memory();
    assert!(super::store_scheduled::in_five(&a, "espresso", false, "source-a").committed);
    assert!(super::store_scheduled::in_five(&b, "espresso", false, "source-b").committed);
    a.query_clock.0.advance(FIVE);
    b.query_clock.0.advance(FIVE);
    let foreign = schedules::select(b.runs(), b.now(), 16).expect("B selected");
    a.run_scheduled(&foreign[0]);
    assert!(a.cart_lines("a").is_empty());
    assert_eq!(a.runs().all_runs().expect("A unchanged")[0].attempts, 0);
    assert_eq!(a.run_due().0, 1);
    assert_eq!(a.cart_lines("a"), [("espresso".to_string(), 1)]);
    assert!(b.cart_lines("a").is_empty());
}

#[test]
fn clock_jobs_do_not_retain_synthetic_presentations_or_private_query_values() {
    let s = memory();
    // Warm real client state first; Clock may invalidate it but must not delete it.
    s.command_answered(ADD, "a", &add_shown("espresso", 1), Some("client-control"))
        .expect("client writes");
    assert!(s.materializer.state("cart:a").is_some());
    assert!(super::store_scheduled::in_five(&s, "espresso", false, "warm-job").committed);
    s.query_clock.0.advance(FIVE);
    s.run_due();
    let entries = s.materializer.entry_count();
    let values = s.queries.stored();
    let state = sqlite_count(&s, "SELECT count(*) FROM state");
    for n in 0..12 {
        assert!(
            super::store_scheduled::in_five(&s, "espresso", false, &format!("bounded-{n}"))
                .committed
        );
        s.query_clock.0.advance(FIVE);
        assert_eq!(s.run_due().0, 1);
    }
    assert_eq!(s.cart_lines("a"), [("espresso".to_string(), 14)]);
    assert_eq!(s.materializer.entry_count(), entries);
    assert_eq!(s.queries.stored(), values);
    assert_eq!(sqlite_count(&s, "SELECT count(*) FROM state"), state);
    assert!(
        s.materializer.state("cart:a").is_some(),
        "real client state retained"
    );
    assert!(
        s.interactions
            .lock()
            .expect("client queues")
            .keys()
            .all(|k| !k.starts_with("on-behalf:"))
    );
    assert!(
        s.applied
            .lock()
            .expect("client applied")
            .keys()
            .all(|k| !k.starts_with("on-behalf:"))
    );
    assert!(s.scheduled_locks.lock().expect("scoped locks").len() <= 1);
}

#[test]
fn memory_cancellation_after_evaluation_is_fenced_before_publication() {
    let Served { mut server, _dir } = memory();
    audit(&server);
    assert!(
        super::store_scheduled::in_five(&server, "espresso", false, "memory-admitted").committed
    );
    let target = Arc::new(std::sync::OnceLock::<std::sync::Weak<Server>>::new());
    let captured = target.clone();
    let fired = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let observed = fired.clone();
    super::scheduled_probe::probe(
        &mut server,
        Some(Box::new(move || {
            let server = captured
                .get()
                .expect("target installed")
                .upgrade()
                .expect("live host");
            server
                .materializer
                .command(|tx| {
                    tx.execute("DELETE FROM schedules", [])
                        .map_err(|_| "cancellation setup refused".to_string())?;
                    Ok::<_, String>(Vec::new())
                })
                .expect("independent cancellation after early admission");
            observed.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        })),
        false,
        false,
    );
    let s = Arc::new(server);
    target
        .set(Arc::downgrade(&s))
        .expect("target installed once");
    s.query_clock.0.advance(FIVE);
    assert_eq!(s.run_due().0, 1);
    assert_eq!(fired.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert!(
        s.cart_lines("a").is_empty(),
        "staged application writes never publish after cancellation"
    );
    assert_eq!(sqlite_count(&s, "SELECT count(*) FROM schedules"), 0);
    assert_eq!(sqlite_count(&s, "SELECT count(*) FROM interactions"), 0);
    assert_eq!(
        sqlite_count(&s, "SELECT count(*) FROM scheduler_events_seen"),
        0
    );
}

#[test]
fn memory_suppressed_completion_delete_cannot_publish_a_successful_action() {
    let s = memory();
    audit(&s);
    assert!(
        super::store_scheduled::in_five(&s, "espresso", false, "suppressed-memory-delete")
            .committed
    );
    s.materializer.command(|tx| {
        tx.execute_batch("CREATE TRIGGER suppress_completion BEFORE DELETE ON schedules BEGIN SELECT RAISE(IGNORE); END;").map_err(|_|"fault setup refused".to_string())?;
        Ok::<_,String>(Vec::new())
    }).expect("suppressed DELETE fault installed");
    s.query_clock.0.advance(FIVE);
    assert_eq!(s.run_due().0, 1);
    assert!(s.cart_lines("a").is_empty());
    assert_eq!(sqlite_count(&s, "SELECT count(*) FROM interactions"), 0);
    assert_eq!(
        sqlite_count(&s, "SELECT count(*) FROM scheduler_events_seen"),
        0
    );
    let pending = s.runs().all_runs().expect("pending refusal");
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].attempts, 1);
    s.materializer
        .command(|tx| {
            tx.execute_batch("DROP TRIGGER suppress_completion")
                .map_err(|_| "fault removal refused".to_string())?;
            Ok::<_, String>(Vec::new())
        })
        .expect("fault removed");
    s.query_clock.0.advance(2000);
    assert_eq!(s.run_due().0, 1);
    assert_eq!(s.cart_lines("a"), [("espresso".to_string(), 1)]);
    assert_eq!(sqlite_count(&s, "SELECT count(*) FROM schedules"), 0);
    assert_eq!(
        sqlite_count(&s, "SELECT count(*) FROM scheduler_events_seen"),
        1
    );
}
