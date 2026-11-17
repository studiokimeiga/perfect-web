//! Actual staged leases + canonical proposals + PG rows/receipts/local files.
//! Direct trusted bridge calls here are not compiled resource/server acceptance.
use super::tables::Asserted;
use super::*;
use crate::uploads as actual_uploads;
trait AssertedSome<T> {
    fn asserted_some(self, why: &str) -> T;
}
impl<T> AssertedSome<T> for Option<T> {
    fn asserted_some(self, why: &str) -> T {
        assert!(self.is_some(), "{why}");
        match self {
            Some(value) => value,
            None => unreachable!(),
        }
    }
}

use crate::uploads::generic::{Invocation, Receipt};
use pw_table_postgres::{Interaction, PgTables, Records};
const KEY: &str = "logical-interaction";
const SENT: &str = "same immutable reader and canonical arguments";
fn leases(build: &std::path::Path, root: &std::path::Path) -> actual_uploads::Uploads {
    let declared = serde_json::from_str(
        &std::fs::read_to_string(build.join("uploads.json"))
            .asserted("compiler upload declarations"),
    )
    .asserted("compiler declarations decode");
    actual_uploads::Uploads::with(
        declared,
        Box::new(crate::blob::MemoryBlobs::default()),
        Box::new(crate::blob::LocalBlobs::new(root).asserted("actual shared physical root")),
        std::time::Duration::from_secs(3600),
    )
}
fn stage_image(u: &actual_uploads::Uploads) {
    let bytes = actual_uploads::tests::png(6, 4);
    let (ty, body) = actual_uploads::tests::form(&bytes, "image.png", "image/png");
    let headers = actual_uploads::tests::headers_for(&ty, body.len());
    assert_eq!(
        u.leases()
            .asserted_some("declared")
            .answer(
                "POST",
                "/uploads/post-image",
                &headers,
                "reader",
                false,
                &mut std::io::Cursor::new(body)
            )
            .status,
        303
    );
}
fn frame(
    db: &PgTables,
    u: &actual_uploads::Uploads,
) -> (
    pw_table_postgres::Prepared,
    Invocation,
    pw_host::assets::Reference,
    String,
) {
    let mut command = db
        .begin()
        .asserted("actual namespace then command transaction");
    let marker = command.asset_receipt_marker();
    let publish = command.asset_publisher();
    let mut inv = Invocation::new(
        u.leases().asserted_some("actual staged leases"),
        "reader",
        identity::ReaderIdentity::Guest,
    );
    let session = identity::session_handle("reader");
    let token = inv
        .claim("PostImage", &session)
        .asserted("actual claim")
        .asserted_some("own canonical lease");
    marker().asserted("acquired/discarded claims require an actual receipt");
    let image = inv
        .publish("PostImage", &session, &token, move |decl, d, bytes| {
            publish(decl, d, bytes).map_err(|e| format!("{e:?}"))
        })
        .asserted("same command publisher callback")
        .asserted_some("published actual lease");
    command
        .write(
            "pw:tables/assets#insert",
            &[serde_json::json!({"id":1,"image":image,"alt":"actual claim"})],
        )
        .asserted("actual row");
    let answer = inv
        .receipt_answer(r#"{"committed":true,"result":null,"why":null,"refused":null}"#)
        .asserted("platform attempt envelope");
    inv.begin_commit(Some(&data::Recorded {
        key: KEY,
        sent: SENT,
        answer: &answer,
        at_ms: 1000,
        forget_before_ms: 0,
    }))
    .asserted("pending before possible COMMIT");
    (
        command
            .prepare()
            .asserted("actual rows and registry prepared"),
        inv,
        image,
        answer,
    )
}
fn records(answer: &str) -> Records<'_> {
    Records {
        interaction: Some(Interaction {
            key: KEY,
            sent: SENT,
            answer,
            at_ms: 1000,
            forget_before_ms: 0,
        }),
        outbox: &[],
    }
}
fn raw(db: &PgTables) -> Option<Receipt> {
    db.inspect_asset_interaction(KEY)
        .asserted("raw actual receipt under reacquired namespace")
        .map(|r| Receipt {
            key: r.key,
            sent: r.sent,
            answer: r.answer,
            at_ms: r.at_ms,
        })
}
#[test]
fn actual_lease_row_bytes_and_attempt_receipt_commit_on_the_same_pg_command() {
    let Some(url) = super::tables_pg::database() else {
        return;
    };
    let schema = super::tables_pg::Schema::new(&url, "claims_actual");
    let (_built, build) = super::upload_generations::build();
    let root = tempfile::TempDir::new().unwrap();
    let disk = Arc::new(super::upload_generations::Disk::new(root.path()));
    let db = super::upload_generations::open(
        &url,
        schema.database().schema.as_deref().unwrap(),
        &build,
        disk,
    );
    let u = leases(&build, root.path());
    stage_image(&u);
    let (prepared, mut inv, image, answer) = frame(&db, &u);
    prepared
        .publish_with(records(&answer))
        .asserted("real row/receipt/file COMMIT");
    inv.applied()
        .asserted("known success consumes actual claim");
    assert!(u.leases().asserted_some("leases").shown("reader").is_none());
    let found = db
        .read_asset("PostImage", &image.key, "png")
        .asserted("authoritative actual file")
        .asserted_some("live physical image");
    assert_eq!(
        found.1,
        actual_uploads::strip::strip(&actual_uploads::tests::png(6, 4))
            .asserted("canonical expected bytes")
    );
    let persisted = raw(&db).asserted_some("raw durable attempt");
    assert_eq!(
        persisted.key, KEY,
        "actual persisted interaction key is preserved"
    );
    assert_eq!(persisted.answer, answer);
    assert!(
        db.inspect_asset_interaction("other-interaction")
            .asserted("other actual key lookup")
            .is_none(),
        "a matching body is never proof for a different interaction key"
    );
}
#[test]
fn lost_actual_commit_reply_keeps_claim_until_raw_nonce_receipt_is_inspected_under_new_lifetime() {
    let Some(url) = super::tables_pg::database() else {
        return;
    };
    let schema = super::tables_pg::Schema::new(&url, "claims_lost");
    let proxy = super::tables_pg::LosingProxy::start(&url);
    let (_built, build) = super::upload_generations::build();
    let root = tempfile::TempDir::new().unwrap();
    let disk = Arc::new(super::upload_generations::Disk::new(root.path()));
    let name = schema.database().schema.unwrap();
    let a = super::upload_generations::open(&proxy.url(&url), &name, &build, disk);
    let b = super::upload_generations::open(
        &url,
        &name,
        &build,
        Arc::new(super::upload_generations::Disk::new(root.path())),
    );
    let u = leases(&build, root.path());
    stage_image(&u);
    let (prepared, inv, image, answer) = frame(&a, &u);
    proxy.armed.store(true, std::sync::atomic::Ordering::SeqCst);
    let outcome = prepared.publish_with(records(&answer));
    assert!(
        matches!(outcome, Err(pw_table_postgres::Error::OutcomeUnknown(_))),
        "real unanswered COMMIT stays uncertain: {outcome:?}"
    );
    drop(inv);
    assert_eq!(proxy.lost.load(std::sync::atomic::Ordering::SeqCst), 1);
    let held = u.leases().asserted_some("actual local ledger");
    let ticket = held
        .pending_attempts(1)
        .asserted("pending")
        .into_iter()
        .next()
        .asserted_some("actual unanswered attempt held");
    assert!(
        b.recalled(KEY, 1001)
            .asserted("ordinary browser replay age policy")
            .is_none()
    );
    let proof = raw(&b).asserted_some("positive raw proof survives the age-filtered replay API");
    assert_eq!(proof.answer, answer);
    assert!(
        held.reconcile_attempt(&ticket, Some(&proof))
            .asserted("matching actual raw attempt settles the local claim")
    );
    assert!(held.shown("reader").is_none());
    assert!(held.pending_attempts(1).asserted("settled").is_empty());
    assert!(
        b.read_asset("PostImage", &image.key, "png")
            .asserted("actual committed bytes remain")
            .is_some()
    );
}
#[test]
fn unknown_rolled_back_then_other_host_equal_clock_retry_cannot_settle_the_old_local_claim() {
    let Some(url) = super::tables_pg::database() else {
        return;
    };
    let schema = super::tables_pg::Schema::new(&url, "claims_retry");
    let proxy = super::tables_pg::LosingProxy::start(&url);
    let (_built, build) = super::upload_generations::build();
    let root = tempfile::TempDir::new().unwrap();
    let disk = Arc::new(super::upload_generations::Disk::new(root.path()));
    let name = schema.database().schema.unwrap();
    let a = super::upload_generations::open(&proxy.url(&url), &name, &build, disk);
    let b = super::upload_generations::open(
        &url,
        &name,
        &build,
        Arc::new(super::upload_generations::Disk::new(root.path())),
    );
    let ua = leases(&build, root.path());
    let ub = leases(&build, root.path());
    stage_image(&ua);
    stage_image(&ub);
    let (prepared, inv, old, answer_a) = frame(&a, &ua);
    proxy
        .before_forward
        .store(true, std::sync::atomic::Ordering::SeqCst);
    proxy.armed.store(true, std::sync::atomic::Ordering::SeqCst);
    let outcome = prepared.publish_with(records(&answer_a));
    assert!(
        matches!(outcome, Err(pw_table_postgres::Error::OutcomeUnknown(_))),
        "client cannot know pre-forward rollback: {outcome:?}"
    );
    drop(inv);
    assert!(
        raw(&b).is_none(),
        "reacquired namespace after actual backend rollback has no receipt"
    );
    assert_eq!(schema.texts("SELECT count(*)::text FROM \"Assets\""), ["0"]);
    let (prepared, mut inv, new, answer_b) = frame(&b, &ub);
    assert_ne!(old.generation, new.generation);
    assert_ne!(answer_a, answer_b);
    let decoded_a = Answered::from_browser_receipt(&answer_a).asserted("logical A answer");
    let decoded_b = Answered::from_browser_receipt(&answer_b).asserted("logical B answer");
    assert_eq!(
        decoded_a, decoded_b,
        "same logical result and clock must still have distinct attempt proof"
    );
    prepared
        .publish_with(records(&answer_b))
        .asserted("other host equal key/sent/time actually commits");
    inv.applied().asserted("own known-applied B claim ends");
    let held = ua.leases().asserted_some("A local ledger");
    let ticket = held
        .pending_attempts(1)
        .asserted("pending")
        .into_iter()
        .next()
        .asserted_some("A pending");
    let proof = raw(&b).asserted_some("B actual durable winner");
    assert!(
        !held
            .reconcile_attempt(&ticket, Some(&proof))
            .asserted("B receipt is not A attempt proof")
    );
    assert_eq!(held.pending_attempts(1).asserted("A unresolved").len(), 1);
    assert!(
        b.read_asset("PostImage", &new.key, "png")
            .asserted("winner actual bytes")
            .is_some()
    );
}
#[test]
fn another_hosts_fast_pruning_cannot_turn_a_committed_missing_receipt_into_rollback_evidence() {
    let Some(url) = super::tables_pg::database() else {
        return;
    };
    let schema = super::tables_pg::Schema::new(&url, "claims_pruned");
    let proxy = super::tables_pg::LosingProxy::start(&url);
    let (_built, build) = super::upload_generations::build();
    let root = tempfile::TempDir::new().unwrap();
    let disk = Arc::new(super::upload_generations::Disk::new(root.path()));
    let name = schema.database().schema.unwrap();
    let a = super::upload_generations::open(&proxy.url(&url), &name, &build, disk);
    let b = super::upload_generations::open(
        &url,
        &name,
        &build,
        Arc::new(super::upload_generations::Disk::new(root.path())),
    );
    let u = leases(&build, root.path());
    stage_image(&u);
    let (prepared, inv, image, answer) = frame(&a, &u);
    proxy.armed.store(true, std::sync::atomic::Ordering::SeqCst);
    let outcome = prepared.publish_with(records(&answer));
    assert!(matches!(
        outcome,
        Err(pw_table_postgres::Error::OutcomeUnknown(_))
    ));
    drop(inv);
    assert_eq!(raw(&b).asserted_some("A actually committed").answer, answer);
    let command = b
        .begin()
        .asserted("other host starts later in namespace order");
    command
        .prepare()
        .asserted("no new assets needed to prune old receipts")
        .publish_with(Records {
            interaction: Some(Interaction {
                key: "different-interaction",
                sent: "other arguments",
                answer: "applied",
                at_ms: 172_800_001,
                forget_before_ms: 86_400_001,
            }),
            outbox: &[],
        })
        .asserted("fast clock actually prunes A");
    assert!(raw(&b).is_none());
    assert!(
        b.read_asset("PostImage", &image.key, "png")
            .asserted("A committed row and actual bytes remain LIVE")
            .is_some()
    );
    let held = u.leases().asserted_some("actual local pending");
    let ticket = held
        .pending_attempts(1)
        .asserted("pending")
        .into_iter()
        .next()
        .asserted_some("A unresolved");
    assert!(
        !held
            .reconcile_attempt(&ticket, None)
            .asserted("missing receipt is not retained-negative proof")
    );
    assert_eq!(
        held.pending_attempts(1)
            .asserted("pending remains held")
            .len(),
        1
    );
}

