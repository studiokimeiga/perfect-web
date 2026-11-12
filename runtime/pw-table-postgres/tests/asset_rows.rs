//! Actual PostgreSQL rows/registry/storage lifecycle, with trusted proposals and
//! a canonical-byte test store. No session/claim/HTTP acceptance follows.
#![allow(dead_code)]
include!("../../pw-table-memory/tests/support/owned.rs");
include!("../../pw-table-memory/tests/support/assets.rs");
mod common;
use pw_host::assets::{Descriptor, Namespace, Reference, Storage};
use pw_table_postgres::{Interaction, Options, PgTables, Records, Uploads};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, io, sync::Mutex};
type CollectHook = Arc<dyn Fn() + Send + Sync>;
type ReservedBytes = BTreeMap<(Namespace, String), (Descriptor, Option<Vec<u8>>)>;
#[derive(Default)]
struct ByteStore {
    names: Mutex<BTreeSet<Namespace>>,
    kept: Mutex<ReservedBytes>,
    keeps: AtomicUsize,
    collections: AtomicUsize,
    before_collect: Mutex<Option<CollectHook>>,
    keep_delay_ms: AtomicUsize,
    fail_keep: std::sync::atomic::AtomicBool,
    fail_collect: std::sync::atomic::AtomicBool,
}
fn asset_until(mut condition: impl FnMut() -> bool) -> bool {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        if condition() {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}
fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
impl Storage for ByteStore {
    fn enroll(&self, n: &Namespace, fresh: bool) -> io::Result<()> {
        let mut names = self.names.lock().unwrap();
        if fresh {
            if !names.insert(n.clone()) {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "reserved namespace",
                ));
            }
        } else if !names.contains(n) {
            return Err(io::Error::other("missing marker"));
        }
        Ok(())
    }
    fn keep_new(&self, n: &Namespace, d: &Descriptor, bytes: &[u8]) -> io::Result<()> {
        self.keeps.fetch_add(1, Ordering::SeqCst);
        if self.fail_keep.load(Ordering::SeqCst) {
            return Err(io::Error::other("keep refused"));
        }
        if hash(bytes) != d.reference.key || bytes.len() as u64 != d.bytes {
            return Err(io::Error::other("canonical bytes differ"));
        }
        let mut kept = self.kept.lock().unwrap();
        let key = (n.clone(), d.reference.generation.clone());
        if kept.contains_key(&key) {
            return Err(io::Error::other("generation reserved"));
        }
        kept.insert(key, (d.clone(), Some(bytes.to_vec())));
        drop(kept);
        std::thread::sleep(std::time::Duration::from_millis(
            self.keep_delay_ms.load(Ordering::SeqCst) as u64,
        ));
        Ok(())
    }
    fn get_generation(&self, n: &Namespace, d: &Descriptor) -> io::Result<Option<Vec<u8>>> {
        let kept = self.kept.lock().unwrap();
        let Some((descriptor, bytes)) = kept.get(&(n.clone(), d.reference.generation.clone()))
        else {
            return Ok(None);
        };
        if descriptor != d {
            return Err(io::Error::other("immutable descriptor differs"));
        }
        Ok(bytes.clone())
    }
    fn collect_generation(&self, n: &Namespace, d: &Descriptor) -> io::Result<()> {
        self.collections.fetch_add(1, Ordering::SeqCst);
        let hook = self.before_collect.lock().unwrap().clone();
        if let Some(hook) = hook {
            hook();
        }
        if self.fail_collect.load(Ordering::SeqCst) {
            return Err(io::Error::other("collection refused"));
        }
        let mut kept = self.kept.lock().unwrap();
        let Some((descriptor, bytes)) = kept.get_mut(&(n.clone(), d.reference.generation.clone()))
        else {
            return Err(io::Error::other("missing reservation"));
        };
        if descriptor != d {
            return Err(io::Error::other("immutable descriptor differs"));
        }
        *bytes = None;
        Ok(())
    }
}
fn candidate(generation: char) -> Descriptor {
    Descriptor {
        reference: Reference {
            key: hash(b"abc"),
            generation: generation.to_string().repeat(32),
            kind: "png".into(),
            width: 6,
            height: 4,
        },
        bytes: 3,
    }
}
fn row(id: i64, r: Option<&Reference>) -> serde_json::Value {
    json!({"id":id,"image":r,"alt":"actual row"})
}
fn open(s: &common::Scratch, store: &Arc<ByteStore>) -> PgTables {
    let p = asset_program();
    PgTables::open_with_uploads(
        &s.url,
        s.schema(),
        p.tables,
        Options::default(),
        Uploads {
            metadata: p.uploads,
            store: store.clone(),
            fresh_namespace: Namespace::from_bytes([1; 16]),
        },
    )
    .assert_ok("bound adapter")
}
fn record(key: &str) -> Records<'_> {
    Records {
        interaction: Some(Interaction {
            key,
            sent: "canonical command",
            answer: "applied",
            at_ms: 1000,
            forget_before_ms: 0,
        }),
        outbox: &[],
    }
}
fn seed(db: &PgTables) -> Reference {
    let mut c = db
        .begin()
        .assert_ok("namespace lifetime begins before table work");
    let r = c
        .propose_asset("PostImage", candidate('b'), Arc::from(b"abc".as_slice()))
        .assert_ok("verified publication");
    c.write("pw:tables/assets#insert", &[row(1, Some(&r))])
        .assert_ok("row");
    c.prepare()
        .assert_ok("prepare")
        .publish_with(record("seed"))
        .assert_ok("commit row,registry,receipt,bytes");
    r
}
#[test]
fn fresh_publication_keeps_bytes_and_commits_registry_rows_and_actual_receipt_together() {
    let Some(url) = common::database() else {
        return;
    };
    let s = common::Scratch::new(&url, "asset_rows");
    let store = Arc::new(ByteStore::default());
    let db = open(&s, &store);
    let r = seed(&db);
    assert_eq!(store.keeps.load(Ordering::SeqCst), 1);
    assert_eq!(
        s.texts("SELECT declaration || ':' || count::text FROM pw_asset_references"),
        ["PostImage:1"]
    );
    assert_eq!(s.texts("SELECT key FROM pw_interactions"), ["seed"]);
    assert_eq!(
        db.read("pw:tables/assets#get", &[json!(1)])
            .assert_ok("committed row"),
        row(1, Some(&r))
    );
    assert_eq!(
        store
            .get_generation(db.upload_namespace().unwrap(), &candidate('b'))
            .assert_ok("bytes"),
        Some(b"abc".to_vec())
    );
}
#[test]
fn a_missing_real_receipt_refuses_before_keep_and_rolls_back_rows_and_registry() {
    let Some(url) = common::database() else {
        return;
    };
    let s = common::Scratch::new(&url, "asset_receipt");
    let store = Arc::new(ByteStore::default());
    let db = open(&s, &store);
    let mut c = db.begin().assert_ok("begin");
    let r = c
        .propose_asset("PostImage", candidate('b'), Arc::from(b"abc".as_slice()))
        .assert_ok("publication");
    c.write("pw:tables/assets#insert", &[row(1, Some(&r))])
        .assert_ok("row");
    assert!(
        c.commit().is_err(),
        "compiler idempotency does not substitute an actual record"
    );
    assert_eq!(store.keeps.load(Ordering::SeqCst), 0);
    assert_eq!(
        s.texts("SELECT count(*)::text FROM pw_asset_generations"),
        ["0"]
    );
    assert_eq!(
        db.read("pw:tables/assets#get", &[json!(1)])
            .assert_ok("no row"),
        json!(null)
    );
    let c = db.begin().assert_ok("begin claimed/discarded");
    c.require_upload_receipt()
        .assert_ok("actual acquire marker");
    assert!(
        c.commit().is_err(),
        "discarded/held claims also need a receipt even without publication"
    );
}
#[test]
fn plain_changed_and_retired_refs_cannot_create_authority_but_a_current_ref_can_be_copied() {
    let Some(url) = common::database() else {
        return;
    };
    let s = common::Scratch::new(&url, "asset_adoption");
    let store = Arc::new(ByteStore::default());
    let db = open(&s, &store);
    let mut c = db.begin().assert_ok("begin");
    c.write(
        "pw:tables/assets#insert",
        &[row(1, Some(&candidate('b').reference))],
    )
    .assert_ok("structural row");
    assert!(c.prepare().is_err());
    let r = seed(&db);
    let mut bad = r.clone();
    bad.width = 7;
    let mut c = db.begin().assert_ok("begin");
    c.write("pw:tables/assets#put", &[row(1, Some(&bad))])
        .assert_ok("same net count");
    assert!(c.prepare().is_err());
    let mut c = db.begin().assert_ok("begin copy");
    c.write("pw:tables/assets#insert", &[row(2, Some(&r))])
        .assert_ok("copy");
    c.commit().assert_ok("ordinary same-declaration live copy");
    assert_eq!(
        s.texts("SELECT count::text FROM pw_asset_references"),
        ["2"]
    );
    assert_eq!(store.keeps.load(Ordering::SeqCst), 1);
    let mut c = db.begin().assert_ok("begin remove");
    for id in [1, 2] {
        c.write("pw:tables/assets#delete", &[json!(id)])
            .assert_ok("delete");
    }
    c.commit().assert_ok("last refs retire");
    assert_eq!(
        s.texts("SELECT state FROM pw_asset_generations"),
        ["tombstone"]
    );
    assert_eq!(store.collections.load(Ordering::SeqCst), 1);
    let mut c = db.begin().assert_ok("begin stale");
    c.write("pw:tables/assets#insert", &[row(3, Some(&r))])
        .assert_ok("structural stale ref");
    assert!(c.prepare().is_err());
}
#[test]
fn ignored_publication_and_rejected_row_do_not_keep_or_settle_untouched_old_uses() {
    let Some(url) = common::database() else {
        return;
    };
    let s = common::Scratch::new(&url, "asset_ignored");
    let store = Arc::new(ByteStore::default());
    let db = open(&s, &store);
    let c = db.begin().assert_ok("begin");
    c.propose_asset("PostImage", candidate('c'), Arc::from(b"abc".as_slice()))
        .assert_ok("publication");
    let p = c.prepare().assert_ok("prepare ignored");
    let plans = p.asset_plans().assert_ok("plans");
    let plan = plans.first().assert_ok("one plan");
    assert!(!plan.keep_new && plan.used_publications.is_empty());
    p.publish_with(record("ignored"))
        .assert_ok("explicitly retire ignored proposal");
    assert_eq!(store.keeps.load(Ordering::SeqCst), 0);
    assert_eq!(
        s.texts("SELECT state FROM pw_asset_generations"),
        ["tombstone"]
    );
    let r = seed(&db);
    let mut c = db.begin().assert_ok("begin");
    assert_eq!(
        c.propose_asset("PostImage", candidate('d'), Arc::from(b"abc".as_slice()))
            .assert_ok("same bytes"),
        r
    );
    assert!(matches!(
        c.write("pw:tables/assets#insert", &[row(1, Some(&r))]),
        Err(pw_table_postgres::Error::Conflict { .. })
    ));
    let p = c.prepare().assert_ok("prepare conflict");
    assert!(
        p.asset_plans()
            .assert_ok("plans")
            .first()
            .assert_ok("one plan")
            .used_publications
            .is_empty()
    );
    p.publish_with(record("conflict"))
        .assert_ok("settle ignored independently");
    assert_eq!(store.keeps.load(Ordering::SeqCst), 1);
}
#[test]
fn keep_failure_rolls_back_and_collection_failure_preserves_applied_ack_and_a_durable_job() {
    let Some(url) = common::database() else {
        return;
    };
    let s = common::Scratch::new(&url, "asset_io");
    let store = Arc::new(ByteStore::default());
    let db = open(&s, &store);
    store.fail_keep.store(true, Ordering::SeqCst);
    let mut c = db.begin().assert_ok("begin");
    let r = c
        .propose_asset("PostImage", candidate('b'), Arc::from(b"abc".as_slice()))
        .assert_ok("publication");
    c.write("pw:tables/assets#insert", &[row(1, Some(&r))])
        .assert_ok("row");
    assert!(
        c.prepare()
            .assert_ok("prepare")
            .publish_with(record("failed"))
            .is_err()
    );
    assert_eq!(s.texts("SELECT count(*)::text FROM pw_interactions"), ["0"]);
    assert_eq!(
        s.texts("SELECT count(*)::text FROM pw_asset_generations"),
        ["0"]
    );
    store.fail_keep.store(false, Ordering::SeqCst);
    seed(&db);
    store.fail_collect.store(true, Ordering::SeqCst);
    let mut c = db.begin().assert_ok("begin remove");
    c.write("pw:tables/assets#delete", &[json!(1)])
        .assert_ok("remove");
    c.prepare()
        .assert_ok("prepare")
        .publish_with(record("removed"))
        .assert_ok("known commit remains applied when postcommit storage fails");
    assert_eq!(
        s.texts("SELECT key FROM pw_interactions ORDER BY key"),
        ["removed", "seed"]
    );
    assert_eq!(
        s.texts("SELECT generation FROM pw_asset_collections"),
        ["b".repeat(32)]
    );
    assert_eq!(
        s.texts("SELECT state FROM pw_asset_generations"),
        ["tombstone"]
    );
}

