//! **One contract for assets, on memory and on PostgreSQL alike** (ADR-0440,
//! amended 2026-11-17; L5a's review, its M3: "Memory and PostgreSQL not
//! alike"). L5b gave memory its keep, its collection and its verified read,
//! and left the suite that holds the two layers to one contract to NEXT. Each
//! case below is one scenario, run through the server on each layer
//! (`_in_memory`, `_on_postgresql`), to the same expected answers: what a
//! reader is served, and what the layer keeps. Where the two keep bytes
//! differently (memory's storage, PostgreSQL's blob root), each layer's
//! [`Layered`] reaches its own to lose, change or look at a generation.
//!
//! What it adds: memory's collection at a commit and by its housekeeping,
//! its verified read (bytes lost or changed: `503`, before any `304`), the
//! same bytes kept again as a new generation, and one generation two fields
//! of a row name, kept while either does, through a compiled command (a row
//! past 16 flat values, ADR-0266, amended), each held on PostgreSQL too,
//! where `upload_bridge` held three of them on PostgreSQL alone (those three
//! are this suite's now). What both layers already shared stays in
//! `upload_bridge`, run on each: a claimed image committed and served, a
//! removed row's image not served, a discarded lease, a rotated reader, a
//! foreign declaration's lease, and a commit refused before its rows were
//! published (`a_refused_commit_gives_its_claim_back`,
//! `a_failed_keep_gives_its_claim_back`), whose answer this suite found
//! unlike: memory said "unknown" where PostgreSQL said refused. Both now
//! say refused, and those two tests require it.

use super::tables::Asserted;
use super::upload_bridge::{etag, generation_dir, generation_files, get_with, png};
use super::uploads::{attach, get};
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

/// The suite's table, two images to a row: an id, two `Option<Ref>` (eight
/// flat values each) and its words are 19, past the Canonical ABI's 16, so
/// each `put` passes its arguments in memory (ADR-0266, amended 2026-11-17).
/// Until then no compiled command could write this row, and the suite's kept
/// one image.
const ASSETS: &str = r#"module Assets
import PostImage
type Item = Item { id: Int, image: Option<PostImage.Ref>, thumb: Option<PostImage.Ref>, alt: String }
table Assets
    row Item
    key id
    bounded 100
    asset image from PostImage
    asset thumb from PostImage
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
    invalidates Attached(current_session()), Image(id), Thumb(id), Alt(id)
    transaction serializable
{
    let claim = PostImage.claim(current_session())?
    let image = PostImage.publish(current_session(),claim)?
    let saved = Assets.put(Item { id: id, image: Some(image), thumb: None, alt: alt })
    Ok(id)
}
command KeepBoth(id: Int, alt: String) -> Result<Int,PostImage.Error>
    requires Anyone
    idempotent_by InteractionId
    invalidates Attached(current_session()), Image(id), Thumb(id), Alt(id)
    transaction serializable
{
    let claim = PostImage.claim(current_session())?
    let image = PostImage.publish(current_session(),claim)?
    let saved = Assets.put(Item { id: id, image: Some(image), thumb: Some(image), alt: alt })
    Ok(id)
}
command Unimage(id: Int) -> Int
    requires Anyone
    idempotent_by InteractionId
    invalidates Image(id), Thumb(id), Alt(id)
    transaction serializable
{
    let thumb = match Assets.get(id) { Some(item) => item.thumb, None => None }
    let alt = match Assets.get(id) { Some(item) => item.alt, None => "" }
    let saved = Assets.put(Item { id: id, image: None, thumb: thumb, alt: alt })
    id
}
command Unthumb(id: Int) -> Int
    requires Anyone
    idempotent_by InteractionId
    invalidates Image(id), Thumb(id), Alt(id)
    transaction serializable
{
    let image = match Assets.get(id) { Some(item) => item.image, None => None }
    let alt = match Assets.get(id) { Some(item) => item.alt, None => "" }
    let saved = Assets.put(Item { id: id, image: image, thumb: None, alt: alt })
    id
}
command Remove(id: Int) -> ()
    requires Anyone
    idempotent_by InteractionId
    invalidates Image(id), Thumb(id), Alt(id)
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
public query Thumb(id: Int) -> Option<PostImage.Ref>
    freshness 0.seconds
    consistency snapshot
    cache shared
    key id
    concurrency one_per_key
    on_key_change cancel
    timeout 2.seconds
{
    match Assets.get(id) { Some(item) => item.thumb, None => None }
}
public query Alt(id: Int) -> Option<String>
    freshness 0.seconds
    consistency snapshot
    cache shared
    key id
    concurrency one_per_key
    on_key_change cancel
    timeout 2.seconds
{
    match Assets.get(id) { Some(item) => Some(item.alt), None => None }
}
"#;

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

