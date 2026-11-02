//! **A predicate the program gives a body** (track `row-ownership`,
//! ADR-0384's H2; ADR-0115 as amended): its body answers a `Bool` (PW0606)
//! and reads, and does nothing else (PW6905); a query may require one, as a
//! command may; it is compiled as a component of its own, kind `predicate`,
//! and each export requiring it names that component, where a predicate the
//! program declares with no body names none (the deployment's).

use pw_core::check::{Unit, check_sources};

fn root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The platform's packages and `examples/owned/`'s table, with `files`.
fn program(files: &[(&str, &str)]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for dir in ["packages/pw-std", "packages/pw-platform-web"] {
        let mut paths: Vec<_> = std::fs::read_dir(root().join(dir))
            .expect("packages")
            .map(|e| e.expect("entry").path())
            .filter(|p| p.extension().is_some_and(|x| x == "pw"))
            .collect();
        paths.sort();
        for p in paths {
            out.push((
                format!("{dir}/{}", p.file_name().unwrap().to_string_lossy()),
                std::fs::read_to_string(&p).expect("read"),
            ));
        }
    }
    for f in ["Notes.pw", "NoteData.pw"] {
        out.push((
            f.to_string(),
            std::fs::read_to_string(root().join("examples/owned").join(f)).expect("fixture"),
        ));
    }
    out.extend(files.iter().map(|(n, s)| (n.to_string(), s.to_string())));
    out
}

fn found(files: &[(&str, &str)]) -> Vec<(String, String)> {
    check_sources(&program(files))
        .into_iter()
        .filter(|(n, _)| !n.starts_with("packages/"))
        .flat_map(|(_, d)| d)
        .map(|d| (d.code.to_string(), d.message))
        .collect()
}

const HEAD: &str = "module probe\n\nimport capability.{ User, UserId }\nimport context.{ current_user }\nimport Notes.{ Note, NoteId }\nimport Notes\n\n";

const WROTE: &str = "predicate Wrote(id: NoteId)\n    says \"You can only change a note you wrote.\"\n{\n    match Notes.get(current_user(), id) {\n        Some(_) => true,\n        None => false,\n    }\n}\n";

fn probe(rest: &str) -> String {
    format!("{HEAD}{rest}")
}

/// A domain result held by an assertion, so a test fails at an `assert!`
/// (the mutation producer's native assertion rule), never at an `expect`.
fn held<T, E: std::fmt::Debug>(result: Result<T, E>, what: &str) -> T {
    assert!(result.is_ok(), "{what}: {:?}", result.as_ref().err());
    match result {
        Ok(value) => value,
        Err(_) => unreachable!(),
    }
}

/// An optional domain value held by an assertion.
fn found_one<T>(value: Option<T>, what: &str) -> T {
    assert!(value.is_some(), "{what}");
    match value {
        Some(value) => value,
        None => unreachable!(),
    }
}

#[test]
fn a_body_answers_a_bool() {
    let got = found(&[(
        "probe.pw",
        &probe("predicate Bad(n: Int)\n    says \"Never.\"\n{\n    n + 1\n}\n"),
    )]);
    assert!(
        got.iter()
            .any(|(c, m)| c == "PW0606" && m.contains("declares its result `Bool`")),
        "{got:#?}"
    );
    // The neighbour: one that reads the reader's note checks clean.
    let got = found(&[("probe.pw", &probe(WROTE))]);
    assert!(got.is_empty(), "{got:#?}");
}

#[test]
fn pw6905_a_body_reads_and_does_nothing_else() {
    let got = found(&[(
        "probe.pw",
        &probe(
            "predicate Clears(id: NoteId)\n    says \"Never.\"\n{\n    Notes.delete(current_user(), id)\n    true\n}\n",
        ),
    )]);
    assert!(
        got.iter().any(|(c, m)| c == "PW6905"
            && m.contains(
                "`predicate Clears`'s body does more than read: `database.write<Notes>`"
            )),
        "{got:#?}"
    );
    // A predicate the deployment answers has no body to hold.
    let got = found(&[(
        "probe.pw",
        &probe("predicate SignedIn\n    says \"Sign in.\"\n"),
    )]);
    assert!(got.is_empty(), "{got:#?}");
}

