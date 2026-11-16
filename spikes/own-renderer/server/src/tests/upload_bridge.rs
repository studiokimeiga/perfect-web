//! **A program's declared uploads, claimed by its own commands through the
//! server** (ADR-0440, L5b): an image a reader attaches over HTTP is claimed
//! and published by a compiled command, committed with the row that names it
//! in the command's own transaction, and served from the layer's generation
//! storage; a command that discards ends its lease; a commit that keeps
//! nothing gives the lease back; a row's last removal stops the image being
//! served. Each on memory and on PostgreSQL, through the same contract.

use super::tables::Asserted;
use super::uploads::{attach, exchange, get, split};
use super::*;

const IMAGE: &str = r#"module PostImage
upload PostImage
    route "/uploads/post-image"
    serves "/images"
    max_bytes 5_000_000
    types png, jpeg
    max_width 4096
    max_height 4096
"#;

const ASSETS: &str = r#"module Assets
import PostImage
type Item = Item { id: Int, image: Option<PostImage.Ref>, alt: String }
table Assets
    row Item
    key id
    bounded 100
    asset image from PostImage
    written by anyone
"#;

const DATA: &str = r#"module AssetData
import PostImage
import Assets
source AssetData
    holds PostImage, Assets
    transactions serializable
    reads strong
    changes none
    sets one_call
"#;

const APP: &str = r#"module photos.app
import PostImage
import Assets
import Assets.{ Item }
import context.{ current_session }
import capability.{ Session, SessionId }
opaque type InteractionId = String
command Keep(id: Int, alt: String) -> Result<Int,PostImage.Error>
    requires Anyone
    idempotent_by InteractionId
    invalidates Attached(current_session()), Image(id)
    transaction serializable
{
    let claim = PostImage.claim(current_session())?
    let image = PostImage.publish(current_session(),claim)?
    let saved = Assets.put(Item { id: id, image: Some(image), alt: alt })
    Ok(id)
}
command Drop() -> Result<PostImage.Attached,PostImage.Error>
    requires Anyone
    idempotent_by InteractionId
    invalidates Attached(current_session())
    transaction serializable
{
    let claim = PostImage.claim(current_session())?
    PostImage.discard(current_session(),claim)
}
command Remove(id: Int) -> ()
    requires Anyone
    idempotent_by InteractionId
    invalidates Image(id)
    transaction serializable
{
    Assets.delete(id)
}
session query Attached(session: Session<SessionId>) -> List<PostImage.Attached>
    freshness 0.seconds
    consistency read_your_writes
    cache private
    key session
    concurrency one_per_key
    on_key_change cancel
    timeout 2.seconds
{
    PostImage.attached(session)
}
public query Image(id: Int) -> Option<PostImage.Ref>
    freshness 0.seconds
    consistency snapshot
    cache shared
    key id
    concurrency one_per_key
    on_key_change cancel
    timeout 2.seconds
{
    match Assets.get(id) { Some(item) => item.image, None => None }
}
"#;

pub(super) fn built() -> (tempfile::TempDir, std::path::PathBuf) {
    super::tables::built(
        &[],
        &[
            ("PostImage.pw", IMAGE),
            ("Assets.pw", ASSETS),
            ("AssetData.pw", DATA),
            ("app.pw", APP),
        ],
    )
}

/// The program served from memory.
fn on_memory(out: &std::path::Path) -> Server {
    Server::from_build(out.to_path_buf(), out.to_path_buf()).asserted("served from memory")
}

/// A reader's image, as a camera wrote it.
fn png() -> Vec<u8> {
    crate::uploads::tests::png(6, 4)
}

fn keep(s: &Server, session: &str, id: i64, interaction: &str) -> Answered {
    s.command_answer(
        "photos.app.Keep",
        session,
        &[serde_json::json!(id), serde_json::json!("a picture")],
        Some(interaction),
    )
    .asserted("Keep answered")
}

/// The reference row `id` keeps, as the program reads it.
fn image_of(s: &Server, id: i64) -> Option<(String, String)> {
    match s.query("photos.app.Image", "anyone", &[Val::S64(id)]) {
        Ok(Val::Option(Some(found))) => match *found {
            Val::Record(fields) => {
                let field = |name: &str| {
                    fields
                        .iter()
                        .find(|(n, _)| n == name)
                        .map(|(_, v)| v.clone())
                };
                let key = match field("key") {
                    Some(Val::String(key)) => key,
                    other => panic!("a reference's key: {other:?}"),
                };
                let kind = match field("kind") {
                    Some(Val::Variant(kind, None)) => kind,
                    other => panic!("a reference's kind: {other:?}"),
                };
                Some((key, kind))
            }
            other => panic!("a reference: {other:?}"),
        },
        Ok(Val::Option(None)) => None,
        other => panic!("the Image query: {other:?}"),
    }
}

/// A `GET` of `path` with `extra` headers, its head and body.
fn get_with(s: &Server, path: &str, extra: &str) -> (String, Vec<u8>) {
    split(&exchange(
        s,
        format!(
            "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:1\r\nCookie: pw-session=anyone\r\n{extra}\r\n"
        )
        .as_bytes(),
    ))
}

fn etag(head: &str) -> Option<String> {
    head.lines()
        .find_map(|line| line.strip_prefix("etag: "))
        .map(str::to_string)
}

