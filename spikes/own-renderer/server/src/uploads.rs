//! **A request whose body is a file** (track `uploads`, `docs/PARALLEL.md`,
//! ADR-0253; the track's ADR, `ADR-0260-an-image-on-a-post`).
//!
//! The uploads track owns this module: a typed upload with the limits its
//! program declares, stored through a deployment's blob storage
//! ([`crate::blob`]), and served safely. `main.rs` reaches it at marked
//! seams: `Uploads::claims`, asked for every request before its body is
//! read, and `Uploads::answer` for a request it claims. A command's body is
//! bounded to 64 KiB and read whole; an upload's is bounded by its
//! declaration, and read here.
//!
//! **An upload's life** (a lease):
//!
//! 1. A browser posts a form to the declaration's route. Its body is read
//!    only when its length is within the declaration's bytes; its image's
//!    kind is sniffed from the bytes, and its width and height read from its
//!    header ([`sniff`]); the name and type the browser sent are never read.
//! 2. The image is rewritten without what names a place, a device, a person
//!    or a time ([`strip`], ADR-0383), and what is kept waits in this
//!    server's staging, by its SHA-256, **leased** to the session: one lease
//!    a session, which a second upload replaces.
//! 3. A command **claims** the lease (`resource.acquire<Upload>` in the
//!    program) and either **commits** it with a post or **discards** it
//!    (`resource.release<Upload>`): PW2005 holds the program to one or the
//!    other on every path. The feed's data layer keeps the image with the
//!    post, in the post's own transaction, its bytes put in the deployment's
//!    blob storage before the transaction commits. A claim no transaction
//!    commits is given back.
//! 4. A lease nothing claims ends when its time does, and a staged blob no
//!    lease holds is forgotten.
//!
//! What is served is the deployment's blob storage alone, so a lease is
//! never served but to its session: at the declaration's path and the blob's
//! key, with the type sniffed from its bytes, `nosniff`, a sandbox, and kept
//! forever by any cache, since what a key names never changes.

pub(crate) mod icc;
pub(crate) mod multipart;
pub(crate) mod sniff;
pub(crate) mod strip;

use crate::blob::{BlobKey, BlobStore, LocalBlobs, MemoryBlobs};
use sniff::{Kind, Measured, Unreadable};
use std::collections::BTreeMap;
use std::io::{BufRead, Read, Write};
use std::net::TcpStream;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// **An `upload` declaration, as `pw build` writes it** (`uploads.json`):
/// where a form posts it, where what is committed is served, and its limits.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct Declared {
    /// The declaration's name, `PostImage`.
    pub name: String,
    /// Where a form posts the file: `/uploads/post-image`.
    pub route: String,
    /// Where a committed image is served: `/images`, then `/<key>.<kind>`.
    pub serves: String,
    /// At most this many bytes of image.
    pub max_bytes: u64,
    /// The kinds it may be, by their words: `png`, `jpeg`, `webp`, `gif`.
    pub types: Vec<String>,
    /// At most this wide and this tall, as a browser shows it.
    pub max_width: u32,
    pub max_height: u32,
}

impl Declared {
    fn allows(&self, kind: Kind) -> bool {
        self.types.iter().any(|t| t == kind.word())
    }
}

/// What a form's body may hold beyond its file: its delimiters, and each
/// part's headers.
const FORM_OVERHEAD: u64 = 16 * 1024;

/// How long a lease is kept unclaimed.
const LEASE: Duration = Duration::from_secs(60 * 60);

/// What every session's leases may hold together, in the server's memory, so
/// that sessions, which cost nothing to make, cannot fill it between two
/// sweeps.
const STAGED_CAP: u64 = 128 * 1024 * 1024;

/// How long a sender may take between two reads of its body.
const READ_TIMEOUT: Duration = Duration::from_secs(30);

/// **An upload leased to its session**: ended only by a command's commit or
/// discard, by a second upload, or by its time.
#[derive(Debug)]
struct Lease {
    id: String,
    key: BlobKey,
    kind: Kind,
    width: u32,
    height: u32,
    bytes: u64,
    made: Instant,
    /// A command's transaction holds it, between its claim and its end.
    claimed: bool,
}

/// **A lease a command claimed**: what its post keeps of it. Not `Clone`:
/// a claim is ended once, by [`Shared::committed`], [`Shared::discarded`]
/// or [`Shared::unclaim`], each taking it.
#[derive(Debug, PartialEq, Eq)]
#[must_use = "a claim is committed, discarded or given back"]
pub(crate) struct Claimed {
    pub session: String,
    pub id: String,
    pub key: BlobKey,
    pub kind: Kind,
    pub width: u32,
    pub height: u32,
}

/// **What a page shows of its session's lease**: where, and how large.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Shown {
    pub src: String,
    pub width: u32,
    pub height: u32,
}

#[derive(Default)]
struct State {
    /// One lease a session.
    leases: BTreeMap<String, Lease>,
    /// How many leases hold each staged blob: the same bytes from two
    /// sessions are one blob.
    holds: BTreeMap<BlobKey, u32>,
    next: u64,
    staged_bytes: u64,
    /// What each user leased in the last hour, when and how many bytes: their
    /// budget's spending.
    spent: BTreeMap<String, std::collections::VecDeque<(Instant, u64)>>,
}

/// **A user's budget**: at most so many images in an hour, and at most so
/// many times the declaration's bytes. Each upload is spent, posted or not:
/// what it costs is the server's work and its memory, not the post.
const UPLOADS_AN_HOUR: usize = 30;
const BYTES_AN_HOUR: u64 = 20;
const BUDGET_WINDOW: Duration = Duration::from_secs(60 * 60);

/// What the module shares with the feed's data layer, which commits a lease
/// with a post in the post's own transaction.
pub(crate) struct Shared {
    declared: Vec<Declared>,
    /// Where a lease's bytes wait: this server's memory.
    staged: Box<dyn BlobStore>,
    /// **The deployment's blob storage**: what a committed post shows.
    blobs: Box<dyn BlobStore>,
    state: Mutex<State>,
    lease_for: Duration,
    /// Track `identity`'s principals, handed once: who uploads. Where none
    /// are handed, each session is its own user.
    principals: std::sync::OnceLock<crate::identity::Principals>,
    /// **Whether a committed row names a blob** (ADR-0261), the data
    /// layer's answer, handed once: a blob is served only while one does.
    /// None handed, none is served.
    named: std::sync::OnceLock<Named>,
}

/// **The leases, as the feed's data layer holds them.**
pub(crate) type Leases = Arc<Shared>;

/// **Whether a committed row names the blob with this key** (ADR-0261), as
/// the data layer answers it.
pub(crate) type Named = Box<dyn Fn(&str) -> Result<bool, String> + Send + Sync>;

impl std::fmt::Debug for Shared {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let names: Vec<&str> = self.declared.iter().map(|d| d.name.as_str()).collect();
        f.debug_struct("Leases").field("declared", &names).finish()
    }
}

/// **What a deployment states of its uploads' limits**: each, where it
/// states one, applied to every upload the program declares.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Lowered {
    pub max_bytes: Option<u64>,
    pub max_width: Option<u64>,
    pub max_height: Option<u64>,
}

/// **A deployment may lower a limit and never raise one** (the integrator's
/// ruling on Q1): the program states what an upload may be, and a host may
/// hold a browser to less. A deployment's limit above a program's is not
/// taken as the lesser: the host refuses to start, since a deployment that
/// asks for more than the program allows has misread one or the other.
pub(crate) fn lowered(mut declared: Vec<Declared>, by: &Lowered) -> Result<Vec<Declared>, String> {
    for d in &mut declared {
        let lower = |what: &str, ours: &mut u64, theirs: Option<u64>| -> Result<(), String> {
            match theirs {
                Some(0) => Err(format!("the deployment's {what} for `{}` is zero", d.name)),
                Some(n) if n > *ours => Err(format!(
                    "the deployment's {what} for `{}` is {n}, above the program's {ours}: a \
                     deployment may lower an upload's limit and never raise one",
                    d.name
                )),
                Some(n) => {
                    *ours = n;
                    Ok(())
                }
                None => Ok(()),
            }
        };
        let (mut w, mut h) = (d.max_width as u64, d.max_height as u64);
        let mut bytes = d.max_bytes;
        lower("max_bytes", &mut bytes, by.max_bytes)?;
        lower("max_width", &mut w, by.max_width)?;
        lower("max_height", &mut h, by.max_height)?;
        (d.max_bytes, d.max_width, d.max_height) = (bytes, w as u32, h as u32);
    }
    Ok(declared)
}

/// **Where a development server keeps its blobs**: `PW_BLOB_DIR` where the
/// deployment names it, and `blobs/` beside the documents otherwise.
pub fn blob_root(dist: &std::path::Path) -> std::path::PathBuf {
    match std::env::var("PW_BLOB_DIR") {
        Ok(dir) if !dir.is_empty() => dir.into(),
        _ => dist.join("blobs"),
    }
}

/// What the uploads track keeps for one server: nothing where the program
/// declares no upload.
#[derive(Default)]
pub struct Uploads {
    shared: Option<Leases>,
}

