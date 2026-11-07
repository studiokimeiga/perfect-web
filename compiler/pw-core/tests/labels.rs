//! **What names a form control** (ADR-0143).
//!
//! PW5014 refuses a control nothing names. Until 2026-10-02 it took any `id`
//! as a name, on the assumption that a `<label for>` pointed at it, and
//! nothing checked that one did: T06's unsafe store, a field with an `id`
//! and a placeholder and no label, passed `pw check`. It also refused a
//! control wrapped in its `<label>`, which the HTML standard defines as
//! labelling it.
//!
//! The rule now follows the HTML standard's labeled control and accname 1.2,
//! within the declaration that renders the control. Each test states one
//! part, with controls.

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

/// Every diagnostic, as `code message | each related note`.
fn reported(src: &str) -> Vec<String> {
    let mut sources = library();
    sources.push(("t.pw".to_string(), src.to_string()));
    check_sources(&sources)
        .into_iter()
        .filter(|(n, _)| n == "t.pw")
        .flat_map(|(_, ds)| {
            ds.into_iter().map(|d| {
                let notes: Vec<String> = d.related.iter().map(|r| r.label.clone()).collect();
                format!("{} {} | {}", d.code, d.message, notes.join(" | "))
            })
        })
        .collect()
}

/// A view, given `caption` and `names`, whose form holds `markup`.
fn view(markup: &str) -> String {
    format!(
        "module t\n\nview V(caption: String, names: List<String>) !{{}} {{\n    <form>\n        \
         {markup}\n        <button type=\"submit\">Go</button>\n    </form>\n}}\n"
    )
}

/// The diagnostics `markup` gets, which must be PW5014's or none.
fn labels(markup: &str) -> Vec<String> {
    let got = reported(&view(markup));
    assert!(
        got.iter().all(|d| d.starts_with("PW5014 ")),
        "only PW5014 is in question here, for {markup}:\n{got:#?}"
    );
    got
}

fn clean(markup: &str) {
    assert_eq!(labels(markup), Vec::<String>::new(), "{markup} is named");
}

/// Refused, with a note that says `why`.
fn refused(markup: &str, why: &str) {
    let got = labels(markup);
    assert_eq!(got.len(), 1, "one unnamed control in {markup}: {got:#?}");
    assert!(
        got[0].contains(why),
        "{markup} is refused, and the note says {why:?}: {got:#?}"
    );
}

