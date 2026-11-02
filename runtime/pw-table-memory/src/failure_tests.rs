use super::*;
use pw_core::tables::{FieldSchema, FunctionJson, Plan, Step, TableSchema};
use serde_json::json;
use std::sync::{Once, mpsc};
use std::time::Duration;

const INSERT: &str = "pw:tables/users#insert";
const GET: &str = "pw:tables/users#get";
const FAULT: &str = "injected D2 storage/observer fault";

fn database() -> Arc<MemoryTables> {
    let schema = TableSchema {
        name: "Users".into(),
        source: None,
        fields: vec![FieldSchema {
            name: "id".into(),
            ty: "Int".into(),
            stored: Stored::Int {
                at_least: None,
                at_most: None,
            },
        }],
        key: vec!["id".into()],
        order: None,
        indexes: vec![],
        references: vec![],
    };
    let hash = Schema::of(vec![schema.clone()]).hash;
    let function = |name: &str, primitive: Primitive, by: Vec<String>| FunctionJson {
        name: name.into(),
        import: format!("pw:tables/users#{name}"),
        primitive,
        effect: if primitive.writes() { "write" } else { "read" }.into(),
        by,
        sorted_by: vec![],
        params: vec![],
        returns: String::new(),
    };
    Arc::new(
        MemoryTables::new(TablesJson {
            hash: hash.clone(),
            tables: vec![TableJson {
                schema,
                module: "Users".into(),
                interface: "pw:tables/users".into(),
                row: "User".into(),
                bounded: Some(2),
                functions: vec![
                    function("get", Primitive::Get, vec!["id".into()]),
                    function("insert", Primitive::Insert, vec![]),
                ],
            }],
            migration: Plan {
                from: None,
                to: hash,
                steps: vec![Step::CreateTable {
                    table: "Users".into(),
                }],
            },
        })
        .unwrap(),
    )
}

// Suppress only the exact expected injection on our named fault thread.
// An unexpected setup/product panic still uses the original hook and makes
// assertion-site validation refuse a failing producer.
fn fault_thread(action: impl FnOnce() + Send + 'static) -> std::thread::JoinHandle<()> {
    static HOOK: Once = Once::new();
    HOOK.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let injected = info.payload().downcast_ref::<&str>() == Some(&FAULT);
            if std::thread::current().name() != Some("pw-d2-fault-probe") || !injected {
                previous(info);
            }
        }));
    });
    std::thread::Builder::new()
        .name("pw-d2-fault-probe".into())
        .spawn(action)
        .unwrap()
}

#[test]
fn a_wait_observer_may_read_and_replace_itself_without_lock_reentry() {
    let db = database();
    let mut first = Some(db.begin().unwrap());
    first
        .as_mut()
        .unwrap()
        .write(INSERT, &[json!({"id":1})])
        .unwrap();
    let (sent, heard) = mpsc::channel();
    let weak = Arc::downgrade(&db);
    db.tell_on_wait(move || {
        let db = weak.upgrade().unwrap();
        let value = db.read(GET, &[json!(1)]);
        db.tell_on_wait(|| {});
        let _ = sent.send(value);
    });
    let (done, finished) = mpsc::channel();
    let other = Arc::clone(&db);
    let worker = std::thread::spawn(move || {
        let result = other.begin().map(drop);
        let _ = done.send(result);
    });
    let observed = heard.recv_timeout(Duration::from_secs(3));
    // A regression must produce an assertion instead of deadlocking its
    // cleanup on the lock held by the detached observer.
    if observed.is_err() {
        std::mem::forget(first.take().unwrap());
    }
    assert_eq!(
        observed,
        Ok(Ok(Value::Null)),
        "observer reads the published snapshot outside all locks"
    );
    let committed = first.take().unwrap().commit();
    assert!(committed.is_ok(), "the first remains the open command");
    assert_eq!(finished.recv_timeout(Duration::from_secs(3)), Ok(Ok(())));
    worker.join().unwrap();
    assert_eq!(db.read(GET, &[json!(1)]), Ok(json!({"id":1})));
}