/// **The layer under test, and its hands on what it keeps**: memory's
/// storage, held by the layer the server serves; PostgreSQL's, the files
/// under its blob root.
enum Keeps {
    Memory(Arc<crate::tables::TablesLayer>),
    Postgres(std::path::PathBuf),
}

/// **A server on one layer**, and that layer's storage.
struct Layered {
    server: Server,
    keeps: Keeps,
    /// Kept for its life: PostgreSQL's schema, dropped with it.
    _schema: Option<super::tables_pg::Schema>,
}

impl Layered {
    fn in_memory(out: &std::path::Path) -> Layered {
        let contracts = std::fs::read_to_string(out.join("contracts.json"))
            .map_err(|e| e.to_string())
            .and_then(|text| ComponentContract::from_json(&text).map_err(|e| e.to_string()))
            .asserted("contracts.json");
        let layer = Arc::new(
            crate::tables::TablesLayer::from_build(out, &contracts).asserted("the memory layer"),
        );
        let served: Arc<dyn data::DataLayer> = layer.clone();
        let server = Server::from_build_tables(out.to_path_buf(), out.to_path_buf(), served)
            .asserted("served from memory");
        Layered {
            server,
            keeps: Keeps::Memory(layer),
            _schema: None,
        }
    }

    /// None where no database is named: the PostgreSQL half returns.
    fn on_postgresql(out: &std::path::Path, name: &str) -> Option<Layered> {
        let url = super::tables_pg::database()?;
        let schema = super::tables_pg::Schema::new(&url, name);
        let server = super::tables_pg::serve(out, &schema);
        let blobs = schema.database().blobs;
        Some(Layered {
            server,
            keeps: Keeps::Postgres(blobs),
            _schema: Some(schema),
        })
    }

    /// The generation the committed row `id` names (`Image`), by its key and
    /// kind.
    fn named(&self, query: &str, id: i64) -> Option<(String, String)> {
        match self
            .server
            .query(&format!("photos.app.{query}"), "anyone", &[Val::S64(id)])
        {
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
            other => panic!("the {query} query: {other:?}"),
        }
    }

    /// Whether the layer keeps bytes of the generation `generation`: in
    /// memory's storage, or in its own directory under PostgreSQL's blob root.
    fn kept(&self, key: &str, generation: Option<&str>) -> bool {
        let generation = generation.asserted_some("a generation");
        match &self.keeps {
            Keeps::Memory(layer) => {
                let (storage, namespace) = layer
                    .uploads_for_test()
                    .asserted_some("the layer's uploads");
                storage.holds_generation(namespace, generation)
            }
            Keeps::Postgres(blobs) => generation_files(blobs, key)
                .iter()
                .any(|file| directory_name(file).as_deref() == Some(generation)),
        }
    }

    /// The generation that keeps `key`'s bytes: the one memory's committed
    /// row names by `key` and `kind`; on PostgreSQL, the one whose directory
    /// holds the key's file, named by its generation.
    fn generation(&self, key: &str, kind: &str) -> Option<String> {
        match &self.keeps {
            Keeps::Memory(layer) => layer.generation_for_test("PostImage", key, kind),
            Keeps::Postgres(blobs) => match generation_files(blobs, key).as_slice() {
                [one] => directory_name(one),
                _ => None,
            },
        }
    }

    /// The named generation's bytes lost where they are kept.
    fn lose(&self, key: &str, kind: &str) {
        match &self.keeps {
            Keeps::Memory(layer) => {
                let (storage, namespace) = layer
                    .uploads_for_test()
                    .asserted_some("the layer's uploads");
                let generation = self
                    .generation(key, kind)
                    .asserted_some("a named generation");
                assert!(
                    storage.lose_generation(namespace, &generation),
                    "its bytes were kept"
                );
            }
            Keeps::Postgres(blobs) => {
                let files = generation_files(blobs, key);
                assert_eq!(files.len(), 1, "one file keeps the image: {files:?}");
                make_writable(&files[0]);
                std::fs::remove_file(&files[0]).asserted("its bytes removed");
            }
        }
    }

