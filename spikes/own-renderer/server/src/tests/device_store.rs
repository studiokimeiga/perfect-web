//! **A page that holds a value the device keeps, served** (track
//! `device-store`, ADR-0366): what the host serves for one beyond what
//! TodoMVC's document shows (`tests::todomvc::`). Here, the steps that take
//! a stored value's earlier version to the page's, which the browser runs
//! before it shows the value, served where the page computes nothing else.

use super::*;

/// `src` beside the platform's packages, built as `pw build` builds it, and
/// served.
pub(super) fn served(src: &str) -> Served {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let dir = tempfile::TempDir::with_prefix("pw-device-store-").expect("a temporary directory");
    let mut units = Vec::new();
    for d in ["packages/pw-std", "packages/pw-platform-web"] {
        let mut paths: Vec<std::path::PathBuf> = std::fs::read_dir(root.join(d))
            .unwrap_or_else(|e| panic!("{d}: {e}"))
            .map(|e| e.expect("entry").path())
            .filter(|p| p.extension().is_some_and(|x| x == "pw"))
            .collect();
        paths.sort();
        for p in paths {
            let src = std::fs::read_to_string(&p).expect("read");
            units.push(pw_core::check::Unit {
                hir: pw_core::lower::lower_file(&src, &pw_syntax::parse_tree(&src).green),
                path: p.display().to_string(),
                src,
            });
        }
    }
    units.push(pw_core::check::Unit {
        hir: pw_core::lower::lower_file(src, &pw_syntax::parse_tree(src).green),
        path: "t.pw".to_string(),
        src: src.to_string(),
    });
    let build = pw_core::build::build(&units).expect("it builds");
    assert!(build.refusals().is_empty(), "{:?}", build.refusals());
    let out = dir.path().join("build");
    build.write(&out).expect("the build is written");
    Served {
        server: Server::from_build(out.clone(), out).expect("served"),
        _dir: dir,
    }
}

/// What `path` answers, whole.
fn answer(s: &Server, path: &str) -> String {
    fetched_as(s, path, Some("a"))
        .into_iter()
        .map(|(_, c)| c)
        .collect()
}

/// A word the device keeps at version 2, taken from version 1 by a step,
/// shown as it is beside a query's greeting: the browser computes nothing
/// but the step.
const WORD: &str = "module t\n\nimport String\n\n\
signal word: String = \"\"\n    storage  device \"t-word\"\n    privacy  Device\n    \
conflict last_write_wins\n    version  2\n    migrate  word_from_1\n\n\
fn word_from_1(old: String) -> String !{} {\n    String.trim(old)\n}\n\n\
public query Greeting() -> String !{} {\n    \"hello\"\n}\n\n\
page P() {\n    route \"/\"\n    cache private\n\n    let greeting = query Greeting()\n\n    \
view {\n        <title>t</title>\n        <main>\n            <h1>{greeting}</h1>\n            \
<p id=\"word\">{word}</p>\n        </main>\n    }\n}\n";

#[test]
fn a_page_whose_one_computing_is_a_step_between_versions_is_served_its_steps() {
    let s = served(WORD);
    let page = answer(&s, "/");
    assert!(page.starts_with("HTTP/1.1 200"), "{page}");
    let start = page
        .find("<script type=\"application/json\" id=\"pw-parts\">")
        .expect("a manifest")
        + "<script type=\"application/json\" id=\"pw-parts\">".len();
    let end = start + page[start..].find("</script>").expect("its end");
    let manifest: serde_json::Value = serde_json::from_str(&page[start..end]).expect("JSON");
    assert_eq!(manifest["device"][0]["version"], 2, "{manifest:#}");
    // The module the browser runs the step from is named, and served.
    let module = manifest["computed"].as_str();
    assert!(
        module.is_some(),
        "the migration module is named: {manifest:#}"
    );
    let module = module.expect("the module name was asserted");
    let served = answer(&s, module);
    assert!(served.starts_with("HTTP/1.1 200"), "{served}");
    assert!(served.contains("export const migrations"), "{served}");
    assert!(served.contains("\"word\": ["), "{served}");
}