#[test]
fn a_query_requires_a_predicate_as_a_command_does() {
    let query = "user query Shown(reader: User<UserId>, id: NoteId) -> Option<Note>\n    freshness      0.seconds\n    consistency    read_your_writes\n    cache          private\n    key            reader, id\n    requires       Wrote(id)\n    concurrency    one_per_key\n    on_key_change  cancel\n    timeout        2.seconds\n{\n    Notes.get(reader, id)\n}\n";
    let got = found(&[("probe.pw", &probe(&format!("{WROTE}\n{query}")))]);
    assert!(got.is_empty(), "{got:#?}");
    // Its arguments are the query's parameters.
    let got = found(&[(
        "probe.pw",
        &probe(&format!(
            "{WROTE}\n{}",
            query.replace("Wrote(id)", "Wrote(other)")
        )),
    )]);
    assert!(
        got.iter()
            .any(|(c, m)| c == "PW0335" && m.contains("`other` is not a parameter of `Shown`")),
        "{got:#?}"
    );
    // A function's is no invocation's.
    let got = found(&[(
        "probe.pw",
        &probe(&format!(
            "{WROTE}\nfn read(id: NoteId) -> Int !{{}}\n    requires Wrote(id)\n{{\n    1\n}}\n"
        )),
    )]);
    assert!(
        got.iter()
            .any(|(c, m)| c == "PW0335" && m.contains("belongs to a command or a query")),
        "{got:#?}"
    );
}

/// The build of the platform, `examples/owned/`'s table and `files`.
fn built(files: &[(&str, &str)]) -> pw_core::build::Build {
    let files = program(files);
    let units: Vec<Unit> = files
        .iter()
        .map(|(path, src)| {
            let parsed = pw_syntax::parse_tree(src);
            assert!(parsed.ok(), "{path}: {:?}", parsed.errors);
            Unit {
                path: path.clone(),
                src: src.clone(),
                hir: pw_core::lower::lower_file(src, &parsed.green),
            }
        })
        .collect();
    held(pw_core::build::build(&units), "the probe builds")
}

/// `Wrote`, given its body in a module of its own.
const PREDS: &str = "module probe.preds\n\nimport context.{ current_user }\nimport Notes.{ NoteId }\nimport Notes\n\npredicate Wrote(id: NoteId)\n    says \"You can only change a note you wrote.\"\n{\n    match Notes.get(current_user(), id) {\n        Some(_) => true,\n        None => false,\n    }\n}\n";

const EDIT: &str = "command edit(id: NoteId) -> ()\n    requires    Wrote(id)\n    transaction serializable\n{\n    ()\n}\n";

#[test]
fn pw6925_a_predicate_with_a_body_is_required_where_it_is_seen() {
    // The review's finding 4 (probe G): `Wrote` has a body in
    // `probe.preds`, which `probe` does not import, so its `requires Wrote`
    // named no component and the deployment was asked for `Wrote` instead.
    let unseen = probe(EDIT);
    let got = found(&[("preds.pw", PREDS), ("probe.pw", &unseen)]);
    let told: Vec<&String> = got
        .iter()
        .filter(|(c, _)| c == "PW6925")
        .map(|(_, m)| m)
        .collect();
    assert_eq!(told.len(), 1, "{got:#?}");
    assert!(
        told[0].contains(
            "`requires Wrote` names no predicate `probe` sees, and `probe.preds` gives `Wrote` \
             a body: the host would ask the deployment for it instead"
        ),
        "{got:#?}"
    );
    // The neighbour: imported where it is required, it checks clean, and
    // the requirement names the body's component.
    let seen = probe(&format!("import probe.preds.{{ Wrote }}\n\n{EDIT}"));
    let got = found(&[("preds.pw", PREDS), ("probe.pw", &seen)]);
    assert!(got.is_empty(), "{got:#?}");
    let build = built(&[("preds.pw", PREDS), ("probe.pw", &seen)]);
    assert!(build.refusals().is_empty(), "{:?}", build.refusals());
    let edit = found_one(
        build
            .contracts
            .iter()
            .find(|c| c.component_id == "probe.edit"),
        "the edit command's contract",
    );
    let place = found_one(edit.exports[0].component.as_ref(), "an export's place");
    let required: Vec<(String, Option<String>)> = place
        .authorization
        .iter()
        .map(|r| (r.predicate.clone(), r.component.clone()))
        .collect();
    assert_eq!(
        required,
        [("Wrote".to_string(), Some("probe.preds.Wrote".to_string()))]
    );
    // A predicate the deployment answers, declared nowhere, is still the
    // deployment's: no body shares its name.
    let got = found(&[(
        "probe.pw",
        &probe(&EDIT.replace("Wrote(id)", "MayEdit(id)")),
    )]);
    assert!(!got.iter().any(|(c, _)| c == "PW6925"), "{got:#?}");
    // And one the program declares with no body, beside one with a body of
    // the same name, is PW0351's: a predicate is declared once.
    let got = found(&[
        ("preds.pw", PREDS),
        (
            "probe.pw",
            &probe(&format!(
                "predicate Wrote(id: NoteId)\n    says \"Never.\"\n\n{EDIT}"
            )),
        ),
    ]);
    assert!(
        got.iter()
            .any(|(c, m)| c == "PW0351" && m.contains("`predicate Wrote` is declared in")),
        "{got:#?}"
    );
}