fn rewritten_storage_state_refuses(target: &str, late: bool) {
    let Some(url) = common::database() else {
        return;
    };
    let s = common::Scratch::new(&url, "asset_state_rewrite");
    let store = Arc::new(ByteStore::default());
    let db = open(&s, &store);
    let (table, body) = match (target, late) {
        ("descriptor", false) => (
            "pw_asset_generations".to_string(),
            "NEW.width:=7;".to_string(),
        ),
        ("count", false) => (
            "pw_asset_references".to_string(),
            "NEW.count:=2;".to_string(),
        ),
        ("row", false) => (
            "\"Assets\"".to_string(),
            "NEW.image:=jsonb_set(NEW.image::jsonb,'{width}','7'::jsonb)::json;".to_string(),
        ),
        ("deferred-count", true) => (
            "pw_interactions".to_string(),
            format!(
                "UPDATE \"{}\".pw_asset_references SET count=count+1;",
                s.schema
            ),
        ),
        ("count", true) => (
            "pw_interactions".to_string(),
            format!(
                "UPDATE \"{}\".pw_asset_references SET count=count+1;",
                s.schema
            ),
        ),
        ("row", true) => (
            "pw_interactions".to_string(),
            format!(
                "UPDATE \"{}\".\"Assets\" SET image=jsonb_set(image::jsonb,'{{width}}','7'::jsonb)::json;",
                s.schema
            ),
        ),
        _ => unreachable!(),
    };
    let constraint = if target.starts_with("deferred") {
        "CONSTRAINT "
    } else {
        ""
    };
    let timing = if target.starts_with("deferred") {
        "AFTER"
    } else {
        "BEFORE"
    };
    let deferred = if target.starts_with("deferred") {
        " DEFERRABLE INITIALLY DEFERRED"
    } else {
        ""
    };
    s.sql(&format!("CREATE FUNCTION rewrite_storage_state() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN {body} RETURN NEW; END $$; CREATE {constraint}TRIGGER rewrite_storage {timing} INSERT ON {table}{deferred} FOR EACH ROW EXECUTE FUNCTION rewrite_storage_state()" )).assert_ok("valid rewrite hook");
    let mut c = db.begin().assert_ok("begin");
    let r = c
        .propose_asset("PostImage", candidate('b'), Arc::from(b"abc".as_slice()))
        .assert_ok("proposal");
    c.write("pw:tables/assets#insert", &[row(1, Some(&r))])
        .assert_ok("write");
    let refusal = c
        .prepare()
        .and_then(|p| p.publish_with(record("rewritten")));
    assert!(
        refusal.is_err(),
        "{target} rewrite (late={late}) must not commit inconsistent rows/registry: {refusal:?}"
    );
    assert_eq!(
        s.texts("SELECT count(*)::text FROM pw_asset_generations"),
        ["0"]
    );
    assert_eq!(
        s.texts("SELECT count(*)::text FROM pw_asset_references"),
        ["0"]
    );
    assert_eq!(s.texts("SELECT count(*)::text FROM pw_interactions"), ["0"]);
    assert_eq!(
        db.read("pw:tables/assets#get", &[json!(1)])
            .assert_ok("no committed row"),
        json!(null)
    );
    if !late {
        assert_eq!(
            store.keeps.load(Ordering::SeqCst),
            0,
            "prepare catches inconsistent actual storage state before any keep"
        );
    }
    s.sql(&format!("DROP TRIGGER rewrite_storage ON {table}"))
        .assert_ok("valid neighbor");
    let mut c = db.begin().assert_ok("begin");
    let r = c
        .propose_asset("PostImage", candidate('c'), Arc::from(b"abc".as_slice()))
        .assert_ok("fresh retry");
    c.write("pw:tables/assets#insert", &[row(1, Some(&r))])
        .assert_ok("row");
    c.prepare()
        .assert_ok("prepare valid values")
        .publish_with(record("valid"))
        .assert_ok("valid persisted neighbor commits");
}
#[test]
fn actual_generation_descriptors_are_verified_before_keep() {
    rewritten_storage_state_refuses("descriptor", false);
}
#[test]
fn actual_reference_counts_are_verified_before_keep() {
    rewritten_storage_state_refuses("count", false);
}
#[test]
fn actual_row_asset_fields_are_verified_before_keep() {
    rewritten_storage_state_refuses("row", false);
}
#[test]
fn late_receipt_hooks_cannot_change_registry_counts_after_prepare() {
    rewritten_storage_state_refuses("count", true);
}
#[test]
fn late_receipt_hooks_cannot_change_row_asset_fields_after_prepare() {
    rewritten_storage_state_refuses("row", true);
}

