//! **A declaration named as a handler is `(e) => save(e)`** (ADR-0199,
//! ADR-0195's ruling 12).
//!
//! `on:press|refusable={clear}` names a command, or a function, where a lambda would
//! be written. It is compiled as the lambda that calls it, given the event
//! where it takes one, and held to every rule that lambda is: the event's
//! type (PW0602), a sent command's idempotency (PW0338), an answer the runtime
//! would drop (PW0618), and what it performs in the browser. Until ADR-0199
//! it was checked against its event and refused when built, as a handler
//! with no code.

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
        "examples/store",
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

fn units(src: &str) -> Vec<Unit> {
    let mut sources = library();
    sources.push(("t.pw".to_string(), src.to_string()));
    sources
        .into_iter()
        .map(|(path, src)| Unit {
            path,
            hir: lower_file(&src, &parse_tree(&src).green),
            src,
        })
        .collect()
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

/// A program with a command a handler can send, `extra` declarations, and a
/// button whose handler is `handler`.
fn program(extra: &str, handler: &str) -> String {
    format!(
        "module t\n\n\
         opaque type InteractionId = String\n\n\
         command clear() -> Int\n    requires      SignedIn\n    \
         idempotent_by InteractionId\n{{\n    1\n}}\n\n\
         {extra}\n\n\
         page P() {{\n    cache private\n\n    \
         view {{\n        <main><button type=\"button\" on:press|refusable={{{handler}}}>Go</button></main>\n    }}\n}}\n"
    )
}

#[test]
fn a_command_named_as_a_handler_is_compiled_as_the_lambda_that_calls_it() {
    let src = program("", "clear");
    assert_eq!(reported(&src), Vec::<String>::new());
    let handlers = pw_core::backend::js::compile(&units(&src)).expect("checks");
    let compiled: Vec<(String, Vec<String>)> = handlers
        .iter()
        .filter_map(|h| match &h.module {
            pw_core::backend::wasm::Encoding::Encoded(m) => {
                Some((m.name.clone(), m.commands.clone()))
            }
            _ => None,
        })
        .collect();
    assert!(
        compiled.iter().any(
            |(name, commands)| name == "clear" && commands.iter().any(|c| c.ends_with("clear"))
        ),
        "{compiled:?}"
    );
    // Its lambda form compiles to the same module, but for its identity.
    let lambda = program("", "() => clear()");
    assert_eq!(reported(&lambda), Vec::<String>::new());
    assert_eq!(module_of(&src), module_of(&lambda));
}

#[test]
fn a_named_command_is_held_to_what_its_lambda_form_is() {
    // A command whose answer the runtime would drop (PW0618).
    let answering = program(
        "type Cart = Cart { n: Int }\n\ntype CartError =\n    | Gone\n\ncommand add() -> Result<Cart, CartError>\n    requires      SignedIn\n    idempotent_by InteractionId\n{\n    Ok(Cart { n: 1 })\n}",
        "add",
    );
    assert!(
        reported(&answering)
            .iter()
            .any(|d| d.starts_with("PW0618") && d.contains("drops")),
        "{:?}",
        reported(&answering)
    );
    // A command sent without idempotency (PW0338).
    let unkeyed = program(
        "command ping() -> ()\n    requires      SignedIn\n{\n    ()\n}",
        "ping",
    );
    assert!(
        reported(&unkeyed)
            .iter()
            .any(|d| d.starts_with("PW0338") && d.contains("`ping`")),
        "{:?}",
        reported(&unkeyed)
    );
    // A function given more than its event (PW0602).
    let two = program("fn both(a: Int, b: Int) -> () { () }", "both");
    assert!(
        reported(&two)
            .iter()
            .any(|d| d.starts_with("PW0602") && d.contains("takes 2 parameters")),
        "{:?}",
        reported(&two)
    );
}

#[test]
fn a_named_function_that_changes_nothing_is_refused_as_its_lambda_is() {
    // A function calls no command and changes no signal: pressing it would
    // change nothing, named or written as a lambda.
    for handler in ["noop", "() => noop()"] {
        let src = program("fn noop() -> Int { 1 }", handler);
        let handlers = pw_core::backend::js::compile(&units(&src)).expect("checks");
        let refused: Vec<String> = handlers
            .iter()
            .filter_map(|h| match &h.module {
                pw_core::backend::wasm::Encoding::Encoded(_) => None,
                other => Some(format!("{other}")),
            })
            .collect();
        assert!(
            refused
                .iter()
                .any(|r| r.contains("a handler that calls no command and changes no signal")),
            "{handler}: {refused:?}"
        );
    }
}

/// Each of `t.P`'s event parts' handler identities.
fn identities(b: &pw_core::build::Build) -> Vec<String> {
    let t = b
        .templates
        .iter()
        .find(|t| t.path == "t.P~ui")
        .expect("the page's template");
    let ir = serde_json::to_value(t).expect("serializes");
    let mut out = Vec::new();
    fn walk(v: &serde_json::Value, out: &mut Vec<String>) {
        if v["part"] == "event" {
            out.push(v["handler"].as_str().unwrap_or_default().to_string());
        }
        match v {
            serde_json::Value::Object(o) => o.values().for_each(|x| walk(x, out)),
            serde_json::Value::Array(a) => a.iter().for_each(|x| walk(x, out)),
            _ => {}
        }
    }
    walk(&ir, &mut out);
    out
}

#[test]
fn a_named_handler_is_built_with_an_identity_and_a_module() {
    let b = pw_core::build::build(&units(&program("", "clear"))).expect("builds");
    assert!(b.refusals().is_empty(), "{:?}", b.refusals());
    let ids = identities(&b);
    assert_eq!(ids.len(), 1, "{ids:?}");
    assert!(!ids[0].is_empty(), "the handler has an identity");
    assert!(
        b.handlers.iter().any(|h| matches!(
            &h.module,
            pw_core::backend::wasm::Encoding::Encoded(m) if m.identity == ids[0]
        )),
        "and a module"
    );
}

#[test]
fn a_named_handlers_identity_is_what_its_name_resolves_to() {
    // The same text, `on:press|refusable={clear}`, naming this module's `clear` and
    // another's: two behaviours, two identities, as `() => clear()`'s are.
    let other = "module other\n\nopaque type InteractionId = String\n\n\
                 command clear() -> Int\n    requires      SignedIn\n    \
                 idempotent_by InteractionId\n{\n    1\n}\n";
    let imported = "module t\n\nimport other.{ clear }\n\n\
                    page P() {\n    cache private\n\n    \
                    view {\n        <main><button type=\"button\" on:press|refusable={clear}>Go</button></main>\n    }\n}\n";
    let own = program("", "clear");
    let build = |src: &str| {
        let mut all = units(src);
        all.push(Unit {
            path: "other.pw".to_string(),
            hir: lower_file(other, &parse_tree(other).green),
            src: other.to_string(),
        });
        let b = pw_core::build::build(&all).expect("builds");
        assert!(b.refusals().is_empty(), "{:?}", b.refusals());
        identities(&b)
    };
    let (a, b) = (build(&own), build(imported));
    assert_eq!((a.len(), b.len()), (1, 1), "{a:?} {b:?}");
    assert_ne!(a, b);
}

/// The one module `t.P`'s handler compiles to, its identity written `ID`.
fn module_of(src: &str) -> String {
    let compiled = pw_core::backend::js::compile(&units(src)).expect("checks");
    let modules: Vec<String> = compiled
        .iter()
        .filter(|c| c.declaration == "t.P~ui")
        .filter_map(|c| match &c.module {
            pw_core::backend::wasm::Encoding::Encoded(m) => {
                Some(m.source.replace(&m.identity, "ID"))
            }
            _ => None,
        })
        .collect();
    assert_eq!(modules.len(), 1, "{modules:?}");
    modules[0].clone()
}

#[test]
fn a_named_handler_is_given_its_event() {
    // `on:input|refusable={rename}` is `(e) => rename(e)`: the command is sent the
    // event, and the module is the lambda form's, but for its identity.
    let src = |handler: &str| {
        format!(
            "module t\n\nimport events.{{ InputEvent }}\n\n\
             opaque type InteractionId = String\n\n\
             command rename(e: InputEvent) -> Int\n    requires      SignedIn\n    \
             idempotent_by InteractionId\n{{\n    1\n}}\n\n\
             page P() {{\n    cache private\n\n    \
             view {{\n        <main><input aria-label=\"Name\" on:input|refusable={{{handler}}} /></main>\n    }}\n}}\n"
        )
    };
    assert_eq!(reported(&src("rename")), Vec::<String>::new());
    let named = module_of(&src("rename"));
    assert_eq!(named, module_of(&src("(e) => rename(e)")));
    // The event as the listener read it, sent as it is: its form is the
    // wire's, and it holds no `Int` (ADR-0433).
    assert!(
        named.contains("context.command(\"t.rename\", [v0])"),
        "{named}"
    );
    let b = pw_core::build::build(&units(&src("rename"))).expect("builds");
    assert!(b.refusals().is_empty(), "{:?}", b.refusals());
}

#[test]
fn a_name_that_is_no_function_or_command_is_refused_when_checked() {
    // A handler is compiled from what it names: a local's value, a page, a
    // type, a case or a query has nothing to compile, and each is refused
    // when the program is checked, once (ADR-0199).
    let local = program("", "save").replace("    view {", "    let save = () => 1\n\n    view {");
    for (src, what) in [
        (local, "`save`, a value this declaration binds"),
        (program("", "P"), "`P`, a page"),
        (program("", "InteractionId"), "`InteractionId`, a type"),
        (program("", "Some"), "`Some`, a case"),
        (
            program("query Menu() -> List<Int> {\n    [1]\n}", "Menu"),
            "`Menu`, a query",
        ),
    ] {
        let found = reported(&src);
        assert_eq!(found.len(), 1, "{what}: {found:?}");
        assert!(
            found[0].starts_with("PW0614") && found[0].contains(what),
            "{what}: {found:?}"
        );
    }
    // A local's value that is no function is the value's type's to report.
    let int = program("", "n").replace("    view {", "    let n = 1\n\n    view {");
    assert_eq!(
        reported(&int),
        ["PW0614 `on:press|refusable` is given `Int`, which is not a function to call"]
    );
}

#[test]
fn a_local_that_shadows_a_declaration_is_refused_as_a_local() {
    // `ping` is the page's own value here, not the command: refused as a
    // local, and the command it shadows is neither sent (PW0338) nor run in
    // the browser.
    let src = program(
        "command ping() -> Int\n    requires      SignedIn\n{\n    1\n}",
        "ping",
    )
    .replace("    view {", "    let ping = () => 1\n\n    view {");
    let found = reported(&src);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0].starts_with("PW0614") && found[0].contains("`ping`, a value"),
        "{found:?}"
    );
}

