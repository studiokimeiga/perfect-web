//! **A production host, asked as a browser and a platform ask it** (track
//! `deploy`): the store, as `pw build` builds it from `examples`, served by a
//! server holding a production host's own answers (`production::Production`,
//! which `main` sets for `PW_DEPLOYMENT=production`). Each rule has its
//! control: the same request to a development server.
//!
//! What lives in the process (the HSTS line, the bootstrap at the build's
//! version, the keys, the JSON log, the stop) is `tests/production_process.rs` (beside `src/`)'s,
//! which runs the binary.

use super::*;

/// The store, served by a development server.
fn store() -> Served {
    served_from_patches_in("examples", |app| app.to_string(), &[])
}

/// The store, served by a production host: its assets read from its build,
/// and `token` the one its metrics are read with.
fn production_store(token: Option<&str>) -> Served {
    let served = store();
    let mut pairs: Vec<(&'static str, &str)> = Vec::new();
    if let Some(token) = token {
        pairs.push(("PW_METRICS_TOKEN", token));
    }
    let secrets = production::secrets::Secrets::of(&pairs);
    let own = production::Production::of(&served, &secrets, production::Layer::Store)
        .expect("a production host's own");
    assert!(served.production.set(own).is_ok());
    served
}

/// **A request, written whole, and its answer**: its head as text and its
/// body as bytes, which a compressed body is.
fn asked(s: &Server, request: &str) -> (String, Vec<u8>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind");
    let at = listener.local_addr().expect("address");
    std::thread::scope(|scope| {
        scope.spawn(|| {
            let (stream, _) = listener.accept().expect("accept");
            handle(s, stream);
        });
        let mut client = TcpStream::connect(at).expect("connect");
        client.write_all(request.as_bytes()).expect("request");
        let mut answer = Vec::new();
        let _ = client.read_to_end(&mut answer);
        let split = answer
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .expect("a head");
        (
            String::from_utf8(answer[..split].to_vec()).expect("a head in text"),
            answer[split + 4..].to_vec(),
        )
    })
}

/// A `GET` of `path`, with `headers` (each line ending in CRLF).
fn got(s: &Server, path: &str, headers: &str) -> (String, Vec<u8>) {
    asked(
        s,
        &format!("GET {path} HTTP/1.1\r\nHost: t\r\n{headers}\r\n"),
    )
}

/// A `POST` of `path`, with no body.
fn posted_empty(s: &Server, path: &str) -> (String, Vec<u8>) {
    asked(
        s,
        &format!("POST {path} HTTP/1.1\r\nHost: t\r\nContent-Length: 0\r\n\r\n"),
    )
}

fn status(head: &str) -> u16 {
    head.split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .expect("a status")
}

fn field<'h>(head: &'h str, name: &str) -> Option<&'h str> {
    identity::header(head, name)
}

/// **A production host answers no test control**, as a route it does not
/// have, whatever the method; a development server answers each.
#[test]
fn a_production_host_answers_no_test_control() {
    let production = production_store(None);
    for (method, path) in [
        ("GET", "/bench/clock"),
        ("POST", "/bench/clock?advance=60000"),
        ("POST", "/bench/order?status=preparing"),
        ("POST", "/bench/fail"),
        ("POST", "/command/menu_changed"),
        ("POST", "/command/menu?op=remove&id=1"),
        ("POST", "/command/menu_ghost"),
        ("GET", "/menu-entry"),
        ("GET", "/StorePage.html?items=40"),
    ] {
        let (head, _) = match method {
            "GET" => got(&production, path, ""),
            _ => posted_empty(&production, path),
        };
        assert_eq!(status(&head), 404, "{method} {path}: {head}");
        assert!(
            field(&head, "set-cookie").is_none(),
            "{method} {path}: {head}"
        );
    }
    // The menu is the build's still: no control rewrote it.
    let (head, page) = got(&production, "/StorePage.html", "");
    assert_eq!(status(&head), 200, "{head}");
    assert!(String::from_utf8_lossy(&page).contains("Blue Bottle"));

    let development = store();
    let (head, _) = got(&development, "/bench/clock", "");
    assert_eq!(status(&head), 200, "{head}");
    let (head, _) = posted_empty(&development, "/command/menu_changed");
    assert_eq!(status(&head), 202, "{head}");
}