#[test]
fn namespace_waiters_hold_no_host_turn_and_begin_no_command_snapshot_before_exclusion() {
    let Some(url) = common::database() else {
        return;
    };
    let s = common::Scratch::new(&url, "asset_lock_order");
    let store = Arc::new(ByteStore::default());
    let first = open(&s, &store);
    let p = asset_program();
    let name = format!("asset_waiters_{}", std::process::id());
    let named = common::named(&url, &name);
    let other = Arc::new(
        PgTables::open_with_uploads(
            &named,
            s.schema(),
            p.tables,
            Options::default(),
            Uploads {
                metadata: p.uploads,
                store: store.clone(),
                fresh_namespace: Namespace::from_bytes([2; 16]),
            },
        )
        .assert_ok("second host"),
    );
    let held = first
        .begin()
        .assert_ok("namespace held, command snapshot still lazy");
    let turns = Arc::new(AtomicUsize::new(0));
    let told = Arc::clone(&turns);
    other.tell_on_wait(move || {
        told.fetch_add(1, Ordering::SeqCst);
    });
    let mut children = Vec::new();
    for _ in 0..2 {
        let other = Arc::clone(&other);
        children.push(std::thread::spawn(move || other.begin().map(drop)));
    }
    assert!(
        asset_until(|| common::waiting_on_advisory(&s, &name) == 2),
        "both writers must wait on namespace before taking the host turn"
    );
    assert_eq!(
        turns.load(Ordering::SeqCst),
        0,
        "no waiter occupies the local writer turn"
    );
    let begin_count=s.texts(&format!("SELECT count(*)::text FROM pg_stat_activity WHERE application_name='{name}' AND state='idle in transaction'"));
    assert_eq!(
        begin_count,
        ["0"],
        "neither command began a snapshot before its namespace lock"
    );
    drop(held);
    for child in children {
        child
            .join()
            .expect("writer exited")
            .assert_ok("waiter completed after exclusion released");
    }
}
#[test]
fn application_schema_migration_waits_on_the_same_namespace_before_ddl() {
    let Some(url) = common::database() else {
        return;
    };
    let s = common::Scratch::new(&url, "asset_migration_lock");
    let store = Arc::new(ByteStore::default());
    let first = open(&s, &store);
    let held = first
        .begin()
        .assert_ok("hold namespace before first table snapshot");
    let mut p = asset_program();
    let field: pw_core::tables::FieldSchema = serde_json::from_value(
        json!({"name":"caption","type":"Option<String>","stored":{"option":{"string":{}}}}),
    )
    .assert_ok("nullable non-asset field");
    p.tables.tables[0].schema.fields.push(field);
    p.tables.hash =
        pw_core::tables::Schema::of(p.tables.tables.iter().map(|t| t.schema.clone()).collect())
            .hash;
    p.tables.migration.to = p.tables.hash.clone();
    let name = format!("asset_migrate_{}", std::process::id());
    let named = common::named(&url, &name);
    let schema = s.schema.clone();
    let provider = store.clone();
    let child = std::thread::spawn(move || {
        PgTables::open_with_uploads(
            &named,
            Some(&schema),
            p.tables,
            Options::default(),
            Uploads {
                metadata: p.uploads,
                store: provider,
                fresh_namespace: Namespace::from_bytes([2; 16]),
            },
        )
    });
    assert!(
        asset_until(|| common::waiting_on_advisory(&s, &name) == 1),
        "migration must wait for active command lifetime"
    );
    assert_eq!(s.texts("SELECT count(*)::text FROM information_schema.columns WHERE table_schema=current_schema() AND column_name='caption'"),["0"]);
    drop(held);
    child
        .join()
        .expect("migration exited")
        .assert_ok("migration after command end");
    assert_eq!(s.texts("SELECT count(*)::text FROM information_schema.columns WHERE table_schema=current_schema() AND column_name='caption'"),["1"]);
}

#[test]
fn a_waiting_namespace_lock_cannot_freeze_the_commands_first_snapshot_before_the_previous_commit() {
    let Some(url) = common::database() else {
        return;
    };
    let s = common::Scratch::new(&url, "asset_fresh_snapshot");
    let store = Arc::new(ByteStore::default());
    let first = open(&s, &store);
    let p = asset_program();
    let name = format!("asset_snapshot_{}", std::process::id());
    let named = common::with_setting(
        &common::named(&url, &name),
        "default_transaction_isolation",
        "serializable",
    );
    let other = PgTables::open_with_uploads(
        &named,
        s.schema(),
        p.tables,
        Options::default(),
        Uploads {
            metadata: p.uploads,
            store: store.clone(),
            fresh_namespace: Namespace::from_bytes([2; 16]),
        },
    )
    .assert_ok("second host with snapshot-forming default");
    let mut held = first.begin().assert_ok("hold namespace");
    held.write("pw:tables/assets#insert", &[row(1, None)])
        .assert_ok("uncommitted row");
    let child = std::thread::spawn(move || {
        let c = other.begin()?;
        c.read("pw:tables/assets#get", &[json!(1)])
    });
    assert!(
        asset_until(|| common::waiting_on_advisory(&s, &name) == 1),
        "second host really waits before previous commit"
    );
    held.commit().assert_ok("previous command commits");
    assert_eq!(
        child
            .join()
            .expect("reader completed")
            .assert_ok("fresh command snapshot"),
        row(1, None),
        "namespace-lock transaction must end before the command BEGIN/snapshot"
    );
}

