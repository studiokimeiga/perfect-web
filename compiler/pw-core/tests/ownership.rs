//! **No ownership syntax is required for ordinary application values** (E10
//! gate item 5).
//!
//! Charter §14 M10 task 6: "Keep ordinary source free of borrow/lifetime
//! syntax." Task 7: "Expose affine annotations only for scarce resources where
//! they express a real invariant."
//!
//! In Pleris a value is affine because its PRODUCER's effect row says
//! `resource.acquire<T>`. That is a fact about one library function, and
//! nothing is written where the value is used. These tests make that a
//! property of the programs this project has, rather than a sentence about the
//! language: what the store and the accepted corpus declare affine, that none
//! of the store's values is, and that the language has no borrow, lifetime or
//! move syntax to write.

use std::collections::BTreeSet;

use pw_core::hir::Hir;
use pw_core::lower::lower_file;
use pw_core::resolve::Workspace;
use pw_core::signatures::Signatures;
use pw_syntax::parse_tree;

fn sources(dirs: &[&str], files: &[&str]) -> Vec<String> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut out = Vec::new();
    for d in dirs {
        let mut ps: Vec<std::path::PathBuf> = std::fs::read_dir(root.join(d))
            .unwrap_or_else(|e| panic!("{d}: {e}"))
            .map(|e| e.expect("entry").path())
            .filter(|p| p.extension().is_some_and(|x| x == "pw"))
            .collect();
        ps.sort();
        for p in ps {
            out.push(std::fs::read_to_string(&p).expect("read"));
        }
    }
    for f in files {
        out.push(std::fs::read_to_string(root.join(f)).expect("read"));
    }
    out
}

const LIBRARY: [&str; 3] = [
    "packages/pw-std",
    "packages/pw-platform-web",
    "examples/lib",
];

/// The store, and the accepted corpus: the two programs `just ci` checks.
fn programs() -> Vec<(&'static str, Vec<String>)> {
    let mut store_dirs = LIBRARY.to_vec();
    store_dirs.push("examples/store");
    let mut corpus_dirs = LIBRARY.to_vec();
    corpus_dirs.push("examples/accepted");
    vec![
        ("store", sources(&store_dirs, &["examples/domain.pw"])),
        (
            "accepted corpus",
            sources(&corpus_dirs, &["examples/domain.pw"]),
        ),
    ]
}

/// Every type some signature's effect row acquires: what the program makes
/// affine, read from the rows `affine.rs` reads.
fn acquired(sigs: &Signatures) -> BTreeSet<String> {
    sigs.all_signatures()
        .flat_map(|s| s.effects.iter())
        .filter_map(|e| e.strip_prefix("resource.acquire<")?.strip_suffix('>'))
        .map(str::to_string)
        .collect()
}

/// Does this written type mention `name` as a whole word?
fn mentions(ty: &str, name: &str) -> bool {
    ty.split(|c: char| !c.is_alphanumeric() && c != '_')
        .any(|word| word == name)
}

#[test]
fn only_scarce_resources_are_affine() {
    for (what, srcs) in programs() {
        let hirs: Vec<Hir> = srcs
            .iter()
            .map(|s| lower_file(s, &parse_tree(s).green))
            .collect();
        let refs: Vec<&Hir> = hirs.iter().collect();
        let ws = Workspace::build(&refs);
        let sigs = Signatures::build(&ws, &refs);
        let affine = acquired(&sigs);
        println!("{what}: affine by an acquiring producer: {affine:?}");
        // A connection, a transaction, an imperative widget's handle, the
        // standard library's generic helpers over any resource, and an
        // upload's claim (corpus C34's A-061, ADR-0440: its generated `claim`
        // acquires it, the `FileUpload` of charter §7.7, which a command's
        // transaction holds). Anything new here is a visible change, which
        // is the point of listing them.
        for t in &affine {
            assert!(
                [
                    "DatabaseConnection",
                    "DatabaseTransaction",
                    "MapHandle",
                    "PostImage.Claim",
                    "R",
                    "T"
                ]
                .contains(&t.as_str()),
                "{what}: `{t}` became affine"
            );
        }
    }
}

#[test]
fn no_store_value_is_affine_or_annotated() {
    let (_, srcs) = programs().remove(0);
    let hirs: Vec<Hir> = srcs
        .iter()
        .map(|s| lower_file(s, &parse_tree(s).green))
        .collect();
    let refs: Vec<&Hir> = hirs.iter().collect();
    let ws = Workspace::build(&refs);
    let sigs = Signatures::build(&ws, &refs);
    let affine: Vec<String> = acquired(&sigs)
        .into_iter()
        .filter(|t| t.len() > 1)
        .collect();

    let mut declarations = 0;
    let mut positions = 0;
    for s in sigs
        .all_signatures()
        .filter(|s| s.path.starts_with("store.page."))
    {
        declarations += 1;
        let types = s
            .params
            .iter()
            .chain(std::iter::once(&s.returns))
            .flatten()
            .filter_map(|t| t.resolved())
            .map(|t| t.to_string());
        for ty in types {
            positions += 1;
            for a in &affine {
                assert!(
                    !mentions(&ty, a),
                    "`{}` carries `{ty}`, which is affine",
                    s.path
                );
            }
        }
    }
    println!(
        "store: {declarations} declarations, {positions} typed value positions, 0 affine, \
         0 ownership annotations"
    );
    assert!(declarations >= 5, "the page's commands and queries");
    assert!(positions >= 10);
}

#[test]
fn the_language_has_no_borrow_lifetime_or_move_syntax() {
    // Each annotation a language with ownership would ask for, next to the
    // same declaration without it. The plain one parses; the annotated one is
    // not Pleris.
    let plain = "fn f(x: Int) -> Int { x }\n";
    assert!(parse_tree(plain).ok(), "the control parses");
    for annotated in [
        "fn f(x: &Int) -> Int { x }\n",
        "fn f(x: &mut Int) -> Int { x }\n",
        "fn f<'a>(x: Int) -> Int { x }\n",
        "fn f(x: 'a Int) -> Int { x }\n",
        "fn f(move x: Int) -> Int { x }\n",
        "fn f(x: Box<Int>) -> Int { *x }\n",
    ] {
        println!("not Pleris: {}", annotated.trim());
        let parsed = parse_tree(annotated);
        let lowered = lower_file(annotated, &parsed.green);
        let refs = vec![&lowered];
        let ws = Workspace::build(&refs);
        let sigs = Signatures::build(&ws, &refs);
        let unresolved = sigs
            .all_signatures()
            .flat_map(|s| s.params.iter())
            .any(|p| !p.as_ref().is_some_and(|t| t.resolved().is_some()));
        assert!(
            !parsed.ok() || unresolved,
            "`{}` was accepted as Pleris",
            annotated.trim()
        );
    }
}
