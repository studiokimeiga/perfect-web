//! **A page no reader decides is one document** (the prerender's ADR): which
//! pages a build marks. A page is marked where nothing its reader brings can
//! change its document and nothing on it ever asks a server anything.
//!
//! Each case changes one thing of the page that is marked, so that a rule
//! dropped from the mark is a case that fails.

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

/// `src`, built, as `pw build` builds it: a case that does not build is a
/// mistake in the case.
fn built(src: &str) -> pw_core::build::Build {
    let mut sources = library();
    sources.push(("t.pw".to_string(), src.to_string()));
    let units: Vec<Unit> = sources
        .into_iter()
        .map(|(path, src)| Unit {
            path,
            hir: lower_file(&src, &parse_tree(&src).green),
            src,
        })
        .collect();
    let build = pw_core::build::build(&units).unwrap_or_else(|why| panic!("{why}\n{src}"));
    assert_eq!(build.refusals(), Vec::<String>::new(), "{src}");
    build
}

/// Whether `src`'s page `P` is marked.
fn marked(src: &str) -> bool {
    let build = built(src);
    let plan = build
        .pages
        .iter()
        .find(|p| p.page == "t.P~ui")
        .unwrap_or_else(|| panic!("no page P in\n{src}"))
        .plan
        .as_ref()
        .unwrap_or_else(|why| panic!("{why}\n{src}"));
    plan.prerender
}

/// What the cases share: a query, and a command a page's handler may call
/// with the type its idempotency names.
const DECLARED: &str = "opaque type InteractionId = String\n\n\
    public query Greeting() -> String !{} {\n    \"a\"\n}\n\n\
    command send(text: String) -> Result<Int, String>\n    requires Anyone\n    \
    idempotent_by InteractionId\n{\n    Ok(1)\n}\n\n";

/// The press that changes only a signal.
const LOCAL: &str = "<button type=\"button\" on:press={() => count = count + 1}>{count}</button>";

/// The press that sends a command.
const SENDS: &str = "<button type=\"button\" on:press={() => match send(\"a\") \
    { Ok(_) => (), Err(_) => () }}>Send</button>";

/// Module `t`: `DECLARED`, what `before` declares, and page `P` whose head
/// is `head`, whose clauses are `clauses`, whose body holds its signals and
/// then `body`, and whose view holds `markup`.
fn program(before: &str, head: &str, clauses: &str, body: &str, markup: &str) -> String {
    format!(
        "module t\n\n{DECLARED}{before}{head} {{\n{clauses}\n    signal count: Int = 0\n    \
         signal rows: List<Int> = [1, 2]\n\n{body}    \
         view {{\n        <title>P</title>\n        {markup}\n    }}\n}}\n"
    )
}

const ROUTE: &str = "    route \"/p\"\n";

/// The page that is marked: an address, its signals, and a press that
/// changes one.
fn plain() -> String {
    program("", "page P()", ROUTE, "", LOCAL)
}

/// A list the browser renders again, under a block a signal decides
/// (ADR-0137), each row holding `row`.
fn rows_of(row: &str) -> String {
    format!("{{#if count >= 0}}{{#each rows as n (n)}}{row}{{/each}}{{/if}}")
}

#[test]
fn a_page_of_signals_at_an_address_is_marked() {
    assert!(marked(&plain()));
}

/// A press that changes only a signal is no reason to keep a page unmarked,
/// wherever it is: a mark that gave up on a handler at all, or on a block,
/// would refuse every page worth marking.
#[test]
fn a_handler_that_stays_in_the_browser_leaves_a_page_marked() {
    let view = "view Here() !{} {\n    <p>here</p>\n}\n\n";
    for markup in [
        format!("{{#if count > 0}}{LOCAL}{{/if}}"),
        rows_of(LOCAL),
        "<Here />".to_string(),
    ] {
        let src = program(view, "page P()", ROUTE, "", &markup);
        assert!(marked(&src), "{src}");
    }
    // A page with no handler at all.
    let src = program("", "page P()", ROUTE, "", "<p>{count}</p>");
    assert!(marked(&src), "{src}");
}

/// A value the device keeps is held in the browser: the document holds its
/// declared default, whoever asks.
#[test]
fn a_value_the_device_keeps_leaves_a_page_marked() {
    let kept = "signal kept: Int = 0\n    storage  device \"kept\"\n    \
                privacy  Device\n    conflict last_write_wins\n\n";
    let src = program(kept, "page P()", ROUTE, "", "<p>{kept}</p>");
    assert!(marked(&src), "{src}");
}

#[test]
fn a_page_with_no_address_is_not_marked() {
    let src = program("", "page P()", "", "", LOCAL);
    assert!(!marked(&src), "{src}");
}

/// What a request brings decides the document: a path's value and a query's.
#[test]
fn a_page_its_address_gives_a_value_is_not_marked() {
    for (head, clauses) in [
        ("page P(id: String)", "    route \"/p/{id}\"\n"),
        ("page P(q: Option<String>)", "    route \"/p{?q}\"\n"),
    ] {
        let src = program("", head, clauses, "", LOCAL);
        assert!(!marked(&src), "{src}");
    }
}