#[test]
fn final_moves_and_repeated_replacement_do_not_retire_live_bytes_or_use_transient_publications() {
    let Some(url) = common::database() else {
        return;
    };
    let s = common::Scratch::new(&url, "asset_net_moves");
    let store = Arc::new(ByteStore::default());
    let db = open(&s, &store);
    let old = seed(&db);
    for reverse in [false, true] {
        let (from, to) = if reverse { (2, 1) } else { (1, 2) };
        let mut c = db.begin().assert_ok("begin move");
        if reverse {
            c.write("pw:tables/assets#put", &[row(to, Some(&old))])
                .assert_ok("put first");
        }
        c.write("pw:tables/assets#delete", &[json!(from)])
            .assert_ok("delete");
        if !reverse {
            c.write("pw:tables/assets#put", &[row(to, Some(&old))])
                .assert_ok("put last");
        }
        let p = c.prepare().assert_ok("final net move");
        let plans = p.asset_plans().assert_ok("plans");
        let plan = plans.first().assert_ok("one generation");
        assert_eq!(plan.generation.references.get("PostImage"), Some(&1));
        assert!(!plan.collect && !plan.keep_new);
        p.publish()
            .assert_ok("move commits without another claim/receipt");
    }
    let mut c = db.begin().assert_ok("begin repeated same-key writes");
    let mut incoming = candidate('c');
    incoming.reference.key = hash(b"xyz");
    let transient = c
        .propose_asset("PostImage", incoming, Arc::from(b"xyz".as_slice()))
        .assert_ok("fresh different bytes");
    c.write("pw:tables/assets#put", &[row(1, Some(&transient))])
        .assert_ok("transient replacement");
    c.write("pw:tables/assets#put", &[row(1, Some(&old))])
        .assert_ok("restore final old descriptor");
    let p = c.prepare().assert_ok("original/final only");
    let plans = p.asset_plans().assert_ok("plans");
    assert_eq!(plans.len(), 2);
    let unused = plans
        .iter()
        .find(|p| p.generation.descriptor.reference == transient)
        .assert_ok("ignored plan");
    assert!(unused.used_publications.is_empty() && !unused.keep_new);
    p.publish_with(record("restored"))
        .assert_ok("commit restored row");
    assert_eq!(store.keeps.load(Ordering::SeqCst), 1);
    assert_eq!(store.collections.load(Ordering::SeqCst), 0);
    assert_eq!(
        s.texts("SELECT state FROM pw_asset_generations ORDER BY generation"),
        ["live", "tombstone"]
    );
    assert_eq!(
        s.texts("SELECT count::text FROM pw_asset_references"),
        ["1"]
    );
}
#[test]
fn changing_one_generation_preserves_unrelated_live_descriptors_counts_and_bytes() {
    let Some(url) = common::database() else {
        return;
    };
    let s = common::Scratch::new(&url, "asset_other_live");
    let store = Arc::new(ByteStore::default());
    let db = open(&s, &store);
    let old = seed(&db);
    let mut c = db.begin().assert_ok("begin second bytes");
    let mut descriptor = candidate('c');
    descriptor.reference.key = hash(b"xyz");
    let other = c
        .propose_asset(
            "PostImage",
            descriptor.clone(),
            Arc::from(b"xyz".as_slice()),
        )
        .assert_ok("new generation");
    c.write("pw:tables/assets#insert", &[row(2, Some(&other))])
        .assert_ok("second row");
    c.prepare()
        .assert_ok("prepare")
        .publish_with(record("second"))
        .assert_ok("commit");
    let mut c = db.begin().assert_ok("begin remove one");
    c.write("pw:tables/assets#delete", &[json!(1)])
        .assert_ok("remove old");
    c.commit().assert_ok("commit one last removal");
    assert_eq!(
        s.texts("SELECT generation || ':' || count::text FROM pw_asset_references"),
        [format!("{}:1", other.generation)]
    );
    assert_eq!(
        s.texts("SELECT generation FROM pw_asset_generations WHERE state='live'"),
        std::slice::from_ref(&other.generation)
    );
    assert_eq!(
        store
            .get_generation(db.upload_namespace().unwrap(), &descriptor)
            .assert_ok("unrelated actual bytes"),
        Some(b"xyz".to_vec())
    );
    assert!(
        store
            .get_generation(db.upload_namespace().unwrap(), &candidate('b'))
            .assert_ok("old payload gone")
            .is_none()
    );
    assert_eq!(
        db.read("pw:tables/assets#get", &[json!(2)])
            .assert_ok("unrelated row"),
        row(2, Some(&other))
    );
    assert_ne!(old, other);
}
#[test]
fn preparing_one_asset_key_returns_no_unrelated_rows_or_non_asset_payload() {
    let Some(url) = common::database() else {
        return;
    };
    let s = common::Scratch::new(&url, "asset_metadata_cost");
    let store = Arc::new(ByteStore::default());
    let db = open(&s, &store);
    let reference = seed(&db);
    let wide = "non-asset payload".repeat(1000);
    let mut c = db.begin().assert_ok("begin unrelated rows");
    for id in 2..=40 {
        c.write(
            "pw:tables/assets#insert",
            &[json!({"id":id,"image":null,"alt":wide})],
        )
        .assert_ok("unrelated wide row");
    }
    c.commit().assert_ok("commit unrelated payload");
    let mut c = db.begin().assert_ok("begin one touched key");
    c.write(
        "pw:tables/assets#put",
        &[json!({"id":1,"image":reference,"alt":wide})],
    )
    .assert_ok("one wide row");
    let before = (
        db.returned_rows(),
        db.returned_bytes(),
        db.operation_statements(),
    );
    let p = c.prepare().assert_ok("prepare only scoped metadata");
    let work = (
        db.returned_rows() - before.0,
        db.returned_bytes() - before.1,
        db.operation_statements() - before.2,
    );
    assert!(
        work.0 <= 16,
        "one touched reference must not return other table rows: {work:?}"
    );
    assert!(
        work.1 < 4096,
        "registry/asset verification must not fetch the non-asset payload: {work:?}"
    );
    assert!(
        work.2 >= 8,
        "actual registry/projection work remains visible: {work:?}"
    );
    p.publish().assert_ok("commit");
}

fn shaped(s: &common::Scratch, store: &Arc<ByteStore>, dual: bool) -> PgTables {
    let post = r#"module PostImage
upload PostImage
    route "/uploads/photo"
    serves "/photos"
    max_bytes 5_000_000
    types png
    max_width 4096
    max_height 4096
"#;
    let avatar = r#"module Avatar
upload Avatar
    route "/uploads/avatar"
    serves "/avatars"
    max_bytes 100_000
    types png
    max_width 64
    max_height 64
"#;
    let table = if dual {
        r#"module Assets
import PostImage
import Avatar
type Item = Item { id: Int, image: Option<PostImage.Ref>, avatar: Option<Avatar.Ref> }
table Assets
    row Item
    key id
    bounded 100
    asset image from PostImage
    asset avatar from Avatar
"#
    } else {
        r#"module Assets
import PostImage
type Item = Item { group: String, id: Int, image: Option<PostImage.Ref> }
table Assets
    row Item
    key group, id
    bounded 100
    asset image from PostImage
"#
    };
    let source = format!(
        r#"module AssetData
import PostImage
import Assets
{}
source AssetData
    holds PostImage, Assets{}
    transactions serializable
    reads strong
    changes none
    sets one_call
"#,
        if dual { "import Avatar" } else { "" },
        if dual { ", Avatar" } else { "" }
    );
    let mut files = vec![
        ("PostImage.pw", post),
        ("Assets.pw", table),
        ("AssetData.pw", source.as_str()),
    ];
    if dual {
        files.push(("Avatar.pw", avatar));
    }
    let p = built_from(&files);
    PgTables::open_with_uploads(
        &s.url,
        s.schema(),
        p.tables,
        Options::default(),
        Uploads {
            metadata: p.uploads,
            store: store.clone(),
            fresh_namespace: Namespace::from_bytes([1; 16]),
        },
    )
    .assert_ok("compiled asset shape")
}
#[test]
fn declarations_share_live_bytes_only_after_independent_publication_and_keep_current_counts_separate()
 {
    let Some(url) = common::database() else {
        return;
    };
    let s = common::Scratch::new(&url, "asset_declarations");
    let store = Arc::new(ByteStore::default());
    let db = shaped(&s, &store, true);
    let mut c = db.begin().assert_ok("begin photo");
    let r = c
        .propose_asset("PostImage", candidate('b'), Arc::from(b"abc".as_slice()))
        .assert_ok("photo claim");
    c.write(
        "pw:tables/assets#insert",
        &[json!({"id":1,"image":r,"avatar":null})],
    )
    .assert_ok("photo row");
    c.prepare()
        .assert_ok("prepare")
        .publish_with(record("photo"))
        .assert_ok("photo commit");
    let mut c = db.begin().assert_ok("begin forged enrollment");
    c.write(
        "pw:tables/assets#put",
        &[json!({"id":1,"image":r,"avatar":r})],
    )
    .assert_ok("structurally valid row");
    assert!(
        c.prepare().is_err(),
        "Photo current liveness cannot authorize Avatar adoption"
    );
    let mut c = db.begin().assert_ok("begin actual avatar");
    let avatar = c
        .propose_asset("Avatar", candidate('c'), Arc::from(b"abc".as_slice()))
        .assert_ok("independent avatar claim");
    assert_eq!(avatar, r);
    c.write(
        "pw:tables/assets#put",
        &[json!({"id":1,"image":r,"avatar":avatar})],
    )
    .assert_ok("both fields");
    c.prepare()
        .assert_ok("prepare")
        .publish_with(record("avatar"))
        .assert_ok("share current physical generation");
    assert_eq!(store.keeps.load(Ordering::SeqCst), 1);
    assert_eq!(
        s.texts(
            "SELECT declaration || ':' || count::text FROM pw_asset_references ORDER BY declaration"
        ),
        ["Avatar:1", "PostImage:1"]
    );
    let mut c = db.begin().assert_ok("begin remove photo only");
    c.write(
        "pw:tables/assets#put",
        &[json!({"id":1,"image":null,"avatar":avatar})],
    )
    .assert_ok("avatar remains");
    c.commit().assert_ok("declaration count ends");
    assert_eq!(
        s.texts("SELECT declaration || ':' || count::text FROM pw_asset_references"),
        ["Avatar:1"]
    );
    assert_eq!(store.collections.load(Ordering::SeqCst), 0);
    assert!(
        db.read_asset("PostImage", &r.key, "png")
            .assert_ok("photo own count ended")
            .is_none()
    );
    assert_eq!(
        db.read_asset("Avatar", &r.key, "png")
            .assert_ok("avatar remains current"),
        Some((candidate('b'), b"abc".to_vec()))
    );
    let mut c = db.begin().assert_ok("begin stale own adoption");
    c.write(
        "pw:tables/assets#put",
        &[json!({"id":1,"image":r,"avatar":avatar})],
    )
    .assert_ok("structural ref");
    assert!(
        c.prepare().is_err(),
        "Avatar liveness cannot keep Photo adoptable after Photo reaches zero"
    );
    let mut c = db.begin().assert_ok("begin last removal");
    c.write("pw:tables/assets#delete", &[json!(1)])
        .assert_ok("remove avatar");
    c.commit().assert_ok("retire shared bytes once");
    assert_eq!(store.collections.load(Ordering::SeqCst), 1);
}
#[test]
fn prefix_deletion_uses_actual_removed_keys_and_nul_string_keys_remain_indexable() {
    let Some(url) = common::database() else {
        return;
    };
    let s = common::Scratch::new(&url, "asset_prefix");
    let store = Arc::new(ByteStore::default());
    let db = shaped(&s, &store, false);
    let mut c = db.begin().assert_ok("begin");
    let r = c
        .propose_asset("PostImage", candidate('b'), Arc::from(b"abc".as_slice()))
        .assert_ok("publication");
    for (group, id) in [("a\0", 1), ("a\0", 2), ("b", 1)] {
        c.write(
            "pw:tables/assets#insert",
            &[json!({"group":group,"id":id,"image":r})],
        )
        .assert_ok("three refs");
    }
    c.prepare()
        .assert_ok("prepare")
        .publish_with(record("three"))
        .assert_ok("commit");
    let mut c = db.begin().assert_ok("begin prefix remove");
    c.write("pw:tables/assets#delete-of-group", &[json!("a\0")])
        .assert_ok("two matching keys");
    let p = c.prepare().assert_ok("prepare prefix");
    let plans = p.asset_plans().assert_ok("plans");
    let plan = plans.first().assert_ok("one generation");
    assert_eq!(plan.generation.references.get("PostImage"), Some(&1));
    assert!(!plan.collect);
    p.publish().assert_ok("commit group");
    assert_eq!(
        db.read("pw:tables/assets#get", &[json!("b"), json!(1)])
            .assert_ok("untouched group"),
        json!({"group":"b","id":1,"image":r})
    );
    assert_eq!(store.collections.load(Ordering::SeqCst), 0);
    let mut c = db.begin().assert_ok("begin final group");
    c.write("pw:tables/assets#delete-of-group", &[json!("b")])
        .assert_ok("last key");
    c.commit().assert_ok("retire last reference");
    assert_eq!(store.collections.load(Ordering::SeqCst), 1);
    assert_eq!(
        s.texts("SELECT count(*)::text FROM pw_asset_references"),
        ["0"]
    );
}