impl Uploads {
    /// **The uploads a build declares** (`uploads.json`): what posts commit
    /// kept in the directory `root`, the development server's blob storage,
    /// and what leases hold in memory, since no lease outlives the server
    /// that made it. A build that declares none has none, and claims nothing.
    pub fn from_build(build: &std::path::Path, root: &std::path::Path) -> Result<Uploads, String> {
        let declared: Vec<Declared> = match std::fs::read_to_string(build.join("uploads.json")) {
            Ok(text) => serde_json::from_str(&text).map_err(|e| format!("uploads.json: {e}"))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(format!("uploads.json: {e}")),
        };
        if declared.is_empty() {
            return Ok(Uploads::default());
        }
        let deployment = |name: &str| -> Result<Option<u64>, String> {
            match std::env::var(name) {
                Ok(v) if !v.is_empty() => v
                    .parse()
                    .map(Some)
                    .map_err(|_| format!("{name}={v} is not a count")),
                _ => Ok(None),
            }
        };
        let declared = lowered(
            declared,
            &Lowered {
                max_bytes: deployment("PW_UPLOAD_MAX_BYTES")?,
                max_width: deployment("PW_UPLOAD_MAX_WIDTH")?,
                max_height: deployment("PW_UPLOAD_MAX_HEIGHT")?,
            },
        )?;
        let blobs = LocalBlobs::new(root).map_err(|e| format!("{}: {e}", root.display()))?;
        Ok(Uploads::with(
            declared,
            Box::new(MemoryBlobs::default()),
            Box::new(blobs),
            LEASE,
        ))
    }

    /// The uploads `declared`, staged in `staged` and committed to `blobs`,
    /// a lease kept `lease_for`.
    pub(crate) fn with(
        declared: Vec<Declared>,
        staged: Box<dyn BlobStore>,
        blobs: Box<dyn BlobStore>,
        lease_for: Duration,
    ) -> Uploads {
        Uploads {
            shared: Some(Arc::new(Shared {
                declared,
                staged,
                blobs,
                state: Mutex::new(State::default()),
                lease_for,
                principals: std::sync::OnceLock::new(),
                named: std::sync::OnceLock::new(),
            })),
        }
    }

    /// The leases, for the data layer that commits them with a post.
    /// **What a server says of its uploads as it starts**: each one's route
    /// and limits, as the deployment holds a browser to them, and how many
    /// blobs its storage keeps.
    pub fn describe(&self) -> Option<String> {
        let shared = self.shared.as_ref()?;
        let each: Vec<String> = shared
            .declared
            .iter()
            .map(|d| {
                format!(
                    "{} at {} (at most {} bytes, {} by {}, {}), served at {}",
                    d.name,
                    d.route,
                    d.max_bytes,
                    d.max_width,
                    d.max_height,
                    d.types.join(", "),
                    d.serves
                )
            })
            .collect();
        let kept = shared.blobs.count().map_or_else(
            |e| format!("its blob storage unreadable: {e}"),
            |n| format!("{n} blob(s) kept"),
        );
        Some(format!("{}; {kept}", each.join("; ")))
    }

    /// **Who uploads** (track `identity`, ADR-0258): the identity's
    /// principals, handed once when the server is built, as a data layer is
    /// handed them (`DataLayer::identified_by`).
    pub fn identified_by(&self, principals: crate::identity::Principals) {
        if let Some(shared) = &self.shared {
            let _ = shared.principals.set(principals);
        }
    }

    /// **Which blobs a committed row names** (ADR-0261): the data layer's
    /// answer, handed once when the server is built, as it is handed the
    /// leases. A blob no row names is not served.
    pub(crate) fn named_by(&self, named: Named) {
        if let Some(shared) = &self.shared {
            let _ = shared.named.set(named);
        }
    }

    pub(crate) fn leases(&self) -> Option<Leases> {
        self.shared.clone()
    }

    /// **Does this module answer `method` `route`?** Asked before the body is
    /// read, so the command path never reads an upload's body: a form posted
    /// to a declaration's route, a session's own lease shown under it, and a
    /// committed image under the path it is served at.
    pub fn claims(&self, method: &str, route: &str) -> bool {
        let Some(shared) = &self.shared else {
            return false;
        };
        shared.declared.iter().any(|d| match method {
            "POST" => route == d.route,
            "GET" | "HEAD" => under(route, &d.serves).is_some() || under(route, &d.route).is_some(),
            _ => false,
        })
    }

    /// **Answer a request this module claimed**: read its body from
    /// `reader`, within the upload's limits, and respond on `stream`.
    #[allow(clippy::too_many_arguments)]
    pub fn answer(
        &self,
        method: &str,
        route: &str,
        headers: &str,
        session: &str,
        fresh: bool,
        reader: &mut dyn BufRead,
        stream: &mut TcpStream,
    ) {
        let _ = stream.set_read_timeout(Some(READ_TIMEOUT));
        let answer = match &self.shared {
            Some(shared) => shared.answer(method, route, headers, session, fresh, reader),
            None => Answer::text(404, "no upload is declared"),
        };
        answer.write(method == "HEAD", stream);
        // A body not read, where the answer came first: read up to what the
        // declaration allows, so the sender reads the answer and not a
        // reset connection.
        if answer.unread > 0 {
            let _ = std::io::copy(&mut reader.take(answer.unread), &mut std::io::sink());
        }
    }
}

/// The rest of `route` under `prefix`, `/images/abc` under `/images`.
fn under<'a>(route: &'a str, prefix: &str) -> Option<&'a str> {
    route
        .strip_prefix(prefix)?
        .strip_prefix('/')
        .filter(|rest| !rest.is_empty())
}

/// A header's value, by its name in any case.
fn header<'a>(headers: &'a str, name: &str) -> Option<&'a str> {
    headers.lines().find_map(|l| {
        let (k, v) = l.split_once(':')?;
        k.trim().eq_ignore_ascii_case(name).then(|| v.trim())
    })
}

/// **Is a request that changes something from this origin?** Go 1.25's
/// `net/http.CrossOriginProtection`: `Sec-Fetch-Site` where the browser
/// sends it, `same-origin` or `none`; otherwise `Origin`'s host against
/// `Host`; a request with neither is not a browser's, or a same-origin one
/// from a browser that sends neither. The identity track holds every other
/// route to the same check (`Identity::answer`); an upload is answered
/// before it, so holds itself to it.
pub(crate) fn same_origin(headers: &str) -> bool {
    crate::identity::same_origin("POST", headers).is_ok()
}

/// **Where a browser goes once its file is leased**: the page it posted
/// from, as its `Referer` names it, where that is this origin; `/`
/// otherwise. A path alone, never another origin's.
fn back(headers: &str) -> String {
    let path = header(headers, "referer").and_then(|r| {
        let rest = r
            .strip_prefix("http://")
            .or_else(|| r.strip_prefix("https://"))?;
        let (host, path) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
        let ours = header(headers, "host").is_some_and(|h| h.eq_ignore_ascii_case(host));
        (ours && path.starts_with('/') && !path.starts_with("//")).then(|| path.to_string())
    });
    path.unwrap_or_else(|| "/".to_string())
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// **An answer**: its status, headers and body, and how much of the
/// request's body it leaves unread.
#[derive(Debug)]
pub(crate) struct Answer {
    pub status: u16,
    pub headers: Vec<(&'static str, String)>,
    pub body: Vec<u8>,
    pub unread: u64,
}

/// What every answer this module gives carries: the type it states is the
/// type, and a document it serves runs nothing.
const GUARDED: &[(&str, &str)] = &[
    ("x-content-type-options", "nosniff"),
    ("content-security-policy", "default-src 'none'; sandbox"),
];

impl Answer {
    fn new(status: u16, mime: &str, body: Vec<u8>) -> Answer {
        let mut headers: Vec<(&'static str, String)> =
            GUARDED.iter().map(|(k, v)| (*k, v.to_string())).collect();
        headers.push(("content-type", mime.to_string()));
        headers.push(("cache-control", "private, no-store".to_string()));
        Answer {
            status,
            headers,
            body,
            unread: 0,
        }
    }

    fn text(status: u16, why: &str) -> Answer {
        Answer::new(status, "text/plain; charset=utf-8", why.as_bytes().to_vec())
    }

    /// **A form's file refused**: why, said to the person, and the way back.
    fn refused(status: u16, why: &str, back: &str) -> Answer {
        let page = format!(
            "<!doctype html><html lang=\"en\"><meta charset=\"utf-8\"><title>Not attached</title>\
             <main><p role=\"alert\">{}</p><p><a href=\"{}\">Back</a></p></main></html>",
            escape(why),
            escape(back)
        );
        // It runs nothing, and its link is followed: no sandbox, which would
        // make it an opaque origin, and no script, style or image.
        let mut a = Answer::new(status, "text/html; charset=utf-8", page.into_bytes());
        a.set("content-security-policy", "default-src 'none'".to_string());
        a
    }

    fn set(&mut self, name: &'static str, value: String) {
        self.headers.retain(|(k, _)| *k != name);
        self.headers.push((name, value));
    }

    #[cfg(test)]
    pub(crate) fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| *k == name)
            .map(|(_, v)| v.as_str())
    }

    fn write(&self, head_only: bool, stream: &mut impl Write) {
        let mut head = format!("HTTP/1.1 {} {}\r\n", self.status, reason(self.status));
        for (k, v) in &self.headers {
            head.push_str(&format!("{k}: {v}\r\n"));
        }
        head.push_str(&format!(
            "content-length: {}\r\nconnection: close\r\n\r\n",
            self.body.len()
        ));
        let _ = stream.write_all(head.as_bytes());
        if !head_only {
            let _ = stream.write_all(&self.body);
        }
        let _ = stream.flush();
    }
}

fn reason(code: u16) -> &'static str {
    match code {
        200 => "OK",
        303 => "See Other",
        304 => "Not Modified",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        409 => "Conflict",
        429 => "Too Many Requests",
        411 => "Length Required",
        413 => "Content Too Large",
        415 => "Unsupported Media Type",
        422 => "Unprocessable Content",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        _ => "",
    }
}