#[test]
fn actual_nonce_rewrite_refuses_before_physical_keep_and_known_rollback_returns_its_lease() {
    let Some(url) = super::tables_pg::database() else {
        return;
    };
    let schema = super::tables_pg::Schema::new(&url, "claims_nonce_rewrite");
    let (_built, build) = super::upload_generations::build();
    let root = tempfile::TempDir::new().unwrap();
    let disk = Arc::new(super::upload_generations::Disk::new(root.path()));
    let db = super::upload_generations::open(
        &url,
        schema.database().schema.as_deref().unwrap(),
        &build,
        disk,
    );
    let u = leases(&build, root.path());
    stage_image(&u);
    schema.sql("CREATE FUNCTION erase_upload_attempt() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN NEW.answer=(NEW.answer::jsonb-'upload_attempt')::text; RETURN NEW; END $$; CREATE TRIGGER erase_upload_attempt BEFORE INSERT ON pw_interactions FOR EACH ROW EXECUTE FUNCTION erase_upload_attempt()").asserted("actual raw receipt rewrite hook");
    let (prepared, mut inv, _image, answer) = frame(&db, &u);
    let refused = prepared.publish_with(records(&answer));
    assert!(
        matches!(refused, Err(pw_table_postgres::Error::Refused(_))),
        "receipt attempt rewrite is known refusal: {refused:?}"
    );
    inv.refused()
        .asserted("known rollback returns the exact staged lease");
    assert!(u.leases().asserted_some("ledger").shown("reader").is_some());
    assert_eq!(schema.texts("SELECT count(*)::text FROM \"Assets\""), ["0"]);
    assert!(raw(&db).is_none());
    let physical = root
        .path()
        .join(".generic")
        .join(db.upload_namespace().asserted_some("namespace").hex())
        .join("generations");
    assert_eq!(
        std::fs::read_dir(physical)
            .asserted("real generation reservations")
            .count(),
        0,
        "known receipt rewrite refuses BEFORE any keep/reservation"
    );
    schema
        .sql("DROP TRIGGER erase_upload_attempt ON pw_interactions")
        .asserted("valid retry neighbor");
    let (prepared, mut inv, _, answer) = frame(&db, &u);
    prepared
        .publish_with(records(&answer))
        .asserted("valid actual raw attempt commits");
    inv.applied().asserted("valid known commit consumes lease");
}
