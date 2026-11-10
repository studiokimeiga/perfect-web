//! **A page no reader decides is one document** (ADR-0409), the
//! host's part: what it sends for a page the build marked, over a real
//! connection, and that everything else is as it was. Which pages are marked
//! is the compiler's (`compiler/pw-core/tests/prerender.rs`).

use super::device_store::served;
use super::*;

/// A page that is marked, `Home`; one a query fills, `Bound`; and one whose
/// press sends a command, `Sends`.
const APP: &str = "module t\n\n\
opaque type InteractionId = String\n\n\
public query Greeting() -> String !{} {\n    \"hello\"\n}\n\n\
command send(text: String) -> Result<Int, String>\n    requires Anyone\n    \
idempotent_by InteractionId\n{\n    Ok(1)\n}\n\n\
page Home() {\n    route \"/\"\n\n    signal count: Int = 0\n    \
signal rows: List<Int> = [1, 2]\n\n    view {\n        <title>Home</title>\n        \
<button type=\"button\" on:press={() => count = count + 1}>{count}</button>\n        \
{#if count >= 0}{#each rows as n (n)}<p>{n}</p>{/each}{/if}\n    }\n}\n\n\
page Bound() {\n    route \"/bound\"\n\n    let greeting = query Greeting()\n\n    \
view {\n        <title>Bound</title>\n        <p>{greeting}</p>\n    }\n}\n\n\
page Sends() {\n    route \"/sends\"\n\n    view {\n        <title>Sends</title>\n        \
<button type=\"button\" on:press={() => match send(\"a\") { Ok(_) => (), Err(_) => () }}>\
Send</button>\n    }\n}\n";

const HOME: &str = "t.Home~ui";

/// Each marked page rendered, as a host does before it listens.
fn rendered(s: &Served) {
    let done = s.prerender();
    assert!(done.is_ok(), "{done:?}");
}

/// [`APP`], served, its marked page rendered.
fn home() -> Served {
    let s = served(APP);
    rendered(&s);
    s
}

/// `request` sent over a connection of its own: the answer's head and body.
fn asked(s: &Server, request: &str) -> (String, Vec<u8>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind");
    let at = listener.local_addr().expect("address");
    std::thread::scope(|scope| {
        scope.spawn(|| {
            let (stream, _) = listener.accept().expect("accept");
            handle(s, stream);
        });
        let mut client = TcpStream::connect(at).expect("connect");
        client.write_all(request.as_bytes()).expect("the request");
        let mut answer = Vec::new();
        let _ = client.read_to_end(&mut answer);
        let split = answer
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .expect("a head");
        (
            String::from_utf8_lossy(&answer[..split + 2]).to_string(),
            answer[split + 4..].to_vec(),
        )
    })
}

/// A `GET` of `path`, with `headers` and no cookie unless they name one.
fn get(s: &Server, path: &str, headers: &str) -> (String, Vec<u8>) {
    asked(
        s,
        &format!("GET {path} HTTP/1.1\r\nHost: t\r\nConnection: close\r\n{headers}\r\n"),
    )
}

/// A header's value in `head`, by its name as the host writes it.
fn header<'h>(head: &'h str, name: &str) -> Option<&'h str> {
    head.lines()
        .find_map(|line| line.strip_prefix(name)?.strip_prefix(": "))
}

fn status(head: &str) -> &str {
    head.split_whitespace().nth(1).unwrap_or_default()
}

/// **The one way a reader could be told apart**: a cookie, a store a reader
/// fills, or a cache told to keep it from others.
#[test]
fn every_reader_is_sent_the_same_document_and_named_nothing() {
    let s = home();
    s.identity.signed_in_for_test("reader", "ada", "Ada");
    let (first, fresh) = get(&s, "/", "");
    let (second, returning) = get(&s, "/", "Cookie: pw-session=reader\r\n");
    assert_eq!(status(&first), "200", "{first}");
    assert_eq!(status(&second), "200", "{second}");
    assert_eq!(fresh, returning);
    let page = String::from_utf8_lossy(&fresh).to_string();
    assert!(page.contains("<title>Home</title>"), "{page}");
    for head in [&first, &second] {
        assert_eq!(header(head, "set-cookie"), None, "{head}");
        assert_eq!(
            header(head, "cache-control"),
            Some("public, no-cache"),
            "{head}"
        );
        assert_eq!(header(head, "vary"), Some("accept-encoding"), "{head}");
    }
    assert_eq!(header(&first, "etag"), header(&second, "etag"));
    assert!(header(&first, "etag").is_some(), "{first}");
}