#[test]
fn an_id_names_nothing_by_itself() {
    // T06's unsafe store, reduced: before ADR-0143 this passed.
    refused(
        r#"<input id="q" type="text" name="q" placeholder="Query" />"#,
        "no `<label for=\"q\">` in `V` names this `id`",
    );
    // The other `id` is there: a `for` that names nothing is PW5032's since
    // ADR-0185.
    refused(
        r#"<label for="other">Query</label><p id="other">Other</p><input id="q" type="text" name="q" />"#,
        "no `<label for=\"q\">` in `V` names this `id`",
    );
    // Controls: a label for it, before or after it.
    clean(r#"<label for="q">Query</label><input id="q" type="text" name="q" />"#);
    clean(r#"<input id="q" type="text" name="q" /><label for="q">Query</label>"#);
    clean(r#"<label for="s">Size</label><select id="s" name="s"><option>S</option></select>"#);
}

#[test]
fn a_label_names_its_control_only_when_it_has_text() {
    refused(
        r#"<label for="q"></label><input id="q" type="text" name="q" />"#,
        "this `<label for=\"q\">` has no text",
    );
    refused(
        r#"<label for="q">   </label><input id="q" type="text" name="q" />"#,
        "has no text",
    );
    // Hidden text is not read (accname 1.2, step 2A).
    refused(
        r#"<label for="q"><span aria-hidden="true">*</span></label><input id="q" type="text" name="q" />"#,
        "has no text",
    );
    // Controls: text in an element, an interpolation, an image's `alt`.
    clean(r#"<label for="q"><span>Query</span></label><input id="q" type="text" name="q" />"#);
    clean(r#"<label for="q">{caption}</label><input id="q" type="text" name="q" />"#);
    clean(
        r#"<label for="q"><img src="/q.png" alt="Query" /></label><input id="q" type="text" name="q" />"#,
    );
}

#[test]
fn a_label_that_wraps_its_control_names_it() {
    // Refused before ADR-0143: the rule read only the control's attributes.
    clean(r#"<label>Query <input type="text" name="q" /></label>"#);
    clean(r#"<label>Size <select name="s"><option>S</option></select></label>"#);
    clean(r#"<label>Note <textarea name="n"></textarea></label>"#);
    clean(r#"<label><span>Query</span> <input type="text" name="q" /></label>"#);
    // A hidden input is not labelable, so the one after it is the first.
    clean(
        r#"<label>Query <input type="hidden" name="t" value="1" /><input type="text" name="q" /></label>"#,
    );
    // Controls: no text, and its own options are not its label.
    refused(
        r#"<label><input type="text" name="q" /></label>"#,
        "this `<label>` has no text",
    );
    refused(
        r#"<label><select name="s"><option>S</option></select></label>"#,
        "this `<label>` has no text",
    );
}

#[test]
fn a_wrapping_label_names_only_its_first_control() {
    refused(
        r#"<label>Go <button type="button">x</button> <input type="text" name="q" /></label>"#,
        "names the first control inside it, which is another",
    );
    // With `for`, a label names that element, not what it wraps.
    refused(
        r#"<label for="other">Query <input type="text" name="q" /></label><input id="other" type="text" name="o" />"#,
        "has a `for`, so it names that element, not what it wraps",
    );
}

#[test]
fn which_control_a_label_names_must_not_depend_on_what_renders() {
    // In a block, the first control is whichever renders.
    refused(
        r#"<label>Query {#if caption == ""}<input type="text" name="a" />{/if}</label>"#,
        "depends on what renders inside it",
    );
    // A composed view's markup is not this declaration's to read.
    let src = "module t\n\nview Star() !{} {\n    <span>*</span>\n}\n\n\
               view V() !{} {\n    <form>\n        <label>Query <Star /> <input type=\"text\" \
               name=\"q\" /></label>\n        <button type=\"submit\">Go</button>\n    </form>\n}\n";
    let got = reported(src);
    assert_eq!(got.len(), 1, "{got:#?}");
    assert!(
        got[0].contains("depends on what renders inside it"),
        "{got:#?}"
    );
    // Control: a block that only holds text.
    clean(
        r#"<label>{#if caption == ""}Query{:else}{caption}{/if} <input type="text" name="q" /></label>"#,
    );
}

#[test]
fn aria_labelledby_must_reach_an_element_with_text() {
    // Since ADR-0185 a reference to nothing is PW5032's, the defect, and the
    // field it leaves unnamed is not reported again.
    assert_eq!(
        reported(&view(
            r#"<input type="text" name="q" aria-labelledby="nowhere" />"#
        )),
        [
            "PW5032 `aria-labelledby` names `nowhere`, which no element of `V` has | `V` renders this"
        ]
    );
    refused(
        r#"<p id="h"></p><input type="text" name="q" aria-labelledby="h" />"#,
        "no element in `V` with text has one of these ids",
    );
    refused(
        r#"<input type="text" name="q" aria-labelledby={caption} />"#,
        "these ids are computed",
    );
    // Controls: one IDREF that reaches text is enough (accname 1.2, 2B).
    clean(r#"<h2 id="h">Query</h2><input type="text" name="q" aria-labelledby="h" />"#);
    // The field is named by `h`; the id that names nothing is a reference to
    // nothing all the same, PW5032's since ADR-0185.
    assert_eq!(
        reported(&view(
            r#"<h2 id="h">Query</h2><input type="text" name="q" aria-labelledby="nowhere h" />"#
        )),
        [
            "PW5032 `aria-labelledby` names `nowhere`, which no element of `V` has | `V` renders this"
        ]
    );
    clean(r#"<p id="h" aria-label="Query"></p><input type="text" name="q" aria-labelledby="h" />"#);
}

#[test]
fn a_blank_aria_label_names_nothing() {
    // accname 1.2, step 2C: an empty or blank `aria-label` is skipped.
    refused(
        r#"<input type="text" name="q" aria-label="" />"#,
        "this `aria-label` is blank",
    );
    refused(
        r#"<input type="text" name="q" aria-label="  " />"#,
        "this `aria-label` is blank",
    );
    refused(
        r#"<input type="text" name="q" aria-label />"#,
        "this `aria-label` is blank",
    );
    // Controls: text, and a name the program computes.
    clean(r#"<input type="text" name="q" aria-label="Query" />"#);
    clean(r#"<input type="text" name="q" aria-label={caption} />"#);
}

#[test]
fn a_label_names_the_first_element_with_its_id() {
    // HTML: `for` names the first element in tree order with that `id`. Since
    // ADR-0185 the `id` two elements have is the defect, PW5031's, and the
    // field the label misses is not reported again.
    assert_eq!(
        reported(&view(
            r#"<p id="q">Note</p><label for="q">Query</label><input id="q" type="text" name="q" />"#
        )),
        ["PW5031 `q` names two elements of `V` | `q` is this element's too"]
    );
    // The control first: the label names it, and only the `id` is the
    // defect. Until 2026-10-11 nothing held the label to the first: a label
    // naming the last element with its `id` passed every test.
    assert_eq!(
        reported(&view(
            r#"<label for="q">Query</label><input id="q" type="text" name="q" /><p id="q">Note</p>"#
        )),
        ["PW5031 `q` names two elements of `V` | `q` is this element's too"]
    );
    // A computed `id` cannot be matched to a `for`.
    refused(
        r#"<label for="q">Query</label><input id={caption} type="text" name="q" />"#,
        "this `id` is computed",
    );
}

#[test]
fn an_id_inside_each_names_no_one_row() {
    // Every row has the `id`, so a `for` names only the first row's control.
    // Since ADR-0185 the row's `id` is the defect, PW5031's, and the field is
    // not reported again.
    assert_eq!(
        reported(&view(
            r#"{#each names as n (n)}<label for="qty">{n}</label><input id="qty" type="text" name="qty" />{/each}"#
        )),
        ["PW5031 `qty` names every row of a list in `V` | `V` renders this"]
    );
    // Control: each row's label wraps its own control.
    clean(r#"{#each names as n (n)}<label>{n} <input type="text" name="qty" /></label>{/each}"#);
}

#[test]
fn a_reference_to_an_id_every_row_has_is_its_ids_defect_alone() {
    // `aria-labelledby` reaches the first row's element, as HTML's reference
    // does, and that every row has `t` is PW5031's. Until 2026-10-11 such an
    // `id` named no element, and the control was reported too (PW5014): the
    // one defect twice, where ADR-0185 reports it once.
    assert_eq!(
        reported(&view(
            r#"<input type="text" name="q" aria-labelledby="t" />{#each names as n (n)}<span id="t">{n}</span>{/each}"#
        )),
        ["PW5031 `t` names every row of a list in `V` | `V` renders this"]
    );
}

#[test]
fn a_hidden_input_is_no_control() {
    clean(r#"<input type="hidden" name="t" value="1" />"#);
    clean(r#"<input type="HIDDEN" name="t" value="1" />"#);
}

// --- what names an input button (the input-button ADR) ----------------------
//
// HTML gives each button state its own label (4.10.5.1.18 to .21), and
// HTML-AAM's name for it follows (4.1.2, 4.1.3). Until 2026-11-06 PW5014
// passed `submit` and `button` inputs whatever they held and refused `reset`
// and `image` ones, which HTML names, for having no `<label>`.

/// The one diagnostic `markup` gets, whole: its message and every note.
fn refused_as(markup: &str, expected: &str) {
    assert_eq!(labels(markup), [expected], "{markup}");
}

const A_FIELD: &str = "PW5014 `<input>` has no associated label | `V` renders this control";

/// A `submit` or `reset` input with no `value` shows the label HTML gives
/// it: "an implementation-defined string that means "Reset" or some such".
#[test]
fn a_reset_or_submit_button_with_no_value_has_the_label_html_gives_it() {
    // Refused until 2026-11-06, for having no `<label>`.
    clean(r#"<input type="reset" />"#);
    clean(r#"<input type="RESET" />"#);
    clean(r#"<input type="submit" />"#);
    // Control: a text field has no label of its own, and is refused as one.
    refused_as(r#"<input type="text" name="q" />"#, A_FIELD);
}

#[test]
fn a_button_is_named_by_its_value() {
    clean(r#"<input type="reset" value="Clear the search" />"#);
    clean(r#"<input type="submit" value="Send" />"#);
    clean(r#"<input type="button" value="Send" />"#);
    clean(r#"<input type="Button" value="Send" />"#);
    // A value the program computes is taken as a name, as an `aria-label`'s
    // is (ADR-0143).
    clean(r#"<input type="button" value={caption} />"#);
    clean(r#"<input type="reset" value={caption} />"#);
}

/// An empty `value` is the button's label all the same, so the button shows
/// none, and HTML-AAM gives the default only where `value` is absent.
#[test]
fn a_button_whose_value_is_blank_has_no_label() {
    for (markup, typ) in [
        (r#"<input type="reset" value="" />"#, "reset"),
        (r#"<input type="submit" value="  " />"#, "submit"),
        (r#"<input type="SUBMIT" value />"#, "submit"),
        (r#"<input type="button" value="" />"#, "button"),
    ] {
        refused_as(
            markup,
            &format!(
                "PW5014 `<input type=\"{typ}\">` has no label | `V` renders this control | \
                 this `value` is the button's label, and it is blank"
            ),
        );
    }
}

/// HTML: a `button` input's label "must be the value of that attribute;
/// otherwise, it must be the empty string". It has no default.
#[test]
fn a_button_input_with_no_value_has_no_label() {
    let none = "PW5014 `<input type=\"button\">` has no label | `V` renders this control | \
                an `<input type=\"button\">` is named by its `value`, and this one has none";
    refused_as(r#"<input type="button" />"#, none);
    // Its `id` is not why: no `<label for>` would be the repair a field takes.
    refused_as(r#"<input type="button" id="b" />"#, none);
}

#[test]
fn a_button_with_no_label_of_its_own_is_named_as_any_control_is() {
    // An icon button: the image is the page's, the name an `aria-label`.
    clean(r#"<input type="submit" value="" aria-label="Search" />"#);
    clean(r#"<input type="button" aria-label="Close" />"#);
    clean(r#"<h2 id="h">Close</h2><input type="button" aria-labelledby="h" />"#);
    // HTML-AAM 4.1.2, step 2: the label it is associated with.
    clean(r#"<label>Close <input type="button" /></label>"#);
    clean(r#"<label for="b">Close</label><input type="button" id="b" />"#);
    // Control: a blank `aria-label` names nothing here either.
    refused(
        r#"<input type="button" aria-label="" />"#,
        "this `aria-label` is blank",
    );
}

/// The refusal says how a button is named: by its `value` first.
#[test]
fn a_button_is_refused_with_the_repair_a_button_takes() {
    let src = view(r#"<input type="button" />"#);
    let mut sources = library();
    sources.push(("t.pw".to_string(), src));
    let diags: Vec<_> = check_sources(&sources)
        .into_iter()
        .filter(|(n, _)| n == "t.pw")
        .flat_map(|(_, ds)| ds)
        .collect();
    assert_eq!(diags.len(), 1, "{diags:#?}");
    let d = &diags[0];
    assert_eq!((d.code, d.reason), ("PW5014", "button_without_label"));
    assert_eq!(d.message, "`<input type=\"button\">` has no label");
    assert_eq!(
        d.repairs.first().map(|r| r.description.as_str()),
        Some("write `value=\"…\"` saying what the button does")
    );
    assert!(
        d.explanation
            .as_deref()
            .is_some_and(|e| e.contains("is announced as \"button\"")),
        "{d:#?}"
    );
    // Control: a field is refused with a field's repair.
    let src = view(r#"<input type="text" name="q" />"#);
    let mut sources = library();
    sources.push(("t.pw".to_string(), src));
    let d = check_sources(&sources)
        .into_iter()
        .filter(|(n, _)| n == "t.pw")
        .flat_map(|(_, ds)| ds)
        .next()
        .expect("refused");
    assert_eq!((d.code, d.reason), ("PW5014", "control_without_label"));
    assert!(
        d.repairs[0]
            .description
            .starts_with("associate a `<label for=...>`"),
        "{d:#?}"
    );
}

/// An image button's label is its `alt` (HTML 4.10.5.1.19), which HTML
/// requires and does not let be empty: PW5053's to hold (`images.rs`), and
/// not reported a second time here.
#[test]
fn an_image_button_is_named_by_its_alt_and_reported_once() {
    // Refused until 2026-11-06, for having no `<label>`.
    clean(r#"<input type="image" src="/search.png" alt="Search" />"#);
    clean(r#"<input type="IMAGE" src="/search.png" alt="Search" />"#);
    for markup in [
        r#"<input type="image" src="/search.png" />"#,
        r#"<input type="image" src="/search.png" alt="" aria-label="Search" />"#,
    ] {
        let got = reported(&view(markup));
        assert_eq!(got.len(), 1, "{markup}: {got:#?}");
        assert!(got[0].starts_with("PW5053 "), "{markup}: {got:#?}");
    }
}

/// `type` is an `<input>`'s attribute: on a `<select>` or a `<textarea>` it
/// makes no button and hides nothing.
#[test]
fn only_an_input_has_a_type() {
    for (markup, tag) in [
        (r#"<textarea type="image" name="n"></textarea>"#, "textarea"),
        (r#"<textarea type="reset" name="n"></textarea>"#, "textarea"),
        (
            r#"<select type="hidden" name="s"><option>S</option></select>"#,
            "select",
        ),
    ] {
        refused_as(
            markup,
            &format!("PW5014 `<{tag}>` has no associated label | `V` renders this control"),
        );
    }
}