/// `bytes` in megabytes, to a tenth, as a person reads a limit.
fn megabytes(bytes: u64) -> String {
    let tenths = bytes * 10 / 1_000_000;
    if tenths.is_multiple_of(10) {
        format!("{} MB", tenths / 10)
    } else {
        format!("{}.{} MB", tenths / 10, tenths % 10)
    }
}

impl Shared {
    pub(crate) fn answer(
        &self,
        method: &str,
        route: &str,
        headers: &str,
        session: &str,
        fresh: bool,
        reader: &mut dyn BufRead,
    ) -> Answer {
        if method == "POST"
            && let Some(declared) = self.declared.iter().find(|d| d.route == route)
        {
            return self.upload(declared, headers, session, fresh, reader);
        }
        for d in &self.declared {
            if let Some(name) = under(route, &d.serves) {
                return self.serve(name, headers);
            }
            if let Some(id) = under(route, &d.route) {
                return self.preview(id, session);
            }
        }
        Answer::text(404, "not found")
    }

    /// **A form's file, leased to its session**, or refused with why.
    fn upload(
        &self,
        declared: &Declared,
        headers: &str,
        session: &str,
        fresh: bool,
        reader: &mut dyn BufRead,
    ) -> Answer {
        let back = back(headers);
        // A session the browser has not been given holds nothing it can use.
        if fresh {
            return Answer::refused(403, "Open the page, then attach the image.", &back);
        }
        if !same_origin(headers) {
            return Answer::text(403, "a cross-origin request");
        }
        // Who uploads: a session's principal, which `requires SignedIn`
        // holds a post to. One signed out has nothing to post it with.
        let user = match self.principals.get() {
            Some(principals) => principals.of(session).map(|p| p.user),
            None => Some(format!("u-{session}")),
        };
        let Some(user) = user else {
            return Answer::refused(403, "Sign in to attach an image.", &back);
        };
        if header(headers, "transfer-encoding").is_some() {
            return Answer::text(411, "an upload states its length");
        }
        let Some(length) = header(headers, "content-length").and_then(|v| v.parse::<u64>().ok())
        else {
            return Answer::text(411, "an upload states its length");
        };
        let too_large = format!(
            "The image is larger than {}.",
            megabytes(declared.max_bytes)
        );
        // Bounded before a byte is read: a length nothing checks is an
        // allocation anyone can ask for.
        if length > declared.max_bytes + FORM_OVERHEAD {
            let mut a = Answer::refused(413, &too_large, &back);
            a.unread = length.min(2 * (declared.max_bytes + FORM_OVERHEAD));
            return a;
        }
        let Some(boundary) = header(headers, "content-type").and_then(multipart::boundary) else {
            return Answer::refused(415, "The form is not sent as multipart/form-data.", &back);
        };
        let mut body = Vec::with_capacity(length as usize);
        if reader.take(length).read_to_end(&mut body).is_err() || body.len() as u64 != length {
            return Answer::text(400, "the body ended before its length");
        }
        let parts = match multipart::parts(&body, &boundary) {
            Ok(parts) => parts,
            Err(why) => {
                return Answer::refused(400, &format!("The form is malformed: {why}."), &back);
            }
        };
        // The one part that is a file's, whatever its field is named: PW5603
        // holds a form to one file input.
        let files: Vec<&multipart::Part> = parts.iter().filter(|p| p.file).collect();
        let [file] = files[..] else {
            return Answer::refused(400, "The form sends one image at a time.", &back);
        };
        let bytes = file.data;
        if bytes.is_empty() {
            return Answer::refused(400, "No image was chosen.", &back);
        }
        if bytes.len() as u64 > declared.max_bytes {
            return Answer::refused(413, &too_large, &back);
        }
        let kinds = declared.types.join(", ").to_uppercase();
        let unreadable = |read| match read {
            Unreadable::Unrecognized => {
                let why = format!("The file is not an image: only {kinds}.");
                Answer::refused(415, &why, &back)
            }
            Unreadable::Malformed(kind, why) => {
                let why = format!(
                    "The file is not a whole {} image: {why}.",
                    kind.word().to_uppercase()
                );
                Answer::refused(422, &why, &back)
            }
        };
        match sniff::measure(bytes) {
            Ok(m) if declared.allows(m.kind) => {}
            Ok(m) => {
                let why = format!(
                    "A {} image cannot be attached: only {kinds}.",
                    m.kind.word().to_uppercase()
                );
                return Answer::refused(415, &why, &back);
            }
            Err(read) => return unreadable(read),
        }
        // **Kept without what names a place, a device, a person or a time**
        // (ADR-0383): rewritten before it is kept, a file whose container
        // does not parse refused whole; then measured as it is kept, which
        // is what is served.
        let (kept, measured) = match strip::strip(bytes)
            .and_then(|kept| sniff::measure(&kept).map(|measured| (kept, measured)))
        {
            Ok(read) => read,
            Err(read) => return unreadable(read),
        };
        if measured.width > declared.max_width || measured.height > declared.max_height {
            let why = format!(
                "The image is {} by {} pixels, and may be at most {} by {}.",
                measured.width, measured.height, declared.max_width, declared.max_height
            );
            return Answer::refused(422, &why, &back);
        }
        match self.lease(
            session,
            &user,
            declared,
            &kept,
            bytes.len() as u64,
            measured,
        ) {
            Ok(()) => {
                let mut a = Answer::new(303, "text/plain; charset=utf-8", Vec::new());
                a.set("location", back);
                a
            }
            Err((status, why, retry)) => {
                let mut a = Answer::refused(status, why, &back);
                if let Some(after) = retry {
                    a.set("retry-after", after.as_secs().max(1).to_string());
                }
                a
            }
        }
    }

    /// **`bytes` staged, and leased to `session`**, its earlier lease ended.
    /// `sent` is how many bytes its sender sent, which the user's budget and
    /// the staging's cap count: what is kept may be fewer (ADR-0383), and
    /// what the server read and rewrote is what it cost.
    fn lease(
        &self,
        session: &str,
        user: &str,
        declared: &Declared,
        bytes: &[u8],
        sent: u64,
        m: Measured,
    ) -> Result<(), (u16, &'static str, Option<Duration>)> {
        let mut state = self.state.lock().expect("uploads");
        self.sweep(&mut state);
        if state.leases.get(session).is_some_and(|l| l.claimed) {
            return Err((409, "The image attached before is being posted.", None));
        }
        let size = sent;
        let staged_size = size.max(bytes.len() as u64);
        // The user's budget, for the hour before now.
        let now = Instant::now();
        let spent = state.spent.entry(user.to_string()).or_default();
        while spent
            .front()
            .is_some_and(|(at, _)| now.duration_since(*at) >= BUDGET_WINDOW)
        {
            spent.pop_front();
        }
        let bytes_spent: u64 = spent.iter().map(|(_, b)| b).sum();
        if spent.len() >= UPLOADS_AN_HOUR || bytes_spent + size > BYTES_AN_HOUR * declared.max_bytes
        {
            let retry = spent
                .front()
                .map(|(at, _)| BUDGET_WINDOW.saturating_sub(now.duration_since(*at)));
            return Err((
                429,
                "You have attached as many images as an hour allows; try again later.",
                retry,
            ));
        }
        let replaced = state.leases.get(session).map_or(0, |l| l.bytes);
        if state.staged_bytes - replaced + staged_size > STAGED_CAP {
            return Err((
                503,
                "Too many images are waiting to be posted; try again later.",
                None,
            ));
        }
        let key = self
            .staged
            .put(bytes)
            .map_err(|_| (500, "The image could not be kept.", None))?;
        // Held before the earlier lease lets its bytes go: the same bytes
        // attached again are the same blob, which ending it first forgot.
        *state.holds.entry(key.clone()).or_default() += 1;
        if let Some(old) = state.leases.remove(session) {
            self.end(&mut state, old);
        }
        state.next += 1;
        let id = format!("up-{}", state.next);
        state.staged_bytes += staged_size;
        state
            .spent
            .entry(user.to_string())
            .or_default()
            .push_back((now, size));
        state.leases.insert(
            session.to_string(),
            Lease {
                id,
                key,
                kind: m.kind,
                width: m.width,
                height: m.height,
                bytes: staged_size,
                made: Instant::now(),
                claimed: false,
            },
        );
        Ok(())
    }

    /// **A lease ended**: its hold on its staged blob given up, and the blob
    /// forgotten where no other lease holds it.
    fn end(&self, state: &mut State, lease: Lease) {
        state.staged_bytes -= lease.bytes;
        if let Some(holds) = state.holds.get_mut(&lease.key) {
            *holds -= 1;
            if *holds == 0 {
                state.holds.remove(&lease.key);
                let _ = self.staged.delete(&lease.key);
            }
        }
    }

    /// Every lease whose time is up and that nothing claims, ended.
    fn sweep(&self, state: &mut State) {
        let now = Instant::now();
        let over: Vec<String> = state
            .leases
            .iter()
            .filter(|(_, l)| !l.claimed && now.duration_since(l.made) >= self.lease_for)
            .map(|(s, _)| s.clone())
            .collect();
        for session in over {
            if let Some(lease) = state.leases.remove(&session) {
                self.end(state, lease);
            }
        }
    }