/// A document's row markers with each row's token taken out:
/// `<!--pw:s3@token-->` as `<!--pw:s3-->`.
fn without_row_tokens(document: &str) -> String {
    let mut out = String::new();
    let mut rest = document;
    while let Some(at) = rest.find("<!--pw:") {
        let (before, comment) = rest.split_at(at);
        out.push_str(before);
        let end = comment.find("-->").expect("a marker's end") + "-->".len();
        match comment[..end].split_once('@') {
            Some((marker, _)) => {
                out.push_str(marker);
                out.push_str("-->");
            }
            None => out.push_str(&comment[..end]),
        }
        rest = &comment[end..];
    }
    out.push_str(rest);
    out
}

/// **A page rendered once is the page**: what the same page rendered for a
/// request would have been, on the address that still does. But for each
/// row's token, which is keyed by whose the document is: every reader's
/// here, one session's there.
#[test]
fn it_is_the_document_a_request_would_have_been_rendered() {
    let s = home();
    let (_, once) = get(&s, "/", "");
    let (rendered, again) = get(&s, &format!("/page/{HOME}"), "");
    assert_eq!(status(&rendered), "200", "{rendered}");
    let (once, again) = (
        String::from_utf8_lossy(&once).to_string(),
        String::from_utf8_lossy(&again).to_string(),
    );
    assert_ne!(once, again, "each row's token is its partition's");
    assert_eq!(without_row_tokens(&once), without_row_tokens(&again));
}

/// A page with no row has no token, and is the same bytes.
#[test]
fn a_page_with_no_rows_is_the_same_bytes_as_a_request_would_have_been() {
    let app = APP.replace("rows: List<Int> = [1, 2]", "rows: List<Int> = []");
    let s = served(&app);
    rendered(&s);
    let (_, once) = get(&s, "/", "");
    let (_, again) = get(&s, &format!("/page/{HOME}"), "");
    assert_eq!(
        String::from_utf8_lossy(&once),
        String::from_utf8_lossy(&again)
    );
}

/// The address that still renders is a session's, as it always was: what
/// tells the two answers apart is the answer, not the page.
#[test]
fn the_address_that_still_renders_is_as_it_was() {
    let s = home();
    let (head, _) = get(&s, &format!("/page/{HOME}"), "");
    assert_eq!(
        header(&head, "cache-control"),
        Some("private, no-store"),
        "{head}"
    );
    assert!(header(&head, "set-cookie").is_some(), "{head}");
}

/// Each representation's bytes, decoded, are the document, and the request
/// chooses among them: Brotli first, then gzip, then neither.
#[test]
fn each_representation_holds_the_document() {
    let s = home();
    let (plain_head, plain) = get(&s, "/", "");
    assert_eq!(
        header(&plain_head, "content-encoding"),
        None,
        "{plain_head}"
    );

    let (head, br) = get(&s, "/", "Accept-Encoding: gzip, br\r\n");
    assert_eq!(header(&head, "content-encoding"), Some("br"), "{head}");
    let mut decoded = Vec::new();
    let unpacked = brotli::BrotliDecompress(&mut &br[..], &mut decoded);
    assert!(unpacked.is_ok(), "{unpacked:?}");
    assert_eq!(decoded, plain);
    assert!(br.len() < plain.len(), "{} of {}", br.len(), plain.len());

    let (head, gzip) = get(&s, "/", "Accept-Encoding: gzip, br;q=0\r\n");
    assert_eq!(header(&head, "content-encoding"), Some("gzip"), "{head}");
    // RFC 1952: the two bytes that say gzip, and method 8, DEFLATE.
    assert_eq!(gzip[..3], [0x1f, 0x8b, 8]);
    let body = &gzip[10..gzip.len() - 8];
    let inflated = miniz_oxide::inflate::decompress_to_vec(body);
    assert_eq!(inflated.as_deref().unwrap_or_default(), plain);
    let trailer = &gzip[gzip.len() - 8..];
    assert_eq!(trailer[..4], crate::prerender::crc32(&plain).to_le_bytes());
    assert_eq!(trailer[4..], (plain.len() as u32).to_le_bytes());

    let (head, _) = get(&s, "/", "Accept-Encoding: br;q=0, gzip;q=0\r\n");
    assert_eq!(header(&head, "content-encoding"), None, "{head}");
}

/// CRC-32 as RFC 1952 states it, on the check value every reference gives
/// for these nine bytes.
#[test]
fn a_gzip_trailers_checksum_is_crc_32() {
    assert_eq!(crate::prerender::crc32(b"123456789"), 0xCBF4_3926);
    assert_eq!(crate::prerender::crc32(b""), 0);
}

