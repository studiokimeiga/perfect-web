//! A declared upload owns its route even under a command/stream adapter path.
use super::*;
use std::time::Duration;

const COOKIE: &str = "reserved-path-upload-reader";

fn exchange(server: &Server, request: &[u8]) -> Vec<u8> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("listener");
    let at = listener.local_addr().expect("address");
    std::thread::scope(|scope| {
        let worker = scope.spawn(|| {
            let (stream, _) = listener.accept().expect("accept");
            handle(server, stream);
        });
        let mut client = TcpStream::connect(at).expect("client");
        client
            .set_read_timeout(Some(Duration::from_secs(10)))
            .expect("bounded response");
        client.write_all(request).expect("request");
        let mut reply = Vec::new();
        let received = client.read_to_end(&mut reply);
        let finished = worker.join();
        assert!(
            finished.is_ok(),
            "declared upload routes must receive admission before dispatch"
        );
        received.expect("whole response");
        reply
    })
}

fn answer(bytes: &[u8]) -> (&str, &[u8]) {
    let at = bytes
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("HTTP response head");
    (
        std::str::from_utf8(&bytes[..at]).expect("HTTP metadata"),
        &bytes[at + 4..],
    )
}

fn uploaded_at(change: fn(&str) -> String, route: &str) {
    let server = served_feed_with(change);
    assert!(
        server.identity.principals().kept().is_none(),
        "this fixture selects memory explicitly"
    );
    server.identity.signed_in_for_test(COOKIE, "alice", "Alice");
    let image = crate::uploads::tests::png(13, 5);
    let (kind, body) = crate::uploads::tests::form(&image, "route.png", "image/png");
    let mut request = format!(
        "POST {route} HTTP/1.1\r\nCookie: pw-session={COOKIE}\r\n{}\r\n",
        crate::uploads::tests::headers_for(&kind, body.len())
    )
    .into_bytes();
    request.extend(body);
    let reply = exchange(&server, &request);
    let (head, _) = answer(&reply);
    assert!(
        head.starts_with("HTTP/1.1 303"),
        "a valid declared upload must succeed under the reserved adapter prefix"
    );
    let reader = server.identity.principals().bound_reader(COOKIE);
    let shown = server
        .identity
        .principals()
        .with_reader(COOKIE, &reader, || {
            server
                .uploads
                .leases()
                .expect("declared upload")
                .shown(COOKIE)
                .expect("current reader's attachment")
        });
    assert!(shown.src.starts_with(&format!("{route}/")));
    let reply = exchange(
        &server,
        format!(
            "GET {} HTTP/1.1\r\nHost: 127.0.0.1:3143\r\nCookie: pw-session={COOKIE}\r\n\r\n",
            shown.src
        )
        .as_bytes(),
    );
    let (head, bytes) = answer(&reply);
    assert!(
        head.starts_with("HTTP/1.1 200"),
        "the admitted upload owner can preview the resulting lease"
    );
    assert_eq!(bytes, image, "the lease retains the exact private image");
}

#[test]
fn a_declared_upload_under_command_prefix_is_admitted_before_dispatch() {
    uploaded_at(
        |app| app.replace("/uploads/post-image", "/command/attach"),
        "/command/attach",
    );
}

#[test]
fn a_declared_upload_at_stream_path_is_admitted_before_dispatch() {
    uploaded_at(
        |app| app.replace("/uploads/post-image", "/stream"),
        "/stream",
    );
}