/// **A claimed image is committed with its row, and served**: attached over
/// HTTP, claimed and published by `Keep`, its row naming it; served whole
/// as the layer keeps it, its `ETag` answered `304`; the lease taken, so a
/// second `Keep` claims nothing; the interaction sent again answered as it
/// was, and nothing run again.
fn claimed_image_is_committed_and_served(s: &Server) {
    assert!(
        attach(s, "reader", &png()).contains(" 303 "),
        "the upload staged"
    );
    let kept = s.command_answer(
        "photos.app.Keep",
        "reader",
        &[serde_json::json!(1), serde_json::json!("a picture")],
        Some("keep-1"),
    );
    // Committed, said here (the review of a7c3377b: a helper's `asserted`
    // caught a refused commit first, which is no rule of this test's).
    assert!(matches!(&kept, Ok(answer) if answer.committed), "{kept:?}");
    let Ok(kept) = kept else {
        unreachable!("asserted above")
    };
    // Its claim applied: the lease taken, nothing left pending.
    let leases = s.uploads.leases().expect("staged leases");
    assert!(
        leases.pending_attempts(10).expect("pending").is_empty(),
        "applied"
    );
    let (key, kind) = image_of(s, 1).expect("the row names its image");
    let (head, body) = get(s, &format!("/images/{key}.{kind}"));
    assert!(head.starts_with("http/1.1 200"), "{head}");
    assert_eq!(
        crate::blob::BlobKey::of(&body).hex(),
        key,
        "the image whole"
    );
    let tag = etag(&head).expect("an ETag");
    let (again, _) = get_with(
        s,
        &format!("/images/{key}.{kind}"),
        &format!("If-None-Match: {tag}\r\n"),
    );
    assert!(again.starts_with("http/1.1 304"), "{again}");
    // Its lease is taken: no second image to claim.
    let second = keep(s, "reader", 2, "keep-2");
    assert!(!second.committed, "{second:?}");
    assert!(image_of(s, 2).is_none());
    // The first interaction, sent again, is answered as it was.
    let retried = keep(s, "reader", 1, "keep-1");
    assert_eq!(retried, kept);
    assert_eq!(image_of(s, 1).map(|(k, _)| k), Some(key));
    // And its durable record answers so, not this host's cache of it: what
    // another host, or this one restarted, is answered from (the review of
    // a7c3377b: a retry on one host is its cache's).
    let args = [serde_json::json!(1), serde_json::json!("a picture")];
    let (record, _, sent) = s.receipt_keys("reader", "photos.app.Keep", "keep-1", &args);
    let recorded = s.recalled_interaction(&record).asserted("the record read");
    assert!(
        matches!(&recorded, Some((first, _)) if *first == sent),
        "{recorded:?}"
    );
    let answer = recorded.map(|(_, answer)| answer).unwrap_or_default();
    assert_eq!(Answered::from_browser_receipt(&answer).ok(), Some(kept));
}

/// **A discard ends the lease, and its command's record is kept**: `Drop`
/// claims and discards, writing no row, and commits all the same, so its
/// lease is gone and the interaction sent again is answered from its record.
fn a_discard_ends_the_lease_and_keeps_its_record(s: &Server) {
    assert!(
        attach(s, "reader", &png()).contains(" 303 "),
        "the upload staged"
    );
    let dropped = s
        .command_answer("photos.app.Drop", "reader", &[], Some("drop-1"))
        .asserted("Drop answered");
    assert!(dropped.committed, "{dropped:?}");
    // Ended, neither held nor pending.
    let leases = s.uploads.leases().asserted_some("staged leases");
    assert!(
        leases.pending_attempts(10).asserted("pending").is_empty(),
        "ended, not held"
    );
    // Nothing left to claim.
    let kept = keep(s, "reader", 1, "keep-after-drop");
    assert!(!kept.committed, "{kept:?}");
    let again = s
        .command_answer("photos.app.Drop", "reader", &[], Some("drop-1"))
        .asserted("Drop answered again");
    assert_eq!(again, dropped);
    // Its lease ended, the session stages another image.
    assert!(
        attach(s, "reader", &png()).contains(" 303 "),
        "another image staged"
    );
}

/// **An image no row names any more is not served**: its row removed, the
/// image's address answers `404`.
fn a_removed_rows_image_is_not_served(s: &Server) {
    assert!(
        attach(s, "reader", &png()).contains(" 303 "),
        "the upload staged"
    );
    assert!(keep(s, "reader", 1, "keep-1").committed);
    let (key, kind) = image_of(s, 1).expect("the row names its image");
    let removed = s
        .command_answer(
            "photos.app.Remove",
            "reader",
            &[serde_json::json!(1)],
            Some("remove-1"),
        )
        .asserted("Remove answered");
    assert!(removed.committed, "{removed:?}");
    assert!(image_of(s, 1).is_none());
    let (head, _) = get(s, &format!("/images/{key}.{kind}"));
    assert!(head.starts_with("http/1.1 404"), "{head}");
}

/// **A commit that keeps nothing gives its claim back**: `fail` makes the
/// next commit refuse; the lease is the reader's again, and the next `Keep`
/// commits it.
fn a_refused_commit_gives_its_claim_back(s: &Server, fail: impl FnOnce(&Server)) {
    assert!(
        attach(s, "reader", &png()).contains(" 303 "),
        "the upload staged"
    );
    fail(s);
    let refused = s.command_answer(
        "photos.app.Keep",
        "reader",
        &[serde_json::json!(1), serde_json::json!("a picture")],
        Some("keep-refused"),
    );
    // Refused (PostgreSQL), or unconfirmed (memory, whose host cannot tell a
    // refused publication from a lost one): nothing committed either way,
    // and its rows were never published, so its claim kept nothing.
    assert!(
        matches!(&refused, Ok(answer) if !answer.committed)
            || matches!(&refused, Err(why) if why.contains("unknown")),
        "{refused:?}"
    );
    assert!(image_of(s, 1).is_none());
    let kept = keep(s, "reader", 1, "keep-again");
    assert!(kept.committed, "the lease was given back: {kept:?}");
    assert!(image_of(s, 1).is_some());
}

#[test]
fn a_claimed_image_is_committed_and_served_in_memory() {
    let (_dir, out) = built();
    claimed_image_is_committed_and_served(&on_memory(&out));
}

#[test]
fn a_claimed_image_is_committed_and_served_on_postgresql() {
    let Some(url) = super::tables_pg::database() else {
        return;
    };
    let schema = super::tables_pg::Schema::new(&url, "bridge_served");
    let (_dir, out) = built();
    claimed_image_is_committed_and_served(&super::tables_pg::serve(&out, &schema));
}

#[test]
fn a_discard_ends_the_lease_and_keeps_its_record_in_memory() {
    let (_dir, out) = built();
    a_discard_ends_the_lease_and_keeps_its_record(&on_memory(&out));
}

