//! **What a page this host serves may load and run** (`csp`; ADR-0195's
//! ruling 13). Every HTML answer carries a Content-Security-Policy: a
//! document's allows the inline script that starts its runtime and the style
//! it holds, each by its hash, and what the build and the deployment ask;
//! every other HTML answer runs nothing. Each rule's test has its control.

use super::*;

/// The store, as `pw build` builds it from `examples`, and served.
fn store() -> Served {
    served_from_patches_in("examples", |app| app.to_string(), &[])
}

/// A response, whole: its head and its body.
fn answer(s: &Server, path: &str) -> (String, String) {
    let whole: String = fetched(s, path).into_iter().map(|(_, c)| c).collect();
    let (head, body) = whole.split_once("\r\n\r\n").expect("a head and a body");
    (head.to_string(), body.to_string())
}

/// The answer's policies, as many as it sends.
fn policies(head: &str) -> Vec<String> {
    head.lines()
        .filter_map(|l| l.split_once(':'))
        .filter(|(k, _)| k.eq_ignore_ascii_case("content-security-policy"))
        .map(|(_, v)| v.trim().to_string())
        .collect()
}

/// A directive's sources, as its policy lists them.
fn directive<'p>(policy: &'p str, name: &str) -> Option<Vec<&'p str>> {
    policy.split(';').map(str::trim).find_map(|d| {
        let mut words = d.split_ascii_whitespace();
        (words.next() == Some(name)).then(|| words.collect())
    })
}

/// Each `<tag>` element's attributes and text, as a browser reads a raw
/// text element: its text runs to its end tag.
fn elements<'d>(document: &'d str, tag: &str) -> Vec<(&'d str, &'d str)> {
    let lower = document.to_ascii_lowercase();
    let (open, close) = (format!("<{tag}"), format!("</{tag}"));
    let mut found = Vec::new();
    let mut at = 0;
    while let Some(start) = lower[at..].find(&open).map(|i| at + i) {
        let attributes_end = start + lower[start..].find('>').expect("a tag ends");
        let text_end = attributes_end + lower[attributes_end..].find(&close).expect("an end tag");
        found.push((
            &document[start + open.len()..attributes_end],
            &document[attributes_end + 1..text_end],
        ));
        at = text_end;
    }
    found
}

/// **The runtime is started by the text its policy hashes**: `RUNTIME` is
/// `RUNTIME_SOURCE` in a script element, and nothing else.
#[test]
fn the_runtime_is_started_by_the_text_its_policy_hashes() {
    assert_eq!(RUNTIME, format!("<script>{RUNTIME_SOURCE}</script>"));
}

/// **A document allows each inline script and style it holds, by its hash,
/// and nothing inline else**: whatever the browser would run is in the
/// policy the document is sent with, once; a data block (`pw-parts`) is
/// not a script a policy governs (HTML, "data block"). The store takes no
/// payments here, so it frames nothing, and its markup writes no `style`
/// attribute, so none is allowed.
#[test]
fn a_document_allows_each_inline_script_and_style_it_holds_by_its_hash() {
    let s = store();
    for path in ["/", "/cart", "/stores/47", "/StorePage.html"] {
        let (head, body) = answer(&s, path);
        // The home page and the cart stream nothing, so they are written
        // whole; the store's page streams its estimate and its
        // recommendations, and is written whole only where they settle
        // first, as they may (a streamed head is
        // `a_streamed_document_carries_its_policy_in_its_head`'s).
        if matches!(path, "/" | "/cart") {
            assert!(head.contains("content-length"), "{path}: {head}");
        }
        let [policy] = policies(&head)
            .try_into()
            .unwrap_or_else(|p: Vec<String>| panic!("{path}: one policy, not {}: {head}", p.len()));
        let scripts = directive(&policy, "script-src").expect("script-src");
        let ran: Vec<&str> = elements(&body, "script")
            .into_iter()
            .filter(|(attributes, _)| !attributes.contains("type=\"application/json\""))
            .map(|(_, text)| text)
            .collect();
        assert_eq!(ran, vec![RUNTIME_SOURCE], "{path}: the one script it runs");
        for text in &ran {
            assert!(
                scripts.contains(&csp::sha256_source(text).as_str()),
                "{path}: `{text}` is allowed: {policy}"
            );
        }
        let styles = directive(&policy, "style-src").expect("style-src");
        let held = elements(&body, "style");
        assert!(!held.is_empty(), "{path}: the store's style is in its head");
        for (_, text) in held {
            assert!(
                styles.contains(&csp::sha256_source(text).as_str()),
                "{path}: its style is allowed: {policy}"
            );
        }
        for (name, sources) in [
            ("default-src", vec!["'none'"]),
            ("img-src", vec!["'self'"]),
            ("connect-src", vec!["'self'"]),
            ("form-action", vec!["'self'"]),
            ("frame-ancestors", vec!["'none'"]),
            ("base-uri", vec!["'none'"]),
        ] {
            assert_eq!(directive(&policy, name), Some(sources), "{path}: {policy}");
        }
        assert_eq!(scripts[..1], ["'self'"], "{policy}");
        assert!(!policy.contains("'wasm-unsafe-eval'"), "{policy}");
        for absent in ["frame-src", "style-src-attr"] {
            assert_eq!(directive(&policy, absent), None, "{path}: {policy}");
        }
        assert!(!policy.contains("'unsafe-inline'"), "{policy}");
        assert!(!policy.contains("'unsafe-eval'"), "{policy}");
    }
}