    /// The named generation's bytes changed where they are kept, their
    /// length kept.
    fn corrupt(&self, key: &str, kind: &str) {
        match &self.keeps {
            Keeps::Memory(layer) => {
                let (storage, namespace) = layer
                    .uploads_for_test()
                    .asserted_some("the layer's uploads");
                let generation = self
                    .generation(key, kind)
                    .asserted_some("a named generation");
                assert!(
                    storage.corrupt_generation(namespace, &generation),
                    "its bytes were kept"
                );
            }
            Keeps::Postgres(blobs) => {
                let files = generation_files(blobs, key);
                assert_eq!(files.len(), 1, "one file keeps the image: {files:?}");
                let mut bytes = std::fs::read(&files[0]).asserted("its bytes read");
                let at = bytes.len() / 2;
                bytes[at] ^= 0xff;
                make_writable(&files[0]);
                std::fs::write(&files[0], &bytes).asserted("its bytes changed");
            }
        }
    }

    /// The next collection of `key`'s generation fails, until `mend`.
    fn fail_collection(&self, key: &str) -> Option<std::path::PathBuf> {
        use std::os::unix::fs::PermissionsExt;
        match &self.keeps {
            Keeps::Memory(_) => {
                crate::tables::FAIL_NEXT_COLLECT.with(|fail| fail.set(true));
                None
            }
            Keeps::Postgres(blobs) => {
                let dir = generation_dir(blobs, key);
                std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555))
                    .asserted("the generation's directory made read-only");
                let probe = dir.join("probe");
                let refused = std::fs::write(&probe, b"").is_err();
                let _ = std::fs::remove_file(&probe);
                assert!(
                    refused,
                    "the directory refuses a change: the tests run as a user its mode binds, not root"
                );
                Some(dir)
            }
        }
    }

    fn mend(&self, dir: Option<std::path::PathBuf>) {
        use std::os::unix::fs::PermissionsExt;
        crate::tables::FAIL_NEXT_COLLECT.with(|fail| fail.set(false));
        if let Some(dir) = dir {
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755))
                .asserted("the generation's directory writable again");
        }
    }

    fn command(&self, name: &str, args: &[serde_json::Value], interaction: &str) -> Answered {
        self.server
            .command_answer(
                &format!("photos.app.{name}"),
                "reader",
                args,
                Some(interaction),
            )
            .asserted(&format!("{name} answered"))
    }

    fn keep(&self, command: &str, id: i64, interaction: &str) -> Answered {
        assert!(
            attach(&self.server, "reader", &png()).contains(" 303 "),
            "the upload staged"
        );
        self.command(
            command,
            &[serde_json::json!(id), serde_json::json!("a picture")],
            interaction,
        )
    }
}

/// The name of the directory a generation's file is kept in: its generation.
fn directory_name(file: &std::path::Path) -> Option<String> {
    file.parent()
        .and_then(std::path::Path::file_name)
        .map(|name| name.to_string_lossy().into_owned())
}

fn make_writable(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o644))
        .asserted("the file made writable");
}

fn status(head: &str) -> &str {
    head.split_whitespace().nth(1).unwrap_or("")
}

/// **1. The same bytes kept again are a new generation**: a row removed, its
/// generation collected, the same image kept for another row is served from
/// a generation of its own, and the first's bytes stay gone.
fn the_same_bytes_kept_again_are_a_new_generation(l: &Layered) {
    assert!(l.keep("Keep", 1, "keep-1").committed);
    let (key, kind) = l.named("Image", 1).asserted_some("row 1 names its image");
    let first = l
        .generation(&key, &kind)
        .asserted_some("the first generation");
    assert!(
        l.command("Remove", &[serde_json::json!(1)], "remove-1")
            .committed
    );
    assert!(
        !l.kept(&key, Some(&first)),
        "the first generation collected at its commit"
    );
    assert!(l.keep("Keep", 2, "keep-2").committed);
    let (again, kind_again) = l.named("Image", 2).asserted_some("row 2 names its image");
    assert_eq!(
        (&again, &kind_again),
        (&key, &kind),
        "the same bytes, the same key"
    );
    let second = l
        .generation(&again, &kind_again)
        .asserted_some("the second generation");
    assert_ne!(first, second, "a generation of its own");
    let (head, body) = get(&l.server, &format!("/images/{key}.{kind}"));
    assert_eq!(status(&head), "200", "{head}");
    assert_eq!(body, png());
    assert!(l.kept(&key, Some(&second)), "the second kept");
    assert!(!l.kept(&key, Some(&first)), "the first's bytes stay gone");
}