#[test]
fn an_observer_panic_does_not_poison_the_database() {
    let db = database();
    let mut first = db.begin().unwrap();
    first.write(INSERT, &[json!({"id":1})]).unwrap();
    db.tell_on_wait(|| std::panic::panic_any(FAULT));
    let other = Arc::clone(&db);
    assert!(
        fault_thread(move || {
            let _ = other.begin();
        })
        .join()
        .is_err()
    );
    assert!(
        !db.inner.working.is_poisoned() && !db.inner.told.is_poisoned(),
        "observer runs outside locks"
    );
    first.commit().unwrap();
    assert_eq!(db.read(GET, &[json!(1)]), Ok(json!({"id":1})));
    assert!(db.begin().is_ok());
}

#[test]
fn internal_working_poison_wakes_every_existing_waiter() {
    let db = database();
    let first = db.begin().unwrap();
    let id = first.id;
    let (sent, heard) = mpsc::channel();
    db.tell_on_wait(move || {
        let _ = sent.send(());
    });
    let (done, finished) = mpsc::channel();
    let mut workers = Vec::new();
    for _ in 0..2 {
        let other = Arc::clone(&db);
        let done = done.clone();
        workers.push(std::thread::spawn(move || {
            let _ = done.send(other.begin().map(drop));
        }));
    }
    for _ in 0..2 {
        let contender = heard.recv_timeout(Duration::from_secs(3));
        assert!(
            contender.is_ok(),
            "each contender waits for the active command"
        );
    }
    let owner = db.inner.working_of(id);
    assert!(
        owner.is_ok(),
        "contenders did not replace the active command"
    );
    drop(owner);
    let broken = Arc::clone(&db);
    assert!(
        fault_thread(move || {
            let _locked = broken.inner.working_of(id).unwrap();
            std::panic::panic_any(FAULT);
        })
        .join()
        .is_err()
    );
    assert!(db.inner.working.is_poisoned());
    for _ in 0..2 {
        assert_eq!(
            finished.recv_timeout(Duration::from_secs(3)),
            Ok(Err(Error::StoragePoisoned))
        );
    }
    for worker in workers {
        worker.join().unwrap();
    }
    assert!(matches!(db.begin(), Err(Error::StoragePoisoned)));
    assert_eq!(db.read(GET, &[json!(1)]), Err(Error::StoragePoisoned));
    // Even clearing the raw poison marker in a test cannot resume this database.
    db.inner.working.clear_poison();
    assert_eq!(db.read(GET, &[json!(1)]), Err(Error::StoragePoisoned));
    drop(first);
}

#[test]
fn a_poisoned_working_lock_cannot_report_a_successful_commit() {
    let db = database();
    let mut command = db.begin().unwrap();
    command.write(INSERT, &[json!({"id":1})]).unwrap();
    let id = command.id;
    let broken = Arc::clone(&db);
    assert!(
        fault_thread(move || {
            let _locked = broken.inner.working_of(id).unwrap();
            std::panic::panic_any(FAULT);
        })
        .join()
        .is_err()
    );
    assert!(db.inner.working.is_poisoned());
    assert_eq!(command.commit(), Err(Error::StoragePoisoned));
    assert_eq!(db.read(GET, &[json!(1)]), Err(Error::StoragePoisoned));
}

#[test]
fn a_poisoned_published_lock_cannot_report_a_successful_commit() {
    let db = database();
    let mut command = db.begin().unwrap();
    command.write(INSERT, &[json!({"id":1})]).unwrap();
    let broken = Arc::clone(&db);
    assert!(
        fault_thread(move || {
            let _locked = broken.inner.published.lock().unwrap();
            std::panic::panic_any(FAULT);
        })
        .join()
        .is_err()
    );
    assert!(db.inner.published.is_poisoned());
    assert_eq!(command.commit(), Err(Error::StoragePoisoned));
    assert!(matches!(db.begin(), Err(Error::StoragePoisoned)));
    assert_eq!(db.read(GET, &[json!(1)]), Err(Error::StoragePoisoned));
}

