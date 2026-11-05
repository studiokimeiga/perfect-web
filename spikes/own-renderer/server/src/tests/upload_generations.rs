//! Real PostgreSQL + generation-isolated LocalBlobs + upload handler over TCP.
//! Proposals are trusted test inputs. No claim/session or full Server startup
//! acceptance follows; the generic startup guard remains closed.
use super::tables::Asserted;
use super::*;
use crate::blob::{BlobKey, LocalBlobs, MemoryBlobs};
use pw_host::assets::{Descriptor, Namespace, Reference, Storage};
use pw_table_postgres::{Interaction, Options, PgTables, Records};
use std::io::{self, BufRead, BufReader, Cursor, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{
    Condvar,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::time::Duration;

type Hook = Arc<dyn Fn() + Send + Sync>;
struct Disk {
    files: LocalBlobs,
    before_collect: Mutex<Option<Hook>>,
    fail_collect: AtomicBool,
}
impl Disk {
    fn new(path: &std::path::Path) -> Self {
        Self {
            files: LocalBlobs::new(path).asserted("real local provider"),
            before_collect: Mutex::new(None),
            fail_collect: AtomicBool::new(false),
        }
    }
}
impl Storage for Disk {
    fn enroll(&self, n: &Namespace, fresh: bool) -> io::Result<()> {
        self.files.enroll(n, fresh)
    }
    fn keep_new(&self, n: &Namespace, d: &Descriptor, b: &[u8]) -> io::Result<()> {
        self.files.keep_new(n, d, b)
    }
    fn get_generation(&self, n: &Namespace, d: &Descriptor) -> io::Result<Option<Vec<u8>>> {
        self.files.get_generation(n, d)
    }
    fn collect_generation(&self, n: &Namespace, d: &Descriptor) -> io::Result<()> {
        let hook = self.before_collect.lock().unwrap().clone();
        if let Some(hook) = hook {
            hook();
        }
        if self.fail_collect.load(Ordering::SeqCst) {
            return Err(io::Error::other("deferred actual file collection"));
        }
        self.files.collect_generation(n, d)
    }
}
fn build() -> (tempfile::TempDir, std::path::PathBuf) {
    super::tables::built(
        &[],
        &[
            (
                "PostImage.pw",
                r#"module PostImage
upload PostImage
    route "/uploads/post-image"
    serves "/images"
    max_bytes 5_000_000
    types png, jpeg
    max_width 4096
    max_height 4096
"#,
            ),
            (
                "Assets.pw",
                r#"module Assets
import PostImage
type Item = Item { id: Int, image: Option<PostImage.Ref>, alt: String }
table Assets
    row Item
    key id
    bounded 100
    asset image from PostImage
"#,
            ),
            (
                "Avatar.pw",
                r#"module Avatar
upload Avatar
    route "/uploads/avatar"
    serves "/avatars"
    max_bytes 5_000_000
    types png, jpeg
    max_width 4096
    max_height 4096
"#,
            ),
            (
                "Avatars.pw",
                r#"module Avatars
import Avatar
type Item = Item { id: Int, image: Option<Avatar.Ref>, alt: String }
table Avatars
    row Item
    key id
    bounded 100
    asset image from Avatar
"#,
            ),
            (
                "AssetData.pw",
                r#"module AssetData
import Assets
import PostImage
import Avatar
import Avatars
source AssetData
    holds PostImage, Assets, Avatar, Avatars
    transactions serializable
    reads strong
    changes none
    sets one_call
"#,
            ),
            (
                "app.pw",
                r#"module images.app
import PostImage
public query Url(image: PostImage.Ref) -> String
    freshness 0.seconds
    consistency snapshot
    cache shared
    key image
    concurrency one_per_key
    on_key_change cancel
    timeout 2.seconds
{
    PostImage.src(image)
}
"#,
            ),
        ],
    )
}
fn open(url: &str, schema: &str, build: &std::path::Path, storage: Arc<Disk>) -> Arc<PgTables> {
    Arc::new(
        PgTables::open_with_uploads(
            url,
            Some(schema),
            serde_json::from_str(&std::fs::read_to_string(build.join("tables.json")).unwrap())
                .unwrap(),
            Options::default(),
            pw_table_postgres::Uploads {
                metadata: std::fs::read_to_string(build.join("uploads.json")).unwrap(),
                store: storage,
                fresh_namespace: Namespace::from_bytes([3; 16]),
            },
        )
        .asserted("real enrolled PostgreSQL and filesystem"),
    )
}
fn record(key: &str) -> Records<'_> {
    Records {
        interaction: Some(Interaction {
            key,
            sent: "canonical trusted proposal",
            answer: "applied",
            at_ms: 1000,
            forget_before_ms: 0,
        }),
        outbox: &[],
    }
}
fn descriptor(bytes: &[u8], generation: char) -> Descriptor {
    let m = crate::uploads::sniff::measure(bytes).asserted("sanitized image dimensions");
    Descriptor {
        reference: Reference {
            key: BlobKey::of(bytes).hex().into(),
            generation: generation.to_string().repeat(32),
            kind: m.kind.word().into(),
            width: m.width,
            height: m.height,
        },
        bytes: bytes.len() as u64,
    }
}
fn publish(db: &PgTables, bytes: &[u8], generation: char, id: i64, receipt: &str) -> Reference {
    let mut c = db.begin().asserted("actual namespace lifetime");
    let image = c
        .propose_asset("PostImage", descriptor(bytes, generation), Arc::from(bytes))
        .asserted("trusted sanitized proposal");
    c.write(
        "pw:tables/assets#insert",
        &[serde_json::json!({"id":id,"image":image,"alt":"real file"})],
    )
    .asserted("actual row");
    c.prepare()
        .asserted("prepare registry")
        .publish_with(record(receipt))
        .asserted("commit real file and receipt");
    image
}
fn uploads(db: &Arc<PgTables>, root: &std::path::Path) -> crate::uploads::Uploads {
    let mut avatar = crate::uploads::tests::declared();
    avatar.name = "Avatar".into();
    avatar.route = "/uploads/avatar".into();
    avatar.serves = "/avatars".into();
    let u = crate::uploads::Uploads::with(
        vec![crate::uploads::tests::declared(), avatar],
        Box::new(MemoryBlobs::default()),
        Box::new(LocalBlobs::new(root).unwrap()),
        Duration::from_secs(3600),
    );
    let reader = Arc::clone(db);
    u.generations_by(Box::new(move |decl, key, kind| {
        reader
            .read_asset(decl, key, kind)
            .map_err(|e| format!("{e:?}"))
    }))
    .asserted("authoritative public generation reader");
    u
}
/// Send and parse actual HTTP bytes through the public upload handler. This
/// isolates the handler from unfinished generic command/startup composition.
fn http(
    u: &crate::uploads::Uploads,
    method: &str,
    image: &Reference,
    extra: &str,
) -> (u16, String, Vec<u8>) {
    http_at(
        u,
        method,
        &format!("/images/{}.{}", image.key, image.kind),
        extra,
    )
}
fn http_at(
    u: &crate::uploads::Uploads,
    method: &str,
    route: &str,
    extra: &str,
) -> (u16, String, Vec<u8>) {
    let listener = TcpListener::bind("127.0.0.1:0").asserted("HTTP listener");
    let mut client = TcpStream::connect(listener.local_addr().unwrap()).asserted("HTTP client");
    client
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    client
        .write_all(
            format!("{method} {route} HTTP/1.1\r\nHost: localhost\r\n{extra}\r\n").as_bytes(),
        )
        .asserted("HTTP request bytes");
    let (mut stream, _) = listener.accept().asserted("HTTP connection");
    let mut read = BufReader::new(stream.try_clone().unwrap());
    let mut first = String::new();
    read.read_line(&mut first).asserted("request line");
    assert_eq!(first, format!("{method} {route} HTTP/1.1\r\n"));
    let mut headers = String::new();
    loop {
        let mut line = String::new();
        read.read_line(&mut line).asserted("request header");
        if line == "\r\n" {
            break;
        }
        headers.push_str(&line);
    }
    assert!(u.claims(method, route), "actual upload route is claimed");
    u.answer(
        method,
        route,
        &headers,
        "reader",
        false,
        &mut Cursor::new(Vec::<u8>::new()),
        &mut stream,
    );
    drop(read);
    drop(stream);
    let mut response = Vec::new();
    client
        .read_to_end(&mut response)
        .asserted("HTTP response bytes");
    let split = response
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("HTTP separator");
    let headers = String::from_utf8(response[..split].to_vec()).unwrap();
    let status = headers.split_whitespace().nth(1).unwrap().parse().unwrap();
    (status, headers, response[split + 4..].to_vec())
}
#[test]
fn current_generation_http_validates_real_png_and_jpeg_files_before_304_and_head() {
    let Some(url) = super::tables_pg::database() else {
        return;
    };
    let schema = super::tables_pg::Schema::new(&url, "asset_http");
    let (_built, build) = build();
    let root = tempfile::TempDir::new().unwrap();
    let disk = Arc::new(Disk::new(root.path()));
    let db = open(
        &url,
        schema.database().schema.as_deref().unwrap(),
        &build,
        Arc::clone(&disk),
    );
    let u = uploads(&db, root.path());
    for (n, input) in [
        crate::uploads::tests::png(6, 4),
        crate::uploads::strip::tests::fixture("eight-by-five.jpeg"),
    ]
    .iter()
    .enumerate()
    {
        let bytes = crate::uploads::strip::strip(input).asserted("actual sanitizer");
        let image = publish(
            &db,
            &bytes,
            if n == 0 { 'b' } else { 'c' },
            n as i64 + 1,
            &format!("image-{n}"),
        );
        let (status, head, body) = http(&u, "GET", &image, "");
        assert_eq!(status, 200);
        assert_eq!(body, bytes);
        assert!(head.contains("x-content-type-options: nosniff"));
        assert!(head.contains("cross-origin-resource-policy: same-origin"));
        assert!(head.contains("cache-control: public, max-age=31536000, immutable"));
        let (status, _, body) = http(
            &u,
            "GET",
            &image,
            &format!("If-None-Match: \"{}\"\r\n", image.key),
        );
        assert_eq!(status, 304);
        assert!(body.is_empty());
        let (status, head, body) = http(&u, "HEAD", &image, "");
        assert_eq!(status, 200);
        assert!(body.is_empty());
        assert!(head.contains(&format!("content-length: {}", bytes.len())));
        let path = root
            .path()
            .join(".generic")
            .join(db.upload_namespace().unwrap().hex())
            .join("generations")
            .join(&image.generation)
            .join(&image.key);
        std::fs::write(&path, b"broken").asserted("corrupt an actual LIVE payload");
        assert_eq!(
            http(&u, "GET", &image, "If-None-Match: *\r\n").0,
            503,
            "corruption beats 304"
        );
        std::fs::remove_file(path).asserted("remove actual LIVE payload");
        assert_eq!(
            http(&u, "HEAD", &image, "If-None-Match: *\r\n").0,
            503,
            "LIVE missing beats HEAD/304"
        );
    }
}
#[test]
fn retired_generation_http_is_404_before_304_even_while_actual_payload_waits_for_collection() {
    let Some(url) = super::tables_pg::database() else {
        return;
    };
    let schema = super::tables_pg::Schema::new(&url, "asset_http_retired");
    let (_built, build) = build();
    let root = tempfile::TempDir::new().unwrap();
    let disk = Arc::new(Disk::new(root.path()));
    let db = open(
        &url,
        schema.database().schema.as_deref().unwrap(),
        &build,
        Arc::clone(&disk),
    );
    let bytes = crate::uploads::strip::strip(&crate::uploads::tests::png(6, 4)).unwrap();
    let image = publish(&db, &bytes, 'b', 1, "seed");
    let u = uploads(&db, root.path());
    disk.fail_collect.store(true, Ordering::SeqCst);
    let mut c = db.begin().asserted("retire");
    c.write("pw:tables/assets#delete", &[serde_json::json!(1)])
        .asserted("last ref");
    c.commit().asserted("retirement applies");
    assert_eq!(
        disk.get_generation(db.upload_namespace().unwrap(), &descriptor(&bytes, 'b'))
            .unwrap(),
        Some(bytes)
    );
    assert_eq!(
        http(&u, "GET", &image, "If-None-Match: *\r\n").0,
        404,
        "physical bytes and matching validators confer no authority"
    );
    disk.fail_collect.store(false, Ordering::SeqCst);
    assert_eq!(db.collect_assets(1).asserted("real bounded cleanup"), 1);
}
#[test]
fn backend_death_stale_local_unlink_preserves_new_same_hash_generation_and_http() {
    let Some(url) = super::tables_pg::database() else {
        return;
    };
    let schema = super::tables_pg::Schema::new(&url, "asset_http_death");
    let (_built, build) = build();
    let root = tempfile::TempDir::new().unwrap();
    let disk = Arc::new(Disk::new(root.path()));
    let name = format!("asset_file_a_{}", std::process::id());
    let joiner = if url.contains('?') { '&' } else { '?' };
    let named = format!("{url}{joiner}application_name={name}");
    let schema_name = schema.database().schema.unwrap();
    let a = open(&named, &schema_name, &build, Arc::clone(&disk));
    let b = open(&url, &schema_name, &build, Arc::new(Disk::new(root.path())));
    let bytes = crate::uploads::strip::strip(&crate::uploads::tests::png(6, 4)).unwrap();
    let old = publish(&a, &bytes, 'b', 1, "seed");
    let (entered, observed) = mpsc::channel();
    let release = Arc::new((Mutex::new(false), Condvar::new()));
    let wait = Arc::clone(&release);
    let resumed = Arc::new(AtomicBool::new(false));
    let finished = Arc::clone(&resumed);
    *disk.before_collect.lock().unwrap() = Some(Arc::new(move || {
        let _ = entered.send(());
        let (flag, ready) = &*wait;
        let held = flag.lock().unwrap();
        let (held, timeout) = ready
            .wait_timeout_while(held, Duration::from_secs(5), |done| !*done)
            .unwrap();
        finished.store(*held && !timeout.timed_out(), Ordering::SeqCst);
    }));
    let mut c = a.begin().asserted("A held namespace");
    c.write("pw:tables/assets#delete", &[serde_json::json!(1)])
        .asserted("A last ref");
    let p = c.prepare().asserted("A prepare");
    let child = std::thread::spawn(move || p.publish_with(record("removed")));
    assert!(
        observed.recv_timeout(Duration::from_secs(5)).is_ok(),
        "collector reached after fresh no-ref check"
    );
    assert_eq!(
        schema.texts("SELECT state FROM pw_asset_generations"),
        ["tombstone"]
    );
    assert_eq!(schema.texts(&format!("SELECT count(*)::text FROM pg_locks l JOIN pg_stat_activity a USING(pid) WHERE a.application_name='{name}' AND l.locktype='advisory' AND l.granted")),["1"]);
    assert_eq!(schema.texts(&format!("SELECT pg_terminate_backend(pid)::text FROM pg_stat_activity WHERE application_name='{name}'")),["true"]);
    let new = publish(&b, &bytes, 'c', 2, "republished");
    assert_ne!(old.generation, new.generation);
    let u = uploads(&b, root.path());
    assert_eq!(http(&u, "GET", &new, "").0, 200);
    let (flag, ready) = &*release;
    *flag.lock().unwrap() = true;
    ready.notify_all();
    child
        .join()
        .expect("A returned")
        .asserted("A known-applied Ack survives dead completion connection");
    assert!(
        resumed.load(Ordering::SeqCst),
        "old actual unlink resumed only after B committed, never after timeout"
    );
    assert_eq!(
        disk.get_generation(b.upload_namespace().unwrap(), &descriptor(&bytes, 'b'))
            .unwrap(),
        None
    );
    assert_eq!(
        disk.get_generation(b.upload_namespace().unwrap(), &descriptor(&bytes, 'c'))
            .unwrap(),
        Some(bytes.clone())
    );
    let (status, _, body) = http(&u, "GET", &new, "");
    assert_eq!(status, 200);
    assert_eq!(body, bytes);
    assert_eq!(http(&u, "GET", &new, "If-None-Match: *\r\n").0, 304);
    assert_eq!(
        schema.texts("SELECT generation FROM pw_asset_collections"),
        [old.generation]
    );
    assert_eq!(
        b.collect_assets(1).asserted("retry old captured file job"),
        1
    );
    assert_eq!(http(&u, "GET", &new, "").0, 200);
}

fn invalid_metadata(case: &str) {
    let Some(url) = super::tables_pg::database() else {
        return;
    };
    let schema = super::tables_pg::Schema::new(&url, "asset_http_metadata");
    let (_built, build) = build();
    let root = tempfile::TempDir::new().unwrap();
    let disk = Arc::new(Disk::new(root.path()));
    let db = open(
        &url,
        schema.database().schema.as_deref().unwrap(),
        &build,
        disk,
    );
    let bytes = crate::uploads::strip::strip(&crate::uploads::tests::png(6, 4)).unwrap();
    let mut bad = descriptor(&bytes, 'b');
    if case == "kind" {
        bad.reference.kind = "jpeg".into();
    } else {
        bad.reference.width += 1;
    }
    let mut c = db.begin().asserted("trusted lower bridge proposal");
    let image = c
        .propose_asset("PostImage", bad.clone(), Arc::from(bytes.as_slice()))
        .asserted("structural metadata valid but mismatched to actual image");
    c.write(
        "pw:tables/assets#insert",
        &[serde_json::json!({"id":1,"image":image,"alt":"metadata witness"})],
    )
    .asserted("actual row");
    c.prepare()
        .asserted("prepare")
        .publish_with(record("seed"))
        .asserted("trusted lower proposal commits hash-valid file");
    assert_eq!(
        db.read_asset("PostImage", &image.key, &image.kind)
            .asserted("provider resolves hash-valid bytes"),
        Some((bad, bytes))
    );
    let u = uploads(&db, root.path());
    assert_eq!(
        http(&u, "GET", &image, "If-None-Match: *\r\n").0,
        503,
        "HTTP must validate measured {case} before cache validators even with hash-valid actual files"
    );
}
#[test]
fn generation_http_rejects_actual_file_kind_differing_from_the_descriptor() {
    invalid_metadata("kind");
}
#[test]
fn generation_http_rejects_actual_file_dimensions_differing_from_the_descriptor() {
    invalid_metadata("dimensions");
}
#[test]
fn generation_http_uses_each_routes_current_same_declaration_reference_counts() {
    let Some(url) = super::tables_pg::database() else {
        return;
    };
    let schema = super::tables_pg::Schema::new(&url, "asset_http_declarations");
    let (_built, build) = build();
    let root = tempfile::TempDir::new().unwrap();
    let disk = Arc::new(Disk::new(root.path()));
    let db = open(
        &url,
        schema.database().schema.as_deref().unwrap(),
        &build,
        disk,
    );
    let bytes = crate::uploads::strip::strip(&crate::uploads::tests::png(6, 4)).unwrap();
    let image = publish(&db, &bytes, 'b', 1, "post");
    let u = uploads(&db, root.path());
    let avatar_route = format!("/avatars/{}.{}", image.key, image.kind);
    assert_eq!(http(&u, "GET", &image, "").0, 200);
    assert_eq!(
        http_at(&u, "GET", &avatar_route, "If-None-Match: *\r\n").0,
        404,
        "other declaration's hash/bytes do not enroll this declaration"
    );
    let mut c = db.begin().asserted("independent Avatar publication");
    let avatar = c
        .propose_asset(
            "Avatar",
            descriptor(&bytes, 'c'),
            Arc::from(bytes.as_slice()),
        )
        .asserted("current live physical generation reused");
    assert_eq!(avatar.generation, image.generation);
    c.write(
        "pw:tables/avatars#insert",
        &[serde_json::json!({"id":1,"image":avatar,"alt":"avatar"})],
    )
    .asserted("Avatar row");
    c.prepare()
        .asserted("prepare independent enrollment")
        .publish_with(record("avatar"))
        .asserted("own reference commits");
    assert_eq!(http_at(&u, "GET", &avatar_route, "").0, 200);
    let mut c = db.begin().asserted("remove PostImage only");
    c.write("pw:tables/assets#delete", &[serde_json::json!(1)])
        .asserted("last PostImage ref");
    c.commit().asserted("other declaration remains live");
    assert_eq!(http(&u, "GET", &image, "If-None-Match: *\r\n").0, 404);
    assert_eq!(
        http_at(&u, "GET", &avatar_route, "If-None-Match: *\r\n").0,
        304
    );
    let mut c = db.begin().asserted("remove Avatar");
    c.write("pw:tables/avatars#delete", &[serde_json::json!(1)])
        .asserted("last total ref");
    c.commit().asserted("retire total generation");
    assert_eq!(
        http_at(&u, "HEAD", &avatar_route, "If-None-Match: *\r\n").0,
        404
    );
}
