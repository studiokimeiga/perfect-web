//! **A dependency-graph clause belongs to a declaration that can mean it**
//! (ADR-0092).
//!
//! ADR-0007 made invalidation explicit: a command emits typed events and
//! invalidates the entries it changes, and a resource or a materialization
//! listens for events and depends on resources. Until 2026-09-26 a clause
//! checked on any declaration. A query that `emits`, a command that listens,
//! a `fn` that emits and a page that invalidates each checked, and nothing
//! read what they said. Each test states one case, with a control.

use pw_core::check::check_sources;

fn reported(src: &str) -> Vec<String> {
    check_sources(&[("t.pw".to_string(), src.to_string())])
        .into_iter()
        .flat_map(|(_, ds)| ds.into_iter().map(|d| format!("{} {}", d.code, d.message)))
        .collect()
}

fn one(src: &str, says: &str) {
    let found = reported(src);
    assert_eq!(found.len(), 1, "{src}\n{found:#?}");
    assert!(found[0].contains(says), "{src}\n{found:#?}");
}

fn clean(src: &str) {
    let found = reported(src);
    assert!(found.is_empty(), "{src}\n{found:#?}");
}

const DECLS: &str = "module t\n\nevent Moved(to: Int)\n\nquery Box(id: Int) -> Int !{} { id }\n\n";

/// A declaration written `head(params)`, with one clause, and a body.
fn with(head: &str, clause: &str) -> String {
    // A command says who may run it (the requires ADR).
    let who = if head.starts_with("command ") {
        "\n    requires Anyone"
    } else {
        ""
    };
    format!("{DECLS}{head} -> Int !{{}}{who}\n    {clause}\n{{\n    0\n}}\n")
}

#[test]
fn a_command_emits_and_invalidates() {
    clean(&with("command shift(by: Int)", "emits Moved(by)"));
    clean(&with("command shift(by: Int)", "invalidates Box(by)"));
    one(
        &with("query Other(by: Int)", "emits Moved(by)"),
        "PW5105 `Other` is a query, and `emits` belongs to a command",
    );
    one(
        &with("query Other(by: Int)", "invalidates Box(by)"),
        "PW5105 `Other` is a query, and `invalidates` belongs to a command",
    );
    one(
        &with("fn helper(by: Int)", "emits Moved(by)"),
        "PW5105 `helper` is a function, and `emits` belongs to a command",
    );
}

#[test]
fn a_resource_listens() {
    clean(&with("query Other(by: Int)", "invalidates_on Moved(by)"));
    one(
        &with("command shift(by: Int)", "invalidates_on Moved(by)"),
        "PW5105 `shift` is a command, and `invalidates_on` belongs to a resource or a materialization",
    );
}

#[test]
fn a_materialization_depends() {
    clean(&format!(
        "{DECLS}materialize Panel(id: Int) {{\n    placement origin\n    partition private\n    \
         depends_on Box(id)\n    invalidates_on Moved(id)\n    regenerate on_invalidation\n}}\n"
    ));
    one(
        &with("command shift(by: Int)", "depends_on Box(by)"),
        "PW5105 `shift` is a command, and `depends_on` belongs to a materialization",
    );
}

/// **Every head belongs somewhere, and is refused elsewhere** (ADR-0216, the
/// owner's ruling 0092-b; ADR-0210's urgent defect 5). Until then only the
/// graph's four clauses were placed: `freshness` on a command, `retry` on a
/// function, checked, and nothing read them.
#[test]
fn a_head_on_a_declaration_that_does_not_read_it_is_refused() {
    one(
        &with("command shift(by: Int)", "freshness 30.seconds"),
        "PW5105 `shift` is a command, and `freshness` belongs to a query, a subscription or a resource",
    );
    one(
        &with("command shift(by: Int)", "on_key_change cancel"),
        "PW5105 `shift` is a command, and `on_key_change` belongs to a query or a resource",
    );
    one(
        &with("command shift(by: Int)", "cache shared"),
        "PW5105 `shift` is a command, and `cache` belongs to a query, a subscription, a resource or a page",
    );
    one(
        &with("fn shift(by: Int)", "retry bounded_exponential(max = 2)"),
        "PW5105 `shift` is a function, and `retry` belongs to a query, a resource or a command",
    );
    // Where each is read, it is not.
    clean(&with("query Read(by: Int)", "freshness 30.seconds"));
}

/// **Every head the grammar knows has a place** (ADR-0216): the table fails
/// closed, so a head added to the language without one would be refused
/// nowhere and read nowhere.
#[test]
fn every_head_the_grammar_knows_has_a_place() {
    let unplaced: Vec<&str> = pw_syntax::POLICY_KEYWORDS
        .iter()
        .copied()
        .filter(|h| pw_core::policy::declared_by(h).is_none())
        .collect();
    assert_eq!(unplaced, Vec::<&str>::new());
}

/// **A code body admits no clause** (ADR-0216, the owner's ruling 0047-a,
/// its interim). `cache nothing_y` in a function's body was a clause nothing
/// read, and `nothing_y` was never resolved.
#[test]
fn a_clause_word_in_a_functions_body_is_a_name() {
    let src = format!("{DECLS}fn f() -> Int !{{}} {{\n    cache nothing_y\n    0\n}}\n");
    let found = reported(&src);
    assert!(
        found
            .iter()
            .any(|d| d.starts_with("PW0021") && d.contains("nothing_y")),
        "{found:#?}"
    );
}