#[path = "support/losing_proxy.rs"]
mod losing_proxy;
#[test]
fn an_actual_applied_commit_with_a_lost_answer_reports_unknown_and_does_no_collection() {
    let Some(url) = common::database() else {
        return;
    };
    let s = common::Scratch::new(&url, "asset_unknown");
    let proxy = losing_proxy::LosingProxy::start(&url);
    let store = Arc::new(ByteStore::default());
    let via = proxy.url(&url);
    assert!(
        via != url,
        "actual database transport must pass through the loss instrument"
    );
    let p = asset_program();
    let db = PgTables::open_with_uploads(
        &via,
        s.schema(),
        p.tables,
        Options::default(),
        Uploads {
            metadata: p.uploads,
            store: store.clone(),
            fresh_namespace: Namespace::from_bytes([1; 16]),
        },
    )
    .assert_ok("proxied adapter");
    seed(&db);
    let mut c = db.begin().assert_ok("begin removal");
    c.write("pw:tables/assets#delete", &[json!(1)])
        .assert_ok("remove");
    let p = c.prepare().assert_ok("prepared removal");
    proxy.armed.store(true, Ordering::SeqCst);
    let outcome = p.publish_with(record("lost-removal"));
    assert!(
        matches!(outcome, Err(pw_table_postgres::Error::OutcomeUnknown(_))),
        "lost actual COMMIT response remains unknown: {outcome:?}"
    );
    assert_eq!(proxy.lost.load(Ordering::SeqCst), 1);
    assert_eq!(
        s.texts("SELECT key FROM pw_interactions WHERE key='lost-removal'"),
        ["lost-removal"],
        "database really committed"
    );
    assert_eq!(
        s.texts("SELECT state FROM pw_asset_generations"),
        ["tombstone"]
    );
    assert_eq!(
        s.texts("SELECT generation FROM pw_asset_collections"),
        ["b".repeat(32)]
    );
    assert_eq!(
        store.collections.load(Ordering::SeqCst),
        0,
        "unknown COMMIT cannot perform physical GC"
    );
    assert_eq!(
        store
            .get_generation(db.upload_namespace().unwrap(), &candidate('b'))
            .assert_ok("uncertain cleanup retained"),
        Some(b"abc".to_vec())
    );
}
#[test]
fn a_fresh_unknown_commit_keeps_its_bytes_and_durable_record_without_guessing_rollback() {
    let Some(url) = common::database() else {
        return;
    };
    let s = common::Scratch::new(&url, "asset_unknown_fresh");
    let proxy = losing_proxy::LosingProxy::start(&url);
    let store = Arc::new(ByteStore::default());
    let via = proxy.url(&url);
    assert!(via != url);
    let p = asset_program();
    let db = PgTables::open_with_uploads(
        &via,
        s.schema(),
        p.tables,
        Options::default(),
        Uploads {
            metadata: p.uploads,
            store: store.clone(),
            fresh_namespace: Namespace::from_bytes([1; 16]),
        },
    )
    .assert_ok("proxied adapter");
    let mut c = db.begin().assert_ok("begin");
    let r = c
        .propose_asset("PostImage", candidate('b'), Arc::from(b"abc".as_slice()))
        .assert_ok("proposal");
    c.write("pw:tables/assets#insert", &[row(1, Some(&r))])
        .assert_ok("row");
    let p = c.prepare().assert_ok("prepare");
    proxy.armed.store(true, Ordering::SeqCst);
    let outcome = p.publish_with(record("lost-fresh"));
    assert!(
        matches!(outcome, Err(pw_table_postgres::Error::OutcomeUnknown(_))),
        "unknown transport result: {outcome:?}"
    );
    assert_eq!(proxy.lost.load(Ordering::SeqCst), 1);
    assert_eq!(s.texts("SELECT state FROM pw_asset_generations"), ["live"]);
    assert_eq!(
        s.texts("SELECT declaration || ':' || count::text FROM pw_asset_references"),
        ["PostImage:1"]
    );
    assert_eq!(s.texts("SELECT key FROM pw_interactions"), ["lost-fresh"]);
    assert_eq!(store.keeps.load(Ordering::SeqCst), 1);
    assert_eq!(store.collections.load(Ordering::SeqCst), 0);
    assert_eq!(
        db.read("pw:tables/assets#get", &[json!(1)])
            .assert_ok("actual database state"),
        row(1, Some(&r))
    );
    assert_eq!(
        store
            .get_generation(db.upload_namespace().unwrap(), &candidate('b'))
            .assert_ok("kept before uncertain commit"),
        Some(b"abc".to_vec())
    );
}

