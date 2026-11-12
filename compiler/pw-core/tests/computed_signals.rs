//! **A value computed from a signal is the browser's** (ADR-0227, ruling
//! 0073-a).
//!
//! The feed's draft says what is left of it, `{280 - String.length(draft)}
//! left`, and its post button is `disabled` while there is no post to send.
//! A host computes neither again: a draft changes as a person types, and no
//! host hears of it. The compiler lifts each into a function (ADR-0226), a
//! component a host runs for the page's first render, and compiles the same
//! function into the page's module, which the browser runs each time the
//! signal changes. Until ADR-0227 one was refused at build: "which the
//! browser computes".
//!
//! Each test states one part, with controls.

use pw_core::check::{Unit, check_sources};
use pw_core::lower::lower_file;
use pw_syntax::parse_tree;

/// The platform's packages, and `files` after them.
fn program(files: &[(&str, String)]) -> Vec<(String, String)> {
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
    out.extend(files.iter().map(|(n, s)| (n.to_string(), s.clone())));
    out
}

fn reported(files: &[(String, String)]) -> Vec<String> {
    check_sources(files)
        .into_iter()
        .filter(|(n, _)| !n.contains("packages/"))
        .flat_map(|(_, ds)| ds.into_iter().map(|d| format!("{} {}", d.code, d.message)))
        .collect()
}

/// The build, or what refused it: a template part the renderer cannot
/// render, or each refusal of a build that completed.
fn build(files: &[(String, String)]) -> Result<pw_core::build::Build, String> {
    let units: Vec<Unit> = files
        .iter()
        .map(|(path, src)| Unit {
            path: path.clone(),
            hir: lower_file(src, &parse_tree(src).green),
            src: src.clone(),
        })
        .collect();
    let b = pw_core::build::build(&units)?;
    match b.refusals() {
        refused if refused.is_empty() => Ok(b),
        refused => Err(refused.join("\n")),
    }
}

fn built(files: &[(String, String)]) -> pw_core::build::Build {
    let result = build(files);
    assert!(
        result.is_ok(),
        "supported computed signal: {:?}",
        result.as_ref().err()
    );
    result.unwrap()
}

fn feed() -> Vec<(String, String)> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let src = std::fs::read_to_string(root.join("examples/feed/app.pw")).expect("the feed");
    program(&[("app.pw", src)])
}

/// A page of a post and its UI state, its markup `markup`.
fn page(markup: &str) -> Vec<(String, String)> {
    program(&[(
        "t.pw",
        format!(
            "module t\n\n\
             type Panel = Panel {{ open: Bool, label: String }}\n\n\
             type Post = Post {{ id: String, likes: Int, pinned: Bool }}\n\n\
             fn shown(open: Bool) -> String !{{}} {{\n    if open {{\n        \"open\"\n    }} else {{\n        \"shut\"\n    }}\n}}\n\n\
             view Opened(open: Bool) !{{}} {{\n    <p>{{shown(open)}}</p>\n}}\n\n\
             public query Thread(id: String) -> Post !{{}} {{\n    \
             Post {{ id: id, likes: 2, pinned: true }}\n}}\n\n\
             page P(id: String) {{\n    route \"/p/{{id}}\"\n    cache private\n\n    \
             signal open: Bool = false\n    \
             signal count: Int = 3\n    \
             signal panel: Panel = Panel {{ open: true, label: \"Menu\" }}\n    \
             let post = query Thread(id)\n\n    \
             view {{\n        <title>P</title>\n        <main>\n            {markup}\n            \
             <button type=\"button\" on:press={{() => open = !open}}>Toggle</button>\n            \
             <button type=\"button\" on:press={{() => count = count + 1}}>More</button>\n        \
             </main>\n    }}\n}}\n"
        ),
    )])
}

fn plan_of<'b>(b: &'b pw_core::build::Build, page: &str) -> &'b pw_core::page_values::PageValues {
    b.pages
        .iter()
        .find(|p| p.page == page)
        .unwrap_or_else(|| panic!("no page {page}"))
        .plan
        .as_ref()
        .unwrap_or_else(|e| panic!("{page}: {e}"))
}