    /// **A committed image**, `<key>.<kind>`, from the deployment's blob
    /// storage, its type sniffed from its bytes again as it is served.
    fn serve(&self, name: &str, headers: &str) -> Answer {
        let Some((key, ext)) = name
            .split_once('.')
            .and_then(|(hex, ext)| Some((BlobKey::parse(hex)?, ext)))
        else {
            return Answer::text(404, "not found");
        };
        // **Served while a committed row names it** (ADR-0261): a deleted
        // post's image is not, whether or not its bytes are collected yet,
        // and a layer that cannot say is not taken to say yes.
        match self.named.get().map(|named| named(key.hex())) {
            Some(Ok(true)) => {}
            Some(Ok(false)) | None => return Answer::text(404, "not found"),
            Some(Err(why)) => {
                eprintln!("pw dev server: whether a post names {}: {why}", key.hex());
                return Answer::text(503, "unavailable");
            }
        }
        let found = self.blobs.get(&key).ok().flatten().and_then(|bytes| {
            let kind = sniff::sniff(&bytes)?;
            (kind.word() == ext).then_some((kind, bytes))
        });
        let Some((kind, bytes)) = found else {
            return Answer::text(404, "not found");
        };
        let etag = format!("\"{}\"", key.hex());
        let not_modified = header(headers, "if-none-match")
            .is_some_and(|v| v.split(',').any(|t| t.trim() == etag || t.trim() == "*"));
        let (status, body) = if not_modified {
            (304, Vec::new())
        } else {
            (200, bytes)
        };
        let mut a = Answer::new(status, kind.mime(), body);
        // What a key names never changes: any cache may keep it, forever.
        a.set(
            "cache-control",
            "public, max-age=31536000, immutable".to_string(),
        );
        a.set("etag", etag);
        a.set(
            "content-disposition",
            format!("inline; filename=\"{}.{}\"", key.hex(), kind.word()),
        );
        a.set("cross-origin-resource-policy", "same-origin".to_string());
        a
    }

    /// **A session's own lease**, shown to it before it posts: kept by no
    /// cache, and to no other session.
    fn preview(&self, id: &str, session: &str) -> Answer {
        let lease = {
            let state = self.state.lock().expect("uploads");
            state
                .leases
                .get(session)
                .filter(|l| l.id == id)
                .map(|l| (l.key.clone(), l.kind))
        };
        let Some((key, kind)) = lease else {
            return Answer::text(404, "not found");
        };
        match self.staged.get(&key) {
            Ok(Some(bytes)) => Answer::new(200, kind.mime(), bytes),
            _ => Answer::text(404, "not found"),
        }
    }

    // ---- What a command's transaction asks of the leases. ----

    /// **What `session` has leased**, as its page shows it.
    pub(crate) fn shown(&self, session: &str) -> Option<Shown> {
        let state = self.state.lock().expect("uploads");
        let lease = state.leases.get(session)?;
        let route = &self.declared.first()?.route;
        Some(Shown {
            src: format!("{route}/{}", lease.id),
            width: lease.width,
            height: lease.height,
        })
    }

    /// **A command claims `session`'s lease** until its transaction ends: no
    /// other command can, and no upload or sweep ends it.
    pub(crate) fn claim(&self, session: &str) -> Option<Claimed> {
        let mut state = self.state.lock().expect("uploads");
        let lease = state.leases.get_mut(session).filter(|l| !l.claimed)?;
        lease.claimed = true;
        Some(Claimed {
            session: session.to_string(),
            id: lease.id.clone(),
            key: lease.key.clone(),
            kind: lease.kind,
            width: lease.width,
            height: lease.height,
        })
    }

    /// **A claimed lease's bytes, put in the deployment's blob storage**,
    /// before the transaction that commits its post: a committed post never
    /// names bytes the deployment does not have.
    pub(crate) fn keep(&self, claimed: &Claimed) -> Result<(), String> {
        let bytes = self
            .staged
            .get(&claimed.key)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("lease {} has no bytes", claimed.id))?;
        let key = self.blobs.put(&bytes).map_err(|e| e.to_string())?;
        if key != claimed.key {
            return Err(format!("lease {} kept as another blob", claimed.id));
        }
        Ok(())
    }

    /// **A claim whose post committed**: the lease ended, its post holding
    /// the image in the deployment's storage.
    pub(crate) fn committed(&self, claimed: Claimed) {
        self.take(claimed);
    }

    /// **A claim the command discarded, and committed**: the lease ended.
    pub(crate) fn discarded(&self, claimed: Claimed) {
        self.take(claimed);
    }

    fn take(&self, claimed: Claimed) {
        let mut state = self.state.lock().expect("uploads");
        let ours = state
            .leases
            .get(&claimed.session)
            .is_some_and(|l| l.id == claimed.id && l.claimed);
        if ours && let Some(lease) = state.leases.remove(&claimed.session) {
            self.end(&mut state, lease);
        }
    }

    /// **A claim no transaction committed, given back**: the lease is its
    /// session's again, to post or to replace.
    pub(crate) fn unclaim(&self, claimed: Claimed) {
        let mut state = self.state.lock().expect("uploads");
        if let Some(lease) = state
            .leases
            .get_mut(&claimed.session)
            .filter(|l| l.id == claimed.id)
        {
            lease.claimed = false;
        }
    }

    /// **A blob no post names any more, deleted** (ADR-0261), once the
    /// transaction that deleted its last post has committed. `named` is
    /// whether a committed row still names it, which the caller reads while
    /// no other command can commit. Kept while a command holds a claim to
    /// the same bytes: its post will name them. The claims are read and the
    /// blob deleted under the leases' lock, so a claim taken after is one
    /// whose `keep` puts the bytes back before its post commits. Answers
    /// whether it is gone.
    pub(crate) fn collect(&self, key: &str, named: bool) -> Result<bool, String> {
        let Some(key) = BlobKey::parse(key) else {
            return Err(format!("`{key}` is no blob's key"));
        };
        let state = self.state.lock().expect("uploads");
        if named || state.leases.values().any(|l| l.claimed && l.key == key) {
            return Ok(false);
        }
        self.blobs.delete(&key).map_err(|e| e.to_string())?;
        Ok(true)
    }

    /// How many blobs its leases hold, and how many its posts committed.
    #[cfg(test)]
    pub(crate) fn counted(&self) -> (usize, usize) {
        (
            self.staged.count().expect("staged"),
            self.blobs.count().expect("blobs"),
        )
    }

    /// Where a committed image is served.
    #[cfg(test)]
    pub(crate) fn src(&self, key: &BlobKey, kind: Kind) -> String {
        format!("{}/{}.{}", self.serves(), key.hex(), kind.word())
    }
}

// ---- What the feed's data layers keep of an upload, and how. ----

use pw_host::engine::{HostFn, Val};

/// **An image as a post keeps it** (the feed's `Row`, and its columns on
/// PostgreSQL): its blob's key and kind, its width and height as a browser
/// shows it, and its author's words for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ImageRow {
    pub key: String,
    pub kind: Kind,
    pub width: u32,
    pub height: u32,
    pub alt: String,
}

impl ImageRow {
    /// **The program's `Image`**: where it is served, its size, its words.
    pub(crate) fn val(&self, serves: &str) -> Val {
        Val::Record(vec![
            (
                "src".into(),
                Val::String(format!("{serves}/{}.{}", self.key, self.kind.word())),
            ),
            ("width".into(), Val::S64(self.width as i64)),
            ("height".into(), Val::S64(self.height as i64)),
            ("alt".into(), Val::String(self.alt.clone())),
        ])
    }
}

/// **A post's images, as the program's `images: List<Image>`**: none, or
/// the one it carries.
pub(crate) fn images_val(image: Option<&ImageRow>, leases: Option<&Leases>) -> Val {
    let serves = leases.map_or("/images", |l| l.serves());
    Val::List(image.map(|i| i.val(serves)).into_iter().collect())
}

impl Shared {
    /// Where committed images are served.
    pub(crate) fn serves(&self) -> &str {
        self.declared
            .first()
            .map_or("/images", |d| d.serves.as_str())
    }
}

/// **The read a page makes of its session's lease**, the program's
/// `List<Attached>`, none or one: `feed:data/uploads#attached`.
pub(crate) fn attached_op(leases: Option<Leases>) -> HostFn {
    Arc::new(move |args: &[Val]| match args {
        [Val::String(session)] => {
            let shown = leases.as_ref().and_then(|l| l.shown(session));
            Ok(vec![Val::List(
                shown
                    .map(|s| {
                        Val::Record(vec![
                            ("src".into(), Val::String(s.src)),
                            ("width".into(), Val::S64(s.width as i64)),
                            ("height".into(), Val::S64(s.height as i64)),
                        ])
                    })
                    .into_iter()
                    .collect(),
            )])
        }
        other => Err(format!("uploads#attached received {other:?}")),
    })
}

/// What a command did with a claim.
#[derive(Debug, PartialEq, Eq)]
enum Fate {
    Held,
    Committed,
    Discarded,
}

/// **What one command's transaction holds of the leases**: each claim, and
/// what the command did with it. Ended by [`Claims::settle`] once the
/// transaction commits; dropped without it, every claim is given back, so a
/// command that fails, refuses or traps leaves its session's lease as it was.
pub(crate) struct Claims {
    leases: Option<Leases>,
    held: Vec<(Claimed, Fate)>,
    settled: bool,
}

/// A command's claims, shared by its operations.
pub(crate) type Claiming = Arc<Mutex<Claims>>;

