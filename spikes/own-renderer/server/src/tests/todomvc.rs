//! **TodoMVC, built and served** (track `todomvc`, docs/PARALLEL.md, W7; its
//! todos on the device since track `device-store`, ADR-0366): the page the
//! host renders before the device is read, what its document says the
//! browser is to read, and that no host holds a todo. What the browser does
//! with the device is the browser's suite's (`e2e/todomvc.spec.mjs`).

use super::*;

const PAGE: &str = "todomvc.app.Todos~ui";

/// The program, built as `pw build` builds it, and served.
pub(super) fn served_todomvc() -> Served {
    let (dir, out) = built_todomvc();
    Served {
        server: Server::from_build(out.clone(), out).expect("served"),
        _dir: dir,
    }
}

/// The program, built, and where.
fn built_todomvc() -> (tempfile::TempDir, std::path::PathBuf) {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let dir = tempfile::TempDir::with_prefix("pw-todomvc-").expect("a temporary directory");
    let mut units = Vec::new();
    for d in [
        "packages/pw-std",
        "packages/pw-platform-web",
        "examples/todomvc",
    ] {
        let mut paths: Vec<std::path::PathBuf> = std::fs::read_dir(root.join(d))
            .unwrap_or_else(|e| panic!("{d}: {e}"))
            .map(|e| e.expect("entry").path())
            .filter(|p| p.extension().is_some_and(|x| x == "pw"))
            .collect();
        paths.sort();
        for p in paths {
            let src = std::fs::read_to_string(&p).expect("read");
            units.push(pw_core::check::Unit {
                hir: pw_core::lower::lower_file(&src, &pw_syntax::parse_tree(&src).green),
                path: p.display().to_string(),
                src,
            });
        }
    }
    let build = pw_core::build::build(&units);
    assert!(
        build.is_ok(),
        "the valid TodoMVC program builds: {:?}",
        build.as_ref().err()
    );
    let build = build.expect("the executable build was asserted");
    assert!(build.refusals().is_empty(), "{:?}", build.refusals());
    let out = dir.path().join("build");
    build.write(&out).expect("the build is written");
    (dir, out)
}

/// The page `session` is served, as a browser is answered: the document
/// whole, its root and its head among it.
fn page(s: &Server, session: &str) -> String {
    let answer: String = fetched_as(s, "/", Some(session))
        .into_iter()
        .map(|(_, c)| c)
        .collect();
    assert!(answer.starts_with("HTTP/1.1 200"), "{answer}");
    // The page that is served there is TodoMVC's.
    assert!(
        answer.contains(&format!("\"template\":\"{PAGE}\"")),
        "{answer}"
    );
    answer
}

/// An element's opening tag, the first with `marker` in it.
fn tag_of<'a>(html: &'a str, marker: &str) -> &'a str {
    let at = html
        .find(marker)
        .unwrap_or_else(|| panic!("`{marker}` in {html}"));
    let start = html[..at].rfind('<').expect("its tag's start");
    &html[start..at + html[at..].find('>').expect("its tag's end") + 1]
}

/// The document's parts manifest.
fn manifest(html: &str) -> serde_json::Value {
    let start = html
        .find("<script type=\"application/json\" id=\"pw-parts\">")
        .expect("a manifest")
        + "<script type=\"application/json\" id=\"pw-parts\">".len();
    let end = start + html[start..].find("</script>").expect("its end");
    serde_json::from_str(&html[start..end]).expect("JSON")
}

/// **No todos, as the host renders them: `.main` and `.footer` hidden, not
/// absent** (the spec's "when there are no todos, `#main` and `#footer`
/// should be hidden"), at the declared default; and each element the device
/// decides marked, hidden while the document reads the device.
#[test]
fn the_host_renders_the_default_and_hides_what_the_device_decides() {
    let s = served_todomvc();
    let html = page(&s, "a");
    assert!(
        html.contains("<html lang=\"en\" data-pw-device=\"reading\">"),
        "{html}"
    );
    assert!(
        html.contains(&format!("<style>{}</style>", crate::device::READING)),
        "{html}"
    );
    // Each style the head holds is one the document's policy allows by its
    // hash (ADR-0333), so the browser applies it: else what the device
    // decides would show at the default while it is read.
    let policy = html
        .lines()
        .find(|l| l.starts_with("content-security-policy:"))
        .unwrap_or_else(|| panic!("a policy in {html}"));
    for style in [crate::device::READING, crate::device::UNSCRIPTED] {
        let hash = crate::csp::sha256_source(style);
        assert!(policy.contains(&hash), "{hash} in {policy}");
    }
    for marker in ["class=\"main\"", "class=\"footer\""] {
        let tag = tag_of(&html, marker);
        assert!(tag.contains(" hidden"), "{tag}");
        assert!(tag.contains(" data-pw-device"), "{tag}");
    }
    let toggle_all = tag_of(&html, "class=\"toggle-all\"");
    assert!(!toggle_all.contains(" checked"), "{toggle_all}");
    // The count, at the default: in an element the device decides.
    assert!(
        html.contains("<strong data-pw-device><!--pw:s"),
        "the count's element is marked: {html}"
    );
    // What is the page's own is not: the header and its focused field.
    let field = tag_of(&html, "class=\"new-todo\"");
    assert!(field.contains(" autofocus"), "{field}");
    assert!(!field.contains("data-pw-device"), "{field}");
    assert!(
        !tag_of(&html, "class=\"header\"").contains("data-pw-device"),
        "{html}"
    );
    // Control: no list is rendered, since the default is none.
    assert!(!html.contains("class=\"toggle\""), "{html}");
}

