//! **An image on a post** (track `uploads`, ADR-0260): the feed's own
//! upload, attached by a form over HTTP through the seam `main.rs` gives it,
//! committed with its post by the program's command in the post's own
//! transaction, shown with its width, height and words, and served from
//! the deployment's blob storage; in memory, and on PostgreSQL where a
//! database is named. Each test states one case, with its control.

use super::*;

/// A fixture a real encoder wrote (`scripts/uploads_fixtures.py`).
fn fixture(name: &str) -> Vec<u8> {
    let at = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../e2e/uploads");
    std::fs::read(at.join(name)).expect("a fixture")
}

/// **One request over a connection, as a browser sends it**, through the
/// server's own `handle`, and its whole answer.
fn exchange(s: &Server, request: &[u8]) -> Vec<u8> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind");
    let at = listener.local_addr().expect("address");
    std::thread::scope(|scope| {
        scope.spawn(|| {
            let (stream, _) = listener.accept().expect("accept");
            handle(s, stream);
        });
        let mut client = TcpStream::connect(at).expect("connect");
        client.write_all(request).expect("request");
        let mut answer = Vec::new();
        let _ = client.read_to_end(&mut answer);
        answer
    })
}

/// The answer's head, as text, and its body.
fn split(answer: &[u8]) -> (String, Vec<u8>) {
    let end = answer
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("a head");
    (
        String::from_utf8_lossy(&answer[..end]).to_lowercase(),
        answer[end + 4..].to_vec(),
    )
}

/// **A form attaching `file`**, as `session`'s browser posts it from the
/// home page: same-origin, multipart, the file named as its sender likes.
fn attach(s: &Server, session: &str, file: &[u8]) -> String {
    let (content_type, body) = crate::uploads::tests::form(file, "../../photo.html", "text/html");
    let mut request = format!(
        "POST /uploads/post-image HTTP/1.1\r\nHost: 127.0.0.1:1\r\nCookie: pw-session={session}\r\n\
         Origin: http://127.0.0.1:1\r\nSec-Fetch-Site: same-origin\r\nReferer: http://127.0.0.1:1/\r\n\
         Content-Type: {content_type}\r\nContent-Length: {}\r\n\r\n",
        body.len()
    )
    .into_bytes();
    request.extend(body);
    split(&exchange(s, &request)).0
}

/// A `GET` of `path`, and its answer.
fn get(s: &Server, path: &str) -> (String, Vec<u8>) {
    split(&exchange(
        s,
        format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:1\r\nCookie: pw-session=anyone\r\n\r\n")
            .as_bytes(),
    ))
}

/// `session`'s page `page`, as the document has it.
fn page(s: &Server, session: &str, page: &str, params: &Params) -> String {
    s.serve_document_settled(session, page, params, &[])
        .expect("served")
        .0
}

fn post_image(s: &Server, session: &str, text: &str, alt: &str, interaction: &str) -> Answered {
    s.command_answered(
        "feed.app.post_image",
        session,
        &[Val::String(text.into()), Val::String(alt.into())],
        Some(interaction),
    )
    .expect("runs")
}

/// How many blobs the server's leases hold (`staged`), or its posts
/// committed to the deployment's blob storage.
fn blobs(s: &Server, which: &str) -> usize {
    let (staged, committed) = s.uploads.leases().expect("declared").counted();
    if which == "staged" { staged } else { committed }
}

