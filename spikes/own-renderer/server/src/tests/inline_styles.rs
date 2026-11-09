//! **A page's stylesheets, small together, are part of its document** (the
//! inline styles' ADR): a first paint waits on no request for them, and the
//! page's policy allows what it now holds, and nothing changes for a page
//! whose stylesheets cannot be moved as they are.

use super::*;

const LINK_A: &str = "<link rel=\"stylesheet\" href=\"/a.css?b=V\">";
const LINK_B: &str = "<link rel=\"stylesheet\" href=\"/b.css?b=V\">";

/// A build holding each `(name, text, bytes as sent)`, whose packaging said to
/// inline where `says` is.
fn delivery(says: bool, files: &[(&str, &str, usize)]) -> (tempfile::TempDir, Delivery) {
    let dir = tempfile::tempdir().expect("a directory");
    let flags = if says { ", \"inline\": true" } else { "" };
    std::fs::write(
        dir.path().join("delivery.json"),
        format!("{{\"versioned\": true{flags}}}\n"),
    )
    .expect("written");
    for (name, text, sent) in files {
        std::fs::write(dir.path().join(name), text).expect("written");
        std::fs::write(dir.path().join(format!("{name}.br")), vec![0u8; *sent]).expect("written");
    }
    let delivery = Delivery::of(dir.path(), dir.path(), "b1");
    (dir, delivery)
}

fn page(body: &str) -> String {
    format!("<!doctype html>\n<head>\n<title>t</title>\n</head>\n<body>\n{body}\n</body>")
}

/// Each `<link>` of `document` at this delivery's version, as it is served.
fn at_version(delivery: &Delivery, document: &str) -> String {
    let version = delivery.version.as_deref().expect("a delivery");
    document.replace("?b=V", &format!("?b={version}"))
}

#[test]
fn a_pages_stylesheets_small_together_are_in_its_head_in_their_order() {
    let (_dir, delivery) = delivery(
        true,
        &[("a.css", "a{color:red}", 100), ("b.css", "b{color:blue}", 100)],
    );
    let document = at_version(&delivery, &page(&format!("{LINK_A}{LINK_B}<main></main>")));
    let (inlined, style) = delivery.styles_inline(document);
    assert_eq!(style.as_deref(), Some("a{color:red}\nb{color:blue}"));
    assert!(
        inlined.contains("<style>a{color:red}\nb{color:blue}</style>\n</head>"),
        "{inlined}"
    );
    assert!(!inlined.contains("<link"), "{inlined}");
    assert!(inlined.contains("<body>\n<main></main>"), "{inlined}");
}

#[test]
fn a_page_with_no_stylesheet_is_as_it_was() {
    let (_dir, delivery) = delivery(true, &[("a.css", "a{}", 100)]);
    let document = page("<main></main>");
    assert_eq!(delivery.styles_inline(document.clone()), (document, None));
}

#[test]
fn stylesheets_too_large_together_stay_links() {
    let (_dir, delivery) = delivery(
        true,
        &[
            ("a.css", "a{}", INLINE_AT_MOST / 2 + 1),
            ("b.css", "b{}", INLINE_AT_MOST / 2),
        ],
    );
    let document = at_version(&delivery, &page(&format!("{LINK_A}{LINK_B}")));
    assert_eq!(delivery.styles_inline(document.clone()), (document, None));
}

#[test]
fn stylesheets_exactly_at_the_bound_are_inlined() {
    let (_dir, delivery) = delivery(
        true,
        &[
            ("a.css", "a{}", INLINE_AT_MOST / 2),
            ("b.css", "b{}", INLINE_AT_MOST - INLINE_AT_MOST / 2),
        ],
    );
    let document = at_version(&delivery, &page(&format!("{LINK_A}{LINK_B}")));
    assert!(delivery.styles_inline(document).1.is_some());
}

#[test]
fn a_build_that_does_not_say_so_keeps_its_links() {
    let (_dir, delivery) = delivery(false, &[("a.css", "a{}", 100)]);
    let document = at_version(&delivery, &page(LINK_A));
    assert_eq!(delivery.styles_inline(document.clone()), (document, None));
}

#[test]
fn one_stylesheet_that_cannot_move_keeps_them_all() {
    for (why, text) in [
        ("an import", "@import \"x.css\";a{}"),
        ("an import, as the sheet might write it", "@IMPORT url(x.css);a{}"),
        ("a namespace", "@namespace svg url(http://x);a{}"),
        ("a relative image", "a{background:url(i.png)}"),
        ("a quoted relative image", "a{background:url( 'i.png' )}"),
        ("the end of a style element", "a{content:\"</style>\"}"),
        ("that end, as HTML also reads it", "a{content:\"</STYLE \"}"),
    ] {
        let (_dir, delivery) = delivery(
            true,
            &[("a.css", text, 100), ("b.css", "b{}", 100)],
        );
        let document = at_version(&delivery, &page(&format!("{LINK_A}{LINK_B}")));
        assert_eq!(
            delivery.styles_inline(document.clone()),
            (document, None),
            "{why}"
        );
    }
}