/// **A production host says it lives, and whether it is ready**, to anyone,
/// with no session and nothing a cache keeps; a development server has no
/// such routes.
#[test]
fn a_production_host_says_it_lives_and_is_ready() {
    let production = production_store(None);
    for (path, said) in [(production::HEALTH, "live"), (production::READY, "ready")] {
        let (head, body) = got(&production, path, "");
        assert_eq!(status(&head), 200, "{path}: {head}");
        let body: serde_json::Value = serde_json::from_slice(&body).expect("JSON");
        assert_eq!(body["status"], said, "{path}");
        assert_eq!(field(&head, "cache-control"), Some("no-store"), "{path}");
        assert!(field(&head, "set-cookie").is_none(), "{path}: {head}");
        assert_eq!(field(&head, "x-content-type-options"), Some("nosniff"));
    }
    let development = store();
    let (head, _) = got(&development, production::HEALTH, "");
    assert_eq!(status(&head), 404, "{head}");
}

/// **A production host serves its build's assets from memory**: each
/// handler's module, compressed where the request accepts it and
/// validated by its digest; kept for good at the build's version, asked
/// again at any other; and none of it names a session.
#[test]
fn a_production_host_serves_its_builds_assets() {
    let production = production_store(None);
    let own = production.production.get().expect("its own");
    let version = own.assets.version().to_string();
    let identity = production
        .handler_identities()
        .into_iter()
        .next()
        .expect("a handler");
    let path = format!("/handler/{identity}.mjs");
    assert!(own.assets.routes().any(|r| r == path), "{path}");
    let module = std::fs::read(production.handler_module(&identity)).expect("its module");

    let (head, body) = got(&production, &path, "");
    assert_eq!(status(&head), 200, "{head}");
    assert_eq!(body, module);
    assert_eq!(
        field(&head, "content-type"),
        Some("text/javascript; charset=utf-8")
    );
    assert_eq!(
        field(&head, "cache-control"),
        Some(production::assets::ASKED)
    );
    assert!(field(&head, "set-cookie").is_none(), "{head}");
    let tag = field(&head, "etag").expect("an etag").to_string();

    let (head, body) = got(&production, &format!("{path}?v={version}"), "");
    assert_eq!(
        field(&head, "cache-control"),
        Some(production::assets::KEPT)
    );
    assert_eq!(body, module);

    let (head, body) = got(&production, &path, &format!("If-None-Match: {tag}\r\n"));
    assert_eq!(status(&head), 304, "{head}");
    assert!(body.is_empty());

    let (head, body) = got(&production, &path, "Accept-Encoding: gzip, br\r\n");
    if module.len() > 200 {
        assert_eq!(field(&head, "content-encoding"), Some("gzip"), "{head}");
        let held =
            miniz_oxide::inflate::decompress_to_vec(&body[10..body.len() - 8]).expect("DEFLATE");
        assert_eq!(held, module);
    }
    // A handler this build does not know is refused as before.
    let (head, _) = got(&production, "/handler/0000000000000000.mjs", "");
    assert_eq!(status(&head), 404, "{head}");
}

/// **A production host serves no other file of its build**: not its
/// metadata, not a page as a file; a development server serves its
/// build's files as they are.
#[test]
fn a_production_host_serves_no_other_file_of_its_build() {
    let production = production_store(None);
    for path in [
        "/templates.json",
        "/contracts.json",
        "/build-id",
        "/graph.json",
    ] {
        let (head, _) = got(&production, path, "");
        assert_eq!(status(&head), 404, "{path}: {head}");
    }
    let development = store();
    let (head, _) = got(&development, "/templates.json", "");
    assert_eq!(status(&head), 200, "{head}");
}