/// **An image attached is posted with its post, and served from the
/// deployment's blob storage**: shown with its width, height and words to
/// every reader, in a timeline and in its thread.
fn attached_and_posted(s: &Server) {
    let png = fixture("six-by-four.png");
    let key = crate::blob::BlobKey::of(&png);
    let src = format!("/images/{}.png", key.hex());
    let home = page(s, "a", "feed.app.Home~ui", &Params::new());
    assert!(home.contains("action=\"/uploads/post-image\""), "{home}");
    assert!(!home.contains("attached-image"), "nothing attached yet");

    let head = attach(s, "a", &png);
    assert!(head.starts_with("http/1.1 303"), "{head}");
    assert!(head.contains("location: /\r\n"), "{head}");
    let home = page(s, "a", "feed.app.Home~ui", &Params::new());
    assert!(home.contains("class=\"attached-image\""), "{home}");
    assert!(
        home.contains("width=\"6\"") && home.contains("height=\"4\""),
        "{home}"
    );
    // Another reader sees none of it, and it is served to no one yet.
    assert!(!page(s, "b", "feed.app.Home~ui", &Params::new()).contains("attached-image"));
    assert!(get(s, &src).0.starts_with("http/1.1 404"));
    assert_eq!((blobs(s, "staged"), blobs(s, "blobs")), (1, 0));

    let answered = post_image(
        s,
        "a",
        "A gradient",
        "Red to the right, green downward",
        "i-1",
    );
    assert!(answered.committed, "{:?}", answered.result);
    let shown = format!(
        "class=\"photo\" src=\"{src}\" width=\"6\" height=\"4\" \
         alt=\"Red to the right, green downward\">"
    );
    let theirs = page(s, "b", "feed.app.Home~ui", &Params::new());
    assert!(theirs.contains(&shown), "{theirs}");
    // The newest post, first in the timeline, is the one just posted.
    let at = theirs.find("href=\"/post/").expect("a post's link") + "href=\"/post/".len();
    let id = theirs[at..at + theirs[at..].find('"').expect("its end")].to_string();
    let thread = page(
        s,
        "b",
        "feed.app.PostPage~ui",
        &Params::from([("id".to_string(), id)]),
    );
    assert!(thread.contains(&shown), "{thread}");
    // The author's lease is ended.
    let mine = page(s, "a", "feed.app.Home~ui", &Params::new());
    assert!(!mine.contains("attached-image"), "{mine}");
    assert_eq!((blobs(s, "staged"), blobs(s, "blobs")), (0, 1));

    let (head, body) = get(s, &src);
    assert!(head.starts_with("http/1.1 200"), "{head}");
    assert_eq!(body, png);
    for header in [
        "content-type: image/png",
        "x-content-type-options: nosniff",
        "cache-control: public, max-age=31536000, immutable",
        "content-security-policy: default-src 'none'; sandbox",
    ] {
        assert!(head.contains(header), "{header}: {head}");
    }
}

#[test]
fn an_image_attached_is_posted_with_its_post_and_served() {
    attached_and_posted(&served_feed());
}

/// **What names a place, a device, a person or a time is neither kept nor
/// served** (track `upload-metadata`, ADR-XXXX): a phone's photo, attached
/// over HTTP and posted, is shown turned as it was taken, and served by the
/// key of the bytes kept, which hold no GPS position, camera, time, XMP,
/// IPTC or comment.
#[test]
fn a_photos_metadata_is_neither_kept_nor_served() {
    use crate::uploads::strip::tests::{identified, laden_jpeg};
    let s = served_feed();
    let laden = laden_jpeg();
    assert_eq!(identified(&laden).len(), 6, "{:?}", identified(&laden));
    assert!(attach(&s, "a", &laden).starts_with("http/1.1 303"));
    let answered = post_image(&s, "a", "Where I was", "A gradient, turned", "i-1");
    assert!(answered.committed, "{:?}", answered.result);
    let theirs = page(&s, "b", "feed.app.Home~ui", &Params::new());
    let at = theirs.find("src=\"/images/").expect("an image") + "src=\"".len();
    let src = &theirs[at..at + theirs[at..].find('"').expect("its end")];
    let after = &theirs[at..];
    assert!(after.contains("width=\"5\" height=\"8\""), "{theirs}");
    let (head, body) = get(&s, src);
    assert!(head.starts_with("http/1.1 200"), "{head}");
    assert!(head.contains("content-type: image/jpeg"), "{head}");
    assert_eq!(identified(&body), Vec::<&str>::new());
    let key = crate::blob::BlobKey::of(&body);
    assert_eq!(src, format!("/images/{}.jpeg", key.hex()), "served as kept");
    assert_eq!(body, crate::uploads::strip::strip(&laden).expect("kept"));
}

/// **Posting an image with none attached is not found, and commits
/// nothing**; a post without one is a post as before.
fn none_attached(s: &Server) {
    let answered = post_image(s, "a", "No picture", "Nothing", "i-1");
    assert!(!answered.committed, "{:?}", answered.result);
    assert!(!page(s, "b", "feed.app.Home~ui", &Params::new()).contains("No picture"));
    let answered = s
        .command_answered(
            "feed.app.post",
            "a",
            &[Val::String("Words alone".into())],
            Some("i-2"),
        )
        .expect("runs");
    assert!(answered.committed);
    let home = page(s, "b", "feed.app.Home~ui", &Params::new());
    assert!(
        home.contains("Words alone") && !home.contains("class=\"photo\""),
        "{home}"
    );
}

#[test]
fn posting_an_image_with_none_attached_commits_nothing() {
    none_attached(&served_feed());
}

/// **An attached image removed is forgotten, and never served.**
fn discarded(s: &Server) {
    let gif = fixture("five-by-nine.gif");
    assert!(attach(s, "a", &gif).starts_with("http/1.1 303"));
    let answered = s
        .command_answered("feed.app.discard_image", "a", &[], Some("i-1"))
        .expect("runs");
    assert!(answered.committed, "{:?}", answered.result);
    assert!(!page(s, "a", "feed.app.Home~ui", &Params::new()).contains("attached-image"));
    assert_eq!((blobs(s, "staged"), blobs(s, "blobs")), (0, 0));
    let src = format!("/images/{}.gif", crate::blob::BlobKey::of(&gif).hex());
    assert!(get(s, &src).0.starts_with("http/1.1 404"));
    // Nothing is left to post.
    assert!(!post_image(s, "a", "Too late", "Nothing", "i-2").committed);
}

