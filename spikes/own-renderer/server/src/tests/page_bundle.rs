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
