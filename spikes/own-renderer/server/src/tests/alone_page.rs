//! **A page alone in its build is one document** (ADR-0426), the
//! host's part: where the packaging wrote the page's code and the device's
//! runtime as one module, the page's document holds it and the build's
//! handler table, and asks for nothing as it boots. Any other page is
//! delivered as before.

use super::*;

const PAGE: &str = "todomvc.app.Todos~ui";
const MODULE: &str = "export const alone = 1;\n";

/// TodoMVC, a page alone in its build, packaged for production: the
/// device's runtime, the page's module and, where `alone` is given, the one
/// module of both.
fn todomvc_with(alone: Option<&str>) -> (tempfile::TempDir, Server) {
    let (dir, out) = super::todomvc::built_todomvc();
    std::fs::write(
        out.join("delivery.json"),
        "{\"versioned\": true, \"device\": true, \"bundled\": true}\n",
    )
    .expect("written");
    for file in ["pw-runtime.mjs", "pw-runtime-device.mjs"] {
        std::fs::write(out.join(file), "export {};\n").expect("written");
    }
    let bundles = out.join("bundles");
    std::fs::create_dir_all(&bundles).expect("a directory");
    std::fs::write(bundles.join(format!("{PAGE}.mjs")), "export const a = 1;\n").expect("written");
    if let Some(text) = alone {
        let modules = out.join("alone");
        std::fs::create_dir_all(&modules).expect("a directory");
        std::fs::write(modules.join(format!("{PAGE}.mjs")), text).expect("written");
    }
    let s = Server::from_build(out.clone(), out).expect("served");
    s.prerender().expect("prerendered");
    (dir, s)
}

fn body(answer: &str) -> &str {
    answer.split_once("\r\n\r\n").map_or("", |(_, b)| b)
}

/// The text of the data block holding the build's handler table, where the
/// document has one.
fn table_in(html: &str) -> Option<&str> {
    const OPEN: &str = "<script type=\"application/json\" id=\"pw-handlers\">";
    let start = html.find(OPEN)? + OPEN.len();
    let end = html[start..].find("</script>")?;
    Some(&html[start..start + end])
}

#[test]
fn a_page_alone_in_its_build_holds_its_code_and_asks_for_nothing_as_it_boots() {
    let (_dir, s) = todomvc_with(Some(MODULE));
    let html = super::todomvc::page(&s, "a");
    assert!(
        html.contains(&format!("<script type=\"module\">{MODULE}</script>")),
        "{html}"
    );
    assert!(!html.contains("modulepreload"), "{html}");
    assert!(!html.contains("rel=\"preload\""), "{html}");
    assert!(!html.contains("pw-runtime-device.mjs"), "{html}");
    assert!(!html.contains(&s.delivery.runtime_source()), "{html}");
    assert_eq!(
        html.matches("<script type=\"module\">").count(),
        1,
        "{html}"
    );
}

#[test]
fn its_policy_allows_its_one_script_by_its_hash() {
    let (_dir, s) = todomvc_with(Some(MODULE));
    let html = super::todomvc::page(&s, "a");
    let policy = html
        .lines()
        .find(|l| l.starts_with("content-security-policy:"))
        .unwrap_or_default();
    assert!(policy.contains(&csp::sha256_source(MODULE)), "{html}");
    // And nothing else: the scripts that start a runtime it does not fetch
    // are no part of it.
    let script_src = policy
        .split(';')
        .find(|d| d.trim_start().starts_with("script-src"))
        .unwrap_or_default();
    assert_eq!(script_src.matches("'sha256-").count(), 1, "{policy}");
}

#[test]
fn its_table_is_the_one_its_host_answers_for_its_build() {
    let (_dir, s) = todomvc_with(Some(MODULE));
    let html = super::todomvc::page(&s, "a");
    let answer = get_as(&s, "a", "/pw-handlers");
    let said = table_in(&html).unwrap_or_default();
    let read: Option<String> = serde_json::from_str(said).ok();
    assert_eq!(read.as_deref(), Some(body(&answer)), "{html}");
    assert!(
        body(&answer).contains(&format!("#bundle|{PAGE}|")),
        "{answer}"
    );
    // No `<` in a script element's text (ADR-0097).
    assert!(!said.contains('<'), "{said}");
}

#[test]
fn a_module_the_parser_would_not_read_as_one_script_is_fetched_as_before() {
    for text in [
        "export const a = \"</script>\";\n",
        "export const a = \"</SCRIPT \";\n",
        "export const a = \"<!-- <script>\";\n",
        "export const a = \"<Script\";\n",
    ] {
        let (_dir, s) = todomvc_with(Some(text));
        let html = super::todomvc::page(&s, "a");
        assert!(!html.contains("<script type=\"module\">"), "{text}: {html}");
        assert!(table_in(&html).is_none(), "{text}: {html}");
        let runtime = s.delivery.named("/pw-runtime-device.mjs");
        assert!(
            html.contains(&format!("<link rel=\"modulepreload\" href=\"{runtime}\">")),
            "{text}: {html}"
        );
    }
}

/// **A comment's opening alone is no script's end**: the runtime writes the
/// anchors around a row as comments, and only a `<script` after one could
/// hide the script's end from the parser (the HTML Standard's tokenizer,
/// its script data escaped states).
#[test]
fn a_module_that_writes_comments_is_held_in_its_document() {
    let text = "export const a = \"<!--pw:s1-->\";\n";
    let (_dir, s) = todomvc_with(Some(text));
    let html = super::todomvc::page(&s, "a");
    assert!(
        html.contains(&format!("<script type=\"module\">{text}</script>")),
        "{html}"
    );
}