#[test]
fn an_attached_image_removed_is_forgotten() {
    discarded(&served_feed());
}

/// **A command that claims the image and does not commit gives it back**:
/// it is the session's still, and posted after.
#[test]
fn a_command_that_does_not_commit_gives_its_claim_back() {
    let s = served_feed_with(|app| {
        format!(
            "{app}\ncommand claim_and_refuse() -> Result<Post, FeedError>\n    \
             requires      SignedIn\n    \
             invalidates   Attached(current_session())\n{{\n    \
             let image = claim(current_session())?\n    \
             let _gone = discard(current_session(), image)\n    \
             Err(Unavailable)\n}}\n"
        )
    });
    assert!(attach(&s, "a", &fixture("eight-by-five.jpeg")).starts_with("http/1.1 303"));
    let refused = s
        .command_answered("feed.app.claim_and_refuse", "a", &[], Some("i-1"))
        .expect("runs");
    assert!(!refused.committed, "{:?}", refused.result);
    assert!(page(&s, "a", "feed.app.Home~ui", &Params::new()).contains("attached-image"));
    assert_eq!(blobs(&s, "staged"), 1);
    assert!(post_image(&s, "a", "Still mine", "A gradient", "i-2").committed);
}

/// **The upload's body is read by the upload, within its own limits**: a
/// form larger than a command's 64 KiB is read whole where the seam gives
/// it to the upload, and one past the program's 5 MB is refused before a
/// byte is kept.
#[test]
fn an_upload_is_read_within_its_own_limits_not_a_commands() {
    let s = served_feed();
    let mut large = fixture("six-by-four.png");
    large.resize(100 * 1024, 0);
    assert!(attach(&s, "a", &large).starts_with("http/1.1 303"));
    let mut over = fixture("six-by-four.png");
    over.resize(5_000_001, 0);
    let head = attach(&s, "b", &over);
    assert!(head.starts_with("http/1.1 413"), "{head}");
    // And a command's body is held to 64 KiB still.
    let posted = split(&exchange(
        &s,
        format!(
            "POST /command/feed.app.post HTTP/1.1\r\nHost: t\r\nCookie: pw-session=a\r\n\
             content-length: {}\r\n\r\n",
            70 * 1024
        )
        .as_bytes(),
    ))
    .0;
    assert!(posted.starts_with("http/1.1 413"), "{posted}");
    assert_eq!(blobs(&s, "staged"), 1, "a's alone");
}

/// **One signed out attaches nothing** (track `identity`, ADR-0258): the
/// development provider's accounts model, where a session no sign-in
/// opened is no one, and `requires SignedIn` holds a post to a principal.
#[test]
fn one_signed_out_attaches_nothing() {
    let s = served_feed();
    let provider =
        crate::accounts::DevProvider::start(&crate::identity::Deployment::development(3143))
            .expect("the development provider");
    s.identity
        .use_provider(Arc::new(provider), "http://127.0.0.1:3143/sign-in/callback");
    let head = attach(&s, "a", &fixture("six-by-four.png"));
    assert!(head.starts_with("http/1.1 403"), "{head}");
    assert_eq!(blobs(&s, "staged"), 0);
    // The control: the guest model, where every session is its own guest.
    s.identity.use_guests();
    assert!(attach(&s, "a", &fixture("six-by-four.png")).starts_with("http/1.1 303"));
}

/// **What is not an image is never kept**, whatever its sender calls it.
#[test]
fn what_is_not_an_image_is_refused_whatever_it_is_called() {
    let s = served_feed();
    let head = attach(&s, "a", &fixture("not-an-image.png"));
    assert!(head.starts_with("http/1.1 415"), "{head}");
    let head = attach(&s, "a", &fixture("too-wide.png"));
    assert!(head.starts_with("http/1.1 422"), "{head}");
    assert_eq!(blobs(&s, "staged"), 0);
    // The control: an image.
    assert!(attach(&s, "a", &fixture("ten-by-six-lossy.webp")).starts_with("http/1.1 303"));
}

/// The newest post's id, first in the home page's timeline.
fn newest(s: &Server) -> String {
    let home = page(s, "b", "feed.app.Home~ui", &Params::new());
    let at = home.find("href=\"/post/").expect("a post's link") + "href=\"/post/".len();
    home[at..at + home[at..].find('"').expect("its end")].to_string()
}