#[test]
fn a_discard_ends_the_lease_and_keeps_its_record_on_postgresql() {
    let Some(url) = super::tables_pg::database() else {
        return;
    };
    let schema = super::tables_pg::Schema::new(&url, "bridge_discard");
    let (_dir, out) = built();
    a_discard_ends_the_lease_and_keeps_its_record(&super::tables_pg::serve(&out, &schema));
}

#[test]
fn a_removed_rows_image_is_not_served_in_memory() {
    let (_dir, out) = built();
    a_removed_rows_image_is_not_served(&on_memory(&out));
}

#[test]
fn a_removed_rows_image_is_not_served_on_postgresql() {
    let Some(url) = super::tables_pg::database() else {
        return;
    };
    let schema = super::tables_pg::Schema::new(&url, "bridge_removed");
    let (_dir, out) = built();
    a_removed_rows_image_is_not_served(&super::tables_pg::serve(&out, &schema));
}

#[test]
fn a_refused_commit_gives_its_claim_back_in_memory() {
    let (_dir, out) = built();
    a_refused_commit_gives_its_claim_back(&on_memory(&out), |s| {
        *s.before_commit.lock().unwrap() = Some(Box::new(|_| {
            crate::tables::FAIL_NEXT_PUBLISH.with(|fail| fail.set(true));
        }));
    });
}

#[test]
fn a_refused_commit_gives_its_claim_back_on_postgresql() {
    let Some(url) = super::tables_pg::database() else {
        return;
    };
    let schema = super::tables_pg::Schema::new(&url, "bridge_refused");
    let (_dir, out) = built();
    let served = super::tables_pg::serve(&out, &schema);
    a_refused_commit_gives_its_claim_back(&served, |_| {
        // The next insert refused at its commit, once: counted by a
        // sequence, which no rollback takes back.
        schema.sql("CREATE SEQUENCE refusals; CREATE FUNCTION refuse_once() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF nextval(quote_ident(TG_TABLE_SCHEMA) || '.refusals') = 1 THEN RAISE EXCEPTION 'fixture refuses commit'; END IF; RETURN NULL; END $$; CREATE CONSTRAINT TRIGGER refuse_once AFTER INSERT ON \"Assets\" DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION refuse_once()").asserted("refusal installed");
    });
}

/// The program on PostgreSQL behind a proxy that loses the next COMMIT's
/// answer once armed (`before_forward`: before the database sees it).
fn behind_a_losing_proxy(
    schema: &super::tables_pg::Schema,
    out: &std::path::Path,
    before_forward: bool,
) -> (super::tables_pg::LosingProxy, Server) {
    let url = super::tables_pg::database().asserted_some("a database");
    let proxy = super::tables_pg::LosingProxy::start(&url);
    let mut database = schema.database();
    database.url = proxy.url(&database.url);
    let s = super::tables_pg::served_on(out, &database);
    proxy
        .before_forward
        .store(before_forward, std::sync::atomic::Ordering::SeqCst);
    let armed = proxy.armed.clone();
    *s.before_commit.lock().unwrap() = Some(Box::new(move |_| {
        armed.store(true, std::sync::atomic::Ordering::SeqCst);
    }));
    (proxy, s)
}

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

/// **A commit whose answer was lost settles its claim from its own
/// receipt** (ADR-0440, L5b): the COMMIT landed and its answer was lost;
/// once the command holds nothing, the host reads the attempt's raw receipt
/// under the reacquired namespace lock, finds its own nonce and applies the
/// claim. The command is answered committed, the image served, the lease
/// taken, and nothing left pending.
#[test]
fn a_lost_commit_answer_settles_its_claim_from_its_receipt_on_postgresql() {
    if super::tables_pg::database().is_none() {
        return;
    }
    let (_dir, out) = built();
    let schema = super::tables_pg::Schema::new(
        &super::tables_pg::database().asserted_some("a database"),
        "bridge_lost",
    );
    let (proxy, s) = behind_a_losing_proxy(&schema, &out, false);
    assert!(
        attach(&s, "reader", &png()).contains(" 303 "),
        "the upload staged"
    );
    let landed = keep(&s, "reader", 1, "keep-lost");
    assert_eq!(
        proxy.lost.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "the answer lost"
    );
    assert!(landed.committed, "{landed:?}");
    let (key, kind) = image_of(&s, 1).expect("the row names its image");
    let (head, _) = get(&s, &format!("/images/{key}.{kind}"));
    assert!(head.starts_with("http/1.1 200"), "{head}");
    let leases = s.uploads.leases().asserted_some("staged leases");
    assert!(
        leases.pending_attempts(10).asserted("pending").is_empty(),
        "applied"
    );
    assert!(
        !keep(&s, "reader", 2, "keep-after").committed,
        "the lease was taken"
    );
}

/// **A commit that never reached the database holds its claim** (ADR-0440,
/// L5b: absence is never rollback): its COMMIT dropped before the database
/// saw it, the attempt's receipt is absent, which proves nothing (another
/// host may have pruned a committed one), so the claim stays held and
/// pending, and the lease is neither given back nor taken.
#[test]
fn a_commit_that_never_arrived_holds_its_claim_on_postgresql() {
    if super::tables_pg::database().is_none() {
        return;
    }
    let (_dir, out) = built();
    let schema = super::tables_pg::Schema::new(
        &super::tables_pg::database().asserted_some("a database"),
        "bridge_never",
    );
    let (_proxy, s) = behind_a_losing_proxy(&schema, &out, true);
    assert!(
        attach(&s, "reader", &png()).contains(" 303 "),
        "the upload staged"
    );
    let unknown = s.command_answer(
        "photos.app.Keep",
        "reader",
        &[serde_json::json!(1), serde_json::json!("a picture")],
        Some("keep-never"),
    );
    assert!(
        !matches!(&unknown, Ok(answer) if answer.committed),
        "{unknown:?}"
    );
    assert!(image_of(&s, 1).is_none(), "nothing committed");
    let leases = s.uploads.leases().asserted_some("staged leases");
    assert_eq!(
        leases.pending_attempts(10).asserted("pending").len(),
        1,
        "held"
    );
    assert!(
        leases.due_attempts(10).asserted("due").is_empty(),
        "inspected once, for good"
    );
    // Held, not given back: a second Keep claims nothing.
    assert!(!keep(&s, "reader", 2, "keep-held").committed);
}

