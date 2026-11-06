//! **A control is shown where its command's predicates hold** (ADR-0307).
//!
//! The owner's finding of 2026-10-08: signed out, a reader pressed the
//! feed's Post and the server refused it in silence. A page asks a predicate
//! of its reader by naming it, `{#if SignedIn}`, and the deployment answers
//! it as it answers `requires`; a control whose command requires one is
//! shown only where the page asks it and it holds (PW5048). A name a
//! template reads is a value (PW0629), and a page that asks its reader
//! anything is its reader's (PW5049). Each test states one case, with its
//! control.

use pw_core::check::check_sources;

fn program(src: &[(&str, &str)]) -> Vec<(String, String)> {
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
    for (name, text) in src {
        out.push((name.to_string(), text.to_string()));
    }
    out
}

/// Each diagnostic reported in the test's own files, as `CODE message`.
fn reported(src: &str) -> Vec<String> {
    check_sources(&program(&[("t.pw", src)]))
        .into_iter()
        .filter(|(n, _)| n == "t.pw")
        .flat_map(|(_, ds)| ds.into_iter().map(|d| format!("{} {}", d.code, d.message)))
        .collect()
}

fn codes(found: &[String], code: &str) -> usize {
    found.iter().filter(|d| d.starts_with(code)).count()
}

/// Module `t`: a predicate, a command that requires it, and a page whose
/// view is `markup`, of whose visibility `visibility` says.
fn page(visibility: &str, markup: &str) -> String {
    format!(
        "module t\n\nimport context.{{ current_session }}\n\n\
         opaque type InteractionId = String\n\n\
         predicate SignedIn\n    says \"Sign in to do this.\"\n\n\
         predicate OwnsNote(note: Int)\n    says \"Only its author can do this.\"\n\n\
         command post_note(text: String) -> Result<Int, String>\n    \
         requires      SignedIn\n    idempotent_by InteractionId\n{{\n    Ok(1)\n}}\n\n\
         command plain_note(text: String) -> Result<Int, String>\n    \
         idempotent_by InteractionId\n{{\n    Ok(1)\n}}\n\n\
         {visibility}page P() {{\n    route \"/p\"\n{cache}\n    view {{\n        <title>P</title>\n        \
         <main>\n            {markup}\n        </main>\n    }}\n}}\n",
        cache = if visibility.is_empty() {
            ""
        } else {
            "    cache private\n"
        },
    )
}

const POST: &str = "<button type=\"button\" on:press={() => match post_note(\"a\") { Ok(_) => (), Err(_) => () }}>Post</button>";
const PLAIN: &str = "<button type=\"button\" on:press={() => match plain_note(\"a\") { Ok(_) => (), Err(_) => () }}>Note</button>";

#[test]
fn a_control_under_its_predicate_is_shown_where_it_holds() {
    for markup in [
        format!("{{#if SignedIn}}{POST}{{/if}}"),
        format!("{{#if SignedIn & true}}{POST}{{/if}}"),
        format!("{{#if SignedIn}}{{#if true}}{POST}{{/if}}{{/if}}"),
        format!("<form hidden={{!SignedIn}}>{POST}</form>"),
        format!("<div hidden={{false | !SignedIn}}>{POST}</div>"),
    ] {
        let found = reported(&page("session ", &markup));
        assert_eq!(codes(&found, "PW5048"), 0, "{markup}: {found:#?}");
    }
}

#[test]
fn a_control_where_its_predicate_is_not_asked_is_refused() {
    for markup in [
        POST.to_string(),
        format!("{{#if true}}{POST}{{/if}}"),
        // An `{:else}` is where the condition is false.
        format!("{{#if SignedIn}}<p>in</p>{{:else}}{POST}{{/if}}"),
        // `hidden={SignedIn}` hides it where it holds.
        format!("<form hidden={{SignedIn}}>{POST}</form>"),
        // A disjunction asserts neither side.
        format!("{{#if SignedIn | true}}{POST}{{/if}}"),
    ] {
        let found = reported(&page("session ", &markup));
        assert_eq!(codes(&found, "PW5048"), 1, "{markup}: {found:#?}");
    }
    // The control: a command that requires nothing needs no condition.
    let found = reported(&page("session ", PLAIN));
    assert_eq!(codes(&found, "PW5048"), 0, "{found:#?}");
}

#[test]
fn a_name_that_holds_no_value_is_refused_where_a_template_reads_it() {
    // A type, a page and a predicate with parameters: until 2026-10-10 each
    // built, and the page failed where it was served.
    for (name, what) in [
        ("InteractionId", "a type"),
        ("P", "a page"),
        ("OwnsNote", "a predicate with parameters"),
    ] {
        let found = reported(&page("session ", &format!("{{#if {name}}}<p>x</p>{{/if}}")));
        assert!(
            found
                .iter()
                .any(|d| d.starts_with("PW0629") && d.contains(what)),
            "{name}: {found:#?}"
        );
    }
    // The control: a predicate with no parameters is the reader's answer.
    let found = reported(&page("session ", "{#if SignedIn}<p>x</p>{/if}"));
    assert_eq!(codes(&found, "PW0629"), 0, "{found:#?}");
}