#[test]
fn the_final_session_clock_fence_runs_after_blocking_keep_on_the_same_single_connection() {
    use pw_table_postgres::{Fence, Lifetimes, Session};
    use std::time::Duration;
    let Some(url) = common::database() else {
        return;
    };
    let s = common::Scratch::new(&url, "asset_late_fence");
    let store = Arc::new(ByteStore::default());
    let p = asset_program();
    let db = PgTables::open_with_uploads(
        &url,
        s.schema(),
        p.tables,
        Options {
            connections: 1,
            wait: Duration::from_millis(200),
            ..Default::default()
        },
        Uploads {
            metadata: p.uploads,
            store: store.clone(),
            fresh_namespace: Namespace::from_bytes([1; 16]),
        },
    )
    .assert_ok("one reserved connection for fence/lifetime/rows/commit");
    let reader = Session {
        user: "ada".into(),
        handle: "@ada".into(),
        name: "Ada".into(),
        issuer: "https://id.example.test".into(),
        epoch: [1; 16],
    };
    let session_hash = vec![7; 32];
    db.open_session(&session_hash, &reader).assert_ok("session");
    let fence = Fence {
        hash: session_hash.clone(),
        expected: Some(reader.clone()),
        lifetimes: Lifetimes {
            idle: Duration::from_secs(86400),
            absolute: Duration::from_secs(1),
        },
    };
    let mut c = db
        .begin_fenced(fence)
        .assert_ok("same lease takes session fence then upload lifetime before first snapshot");
    let r = c
        .propose_asset("PostImage", candidate('b'), Arc::from(b"abc".as_slice()))
        .assert_ok("proposal");
    c.write("pw:tables/assets#insert", &[row(1, Some(&r))])
        .assert_ok("row");
    let p = c.prepare().assert_ok("prepare");
    store.keep_delay_ms.store(1200, Ordering::SeqCst);
    let outcome = p.publish_with(record("expired-during-keep"));
    assert!(
        matches!(&outcome,Err(pw_table_postgres::Error::Refused(why)) if why.contains("session")),
        "expired session cannot commit after a slow physical keep: {outcome:?}"
    );
    assert_eq!(
        store.keeps.load(Ordering::SeqCst),
        1,
        "the keep actually ran before the late refusal"
    );
    assert_eq!(s.texts("SELECT count(*)::text FROM pw_interactions"), ["0"]);
    assert_eq!(
        s.texts("SELECT count(*)::text FROM pw_asset_generations"),
        ["0"]
    );
    assert_eq!(
        db.read("pw:tables/assets#get", &[json!(1)])
            .assert_ok("row rolled back"),
        json!(null)
    );
    assert_eq!(
        store.collections.load(Ordering::SeqCst),
        0,
        "a known rollback performs no GC"
    );
    store.keep_delay_ms.store(0, Ordering::SeqCst);
    db.open_session(&session_hash, &reader)
        .assert_ok("renew session");
    let mut c = db
        .begin_fenced(Fence {
            hash: session_hash,
            expected: Some(reader),
            lifetimes: Lifetimes {
                idle: Duration::from_secs(86400),
                absolute: Duration::from_secs(86400),
            },
        })
        .assert_ok("valid neighbor");
    let r = c
        .propose_asset("PostImage", candidate('c'), Arc::from(b"abc".as_slice()))
        .assert_ok("fresh generation after kept orphan");
    c.write("pw:tables/assets#insert", &[row(1, Some(&r))])
        .assert_ok("row");
    c.prepare()
        .assert_ok("prepare")
        .publish_with(record("valid-after-renewal"))
        .assert_ok("valid session commits on one connection");
}

#[test]
fn authoritative_reads_require_current_own_refs_and_distinguish_missing_live_storage_from_no_ref() {
    let Some(url) = common::database() else {
        return;
    };
    let s = common::Scratch::new(&url, "asset_authoritative");
    let store = Arc::new(ByteStore::default());
    let db = open(&s, &store);
    let r = seed(&db);
    assert_eq!(
        db.read_asset("PostImage", &r.key, "png")
            .assert_ok("LIVE bytes"),
        Some((candidate('b'), b"abc".to_vec()))
    );
    assert!(
        db.read_asset("PostImage", &r.key, "jpeg")
            .assert_ok("wrong kind")
            .is_none()
    );
    let namespace = db.upload_namespace().cloned().assert_ok("namespace");
    store
        .kept
        .lock()
        .unwrap()
        .get_mut(&(namespace.clone(), r.generation.clone()))
        .unwrap()
        .1 = None;
    assert!(
        db.read_asset("PostImage", &r.key, "png").is_err(),
        "LIVE missing storage must never look absent or eligible for304"
    );
    store
        .kept
        .lock()
        .unwrap()
        .get_mut(&(namespace, r.generation.clone()))
        .unwrap()
        .1 = Some(b"abc".to_vec());
    store.fail_collect.store(true, Ordering::SeqCst);
    let mut c = db.begin().assert_ok("begin");
    c.write("pw:tables/assets#delete", &[json!(1)])
        .assert_ok("last row");
    c.commit().assert_ok("tombstone while file remains");
    assert!(
        db.read_asset("PostImage", &r.key, "png")
            .assert_ok("no current references")
            .is_none(),
        "orphan/tombstone physical bytes cannot be served"
    );
    store.fail_collect.store(false, Ordering::SeqCst);
    assert_eq!(db.collect_assets(0).assert_ok("zero bound"), 0);
    assert_eq!(db.collect_assets(1).assert_ok("retry durable cleanup"), 1);
    assert_eq!(
        s.texts("SELECT count(*)::text FROM pw_asset_collections"),
        ["0"]
    );
    assert_eq!(db.collect_assets(1).assert_ok("idempotent retry"), 0);
    let c = db.begin().assert_ok("begin old identity");
    assert!(
        c.propose_asset("PostImage", candidate('b'), Arc::from(b"abc".as_slice()))
            .is_err(),
        "collected descriptor identity remains reserved"
    );
}

#[test]
fn backend_death_after_committed_no_ref_check_cannot_make_old_collection_remove_new_same_hash_bytes()
 {
    use std::sync::{Condvar, mpsc};
    use std::time::Duration;
    let Some(url) = common::database() else {
        return;
    };
    let s = common::Scratch::new(&url, "asset_backend_death");
    let store = Arc::new(ByteStore::default());
    let name = format!("asset_collect_a_{}", std::process::id());
    let named = common::named(&url, &name);
    let p = asset_program();
    let first = PgTables::open_with_uploads(
        &named,
        s.schema(),
        p.tables,
        Options::default(),
        Uploads {
            metadata: p.uploads,
            store: store.clone(),
            fresh_namespace: Namespace::from_bytes([1; 16]),
        },
    )
    .assert_ok("host A");
    let old = seed(&first);
    let second = open(&s, &store);
    let (entered, observed) = mpsc::channel();
    let release = Arc::new((Mutex::new(false), Condvar::new()));
    let wait = Arc::clone(&release);
    let resumed_after_release = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let resumed = Arc::clone(&resumed_after_release);
    *store.before_collect.lock().unwrap() = Some(Arc::new(move || {
        let _ = entered.send(());
        let (flag, ready) = &*wait;
        let held = flag.lock().unwrap();
        let (held, timeout) = ready
            .wait_timeout_while(held, Duration::from_secs(5), |done| !*done)
            .unwrap();
        resumed.store(*held && !timeout.timed_out(), Ordering::SeqCst);
    }));
    let mut c = first.begin().assert_ok("A removes last ref");
    c.write("pw:tables/assets#delete", &[json!(1)])
        .assert_ok("last row");
    let p = c.prepare().assert_ok("A prepared");
    let child = std::thread::spawn(move || p.publish_with(record("removed-before-death")));
    assert!(
        observed.recv_timeout(Duration::from_secs(5)).is_ok(),
        "actual collector reached after its fresh no-ref inspection"
    );
    assert_eq!(
        s.texts("SELECT state FROM pw_asset_generations"),
        ["tombstone"],
        "collector runs only after actual SQL COMMIT"
    );
    assert_eq!(s.texts(&format!("SELECT count(*)::text FROM pg_locks l JOIN pg_stat_activity a USING(pid) WHERE a.application_name='{name}' AND l.locktype='advisory' AND l.granted")),["1"],"the same backend retains namespace exclusion through fresh inspection and physical collection");

    assert_eq!(
        s.texts("SELECT key FROM pw_interactions WHERE key='removed-before-death'"),
        ["removed-before-death"]
    );
    assert_eq!(s.texts(&format!("SELECT pg_terminate_backend(pid)::text FROM pg_stat_activity WHERE application_name='{name}'")),["true"],"terminate the actual backend holding upload lifetime");
    *store.before_collect.lock().unwrap() = None;
    let mut c = second
        .begin()
        .assert_ok("backend death releases exclusion for host B");
    let new = c
        .propose_asset("PostImage", candidate('c'), Arc::from(b"abc".as_slice()))
        .assert_ok("same bytes fresh generation");
    assert_ne!(new.generation, old.generation);
    c.write("pw:tables/assets#insert", &[row(2, Some(&new))])
        .assert_ok("new row");
    c.prepare()
        .assert_ok("B prepared")
        .publish_with(record("republished-after-death"))
        .assert_ok("B commits before old unlink resumes");
    let (flag, ready) = &*release;
    *flag.lock().unwrap() = true;
    ready.notify_all();
    child
        .join()
        .expect("old collector returned")
        .assert_ok("A's known applied ack survives failed postcommit DB completion");
    assert!(
        resumed_after_release.load(Ordering::SeqCst),
        "old unlink must resume after B actually committed and released it; a timed-out schedule earns no credit"
    );
    assert_eq!(
        second
            .read_asset("PostImage", &new.key, "png")
            .assert_ok("authoritative current read"),
        Some((candidate('c'), b"abc".to_vec())),
        "old captured-G unlink preserves the new actual bytes"
    );
    assert_eq!(
        s.texts("SELECT generation FROM pw_asset_collections"),
        [old.generation],
        "old dead connection leaves completion pending"
    );
    assert_eq!(
        second
            .collect_assets(1)
            .assert_ok("bounded retry old capture"),
        1
    );
    assert_eq!(
        second
            .read_asset("PostImage", &new.key, "png")
            .assert_ok("read after retry"),
        Some((candidate('c'), b"abc".to_vec()))
    );
}