#[test]
fn pw6925_a_page_asks_no_predicate_with_a_body() {
    // A page asks a predicate of its reader through the deployment
    // (ADR-0307), which knows nothing of a body: a `SignedIn` the program
    // answers `false` would be told the deployment's `SignedIn`.
    let mine = "predicate Mine\n    says \"Only its author.\"\n{\n    true\n}\n";
    let page = "session page Home() {\n    route     \"/\"\n    placement origin\n    cache     private\n\n    view {\n        <title>Home</title>\n        <main>\n            {#if Mine}<p id=\"mine\">Yours</p>{/if}\n        </main>\n    }\n}\n";
    let got = found(&[("probe.pw", &probe(&format!("{mine}\n{page}")))]);
    assert!(
        got.iter().any(|(c, m)| c == "PW6925"
            && m.contains("`Home` asks `Mine` of its reader, and the program gives `Mine` a body")),
        "{got:#?}"
    );
    // The neighbour: the deployment's predicate, declared with no body, is
    // what a page asks.
    let theirs = "predicate Mine\n    says \"Only its author.\"\n";
    let got = found(&[("probe.pw", &probe(&format!("{theirs}\n{page}")))]);
    assert!(!got.iter().any(|(c, _)| c == "PW6925"), "{got:#?}");
}

#[test]
fn pw6907_a_query_that_requires_a_predicate_is_never_cached_shared() {
    // The review's finding 7b (probe H): a public query cached shared,
    // answered to whoever asks next whether or not `Wrote` holds for them.
    let gated = |cache: &str| {
        probe(&format!(
            "{WROTE}\npublic query Gated(id: NoteId) -> Int\n    freshness      60.seconds\n    consistency    snapshot\n    cache          {cache}\n    key            id\n    requires       Wrote(id)\n    concurrency    one_per_key\n    on_key_change  cancel\n    timeout        2.seconds\n{{\n    1\n}}\n"
        ))
    };
    let got = found(&[("probe.pw", &gated("shared"))]);
    let told: Vec<&String> = got
        .iter()
        .filter(|(c, _)| c == "PW6907")
        .map(|(_, m)| m)
        .collect();
    assert_eq!(told.len(), 1, "{got:#?}");
    assert!(
        told[0].contains("`Gated` requires Wrote(id) and declares a shared cache"),
        "{got:#?}"
    );
    // Whatever answers the predicate: the deployment's too.
    let deployment = gated("shared")
        .replace(WROTE, "predicate SignedIn\n    says \"Sign in.\"\n")
        .replace("Wrote(id)", "SignedIn");
    assert!(
        found(&[("probe.pw", &deployment)])
            .iter()
            .any(|(c, _)| c == "PW6907"),
        "a deployment's predicate in a shared cache passed"
    );
    // The neighbour: cached for its reader alone.
    let got = found(&[("probe.pw", &gated("private"))]);
    assert!(!got.iter().any(|(c, _)| c == "PW6907"), "{got:#?}");
}

#[test]
fn a_bodied_predicate_is_a_component_its_requirements_name() {
    let src = probe(&format!(
        "{WROTE}\npredicate SignedIn\n    says \"Sign in.\"\n\ncommand touch(id: NoteId) -> ()\n    requires SignedIn, Wrote(id)\n{{\n    ()\n}}\n"
    ));
    let files = program(&[("probe.pw", &src)]);
    let units: Vec<Unit> = files
        .iter()
        .map(|(path, src)| {
            let parsed = pw_syntax::parse_tree(src);
            assert!(parsed.ok(), "{path}: {:?}", parsed.errors);
            Unit {
                path: path.clone(),
                src: src.clone(),
                hir: pw_core::lower::lower_file(src, &parsed.green),
            }
        })
        .collect();
    let build = held(pw_core::build::build(&units), "the probe builds");
    assert!(build.refusals().is_empty(), "{:?}", build.refusals());
    let contract = |id: &str| {
        found_one(
            build.contracts.iter().find(|c| c.component_id == id),
            &format!("no contract {id}"),
        )
    };
    assert_eq!(contract("probe.Wrote").exports[0].kind, "predicate");
    assert!(
        build
            .wit
            .contains("wrote: func(arg0: notes-note-id) -> bool;"),
        "{}",
        build.wit
    );
    let place = found_one(
        contract("probe.touch").exports[0].component.as_ref(),
        "an export's place",
    );
    let required: Vec<(String, Option<String>)> = place
        .authorization
        .iter()
        .map(|r| (r.predicate.clone(), r.component.clone()))
        .collect();
    assert_eq!(
        required,
        [
            ("SignedIn".to_string(), None),
            ("Wrote".to_string(), Some("probe.Wrote".to_string()))
        ]
    );
    // A predicate the deployment answers is no component.
    assert!(
        !build
            .contracts
            .iter()
            .any(|c| c.component_id == "probe.SignedIn"),
        "a deployment's predicate became a component"
    );
}