/// **2. A removed row's generation is collected at its commit**: `404`, and
/// its bytes gone from what the layer keeps.
fn a_removed_rows_generation_is_collected_at_its_commit(l: &Layered) {
    assert!(l.keep("Keep", 1, "keep-1").committed);
    let (key, kind) = l.named("Image", 1).asserted_some("the row names its image");
    let generation = l.generation(&key, &kind);
    assert!(l.kept(&key, generation.as_deref()), "kept");
    assert!(
        l.command("Remove", &[serde_json::json!(1)], "remove-1")
            .committed
    );
    assert_eq!(
        status(&get(&l.server, &format!("/images/{key}.{kind}")).0),
        "404"
    );
    assert!(
        !l.kept(&key, generation.as_deref()),
        "collected at its commit"
    );
}

/// **3. A collection that fails at its commit is made by housekeeping**: the
/// row removed while its collection fails; `404` at once, even naming the
/// `ETag` it was served with; its bytes kept until the fault is mended, and
/// the next pass collects them.
fn a_collection_that_failed_is_made_by_housekeeping(l: &Layered) {
    assert!(l.keep("Keep", 1, "keep-1").committed);
    let (key, kind) = l.named("Image", 1).asserted_some("the row names its image");
    let generation = l.generation(&key, &kind);
    let path = format!("/images/{key}.{kind}");
    let (head, _) = get(&l.server, &path);
    let tag = etag(&head).asserted_some("an ETag");
    let fault = l.fail_collection(&key);
    let removed = l.server.command_answer(
        "photos.app.Remove",
        "reader",
        &[serde_json::json!(1)],
        Some("remove-1"),
    );
    // Mended before any assertion, so a failing one leaves no read-only
    // directory behind.
    l.mend(fault);
    assert!(
        matches!(&removed, Ok(answer) if answer.committed),
        "{removed:?}"
    );
    assert_eq!(status(&get(&l.server, &path).0), "404", "named by no row");
    let (again, _) = get_with(&l.server, &path, &format!("If-None-Match: {tag}\r\n"));
    assert_eq!(
        status(&again),
        "404",
        "never a 304 for a generation no row names: {again}"
    );
    assert!(
        l.kept(&key, generation.as_deref()),
        "its collection failed: its bytes kept"
    );
    l.server.run_due();
    assert!(
        !l.kept(&key, generation.as_deref()),
        "collected by housekeeping"
    );
}

/// **4. Named bytes lost are answered `503`**, a `GET` and one naming its
/// `ETag` alike: a generation a committed row names and the layer
/// cannot give whole is unavailable, never `404` or `304`.
fn named_bytes_lost_are_answered_503(l: &Layered) {
    assert!(l.keep("Keep", 1, "keep-1").committed);
    let (key, kind) = l.named("Image", 1).asserted_some("the row names its image");
    let path = format!("/images/{key}.{kind}");
    let (head, _) = get(&l.server, &path);
    let tag = etag(&head).asserted_some("an ETag");
    l.lose(&key, &kind);
    assert_eq!(status(&get(&l.server, &path).0), "503");
    let (again, _) = get_with(&l.server, &path, &format!("If-None-Match: {tag}\r\n"));
    assert_eq!(status(&again), "503", "before any 304: {again}");
}

/// **5. Changed bytes are answered `503`, before any `304`**: read bytes are
/// held to their generation's hash and length before `If-None-Match` is.
fn changed_bytes_are_answered_503_before_any_304(l: &Layered) {
    assert!(l.keep("Keep", 1, "keep-1").committed);
    let (key, kind) = l.named("Image", 1).asserted_some("the row names its image");
    let path = format!("/images/{key}.{kind}");
    let (head, _) = get(&l.server, &path);
    assert_eq!(status(&head), "200");
    let tag = etag(&head).asserted_some("an ETag");
    l.corrupt(&key, &kind);
    assert_eq!(status(&get(&l.server, &path).0), "503");
    let (again, _) = get_with(&l.server, &path, &format!("If-None-Match: {tag}\r\n"));
    assert_eq!(status(&again), "503", "before any 304: {again}");
}