#[test]
fn a_build_whose_packaging_wrote_no_such_module_fetches_its_runtime_and_code() {
    let (_dir, s) = todomvc_with(None);
    let html = super::todomvc::page(&s, "a");
    assert!(!html.contains("<script type=\"module\">"), "{html}");
    assert!(table_in(&html).is_none(), "{html}");
    let module = s.delivery.named(&format!("/bundle/{PAGE}.mjs"));
    assert!(
        html.contains(&format!("<link rel=\"modulepreload\" href=\"{module}\">")),
        "{html}"
    );
}

/// A packaging's directory, versioned, with the device's runtime and a
/// module of both for `page`, as a build of one page has.
fn alone_delivery(page: &str) -> (tempfile::TempDir, Delivery) {
    let dir = tempfile::tempdir().expect("a directory");
    std::fs::write(
        dir.path().join("delivery.json"),
        "{\"versioned\": true, \"device\": true, \"bundled\": true}\n",
    )
    .expect("written");
    for file in ["pw-runtime.mjs", "pw-runtime-device.mjs"] {
        std::fs::write(dir.path().join(file), "export {};\n").expect("written");
    }
    let bundles = dir.path().join("bundles");
    std::fs::create_dir_all(&bundles).expect("a directory");
    std::fs::write(bundles.join(format!("{page}.mjs")), "export const a = 1;\n").expect("written");
    let modules = dir.path().join("alone");
    std::fs::create_dir_all(&modules).expect("a directory");
    std::fs::write(modules.join(format!("{page}.mjs")), MODULE).expect("written");
    let delivery = Delivery::of(dir.path(), dir.path(), "b1");
    (dir, delivery)
}

#[test]
fn a_delivery_names_the_page_its_packaging_wrote_alone() {
    let (_dir, delivery) = alone_delivery("p");
    assert_eq!(delivery.alone_page(), Some("p"));
    // A second such module is no page alone in its build.
    let modules = _dir.path().join("alone");
    std::fs::write(modules.join("q.mjs"), MODULE).expect("written");
    let two = Delivery::of(_dir.path(), _dir.path(), "b1");
    assert_eq!(two.alone_page(), None);
    // Nor is one in a build with no device's runtime.
    std::fs::remove_file(modules.join("q.mjs")).expect("removed");
    std::fs::write(_dir.path().join("delivery.json"), "{\"versioned\": true}\n").expect("written");
    let undelivered = Delivery::of(_dir.path(), _dir.path(), "b1");
    assert_eq!(undelivered.alone_page(), None);
}

/// Two pages, each on the device alone: no reader decides either, and
/// neither has a layout.
const TWO: &str = "module t\n\n\
page P() {\n    route \"/\"\n\n    signal count: Int = 0\n\n    \
view {\n        <title>P</title>\n        \
<button type=\"button\" on:press={() => count = count + 1}>{count + 1}</button>\n    }\n}\n\n\
page Q() {\n    route \"/q\"\n\n    signal count: Int = 0\n\n    \
view {\n        <title>Q</title>\n        \
<button type=\"button\" on:press={() => count = count + 1}>{count + 1}</button>\n    }\n}\n";

/// **A page of a build of several is not alone**, whatever a module says:
/// the host holds the build to one page, so that each page of a site keeps
/// the one runtime a browser keeps for all of them.
/// A page of presses alone: nothing on it is computed by a host.
const PRESSES: &str = "module t\n\n\
page P() {\n    route \"/\"\n\n    signal count: Int = 0\n\n    \
view {\n        <title>P</title>\n        \
<button type=\"button\" on:press={() => count = count + 1}>{count}</button>\n    }\n}\n";

/// **A build that computes nothing on a host is served**: its contracts
/// name no component, and its build has no `components` directory.
#[test]
fn a_build_whose_contracts_name_no_component_is_served_with_none() {
    let s = super::device_store::served(PRESSES);
    let answer = get_as(&s, "a", "/");
    assert!(answer.starts_with("HTTP/1.1 200"), "{answer}");
}

/// **A build whose contracts export a computed value and that lost its
/// components did not finish**: the host refuses it, as before.
#[test]
fn a_build_that_computes_on_a_host_and_has_no_components_is_refused() {
    let (_dir, out) = super::todomvc::built_todomvc();
    let _ = std::fs::remove_dir_all(out.join("components"));
    let refused = Server::from_build(out.clone(), out)
        .err()
        .unwrap_or_default();
    assert!(refused.contains("components"), "{refused}");
}

#[test]
fn a_page_of_a_build_of_several_fetches_its_runtime_whatever_its_packaging_wrote() {
    let mut s = super::device_store::served(TWO);
    let page = s.plans.keys().next().cloned().unwrap_or_default();
    let (_dir, delivery) = alone_delivery(&page);
    assert_eq!(delivery.alone_page(), Some(page.as_str()));
    s.server.delivery = delivery;
    s.prerender().expect("prerendered");
    for route in ["/", "/q"] {
        let answer = get_as(&s, "a", route);
        assert!(answer.starts_with("HTTP/1.1 200"), "{route}: {answer}");
        assert!(
            !answer.contains("<script type=\"module\">"),
            "{route}: {answer}"
        );
        assert!(
            answer.contains("pw-runtime-device.mjs"),
            "{route}: {answer}"
        );
    }
}
