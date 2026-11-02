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

/// **The feed, its upload declared at `route` by its build alone**: `pw
/// check` refuses a program an upload under a path the platform answers
/// itself (PW7001, track `deploy`), so only a build made by other means
/// names one, and the host's admission is held for that build all the same.
/// The feed is built as it is, and its `uploads.json` then names `route`.
fn served_feed_uploading_at(route: &str) -> Served {
    let (dir, out) = built_feed_with(|app| app.to_string());
    let declared = out.join("uploads.json");
    let text = std::fs::read_to_string(&declared).expect("uploads.json");
    assert!(text.contains("\"/uploads/post-image\""), "{text}");
    std::fs::write(
        &declared,
        text.replace("\"/uploads/post-image\"", &format!("\"{route}\"")),
    )
    .expect("uploads.json written");
    Served {
        server: Server::from_build(out.clone(), out).expect("served"),
        _dir: dir,
    }
}

fn uploaded_at(route: &str) {
    let server = served_feed_uploading_at(route);
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
    uploaded_at("/command/attach");
}

#[test]
fn a_declared_upload_at_stream_path_is_admitted_before_dispatch() {
    uploaded_at("/stream");
}