/// **A deployment that takes payments frames its provider, and only it**:
/// the order page frames the provider's page (ADR-0332), whose origin is
/// the policy's one frame source.
#[test]
fn a_deployment_that_takes_payments_frames_its_provider_alone() {
    paying_store(|s| {
        let provider = s.payments.get().expect("payments").provider.clone();
        let (head, _) = answer(s, "/stores/47");
        let [policy] = policies(&head).try_into().expect("one policy");
        assert_eq!(
            directive(&policy, "frame-src"),
            Some(vec![provider.as_str()]),
            "{policy}"
        );
    });
}

/// The store taking payments from the development provider, as the order's
/// tests have it (`store_payments`), its webhook unheard here.
fn paying_store(test: impl FnOnce(&Server)) {
    let s = store();
    let port = TcpListener::bind(("127.0.0.1", 0))
        .and_then(|l| l.local_addr())
        .expect("a free port")
        .port();
    let provider_port = TcpListener::bind(("127.0.0.1", 0))
        .and_then(|l| l.local_addr())
        .expect("a free port")
        .port();
    let (configured, _origin) =
        payments::development(&format!("http://127.0.0.1:{port}"), port, provider_port)
            .expect("the development provider");
    assert!(s.payments.set(configured).is_ok(), "set once");
    test(&s);
}

/// **A sign-in that leaves this origin is a form's to send there**: where
/// the deployment's identity provider is at its own origin, a form that
/// starts a sign-in is answered with a redirect there, and the policy lets
/// it go; the development provider, served here, adds nothing.
#[test]
fn a_sign_in_at_another_origin_is_a_forms_to_reach() {
    struct Elsewhere;
    impl identity::Provider for Elsewhere {
        fn issuer(&self) -> String {
            "https://id.example".into()
        }
        fn client_id(&self) -> String {
            "c".into()
        }
        fn authorization_endpoint(&self) -> String {
            "https://id.example/authorize".into()
        }
        fn exchange(&self, _: &str, _: &str, _: &str) -> Result<identity::Claims, String> {
            Err("unused".into())
        }
    }
    let s = store();
    let (head, _) = answer(&s, "/stores/47");
    let [guest] = policies(&head).try_into().expect("one policy");
    assert_eq!(directive(&guest, "form-action"), Some(vec!["'self'"]));
    s.identity
        .use_provider(Arc::new(Elsewhere), "http://127.0.0.1/sign-in/callback");
    let (head, _) = answer(&s, "/stores/47");
    let [elsewhere] = policies(&head).try_into().expect("one policy");
    assert_eq!(
        directive(&elsewhere, "form-action"),
        Some(vec!["'self'", "https://id.example"]),
        "{elsewhere}"
    );
}

/// **A streamed document carries its policy in its head**, which is
/// written before any region settles (ADR-0148): the same policy as a
/// document written whole.
#[test]
fn a_streamed_document_carries_its_policy_in_its_head() {
    let s = served_from(with_recommendations, None);
    recommend(&s, 200, None);
    let (head, body) = answer(&s, "/StorePage.html");
    assert!(!head.contains("content-length"), "streamed: {head}");
    assert!(
        body.contains("<template for="),
        "a region settled after: {body}"
    );
    assert_eq!(policies(&head), vec![s.policy()], "{head}");
    assert!(
        directive(&s.policy(), "script-src")
            .expect("script-src")
            .contains(&csp::sha256_source(RUNTIME_SOURCE).as_str()),
        "{head}"
    );
}

