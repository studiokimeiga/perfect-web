//! **A clause is read once** (ADR-0240).
//!
//! A clause naming declarations by their keys, `depends_on`, `invalidates_on`,
//! `emits` or `invalidates`, was read twice. Lowering read it with the grammar
//! into its keys (ADR-0088), which the checker, the contracts and the backend
//! read. The resource graph split its text at its commas, counting brackets.
//! The two disagreed:
//! - a value ADR-0237 reads past a missing comma was a key and no edge, so
//!   the feed's thread was refused again for a listener its clause names;
//! - a parenthesis inside a key's string ended the key early: `emits
//!   Searched(")"), Other(1)` had no edge to `Other`, which nothing declares,
//!   and checked.
//!
//! And an interface's keys were lowered by nothing, so the graph's text was
//! their one reading: never bound to a parameter, never typed. Each test
//! states one case, with a control.

use pw_core::check::{Unit, check_sources};
use pw_core::graph::{EdgeKind, Graph};
use pw_core::lower::lower_file;
use pw_core::resolve::Workspace;
use pw_syntax::parse_tree;

/// The platform's packages, and `src` as `app.pw` after them.
fn program(src: &str) -> Vec<(String, String)> {
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
    out.push(("app.pw".to_string(), src.to_string()));
    out
}

/// What the checker reports of `app.pw`: each code, its message and the text
/// it underlines.
fn found(src: &str) -> Vec<(String, String, String)> {
    check_sources(&program(src))
        .into_iter()
        .filter(|(n, _)| n == "app.pw")
        .flat_map(|(_, ds)| ds)
        .map(|d| {
            (
                d.code.to_string(),
                d.message.clone(),
                src[d.primary_span.clone()].to_string(),
            )
        })
        .collect()
}

/// The resource graph of the packages and `src`.
fn graph(src: &str) -> Graph {
    let units: Vec<Unit> = program(src)
        .into_iter()
        .map(|(path, src)| Unit {
            hir: lower_file(&src, &parse_tree(&src).green),
            path,
            src,
        })
        .collect();
    let hirs: Vec<&pw_core::hir::Hir> = units.iter().map(|u| &u.hir).collect();
    Graph::build(&hirs, &Workspace::build(&hirs))
}

fn feed() -> String {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    std::fs::read_to_string(root.join("examples/feed/app.pw")).expect("the feed")
}

#[test]
fn the_graph_reads_the_keys_lowering_made() {
    // A missing comma is the one error, and the clause's second value is an
    // edge as it is a key: the thread listens for posts, so `reply`, which
    // speculates on it, is not refused for a listener it lacks (PW5107).
    let app = feed().replace(
        "invalidates_on Liked(id), Posted(_)",
        "invalidates_on Liked(id) Posted(_)",
    );
    assert_eq!(
        found(&app),
        [(
            "PW0016".to_string(),
            "a clause's values are separated by commas".to_string(),
            "Posted(_)".to_string()
        )]
    );
    let listens: Vec<(String, Vec<String>)> = graph(&app)
        .edges
        .into_iter()
        .filter(|e| e.from == "feed.app.Thread" && e.kind == EdgeKind::InvalidatedBy)
        .map(|e| (e.to, e.key))
        .collect();
    assert_eq!(
        listens,
        [
            // A post deleted (track `identity`): every thread may hold it as
            // a reply.
            ("feed.app.Deleted".to_string(), vec!["_".to_string()]),
            ("feed.app.Liked".to_string(), vec!["id".to_string()]),
            ("feed.app.Posted".to_string(), vec!["_".to_string()]),
        ]
    );
}

#[test]
fn a_parenthesis_in_a_keys_string_ends_nothing() {
    let src = |emits: &str| {
        format!(
            "module m\n\nevent Searched(term: String)\n\n\
             command search() -> Result<Int, String>\n    requires Anyone\n    emits {emits}\n{{\n    Ok(1)\n}}\n"
        )
    };
    // The control.
    assert_eq!(found(&src("Searched(\")\")")), vec![]);
    // The key after it names nothing, at its name.
    assert_eq!(
        found(&src("Searched(\")\"), Other(1)")),
        [(
            "PW5100".to_string(),
            "`search` emits `Other`, which nothing declares".to_string(),
            "Other".to_string()
        )]
    );
}