// ---------------------------------------------------------------------------
// The tests owed before L5b's landing (ADR-0440): another attempt's record
// first, an equal-clock retry by another host, a reconcile the namespace lock
// delayed, a rotated reader, a foreign declaration, a live generation's
// corrupt bytes and a keep that fails.

/// A second image, other than [`png`]'s, so two leases are told apart by
/// what their rows name.
fn other_png() -> Vec<u8> {
    crate::uploads::tests::png(5, 3)
}

/// The program on PostgreSQL on two hosts of one schema, the second shared
/// so that a hook of the first's can run it.
fn two_hosts(
    schema: &super::tables_pg::Schema,
    out: &std::path::Path,
) -> (Server, std::sync::Arc<Server>) {
    (
        super::tables_pg::serve(out, schema),
        std::sync::Arc::new(super::tables_pg::serve(out, schema)),
    )
}

/// **A commit another attempt's record answered first gives its claim
/// back** (ADR-0440, L5b: `Recorded` before the `COMMIT` is a known
/// non-commit of this transaction). Host A's `Keep` finds no record of its
/// interaction; before its authority, host B commits the same interaction
/// with its own lease. A claims and publishes its own, finds B's record at
/// its commit and keeps nothing, and is answered from B's record. A's lease
/// is the reader's again; B's is taken.
#[test]
fn a_commit_another_attempt_recorded_first_gives_its_claim_back_on_postgresql() {
    let Some(url) = super::tables_pg::database() else {
        return;
    };
    let (_dir, out) = built();
    let schema = super::tables_pg::Schema::new(&url, "bridge_recorded");
    let (a, b) = two_hosts(&schema, &out);
    assert!(
        attach(&a, "reader", &png()).contains(" 303 "),
        "staged on A"
    );
    assert!(
        attach(&b, "reader", &other_png()).contains(" 303 "),
        "staged on B"
    );
    let first: std::sync::Arc<std::sync::Mutex<Option<Answered>>> = Default::default();
    let (into, other) = (std::sync::Arc::clone(&first), std::sync::Arc::clone(&b));
    *a.before_command_authority.lock().unwrap() = Some(Box::new(move |_| {
        *into.lock().unwrap() = Some(keep(&other, "reader", 1, "keep-1"));
    }));
    let answered = keep(&a, "reader", 1, "keep-1");
    let first = first
        .lock()
        .unwrap()
        .take()
        .asserted_some("B committed first");
    assert!(first.committed, "{first:?}");
    assert_eq!(answered, first, "A is answered from B's record");
    let leases = a.uploads.leases().asserted_some("A's leases");
    assert!(
        leases.pending_attempts(10).asserted("pending").is_empty(),
        "A's claim settled"
    );
    let (theirs, _) = image_of(&a, 1).asserted_some("row 1 names B's image");
    assert!(
        keep(&a, "reader", 2, "keep-2").committed,
        "A's lease given back"
    );
    let (ours, _) = image_of(&a, 2).asserted_some("row 2 names A's image");
    assert_ne!(ours, theirs, "each host's own image");
    assert!(
        !keep(&b, "reader", 3, "keep-3").committed,
        "B's lease taken"
    );
}

/// **Another attempt's receipt, this attempt's own but for its nonce,
/// settles nothing** (ADR-0440, L5b: a key, a sent body, a deterministic
/// answer and a millisecond name the interaction, never the attempt). Both
/// hosts read one instant. Host A's `Keep` loses its `COMMIT` before the
/// database sees it, so A rolled back. Before A inspects its own receipt,
/// host B retries the interaction with its own lease and commits: B's
/// receipt is what A's would have been in all but its nonce. A holds its
/// claim for good, neither applied nor given back, and answers the
/// interaction from B's receipt.
#[test]
fn another_attempts_receipt_alike_but_for_its_nonce_settles_nothing_on_postgresql() {
    if super::tables_pg::database().is_none() {
        return;
    }
    let (_dir, out) = built();
    let schema = super::tables_pg::Schema::new(
        &super::tables_pg::database().asserted_some("a database"),
        "bridge_equal_clock",
    );
    let (proxy, a) = behind_a_losing_proxy(&schema, &out, true);
    let b = std::sync::Arc::new(super::tables_pg::serve(&out, &schema));
    let instant = a.now();
    *a.frozen_now.lock().unwrap() = Some(instant);
    *b.frozen_now.lock().unwrap() = Some(instant);
    assert!(
        attach(&a, "reader", &png()).contains(" 303 "),
        "staged on A"
    );
    assert!(
        attach(&b, "reader", &other_png()).contains(" 303 "),
        "staged on B"
    );
    let retried: std::sync::Arc<std::sync::Mutex<Option<Answered>>> = Default::default();
    let (into, other) = (std::sync::Arc::clone(&retried), std::sync::Arc::clone(&b));
    *a.before_upload_inspection.lock().unwrap() = Some(Box::new(move |_| {
        *into.lock().unwrap() = Some(keep(&other, "reader", 1, "keep-1"));
    }));
    let answered = a.command_answer(
        "photos.app.Keep",
        "reader",
        &[serde_json::json!(1), serde_json::json!("a picture")],
        Some("keep-1"),
    );
    assert_eq!(
        proxy.lost.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "A's COMMIT dropped"
    );
    let retried = retried.lock().unwrap().take().asserted_some("B retried");
    assert!(retried.committed, "{retried:?}");
    assert_eq!(
        answered.as_ref().ok(),
        Some(&retried),
        "A answers the interaction from B's receipt: {answered:?}"
    );
    let leases = a.uploads.leases().asserted_some("A's leases");
    let pending = leases.pending_attempts(10).asserted("pending");
    assert_eq!(pending.len(), 1, "A's claim held");
    assert!(
        leases.due_attempts(10).asserted("due").is_empty(),
        "for good: its inspection was made"
    );
    // B's receipt is what A's own would have been, but for its nonce.
    let proof = pending[0].proof();
    let kept = a
        .data
        .upload_receipt(&proof.key)
        .asserted("the receipt read")
        .asserted_some("B's receipt");
    assert_eq!(
        (&kept.sent, kept.at_ms),
        (&proof.sent, proof.at_ms),
        "one sent body, one millisecond"
    );
    assert_ne!(kept.answer, proof.answer, "two nonces");
    assert_eq!(
        Answered::from_kept(&kept.answer),
        Answered::from_kept(&proof.answer),
        "one answer"
    );
    // Held, neither given back nor taken twice.
    assert!(!keep(&a, "reader", 2, "keep-2").committed, "A's lease held");
    assert!(image_of(&a, 1).is_some(), "row 1, B's");
    assert!(
        !keep(&b, "reader", 3, "keep-3").committed,
        "B's lease taken"
    );
}