#[test]
fn an_address_that_means_the_same_from_the_document_can_move() {
    for text in [
        "a{background:url(data:image/png;base64,AA)}",
        "a{background:url(\"https://x/i.png\")}",
        "a{background:url(http://x/i.png)}",
        "a{background:url(/i.png)}",
        "a{background:url(//x/i.png)}",
        "a{fill:url(#g)}",
    ] {
        let (_dir, delivery) = delivery(true, &[("a.css", text, 100)]);
        let document = at_version(&delivery, &page(LINK_A));
        assert_eq!(delivery.styles_inline(document).1.as_deref(), Some(text));
    }
}

#[test]
fn a_stylesheet_the_build_does_not_hold_keeps_them_all() {
    let (_dir, delivery) = delivery(true, &[("a.css", "a{}", 100)]);
    let other = "<link rel=\"stylesheet\" href=\"https://cdn/x.css\">";
    let document = at_version(&delivery, &page(&format!("{LINK_A}{other}")));
    assert_eq!(delivery.styles_inline(document.clone()), (document, None));
}

#[test]
fn a_link_that_says_more_than_its_address_keeps_them_all() {
    let (_dir, delivery) = delivery(true, &[("a.css", "a{}", 100)]);
    let print = "<link rel=\"stylesheet\" href=\"/a.css?b=V\" media=\"print\">";
    let document = at_version(&delivery, &page(&format!("{LINK_A}{print}")));
    assert_eq!(delivery.styles_inline(document.clone()), (document, None));
}

#[test]
fn a_style_of_the_pages_own_keeps_them_all_where_the_order_would_change() {
    let (_dir, delivery) = delivery(true, &[("a.css", "a{}", 100)]);
    let own = "<style>p{}</style>";
    let document = at_version(&delivery, &page(&format!("{own}{LINK_A}")));
    assert_eq!(delivery.styles_inline(document.clone()), (document, None));
}

#[test]
fn a_stylesheet_with_nothing_said_of_how_it_is_sent_stays_a_link() {
    let dir = tempfile::tempdir().expect("a directory");
    std::fs::write(
        dir.path().join("delivery.json"),
        "{\"versioned\": true, \"inline\": true}\n",
    )
    .expect("written");
    std::fs::write(dir.path().join("a.css"), "a{}").expect("written");
    let delivery = Delivery::of(dir.path(), dir.path(), "b1");
    let document = at_version(&delivery, &page(LINK_A));
    assert_eq!(delivery.styles_inline(document.clone()), (document, None));
}

/// **The document's policy allows what it holds, and the page is one document
/// as before.**
#[test]
fn a_production_page_holds_its_stylesheets_and_its_policy_allows_them() {
    let (_dir, out) = super::todomvc::built_todomvc();
    std::fs::write(
        out.join("delivery.json"),
        "{\"versioned\": true, \"device\": true, \"inline\": true}\n",
    )
    .expect("written");
    for file in ["pw-runtime.mjs", "pw-runtime-device.mjs"] {
        std::fs::write(out.join(file), "export {};\n").expect("written");
    }
    for (name, text) in [
        ("todomvc-base.css", "html{margin:0}"),
        ("todomvc-app.css", ".todoapp{color:red}"),
        ("todomvc-pleris.css", ".toggle{color:blue}"),
    ] {
        std::fs::write(out.join(name), text).expect("written");
        std::fs::write(out.join(format!("{name}.br")), vec![0u8; 100]).expect("written");
    }
    let s = Server::from_build(out.clone(), out).expect("served");
    s.prerender().expect("prerendered");
    let answer = get_as(&s, "a", "/");
    let style = "html{margin:0}\n.todoapp{color:red}\n.toggle{color:blue}";
    assert!(
        answer.contains(&format!("<style>{style}</style>\n</head>")),
        "{answer}"
    );
    assert!(!answer.contains("rel=\"stylesheet\""), "{answer}");
    let policy = answer
        .lines()
        .find(|l| l.starts_with("content-security-policy:"))
        .unwrap_or_else(|| panic!("a policy in {answer}"));
    assert!(policy.contains(&csp::sha256_source(style)), "{policy}");
}

/// A page one document is not: rendered for each reader, and sent as it is.
const BOUND: &str = "module t\n\n\
public query Greeting() -> String !{} {\n    \"hello\"\n}\n\n\
page Bound() {\n    route \"/bound\"\n\n    let greeting = query Greeting()\n\n    \
view {\n        <title>Bound</title>\n        \
<link rel=\"stylesheet\" href=\"/a.css\" />\n        <p>{greeting}</p>\n    }\n}\n";

#[test]
fn a_page_rendered_for_each_reader_keeps_its_links() {
    let mut s = super::device_store::served(BOUND);
    let (_dir, delivery) = delivery(true, &[("a.css", "a{}", 100)]);
    s.server.delivery = delivery;
    s.prerender().expect("prerendered");
    let answer = get_as(&s, "a", "/bound");
    assert!(answer.starts_with("HTTP/1.1 200"), "{answer}");
    assert!(answer.contains("rel=\"stylesheet\""), "{answer}");
    assert!(!answer.contains("<style>a{}"), "{answer}");
}
