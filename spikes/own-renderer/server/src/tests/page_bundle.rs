//! **A page's code in one module** (the page bundle's ADR), the host's part:
//! a bundled build names each page's module on its handler table and serves
//! it at its delivery, and any other build names and serves none.

use super::*;

const PAGE: &str = "feed.app.Home~ui";

/// The feed, built, with what `delivery.json` says and a module for its page.
fn built(delivery: &str) -> Server {
    let (_dir, out) = built_feed_with(|app| app.to_string());
    std::fs::write(out.join("delivery.json"), delivery).expect("written");
    let bundles = out.join("bundles");
    std::fs::create_dir_all(&bundles).expect("a directory");
    std::fs::write(bundles.join(format!("{PAGE}.mjs")), "export const a = 1;\n").expect("written");
    let s = Server::from_build(out.clone(), out).expect("served");
    std::mem::forget(_dir);
    s
}

fn body(answer: &str) -> &str {
    answer.split_once("\r\n\r\n").map_or("", |(_, b)| b)
}

#[test]
fn a_bundled_build_names_its_pages_module_on_its_table() {
    let s = built("{\"warm\": true, \"versioned\": true, \"bundled\": true}\n");
    let answer = get_as(&s, "a", "/pw-handlers");
    let table = body(&answer);
    assert!(
        table.contains(&format!("#bundle|{PAGE}|/bundle/{PAGE}.mjs\n")),
        "{table}"
    );
}

#[test]
fn a_build_that_does_not_say_so_names_no_module_and_serves_none() {
    let s = built("{\"warm\": true, \"versioned\": true}\n");
    let answer = get_as(&s, "a", "/pw-handlers");
    assert!(!body(&answer).contains("#bundle"), "{answer}");
    let module = get_as(&s, "a", &format!("/bundle/{PAGE}.mjs"));
    assert!(module.starts_with("HTTP/1.1 404"), "{module}");
}

#[test]
fn a_bundled_module_is_kept_a_year_at_its_delivery_and_revalidated_at_another() {
    let s = built("{\"warm\": true, \"versioned\": true, \"bundled\": true}\n");
    let version = s.delivery.version.clone().expect("a delivery id");
    let kept = get_as(&s, "a", &format!("/bundle/{PAGE}.mjs?b={version}"));
    assert!(kept.starts_with("HTTP/1.1 200"), "{kept}");
    assert!(kept.contains("text/javascript"), "{kept}");
    assert!(
        kept.contains("cache-control: public, max-age=31536000, immutable\r\n"),
        "{kept}"
    );
    assert_eq!(body(&kept), "export const a = 1;\n");
    let other = get_as(&s, "a", &format!("/bundle/{PAGE}.mjs?b=0000000000000000"));
    assert!(other.starts_with("HTTP/1.1 200"), "{other}");
    assert!(other.contains("public, no-cache"), "{other}");
}

#[test]
fn only_a_module_its_packaging_wrote_is_served() {
    let s = built("{\"warm\": true, \"versioned\": true, \"bundled\": true}\n");
    for path in [
        "/bundle/feed.app.Nowhere~ui.mjs",
        "/bundle/../contracts.json",
        "/bundle/..%2Fcontracts.json",
        "/bundle/",
    ] {
        let answer = get_as(&s, "a", path);
        assert!(answer.starts_with("HTTP/1.1 404"), "{path}: {answer}");
    }
}

#[test]
fn a_bundle_is_one_of_what_a_delivery_digests() {
    let id = |bundle: &str| {
        let dir = tempfile::tempdir().expect("a directory");
        std::fs::write(
            dir.path().join("delivery.json"),
            "{\"versioned\": true, \"bundled\": true}\n",
        )
        .expect("written");
        std::fs::create_dir_all(dir.path().join("bundles")).expect("a directory");
        std::fs::write(dir.path().join("bundles").join("p.mjs"), bundle).expect("written");
        Delivery::of(dir.path(), dir.path(), "b1").version
    };
    assert_ne!(id("export const a = 1;"), id("export const a = 2;"));
}

/// A build whose packaging wrote the device's runtime.
fn device_delivery() -> (tempfile::TempDir, Delivery) {
    let dir = tempfile::tempdir().expect("a directory");
    std::fs::write(
        dir.path().join("delivery.json"),
        "{\"versioned\": true, \"device\": true}\n",
    )
    .expect("written");
    for file in ["pw-runtime.mjs", "pw-runtime-device.mjs"] {
        std::fs::write(dir.path().join(file), "export {};\n").expect("written");
    }
    let delivery = Delivery::of(dir.path(), dir.path(), "b1");
    (dir, delivery)
}