#[test]
fn an_observer_can_end_the_owner_before_the_waiter_starts_waiting() {
    let db = database();
    let mut first = db.begin().unwrap();
    first.write(INSERT, &[json!({"id":1})]).unwrap();
    let owner = Arc::new(Mutex::new(Some(first)));
    db.tell_on_wait(move || {
        owner.lock().unwrap().take().unwrap().commit().unwrap();
    });
    let other = Arc::clone(&db);
    let (sent, heard) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let _ = sent.send(other.begin().map(drop));
    });
    assert_eq!(
        heard.recv_timeout(Duration::from_secs(3)),
        Ok(Ok(())),
        "recheck after callback prevents a lost wake"
    );
    worker.join().unwrap();
    assert_eq!(db.read(GET, &[json!(1)]), Ok(json!({"id":1})));
}

#[test]
fn a_prepared_publish_refuses_each_poisoned_storage_lock() {
    for working in [true, false] {
        let db = database();
        let mut command = db.begin().unwrap();
        let id = command.id;
        command.write(INSERT, &[json!({"id":1})]).unwrap();
        let prepared = command.prepare().unwrap();
        let broken = Arc::clone(&db);
        assert!(
            fault_thread(move || {
                if working {
                    let _lock = broken.inner.working_of(id).unwrap();
                    std::panic::panic_any(FAULT);
                } else {
                    let _lock = broken.inner.published.lock().unwrap();
                    std::panic::panic_any(FAULT);
                }
            })
            .join()
            .is_err()
        );
        assert_eq!(prepared.publish(), Err(Error::StoragePoisoned));
        assert_eq!(db.read(GET, &[json!(1)]), Err(Error::StoragePoisoned));
    }
}

#[test]
fn a_prepared_command_dropped_unpublished_releases_its_turn_and_rows() {
    let db = database();
    let mut command = db.begin().unwrap();
    let id = command.id;
    command.write(INSERT, &[json!({"id":1})]).unwrap();
    let prepared = command.prepare().unwrap();
    drop(prepared);
    assert!(matches!(db.inner.working_of(id), Err(Error::Ended)));
    assert_eq!(db.read(GET, &[json!(1)]), Ok(Value::Null));
    let next = db.begin().unwrap();
    assert_eq!(next.read(GET, &[json!(1)]), Ok(Value::Null));
}

struct ObserverCapture(Option<Box<dyn FnOnce() + Send + Sync>>);
impl Drop for ObserverCapture {
    fn drop(&mut self) {
        if let Some(action) = self.0.take() {
            action();
        }
    }
}

#[test]
fn replacing_an_observer_drops_its_capture_outside_the_registration_lock() {
    let db = database();
    let capture = ObserverCapture(Some(Box::new(|| std::panic::panic_any(FAULT))));
    db.tell_on_wait(move || {
        std::hint::black_box(&capture);
    });
    let other = Arc::clone(&db);
    assert!(
        fault_thread(move || {
            other.tell_on_wait(|| {});
        })
        .join()
        .is_err()
    );
    assert!(
        !db.inner.told.is_poisoned(),
        "observer capture is dropped outside registration lock"
    );
    assert!(db.begin().is_ok());
}

#[test]
fn a_replaced_observer_capture_may_register_another_observer() {
    let db = database();
    let weak = Arc::downgrade(&db);
    let capture = ObserverCapture(Some(Box::new(move || {
        weak.upgrade().unwrap().tell_on_wait(|| {});
    })));
    db.tell_on_wait(move || {
        std::hint::black_box(&capture);
    });
    let other = Arc::clone(&db);
    let (sent, heard) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        other.tell_on_wait(|| {});
        let _ = sent.send(());
    });
    assert_eq!(
        heard.recv_timeout(Duration::from_secs(3)),
        Ok(()),
        "captured Drop can reenter registration"
    );
    worker.join().unwrap();
}