#[test]
fn an_interfaces_keys_are_bound_and_typed() {
    // A declaration with no body is an interface. Its listener's key binds a
    // parameter and is typed against the event, as any declaration's is.
    let src = |key: &str, param: &str| {
        format!(
            "module m\n\nevent Changed(id: Int)\n\n\
             query Read(id: {param}) -> Int\n    invalidates_on Changed({key})\n"
        )
    };
    // The control.
    assert_eq!(found(&src("id", "Int")), vec![]);
    assert_eq!(
        found(&src("nosuch", "Int")),
        [(
            "PW5104".to_string(),
            "`Read` listens for `Changed` with `nosuch`, which is none of its parameters"
                .to_string(),
            "nosuch".to_string()
        )]
    );
    let typed = found(&src("id", "String"));
    assert_eq!(typed.len(), 1, "{typed:#?}");
    assert_eq!(
        (typed[0].0.as_str(), typed[0].1.as_str()),
        (
            "PW0605",
            "argument 1 of `m.Changed` is declared `Int` and this is `String`"
        )
    );
}

#[test]
fn an_interfaces_clause_is_parsed_with_its_terms() {
    // ADR-0237 parsed an interface's clauses for their errors alone; they are
    // lowered now, and their errors are the lowering's, once.
    let src = "module m\n\nevent Changed(id: String)\nevent Other(id: String)\n\n\
               query Read(id: String) -> Int\n    invalidates_on Changed(id) Other(id)\n";
    assert_eq!(
        found(src),
        [(
            "PW0016".to_string(),
            "a clause's values are separated by commas".to_string(),
            "Other(id)".to_string()
        )]
    );
}

#[test]
fn an_interfaces_terms_are_checked_as_any_declarations() {
    // A command with no body, its clauses' terms in their own arena: each
    // rule that reads a term reads them, as it reads a body's.
    let src = |clause: &str| {
        format!(
            "module m\n\nevent Changed(id: Int)\n\n\
             query Thing(x: Int) -> Int\n    invalidates_on Changed(x)\n{{\n    x\n}}\n\n\
             fn label(n: Int) -> String !{{}} {{\n    \"x\"\n}}\n\n\
             fn seen(n: Int) -> Int !{{ session.read }} {{\n    n\n}}\n\n\
             fn apply(f: fn(Int, Int) -> Int, n: Int) -> Int !{{}} {{\n    f(n, n)\n}}\n\n\
             command c(x: Int) -> Result<Int, String>\n    requires Anyone\n    {clause}\n"
        )
    };
    let codes = |clause: &str| -> Vec<String> {
        found(&src(clause))
            .into_iter()
            .map(|(code, ..)| code)
            .collect()
    };
    // The control.
    assert_eq!(
        codes("emits Changed(x)\n    optimistic Thing(x) as t => t + 1"),
        Vec::<String>::new()
    );
    // A key's name is resolved.
    assert_eq!(codes("emits Changed(nosuch)"), ["PW0021"]);
    // A speculation's target is invalidated by the command.
    assert_eq!(codes("optimistic Thing(x) as t => t + 1"), ["PW5107"]);
    // A transition performs nothing.
    assert_eq!(
        codes("emits Changed(x)\n    optimistic Thing(x) as t => seen(t)"),
        ["PW0330"]
    );
    // A target is a resource's entry.
    assert_eq!(
        codes("emits Changed(x)\n    optimistic label(x) as t => t"),
        ["PW0331"]
    );
    // A transition produces its target's value.
    assert_eq!(
        codes("emits Changed(x)\n    optimistic Thing(x) as t => label(t)"),
        ["PW0331"]
    );
    // A name is bound once.
    assert_eq!(
        codes("emits Changed(x)\n    optimistic Thing(x) as t => apply((a, a) => a, t)"),
        ["PW0028"]
    );
}