#[test]
fn deferred_receipt_hooks_finish_before_final_registry_verification_and_commit() {
    rewritten_storage_state_refuses("deferred-count", true);
}
#[test]
fn two_genuine_cleanup_jobs_obey_zero_one_and_two_limits_and_complete_only_captured_jobs() {
    let Some(url) = common::database() else {
        return;
    };
    let s = common::Scratch::new(&url, "asset_gc_bound");
    let store = Arc::new(ByteStore::default());
    let db = open(&s, &store);
    seed(&db);
    let mut c = db.begin().assert_ok("begin second");
    let mut d = candidate('c');
    d.reference.key = hash(b"xyz");
    let r = c
        .propose_asset("PostImage", d, Arc::from(b"xyz".as_slice()))
        .assert_ok("new bytes");
    c.write("pw:tables/assets#insert", &[row(2, Some(&r))])
        .assert_ok("row");
    c.prepare()
        .assert_ok("prepare")
        .publish_with(record("second"))
        .assert_ok("commit");
    store.fail_collect.store(true, Ordering::SeqCst);
    let mut c = db.begin().assert_ok("begin retire both");
    for id in [1, 2] {
        c.write("pw:tables/assets#delete", &[json!(id)])
            .assert_ok("delete");
    }
    c.commit()
        .assert_ok("both tombstones/jobs commit despite cleanup failure");
    assert_eq!(
        s.texts("SELECT count(*)::text FROM pw_asset_collections"),
        ["2"]
    );
    store.fail_collect.store(false, Ordering::SeqCst);
    assert_eq!(db.collect_assets(0).assert_ok("zero"), 0);
    assert_eq!(
        s.texts("SELECT count(*)::text FROM pw_asset_collections"),
        ["2"]
    );
    assert_eq!(db.collect_assets(1).assert_ok("one"), 1);
    assert_eq!(
        s.texts("SELECT generation FROM pw_asset_collections"),
        ["c".repeat(32)]
    );
    assert_eq!(db.collect_assets(2).assert_ok("remaining"), 1);
    assert_eq!(
        s.texts("SELECT count(*)::text FROM pw_asset_collections"),
        ["0"]
    );
    assert_eq!(
        s.texts("SELECT count(*)::text FROM pw_asset_generations WHERE state='tombstone'"),
        ["2"],
        "retired identities stay reserved"
    );
}
#[test]
fn suppressed_collection_completion_leaves_a_retryable_job_and_never_refuses_the_applied_command() {
    let Some(url) = common::database() else {
        return;
    };
    let s = common::Scratch::new(&url, "asset_gc_completion");
    let store = Arc::new(ByteStore::default());
    let db = open(&s, &store);
    seed(&db);
    s.sql("CREATE FUNCTION suppress_collection_completion() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RETURN NULL; END $$; CREATE TRIGGER suppress_completion BEFORE DELETE ON pw_asset_collections FOR EACH ROW EXECUTE FUNCTION suppress_collection_completion()").assert_ok("suppressed actual completion");
    let mut c = db.begin().assert_ok("begin");
    c.write("pw:tables/assets#delete", &[json!(1)])
        .assert_ok("last row");
    c.prepare()
        .assert_ok("prepare")
        .publish_with(record("removed"))
        .assert_ok("known applied acknowledgement");
    assert_eq!(
        s.texts("SELECT count(*)::text FROM pw_asset_collections"),
        ["1"]
    );
    assert!(
        db.collect_assets(1).is_err(),
        "a suppressed completion cannot be counted as completed"
    );
    assert_eq!(
        s.texts("SELECT count(*)::text FROM pw_asset_collections"),
        ["1"]
    );
    s.sql("DROP TRIGGER suppress_completion ON pw_asset_collections")
        .assert_ok("valid neighbor");
    assert_eq!(db.collect_assets(1).assert_ok("retry"), 1);
}

#[test]
fn reinserted_collection_completion_is_not_counted_as_done_and_preserves_applied_ack() {
    let Some(url) = common::database() else {
        return;
    };
    let s = common::Scratch::new(&url, "asset_gc_reinsert");
    let store = Arc::new(ByteStore::default());
    let db = open(&s, &store);
    seed(&db);
    s.sql(&format!("CREATE FUNCTION reinsert_completion() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN INSERT INTO \"{}\".pw_asset_collections(generation) VALUES(OLD.generation); RETURN OLD; END $$; CREATE TRIGGER reinsert_completion AFTER DELETE ON pw_asset_collections FOR EACH ROW EXECUTE FUNCTION reinsert_completion()",s.schema)).assert_ok("actual completion rewrite hook");
    let mut c = db.begin().assert_ok("begin retirement");
    c.write("pw:tables/assets#delete", &[json!(1)])
        .assert_ok("last row");
    c.prepare()
        .assert_ok("prepare")
        .publish_with(record("removed"))
        .assert_ok("applied ack survives incomplete collection");
    assert_eq!(
        s.texts("SELECT key FROM pw_interactions ORDER BY key"),
        ["removed", "seed"]
    );
    assert_eq!(
        s.texts("SELECT count(*)::text FROM pw_asset_collections"),
        ["1"]
    );
    assert!(
        db.collect_assets(1).is_err(),
        "returned DELETE row alone cannot acknowledge a completion whose captured job remains"
    );
    assert_eq!(
        s.texts("SELECT count(*)::text FROM pw_asset_collections"),
        ["1"]
    );
    s.sql("DROP TRIGGER reinsert_completion ON pw_asset_collections")
        .assert_ok("valid neighbor");
    assert_eq!(db.collect_assets(1).assert_ok("real completion"), 1);
    assert_eq!(
        s.texts("SELECT count(*)::text FROM pw_asset_collections"),
        ["0"]
    );
}

