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
/// body as bytes, which a compressed body is. An answer with no head is
/// `(no answer)`, whose status is none, for a test's assertion to refuse.
fn asked(s: &Server, request: &str) -> (String, Vec<u8>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind");
    let at = listener.local_addr().expect("address");
    std::thread::scope(|scope| {
        scope.spawn(|| {
            let (stream, _) = listener.accept().expect("accept");
            handle(s, stream);
        });
        let mut client = TcpStream::connect(at).expect("connect");
        let _ = client.write_all(request.as_bytes());
        let mut answer = Vec::new();
        let _ = client.read_to_end(&mut answer);
        match answer.windows(4).position(|w| w == b"\r\n\r\n") {
            Some(split) => (
                String::from_utf8_lossy(&answer[..split]).into_owned(),
                answer[split + 4..].to_vec(),
            ),
            None => ("(no answer)".to_string(), answer),
        }
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

/// The status an answer's head says, or 0 where it says none.
fn status(head: &str) -> u16 {
    head.split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
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
    let (head, page) = got(&production, "/stores/47", "");
    assert_eq!(status(&head), 200, "{head}");
    assert!(String::from_utf8_lossy(&page).contains("Blue Bottle"));

    let development = store();
    let (head, _) = got(&development, "/bench/clock", "");
    assert_eq!(status(&head), 200, "{head}");
    let (head, _) = posted_empty(&development, "/command/menu_changed");
    assert_eq!(status(&head), 202, "{head}");
}

/// **A production host serves the program's routes and the platform's own,
/// and nothing else** (the review's finding 5b: `GET /menu`, a development
/// server's, answered in production): a development server's other routes
/// are refused as routes it does not have, with no session; the program's
/// pages, its commands and what a page's runtime asks are served. Each
/// refusal's control is the development server answering it.
#[test]
fn a_production_host_serves_only_the_programs_routes_and_its_own() {
    let production = production_store(None);
    let development = store();
    for path in [
        "/menu",
        "/StorePage.html",
        "/page/store.page.CartPage~ui",
        "/templates.json",
    ] {
        let (head, _) = got(&production, path, "");
        assert_eq!(status(&head), 404, "{path}: {head}");
        assert!(field(&head, "set-cookie").is_none(), "{path}: {head}");
        let (head, _) = got(&development, path, "");
        assert_eq!(status(&head), 200, "development {path}: {head}");
    }
    // The program's pages at their routes, and the platform's own.
    for path in [
        "/stores/47",
        "/cart",
        "/pw-handlers",
        "/stream?doc=1&since=0",
        "/_pw/health",
    ] {
        let (head, _) = got(&production, path, "");
        assert_eq!(status(&head), 200, "{path}: {head}");
    }
    // A command the program declares is the program's to answer; one it
    // does not is no route at all.
    let (head, _) = posted_empty(&production, "/command/store.page.add_to_cart");
    assert_ne!(status(&head), 404, "{head}");
    let (head, _) = posted_empty(&production, "/command/store.page.no_such_command");
    assert_eq!(status(&head), 404, "{head}");
    assert!(field(&head, "set-cookie").is_none(), "{head}");
}

/// **What a production host serves of a program with uploads, by the
/// pattern its log says** (the review's findings 5b and 5e): an upload's
/// route, a lease under it and a committed image, each by its pattern and
/// never the lease or key a reader holds; a page by its route's; and none
/// of a development server's own, or a command the program does not
/// declare.
#[test]
fn a_production_host_serves_an_uploads_paths_by_their_pattern() {
    let feed = served_feed_with(|app| app.to_string());
    let assets = production::assets::Assets::of(vec![]);
    let served = |method: &str, route: &str| production::served_as(&feed, &assets, method, route);
    assert_eq!(
        served("POST", "/uploads/post-image").as_deref(),
        Some("/uploads/post-image")
    );
    assert_eq!(
        served("GET", "/uploads/post-image/0f3a9c").as_deref(),
        Some("/uploads/post-image/{lease}")
    );
    assert_eq!(
        served("GET", "/images/9b1e2d.png").as_deref(),
        Some("/images/{key}")
    );
    assert_eq!(served("GET", "/post/42").as_deref(), Some("/post/{id}"));
    assert_eq!(served("GET", "/stream").as_deref(), Some("/stream"));
    for (method, route) in [
        ("GET", "/bench/clock"),
        ("GET", "/menu"),
        ("GET", "/page/feed.page.HomePage~ui"),
        ("POST", "/command/feed.page.no_such_command"),
        ("GET", "/images"),
    ] {
        assert_eq!(served(method, route), None, "{method} {route}");
    }
}

/// **The paths the host keeps are the ones `pw check` refuses a program**
/// (PW7001), and every route the development server answers itself is
/// under one: a route answered before the program's that the compiler did
/// not refuse would be a page its program could never be served at. A
/// route that yields to the program's page (its arm guarded by
/// `routed.is_none()`) is no such route.
#[test]
fn the_paths_the_host_keeps_are_the_ones_the_compiler_refuses() {
    assert_eq!(production::RESERVED, pw_core::reserved::RESERVED);
    let source = include_str!("../main.rs");
    let own = source
        .split("\n#[cfg(test)]\nmod tests {")
        .next()
        .unwrap_or(source);
    let mut answered = Vec::new();
    for pattern in [
        "(\"GET\", \"",
        "(\"POST\", \"",
        "route == \"",
        "route.starts_with(\"",
        "strip_prefix(\"/",
    ] {
        for (at, _) in own.match_indices(pattern) {
            let rest = &own[at + pattern.len()..];
            let arm = rest.lines().next().unwrap_or("");
            if arm.contains("routed.is_none()") {
                continue;
            }
            let literal: String = rest.chars().take_while(|c| *c != '"').collect();
            let literal = if pattern.starts_with("strip_prefix") {
                format!("/{literal}")
            } else {
                literal
            };
            if literal.starts_with('/') {
                answered.push(literal);
            }
        }
    }
    assert!(answered.len() > 30, "the routes were read: {answered:?}");
    for route in &answered {
        let route = route.trim_end_matches('/');
        assert!(
            production::reserved(route),
            "main.rs answers `{route}` itself, a path `pw check` does not refuse a program"
        );
    }
}

/// The store, with a page of its own at each path a development server
/// answers only where no page is declared: `/menu`, `/menu-entry` and one
/// under `/page/`.
const PAGES_THERE: &str = r#"

page MenuBoard() {
    route     "/menu"
    placement origin

    view {
        <title>Menu board</title>
        <main><h1>Menu board</h1></main>
    }
}

page MenuEntryPage() {
    route     "/menu-entry"
    placement origin

    view {
        <title>Menu entry</title>
        <main><h1>Menu entry</h1></main>
    }
}

page AboutUs() {
    route     "/page/about"
    placement origin

    view {
        <title>About us</title>
        <main><h1>About us</h1></main>
    }
}
"#;

/// **A development server's own route yields to the program's page there**
/// (the review's finding 5b): `/menu` (store 47's menu, as JSON),
/// `/menu-entry` and `/page/…` answer a program that declares no page at
/// them as before, and one that does gets its page, in development and in
/// production; `pw check` reserves none of them, so a restaurant may have
/// its `/menu`.
#[test]
fn a_development_servers_own_route_yields_to_the_programs_page() {
    let plain = store();
    let (head, body) = got(&plain, "/menu", "");
    assert_eq!(status(&head), 200, "{head}");
    assert!(
        String::from_utf8_lossy(&body).starts_with("{\"items\""),
        "{}",
        String::from_utf8_lossy(&body)
    );
    let paged = served_from_patches_in("examples", |app| format!("{app}{PAGES_THERE}"), &[]);
    let production = served_from_patches_in("examples", |app| format!("{app}{PAGES_THERE}"), &[]);
    let own = production::Production::of(
        &production,
        &production::secrets::Secrets::default(),
        production::Layer::Store,
    )
    .expect("a production host's own");
    assert!(production.production.set(own).is_ok());
    for (path, title) in [
        ("/menu", "Menu board"),
        ("/menu-entry", "Menu entry"),
        ("/page/about", "About us"),
    ] {
        for server in [&paged, &production] {
            let (head, body) = got(server, path, "");
            assert_eq!(status(&head), 200, "{path}: {head}");
            assert!(
                String::from_utf8_lossy(&body).contains(&format!("<title>{title}</title>")),
                "{path}: {}",
                String::from_utf8_lossy(&body)
            );
        }
    }
}

/// **Every response head is written through `production::head`** (the
/// review's finding 5a: an upload's answer wrote its own head, without
/// HSTS or its status for the log): each place a server source formats a
/// response's status line calls it, but two: the development payment
/// provider's own server (another origin, refused in production), and the
/// busy answer of a host at its limit, which no request's thread writes
/// and which carries `production::lines()`.
#[test]
fn every_response_head_is_written_through_the_head_function() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    let mut dirs = vec![root];
    while let Some(dir) = dirs.pop() {
        for entry in std::fs::read_dir(&dir).expect("a directory") {
            let path = entry.expect("an entry").path();
            if path.is_dir() {
                if path.file_name().is_none_or(|n| n != "tests") {
                    dirs.push(path);
                }
            } else if path.extension().is_some_and(|e| e == "rs") {
                files.push(path);
            }
        }
    }
    let mut heads = 0;
    for file in files {
        let text = std::fs::read_to_string(&file).expect("a source");
        let own = text
            .split("#[cfg(test)]\nmod tests")
            .next()
            .unwrap_or(&text);
        let lines: Vec<&str> = own.lines().collect();
        for (at, line) in lines.iter().enumerate() {
            if !line.contains("\"HTTP/1.1 {") && !line.contains("\"HTTP/1.1 200 OK") {
                continue;
            }
            let name = file.display().to_string();
            if name.ends_with("payments.rs") || name.ends_with("connections.rs") {
                continue;
            }
            heads += 1;
            let after = lines[at..(at + 14).min(lines.len())].join("\n");
            assert!(
                after.contains("production::head("),
                "{name}:{}: a response head written without `production::head`:\n{after}",
                at + 1
            );
        }
    }
    assert!(heads >= 6, "the heads were found: {heads}");
}

/// **A production host refuses a build that serves a path the platform
/// keeps** (the review's finding 5b): an upload a build made by hand
/// declares at `/stream`, which `pw check` would have refused; the store as
/// `pw build` builds it is refused nothing.
#[test]
fn a_production_host_refuses_a_build_serving_a_kept_path() {
    assert!(production::build_refusals(&store()).is_empty());
    let (dir, out) = built_feed_with(|app| app.to_string());
    let declared = out.join("uploads.json");
    let text = std::fs::read_to_string(&declared).expect("uploads.json");
    std::fs::write(
        &declared,
        text.replace("\"/uploads/post-image\"", "\"/stream\""),
    )
    .expect("written");
    let feed = Server::from_build(out.clone(), out).expect("served");
    let refused = production::build_refusals(&feed);
    assert_eq!(refused.len(), 1, "{refused:?}");
    assert!(
        refused[0].contains("`PostImage` at `/stream`") && refused[0].contains("PW7001"),
        "{refused:?}"
    );
    drop(dir);
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

/// What `answer` writes to a connection, read as [`asked`] reads an answer.
fn written_by(answer: impl FnOnce(&mut TcpStream) + Send) -> (String, Vec<u8>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind");
    let at = listener.local_addr().expect("address");
    std::thread::scope(|scope| {
        scope.spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            answer(&mut stream);
        });
        let mut client = TcpStream::connect(at).expect("connect");
        let mut out = Vec::new();
        let _ = client.read_to_end(&mut out);
        match out.windows(4).position(|w| w == b"\r\n\r\n") {
            Some(split) => (
                String::from_utf8_lossy(&out[..split]).into_owned(),
                out[split + 4..].to_vec(),
            ),
            None => ("(no answer)".to_string(), out),
        }
    })
}

/// **`serve_file` refuses in production, whatever reaches it**: behind
/// `served_as`, which passes no file of the build, a second refusal of its
/// own (the build's metadata, the paths of the machine that built it);
/// a development server serves its build's files as they are.
#[test]
fn serve_file_refuses_in_production_whatever_reaches_it() {
    let production = production_store(None);
    for route in [
        "/templates.json",
        "/contracts.json",
        "/build-id",
        "/graph.json",
    ] {
        let (head, _) = written_by(|stream| serve_file(&production, stream, route));
        assert_eq!(status(&head), 404, "{route}: {head}");
    }
    let development = store();
    let (head, body) = written_by(|stream| serve_file(&development, stream, "/templates.json"));
    assert_eq!(status(&head), 200, "{head}");
    assert!(!body.is_empty());
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

/// A field of `len` bytes, its line break among them, named `X-<n>`.
fn field_of(n: usize, len: usize) -> String {
    let name = format!("X-{n}: ");
    format!("{name}{}\r\n", "f".repeat(len - name.len() - 2))
}

/// **A head past its bound is refused wherever its lines break** (the
/// review's finding 5c): one of 70,559 bytes whose first 65,536 end on a
/// line break was taken, the rest read as its body. Its neighbours: a head
/// of exactly 65,536 bytes, its blank line the last two, is answered; one
/// byte more, refused.
#[test]
fn a_head_past_its_bound_is_refused_wherever_its_lines_break() {
    let s = store();
    let line = "GET /stores/47 HTTP/1.1\r\nHost: t\r\n";
    // Fields to `to` bytes from the request's first, each line whole.
    let fields_to = |to: usize| {
        let mut head = line.to_string();
        let mut n = 0;
        while to - head.len() > 2000 {
            head.push_str(&field_of(n, 1000));
            n += 1;
        }
        head.push_str(&field_of(n, to - head.len()));
        head
    };
    let at_bound = fields_to(65_536);
    assert_eq!(at_bound.len(), 65_536);
    let mut past = at_bound.clone();
    past.push_str(&field_of(999, 70_559 - 65_536 - 2));
    past.push_str("\r\n");
    assert_eq!(past.len(), 70_559);
    let (head, _) = asked(&s, &past);
    assert_eq!(status(&head), 431, "{head}");

    let exactly = fields_to(65_536 - 2) + "\r\n";
    assert_eq!(exactly.len(), 65_536);
    let (head, _) = asked(&s, &exactly);
    assert_eq!(status(&head), 200, "{head}");
    let over = fields_to(65_536 - 1) + "\r\n";
    assert_eq!(over.len(), 65_537);
    let (head, _) = asked(&s, &over);
    assert_eq!(status(&head), 431, "{head}");
}

/// **A head's lines are read to its blank line**: its request line and its
/// fields, `\r\n` or a bare `\n` ending each (RFC 9112 §2.2); a connection
/// closed before its head ended is answered nothing.
#[test]
fn a_heads_lines_are_read_to_its_blank_line() {
    let read = |bytes: &str| read_head(&mut std::io::BufReader::new(bytes.as_bytes()));
    assert_eq!(
        read("GET / HTTP/1.1\r\nHost: t\r\nA: b\r\n\r\nbody"),
        Head::Read {
            line: "GET / HTTP/1.1\r\n".into(),
            fields: "Host: t\r\nA: b\r\n".into()
        }
    );
    assert_eq!(
        read("GET / HTTP/1.1\nHost: t\n\n"),
        Head::Read {
            line: "GET / HTTP/1.1\n".into(),
            fields: "Host: t\n".into()
        }
    );
    assert_eq!(read(""), Head::Nothing);
    assert_eq!(read("GET / HTTP/1.1\r\nHost: t\r\n"), Head::Cut);
    assert_eq!(read("GET / HTTP"), Head::Cut);
    assert_eq!(read(&"a".repeat(70_000)), Head::TooLarge);
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
    // Read so that a model that ran where it may not fails at the
    // assertion (the mutation controls count only an assertion's failure).
    for model in ["guest", identity::DEV_ACCOUNTS] {
        let refused = s
            .identity
            .configure(&production, Some(model))
            .err()
            .unwrap_or_default();
        assert!(refused.contains("development"), "{model}: {refused}");
    }
    let configured = s
        .identity
        .configure(&production, Some(identity::NO_SIGN_IN));
    assert!(
        configured.is_ok(),
        "no sign-in runs in production: {configured:?}"
    );
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