impl Claims {
    pub(crate) fn new(leases: Option<Leases>) -> Claiming {
        Arc::new(Mutex::new(Claims {
            leases,
            held: Vec::new(),
            settled: false,
        }))
    }

    /// **The image a claim held by this command commits**, `upload` of
    /// `session`, marked committed with its post; none where this command
    /// holds no such claim, or has ended it.
    pub(crate) fn commit(&mut self, session: &str, upload: &str, alt: &str) -> Option<ImageRow> {
        let (claimed, fate) = self
            .held
            .iter_mut()
            .find(|(c, f)| c.session == session && c.id == upload && *f == Fate::Held)?;
        *fate = Fate::Committed;
        Some(ImageRow {
            key: claimed.key.hex().to_string(),
            kind: claimed.kind,
            width: claimed.width,
            height: claimed.height,
            alt: alt.to_string(),
        })
    }

    /// **The images this command commits, put in the deployment's blob
    /// storage**: before its transaction commits, so a committed post never
    /// names bytes the deployment does not have. An error refuses the commit.
    pub(crate) fn keep(&self) -> Result<(), String> {
        let Some(leases) = &self.leases else {
            return Ok(());
        };
        for (claimed, fate) in &self.held {
            if *fate == Fate::Committed {
                leases.keep(claimed)?;
            }
        }
        Ok(())
    }

    /// **The transaction committed**: each committed or discarded claim's
    /// lease ended, and any other given back.
    pub(crate) fn settle(&mut self) {
        self.settled = true;
        let Some(leases) = self.leases.clone() else {
            return;
        };
        for (claimed, fate) in self.held.drain(..) {
            match fate {
                Fate::Committed => leases.committed(claimed),
                Fate::Discarded => leases.discarded(claimed),
                Fate::Held => leases.unclaim(claimed),
            }
        }
    }
}

impl Drop for Claims {
    fn drop(&mut self) {
        if self.settled {
            return;
        }
        if let Some(leases) = self.leases.clone() {
            for (claimed, _) in self.held.drain(..) {
                leases.unclaim(claimed);
            }
        }
    }
}

/// **A command claims its session's lease**: `feed:data/uploads#claim`,
/// the program's `resource.acquire<Upload>`. The handle is the lease's id;
/// none leased, or claimed by another command, is not found.
pub(crate) fn claim_op(claims: Claiming) -> HostFn {
    Arc::new(move |args: &[Val]| match args {
        [Val::String(session)] => {
            let mut claims = claims.lock().expect("claims");
            let claimed = claims.leases.as_ref().and_then(|l| l.claim(session));
            Ok(vec![match claimed {
                Some(claimed) => {
                    let id = claimed.id.clone();
                    claims.held.push((claimed, Fate::Held));
                    crate::feed::ok(Val::String(id))
                }
                None => crate::feed::not_found(),
            }])
        }
        other => Err(format!("uploads#claim received {other:?}")),
    })
}