fn rewritten_actual_receipt_refuses(case: &str) {
    let Some(url) = common::database() else {
        return;
    };
    let s = common::Scratch::new(&url, "asset_exact_receipt");
    let store = Arc::new(ByteStore::default());
    let db = open(&s, &store);
    let receipts = format!("\"{}\".pw_interactions", s.schema);
    let (table, timing, constraint, deferred, body) = match case {
        "key" => (
            "pw_interactions",
            "BEFORE",
            "",
            "",
            "NEW.key='different';".to_string(),
        ),
        "sent" => (
            "pw_interactions",
            "BEFORE",
            "",
            "",
            "NEW.sent='different';".to_string(),
        ),
        "answer" => (
            "pw_interactions",
            "BEFORE",
            "",
            "",
            "NEW.answer='different';".to_string(),
        ),
        "instant" => (
            "pw_interactions",
            "BEFORE",
            "",
            "",
            "NEW.at_ms=999;".to_string(),
        ),
        "removed" => (
            "pw_interactions",
            "AFTER",
            "",
            "",
            format!("DELETE FROM {receipts} WHERE key=NEW.key;"),
        ),
        "outbox" => (
            "pw_outbox",
            "AFTER",
            "",
            "",
            format!("UPDATE {receipts} SET answer='different' WHERE key='wanted';"),
        ),
        "deferred" => (
            "pw_interactions",
            "AFTER",
            "CONSTRAINT ",
            " DEFERRABLE INITIALLY DEFERRED",
            format!("DELETE FROM {receipts} WHERE key=NEW.key;"),
        ),
        _ => unreachable!(),
    };
    s.sql(&format!("CREATE FUNCTION rewrite_receipt() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN {body} RETURN NEW; END $$; CREATE {constraint}TRIGGER rewrite_receipt {timing} INSERT ON {table}{deferred} FOR EACH ROW EXECUTE FUNCTION rewrite_receipt()"))
        .assert_ok("qualified actual receipt hook");
    let mut c = db.begin().assert_ok("begin");
    let r = c
        .propose_asset("PostImage", candidate('b'), Arc::from(b"abc".as_slice()))
        .assert_ok("fresh trusted proposal");
    c.write("pw:tables/assets#insert", &[row(1, Some(&r))])
        .assert_ok("row");
    let outbox = [pw_table_postgres::Outboxed {
        kind: pw_table_postgres::OutboxKind::Event,
        name: "changed".into(),
        values: vec![],
    }];
    let mut records = record("wanted");
    if case == "outbox" {
        records.outbox = &outbox;
    }
    let outcome = c
        .prepare()
        .assert_ok("prepared actual rows")
        .publish_with(records);
    assert!(
        outcome.is_err(),
        "{case} hook must not commit without the exact requested durable receipt: {outcome:?}"
    );
    assert_eq!(s.texts("SELECT count(*)::text FROM pw_interactions"), ["0"]);
    assert_eq!(
        s.texts("SELECT count(*)::text FROM pw_asset_generations"),
        ["0"]
    );
    assert_eq!(
        db.read("pw:tables/assets#get", &[json!(1)])
            .assert_ok("rolled back row"),
        json!(null)
    );
    assert_eq!(
        store.keeps.load(Ordering::SeqCst),
        usize::from(case == "deferred"),
        "immediate receipt/outbox hooks refuse before keep; deferred hooks refuse after keep and before COMMIT"
    );
    s.sql(&format!("DROP TRIGGER rewrite_receipt ON {table}"))
        .assert_ok("remove hook");
    let mut c = db.begin().assert_ok("valid neighboring retry");
    let r = c
        .propose_asset("PostImage", candidate('c'), Arc::from(b"abc".as_slice()))
        .assert_ok("fresh generation after failed keep/rollback");
    c.write("pw:tables/assets#insert", &[row(1, Some(&r))])
        .assert_ok("valid row");
    c.prepare()
        .assert_ok("prepare neighbor")
        .publish_with(record("wanted"))
        .assert_ok("exact receipt commits");
    assert_eq!(
        s.texts("SELECT key||':'||sent||':'||answer||':'||at_ms::text FROM pw_interactions"),
        ["wanted:canonical command:applied:1000"]
    );
}
#[test]
fn actual_upload_receipt_key_cannot_be_rewritten() {
    rewritten_actual_receipt_refuses("key");
}
#[test]
fn actual_upload_receipt_sent_cannot_be_rewritten() {
    rewritten_actual_receipt_refuses("sent");
}
#[test]
fn actual_upload_receipt_answer_cannot_be_rewritten() {
    rewritten_actual_receipt_refuses("answer");
}
#[test]
fn actual_upload_receipt_instant_cannot_be_rewritten() {
    rewritten_actual_receipt_refuses("instant");
}
#[test]
fn actual_upload_receipt_cannot_be_removed_by_an_immediate_hook() {
    rewritten_actual_receipt_refuses("removed");
}
#[test]
fn actual_upload_receipt_is_verified_after_outbox_hooks_before_keep() {
    rewritten_actual_receipt_refuses("outbox");
}
#[test]
fn actual_upload_receipt_is_verified_after_deferred_hooks_before_commit() {
    rewritten_actual_receipt_refuses("deferred");
}

#[test]
fn captured_asset_publisher_and_receipt_marker_borrow_only_the_original_pg_command() {
    let Some(url) = common::database() else {
        return;
    };
    let s = common::Scratch::new(&url, "asset_handles");
    let store = Arc::new(ByteStore::default());
    let db = open(&s, &store);
    let mut c = db.begin().assert_ok("original lifetime");
    let publish = c.asset_publisher();
    let marker = c.asset_receipt_marker();
    marker().assert_ok("same command claim marker");
    let image = publish("PostImage", candidate('b'), Arc::from(b"abc".as_slice()))
        .assert_ok("trusted callback on same lease");
    c.write("pw:tables/assets#insert", &[row(1, Some(&image))])
        .assert_ok("callback did not end original owner");
    let p = c.prepare().assert_ok("original prepares");
    assert!(publish("PostImage", candidate('c'), Arc::from(b"abc".as_slice())).is_err());
    assert!(marker().is_err(), "prepared claims cannot be marked again");
    p.publish_with(record("handles"))
        .assert_ok("same original transaction commits");
    let mut next = db.begin().assert_ok("next original turn");
    assert!(
        publish("PostImage", candidate('c'), Arc::from(b"abc".as_slice())).is_err(),
        "stale callback cannot mutate a next command"
    );
    assert!(marker().is_err());
    next.write("pw:tables/assets#insert", &[row(2, Some(&image))])
        .assert_ok("new turn remains valid");
    next.commit()
        .assert_ok("row-only same declaration copy commits");
    assert_eq!(
        s.texts("SELECT count::text FROM pw_asset_references"),
        ["2"]
    );
}
#[test]
fn raw_asset_interaction_inspection_preserves_attempt_fields_and_stored_time_without_age_filter() {
    let Some(url) = common::database() else {
        return;
    };
    let s = common::Scratch::new(&url, "asset_raw_receipt");
    let store = Arc::new(ByteStore::default());
    let db = open(&s, &store);
    let answer = r#"{"committed":true,"result":null,"upload_attempt":"opaque-attempt","extra":"private-answer-marker"}"#;
    let c = db.begin().assert_ok("actual command");
    c.require_upload_receipt()
        .assert_ok("actual acquired/discarded marker");
    c.prepare()
        .assert_ok("prepare")
        .publish_with(Records {
            interaction: Some(Interaction {
                key: "raw",
                sent: "private-sent-marker",
                answer,
                at_ms: 1000,
                forget_before_ms: 0,
            }),
            outbox: &[],
        })
        .assert_ok("actual raw receipt commits");
    assert!(
        db.recalled("raw", 1001)
            .assert_ok("ordinary age filtered receipt")
            .is_none()
    );
    let raw = db
        .inspect_asset_interaction("raw")
        .assert_ok("raw inspection reacquires namespace")
        .assert_ok("actual retained positive proof");
    assert_eq!(raw.sent, "private-sent-marker");
    assert_eq!(raw.answer, answer);
    assert_eq!(raw.at_ms, 1000);
    let debug = format!("{raw:?}");
    assert!(!debug.contains("private-sent-marker"));
    assert!(!debug.contains("private-answer-marker"));
    assert!(!debug.contains("opaque-attempt"));
    assert!(
        db.inspect_asset_interaction("absent")
            .assert_ok("raw negative is only absence")
            .is_none()
    );
}
#[test]
fn raw_asset_interaction_waits_on_namespace_before_query_with_one_connection_and_no_host_turn() {
    use std::sync::mpsc;
    let Some(url) = common::database() else {
        return;
    };
    let s = common::Scratch::new(&url, "asset_raw_wait");
    let store = Arc::new(ByteStore::default());
    let first = open(&s, &store);
    let name = format!("asset_raw_wait_{}", std::process::id());
    let named = common::named(&url, &name);
    let p = asset_program();
    let second = Arc::new(
        PgTables::open_with_uploads(
            &named,
            s.schema(),
            p.tables,
            Options {
                connections: 1,
                wait: std::time::Duration::from_millis(500),
                ..Options::default()
            },
            Uploads {
                metadata: p.uploads,
                store: store.clone(),
                fresh_namespace: Namespace::from_bytes([2; 16]),
            },
        )
        .assert_ok("single connection inspecting host"),
    );
    let held = first.begin().assert_ok("old transaction lifetime retained");
    let turns = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&turns);
    second.tell_on_wait(move || {
        counted.fetch_add(1, Ordering::SeqCst);
    });
    let (done, observed) = mpsc::channel();
    let worker = Arc::clone(&second);
    let child = std::thread::spawn(move || {
        let result = worker.inspect_asset_interaction("pending");
        let _ = done.send(());
        result
    });
    assert!(
        asset_until(|| common::waiting_on_advisory(&s, &name) == 1),
        "raw receipt query must reacquire the old transaction's namespace"
    );
    assert_eq!(
        turns.load(Ordering::SeqCst),
        0,
        "receipt inspection holds no host writer turn"
    );
    assert!(
        observed.try_recv().is_err(),
        "no inspection result precedes ended old transaction"
    );
    drop(held);
    assert!(
        child
            .join()
            .expect("inspection thread exits")
            .assert_ok("same one reserved lease supports lock and read")
            .is_none()
    );
    drop(
        second
            .begin()
            .assert_ok("inspection released namespace and original connection"),
    );
}