#[test]
fn a_terminal_failure_is_synchronized_with_the_check_to_wait_window() {
    let db = database();
    let first = db.begin().unwrap();
    let id = first.id;
    let (held, ready) = mpsc::channel();
    let (go, proceed) = mpsc::channel();
    let (sent, waited) = mpsc::channel();
    let other = Arc::clone(&db);
    let waiter = std::thread::spawn(move || {
        let lease = other.inner.working_of(id).unwrap();
        // The real window after a successful health check, before the atomic wait.
        other.inner.healthy().unwrap();
        held.send(()).unwrap();
        if proceed.recv().is_err() {
            return;
        }
        let result = lease.wait_after_check().map(drop);
        let _ = sent.send(result);
    });
    ready.recv_timeout(Duration::from_secs(3)).unwrap();
    let broken = Arc::clone(&db);
    assert!(
        fault_thread(move || {
            let _locked = broken.inner.published.lock().unwrap();
            std::panic::panic_any(FAULT);
        })
        .join()
        .is_err()
    );
    let (entered, entering) = mpsc::channel();
    let (done, finished) = mpsc::channel();
    let other = Arc::clone(&db);
    let reader = std::thread::spawn(move || {
        let _ = entered.send(());
        let _ = done.send(other.read(GET, &[json!(1)]));
    });
    entering.recv_timeout(Duration::from_secs(3)).unwrap();
    // Failure publication must take working, which the paused waiter owns.
    // No scheduling assumption can make a synchronized producer finish early.
    let early = finished.recv_timeout(Duration::from_millis(200));
    let _ = go.send(());
    let read = match early.as_ref() {
        Ok(result) => Ok(result.clone()),
        Err(_) => finished.recv_timeout(Duration::from_secs(3)),
    };
    let wait = waited.recv_timeout(Duration::from_secs(3));
    if read.is_ok() {
        reader.join().unwrap();
    }
    if wait.is_ok() {
        waiter.join().unwrap();
    }
    assert_eq!(
        early,
        Err(mpsc::RecvTimeoutError::Timeout),
        "failure publication synchronizes with the working mutex"
    );
    assert_eq!(read, Ok(Err(Error::StoragePoisoned)));
    assert_eq!(
        wait,
        Ok(Err(Error::StoragePoisoned)),
        "failure cannot be lost before Condvar::wait"
    );
    drop(first);
}

#[test]
fn an_outer_caller_panic_rolls_back_without_poisoning_storage() {
    for mode in 0..3 {
        let db = database();
        let other = Arc::clone(&db);
        let joined = fault_thread(move || {
            if mode == 2 {
                let _: Result<(), Error> = other.transaction(|command| {
                    command.write(INSERT, &[json!({"id":1})])?;
                    std::panic::panic_any(FAULT);
                });
            } else {
                let mut command = other.begin().unwrap();
                command.write(INSERT, &[json!({"id":1})]).unwrap();
                if mode == 1 {
                    let _prepared = command.prepare().unwrap();
                    std::panic::panic_any(FAULT);
                }
                std::panic::panic_any(FAULT);
            }
        })
        .join();
        assert!(joined.is_err(), "the caller actually unwound");
        let payload = joined.err().unwrap();
        assert_eq!(
            payload.downcast_ref::<&str>(),
            Some(&FAULT),
            "not a setup or product panic"
        );
        assert!(!db.inner.working.is_poisoned() && !db.inner.published.is_poisoned());
        assert_eq!(
            db.read(GET, &[json!(1)]),
            Ok(Value::Null),
            "rollback during outer unwind keeps healthy storage usable"
        );
        let next = db.begin().unwrap();
        assert_eq!(next.read(GET, &[json!(1)]), Ok(Value::Null));
    }
}