#[test]
fn a_handler_asks_no_predicate() {
    // A handler runs in the browser after the document: the page asks its
    // reader when it is rendered.
    let markup =
        "<button type=\"button\" on:press={() => if SignedIn { () } else { () }}>Ask</button>";
    let found = reported(&page("session ", markup));
    assert!(
        found
            .iter()
            .any(|d| d.starts_with("PW0629") && d.contains("a handler asks")),
        "{found:#?}"
    );
}

#[test]
fn a_page_served_to_everyone_asks_its_reader_nothing() {
    let markup = "{#if SignedIn}<p>x</p>{/if}";
    let found = reported(&page("", markup));
    assert_eq!(codes(&found, "PW5049"), 1, "{found:#?}");
    let found = reported(&page("session ", markup));
    assert_eq!(codes(&found, "PW5049"), 0, "{found:#?}");
}

#[test]
fn a_refusable_control_is_shown_to_every_reader() {
    // A part served to everyone cannot be decided by its reader: such a
    // control is marked, shown to each, and its refusal told (ADR-0302).
    let refusable = POST.replace("on:press=", "on:press|refusable=");
    let found = reported(&page("session ", &refusable));
    assert_eq!(codes(&found, "PW5048"), 0, "{found:#?}");
    // The control: unmarked, it is held to the rule.
    let found = reported(&page("session ", POST));
    assert_eq!(codes(&found, "PW5048"), 1, "{found:#?}");
}

/// Whether `text` holds a date written `YYYY-MM-DD`.
fn dated(text: &str) -> bool {
    let shape = |w: &[u8]| {
        w.iter().enumerate().all(|(i, b)| {
            if i == 4 || i == 7 {
                *b == b'-'
            } else {
                b.is_ascii_digit()
            }
        })
    };
    text.as_bytes().windows(10).any(shape)
}

/// **PW5048's note gives its reader the rule and its reason**, as `pw check`
/// prints it and the guide shows it, and tells no story of the project's.
/// Until 2026-11-06 it opened with one, dated: the owner, signed out,
/// pressing Post (2026-10-08).
#[test]
fn a_refused_controls_note_says_the_rule_and_why() {
    let notes: Vec<String> = check_sources(&program(&[("t.pw", &page("session ", POST))]))
        .into_iter()
        .filter(|(n, _)| n == "t.pw")
        .flat_map(|(_, ds)| ds)
        .filter(|d| d.code == "PW5048")
        .filter_map(|d| d.explanation)
        .collect();
    assert_eq!(notes.len(), 1, "{notes:#?}");
    let note = &notes[0];
    assert!(
        note.starts_with("A control the server refuses is a dead control"),
        "the reason first: {note}"
    );
    assert!(note.contains("(ADR-0302)"), "{note}");
    assert!(
        note.contains("(`predicate SignedIn says \"…\"`)"),
        "how the program declares what it asks: {note}"
    );
    assert!(
        !dated(note) && !note.contains("the owner"),
        "no story of the project's: {note}"
    );
    // The control: the shape `dated` reads.
    assert!(dated("pressed Post (2026-10-08).") && !dated("ADR-0302"));
}

#[test]
fn a_predicates_answer_is_a_bool() {
    // Compared with a number, it is refused as a `Bool` is (PW0609), where a
    // name of no type would pass undecided.
    let found = reported(&page("session ", "{#if SignedIn == 1}<p>x</p>{/if}"));
    assert!(found.iter().any(|d| d.starts_with("PW0609")), "{found:#?}");
    // The control: a `Bool` it is compared with agrees.
    let found = reported(&page("session ", "{#if SignedIn == true}<p>x</p>{/if}"));
    assert_eq!(codes(&found, "PW0609"), 0, "{found:#?}");
}

/// Module `t`'s page, built: its plan.
fn planned(markup: &str) -> pw_core::page_values::PageValues {
    let units: Vec<pw_core::check::Unit> = program(&[("t.pw", &page("session ", markup))])
        .into_iter()
        .map(|(path, src)| pw_core::check::Unit {
            hir: pw_core::lower::lower_file(&src, &pw_syntax::parse_tree(&src).green),
            path,
            src,
        })
        .collect();
    let b = pw_core::build::build(&units).expect("builds");
    assert!(b.refusals().is_empty(), "{:#?}", b.refusals());
    b.pages
        .iter()
        .find(|p| p.page == "t.P~ui")
        .and_then(|p| p.plan.as_ref().ok())
        .cloned()
        .expect("the page's plan")
}

#[test]
fn the_plan_names_what_the_page_asks_and_a_host_computes_from_the_answer() {
    // A condition, a text part, and an attribute computed from the answer:
    // the plan names `SignedIn`, and the attribute is the host's to compute
    // from it, read by the name no source writes.
    let plan = planned(&format!(
        "{{#if SignedIn}}<p>in</p>{{/if}}<p>{{SignedIn}}</p><form hidden={{!SignedIn}}>{POST}</form>"
    ));
    assert_eq!(plan.predicates, ["SignedIn"]);
    let derived = serde_json::to_value(&plan).expect("JSON")["derived"].clone();
    assert!(
        derived
            .as_array()
            .into_iter()
            .flatten()
            .any(|d| d["binding"] == "SignedIn~holds"),
        "{derived:#?}"
    );
    // The control: a page that asks nothing names nothing.
    let plan = planned(PLAIN);
    assert!(plan.predicates.is_empty(), "{:?}", plan.predicates);
}