#[test]
fn standalone_and_prepared_signal_modules_are_byte_identical() {
    let files = feed();
    let units: Vec<Unit> = files
        .iter()
        .map(|(path, src)| Unit {
            path: path.clone(),
            hir: lower_file(src, &parse_tree(src).green),
            src: src.clone(),
        })
        .collect();
    let standalone = pw_core::backend::computed::compile(&units).expect("standalone backend");
    let prepared = pw_core::program::build(&units);
    assert!(
        prepared.is_ok(),
        "complete executable program: {:?}",
        prepared.as_ref().err()
    );
    let prepared = prepared.unwrap();
    let modules = |compiled: Vec<pw_core::backend::computed::Compiled>| {
        compiled
            .into_iter()
            .map(|c| (c.page, c.module.expect("a supported signal module")))
            .collect::<Vec<_>>()
    };
    let standalone = modules(standalone);
    assert!(
        !standalone.is_empty(),
        "the feed computes from its draft signal"
    );
    assert_eq!(standalone, modules(prepared.computed));
}

#[test]
fn a_value_computed_from_a_signal_is_the_browsers_and_its_first_the_hosts() {
    let files = feed();
    assert_eq!(reported(&files), Vec::<String>::new());
    let b = built(&files);
    let plan = plan_of(&b, "feed.app.Home~ui");
    // What is left, and the post button: each a part the browser sets from
    // the draft, by the component that computes it.
    let computed: Vec<(&str, &str, &str, &str)> = plan
        .live
        .iter()
        // Set in place; a block whose subject it is, ADR-0229's.
        .filter(|l| !l.derived.is_empty() && !matches!(l.kind.as_str(), "conditional" | "match"))
        .map(|l| {
            (
                l.kind.as_str(),
                l.attribute.as_str(),
                l.path.as_str(),
                l.derived.as_str(),
            )
        })
        .collect();
    let [
        (text_kind, _, text_path, text),
        (flag_kind, flag, flag_path, button),
    ] = computed.as_slice()
    else {
        panic!("two computed parts: {:?}", plan.live);
    };
    assert_eq!((*text_kind, *text_path), ("text", "draft"));
    assert_eq!(
        (*flag_kind, *flag, *flag_path),
        ("boolean_attribute", "disabled", "draft")
    );
    // Not the host's to compute again: no part of its plan runs either. (It
    // computes one value of its own, the composer's `hidden`, from its
    // reader, ADR-0302.)
    for p in plan.parts.iter().chain(&plan.derived) {
        assert!(
            !p.steps.iter().any(|s| matches!(
                s,
                pw_core::page_values::Step::Derived(d) if d.as_str() == *text || d.as_str() == *button
            )),
            "{p:?}"
        );
    }
    // Each a component, for the host's first value: typed by the draft.
    for (id, ret) in [(*text, "s64"), (*button, "bool")] {
        let part = id.rsplit('_').next().expect("a part");
        assert!(
            b.wit
                .contains(&format!("derived{part}: func(arg0: string) -> {ret};")),
            "{id}: {}",
            b.wit
        );
        let contract = b
            .contracts
            .iter()
            .find(|c| c.component_id == *id)
            .unwrap_or_else(|| panic!("no contract {id}"));
        assert!(contract.imports.is_empty(), "{contract:?}");
    }
    // And the page's module, the same functions compiled for the browser,
    // each by its part.
    let module = b
        .computed
        .iter()
        .find(|c| c.page == "feed.app.Home~ui")
        .expect("the home page's module");
    let source = module.module.as_ref().unwrap_or_else(|e| panic!("{e}"));
    for id in [text, button] {
        let part = id.rsplit('_').next().expect("a part");
        assert!(
            source.contains(&format!("  \"{part}\": (j) => f")),
            "{source}"
        );
    }
    // A length is a number, as an `Int` within ±2^53 is (ADR-0433).
    assert!(source.contains("Array.from(v0).length"), "{source}");
    assert!(!source.contains("BigInt(Array.from"), "{source}");
    // The thread page's, since its reply form (ADR-0231): its button's
    // `disabled`, from the reply's draft, alone.
    let thread = plan_of(&b, "feed.app.PostPage~ui");
    let replied: Vec<(&str, &str)> = thread
        .live
        .iter()
        .filter(|l| !l.derived.is_empty())
        .map(|l| (l.attribute.as_str(), l.path.as_str()))
        .collect();
    assert_eq!(replied, [("disabled", "answer")], "{:?}", thread.live);
    // Control: a page that computes nothing from a signal has no module.
    let plain = build(&page("<p>{count}</p>")).unwrap_or_else(|e| panic!("{e}"));
    assert!(plain.computed.is_empty(), "{:?}", plain.computed);
    // A module the browser's backend could not write is a refusal: the
    // value would stay at its first.
    let mut b = b;
    b.computed.push(pw_core::backend::computed::Compiled {
        page: "feed.app.Elsewhere".to_string(),
        module: Err("no such function".to_string()),
    });
    assert_eq!(
        b.refusals(),
        ["`feed.app.Elsewhere`'s computed values: no such function"]
    );
}