#[test]
fn a_page_that_talks_to_no_server_starts_the_devices_runtime() {
    let (_dir, delivery) = device_delivery();
    let document = delivery.deliver(format!("<head>{BOOT}</head><body>{RUNTIME}</body>"));
    let on_device = delivery.on_device(document.clone());
    let device = delivery.device_source().expect("a device runtime");
    assert!(
        on_device.contains(&format!("<script>{device}</script>")),
        "{on_device}"
    );
    assert!(on_device.contains("pw-runtime-device.mjs"), "{on_device}");
    assert!(!on_device.contains("/pw-runtime.mjs"), "{on_device}");
    assert!(
        !on_device.contains(&delivery.runtime_source()),
        "{on_device}"
    );
    assert!(document.contains("/pw-runtime.mjs"), "{document}");
}

#[test]
fn a_build_with_no_device_runtime_leaves_a_document_as_it_was() {
    let dir = tempfile::tempdir().expect("a directory");
    std::fs::write(dir.path().join("delivery.json"), "{\"versioned\": true}\n").expect("written");
    let delivery = Delivery::of(dir.path(), dir.path(), "b1");
    assert_eq!(delivery.device_source(), None);
    let document = delivery.deliver(format!("<head>{BOOT}</head><body>{RUNTIME}</body>"));
    assert_eq!(delivery.on_device(document.clone()), document);
}

#[test]
fn only_a_page_no_reader_decides_and_with_no_layout_runs_on_the_device() {
    let plan = |text: &str| serde_json::from_str::<serde_json::Value>(text).expect("JSON");
    assert!(runs_on_device(&plan("{\"prerender\": true}")));
    assert!(runs_on_device(&plan(
        "{\"prerender\": true, \"layout\": null}"
    )));
    assert!(!runs_on_device(&plan("{\"prerender\": false}")));
    assert!(!runs_on_device(&plan("{}")));
    assert!(!runs_on_device(&plan(
        "{\"prerender\": true, \"layout\": {\"path\": \"x\"}}"
    )));
}

#[test]
fn the_policy_allows_each_runtime_a_build_starts() {
    let one = csp::document_with(
        &["a()"],
        "",
        &csp::Needs::default(),
        &csp::Deployment::default(),
    );
    let two = csp::document_with(
        &["a()", "b()"],
        "",
        &csp::Needs::default(),
        &csp::Deployment::default(),
    );
    assert_eq!(one.matches("sha256-").count(), 1, "{one}");
    assert_eq!(two.matches("sha256-").count(), 2, "{two}");
}

#[test]
fn a_file_its_delivery_does_not_name_is_no_device_runtime() {
    let dir = tempfile::tempdir().expect("a directory");
    std::fs::write(dir.path().join("delivery.json"), "{\"versioned\": true}\n").expect("written");
    std::fs::write(dir.path().join("pw-runtime-device.mjs"), "export {};\n").expect("written");
    let delivery = Delivery::of(dir.path(), dir.path(), "b1");
    assert_eq!(delivery.device_source(), None);
}

#[test]
fn a_delivery_that_says_so_with_no_file_has_no_device_runtime() {
    let dir = tempfile::tempdir().expect("a directory");
    std::fs::write(
        dir.path().join("delivery.json"),
        "{\"versioned\": true, \"device\": true}\n",
    )
    .expect("written");
    let delivery = Delivery::of(dir.path(), dir.path(), "b1");
    assert_eq!(delivery.device_source(), None);
}

/// A page that a layout holds, and that no reader decides.
const WITH_LAYOUT: &str = "module t\n\n\
opaque type InteractionId = String\n\n\
command send(text: String) -> Result<Int, String>\n    requires Anyone\n    \
idempotent_by InteractionId\n{\n    Ok(1)\n}\n\n\
layout Shell {\n    view {\n        <p>a</p>\n        <main><slot /></main>\n    }\n}\n\n\
page P() {\n    route \"/p\"\n    layout Shell\n\n    signal count: Int = 0\n\n    \
view {\n        <title>P</title>\n        \
<button type=\"button\" on:press={() => count = count + 1}>{count}</button>\n    }\n}\n";

/// **A page with a layout keeps the whole runtime**: it navigates within the
/// layout, and its navigation is what the device's runtime does not have.
#[test]
fn a_page_with_a_layout_starts_the_whole_runtime() {
    let mut s = super::device_store::served(WITH_LAYOUT);
    let (_dir, delivery) = device_delivery();
    s.server.delivery = delivery;
    s.prerender().expect("prerendered");
    let answer = get_as(&s, "a", "/p");
    assert!(answer.starts_with("HTTP/1.1 200"), "{answer}");
    assert!(
        answer.contains(&format!("<script>{}</script>", s.delivery.runtime_source())),
        "{answer}"
    );
    assert!(!answer.contains("pw-runtime-device.mjs"), "{answer}");
}