/// **6. A generation two fields of a row name is kept while either does**:
/// one image kept in both, one field cleared, it is served and kept, the
/// other naming it still; the other cleared too, its last use ended, it is
/// collected at that commit and answered `404`, the row kept. In each order:
/// the image first for row 1, the thumb first for row 2.
fn a_generation_two_fields_name_is_kept_while_either_does(l: &Layered) {
    for (id, first, second) in [(1, "Unimage", "Unthumb"), (2, "Unthumb", "Unimage")] {
        assert!(l.keep("KeepBoth", id, &format!("keep-both-{id}")).committed);
        let (key, kind) = l.named("Image", id).asserted_some("the image names it");
        assert_eq!(
            l.named("Thumb", id),
            Some((key.clone(), kind.clone())),
            "the thumb names the same"
        );
        let generation = l.generation(&key, &kind).asserted_some("its generation");
        let path = format!("/images/{key}.{kind}");
        let cleared = |command: &str| {
            let interaction = format!("{command}-{id}");
            assert!(
                l.command(command, &[serde_json::json!(id)], &interaction)
                    .committed
            );
        };
        cleared(first);
        let query = |command: &str| {
            if command == "Unimage" {
                "Image"
            } else {
                "Thumb"
            }
        };
        assert_eq!(l.named(query(first), id), None, "{first} cleared its field");
        assert_eq!(
            status(&get(&l.server, &path).0),
            "200",
            "the other field names it still"
        );
        assert!(
            l.kept(&key, Some(&generation)),
            "kept while the other field names it"
        );
        cleared(second);
        assert_eq!(
            l.named(query(second), id),
            None,
            "{second} cleared its field"
        );
        assert_eq!(status(&get(&l.server, &path).0), "404", "named by no field");
        assert!(
            !l.kept(&key, Some(&generation)),
            "collected at the commit that cleared its last field"
        );
        assert_eq!(
            l.server
                .query("photos.app.Alt", "anyone", &[Val::S64(id)])
                .asserted("the Alt query"),
            Val::Option(Some(Box::new(Val::String("a picture".into())))),
            "the row kept"
        );
    }
}

/// Each case, on each layer: `$case_in_memory` and `$case_on_postgresql`,
/// the PostgreSQL half returning where no database is named.
macro_rules! on_both {
    ($($case:ident => $memory:ident, $postgres:ident, $schema:literal;)*) => {$(
        #[test]
        fn $memory() {
            let (_dir, out) = built();
            $case(&Layered::in_memory(&out));
        }
        #[test]
        fn $postgres() {
            let (_dir, out) = built();
            let Some(l) = Layered::on_postgresql(&out, $schema) else { return };
            $case(&l);
        }
    )*};
}

on_both! {
    the_same_bytes_kept_again_are_a_new_generation =>
        the_same_bytes_kept_again_are_a_new_generation_in_memory,
        the_same_bytes_kept_again_are_a_new_generation_on_postgresql, "contract_again";
    a_removed_rows_generation_is_collected_at_its_commit =>
        a_removed_rows_generation_is_collected_at_its_commit_in_memory,
        a_removed_rows_generation_is_collected_at_its_commit_on_postgresql, "contract_collected";
    a_collection_that_failed_is_made_by_housekeeping =>
        a_collection_that_failed_is_made_by_housekeeping_in_memory,
        a_collection_that_failed_is_made_by_housekeeping_on_postgresql, "contract_housekeeping";
    named_bytes_lost_are_answered_503 =>
        named_bytes_lost_are_answered_503_in_memory,
        named_bytes_lost_are_answered_503_on_postgresql, "contract_lost";
    changed_bytes_are_answered_503_before_any_304 =>
        changed_bytes_are_answered_503_before_any_304_in_memory,
        changed_bytes_are_answered_503_before_any_304_on_postgresql, "contract_changed";
    a_generation_two_fields_name_is_kept_while_either_does =>
        a_generation_two_fields_name_is_kept_while_either_does_in_memory,
        a_generation_two_fields_name_is_kept_while_either_does_on_postgresql, "contract_two_fields";
}
