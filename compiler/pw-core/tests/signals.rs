//! **A page's own UI state** (ADR-0130's first slice, ADR-0133).
//!
//! `signal x: T = e` in a page's body: written by a handler, read where the
//! browser reads it again, held and rendered again by the browser. Before
//! 2026-10-02 there was no `signal`: a handler that only changed the page was
//! refused (ADR-0058, "a handler that calls no command"), and a page could not
//! open a panel.
//!
//! Each test states one part of the slice, with controls.

use pw_core::check::{Unit, check_sources};
use pw_core::lower::lower_file;
use pw_syntax::parse_tree;

fn library() -> Vec<(String, String)> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut out = Vec::new();
    for d in [
        "packages/pw-std",
        "packages/pw-platform-web",
        "examples/lib",
    ] {
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
    out.push((
        "domain.pw".to_string(),
        std::fs::read_to_string(root.join("examples/domain.pw")).expect("domain.pw"),
    ));
    out
}

fn reported(src: &str) -> Vec<String> {
    let mut sources = library();
    sources.push(("t.pw".to_string(), src.to_string()));
    check_sources(&sources)
        .into_iter()
        .filter(|(n, _)| n == "t.pw")
        .flat_map(|(_, ds)| ds.into_iter().map(|d| format!("{} {}", d.code, d.message)))
        .collect()
}

fn codes(src: &str) -> Vec<String> {
    reported(src)
        .into_iter()
        .map(|d| d.split(' ').next().unwrap_or_default().to_string())
        .collect()
}

fn build(src: &str) -> pw_core::build::Build {
    let mut sources = library();
    sources.push(("t.pw".to_string(), src.to_string()));
    let units: Vec<Unit> = sources
        .into_iter()
        .map(|(path, src)| Unit {
            path,
            hir: lower_file(&src, &parse_tree(&src).green),
            src,
        })
        .collect();
    pw_core::build::build(&units).expect("the program builds")
}

/// A page with `signals` declared and `body` after them, whose view holds
/// `view`.
fn page(signals: &str, body: &str, view: &str) -> String {
    format!(
        "module t\n\ntype Panel = Shut | Cart | Item(Int)\n\npage P() {{\n    cache private\n\n\
         {signals}\n{body}\n\n    view {{\n        <main>\n{view}\n        </main>\n    }}\n}}\n"
    )
}

const SIGNALS: &str = "    signal panel: Panel = Panel.Shut\n    signal count: Int = 0\n    \
                       signal label: String = \"menu\"";

#[test]
fn a_signal_is_written_by_a_handler_and_read_where_the_browser_reads_again() {
    let press = "            <button type=\"button\" on:press={resumable() => count = count + 1}>Add</button>\n\
                 \x20           <p>{count}</p>";
    // Control: written by a handler, read by a template part.
    assert_eq!(codes(&page(SIGNALS, "", press)), Vec::<String>::new());

    // Written in the body: on the server, before anyone pressed anything.
    let found = reported(&page(SIGNALS, "    count = 1", press));
    assert!(
        found
            .iter()
            .any(|d| d.starts_with("PW5300") && d.contains("`count`")),
        "{found:?}"
    );

    // Read in the body: once, where it never changes.
    let found = reported(&page(SIGNALS, "    let shown = label", press));
    assert!(
        found
            .iter()
            .any(|d| d.starts_with("PW5301") && d.contains("`label`")),
        "{found:?}"
    );
}

#[test]
fn a_handler_changes_a_signal_and_not_a_binding_of_its_body() {
    let view =
        "            <button type=\"button\" on:press={resumable() => n = n + 1}>Add</button>";
    let found = reported(&page(SIGNALS, "    let mut n = 0", view));
    assert!(
        found
            .iter()
            .any(|d| d.starts_with("PW5302") && d.contains("`n`")),
        "{found:?}"
    );

    // Control: a binding the handler declares itself is its own.
    let own = "            <button type=\"button\" on:press={resumable() => {\n\
               \x20               let mut n = 0\n                n = n + 1\n                \
               count = n\n            }}>Add</button>";
    assert_eq!(codes(&page(SIGNALS, "", own)), Vec::<String>::new());
}

#[test]
fn the_plan_holds_each_signals_first_value_and_the_parts_it_decides() {
    let view = "            <p>{label}</p>\n            {#match panel}\n                {:Shut}\n\
                \x20               {:Cart}\n                    <aside>{count}</aside>\n\
                \x20               {:Item(n)}\n                    <aside>{n}</aside>\n\
                \x20           {/match}";
    let b = build(&page(SIGNALS, "", view));
    let plan = b
        .pages
        .iter()
        .find(|p| p.page == "t.P~ui")
        .and_then(|p| p.plan.as_ref().ok())
        .expect("a plan");
    let first: Vec<(String, serde_json::Value)> = plan
        .signals
        .iter()
        .map(|s| (s.name.clone(), s.initial.clone()))
        .collect();
    assert_eq!(
        first,
        [
            ("panel".to_string(), serde_json::json!({ "$case": "shut" })),
            ("count".to_string(), serde_json::json!(0)),
            ("label".to_string(), serde_json::json!("menu")),
        ]
    );
    let live: Vec<(String, String, Vec<String>)> = plan
        .live
        .iter()
        .map(|l| (l.kind.clone(), l.signal.clone(), l.reads.clone()))
        .collect();
    // The block is rendered again for `panel` alone, and `{count}` inside
    // it is set in place (ADR-0142): until 2026-10-02 the block was rendered
    // again for every signal read anywhere in it.
    assert_eq!(
        live,
        [
            ("text".to_string(), "label".to_string(), vec![]),
            (
                "match".to_string(),
                "panel".to_string(),
                vec!["panel".to_string()]
            ),
            ("text".to_string(), "count".to_string(), vec![]),
        ]
    );
}

#[test]
fn a_handler_reaches_a_signal_through_its_context() {
    let view = "            <button type=\"button\" on:press={resumable() => count = count + 1}>Add</button>\n\
                \x20           <button type=\"button\" on:press={resumable() => panel = Panel.Item(3)}>Open</button>";
    let b = build(&page(SIGNALS, "", view));
    let sources: Vec<String> = b
        .handlers
        .iter()
        .map(|h| match &h.module {
            pw_core::backend::wasm::Encoding::Encoded(m) => m.source.clone(),
            other => panic!("a handler was not compiled: {other}"),
        })
        .collect();
    assert!(
        sources
            .iter()
            .any(|s| s.contains("context.get(\"count\")")
                && !s.contains("BigInt(context.get(")
                && s.contains("context.set(\"count\", exact(")),
        "{sources:#?}"
    );
    assert!(
        sources
            .iter()
            .any(|s| s.contains("context.set(\"panel\"") && s.contains("$case: \"item\"")),
        "{sources:#?}"
    );
    assert!(b.refusals().is_empty(), "{:?}", b.refusals());
}

#[test]
fn a_first_value_that_computes_is_refused_by_name() {
    let b = build(&page(
        "    signal count: Int = 1 + 2",
        "",
        "            <p>{count}</p>",
    ));
    let refused = b.refusals();
    assert!(
        refused
            .iter()
            .any(|r| r.contains("`t.P~ui`'s values") && r.contains("an operation")),
        "{refused:?}"
    );

    // Control: a literal.
    let b = build(&page(
        "    signal count: Int = 3",
        "",
        "            <p>{count}</p>",
    ));
    assert!(b.refusals().is_empty(), "{:?}", b.refusals());
}