/// The advisory lock an upload namespace's lifetime is held by, as
/// `pw_table_postgres` derives it: FNV-1a of `pw_upload_lifetime:<hex>`.
fn namespace_lock(schema: &super::tables_pg::Schema) -> i64 {
    let recorded = schema.texts("SELECT namespace FROM pw_asset_namespace");
    assert_eq!(recorded.len(), 1, "one recorded namespace: {recorded:?}");
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in format!("pw_upload_lifetime:{}", recorded[0]).as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    i64::from_ne_bytes(h.to_ne_bytes())
}

/// **The upload namespace's lock, held by a session of the test's own** from
/// the host's first inspection of its own receipt until [`Self::given_back`].
struct NamespaceLock {
    key: i64,
    holder: std::sync::Arc<std::sync::Mutex<Option<postgres::Client>>>,
}

impl NamespaceLock {
    fn held_at_inspection(s: &Server, schema: &super::tables_pg::Schema) -> NamespaceLock {
        let key = namespace_lock(schema);
        let holder: std::sync::Arc<std::sync::Mutex<Option<postgres::Client>>> = Default::default();
        let (into, at) = (std::sync::Arc::clone(&holder), schema.database().url);
        *s.before_upload_inspection.lock().unwrap() = Some(Box::new(move |_| {
            let mut held =
                postgres::Client::connect(&at, postgres::NoTls).asserted("a session of the test's");
            held.query_one("SELECT pg_advisory_lock($1)", &[&key])
                .asserted("the namespace's lock held");
            *into.lock().unwrap() = Some(held);
        }));
        NamespaceLock { key, holder }
    }

    fn given_back(&self) {
        let mut held = self
            .holder
            .lock()
            .unwrap()
            .take()
            .asserted_some("the lock held");
        held.query_one("SELECT pg_advisory_unlock($1)", &[&self.key])
            .asserted("the lock given back");
    }
}

/// **A reconcile the namespace lock delayed is made at the next pass**
/// (ADR-0440, L5b). A's `COMMIT` lands and its answer is lost. While A
/// inspects its own receipt another session holds the upload namespace's
/// lock, so the inspection waits out its five seconds and is not made: the
/// claim stays held and due, though the command is answered from its own
/// receipt. The lock given back, the host's periodic pass reads the
/// receipt, finds its own nonce and applies the claim.
#[test]
fn a_reconcile_the_namespace_lock_delayed_is_made_at_the_next_pass_on_postgresql() {
    if super::tables_pg::database().is_none() {
        return;
    }
    let (_dir, out) = built();
    let schema = super::tables_pg::Schema::new(
        &super::tables_pg::database().asserted_some("a database"),
        "bridge_delayed",
    );
    let (proxy, s) = behind_a_losing_proxy(&schema, &out, false);
    assert!(
        attach(&s, "reader", &png()).contains(" 303 "),
        "the upload staged"
    );
    let lock = NamespaceLock::held_at_inspection(&s, &schema);
    let landed = keep(&s, "reader", 1, "keep-delayed");
    assert_eq!(
        proxy.lost.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "the answer lost"
    );
    assert!(
        landed.committed,
        "answered from its own receipt: {landed:?}"
    );
    let leases = s.uploads.leases().asserted_some("staged leases");
    assert_eq!(
        leases.pending_attempts(10).asserted("pending").len(),
        1,
        "held while the lock is"
    );
    assert_eq!(
        leases.due_attempts(10).asserted("due").len(),
        1,
        "and due again"
    );
    lock.given_back();
    s.run_due();
    assert!(
        leases.pending_attempts(10).asserted("pending").is_empty(),
        "applied at the next pass"
    );
    let (image, kind) = image_of(&s, 1).asserted_some("the row names its image");
    let (head, _) = get(&s, &format!("/images/{image}.{kind}"));
    assert!(head.starts_with("http/1.1 200"), "{head}");
    assert!(
        !keep(&s, "reader", 2, "keep-after").committed,
        "the lease taken"
    );
}

/// What `session`'s page is shown of its PostImage lease: how many.
fn attached(s: &Server, session: &str) -> usize {
    match s.query(
        "photos.app.Attached",
        session,
        &[Val::String(crate::identity::session_handle(session))],
    ) {
        Ok(Val::List(shown)) => shown.len(),
        other => panic!("the Attached query: {other:?}"),
    }
}

/// **A lease is its reader's: a reader rotated since staging claims
/// nothing** (ADR-0440, L5b: a claim keeps its declaration's, its reader's
/// and its lease's identity). Staged as the session's guest; the session
/// then signs in, and its reader is the account: the page is shown none,
/// and `Keep` finds no lease of its reader's and commits nothing. Staged
/// again, as the account, the lease is the account's.
fn a_rotated_reader_claims_nothing_staged_before_it(s: &Server) {
    assert!(
        attach(s, "reader", &png()).contains(" 303 "),
        "staged as the guest"
    );
    assert_eq!(attached(s, "reader"), 1, "shown to the guest");
    s.identity.signed_in_for_test("reader", "u-ada", "Ada");
    assert_eq!(attached(s, "reader"), 0, "the account is shown none");
    let rotated = keep(s, "reader", 1, "keep-rotated");
    assert!(!rotated.committed, "{rotated:?}");
    assert!(image_of(s, 1).is_none());
    assert!(
        attach(s, "reader", &png()).contains(" 303 "),
        "staged as the account"
    );
    assert_eq!(attached(s, "reader"), 1, "shown to the account");
    assert!(
        keep(s, "reader", 2, "keep-account").committed,
        "its own lease"
    );
    assert!(image_of(s, 2).is_some());
}

