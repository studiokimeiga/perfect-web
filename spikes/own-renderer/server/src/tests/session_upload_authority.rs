//! Upload HTTP dispatch must retain the actor admitted before dispatch.
use super::*;
use std::io::Cursor;
use std::time::Duration;

const COOKIE: &str = "http-upload-authority";

fn exchange(server: &Server, path: &str) -> (String, Vec<u8>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("listener");
    let at = listener.local_addr().expect("address");
    std::thread::scope(|scope| {
        scope.spawn(|| {
            let (stream, _) = listener.accept().expect("accept");
            handle(server, stream);
        });
        let mut client = TcpStream::connect(at).expect("client");
        client
            .set_read_timeout(Some(Duration::from_secs(10)))
            .expect("timeout");
        write!(
            client,
            "GET {path} HTTP/1.1\r\nHost: t\r\nCookie: pw-session={COOKIE}\r\n\r\n"
        )
        .expect("request");
        let mut bytes = Vec::new();
        client.read_to_end(&mut bytes).expect("whole response");
        let boundary = bytes
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .expect("HTTP head");
        (
            String::from_utf8(bytes[..boundary].to_vec()).expect("HTTP metadata"),
            bytes[boundary + 4..].to_vec(),
        )
    })
}

fn attach(server: &Server, reader: &identity::ReaderIdentity, image: &[u8]) -> String {
    let principals = server.identity.principals();
    principals.with_reader(COOKIE, reader, || {
        let (ct, body) = crate::uploads::tests::form(image, "private.png", "image/png");
        let headers = crate::uploads::tests::headers_for(&ct, body.len());
        let leases = server.uploads.leases().expect("declared uploads");
        let answer = leases.answer(
            "POST",
            "/uploads/post-image",
            &headers,
            COOKIE,
            false,
            &mut Cursor::new(body),
        );
        assert_eq!(answer.status, 303, "valid upload fixture");
        leases.shown(COOKIE).expect("current attachment").src
    })
}

#[test]
fn upload_http_dispatch_keeps_admitted_reader_when_same_cookie_is_reopened() {
    let server = served_feed();
    let principals = server.identity.principals();
    assert!(
        principals.kept().is_none(),
        "this witness explicitly selects memory"
    );
    server.identity.signed_in_for_test(COOKIE, "alice", "Alice");
    let admitted = principals.bound_reader(COOKIE);
    let first = crate::uploads::tests::png(9, 7);
    let original = attach(&server, &admitted, &first);
    assert_eq!(original, "/uploads/post-image/up-1", "fresh lease counter");
    let (head, body) = exchange(&server, &original);
    assert!(
        head.starts_with("HTTP/1.1 200"),
        "current-reader HTTP preview fixture"
    );
    assert_eq!(body, first, "fixture's private image");

    let requested = "/uploads/post-image/up-2";
    *server.after_page_admission.lock().expect("admission hook") = Some(Box::new(move |server| {
        server.identity.signed_in_for_test(COOKIE, "alice", "Alice");
        let principals = server.identity.principals();
        // Bypass any old invocation binding to stage a genuinely new actor's
        // lease, then leave the suspended HTTP request with its original one.
        let replacement = principals.inspect_reader(COOKIE).expect("live replacement");
        assert_ne!(
            admitted, replacement,
            "fresh generation under the same actor and cookie"
        );
        assert_eq!(
            attach(server, &replacement, &crate::uploads::tests::png(4, 6)),
            requested,
            "replacement fixture uses predicted unique lease ID"
        );
    }));
    let (head, body) = exchange(&server, requested);
    assert!(
        head.starts_with("HTTP/1.1 404"),
        "HTTP upload dispatch must not recapture a replacement generation"
    );
    assert_ne!(
        body,
        crate::uploads::tests::png(4, 6),
        "stale admitted request gets no replacement image bytes"
    );

    let (head, body) = exchange(&server, requested);
    let replacement_bytes = crate::uploads::tests::png(4, 6);
    assert!(
        head.starts_with("HTTP/1.1 200"),
        "fresh admitted reader can preview its own image"
    );
    assert_eq!(
        body, replacement_bytes,
        "positive neighbor keeps the exact staged PNG"
    );
    assert_eq!(
        crate::blob::BlobKey::of(&body),
        crate::blob::BlobKey::of(&replacement_bytes)
    );
}
