//! **A clause names a declaration of its kind, and gives it its key**
//! (ADR-0088).
//!
//! `depends_on Menu(id)`, `invalidates Cart(current_session())` and `emits
//! CartChanged(current_session())` each name a declaration and pass it
//! values. Until 2026-09-26 the value was text: the graph found the name
//! wherever a name might be and never asked what it found, and nothing
//! resolved, counted or typed the values. `invalidates Cart(nosuch)`,
//! `invalidates Cart(item)` and `emits Cart(..)` checked. Each test states
//! one case, with a control.

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

/// A resource, an event, a command that invalidates the one and emits the
/// other, and a materialization that reads the one and listens for the
/// other. Each placeholder is replaced by a test.
const PROGRAM: &str = "module t\n\n\
    event Moved(to: Int)\n\n\
    query Box(id: Int) -> Int !{} { id }\n\n\
    command shift(by: Int, note: String) -> Int !{}\n    requires Anyone\n    \
        invalidates INVALIDATES\n    \
        emits       EMITS\n{\n    by\n}\n\n\
    materialize Panel(id: Int) {\n    \
        placement      origin\n    \
        partition      private\n    \
        depends_on     DEPENDS\n    \
        invalidates_on Moved(id)\n    \
        regenerate     on_invalidation\n}\n";

fn program(invalidates: &str, emits: &str, depends: &str) -> String {
    PROGRAM
        .replace("INVALIDATES", invalidates)
        .replace("EMITS", emits)
        .replace("DEPENDS", depends)
}

#[test]
fn the_program_is_clean() {
    clean(&program("Box(by)", "Moved(by)", "Box(id)"));
}

#[test]
fn a_key_names_what_scope_binds() {
    one(
        &program("Box(nosuch)", "Moved(by)", "Box(id)"),
        "PW0021 `nosuch` does not resolve",
    );
    one(
        &program("Box(by)", "Moved(nosuch)", "Box(id)"),
        "PW0021 `nosuch` does not resolve",
    );
    one(
        &program("Box(by)", "Moved(by)", "Box(nosuch)"),
        "PW0021 `nosuch` does not resolve",
    );
}

#[test]
fn a_resource_is_given_its_key() {
    one(
        &program("Box(note)", "Moved(by)", "Box(id)"),
        "PW0605 argument 1 of `t.Box` is declared `Int` and this is `String`",
    );
    one(
        &program("Box(by, 2)", "Moved(by)", "Box(id)"),
        "PW0604 `t.Box` declares 1 argument and this call passes 2",
    );
    // A name alone is the declaration given no key.
    one(
        &program("Box", "Moved(by)", "Box(id)"),
        "PW0604 `t.Box` declares 1 argument and this call passes 0",
    );
    one(
        &program("Box(by)", "Moved(by)", "Box(\"a\")"),
        "PW0605 argument 1 of `t.Box` is declared `Int` and this is `String`",
    );
    // A key's value may be named, as a call's argument is (ADR-0081).
    clean(&program("Box(id = by)", "Moved(by)", "Box(id = id)"));
    one(
        &program("Box(di = by)", "Moved(by)", "Box(id)"),
        "PW0617 `t.Box` has no parameter `di`",
    );
}

#[test]
fn an_event_is_given_what_it_carries() {
    one(
        &program("Box(by)", "Moved(note)", "Box(id)"),
        "PW0605 argument 1 of `t.Moved` is declared `Int` and this is `String`",
    );
    one(
        &program("Box(by)", "Moved()", "Box(id)"),
        "PW0604 `t.Moved` declares 1 argument and this call passes 0",
    );
    clean(&program("Box(by)", "Moved(by + 1)", "Box(id)"));
}

#[test]
fn a_clause_names_a_declaration_of_its_kind() {
    one(
        &program("Moved(by)", "Moved(by)", "Box(id)"),
        "PW5103 `shift` invalidates `Moved`, which is an event, not a resource",
    );
    one(
        &program("Box(by)", "Box(by)", "Box(id)"),
        "PW5103 `shift` emits `Box`, which is a resource, not an event",
    );
    one(
        &program("Box(by)", "Moved(by)", "Moved(id)"),
        "PW5103 `Panel` depends on `Moved`, which is an event, not a resource",
    );
    // A term of another kind is not given a key: the kind is the whole
    // finding, and `shift`'s own parameters are not what `invalidates` names.
    one(
        &program("shift(note, note)", "Moved(by)", "Box(id)"),
        "PW5103 `shift` invalidates `shift`, which is a command, not a resource",
    );
}

/// **A clause's terms are read where the source writes them.** The value a
/// policy keeps collapses runs of whitespace, and an optimistic clause was
/// parsed from it: with extra spaces before a name, every diagnostic inside
/// the clause underlined the wrong columns.
#[test]
fn a_clause_is_read_where_it_is_written() {
    let src = "module t\n\n\
        query Box(id: Int) -> Int !{} { id }\n\n\
        command shift(by: Int) -> Int !{}\n    requires Anyone\n    \
            optimistic  Box(by)   as   n   =>   n + byy\n    \
            invalidates    Box(byy)\n{\n    by\n}\n";
    let spans: Vec<(String, String)> = check_sources(&[("t.pw".to_string(), src.to_string())])
        .into_iter()
        .flat_map(|(_, ds)| ds)
        .map(|d| (d.code.to_string(), src[d.primary_span.clone()].to_string()))
        .collect();
    assert_eq!(
        spans,
        vec![
            ("PW0021".to_string(), "byy".to_string()),
            ("PW0021".to_string(), "byy".to_string()),
        ],
        "{spans:#?}"
    );
}