/// **The document says what the browser is to read**: the todos, under
/// TodoMVC's own name (`todos-[framework]`), at version 1, the whole list
/// written last every tab's, a list of `{id, title, completed}`.
#[test]
fn the_document_says_what_the_device_keeps() {
    let s = served_todomvc();
    let device = manifest(&page(&s, "a"))["device"].clone();
    assert_eq!(
        device,
        serde_json::json!([{
            "signal": "todos",
            "name": "todos-pleris",
            "version": 1,
            "conflict": "last_write_wins",
            "schemas": [{"t": "list", "of": {"t": "record", "fields": [
                ["id", {"t": "int"}],
                ["title", {"t": "string"}],
                ["completed", {"t": "bool"}],
            ]}}],
        }])
    );
}

/// **No host holds a todo**: the program declares no command and imports no
/// host operation, so every session is served the same page, the default.
#[test]
fn no_host_holds_a_todo() {
    let s = served_todomvc();
    assert!(
        s.contracts.iter().all(|c| c.imports.is_empty()),
        "{:?}",
        s.contracts
            .iter()
            .flat_map(|c| &c.imports)
            .map(|i| &i.interface)
            .collect::<Vec<_>>()
    );
    assert!(
        !s.contracts
            .iter()
            .flat_map(|c| &c.exports)
            .any(|e| e.kind == "command"),
        "TodoMVC declares no command"
    );
    // Two sessions, one page.
    let (a, b) = (page(&s, "a"), page(&s, "b"));
    let body = |h: &str| h.split("<script").next().unwrap_or_default().to_string();
    assert_eq!(body(&a), body(&b));
}

#[test]
fn the_page_has_the_templates_markup() {
    let s = served_todomvc();
    let html = page(&s, "a");
    for marker in [
        "class=\"todoapp\"",
        "class=\"header\"",
        "class=\"new-todo\"",
        "class=\"main\"",
        "class=\"toggle-all\"",
        "class=\"todo-list\"",
        "class=\"footer\"",
        "class=\"todo-count\"",
        "class=\"filters\"",
        "class=\"clear-completed\"",
        "class=\"info\"",
    ] {
        assert!(html.contains(marker), "{marker} in {html}");
    }
    // The new-todo input is focused when the page is loaded, "preferably by
    // using the `autofocus` input attribute".
    assert!(
        tag_of(&html, "class=\"new-todo\"").contains(" autofocus"),
        "{html}"
    );
    // The filters' three links, the default selected.
    assert!(html.contains("href=\"#/\""), "{html}");
    assert!(html.contains("href=\"#/active\""), "{html}");
    assert!(html.contains("href=\"#/completed\""), "{html}");
    assert!(
        tag_of(&html, "href=\"#/\"").contains("class=\"selected\""),
        "{html}"
    );
    // And TodoMVC's own stylesheets, which this host serves.
    assert!(html.contains("/todomvc-app.css"), "{html}");
}

/// **In a production build, the page runs on the device's runtime** (the device
/// runtime's ADR): no reader decides it and it has no layout, so its one
/// document starts the small runtime, and its policy allows that.
#[test]
fn a_production_build_of_the_page_starts_the_devices_runtime() {
    let (_dir, out) = built_todomvc();
    std::fs::write(
        out.join("delivery.json"),
        "{\"versioned\": true, \"device\": true}\n",
    )
    .expect("written");
    for file in ["pw-runtime.mjs", "pw-runtime-device.mjs"] {
        std::fs::write(out.join(file), "export {};\n").expect("written");
    }
    let s = Server::from_build(out.clone(), out).expect("served");
    s.prerender().expect("prerendered");
    let html = page(&s, "a");
    let device = s.delivery.device_source().expect("a device runtime");
    assert!(
        html.contains(&format!("<script>{device}</script>")),
        "{html}"
    );
    assert!(
        !html.contains(&format!("<script>{}</script>", s.delivery.runtime_source())),
        "{html}"
    );
    let policy = html
        .lines()
        .find(|l| l.starts_with("content-security-policy:"))
        .unwrap_or_else(|| panic!("a policy in {html}"));
    assert!(policy.contains(&csp::sha256_source(&device)), "{policy}");
}
