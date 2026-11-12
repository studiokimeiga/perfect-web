//! **A handler's identity is its own file's** (ADR-0135).
//!
//! The build gives each event part the identity `resume_artifacts` derived
//! for its handler, through one map for the whole program. Until 2026-10-02
//! the map was keyed by a declaration's index and an expression's index,
//! both counted within one file, so two pages of the same shape in two files
//! shared a key: the second file's identity replaced the first's, and the
//! first page's button ran the second page's code.
//!
//! Found writing ADR-0134, by two pages that differ only in what their
//! handler adds.

use pw_core::check::Unit;
use pw_core::lower::lower_file;
use pw_syntax::parse_tree;

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

/// A page in module `m` whose one handler adds `step`.
fn page(m: &str, step: u32) -> String {
    format!(
        "module {m}\n\npage P() {{\n    cache private\n\n    signal n: Int = 0\n\n    view {{\n        \
         <main><button type=\"button\" on:press={{() => n = n + {step}}}>Go</button><p>{{n}}</p></main>\n    \
         }}\n}}\n"
    )
}

#[test]
fn two_pages_of_one_shape_in_two_files_keep_their_own_handlers() {
    let mut sources = library();
    sources.push(("a.pw".to_string(), page("a", 1)));
    sources.push(("b.pw".to_string(), page("b", 2)));
    let units: Vec<Unit> = sources
        .into_iter()
        .map(|(path, src)| Unit {
            path,
            hir: lower_file(&src, &parse_tree(&src).green),
            src,
        })
        .collect();
    let b = pw_core::build::build(&units).expect("builds");

    // Each page's event part names a module whose body adds that page's step.
    // Each step a literal, a number as an `Int` within ±2^53 is (ADR-0433).
    for (page, adds) in [("a.P~ui", "1"), ("b.P~ui", "2")] {
        let t = b
            .templates
            .iter()
            .find(|t| t.path == page)
            .expect("a template");
        let ir = serde_json::to_string(t).expect("serializes");
        let identity = ir
            .split("\"handler\":\"")
            .nth(1)
            .and_then(|r| r.split('"').next())
            .expect("an event part with a handler");
        let module = b
            .handlers
            .iter()
            .find_map(|h| match &h.module {
                pw_core::backend::wasm::Encoding::Encoded(m) if m.identity == identity => {
                    Some(m.source.clone())
                }
                _ => None,
            })
            .expect("the identity's module");
        assert!(
            module.contains(&format!("= {adds};")),
            "`{page}` runs a handler that does not add its own step:\n{module}"
        );
    }
}