#[test]
fn a_named_handler_is_held_to_its_event_by_any_path() {
    // `other.rename`, through its module, is `(e) => other.rename(e)`, held
    // to the event as `rename` is.
    let other = "module other\n\nopaque type InteractionId = String\n\n\
                 command rename(n: Int) -> Int\n    requires      SignedIn\n    \
                 idempotent_by InteractionId\n{\n    n\n}\n";
    for (import, handler) in [
        ("import other.{ rename }", "rename"),
        ("import other", "other.rename"),
    ] {
        let src = format!(
            "module t\n\n{import}\n\npage P() {{\n    cache private\n\n    \
             view {{\n        <main><button type=\"button\" on:press|refusable={{{handler}}}>Go</button></main>\n    }}\n}}\n"
        );
        let mut sources = library();
        sources.push(("other.pw".to_string(), other.to_string()));
        sources.push(("t.pw".to_string(), src));
        let found: Vec<String> = check_sources(&sources)
            .into_iter()
            .filter(|(n, _)| n == "t.pw")
            .flat_map(|(_, ds)| ds.into_iter().map(|d| format!("{} {}", d.code, d.message)))
            .collect();
        assert_eq!(
            found,
            ["PW0602 `on:press` expects a handler taking `PressEvent`, found one taking `Int`"],
            "{handler}"
        );
    }
}