#[test]
fn a_rotated_reader_claims_nothing_staged_before_it_in_memory() {
    let (_dir, out) = built();
    a_rotated_reader_claims_nothing_staged_before_it(&on_memory(&out));
}

#[test]
fn a_rotated_reader_claims_nothing_staged_before_it_on_postgresql() {
    let Some(url) = super::tables_pg::database() else {
        return;
    };
    let schema = super::tables_pg::Schema::new(&url, "bridge_rotated");
    let (_dir, out) = built();
    a_rotated_reader_claims_nothing_staged_before_it(&super::tables_pg::serve(&out, &schema));
}

const AVATAR: &str = r#"module Avatar
upload Avatar
    route "/uploads/avatar"
    serves "/avatars"
    max_bytes 5_000_000
    types png, jpeg
    max_width 4096
    max_height 4096
"#;

const PROFILES: &str = r#"module Profiles
import Avatar
type Profile = Profile { id: Int, avatar: Option<Avatar.Ref> }
table Profiles
    row Profile
    key id
    bounded 100
    asset avatar from Avatar
    written by anyone
"#;

const BOTH_DATA: &str = r#"module AssetData
import PostImage
import Avatar
import Assets
import Profiles
source AssetData
    holds PostImage, Avatar, Assets, Profiles
    transactions serializable
    reads strong
    changes none
    sets one_call
"#;

const AVATARS: &str = r#"module photos.avatars
import Avatar
import Profiles
import Profiles.{ Profile }
import context.{ current_session }
opaque type InteractionId = String
command SetAvatar(id: Int) -> Result<Int,Avatar.Error>
    requires Anyone
    idempotent_by InteractionId
    transaction serializable
{
    let claim = Avatar.claim(current_session())?
    let avatar = Avatar.publish(current_session(),claim)?
    let saved = Profiles.put(Profile { id: id, avatar: Some(avatar) })
    Ok(id)
}
"#;

/// The program with a second upload, Avatar, kept by its own table.
fn built_with_avatars() -> (tempfile::TempDir, std::path::PathBuf) {
    super::tables::built(
        &[],
        &[
            ("PostImage.pw", IMAGE),
            ("Avatar.pw", AVATAR),
            ("Assets.pw", ASSETS),
            ("Profiles.pw", PROFILES),
            ("AssetData.pw", BOTH_DATA),
            ("app.pw", APP),
            ("avatars.pw", AVATARS),
        ],
    )
}

/// A form attaching `file` at `route`, as [`attach`] posts one at
/// PostImage's.
fn attach_at(s: &Server, session: &str, route: &str, file: &[u8]) -> String {
    let (content_type, body) = crate::uploads::tests::form(file, "photo.png", "image/png");
    let mut request = format!(
        "POST {route} HTTP/1.1\r\nHost: 127.0.0.1:1\r\nCookie: pw-session={session}\r\n\
         Origin: http://127.0.0.1:1\r\nSec-Fetch-Site: same-origin\r\nReferer: http://127.0.0.1:1/\r\n\
         Content-Type: {content_type}\r\nContent-Length: {}\r\n\r\n",
        body.len()
    )
    .into_bytes();
    request.extend(body);
    split(&exchange(s, &request)).0
}

/// **A claim through one declaration finds no other's lease** (ADR-0440,
/// L5b: staging keys a lease by its session and its declaration). An avatar
/// staged at Avatar's route; `Keep` claims through PostImage, finds none
/// and commits nothing; the avatar's lease is untouched, and `SetAvatar`
/// claims it through its own declaration.
fn a_claim_through_one_declaration_finds_no_others_lease(s: &Server) {
    assert!(
        attach_at(s, "reader", "/uploads/avatar", &png()).contains(" 303 "),
        "an avatar staged"
    );
    let foreign = keep(s, "reader", 1, "keep-foreign");
    assert!(!foreign.committed, "{foreign:?}");
    assert!(image_of(s, 1).is_none());
    let set = s
        .command_answer(
            "photos.avatars.SetAvatar",
            "reader",
            &[serde_json::json!(1)],
            Some("avatar-1"),
        )
        .asserted("SetAvatar answered");
    assert!(set.committed, "its own declaration claims it: {set:?}");
}

#[test]
fn a_claim_through_one_declaration_finds_no_others_lease_in_memory() {
    let (_dir, out) = built_with_avatars();
    a_claim_through_one_declaration_finds_no_others_lease(&on_memory(&out));
}

#[test]
fn a_claim_through_one_declaration_finds_no_others_lease_on_postgresql() {
    let Some(url) = super::tables_pg::database() else {
        return;
    };
    let schema = super::tables_pg::Schema::new(&url, "bridge_foreign");
    let (_dir, out) = built_with_avatars();
    a_claim_through_one_declaration_finds_no_others_lease(&super::tables_pg::serve(&out, &schema));
}

/// The files under `root` named `key`: a generation's bytes, where
/// LocalBlobs keeps them (`.generic/<namespace>/generations/<generation>`).
fn generation_files(root: &std::path::Path, key: &str) -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();
    let mut dirs = vec![root.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                dirs.push(path);
            } else if path.file_name().is_some_and(|name| name == key) {
                found.push(path);
            }
        }
    }
    found
}