/// One validator for each representation, and a 304 only for the one held.
#[test]
fn a_browser_that_holds_a_representation_is_told_so() {
    let s = home();
    let tag_of = |accepts: &str| {
        let (head, _) = get(&s, "/", accepts);
        assert!(header(&head, "etag").is_some(), "a validator: {head}");
        header(&head, "etag").unwrap_or_default().to_string()
    };
    let (plain, br, gzip) = (
        tag_of(""),
        tag_of("Accept-Encoding: br\r\n"),
        tag_of("Accept-Encoding: gzip\r\n"),
    );
    assert_ne!(plain, br);
    assert_ne!(plain, gzip);
    assert_ne!(br, gzip);

    let (head, body) = get(
        &s,
        "/",
        &format!("Accept-Encoding: br\r\nIf-None-Match: {br}\r\n"),
    );
    assert_eq!(status(&head), "304", "{head}");
    assert!(body.is_empty(), "{body:?}");
    assert_eq!(header(&head, "etag"), Some(br.as_str()), "{head}");
    assert_eq!(
        header(&head, "cache-control"),
        Some("public, no-cache"),
        "{head}"
    );
    assert_eq!(header(&head, "vary"), Some("accept-encoding"), "{head}");
    // RFC 9110 §8.6: never a length other than the 200's.
    assert_eq!(header(&head, "content-length"), None, "{head}");
    assert_eq!(header(&head, "set-cookie"), None, "{head}");

    // One tag among several.
    let (head, _) = get(
        &s,
        "/",
        &format!("Accept-Encoding: br\r\nIf-None-Match: \"other\", {br}\r\n"),
    );
    assert_eq!(status(&head), "304", "{head}");

    // RFC 9110 §13.1.2: a recipient compares weakly, so a tag an
    // intermediary weakened still holds it; and `*` holds any.
    for held in [format!("W/{br}"), "*".to_string()] {
        let (head, body) = get(
            &s,
            "/",
            &format!("Accept-Encoding: br\r\nIf-None-Match: {held}\r\n"),
        );
        assert_eq!(status(&head), "304", "{held}: {head}");
        assert!(body.is_empty(), "{held}: {body:?}");
    }

    // Another representation's tag, and a stranger's.
    for held in [plain.clone(), "\"other\"".to_string()] {
        let (head, body) = get(
            &s,
            "/",
            &format!("Accept-Encoding: br\r\nIf-None-Match: {held}\r\n"),
        );
        assert_eq!(status(&head), "200", "{held}: {head}");
        assert!(!body.is_empty(), "{held}");
    }
}

/// A validator names what a document says of itself too: a deployment that
/// changes its policy, the page's bytes the same, is a document a browser
/// must be sent again, not told it holds (the independent review's third).
#[test]
fn what_it_says_of_itself_is_part_of_its_validator() {
    let tag = |policy: &str| {
        let document = crate::prerender::Prerendered::of(
            "<p>the same</p>".to_string(),
            format!("content-security-policy: {policy}\r\n"),
        );
        assert!(document.is_ok(), "it compresses");
        document.unwrap().accepted(|_| false).tag.clone()
    };
    assert_ne!(tag("default-src 'none'"), tag("default-src 'self'"));
}

/// Its rows' tokens are the public partition's: it is every reader's.
#[test]
fn it_is_rendered_in_the_public_partition() {
    let s = home();
    let (_, sent) = get(&s, "/", "");
    let public = s.render_page_in(HOME, &BTreeMap::new(), Partition::Public, &[]);
    assert!(public.is_ok(), "it renders");
    assert_eq!(
        String::from_utf8_lossy(&sent),
        public.map(|(document, _, _)| document).unwrap_or_default()
    );
}

/// What every document says of itself, said once for all of them.
#[test]
fn it_says_what_a_document_says_of_itself() {
    let s = home();
    let (head, _) = get(&s, "/", "");
    let (rendered, _) = get(&s, &format!("/page/{HOME}"), "");
    assert!(header(&head, "content-security-policy").is_some(), "{head}");
    let policy = header(&head, "content-security-policy").unwrap_or_default();
    assert_eq!(
        Some(policy),
        header(&rendered, "content-security-policy"),
        "{head}\n{rendered}"
    );
    assert!(policy.contains("script-src"), "{policy}");
    assert_eq!(
        header(&head, "pw-build"),
        Some(s.build_id.as_str()),
        "{head}"
    );
    assert_eq!(header(&head, "x-content-type-options"), Some("nosniff"));
    assert_eq!(
        header(&head, "content-type"),
        Some("text/html; charset=utf-8"),
        "{head}"
    );
    // A build with no delivery says none.
    assert_eq!(header(&head, "pw-delivery"), None, "{head}");
}

