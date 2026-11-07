//! **An image says what it shows** (PW5053, ADR-0404).
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

// --- an image button (ADR-0405) ----------------------------------
//
// HTML 4.10.5.1.19: an `<input type="image">`'s `alt` "provides the textual
// label for the button", and "must be present, and must contain a non-empty
// string". Where the image is not shown, the element is a submit button whose
// label is that `alt`. WCAG 2.2's F65 names it beside `img` and `area`.

/// A view whose form holds `markup`, beside a field its label names.
fn form(markup: &str) -> String {
    format!(
        "module t\n\nview V(caption: String) !{{}} {{\n    <form>\n        \
         <label>Find <input type=\"search\" name=\"q\" /></label>\n        {markup}\n    </form>\n}}\n"
    )
}

/// Each diagnostic a form holding `markup` gets, as `code message | each
/// related note`, and the text its primary span covers.
fn in_form_at(markup: &str) -> Vec<(String, String)> {
    let src = form(markup);
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

fn in_form(markup: &str) -> Vec<String> {
    in_form_at(markup).into_iter().map(|(d, _)| d).collect()
}

const NO_ALT: &str = "PW5053 `<input type=\"image\">` has no `alt` | `V` renders this button";
const BLANK_ALT: &str = "PW5053 `<input type=\"image\">` has a blank `alt` | `V` renders this \
                         button | this `alt` is the button's label, and it is blank";

#[test]
fn an_image_button_with_no_alt_is_refused() {
    assert_eq!(
        in_form(r#"<input type="image" src="/search.png" />"#),
        [NO_ALT]
    );
    // HTML matches `type`'s keywords whatever their case.
    assert_eq!(
        in_form(r#"<input type="Image" src="/search.png" />"#),
        [NO_ALT]
    );
}

/// Unlike an `<img>`'s, which is empty where the image shows nothing a
/// reader needs: a button always does something a reader needs. HTML-AAM
/// reads an `alt` trimmed of white space, so a blank one names nothing.
#[test]
fn an_image_button_s_alt_is_its_label_and_never_blank() {
    for markup in [
        r#"<input type="image" src="/search.png" alt="" />"#,
        r#"<input type="image" src="/search.png" alt />"#,
        r#"<input type="image" src="/search.png" alt="   " />"#,
    ] {
        assert_eq!(in_form(markup), [BLANK_ALT], "{markup}");
    }
}

#[test]
fn an_image_button_s_alt_says_what_it_does_as_text_or_as_a_value() {
    assert_eq!(
        in_form(r#"<input type="image" src="/search.png" alt="Search" />"#),
        Vec::<String>::new()
    );
    assert_eq!(
        in_form(r#"<input type="image" src="/search.png" alt={caption} />"#),
        Vec::<String>::new()
    );
}

/// As for an `<img>`: HTML requires the `alt` beside each of them, and none
/// is shown where the image does not load.
#[test]
fn an_aria_label_a_title_or_a_label_is_not_an_image_button_s_alt() {
    for markup in [
        r#"<input type="image" src="/search.png" aria-label="Search" />"#,
        r#"<input type="image" src="/search.png" title="Search" />"#,
        r#"<h2 id="h">Search</h2><input type="image" src="/search.png" aria-labelledby="h" />"#,
        r#"<label>Search <input type="image" src="/search.png" /></label>"#,
    ] {
        assert_eq!(in_form(markup), [NO_ALT], "{markup}");
    }
    assert_eq!(
        in_form(r#"<input type="image" src="/search.png" alt="" aria-label="Search" />"#),
        [BLANK_ALT]
    );
}

/// `type` is an `<input>`'s attribute: on another element it makes no image
/// button, and no `alt` is asked of it.
#[test]
fn only_an_input_is_an_image_button() {
    assert_eq!(
        in_form(r#"<textarea type="image" name="n" aria-label="Note"></textarea>"#),
        Vec::<String>::new()
    );
    assert_eq!(
        in_form(r#"<select type="image" name="s" aria-label="Size"><option>S</option></select>"#),
        Vec::<String>::new()
    );
}

/// The image button state alone: an `alt` on another input is not read, and
/// an `<img>` keeps its own rule.
#[test]
fn only_an_input_in_the_image_state_is_held_to_its_alt() {
    assert_eq!(
        in_form(r#"<input type="submit" src="/search.png" alt="" />"#),
        Vec::<String>::new()
    );
    // Control: an `<img>` may have `alt=""`; an image button may not.
    assert_eq!(
        in_form(r#"<img src="/divider.png" alt="" />"#),
        Vec::<String>::new()
    );
}

#[test]
fn each_image_button_is_reported_where_it_is() {
    assert_eq!(
        in_form_at(
            r#"<input type="image" src="/a.png" alt="Go" /><input type="image" src="/b.png" />"#
        ),
        [(
            NO_ALT.to_string(),
            r#"<input type="image" src="/b.png" />"#.to_string()
        )]
    );
}

/// The refusal says what an image button's `alt` is, and the repair.
#[test]
fn an_image_button_is_refused_with_its_own_repair() {
    let mut sources = library();
    sources.push((
        "t.pw".to_string(),
        form(r#"<input type="image" src="/search.png" />"#),
    ));
    let d = check_sources(&sources)
        .into_iter()
        .filter(|(n, _)| n == "t.pw")
        .flat_map(|(_, ds)| ds)
        .next()
        .expect("refused");
    assert_eq!(
        (d.code, d.reason, d.invariant),
        (
            "PW5053",
            "image_button_without_alt",
            pw_core::codes::IMAGE_WITHOUT_ALT.invariant
        )
    );
    assert_eq!(
        d.repairs
            .iter()
            .map(|r| r.description.as_str())
            .collect::<Vec<_>>(),
        ["write `alt=\"…\"` saying what the button does"]
    );
    assert!(
        d.explanation
            .as_deref()
            .is_some_and(|e| e.contains("An image button's `alt` is its label")),
        "{d:#?}"
    );
}

// --- names whatever their case (train-14's review of #82) ----------------
//
// HTML's tokenizer lower-cases a tag's name and an attribute's (HTML
// 13.2.5.8, 13.2.5.33): `<iMg>` is an `<img>`, and `ALT` is its `alt`. Until
// train-14 PW5053 compared both exactly, so `<iMg src=...>` checked clean with
// no `alt`, and `ALT="..."` was refused.

#[test]
fn an_image_and_its_alt_are_read_whatever_their_case() {
    refused(r#"<iMg src="/shop.png" />"#);
    clean(r#"<img src="/shop.png" ALT="The shop's front" />"#);
    clean(r#"<iMG src="/divider.png" Alt="" />"#);
}

#[test]
fn an_image_button_and_its_alt_are_read_whatever_their_case() {
    assert_eq!(
        in_form(r#"<iNPUT type="image" src="/search.png" />"#),
        [NO_ALT]
    );
    assert_eq!(
        in_form(r#"<input TYPE="image" src="/search.png" />"#),
        [NO_ALT]
    );
    assert_eq!(
        in_form(r#"<input type="image" src="/search.png" ALT="Search" />"#),
        Vec::<String>::new()
    );
}