fn delete(s: &Server, session: &str, post: &str, interaction: &str) -> Answered {
    s.command_answered(
        "feed.app.delete",
        session,
        &[Val::String(post.into())],
        Some(interaction),
    )
    .expect("runs")
}

/// **A deleted post's image is no longer served, and its blob is collected
/// where no post names it** (ADR-0261): two posts of the same bytes share
/// one blob, which outlives the first's delete and not the second's.
fn deleted_and_collected(s: &Server) {
    let png = fixture("six-by-four.png");
    let src = format!("/images/{}.png", crate::blob::BlobKey::of(&png).hex());
    assert!(attach(s, "a", &png).starts_with("http/1.1 303"));
    assert!(post_image(s, "a", "First", "A gradient", "i-1").committed);
    let first = newest(s);
    assert!(attach(s, "a", &png).starts_with("http/1.1 303"));
    assert!(post_image(s, "a", "Second", "A gradient", "i-2").committed);
    let second = newest(s);
    assert_ne!(first, second);
    assert_eq!(blobs(s, "blobs"), 1, "the same bytes, one blob");
    // The first deleted: the second names the bytes still, which are served.
    assert!(delete(s, "a", &first, "i-3").committed);
    assert!(get(s, &src).0.starts_with("http/1.1 200"));
    assert_eq!(blobs(s, "blobs"), 1);
    // The second deleted: served to no one, and collected.
    assert!(delete(s, "a", &second, "i-4").committed);
    let (head, _) = get(s, &src);
    assert!(head.starts_with("http/1.1 404"), "{head}");
    assert_eq!(blobs(s, "blobs"), 0);
    // The control: a post's image is served until its post is deleted.
    assert!(attach(s, "a", &png).starts_with("http/1.1 303"));
    assert!(post_image(s, "a", "Third", "A gradient", "i-5").committed);
    assert!(get(s, &src).0.starts_with("http/1.1 200"));
    assert_eq!(blobs(s, "blobs"), 1);
}

#[test]
fn a_deleted_posts_image_is_not_served_and_its_blob_is_collected() {
    deleted_and_collected(&served_feed());
}

fn on_postgres(test: &str) -> Option<super::feed_pg::OnPostgres> {
    let url = match std::env::var("PW_FEED_DATABASE_URL") {
        Ok(url) if !url.is_empty() => url,
        _ => {
            eprintln!("skipped: PW_FEED_DATABASE_URL is not set");
            return None;
        }
    };
    Some(super::feed_pg::served_on(&url, test))
}

/// **On PostgreSQL, the image is the post's own row's**: written in its
/// transaction, read back with it, and all of it or none.
#[test]
fn on_postgres_an_image_is_kept_with_its_post() {
    let Some(s) = on_postgres("post_image") else {
        return;
    };
    attached_and_posted(&s);
    assert_eq!(
        s.db.count(
            "SELECT count(*) FROM posts WHERE image_key IS NOT NULL AND image_kind = 'png' \
             AND image_width = 6 AND image_height = 4"
        ),
        1
    );
    // A row with part of an image is refused by the database itself: the
    // test's own schema holds the constraint that says all five or none.
    // Its constraint found first, materialized, and only then read: a
    // condition beside the schema's could be evaluated first, on another
    // test's constraint as its schema is dropped ("could not open relation
    // with OID", found 2026-10-07).
    let whole = s.db.count(
        "WITH mine AS MATERIALIZED ( \
             SELECT c.oid FROM pg_constraint c JOIN pg_namespace n ON n.oid = c.connamespace \
             WHERE c.conname = 'posts_image_whole' AND n.nspname = current_schema()) \
         SELECT count(*) FROM mine WHERE pg_get_constraintdef(mine.oid) LIKE \
         '%num_nonnulls(image_key, image_kind, image_width, image_height, image_alt)%'",
    );
    assert_eq!(whole, 1);
}

#[test]
fn on_postgres_posting_an_image_with_none_attached_commits_nothing() {
    let Some(s) = on_postgres("post_image_none") else {
        return;
    };
    none_attached(&s);
    assert_eq!(
        s.db.count("SELECT count(*) FROM posts WHERE image_key IS NOT NULL"),
        0
    );
}

#[test]
fn on_postgres_a_deleted_posts_image_is_not_served_and_its_blob_is_collected() {
    let Some(s) = on_postgres("post_image_deleted") else {
        return;
    };
    deleted_and_collected(&s);
    assert_eq!(
        s.db.count("SELECT count(*) FROM posts WHERE image_key IS NOT NULL"),
        1,
        "the third post's alone"
    );
}

#[test]
fn on_postgres_an_attached_image_removed_is_forgotten() {
    let Some(s) = on_postgres("post_image_discard") else {
        return;
    };
    discarded(&s);
}