/// A production build's files are named at one version: its document says
/// which, and names them so.
#[test]
fn a_versioned_build_says_its_delivery_and_names_its_files_at_it() {
    let mut s = served(APP);
    std::fs::write(s.dist.join("delivery.json"), "{\"versioned\": true}").expect("written");
    let (dist, artifacts, build) = (s.dist.clone(), s.artifacts.clone(), s.build_id.clone());
    s.server.delivery = Delivery::of(&dist, &artifacts, &build);
    rendered(&s);
    let version = s.delivery.version.clone().expect("versioned");
    let (head, body) = get(&s, "/", "");
    assert_eq!(
        header(&head, "pw-delivery"),
        Some(version.as_str()),
        "{head}"
    );
    let page = String::from_utf8_lossy(&body).to_string();
    assert!(
        page.contains(&format!("/pw-runtime.mjs?b={version}")),
        "{page}"
    );
}

/// RFC 9110 §9.3.2: its `GET`'s head, and no body.
#[test]
fn a_head_is_answered_as_its_get_is() {
    let s = home();
    let (got, body) = get(&s, "/", "");
    let (head, none) = asked(&s, "HEAD / HTTP/1.1\r\nHost: t\r\n\r\n");
    assert_eq!(status(&head), "200", "{head}");
    assert!(none.is_empty(), "{none:?}");
    assert_eq!(
        header(&head, "content-length"),
        Some(body.len().to_string().as_str()),
        "{head}"
    );
    assert_eq!(header(&head, "etag"), header(&got, "etag"), "{head}");
}

/// A page a query fills, and one whose press reaches the server, are
/// answered as ever: a session's, kept by no cache.
#[test]
fn a_page_someone_decides_is_as_it_was() {
    let s = home();
    for path in ["/bound", "/sends"] {
        let (head, _) = get(&s, path, "");
        assert_eq!(status(&head), "200", "{path}: {head}");
        assert_eq!(
            header(&head, "cache-control"),
            Some("private, no-store"),
            "{path}: {head}"
        );
        assert!(header(&head, "set-cookie").is_some(), "{path}: {head}");
        assert_eq!(header(&head, "etag"), None, "{path}: {head}");
    }
}

/// A host's own route answers before a page's, as it always has: rendering
/// once changes no precedence.
#[test]
fn a_host_route_keeps_its_place() {
    let app = APP.replace("route \"/\"", "route \"/metrics\"");
    let s = served(&app);
    rendered(&s);
    let (head, body) = get(&s, "/metrics", "");
    assert_eq!(status(&head), "200", "{head}");
    let page = String::from_utf8_lossy(&body).to_string();
    assert!(!page.contains("<title>Home</title>"), "{page}");
    assert!(page.contains("\"calls\""), "{page}");
}

/// A request that brings a cookie is admitted as every request is
/// (ADR-0389), and its failed read refuses it: the runtime and modules it
/// would load next are refused in the same outage.
#[test]
fn a_request_with_a_session_is_admitted_as_every_request_is() {
    let s = home();
    // Sessions a database keeps are handed over once, as the host is built, so
    // the outage below is one only where they are kept in memory.
    if s.identity.principals().kept().is_some() {
        return;
    }
    s.identity.signed_in_for_test("reader", "ada", "Ada");
    s.identity
        .principals()
        .keep_in(Arc::new(super::session_overlap::Unavailable));
    let (head, _) = get(&s, "/", "Cookie: pw-session=reader\r\n");
    assert_eq!(status(&head), "503", "{head}");
    // And one that brings none reads no session: it is answered.
    let (head, _) = get(&s, "/", "");
    assert_eq!(status(&head), "200", "{head}");
}

/// A build that cannot show a page did not finish: said before the host
/// listens, as a handler that was not compiled is, and never to a visitor.
#[test]
fn a_page_that_does_not_render_stops_the_host() {
    let mut s = served(APP);
    let plan = s.server.plans.get_mut(HOME).expect("Home's plan");
    plan["live"] = serde_json::json!([{ "derived": "t.none", "part": 999_999 }]);
    let done = s.prerender();
    assert!(done.is_err(), "{done:?}");
    assert!(done.unwrap_err().contains(HOME));
}

/// A host that has not rendered them, as a test's that does not ask, renders
/// each request as it always has.
#[test]
fn nothing_is_one_document_until_it_is_rendered() {
    let s = served(APP);
    let (head, _) = get(&s, "/", "");
    assert_eq!(
        header(&head, "cache-control"),
        Some("private, no-store"),
        "{head}"
    );
}

/// **TodoMVC is one document** (ADR-0409): nothing on it is
/// anyone's and nothing on it asks a server anything, so the build marks it.
#[test]
fn todomvc_is_one_document() {
    let s = super::todomvc::served_todomvc();
    let done = s.prerender();
    assert!(done.is_ok(), "{done:?}");
    let answer: String = fetched_as(&s, "/", None)
        .into_iter()
        .map(|(_, c)| c)
        .collect();
    assert!(
        answer.contains("cache-control: public, no-cache\r\n"),
        "{answer}"
    );
    assert!(!answer.contains("set-cookie"), "{answer}");
}