/// **A live generation whose bytes were corrupted is answered `503`, before
/// any `304`** (ADR-0440, L5b: returned bytes are verified before
/// `If-None-Match` is consulted). The image served whole; then its file in
/// the deployment's blob storage is overwritten. A `GET` is `503`, and so is
/// one naming the `ETag` it was served with.
#[test]
fn a_live_generations_corrupt_bytes_are_answered_503_on_postgresql() {
    use std::os::unix::fs::PermissionsExt;
    let Some(url) = super::tables_pg::database() else {
        return;
    };
    let (_dir, out) = built();
    let schema = super::tables_pg::Schema::new(&url, "bridge_corrupt");
    let s = super::tables_pg::serve(&out, &schema);
    assert!(
        attach(&s, "reader", &png()).contains(" 303 "),
        "the upload staged"
    );
    assert!(keep(&s, "reader", 1, "keep-1").committed);
    let (key, kind) = image_of(&s, 1).asserted_some("the row names its image");
    let path = format!("/images/{key}.{kind}");
    let (head, body) = get(&s, &path);
    assert!(head.starts_with("http/1.1 200"), "{head}");
    let tag = etag(&head).asserted_some("an ETag");
    let files = generation_files(&schema.database().blobs, &key);
    assert_eq!(files.len(), 1, "one file keeps the image: {files:?}");
    let mut corrupt = body.clone();
    corrupt[body.len() / 2] ^= 0xff;
    std::fs::set_permissions(&files[0], std::fs::Permissions::from_mode(0o644))
        .asserted("the file made writable");
    std::fs::write(&files[0], &corrupt).asserted("its bytes corrupted");
    let (head, _) = get(&s, &path);
    assert!(head.starts_with("http/1.1 503"), "{head}");
    let (again, _) = get_with(&s, &path, &format!("If-None-Match: {tag}\r\n"));
    assert!(again.starts_with("http/1.1 503"), "before any 304: {again}");
}

/// **A keep that fails keeps nothing and gives its claim back** (ADR-0440,
/// L5b): `fail` makes the next keep of a new generation's bytes fail, so the
/// commit keeps nothing; `mend` lets bytes be kept again. The lease is the
/// reader's again, and the next `Keep` commits it.
fn a_failed_keep_gives_its_claim_back(s: &Server, fail: impl FnOnce(&Server), mend: impl FnOnce()) {
    assert!(
        attach(s, "reader", &png()).contains(" 303 "),
        "the upload staged"
    );
    fail(s);
    let failed = s.command_answer(
        "photos.app.Keep",
        "reader",
        &[serde_json::json!(1), serde_json::json!("a picture")],
        Some("keep-failed"),
    );
    mend();
    // Refused (PostgreSQL), or unconfirmed (memory, whose host cannot tell a
    // refused publication from a lost one): nothing committed either way.
    assert!(
        matches!(&failed, Ok(answer) if !answer.committed)
            || matches!(&failed, Err(why) if why.contains("unknown")),
        "{failed:?}"
    );
    assert!(image_of(s, 1).is_none(), "nothing kept");
    let leases = s.uploads.leases().asserted_some("staged leases");
    assert!(
        leases.pending_attempts(10).asserted("pending").is_empty(),
        "nothing pending"
    );
    let kept = keep(s, "reader", 1, "keep-again");
    assert!(kept.committed, "the lease was given back: {kept:?}");
    assert!(image_of(s, 1).is_some());
}

#[test]
fn a_keep_that_fails_gives_its_claim_back_in_memory() {
    let (_dir, out) = built();
    a_failed_keep_gives_its_claim_back(
        &on_memory(&out),
        |s| {
            *s.before_commit.lock().unwrap() = Some(Box::new(|_| {
                crate::tables::FAIL_NEXT_KEEP.with(|fail| fail.set(true));
            }));
        },
        || (),
    );
}

/// The keep fails as a deployment's storage would: its generations
/// directory, where each new generation's own is made, takes no new one, its
/// mode made read-only. The namespace is still whole, so nothing before the
/// keep fails; a probe holds that the mode binds whoever runs the tests (not
/// root).
#[test]
fn a_keep_that_fails_gives_its_claim_back_on_postgresql() {
    use std::os::unix::fs::PermissionsExt;
    let Some(url) = super::tables_pg::database() else {
        return;
    };
    let (_dir, out) = built();
    let schema = super::tables_pg::Schema::new(&url, "bridge_keep_fails");
    let s = super::tables_pg::serve(&out, &schema);
    let namespaces: Vec<std::path::PathBuf> =
        std::fs::read_dir(schema.database().blobs.join(".generic"))
            .asserted("the deployment's namespaces")
            .flatten()
            .map(|entry| entry.path())
            .collect();
    assert_eq!(namespaces.len(), 1, "one namespace: {namespaces:?}");
    let generations = namespaces[0].join("generations");
    let (shut, open) = (generations.clone(), generations);
    a_failed_keep_gives_its_claim_back(
        &s,
        move |_| {
            std::fs::set_permissions(&shut, std::fs::Permissions::from_mode(0o555))
                .asserted("the generations made read-only");
            let probe = shut.join("probe");
            let refused = std::fs::create_dir(&probe).is_err();
            let _ = std::fs::remove_dir(&probe);
            assert!(
                refused,
                "the generations take no new directory: the tests run as a user their mode binds, not root"
            );
        },
        move || {
            std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o755))
                .asserted("the generations writable again");
        },
    );
}

/// **A claiming commit its ledger refuses before its `COMMIT` is answered
/// refused** (ADR-0440, L5b; the review of a7c3377b). The host's ledger is
/// as full as its proof budget, so `begin_commit` refuses to hold another
/// attempt and no `COMMIT` is sent. Until then the host answered such a
/// command unknown, and kept that answer for its interaction. Its lease is
/// the reader's again, and once the ledger has room, the next `Keep`
/// commits it. The ledger's other refusal, a claim left with no
/// disposition, is the compiler's (PW2005: an affine claim not consumed on
/// every path).
fn a_commit_its_ledger_refuses_is_answered_refused(s: &Server) {
    assert!(
        attach(s, "reader", &png()).contains(" 303 "),
        "the upload staged"
    );
    let leases = s.uploads.leases().asserted_some("staged leases");
    leases.proofs_full_for_test(true);
    let refused = s.command_answer(
        "photos.app.Keep",
        "reader",
        &[serde_json::json!(1), serde_json::json!("a picture")],
        Some("keep-full"),
    );
    leases.proofs_full_for_test(false);
    assert!(
        matches!(&refused, Ok(answer) if !answer.committed
            && answer.why.as_deref().is_some_and(|why| why.contains("waiting for commit confirmation"))),
        "refused, not unknown: {refused:?}"
    );
    assert!(image_of(s, 1).is_none(), "nothing committed");
    assert!(
        keep(s, "reader", 1, "keep-with-room").committed,
        "the lease was given back"
    );
}