#[test]
fn a_named_handler_is_named_as_its_lambda_forms_call_is() {
    // The module and the event part carry the name of what the handler
    // calls, for the named form as for its lambda form: `clear`, and, for a
    // call through a module, none.
    let other = "module other\n\nopaque type InteractionId = String\n\n\
                 command clear() -> Int\n    requires      SignedIn\n    \
                 idempotent_by InteractionId\n{\n    1\n}\n";
    let names = |handler: &str| {
        let src = format!(
            "module t\n\nimport other\n\npage P() {{\n    cache private\n\n    \
             view {{\n        <main><button type=\"button\" on:press|refusable={{{handler}}}>Go</button></main>\n    }}\n}}\n"
        );
        let mut all = units(&src);
        all.push(Unit {
            path: "other.pw".to_string(),
            hir: lower_file(other, &parse_tree(other).green),
            src: other.to_string(),
        });
        pw_core::backend::js::compile(&all)
            .expect("checks")
            .iter()
            .filter(|h| h.declaration == "t.P~ui")
            .filter_map(|h| match &h.module {
                pw_core::backend::wasm::Encoding::Encoded(m) => Some(m.name.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(names("other.clear"), [""]);
    assert_eq!(names("other.clear"), names("() => other.clear()"));
    let own = |handler: &str| {
        pw_core::backend::js::compile(&units(&program("", handler)))
            .expect("checks")
            .iter()
            .filter(|h| h.declaration == "t.P~ui")
            .filter_map(|h| match &h.module {
                pw_core::backend::wasm::Encoding::Encoded(m) => Some(m.name.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(own("clear"), ["clear"]);
    assert_eq!(own("clear"), own("() => clear()"));
}
