//! **An image says what it shows** (PW5053, the image ADR).
//!
//! The HTML standard requires `alt` of every `<img>` (4.8.4.4): text that
//! says what the image shows, or the empty string for one that shows nothing
//! a reader needs. Until 2026-11-06 `pw check` passed an `<img>` with none,
//! and the guide showed it as a gap in the checks (ADR-0343). Each test
//! states one part, with controls.

use pw_core::check::check_sources;

fn library() -> Vec<(String, String)> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut out = Vec::new();
    for d in ["packages/pw-std", "packages/pw-platform-web"] {
        let mut paths: Vec<std::path::PathBuf> = std::fs::read_dir(root.join(d))
            .unwrap_or_else(|e| panic!("{d}: {e}"))
            .map(|e| e.expect("entry").path())
            .filter(|p| p.extension().is_some_and(|x| x == "pw"))
            .collect();
        paths.sort();
        for p in paths {
            out.push((
                p.display().to_string(),
                std::fs::read_to_string(&p).expect("read"),
            ));
        }
    }
    out
}

/// A view, given `photo` and `photos`, whose figure holds `markup`.
fn view(markup: &str) -> String {
    format!(
        "module t\n\nview V(photo: String, photos: List<String>) !{{}} {{\n    <figure>\n        \
         {markup}\n        <figcaption>The shop</figcaption>\n    </figure>\n}}\n"
    )
}

/// Each diagnostic `markup` gets, as `code message | each related note`, and
/// the text its primary span covers.
fn reported(markup: &str) -> Vec<(String, String)> {
    let src = view(markup);
    let mut sources = library();
    sources.push(("t.pw".to_string(), src.clone()));
    check_sources(&sources)
        .into_iter()
        .filter(|(n, _)| n == "t.pw")
        .flat_map(|(_, ds)| ds)
        .map(|d| {
            let notes: Vec<String> = d.related.iter().map(|r| r.label.clone()).collect();
            (
                format!("{} {} | {}", d.code, d.message, notes.join(" | ")),
                src[d.primary_span.clone()].to_string(),
            )
        })
        .collect()
}

/// The images `markup` refuses, each by the text its report covers; nothing
/// else may be reported.
fn refused_at(markup: &str) -> Vec<String> {
    let got = reported(markup);
    for (d, _) in &got {
        assert_eq!(
            d, "PW5053 `<img>` has no `alt` | `V` renders this image",
            "only PW5053 is in question here, for {markup}"
        );
    }
    got.into_iter().map(|(_, at)| at).collect()
}

fn clean(markup: &str) {
    assert_eq!(
        refused_at(markup),
        Vec::<String>::new(),
        "{markup} has its `alt`"
    );
}

fn refused(markup: &str) {
    assert_eq!(
        refused_at(markup).len(),
        1,
        "one image without `alt` in {markup}"
    );
}

#[test]
fn an_image_with_no_alt_is_refused() {
    refused(r#"<img src="/shop.png" />"#);
    refused(r#"<img src={photo} width="640" height="480">"#);
}

#[test]
fn an_alt_says_what_the_image_shows_as_text_or_as_a_value() {
    clean(r#"<img src="/shop.png" alt="The shop's front, with its blue door" />"#);
    clean(r#"<img src={photo} alt={photo} />"#);
}

/// HTML reads `alt` written alone as `alt=""`: either says the image shows
/// nothing a reader needs, and a screen reader passes over it.
#[test]
fn an_empty_alt_says_the_image_shows_nothing_a_reader_needs() {
    clean(r#"<img src="/divider.png" alt="" />"#);
    clean(r#"<img src="/divider.png" alt />"#);
}

/// Each names the image to a screen reader at most, and none is shown where
/// the image does not load; HTML requires `alt` beside them.
#[test]
fn a_title_an_aria_label_or_a_role_is_not_an_alt() {
    refused(r#"<img src="/shop.png" title="The shop's front" />"#);
    refused(r#"<img src="/shop.png" aria-label="The shop's front" />"#);
    refused(r#"<img src="/divider.png" role="presentation" />"#);
    refused(r#"<img src="/divider.png" aria-hidden="true" />"#);
}

#[test]
fn an_image_a_loop_renders_is_read_once_where_it_is_written() {
    assert_eq!(
        refused_at("{#each photos as p (p)}<img src={p}>{/each}"),
        ["<img src={p}>"]
    );
    clean("{#each photos as p (p)}<img src={p} alt=\"\">{/each}");
}

/// The element is `<img>` alone: a `<picture>`'s `<source>` offers the image
/// another file, and the `<img>` it holds has the `alt`.
#[test]
fn only_an_img_is_held_to_it() {
    clean(
        r#"<picture><source srcset="/shop.avif" type="image/avif"><img src="/shop.png" alt="The shop"></picture>"#,
    );
    assert_eq!(
        refused_at(
            r#"<picture><source srcset="/shop.avif" type="image/avif"><img src="/shop.png"></picture>"#
        ),
        [r#"<img src="/shop.png">"#]
    );
}

#[test]
fn each_image_is_reported_where_it_is() {
    assert_eq!(
        refused_at(r#"<img src="/a.png" alt="A"><img src="/b.png"><img src="/c.png" alt="">"#),
        [r#"<img src="/b.png">"#]
    );
    assert_eq!(
        refused_at(r#"<img src="/b.png"><img src="/c.png">"#),
        [r#"<img src="/b.png">"#, r#"<img src="/c.png">"#]
    );
}
