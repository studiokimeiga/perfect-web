//! Qualified device signals preserve their declaration's type and privacy.
use pw_core::check::{Unit, check_sources};
use pw_core::diagnostics::Diagnostic;

const PREFS: &str = "module prefs\nsignal word: String = \"\"\n    storage device \"prefs-word\"\n    privacy Device\n    conflict last_write_wins\n";

fn sources(import: &str, signals: &str, handler: &str, takes: &str) -> Vec<(String, String)> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut sources = Vec::new();
    for directory in ["packages/pw-std", "packages/pw-platform-web"] {
        let mut paths: Vec<_> = std::fs::read_dir(root.join(directory))
            .expect("packages")
            .map(|entry| entry.expect("entry").path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "pw"))
            .collect();
        paths.sort();
        for path in paths {
            sources.push((
                path.display().to_string(),
                std::fs::read_to_string(path).expect("source"),
            ));
        }
    }
    sources.push(("prefs.pw".into(), PREFS.into()));
    sources.push((
        "app.pw".into(),
        format!(
            r#"module app
{import}
type PublicPrefs = PublicPrefs {{ word: String }}
command send(text: {takes}) -> Result<String, String>
    requires Anyone
    idempotent_by Press
{{ Ok("sent") }}
opaque type Press = String
page P() {{
    route "/"
    cache private
    signal draft: String = ""
    {signals}
    view {{
        <title>t</title>
        <main><button type="button" on:press={{() => {handler}}}>Send</button></main>
    }}
}}
"#
        ),
    ));
    sources
}

fn send(value: &str) -> String {
    format!("match send({value}) {{ Ok(_) => draft = \"\", Err(_) => draft = \"\", }}")
}
fn diagnostics(sources: &[(String, String)]) -> Vec<Diagnostic> {
    check_sources(sources)
        .into_iter()
        .flat_map(|(_, ds)| ds)
        .collect()
}
fn build(sources: &[(String, String)]) -> Result<pw_core::build::Build, String> {
    let units: Vec<_> = sources
        .iter()
        .map(|(path, src)| Unit {
            path: path.clone(),
            src: src.clone(),
            hir: pw_core::lower::lower_file(src, &pw_syntax::parse_tree(src).green),
        })
        .collect();
    pw_core::build::build(&units)
}

#[test]
fn qualified_and_selectively_imported_device_arguments_are_refused_at_their_declaration() {
    for (import, value) in [
        ("import prefs", "prefs.word"),
        ("import prefs.{ word }", "word"),
    ] {
        let sources = sources(import, "", &send(value), "String");
        let found = diagnostics(&sources);
        assert_eq!(
            found.iter().map(|d| d.code).collect::<Vec<_>>(),
            ["PW6801"],
            "{found:?}"
        );
        assert!(
            found[0]
                .related
                .iter()
                .any(|r| r.file.as_deref() == Some("prefs.pw")
                    && PREFS.get(r.span.clone()) == Some("word")
                    && r.label.contains("privacy Device")),
            "{found:?}"
        );
        assert!(
            build(&sources).is_err(),
            "a refused device argument must produce no build"
        );
    }
    let public = sources("import prefs", "", &send("\"public\""), "String");
    assert!(diagnostics(&public).is_empty());
    assert!(
        build(&public)
            .expect("public neighbor builds")
            .refusals()
            .is_empty()
    );
}

#[test]
fn qualified_device_arguments_keep_their_declared_value_type() {
    let sources = sources("import prefs", "", &send("prefs.word"), "Int");
    let found = diagnostics(&sources);
    assert!(
        found.iter().any(|d| d.code == "PW0605"
            && d.message.contains("String")
            && d.message.contains("Int")),
        "{found:?}"
    );
    assert!(found.iter().any(|d| d.code == "PW6801"), "{found:?}");
}

#[test]
fn lexical_record_and_signal_names_do_not_read_a_module_device_value() {
    for (import, signals, value) in [
        (
            "import prefs",
            "signal prefs: PublicPrefs = PublicPrefs { word: \"public\" }",
            "prefs.word",
        ),
        (
            "import prefs.{ word }",
            "signal word: String = \"public\"",
            "word",
        ),
    ] {
        let sources = sources(import, signals, &send(value), "String");
        assert!(
            diagnostics(&sources).is_empty(),
            "{:?}",
            diagnostics(&sources)
        );
        let built = build(&sources).expect("lexical neighbor builds");
        assert!(
            built.refusals().is_empty(),
            "{import}: {:?}",
            built.refusals()
        );
    }
}

#[test]
fn qualified_browser_only_read_still_refuses_unsupported_backend_lowering() {
    let sources = sources("import prefs", "", "draft = prefs.word", "String");
    assert!(
        diagnostics(&sources).is_empty(),
        "{:?}",
        diagnostics(&sources)
    );
    let built = build(&sources).expect("browser-only read checks");
    assert!(
        built
            .refusals()
            .iter()
            .any(|r| r.contains("`prefs` is not bound here")),
        "{:?}",
        built.refusals()
    );
}

#[test]
fn a_captured_public_record_prefix_does_not_steal_the_device_origin() {
    for prefix in ["prefs", "publicPrefs"] {
        let nested = format!(
            "signal {prefix}: PublicPrefs = PublicPrefs {{ word: \"public\" }}\n    fn submit() -> Result<Unit, String> {{ send(\"{{{prefix}.word}} {{secret}}\") }}"
        );
        let mut files = sources(
            "import prefs\nimport prefs.{ secret }",
            &nested,
            "match submit() { Ok(_) => draft = \"\", Err(_) => draft = \"\", }",
            "String",
        );
        let prefs = &mut files
            .iter_mut()
            .find(|(path, _)| path == "prefs.pw")
            .expect("prefs source")
            .1;
        prefs.push_str("\nsignal secret: String = \"\"\n    storage device \"prefs-secret\"\n    privacy Device\n    conflict last_write_wins\n");
        let prefs = prefs.clone();
        let app = &files
            .iter()
            .find(|(path, _)| path == "app.pw")
            .expect("app source")
            .1;
        let found = diagnostics(&files);
        // The nested function is independently disallowed from calling a command
        // and reading a signal; its Device diagnostic must still name the source
        // actually carrying Device, never the captured public record's field.
        let mut codes: Vec<_> = found.iter().map(|d| d.code).collect();
        codes.sort();
        assert_eq!(codes, ["PW0339", "PW5301", "PW6801"], "{prefix}: {found:?}");
        let device: Vec<_> = found.iter().filter(|d| d.code == "PW6801").collect();
        assert_eq!(device.len(), 1, "{prefix}: {found:?}");
        let device = device[0];
        assert!(
            device
                .message
                .starts_with("`secret` is kept on this device"),
            "{prefix}: {device:?}"
        );
        assert_eq!(
            app.get(device.primary_span.clone()),
            Some(format!("\"{{{prefix}.word}} {{secret}}\"").as_str()),
            "{prefix}: {device:?}"
        );
        assert!(
            device
                .related
                .iter()
                .any(|related| related.file.as_deref() == Some("prefs.pw")
                    && prefs.get(related.span.clone()) == Some("secret")
                    && related.label.contains("privacy Device")),
            "{prefix}: {device:?}"
        );
        assert!(
            build(&files).is_err(),
            "the independently refused nested function must not build"
        );
    }
}