/// **A production host's metrics are read only with its token**; without
/// one configured, by no one. A development server's are anyone's.
#[test]
fn a_production_hosts_metrics_are_read_only_with_its_token() {
    let closed = production_store(None);
    let (head, _) = got(&closed, "/metrics", "Authorization: Bearer anything\r\n");
    assert_eq!(status(&head), 404, "{head}");

    let held = production_store(Some("m3tr1cs-t0k3n"));
    let (head, _) = got(&held, "/metrics", "");
    assert_eq!(status(&head), 404, "{head}");
    let (head, _) = got(&held, "/metrics", "Authorization: Bearer m3tr1cs-t0k3\r\n");
    assert_eq!(status(&head), 404, "{head}");
    let (head, body) = got(&held, "/metrics", "Authorization: Bearer m3tr1cs-t0k3n\r\n");
    assert_eq!(status(&head), 200, "{head}");
    let body: serde_json::Value = serde_json::from_slice(&body).expect("JSON");
    assert!(body["calls"].is_object(), "{body}");

    let development = store();
    let (head, _) = got(&development, "/metrics", "");
    assert_eq!(status(&head), 200, "{head}");
}

/// **A production host's page is the program's**: served as a development
/// server serves it, with its policy, `nosniff` and a fresh session.
#[test]
fn a_production_hosts_page_is_the_programs() {
    let production = production_store(None);
    let (head, page) = got(&production, "/stores/47", "");
    assert_eq!(status(&head), 200, "{head}");
    assert!(field(&head, "content-security-policy").is_some(), "{head}");
    assert_eq!(field(&head, "x-content-type-options"), Some("nosniff"));
    assert!(field(&head, "set-cookie").is_some_and(|c| c.starts_with("pw-session=")));
    assert!(String::from_utf8_lossy(&page).contains("Blue Bottle"));
}

/// **A request's head is bounded**: one past 64 KiB, in one field or in
/// many, is refused with 431 before anything is read of it; one within is
/// answered.
#[test]
fn a_requests_head_is_bounded() {
    let s = store();
    let long = "a".repeat(70 * 1024);
    let (head, _) = got(&s, "/stores/47", &format!("X-Long: {long}\r\n"));
    assert_eq!(status(&head), 431, "{head}");
    let many: String = (0..80)
        .map(|i| format!("X-{i}: {}\r\n", "b".repeat(1000)))
        .collect();
    let (head, _) = got(&s, "/stores/47", &many);
    assert_eq!(status(&head), 431, "{head}");
    let (head, _) = asked(
        &s,
        &format!("GET /{} HTTP/1.1\r\nHost: t\r\n\r\n", "c".repeat(70 * 1024)),
    );
    assert_eq!(status(&head), 431, "{head}");

    let within = "d".repeat(60 * 1024);
    let (head, _) = got(&s, "/stores/47", &format!("X-Long: {within}\r\n"));
    assert_eq!(status(&head), 200, "{head}");
}

/// **With no sign-in, every session is anonymous**: a sign-in, a sign-up
/// and a callback are answered that there is none here, `SignedIn` holds
/// for no one, and the store's page is a guest's; and it runs in
/// production, where the development models do not.
#[test]
fn with_no_sign_in_every_session_is_anonymous() {
    let production = identity::Deployment {
        development: false,
        origin: "https://preview.example".to_string(),
    };
    let s = production_store(None);
    for model in ["guest", identity::DEV_ACCOUNTS] {
        let refused = s
            .identity
            .configure(&production, Some(model))
            .expect_err(model);
        assert!(refused.contains("development"), "{model}: {refused}");
    }
    s.identity
        .configure(&production, Some(identity::NO_SIGN_IN))
        .expect("no sign-in runs in production");
    assert!(
        s.identity.describe().starts_with("none"),
        "{}",
        s.identity.describe()
    );
    for path in ["/sign-in", "/sign-up", identity::CALLBACK] {
        let (head, body) = got(&s, path, "");
        assert_eq!(status(&head), 404, "{path}: {head}");
        assert!(
            String::from_utf8_lossy(&body).contains("no sign-in"),
            "{path}"
        );
    }
    let (head, page) = got(&s, "/stores/47", "");
    assert_eq!(status(&head), 200, "{head}");
    assert!(String::from_utf8_lossy(&page).contains("Blue Bottle"));
    assert_eq!(s.identity.answers("SignedIn", "s-anyone"), Ok(false));

    // The guest model, its control: every session signed in.
    let guests = store();
    assert_eq!(guests.identity.answers("SignedIn", "s-anyone"), Ok(true));
}