#[test]
fn a_commit_its_ledger_refuses_is_answered_refused_in_memory() {
    let (_dir, out) = built();
    a_commit_its_ledger_refuses_is_answered_refused(&on_memory(&out));
}

#[test]
fn a_commit_its_ledger_refuses_is_answered_refused_on_postgresql() {
    let Some(url) = super::tables_pg::database() else {
        return;
    };
    let schema = super::tables_pg::Schema::new(&url, "bridge_full");
    let (_dir, out) = built();
    a_commit_its_ledger_refuses_is_answered_refused(&super::tables_pg::serve(&out, &schema));
}

/// **The periodic pass holds for good a claim whose receipt it finds
/// absent** (ADR-0440, L5b: absence is never rollback; the review of
/// a7c3377b, which found the pass's conclusion held by nothing). A's
/// `COMMIT` is dropped before the database sees it. Its own inspection is
/// delayed by the namespace lock, so the claim stays due. The lock given
/// back, the pass finds no receipt and holds the claim for good: pending
/// still, due no more, neither given back nor taken.
#[test]
fn the_periodic_pass_holds_an_absent_receipts_claim_for_good_on_postgresql() {
    if super::tables_pg::database().is_none() {
        return;
    }
    let (_dir, out) = built();
    let schema = super::tables_pg::Schema::new(
        &super::tables_pg::database().asserted_some("a database"),
        "bridge_absent_pass",
    );
    let (_proxy, s) = behind_a_losing_proxy(&schema, &out, true);
    assert!(
        attach(&s, "reader", &png()).contains(" 303 "),
        "the upload staged"
    );
    let lock = NamespaceLock::held_at_inspection(&s, &schema);
    let unknown = s.command_answer(
        "photos.app.Keep",
        "reader",
        &[serde_json::json!(1), serde_json::json!("a picture")],
        Some("keep-absent"),
    );
    assert!(
        !matches!(&unknown, Ok(answer) if answer.committed),
        "{unknown:?}"
    );
    let leases = s.uploads.leases().asserted_some("staged leases");
    assert_eq!(
        leases.due_attempts(10).asserted("due").len(),
        1,
        "due, its own inspection not made"
    );
    lock.given_back();
    s.run_due();
    assert_eq!(
        leases.pending_attempts(10).asserted("pending").len(),
        1,
        "held"
    );
    assert!(
        leases.due_attempts(10).asserted("due").is_empty(),
        "for good: the pass's inspection was made"
    );
    assert!(image_of(&s, 1).is_none(), "nothing committed");
    assert!(
        !keep(&s, "reader", 2, "keep-after").committed,
        "neither given back nor taken"
    );
}

/// The directory a generation's bytes are kept in, by its key.
fn generation_dir(root: &std::path::Path, key: &str) -> std::path::PathBuf {
    let files = generation_files(root, key);
    assert_eq!(files.len(), 1, "one file keeps the image: {files:?}");
    files[0]
        .parent()
        .map(std::path::Path::to_path_buf)
        .unwrap_or_default()
}

/// **A removed row's generation is collected after its commit** (ADR-0440,
/// L5b; the review of a7c3377b: no test held the layer's collection).
/// `Remove` deletes the row that named the image, and the commit that
/// deletes it collects the generation: its bytes leave the deployment's blob
/// storage at once.
#[test]
fn a_removed_rows_generation_is_collected_after_its_commit_on_postgresql() {
    let Some(url) = super::tables_pg::database() else {
        return;
    };
    let (_dir, out) = built();
    let schema = super::tables_pg::Schema::new(&url, "bridge_collected");
    let s = super::tables_pg::serve(&out, &schema);
    let blobs = schema.database().blobs;
    assert!(
        attach(&s, "reader", &png()).contains(" 303 "),
        "the upload staged"
    );
    assert!(keep(&s, "reader", 1, "keep-1").committed);
    let (key, _) = image_of(&s, 1).asserted_some("the row names its image");
    assert_eq!(generation_files(&blobs, &key).len(), 1, "kept");
    let removed = s
        .command_answer(
            "photos.app.Remove",
            "reader",
            &[serde_json::json!(1)],
            Some("remove-1"),
        )
        .asserted("Remove answered");
    assert!(removed.committed, "{removed:?}");
    assert!(
        generation_files(&blobs, &key).is_empty(),
        "collected at its commit"
    );
}

/// **A generation whose collection failed at its commit is collected by the
/// host's housekeeping** (ADR-0440, L5b; the review of a7c3377b): its
/// directory made read-only while `Remove` commits, the bytes stay, and the
/// job with them. The directory writable again, the next pass collects
/// them.
#[test]
fn a_collection_that_failed_at_its_commit_is_made_by_housekeeping_on_postgresql() {
    use std::os::unix::fs::PermissionsExt;
    let Some(url) = super::tables_pg::database() else {
        return;
    };
    let (_dir, out) = built();
    let schema = super::tables_pg::Schema::new(&url, "bridge_collected_later");
    let s = super::tables_pg::serve(&out, &schema);
    let blobs = schema.database().blobs;
    assert!(
        attach(&s, "reader", &png()).contains(" 303 "),
        "the upload staged"
    );
    assert!(keep(&s, "reader", 1, "keep-1").committed);
    let (key, _) = image_of(&s, 1).asserted_some("the row names its image");
    let dir = generation_dir(&blobs, &key);
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555))
        .asserted("the generation's directory made read-only");
    let probe = dir.join("probe");
    let refused = std::fs::write(&probe, b"").is_err();
    let _ = std::fs::remove_file(&probe);
    assert!(
        refused,
        "the directory refuses a change: the tests run as a user its mode binds, not root"
    );
    let removed = s.command_answer(
        "photos.app.Remove",
        "reader",
        &[serde_json::json!(1)],
        Some("remove-1"),
    );
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755))
        .asserted("the generation's directory writable again");
    assert!(
        matches!(&removed, Ok(answer) if answer.committed),
        "{removed:?}"
    );
    assert_eq!(
        generation_files(&blobs, &key).len(),
        1,
        "its collection failed at the commit"
    );
    s.run_due();
    assert!(
        generation_files(&blobs, &key).is_empty(),
        "collected by housekeeping"
    );
}