#[test]
fn a_page_that_binds_a_query_is_not_marked() {
    let src = program(
        "",
        "page P()",
        ROUTE,
        "    let hello = query Greeting()\n\n",
        "<p>{hello}</p>",
    );
    assert!(!marked(&src), "{src}");
}

#[test]
fn a_page_with_a_stream_is_not_marked() {
    let streamed = "public query Slow() -> Result<String, String> !{}\n    \
        delivery streamed\n    timeout  3.seconds\n{\n    Ok(\"a\")\n}\n\n";
    let src = program(
        streamed,
        "page P()",
        ROUTE,
        "",
        "<stream query={Slow()}>\n<placeholder><p>wait</p></placeholder>\n\
         <ready as={said}><p>{said}</p></ready>\n<failed><p>no</p></failed>\n</stream>",
    );
    assert!(!marked(&src), "{src}");
}

/// Whose a page is: each scope but `public`. A page that asks a predicate
/// of its reader is held to one of these by PW5049, so its scope says it.
#[test]
fn a_page_that_is_someones_is_not_marked() {
    let private = "    route \"/p\"\n    cache private\n";
    for head in ["session page P()", "user page P()", "page P()"] {
        let src = program("", head, private, "", LOCAL);
        assert!(!marked(&src), "{src}");
    }
}

/// A command is the server's, wherever the press that sends it is: on the
/// page, in a block's arm, in a row, in a view the page shows.
#[test]
fn a_handler_that_sends_a_command_keeps_a_page_unmarked() {
    let view = format!("view Sender() !{{}} {{\n    {SENDS}\n}}\n\n");
    for markup in [
        SENDS.to_string(),
        format!("{{#if count > 0}}{SENDS}{{/if}}"),
        format!("{{#if count > 0}}<p>a</p>{{:else}}{SENDS}{{/if}}"),
        rows_of(SENDS),
        "<Sender />".to_string(),
    ] {
        let src = program(&view, "page P()", ROUTE, "", &markup);
        assert!(!marked(&src), "{src}");
    }
}

/// A view that contains itself is an instance made at run time (ADR-0203):
/// its handlers are in a template of its own, which the page's markup only
/// names.
#[test]
fn a_view_that_contains_itself_is_read_for_its_handlers() {
    let tree = |press: &str| {
        format!(
            "type Node = Node {{ label: String, kids: List<Node> }}\n\n\
             view Tree(node: Node) !{{}} {{\n    <li>{{node.label}}\n        {press}\n        \
             <ul>{{#each node.kids as kid (kid.label)}}<Tree node={{kid}} />{{/each}}</ul>\n    \
             </li>\n}}\n\n"
        )
    };
    let body = "    signal tree: Node = Node { label: \"a\", kids: [] }\n\n";
    let markup = "<ul><Tree node={tree} /></ul>";
    let quiet = program(&tree("<p>a</p>"), "page P()", ROUTE, body, markup);
    assert!(marked(&quiet), "{quiet}");
    let sends = program(&tree(SENDS), "page P()", ROUTE, body, markup);
    assert!(!marked(&sends), "{sends}");
}

/// A layout's markup is in the page's document, its handlers with it.
#[test]
fn a_layout_is_read_for_its_handlers() {
    let layout = |press: &str| {
        format!(
            "layout Shell {{\n    view {{\n        {press}\n        \
             <main><slot /></main>\n    }}\n}}\n\n"
        )
    };
    let clauses = "    route  \"/p\"\n    layout Shell\n";
    let quiet = program(&layout("<p>a</p>"), "page P()", clauses, "", LOCAL);
    assert!(marked(&quiet), "{quiet}");
    let sends = program(&layout(SENDS), "page P()", clauses, "", LOCAL);
    assert!(!marked(&sends), "{sends}");
}

/// A layout's query fills the page's document as the page's own would.
#[test]
fn a_layout_that_binds_a_query_keeps_a_page_unmarked() {
    let layout = "layout Shell {\n    let hello = query Greeting()\n\n    view {\n        \
                  <p>{hello}</p>\n        <main><slot /></main>\n    }\n}\n\n";
    let clauses = "    route  \"/p\"\n    layout Shell\n";
    let src = program(layout, "page P()", clauses, "", LOCAL);
    assert!(!marked(&src), "{src}");
}

/// **What a host reads**: the plan's JSON says so of a page that is marked,
/// and nothing of one that is not.
#[test]
fn a_page_marked_is_said_so_in_its_plan() {
    let written = |src: &str| {
        let build = built(src);
        let plan = build.pages[0].plan.as_ref().expect("a plan");
        serde_json::to_value(plan).expect("its plan as JSON")
    };
    assert_eq!(written(&plain())["prerender"], serde_json::json!(true));
    let sends = program("", "page P()", ROUTE, "", SENDS);
    assert_eq!(written(&sends).get("prerender"), None);
}