/// **Every other HTML answer runs nothing**, and an answer that is not HTML
/// carries no policy: a page not found, its own text; a page the build
/// rendered with no runtime, served as a file; a file the build does not
/// have, said in plain text.
#[test]
fn every_other_html_answer_runs_nothing() {
    let s = store();
    let (head, body) = answer(&s, "/stores/999");
    assert!(head.starts_with("HTTP/1.1 404"), "{head}");
    assert!(body.contains("<h1>Not found</h1>"), "{body}");
    assert_eq!(policies(&head), vec![csp::BARE.to_string()], "{head}");
    std::fs::write(
        s.dist.join("Rendered.html"),
        "<!doctype html><title>Rendered</title><p>A page.</p>",
    )
    .expect("a page rendered into the build");
    let (head, body) = answer(&s, "/Rendered.html");
    assert!(body.contains("<p>A page.</p>"), "{body}");
    assert_eq!(policies(&head), vec![csp::BARE.to_string()], "{head}");
    let (head, _) = answer(&s, "/no-such-file.txt");
    assert!(head.contains("content-type: text/plain"), "{head}");
    assert_eq!(policies(&head), Vec::<String>::new(), "{head}");
}

/// **Every answer's type is its type**: `nosniff` on a document, streamed
/// or whole, a page not found, a file, a command's answer and the
/// identity's page, so that `'self'` allows only this origin's scripts
/// served as scripts; and a stylesheet the build ships is served as one.
#[test]
fn every_answers_type_is_its_type() {
    let s = store();
    std::fs::write(s.dist.join("a.css"), "p { margin: 0 }").expect("a stylesheet");
    for path in ["/stores/47", "/stores/999", "/a.css", "/no-such-file.txt"] {
        let (head, _) = answer(&s, path);
        assert!(
            head.contains("\r\nx-content-type-options: nosniff\r\n"),
            "{path}: {head}"
        );
    }
    let (head, _) = answer(&s, "/a.css");
    assert!(
        head.contains("\r\ncontent-type: text/css; charset=utf-8\r\n"),
        "{head}"
    );
    let streamed = served_from(with_recommendations, None);
    recommend(&streamed, 200, None);
    let (head, _) = answer(&streamed, "/StorePage.html");
    assert!(!head.contains("content-length"), "streamed: {head}");
    assert!(
        head.contains("\r\nx-content-type-options: nosniff\r\n"),
        "{head}"
    );
}

/// **The host writes no `style` attribute a policy would need**: the
/// page's announcer is served unstyled, and the runtime hides it through
/// the CSSOM.
#[test]
fn the_host_writes_no_style_attribute() {
    assert!(
        !pw_render::ANNOUNCER.contains("style"),
        "{}",
        pw_render::ANNOUNCER
    );
    let s = store();
    let (_, body) = answer(&s, "/stores/47");
    let needs = csp::Needs::of(&[pw_render::ir::Template {
        path: "served".into(),
        name: "served".into(),
        params: Vec::new(),
        schema: String::new(),
        chunks: vec![pw_render::ir::Chunk::Static(
            body.split("<script").next().unwrap_or_default().to_string(),
        )],
    }]);
    assert!(!needs.style_attributes, "{body}");
}

/// **A build's stylesheets may draw with their own `data:` images** (the
/// stylesheets' images' ADR): what the build's files ask, and nothing a build
/// that asks nothing is given.
#[test]
fn a_build_whose_stylesheets_draw_with_data_allows_those_images() {
    for (css, allowed) in [
        ("a{background:url(data:image/png;base64,AA)}", true),
        ("a{background:url('data:image/svg+xml;utf8,<svg/>')}", true),
        ("a{background:url( \"DATA:IMAGE/png;base64,AA\" )}", true),
        ("a{background:url(i.png)}", false),
        ("a{content:\"data:image/png\"}", false),
        ("a{color:red}", false),
    ] {
        let dir = tempfile::tempdir().expect("a directory");
        std::fs::write(dir.path().join("a.css"), css).expect("written");
        assert_eq!(
            crate::stylesheets_draw_with_data(dir.path()),
            allowed,
            "{css}"
        );
    }
    let dir = tempfile::tempdir().expect("a directory");
    std::fs::write(dir.path().join("notes.txt"), "url(data:image/png)").expect("written");
    assert!(!crate::stylesheets_draw_with_data(dir.path()));
}

#[test]
fn a_policy_allows_data_images_only_where_a_build_needs_them() {
    let policy = |data_images| {
        csp::document(
            "go()",
            "",
            &csp::Needs {
                data_images,
                ..csp::Needs::default()
            },
            &csp::Deployment::default(),
        )
    };
    assert!(
        policy(true).contains("img-src 'self' data:;"),
        "{}",
        policy(true)
    );
    assert!(
        policy(false).contains("img-src 'self';"),
        "{}",
        policy(false)
    );
}
