//! **An optimistic transition's value is the value typer's** (ADR-0241).
//!
//! A transition produces the value of the entry it replaces (PW0331,
//! ADR-0025's ruling). The rule read the transition's type through the older
//! typer, which has no answer for a literal, and no answer was no violation:
//! `optimistic Thing(x) as t => "no"`, where `Thing` holds an `Int`, checked,
//! with a body or without. The value typer types every expression, and
//! relates each transition to its own target's value now. Each test states one
//! case, with a control.

use pw_core::check::check_sources;

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

/// A command whose clause is `transition`, with a body or as an interface.
fn command(transition: &str, body: bool) -> String {
    format!(
        "module m\n\nevent Changed(id: Int)\n\n\
         query Thing(x: Int) -> Int\n    invalidates_on Changed(x)\n{{\n    x\n}}\n\n\
         fn label(n: Int) -> String !{{}} {{\n    \"x\"\n}}\n\n\
         command c(x: Int) -> Result<Int, String>\n    requires Anyone\n    emits Changed(x)\n    \
         optimistic Thing(x) as t => {transition}\n{}",
        if body { "{\n    Ok(x)\n}\n" } else { "" }
    )
}

#[test]
fn a_literal_is_a_value_of_its_type() {
    for body in [true, false] {
        // The control.
        assert_eq!(found(&command("t + 1", body)), vec![], "body: {body}");
        assert_eq!(
            found(&command("\"no\"", body)),
            [(
                "PW0331".to_string(),
                "`c`'s optimistic transition produces `String`, but it targets a resource \
                 whose value is `Int`"
                    .to_string(),
                "\"no\"".to_string()
            )],
            "body: {body}"
        );
    }
}

#[test]
fn a_call_is_reported_once() {
    // The older reading caught a call's type; the value typer does, and the
    // two do not both report it.
    let f = found(&command("label(t)", true));
    assert_eq!(f.len(), 1, "{f:#?}");
    assert_eq!(f[0].0, "PW0331");
    assert_eq!(f[0].2, "label(t)");
}

#[test]
fn the_repair_names_the_binder_and_the_target_holds_the_value() {
    let src = command("\"no\"", true);
    let program = program(&src);
    let d = check_sources(&program)
        .into_iter()
        .filter(|(n, _)| n == "app.pw")
        .flat_map(|(_, ds)| ds)
        .next()
        .expect("the error");
    assert_eq!(d.repairs[0].description, "produce an `Int` from `t`");
    assert_eq!(d.related[0].label, "this resource holds `Int`");
    assert_eq!(&src[d.related[0].span.clone()], "Thing(x)");
}

#[test]
fn a_target_is_a_resources_entry() {
    // What `check.rs` still says of a clause: its target is a resource's
    // entry. A call to a function is none, whatever the transition makes.
    let src = command("t", true).replace("optimistic Thing(x) as t", "optimistic label(x) as t");
    assert_eq!(
        found(&src),
        [(
            "PW0331".to_string(),
            "`c`'s optimistic clause targets no resource: `label` is not a resource this file \
             can see"
                .to_string(),
            "label(x)".to_string()
        )]
    );
}