#[test]
fn an_attribute_and_a_views_field_of_a_signal_are_computed_as_well() {
    let files = page("<p title={shown(open)}>x</p>\n            <Opened open={panel.open} />");
    assert_eq!(reported(&files), Vec::<String>::new());
    let b = build(&files).unwrap_or_else(|e| panic!("{e}"));
    let plan = plan_of(&b, "t.P~ui");
    let computed: Vec<(&str, &str, &str, &str)> = plan
        .live
        .iter()
        // Set in place; a block whose subject it is, ADR-0229's.
        .filter(|l| !l.derived.is_empty() && !matches!(l.kind.as_str(), "conditional" | "match"))
        .map(|l| {
            (
                l.kind.as_str(),
                l.attribute.as_str(),
                l.signal.as_str(),
                l.path.as_str(),
            )
        })
        .collect();
    // An attribute set in place, and a view's parameter given a field of
    // the signal: read at that path, the host's first value from the same.
    assert_eq!(
        computed,
        [
            ("attribute", "title", "open", "open"),
            ("text", "", "panel", "panel.open")
        ]
    );
    let source = b
        .computed
        .iter()
        .find(|c| c.page == "t.P~ui")
        .and_then(|c| c.module.as_ref().ok())
        .expect("the page's module");
    assert_eq!(source.matches("(j) => f").count(), 2, "{source}");
    // An `Int` is held in its wire form, which is the function's: it is
    // read as it is (ADR-0433).
    let b = build(&page("<p>{count * 2}</p>")).unwrap_or_else(|e| panic!("{e}"));
    let source = b
        .computed
        .iter()
        .find(|c| c.page == "t.P~ui")
        .and_then(|c| c.module.as_ref().ok())
        .expect("the page's module");
    assert!(source.contains("(j) => f0(j)"), "{source}");
    // Control: the same values read as they are compute nothing.
    let b = build(&page("<p title={panel.label}>{open}</p>")).unwrap_or_else(|e| panic!("{e}"));
    assert!(
        plan_of(&b, "t.P~ui")
            .live
            .iter()
            .all(|l| l.derived.is_empty())
    );
    assert!(b.computed.is_empty(), "{:?}", b.computed);
}

#[test]
fn what_the_browser_does_not_compute_yet_is_refused_by_name() {
    for (markup, why) in [
        (
            "<p>{shown(open & post.pinned)}</p>",
            "computes a value from `open` and `post`, and a host computes one from one value \
             (ruling 0073-a)",
        ),
        // Inside a block a host renders, which the browser does not render
        // again (track `device-store`: inside one the browser renders, it
        // computes it, below).
        (
            "{#if post.pinned}<p>{shown(open)}</p>{/if}",
            "computes a value from the signal `open` inside a block a value other than a signal \
             decides, which the browser does not render again (ruling 0073-a)",
        ),
    ] {
        let files = page(markup);
        assert_eq!(reported(&files), Vec::<String>::new(), "{markup}");
        let result = build(&files);
        assert!(result.is_err(), "{markup} built");
        let e = result.err().unwrap();
        assert!(e.contains(why), "{markup}: {e}");
    }
    // Control: at the top of the page, it builds; and inside a block a signal
    // decides, which the browser renders again, computing it first (track
    // `device-store`, ruling 0073-a's later steps).
    build(&page("<p>{shown(open)}</p>")).unwrap_or_else(|e| panic!("{e}"));
    let b =
        build(&page("{#if open}<p>{shown(panel.open)}</p>{/if}")).unwrap_or_else(|e| panic!("{e}"));
    assert!(
        plan_of(&b, "t.P~ui")
            .live
            .iter()
            .any(|l| l.kind == "value" && !l.derived.is_empty()),
        "{:?}",
        plan_of(&b, "t.P~ui").live
    );
}