/// **A command discards the claim it holds**: `feed:data/uploads#discard`,
/// the program's `resource.release<Upload>`. Its lease ends when the
/// command commits, and what it showed is the answer.
pub(crate) fn discard_op(claims: Claiming, staged: impl Fn() + Send + Sync + 'static) -> HostFn {
    Arc::new(move |args: &[Val]| match args {
        [Val::String(session), Val::String(upload)] => {
            let mut claims = claims.lock().expect("claims");
            let serves_route = claims
                .leases
                .as_ref()
                .and_then(|l| l.declared.first().map(|d| d.route.clone()));
            let Some((claimed, fate)) = claims
                .held
                .iter_mut()
                .find(|(c, f)| c.session == *session && c.id == *upload && *f == Fate::Held)
            else {
                return Ok(vec![crate::feed::not_found()]);
            };
            *fate = Fate::Discarded;
            let shown = Val::Record(vec![
                (
                    "src".into(),
                    Val::String(format!(
                        "{}/{}",
                        serves_route.unwrap_or_default(),
                        claimed.id
                    )),
                ),
                ("width".into(), Val::S64(claimed.width as i64)),
                ("height".into(), Val::S64(claimed.height as i64)),
            ]);
            staged();
            Ok(vec![crate::feed::ok(shown)])
        }
        other => Err(format!("uploads#discard received {other:?}")),
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Domain expectations in mutation tests must fail at a native assert
    /// site; an unwrap panic alone is not valid mutation evidence.
    pub(crate) trait Required {
        type Value;
        fn required(self, why: &str) -> Self::Value;
    }

    impl<T> Required for Option<T> {
        type Value = T;
        fn required(self, why: &str) -> T {
            assert!(self.is_some(), "{why}");
            match self {
                Some(value) => value,
                None => unreachable!(),
            }
        }
    }

    impl<T, E> Required for Result<T, E> {
        type Value = T;
        fn required(self, why: &str) -> T {
            assert!(self.is_ok(), "{why}");
            match self {
                Ok(value) => value,
                Err(_) => unreachable!(),
            }
        }
    }

    use std::io::Cursor;

    pub(crate) fn declared() -> Declared {
        Declared {
            name: "PostImage".into(),
            route: "/uploads/post-image".into(),
            serves: "/images".into(),
            max_bytes: 4096,
            types: vec!["png".into(), "jpeg".into(), "webp".into(), "gif".into()],
            max_width: 640,
            max_height: 480,
        }
    }

    pub(crate) fn uploads(dir: &std::path::Path, d: Declared, lease_for: Duration) -> Uploads {
        let open = |sub: &str| Box::new(LocalBlobs::new(dir.join(sub)).expect("blobs"));
        Uploads::with(vec![d], open("staged"), open("blobs"), lease_for)
    }

    /// A PNG of `w` by `h`: its signature and IHDR, which is what is read,
    /// and its IEND, which the host walks to as it keeps it (ADR-0383).
    pub(crate) fn png(w: u32, h: u32) -> Vec<u8> {
        let mut b = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        b.extend(13u32.to_be_bytes());
        let mut chunk = b"IHDR".to_vec();
        chunk.extend(w.to_be_bytes());
        chunk.extend(h.to_be_bytes());
        chunk.extend([8, 6, 0, 0, 0]);
        let crc = sniff::crc32(&chunk);
        b.extend(&chunk);
        b.extend(crc.to_be_bytes());
        b.extend(0u32.to_be_bytes());
        b.extend(b"IEND");
        b.extend(sniff::crc32(b"IEND").to_be_bytes());
        b
    }

    /// A form posting `file` as `image`, named and typed as a sender likes.
    pub(crate) fn form(file: &[u8], filename: &str, mime: &str) -> (String, Vec<u8>) {
        let boundary = "----pwBoundary7MA4YWxk";
        let mut body = format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"image\"; \
             filename=\"{filename}\"\r\nContent-Type: {mime}\r\n\r\n"
        )
        .into_bytes();
        body.extend(file);
        body.extend(format!("\r\n--{boundary}--\r\n").as_bytes());
        (format!("multipart/form-data; boundary={boundary}"), body)
    }

    pub(crate) fn headers_for(content_type: &str, length: usize) -> String {
        format!(
            "Host: 127.0.0.1:3143\r\nOrigin: http://127.0.0.1:3143\r\n\
             Sec-Fetch-Site: same-origin\r\nReferer: http://127.0.0.1:3143/\r\n\
             Content-Type: {content_type}\r\nContent-Length: {length}\r\n"
        )
    }

    fn shared(u: &Uploads) -> &Shared {
        u.shared.as_deref().expect("declared")
    }

    fn post(u: &Uploads, session: &str, file: &[u8], filename: &str, mime: &str) -> Answer {
        let (ct, body) = form(file, filename, mime);
        let headers = headers_for(&ct, body.len());
        shared(u).answer(
            "POST",
            "/uploads/post-image",
            &headers,
            session,
            false,
            &mut Cursor::new(body),
        )
    }

    fn get(u: &Uploads, route: &str, session: &str, headers: &str) -> Answer {
        shared(u).answer(
            "GET",
            route,
            headers,
            session,
            false,
            &mut Cursor::new(Vec::new()),
        )
    }

    fn dir() -> tempfile::TempDir {
        tempfile::TempDir::with_prefix("pw-uploads-").expect("dir")
    }

    /// How many blobs `dir` holds.
    pub(crate) fn files(dir: &std::path::Path) -> usize {
        std::fs::read_dir(dir)
            .map(|d| {
                d.flat_map(|d| std::fs::read_dir(d.unwrap().path()).unwrap())
                    .count()
            })
            .unwrap_or(0)
    }

    fn body(a: &Answer) -> String {
        String::from_utf8_lossy(&a.body).into_owned()
    }

    #[test]
    fn nothing_is_claimed_where_nothing_is_declared() {
        let u = Uploads::default();
        assert!(!u.claims("POST", "/uploads/post-image"));
        assert!(!u.claims("GET", "/images/x.png"));
        let d = dir();
        let none = Uploads::from_build(d.path(), d.path()).expect("no uploads.json");
        assert!(none.leases().is_none());
    }

    #[test]
    fn a_declarations_routes_are_claimed_and_no_other() {
        let d = dir();
        let u = uploads(d.path(), declared(), LEASE);
        assert!(u.claims("POST", "/uploads/post-image"));
        assert!(u.claims("GET", "/images/abc.png"));
        assert!(u.claims("HEAD", "/images/abc.png"));
        assert!(u.claims("GET", "/uploads/post-image/up-1"));
        assert!(!u.claims("GET", "/uploads/post-image"));
        assert!(!u.claims("POST", "/images/abc.png"));
        assert!(!u.claims("PUT", "/uploads/post-image"));
        assert!(!u.claims("GET", "/imagesx/abc.png"));
        assert!(!u.claims("GET", "/images/"));
        assert!(!u.claims("POST", "/command/feed.app.post"));
    }

    #[test]
    fn a_build_declares_its_uploads_and_a_server_starts_with_no_lease() {
        let d = dir();
        let json = serde_json::to_string(&vec![declared()]).unwrap();
        std::fs::write(d.path().join("uploads.json"), json).unwrap();
        let root = d.path().join("root");
        let kept = LocalBlobs::new(&root).unwrap().put(b"a post's").unwrap();
        let u = Uploads::from_build(d.path(), &root).expect("built");
        assert!(u.claims("POST", "/uploads/post-image"));
        let leases = u.leases().expect("declared");
        assert_eq!(
            leases.staged.count().unwrap(),
            0,
            "no lease outlives its server"
        );
        assert_eq!(
            leases.blobs.count().unwrap(),
            1,
            "what a post committed stays"
        );
        assert_eq!(leases.blobs.get(&kept).unwrap(), Some(b"a post's".to_vec()));
    }

    #[test]
    fn an_image_is_leased_to_its_session_and_shown_to_it_alone() {
        let d = dir();
        let u = uploads(d.path(), declared(), LEASE);
        let a = post(&u, "s-1", &png(64, 48), "../../../etc/passwd", "text/html");
        assert_eq!(a.status, 303, "{}", body(&a));
        assert_eq!(a.header("location"), Some("/"));
        let shown = shared(&u).shown("s-1").expect("leased");
        assert_eq!(
            shown,
            Shown {
                src: "/uploads/post-image/up-1".into(),
                width: 64,
                height: 48
            }
        );
        let mine = get(&u, &shown.src, "s-1", "");
        assert_eq!(
            (mine.status, mine.header("content-type")),
            (200, Some("image/png"))
        );
        assert_eq!(mine.header("cache-control"), Some("private, no-store"));
        assert_eq!(mine.header("x-content-type-options"), Some("nosniff"));
        assert_eq!(
            get(&u, &shown.src, "s-2", "").status,
            404,
            "another's lease"
        );
        // Not served where committed images are: no post holds it.
        let key = BlobKey::of(&png(64, 48));
        let at = format!("/images/{}.png", key.hex());
        assert_eq!(get(&u, &at, "s-1", "").status, 404);
        // Kept under its key, and nowhere a sender's name reaches.
        let staged = d.path().join("staged");
        assert!(staged.join(&key.hex()[..2]).join(key.hex()).is_file());
        assert_eq!(files(&staged), 1);
        assert_eq!(files(&d.path().join("blobs")), 0);
    }

    #[test]
    fn the_kind_is_sniffed_and_never_the_senders() {
        let d = dir();
        let u = uploads(d.path(), declared(), LEASE);
        let html = b"<!doctype html><script>alert(document.cookie)</script>";
        let a = post(&u, "s-1", html, "cat.png", "image/png");
        assert_eq!(a.status, 415);
        assert!(body(&a).contains("not an image"));
        let svg = b"<svg xmlns=\"http://www.w3.org/2000/svg\" onload=\"alert(1)\"/>";
        assert_eq!(post(&u, "s-1", svg, "x.svg", "image/svg+xml").status, 415);
        // A PNG sent as a JPEG is a PNG.
        assert_eq!(
            post(&u, "s-1", &png(2, 2), "x.jpg", "image/jpeg").status,
            303
        );
        let shown = shared(&u).shown("s-1").expect("leased");
        let served = get(&u, &shown.src, "s-1", "");
        assert_eq!(served.header("content-type"), Some("image/png"));
        assert_eq!(files(&d.path().join("staged")), 1, "only the image kept");
    }

    #[test]
    fn a_kind_the_declaration_does_not_allow_is_refused() {
        let d = dir();
        let mut only_jpeg = declared();
        only_jpeg.types = vec!["jpeg".into()];
        let u = uploads(d.path(), only_jpeg, LEASE);
        let a = post(&u, "s-1", &png(2, 2), "x.jpg", "image/jpeg");
        assert_eq!(a.status, 415);
        assert!(body(&a).contains("A PNG image cannot be attached: only JPEG."));
    }

    #[test]
    fn the_size_is_held_before_and_after_the_body_is_read() {
        let d = dir();
        let u = uploads(d.path(), declared(), LEASE);
        // At the limit, and one byte over it.
        let mut at = png(2, 2);
        at.resize(4096, 0);
        assert_eq!(post(&u, "s-1", &at, "a.png", "image/png").status, 303);
        at.push(0);
        let a = post(&u, "s-2", &at, "a.png", "image/png");
        assert_eq!(a.status, 413);
        assert!(body(&a).contains("larger than"));
        // A length over the limit is refused before a byte is read.
        let over = 4096 + FORM_OVERHEAD as usize + 1;
        let headers = headers_for("multipart/form-data; boundary=b", over);
        let mut never = Cursor::new(vec![7u8; 10]);
        let a = shared(&u).answer(
            "POST",
            "/uploads/post-image",
            &headers,
            "s-3",
            false,
            &mut never,
        );
        assert_eq!(a.status, 413);
        assert_eq!(never.position(), 0, "nothing of the body read");
        assert!(
            a.unread > 0,
            "and what is drained after the answer is bounded"
        );
        assert!(a.unread <= 2 * (4096 + FORM_OVERHEAD));
    }

    #[test]
    fn the_dimensions_are_held() {
        let d = dir();
        let u = uploads(d.path(), declared(), LEASE);
        assert_eq!(
            post(&u, "s-1", &png(640, 480), "a.png", "image/png").status,
            303
        );
        let a = post(&u, "s-2", &png(641, 480), "a.png", "image/png");
        assert_eq!(a.status, 422);
        assert!(body(&a).contains("641 by 480 pixels"));
        assert_eq!(
            post(&u, "s-2", &png(640, 481), "a.png", "image/png").status,
            422
        );
        let mut bad = png(4, 4);
        bad[20] ^= 1; // the height, its CRC not
        assert_eq!(post(&u, "s-2", &bad, "a.png", "image/png").status, 422);
    }

    #[test]
    fn a_request_without_a_session_length_or_origin_is_refused() {
        let d = dir();
        let u = uploads(d.path(), declared(), LEASE);
        let (ct, form_body) = form(&png(2, 2), "a.png", "image/png");
        let ok = headers_for(&ct, form_body.len());
        let run = |headers: &str, fresh: bool| {
            shared(&u)
                .answer(
                    "POST",
                    "/uploads/post-image",
                    headers,
                    "s-1",
                    fresh,
                    &mut Cursor::new(form_body.clone()),
                )
                .status
        };
        assert_eq!(run(&ok, true), 403, "a session the browser was not given");
        let cross = ok.replace("Sec-Fetch-Site: same-origin", "Sec-Fetch-Site: cross-site");
        assert_eq!(run(&cross, false), 403);
        let other = ok.replace("Sec-Fetch-Site: same-origin\r\n", "").replace(
            "Origin: http://127.0.0.1:3143",
            "Origin: http://evil.example",
        );
        assert_eq!(run(&other, false), 403);
        let chunked = format!("{ok}Transfer-Encoding: chunked\r\n");
        assert_eq!(run(&chunked, false), 411);
        let unstated = ok.replace(&format!("Content-Length: {}\r\n", form_body.len()), "");
        assert_eq!(run(&unstated, false), 411);
        let plain = ok.replace(&ct, "image/png");
        assert_eq!(run(&plain, false), 415);
        assert_eq!(run(&ok, false), 303, "the control");
    }

    #[test]
    fn a_refusal_goes_back_to_this_origin_alone() {
        assert_eq!(
            back("Host: a:1\r\nReferer: http://a:1/post/p1?x=1\r\n"),
            "/post/p1?x=1"
        );
        assert_eq!(
            back("Host: a:1\r\nReferer: http://evil.example/post\r\n"),
            "/"
        );
        assert_eq!(
            back("Host: a:1\r\nReferer: http://a:1//evil.example/\r\n"),
            "/"
        );
        assert_eq!(back("Host: a:1\r\n"), "/");
        let page = Answer::refused(415, "<b>", "/\"><script>");
        let text = String::from_utf8(page.body).unwrap();
        assert!(
            !text.contains("<b>") && !text.contains("<script>"),
            "{text}"
        );
    }

    #[test]
    fn a_second_upload_ends_the_first_lease_and_its_blob() {
        let d = dir();
        let u = uploads(d.path(), declared(), LEASE);
        let staged = d.path().join("staged");
        assert_eq!(
            post(&u, "s-1", &png(2, 2), "a.png", "image/png").status,
            303
        );
        assert_eq!(
            post(&u, "s-1", &png(3, 3), "a.png", "image/png").status,
            303
        );
        assert_eq!(files(&staged), 1, "the first blob forgotten");
        assert_eq!(shared(&u).shown("s-1").map(|s| s.width), Some(3));
        // The same bytes from two sessions are one blob, held twice.
        assert_eq!(
            post(&u, "s-2", &png(3, 3), "b.png", "image/png").status,
            303
        );
        assert_eq!(
            post(&u, "s-1", &png(4, 4), "a.png", "image/png").status,
            303
        );
        assert_eq!(files(&staged), 2, "s-2 still holds the 3 by 3");
    }

    /// **The same image attached twice by one session is kept** (track
    /// `upload-metadata`'s finding): the second lease holds its bytes before
    /// the first lets them go, so no lease names bytes its staging forgot.
    #[test]
    fn the_same_image_attached_twice_is_kept() {
        let d = dir();
        let u = uploads(d.path(), declared(), LEASE);
        let s = shared(&u);
        for _ in 0..2 {
            assert_eq!(
                post(&u, "s-1", &png(3, 3), "a.png", "image/png").status,
                303
            );
        }
        let shown = s.shown("s-1").expect("leased");
        assert_eq!(get(&u, &shown.src, "s-1", "").status, 200, "shown to it");
        assert_eq!(files(&d.path().join("staged")), 1);
        let claimed = s.claim("s-1").expect("claimed");
        s.keep(&claimed)
            .expect("its bytes kept, so its post commits");
        s.committed(claimed);
        assert_eq!(files(&d.path().join("staged")), 0, "forgotten, posted");
    }

    /// **An image is leased, shown and kept as it is rewritten** (ADR-0383):
    /// without what names a place, a device, a person or a time, by the
    /// rewritten bytes' key, in the deployment's storage as in staging.
    #[test]
    fn an_image_is_leased_and_kept_without_its_metadata() {
        let d = dir();
        let u = uploads(d.path(), declared(), LEASE);
        let s = shared(&u);
        let laden = strip::tests::laden_jpeg();
        assert_eq!(strip::tests::identified(&laden).len(), 9);
        let kept = strip::strip(&laden).expect("kept");
        let key = BlobKey::of(&kept);
        let a = post(&u, "s-1", &laden, "IMG_0001.jpg", "image/jpeg");
        assert_eq!(a.status, 303, "{}", body(&a));
        let shown = s.shown("s-1").expect("leased");
        assert_eq!((shown.width, shown.height), (5, 8), "turned, as taken");
        let mine = get(&u, &shown.src, "s-1", "");
        assert_eq!(mine.body, kept, "its author sees what is kept");
        assert_eq!(strip::tests::identified(&mine.body), Vec::<&str>::new());
        let staged = d.path().join("staged");
        assert!(staged.join(&key.hex()[..2]).join(key.hex()).is_file());
        assert_eq!(files(&staged), 1, "and nothing else");
        let claimed = s.claim("s-1").expect("claimed");
        assert_eq!(claimed.key, key);
        s.keep(&claimed).expect("kept");
        s.committed(claimed);
        let blob = d.path().join("blobs").join(&key.hex()[..2]).join(key.hex());
        assert_eq!(std::fs::read(blob).expect("committed"), kept);
    }

    /// **Measured as it is kept, which is what is served**: of two Exif
    /// structures, a browser turns a JPEG by the first it reads, and the
    /// rewrite keeps that one alone, so a page states the size the browser
    /// shows (read from the bytes as sent, the second said unturned).
    #[test]
    fn an_image_is_measured_as_it_is_kept() {
        let d = dir();
        let u = uploads(d.path(), declared(), LEASE);
        let base = strip::tests::fixture("eight-by-five.jpeg");
        let exif = |o: u16| {
            let mut app1 = vec![0xFF, 0xE1];
            let data = [&b"Exif\0\0"[..], &strip::tests::tiff(o)].concat();
            app1.extend(((data.len() + 2) as u16).to_be_bytes());
            app1.extend(data);
            app1
        };
        let twice = [&base[..2], &exif(6), &exif(1), &base[2..]].concat();
        assert_eq!(sniff::measure(&twice).map(|m| m.width), Ok(8), "as sent");
        assert_eq!(post(&u, "s-1", &twice, "a.jpg", "image/jpeg").status, 303);
        let shown = shared(&u).shown("s-1").expect("leased");
        assert_eq!((shown.width, shown.height), (5, 8));
    }

    /// **A file whose container does not parse is refused whole**: a JPEG
    /// whose header reads, cut before its EOI, is no image to rewrite, and
    /// nothing of it is staged.
    #[test]
    fn a_file_the_rewrite_cannot_walk_is_refused_whole() {
        let d = dir();
        let u = uploads(d.path(), declared(), LEASE);
        let base = strip::tests::fixture("eight-by-five.jpeg");
        let cut = &base[..base.len() - 2];
        assert!(sniff::measure(cut).is_ok(), "its header reads");
        let a = post(&u, "s-1", cut, "a.jpg", "image/jpeg");
        assert_eq!(a.status, 422);
        assert!(
            body(&a).contains("The file is not a whole JPEG image: it ends before its EOI."),
            "{}",
            body(&a)
        );
        assert!(shared(&u).shown("s-1").is_none());
        assert_eq!(files(&d.path().join("staged")), 0);
    }

    #[test]
    fn a_lease_nothing_claims_ends_when_its_time_does() {
        let d = dir();
        let u = uploads(d.path(), declared(), Duration::ZERO);
        let s = shared(&u);
        assert_eq!(
            post(&u, "s-1", &png(2, 2), "a.png", "image/png").status,
            303
        );
        let claimed = s.claim("s-1").expect("claimed before the sweep");
        assert_eq!(
            post(&u, "s-2", &png(3, 3), "b.png", "image/png").status,
            303
        );
        assert!(s.shown("s-1").is_some(), "a claimed lease is not swept");
        s.unclaim(claimed);
        assert_eq!(
            post(&u, "s-3", &png(4, 4), "c.png", "image/png").status,
            303
        );
        assert!(s.shown("s-1").is_none(), "swept once given back");
        assert!(s.shown("s-2").is_none());
        assert_eq!(files(&d.path().join("staged")), 1, "s-3's alone");
    }

    #[test]
    fn a_lease_is_claimed_once_and_ended_once() {
        let d = dir();
        let u = uploads(d.path(), declared(), LEASE);
        let s = shared(&u);
        assert!(s.claim("s-1").is_none(), "nothing leased");
        assert_eq!(
            post(&u, "s-1", &png(64, 48), "a.png", "image/png").status,
            303
        );
        let claimed = s.claim("s-1").expect("claimed");
        assert!(s.claim("s-1").is_none(), "claimed once");
        assert_eq!(
            post(&u, "s-1", &png(3, 3), "a.png", "image/png").status,
            409,
            "and not replaced while claimed"
        );
        assert_eq!((claimed.width, claimed.height), (64, 48));
        // Given back, it is claimed again; committed, it is gone.
        s.unclaim(claimed);
        let claimed = s.claim("s-1").expect("claimed again");
        s.keep(&claimed).expect("kept in the deployment's storage");
        let key = claimed.key.clone();
        s.committed(claimed);
        assert!(s.shown("s-1").is_none());
        assert!(s.claim("s-1").is_none());
        assert_eq!(files(&d.path().join("staged")), 0);
        assert_eq!(files(&d.path().join("blobs")), 1);
        // Served while a post names it, as the layer says (ADR-0261).
        let src = s.src(&key, Kind::Png);
        assert_eq!(
            get(&u, &src, "s-anyone", "").status,
            404,
            "no layer handed: none named"
        );
        let (named, erring) = (
            Arc::new(Mutex::new(std::collections::BTreeSet::<String>::new())),
            Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        u.named_by(Box::new({
            let (named, erring) = (named.clone(), erring.clone());
            move |key: &str| match erring.load(std::sync::atomic::Ordering::SeqCst) {
                true => Err("the layer cannot say".to_string()),
                false => Ok(named.lock().expect("named").contains(key)),
            }
        }));
        assert_eq!(get(&u, &src, "s-anyone", "").status, 404, "named by none");
        named.lock().expect("named").insert(key.hex().to_string());
        // Served now, as what it is, kept forever by any cache.
        assert_eq!(src, format!("/images/{}.png", key.hex()));
        let served = get(&u, &src, "s-anyone", "");
        assert_eq!(served.status, 200);
        assert_eq!(served.body, png(64, 48));
        assert_eq!(served.header("content-type"), Some("image/png"));
        assert_eq!(served.header("x-content-type-options"), Some("nosniff"));
        assert_eq!(
            served.header("cache-control"),
            Some("public, max-age=31536000, immutable")
        );
        assert_eq!(
            served.header("content-security-policy"),
            Some("default-src 'none'; sandbox")
        );
        let etag = served.header("etag").unwrap().to_string();
        let again = get(&u, &src, "s-anyone", &format!("If-None-Match: {etag}\r\n"));
        assert_eq!((again.status, again.body.len()), (304, 0));
        // Its kind is its bytes': the same key as another kind is nothing.
        assert_eq!(get(&u, &src.replace(".png", ".gif"), "s", "").status, 404);
        assert_eq!(get(&u, "/images/../../etc/passwd", "s", "").status, 404);
        // A layer that cannot say is not taken to say yes; and a post that
        // names it no more is served no more, its bytes kept or not.
        erring.store(true, std::sync::atomic::Ordering::SeqCst);
        assert_eq!(get(&u, &src, "s-anyone", "").status, 503);
        erring.store(false, std::sync::atomic::Ordering::SeqCst);
        named.lock().expect("named").clear();
        assert_eq!(get(&u, &src, "s-anyone", "").status, 404);
        assert_eq!(files(&d.path().join("blobs")), 1);

        // A second lease, discarded: its blob forgotten.
        assert_eq!(
            post(&u, "s-1", &png(5, 5), "b.png", "image/png").status,
            303
        );
        let claimed = s.claim("s-1").expect("claimed");
        s.discarded(claimed);
        assert!(s.shown("s-1").is_none());
        assert_eq!(files(&d.path().join("staged")), 0);
        assert_eq!(
            files(&d.path().join("blobs")),
            1,
            "the committed image alone"
        );
    }

    /// **A blob no post names is collected, and one a claim holds is kept**
    /// (ADR-0261): a command in flight with the same bytes will name them.
    #[test]
    fn a_blob_no_post_names_is_collected_unless_a_claim_holds_it() {
        let d = dir();
        let u = uploads(d.path(), declared(), LEASE);
        let s = shared(&u);
        let blobs = || files(&d.path().join("blobs"));
        assert_eq!(
            post(&u, "s-1", &png(64, 48), "a.png", "image/png").status,
            303
        );
        let claimed = s.claim("s-1").expect("claimed");
        s.keep(&claimed).expect("kept");
        let key = claimed.key.hex().to_string();
        s.committed(claimed);
        assert_eq!(blobs(), 1);
        // A post names it still: kept.
        assert_eq!(s.collect(&key, true), Ok(false));
        assert_eq!(blobs(), 1);
        // Another session's lease of the same bytes, claimed by a command in
        // flight that has kept them: kept, since its post will name them.
        assert_eq!(
            post(&u, "s-2", &png(64, 48), "b.png", "image/png").status,
            303
        );
        let theirs = s.claim("s-2").expect("claimed");
        s.keep(&theirs).expect("kept");
        assert_eq!(s.collect(&key, false), Ok(false));
        assert_eq!(blobs(), 1);
        // Given back, and named by no post: collected.
        s.unclaim(theirs);
        assert_eq!(s.collect(&key, false), Ok(true));
        assert_eq!(blobs(), 0);
        // Again: gone already, and no error.
        assert_eq!(s.collect(&key, false), Ok(true));
        // A key that is no key builds no path.
        assert!(s.collect("../../etc/passwd", false).is_err());
    }

    fn an_expanding_profile_image() -> Vec<u8> {
        let profile =
            icc::tests::profile(2, &[(vec![*b"desc"], b"desc\0\0\0\0\0\0\0\x01\0".to_vec())]);
        let data = [&b"ICC_PROFILE\0"[..], &[1, 1], &profile].concat();
        let mut sent = vec![0xff, 0xd8, 0xff, 0xe2];
        sent.extend(((data.len() + 2) as u16).to_be_bytes());
        sent.extend(data);
        sent.extend(&strip::tests::fixture("eight-by-five.jpeg")[2..]);
        assert!(strip::strip(&sent).required("rewritten").len() > sent.len());
        sent
    }

    #[test]
    fn staged_bytes_cover_expansion_but_hourly_bytes_count_the_upload() {
        let d = dir();
        let u = uploads(d.path(), declared(), LEASE);
        let sent = an_expanding_profile_image();
        let kept = strip::strip(&sent).unwrap();
        assert_eq!(post(&u, "growth", &sent, "a.jpg", "image/jpeg").status, 303);
        let shared = u.leases().unwrap();
        {
            let state = shared.state.lock().unwrap();
            assert_eq!(
                state.staged_bytes,
                kept.len() as u64,
                "actual retained bytes accounted"
            );
            assert_eq!(
                state.spent["u-growth"].back().unwrap().1,
                sent.len() as u64,
                "hourly incoming bytes"
            );
        }
        let small = png(2, 2);
        assert_eq!(post(&u, "growth", &small, "a.png", "image/png").status, 303);
        assert_eq!(
            shared.state.lock().unwrap().staged_bytes,
            small.len() as u64,
            "replacement releases expanded hold"
        );
        let claimed = shared.claim("growth").unwrap();
        shared.discarded(claimed);
        assert_eq!(shared.state.lock().unwrap().staged_bytes, 0);
    }

    #[test]
    fn expansion_cannot_cross_the_staging_cap() {
        let d = dir();
        let u = uploads(d.path(), declared(), LEASE);
        let sent = an_expanding_profile_image();
        let shared = u.leases().unwrap();
        let occupied = STAGED_CAP - sent.len() as u64;
        shared.state.lock().unwrap().staged_bytes = occupied;
        let answer = post(&u, "growth", &sent, "a.jpg", "image/jpeg");
        assert_eq!(
            answer.status, 503,
            "rewritten bytes exceed remaining capacity"
        );
        let state = shared.state.lock().unwrap();
        assert_eq!(state.staged_bytes, occupied);
        assert!(!state.leases.contains_key("growth"));
        assert!(state.spent["u-growth"].is_empty());
    }

    /// **A user's budget**: so many images an hour, and so many times the
    /// declaration's bytes, whichever is spent first; another user's is
    /// their own. Spent by an upload, posted or not.
    #[test]
    fn a_user_attaches_within_an_hours_budget() {
        let d = dir();
        let u = uploads(d.path(), declared(), LEASE);
        // Images: each small, the count spent first.
        for n in 0..UPLOADS_AN_HOUR {
            let a = post(&u, "s-1", &png(1 + n as u32, 1), "a.png", "image/png");
            assert_eq!(a.status, 303, "upload {n}");
        }
        let over = post(&u, "s-1", &png(64, 1), "a.png", "image/png");
        assert_eq!(over.status, 429);
        assert!(body(&over).contains("as many images as an hour allows"));
        let retry: u64 = over.header("retry-after").expect("when").parse().unwrap();
        assert!((1..=3600).contains(&retry), "{retry}");
        assert_eq!(
            post(&u, "s-2", &png(64, 1), "a.png", "image/png").status,
            303
        );
        // Bytes: each at the declaration's most, the bytes spent first; the
        // bytes sent, though what is kept drops all past IEND (ADR-0383).
        let mut large = png(2, 2);
        large.resize(4096, 0);
        for n in 0..BYTES_AN_HOUR {
            large[100] = n as u8;
            assert_eq!(
                post(&u, "s-3", &large, "a.png", "image/png").status,
                303,
                "{n}"
            );
        }
        large[100] = 255;
        assert_eq!(post(&u, "s-3", &large, "a.png", "image/png").status, 429);
    }

    #[test]
    fn a_deployment_lowers_a_limit_and_never_raises_one() {
        let lower = |by: Lowered| lowered(vec![declared()], &by);
        let lowered_to = lower(Lowered {
            max_bytes: Some(1000),
            max_width: Some(320),
            max_height: None,
        })
        .expect("lowered");
        assert_eq!(
            (
                lowered_to[0].max_bytes,
                lowered_to[0].max_width,
                lowered_to[0].max_height
            ),
            (1000, 320, 480)
        );
        assert_eq!(
            lower(Lowered::default()).expect("as declared"),
            vec![declared()]
        );
        for raised in [
            Lowered {
                max_bytes: Some(4097),
                ..Lowered::default()
            },
            Lowered {
                max_width: Some(641),
                ..Lowered::default()
            },
            Lowered {
                max_height: Some(0),
                ..Lowered::default()
            },
        ] {
            let why = lower(raised).expect_err("refused");
            assert!(why.contains("deployment"), "{why}");
        }
        // Lowered, the host holds a browser to the lower.
        let d = dir();
        let mut small = declared();
        small.max_width = 63;
        let u = uploads(d.path(), small, LEASE);
        assert_eq!(
            post(&u, "s-1", &png(64, 48), "a.png", "image/png").status,
            422
        );
    }

    /// **A command that does not commit gives its claim back**: its claims
    /// dropped unsettled, whatever it did with them, and settled, each ended
    /// as the command said.
    #[test]
    fn a_claim_no_transaction_commits_is_given_back() {
        let d = dir();
        let u = uploads(d.path(), declared(), LEASE);
        let leases = u.leases().expect("declared");
        assert_eq!(
            post(&u, "s-1", &png(8, 8), "a.png", "image/png").status,
            303
        );
        let call = |op: &HostFn, args: &[Val]| op(args).expect("runs").remove(0);
        let id = {
            let claims = Claims::new(Some(leases.clone()));
            let claimed = call(&claim_op(claims.clone()), &[Val::String("s-1".into())]);
            let Val::Result(Ok(Some(id))) = claimed else {
                panic!("claimed: {claimed:?}")
            };
            let Val::String(id) = *id else { panic!() };
            assert!(claims.lock().unwrap().commit("s-1", &id, "words").is_some());
            assert!(leases.claim("s-1").is_none(), "held while the command runs");
            id
            // Dropped here, unsettled: the command failed.
        };
        assert!(
            leases.shown("s-1").is_some(),
            "the lease is the session's again"
        );
        let claims = Claims::new(Some(leases.clone()));
        let again = call(&claim_op(claims.clone()), &[Val::String("s-1".into())]);
        assert!(matches!(again, Val::Result(Ok(_))), "{again:?}");
        // Another session's handle, or one not claimed, is not found.
        let other = call(
            &discard_op(claims.clone(), || {}),
            &[Val::String("s-2".into()), Val::String(id.clone())],
        );
        assert!(matches!(other, Val::Result(Err(_))), "{other:?}");
        let discarded = call(
            &discard_op(claims.clone(), || {}),
            &[Val::String("s-1".into()), Val::String(id.clone())],
        );
        assert!(matches!(discarded, Val::Result(Ok(_))), "{discarded:?}");
        assert!(
            claims.lock().unwrap().commit("s-1", &id, "words").is_none(),
            "discarded, it is committed with no post"
        );
        claims.lock().unwrap().settle();
        assert!(leases.shown("s-1").is_none(), "settled, the lease is ended");
        assert_eq!(files(&d.path().join("staged")), 0);
        assert_eq!(files(&d.path().join("blobs")), 0, "a discard keeps nothing");
    }
}
