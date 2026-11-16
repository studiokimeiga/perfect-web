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

fn built() -> (tempfile::TempDir, std::path::PathBuf) {
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
    let kept = keep(s, "reader", 1, "keep-1");
    assert!(kept.committed, "{kept:?}");
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
    // Nothing left to claim.
    let kept = keep(s, "reader", 1, "keep-after-drop");
    assert!(!kept.committed, "{kept:?}");
    let again = s
        .command_answer("photos.app.Drop", "reader", &[], Some("drop-1"))
        .asserted("Drop answered again");
    assert_eq!(again, dropped);
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
